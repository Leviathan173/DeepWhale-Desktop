//! 登录令牌抓取：拉起带调试端口的浏览器，等用户登录后通过 CDP 嗅探
//! platform 用量接口的 Authorization 头，把小鲸鱼拿不到但页面会用的
//! Debug 令牌存下来。
use serde_json::{json, Value};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{Duration, Instant};

/// 每次拉起浏览器用独立调试端口 + 独立临时 profile，避免与残留实例
/// 争用 profile SingletonLock 或端口（重试 / 连续两次抓取都会撞车）。
static NEXT_PORT: AtomicU16 = AtomicU16::new(9333);

fn json_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/json")
}

/// 拉起 Edge/Chrome 到指定页面（独立临时 profile，不碰用户日常浏览器）。
/// 返回 (子进程, 调试端口, 临时目录路径)——调用方退出时需清理该目录。
fn spawn_browser(url: &str) -> Result<(Child, u16, std::path::PathBuf), String> {
    let port = NEXT_PORT.fetch_add(1, Ordering::Relaxed);
    let mut path = std::env::temp_dir();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    path.push(format!("dshwl-{}-{}-{}", std::process::id(), stamp, port));
    std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    let profile = path.join("profile");
    std::fs::create_dir_all(&profile).map_err(|e| e.to_string())?;

    let candidates = [
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    ];
    let exe = candidates
        .iter()
        .find(|p| std::path::Path::new(p).exists())
        .copied()
        .ok_or("未找到 Edge/Chrome，请按对应平台提示手动复制登录信息")?;

    let child = Command::new(exe)
        .arg(format!("--remote-debugging-port={port}"))
        // 只放行本机调试页 origin，避免任意网站通过 `*` 嗅探登录态
        .arg(format!(
            "--remote-allow-origins=http://127.0.0.1:{port},http://localhost:{port}"
        ))
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-features=Translate,msEdgeShoppingAssist,AutofillServerCommunication")
        .arg(url)
        .spawn()
        .map_err(|e| format!("浏览器启动失败: {e}"))?;
    Ok((child, port, path))
}

/// 子进程已退出则清理临时目录并返回错误（避免残留登录态 + 避免空等 4 分钟）。
fn child_gone(child: &mut Child, dir: &std::path::Path) -> Option<String> {
    match child.try_wait() {
        Ok(Some(_)) => {
            let _ = std::fs::remove_dir_all(dir);
            Some("浏览器进程已提前退出，请重试".to_string())
        }
        _ => None,
    }
}

pub async fn capture_platform_token(attach: Option<u16>) -> Result<String, String> {
    let client = reqwest::Client::new();
    // 附加模式：先试常驻调试浏览器，复用登录态 reload 逼页面重发用量接口。
    if let Some(port) = attach {
        if let Some(t) = attach_sniff_auth(
            &client,
            port,
            "platform.deepseek.com",
            "https://platform.deepseek.com/usage",
            &["by_api_key/amount", "/api/v0/usage/"],
            "deepseek",
        )
        .await
        {
            return Ok(t);
        }
    }
    let (mut child, port, dir) = spawn_browser("https://platform.deepseek.com/usage")?;
    let started = Instant::now();
    let deadline = started + Duration::from_secs(240);

    // 1. 等浏览器调试端口就绪 + 出现 platform 页 target
    let page_ws = loop {
        if let Some(err) = child_gone(&mut child, &dir) {
            return Err(err);
        }
        if let Some(ws) = try_find_page(&client, port, "platform.deepseek.com").await {
            break ws;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = std::fs::remove_dir_all(&dir);
            return Err("等待浏览器登录超时（4 分钟）".to_string());
        }
        tokio::time::sleep(Duration::from_millis(600)).await;
    };

    // 2. 连上 CDP，只收 Network.requestWillBeSent，等 Authorization 头（用户登录后
    //    页面会自动请求 by_api_key/amount，见 balance::USAGE_URL_BASE）。
    let token = match sniff_auth_header(
        &page_ws,
        deadline,
        &["by_api_key/amount", "/api/v0/usage/"],
        false,
        "请在打开的浏览器里登录 platform.deepseek.com（4 分钟）",
    )
    .await
    {
        Ok(t) => t,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e);
        }
    };
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);
    Ok(token)
}

async fn try_find_page(client: &reqwest::Client, port: u16, url_contains: &str) -> Option<String> {
    let list = client
        .get(json_url(port))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .ok()?
        .json::<Value>()
        .await
        .ok()?;
    let pages = list.as_array()?;
    for p in pages {
        let ws = p.get("webSocketDebuggerUrl")?.as_str()?;
        let url = p.get("url").and_then(|v| v.as_str()).unwrap_or("");
        if url.contains(url_contains) && p.get("type").and_then(|v| v.as_str()) == Some("page") {
            return Some(ws.to_string());
        }
    }
    None
}

pub struct BailianCreds {
    pub cookie: String,
    /// 订阅接口的完整 form body（原样重放给后续请求）。
    pub post_data: String,
    /// 抓到的订阅接口响应体（原始 JSON 字符串），供解析器定稿。
    pub sample: String,
}

const BAILIAN_PLAN_URL: &str =
    "https://bailian.console.aliyun.com/cn-beijing?tab=plan#/efm/subscription/token-plan/personal";

/// 百炼 TokenPlan：附加常驻调试浏览器（可选）或拉起独立浏览器到订阅页，等用户登录后嗅探
/// 订阅/用量接口的请求 Cookie + 完整 form body，并抓一次响应体存为样本
/// （登录后该页会自动请求这两个接口）。
pub async fn capture_bailian_credentials(attach: Option<u16>) -> Result<BailianCreds, String> {
    let client = reqwest::Client::new();
    // 附加模式：登录态在常驻浏览器里，reload 订阅页重发请求即可抓。
    if let Some(port) = attach {
        let url_contains = "bailian.console.aliyun.com";
        if let Some(ws) = find_or_open_page(&client, port, url_contains, BAILIAN_PLAN_URL).await {
            match sniff_bailian(&ws, Instant::now() + Duration::from_secs(45), true).await {
                Ok(c) => return Ok(c),
                Err(e) => {
                    eprintln!("[bailian] 附加 {port} 抓取失败: {e}，回退新浏览器");
                }
            }
        } else {
            eprintln!("[bailian] {port} 不可达或打不开百炼页（Edge 需带独立 --user-data-dir + --remote-debugging-port={port} 启动），回退新浏览器");
        }
    }
    let (mut child, port, dir) = spawn_browser(BAILIAN_PLAN_URL)?;
    let started = Instant::now();
    let deadline = started + Duration::from_secs(240);

    let page_ws = loop {
        if let Some(err) = child_gone(&mut child, &dir) {
            return Err(err);
        }
        if let Some(ws) = try_find_page(&client, port, "bailian.console.aliyun.com").await {
            break ws;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = std::fs::remove_dir_all(&dir);
            return Err("等待百炼登录超时（4 分钟）".to_string());
        }
        tokio::time::sleep(Duration::from_millis(600)).await;
    };

    let creds = match sniff_bailian(&page_ws, deadline, false).await {
        Ok(c) => c,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e);
        }
    };
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);
    Ok(creds)
}

// 只用「用量」接口：它同时返回重置时间(per1WeekResetTime)与使用比例(per1WeekPercentage)，
// 且 body 里的 params.Api 必须匹配查询串的 api，否则服务端会拒。所以捕获/重放都锁 consumption 接口。
const BAILIAN_API: &str = "tokenplan/personal/api/v2/usage";

async fn sniff_bailian(
    ws_url: &str,
    deadline: Instant,
    reload: bool,
) -> Result<BailianCreds, String> {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::Message;

    let (mut ws, _) = connect_async(ws_url).await.map_err(|e| e.to_string())?;
    // CDP：id 从 1 开始顺序发命令
    ws.send(Message::Text(
        json!({ "id": 1, "method": "Network.enable", "params": { "maxPostDataSize": 65536 } })
            .to_string()
            .into(),
    ))
    .await
    .map_err(|e| e.to_string())?;
    // 附加到常驻浏览器时页面早已加载完，主动 reload 逼它重发订阅/用量接口。
    // 用 id 2/3，cmd_id 起点相应后移。
    let mut cmd_id: u64 = 2;
    if reload {
        ws.send(Message::Text(
            json!({ "id": 2, "method": "Page.enable", "params": {} })
                .to_string()
                .into(),
        ))
        .await
        .map_err(|e| e.to_string())?;
        ws.send(Message::Text(
            json!({ "id": 3, "method": "Page.reload", "params": { "ignoreCache": false } })
                .to_string()
                .into(),
        ))
        .await
        .map_err(|e| e.to_string())?;
        cmd_id = 4;
    }

    // 注意：Chrome 的 Cookie 头走 Network.requestWillBeSentExtraInfo，
    // 不在 requestWillBeSent 的请求头里。这里两种事件都收，按 requestId 关联。
    let mut cookie: Option<String> = None;
    let mut request_id: Option<String> = None;
    let mut post_data: Option<String> = None;
    let mut sample: Option<String> = None;
    // CDP 命令 id 必须单调递增；每个命令回执按 id 关联到对应请求。
    let mut post_cmd: Option<u64> = None;
    let mut body_cmd: Option<u64> = None;

    while Instant::now() < deadline {
        let msg = tokio::select! {
            m = ws.next() => match m {
                Some(Ok(m)) => m,
                Some(Err(e)) => return Err(format!("CDP 连接断开: {e}")),
                None => return Err("CDP 连接关闭".to_string()),
            },
            _ = tokio::time::sleep(Duration::from_secs(1)) => continue,
        };
        if let Message::Text(t) = msg {
            if let Ok(v) = serde_json::from_str::<Value>(&t) {
                let method = v
                    .get("method")
                    .and_then(|m| m.as_str())
                    .unwrap_or("")
                    .to_string();
                match method.as_str() {
                    "Network.requestWillBeSentExtraInfo" => {
                        // 该事件的 headers 里有登录 Cookie；所有 balian 请求共用同一会话 Cookie。
                        // 始终取最新（登录后才有 login_aliyunid，登录前只有匿名 cna）。
                        if let Some(ck) = v.pointer("/params/headers").and_then(extract_cookie) {
                            if ck.contains("login_aliyunid") {
                                cookie = Some(ck);
                            }
                        }
                    }
                    "Network.requestWillBeSent" => {
                        let url = v
                            .pointer("/params/request/url")
                            .and_then(|u| u.as_str())
                            .unwrap_or("");
                        // 只认消费/用量接口：action=BroadScopeAspnGateway + api=...usage。
                        // 订阅接口 body 的 params.Api 与 usage 不一致，不能互相复用。
                        if url.contains("data/api.json") && url.contains(BAILIAN_API) {
                            if let Some(rid) = v
                                .pointer("/params/requestId")
                                .and_then(|r| r.as_str())
                                .map(str::to_string)
                            {
                                request_id = Some(rid.clone());
                                post_cmd = Some(cmd_id);
                                let cid = cmd_id;
                                cmd_id += 1;
                                // 主动拿完整 form body（含 params/sec_token）
                                ws.send(Message::Text(
                                    json!({ "id": cid, "method": "Network.getRequestPostData", "params": { "requestId": rid } })
                                        .to_string()
                                        .into(),
                                ))
                                .await
                                .map_err(|e| e.to_string())?;
                            }
                        }
                    }
                    "Network.loadingFinished" => {
                        let rid = v
                            .pointer("/params/requestId")
                            .and_then(|r| r.as_str())
                            .map(str::to_string);
                        // 只对当前已认领的请求取响应体，且仅一次
                        if rid.is_some()
                            && request_id.as_deref() == rid.as_deref()
                            && body_cmd.is_none()
                        {
                            body_cmd = Some(cmd_id);
                            let cid = cmd_id;
                            cmd_id += 1;
                            ws.send(Message::Text(
                                json!({ "id": cid, "method": "Network.getResponseBody", "params": { "requestId": rid } })
                                    .to_string()
                                    .into(),
                            ))
                            .await
                            .map_err(|e| e.to_string())?;
                        }
                    }
                    _ => {}
                }
                // 命令回执：post data / response body 按各自记录的命令 id 归位；
                // 出错（error 字段）即时返回，避免傻等 4 分钟超时。
                if let Some(id) = v.get("id").and_then(|i| i.as_u64()) {
                    if let Some(err) = v
                        .get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str())
                    {
                        if post_cmd == Some(id) || body_cmd == Some(id) {
                            return Err(format!("抓取失败：CDP 拒绝命令: {err}"));
                        }
                    }
                    if post_cmd == Some(id) {
                        if let Some(pd) = v.pointer("/result/postData").and_then(|p| p.as_str()) {
                            if pd.is_empty() {
                                return Err(
                                    "抓取失败：用量接口请求体为空，请检查登录态".to_string()
                                );
                            }
                            post_data = Some(pd.to_string());
                        }
                    } else if body_cmd == Some(id) {
                        if let Some(b) = v
                            .get("result")
                            .and_then(|r| r.get("body"))
                            .and_then(|b| b.as_str())
                        {
                            sample = Some(b.to_string());
                        }
                    }
                }
                // Cookie + 完整 body 都拿到即可完成；响应样本是探测用的，拿不到也不阻塞
                if let (Some(c), Some(p)) = (cookie.clone(), post_data.clone()) {
                    return Ok(BailianCreds {
                        cookie: c,
                        post_data: p,
                        sample: sample.unwrap_or_default(),
                    });
                }
            }
        }
    }
    Err("等待登录超时（4 分钟）：请在打开的浏览器里登录百炼控制台".to_string())
}

fn extract_cookie(headers: &Value) -> Option<String> {
    let obj = headers.as_object()?;
    let (_, v) = obj.iter().find(|(k, _)| k.eq_ignore_ascii_case("cookie"))?;
    let s = v
        .as_str()
        .map(str::to_string)
        .or_else(|| v.as_array()?.first()?.as_str().map(str::to_string))?;
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

async fn sniff_auth_header(
    ws_url: &str,
    deadline: Instant,
    needles: &[&str],
    reload: bool,
    timeout_hint: &str,
) -> Result<String, String> {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::Message;

    let (mut ws, _) = connect_async(ws_url).await.map_err(|e| e.to_string())?;
    // CDP：id 从 1 开始顺序发命令
    ws.send(Message::Text(
        json!({ "id": 1, "method": "Network.enable", "params": {} })
            .to_string()
            .into(),
    ))
    .await
    .map_err(|e| e.to_string())?;
    // 附加到已开着的常驻页面时，请求早已发完；主动 reload 一次逼它再请求。
    if reload {
        ws.send(Message::Text(
            json!({ "id": 2, "method": "Page.enable", "params": {} })
                .to_string()
                .into(),
        ))
        .await
        .map_err(|e| e.to_string())?;
        ws.send(Message::Text(
            json!({ "id": 3, "method": "Page.reload", "params": { "ignoreCache": false } })
                .to_string()
                .into(),
        ))
        .await
        .map_err(|e| e.to_string())?;
    }

    while Instant::now() < deadline {
        let msg = tokio::select! {
            m = ws.next() => match m {
                Some(Ok(m)) => m,
                Some(Err(e)) => return Err(format!("CDP 连接断开: {e}")),
                None => return Err("CDP 连接关闭".to_string()),
            },
            _ = tokio::time::sleep(Duration::from_secs(1)) => continue,
        };
        if let Message::Text(t) = msg {
            if let Ok(v) = serde_json::from_str::<Value>(&t) {
                if v.get("method").and_then(|m| m.as_str()) == Some("Network.requestWillBeSent") {
                    let headers = v.pointer("/params/request/headers");
                    let url = v
                        .pointer("/params/request/url")
                        .and_then(|u| u.as_str())
                        .unwrap_or("");
                    if needles.iter().any(|n| url.contains(n)) {
                        if let Some(auth) = headers.and_then(extract_authorization) {
                            return Ok(auth);
                        }
                    }
                }
            }
        }
    }
    Err(format!("等待登录/抓取超时：{timeout_hint}"))
}

const SUOXIE_DASHBOARD: &str = "https://suoxie.codes/dashboard";

/// /json/new? 后面整串会被当 URL 主体，# 段会被 HTTP 层丢掉，先转义。
fn esc_cdp_url(url: &str) -> String {
    url.replace('%', "%25")
        .replace('#', "%23")
        .replace('?', "%3F")
        .replace('&', "%26")
}

/// 在常驻调试浏览器里找含 `url_contains` 的页面 target；没有就 PUT /json/new
/// 开一个 `open_url` 标签（只开一次，防重试循环狂开标签页）再轮询。
async fn find_or_open_page(
    client: &reqwest::Client,
    port: u16,
    url_contains: &str,
    open_url: &str,
) -> Option<String> {
    for i in 0..8 {
        if let Some(ws) = try_find_page(client, port, url_contains).await {
            return Some(ws);
        }
        if i == 0 {
            let _ = client
                .request(
                    reqwest::Method::PUT,
                    format!("{}/json/new?{}", json_url(port), esc_cdp_url(open_url)),
                )
                .timeout(Duration::from_secs(2))
                .send()
                .await;
        }
        tokio::time::sleep(Duration::from_millis(700)).await;
    }
    None
}

/// 附加抓取公共段：找/开页面 → CDP reload 逼页面重发请求 → 嗅 Authorization 头。
async fn attach_sniff_auth(
    client: &reqwest::Client,
    port: u16,
    url_contains: &str,
    open_url: &str,
    needles: &[&str],
    tag: &str,
) -> Option<String> {
    let Some(ws) = find_or_open_page(client, port, url_contains, open_url).await else {
        eprintln!(
            "[{tag}] {port} 不可达或打不开 {url_contains} 页（Edge 需带独立 --user-data-dir + --remote-debugging-port={port} 启动），回退新浏览器"
        );
        return None;
    };
    match sniff_auth_header(
        &ws,
        Instant::now() + Duration::from_secs(45),
        needles,
        true,
        "请确保常驻调试浏览器里已登录该站点",
    )
    .await
    {
        Ok(t) => Some(t),
        Err(e) => {
            eprintln!("[{tag}] 附加 {port} 抓取失败: {e}，回退新浏览器");
            None
        }
    }
}

/// 梭子蟹（suoxie.codes）：attach=Some(端口) 时优先附加常驻调试 Edge（复用登录态），
/// 不可用/抓取失败再回退拉起新浏览器登录。
pub async fn capture_suoxie_token(attach: Option<u16>) -> Result<String, String> {
    let client = reqwest::Client::new();
    // 1. 附加常驻调试浏览器
    if let Some(port) = attach {
        if let Some(t) = attach_sniff_auth(
            &client,
            port,
            "suoxie.codes",
            SUOXIE_DASHBOARD,
            &["suoxie.codes/api/v1/"],
            "suoxie",
        )
        .await
        {
            return Ok(t);
        }
    }
    // 2. 回退：独立临时 profile 新浏览器 + 登录嗅探
    let (mut child, port, dir) = spawn_browser(SUOXIE_DASHBOARD)?;
    let deadline = Instant::now() + Duration::from_secs(240);
    let page_ws = loop {
        if let Some(err) = child_gone(&mut child, &dir) {
            return Err(err);
        }
        if let Some(ws) = try_find_page(&client, port, "suoxie.codes").await {
            break ws;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = std::fs::remove_dir_all(&dir);
            return Err("等待打开 suoxie.codes 超时（4 分钟）".to_string());
        }
        tokio::time::sleep(Duration::from_millis(600)).await;
    };
    let r = sniff_auth_header(
        &page_ws,
        deadline,
        &["suoxie.codes/api/v1/"],
        false,
        "请在打开的浏览器里登录 suoxie.codes（4 分钟）",
    )
    .await;
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);
    r
}

fn extract_authorization(headers: &Value) -> Option<String> {
    let obj = headers.as_object()?;
    // CDP request.headers 的键是小写，这里不区分大小写找 Authorization
    let (_, v) = obj
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))?;
    let s = v
        .as_str()
        .map(str::to_string)
        .or_else(|| v.as_array()?.first()?.as_str().map(str::to_string))?;
    let t = s.trim().trim_start_matches("Bearer").trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_extract() {
        assert_eq!(
            extract_authorization(&json!({"Authorization": "Bearer abc.123"})).as_deref(),
            Some("abc.123")
        );
        assert_eq!(
            extract_authorization(&json!({"authorization": ["Bearer tok1"]})).as_deref(),
            Some("tok1")
        );
        assert!(extract_authorization(&json!({})).is_none());
        assert!(extract_authorization(&json!({"Authorization": "Bearer "})).is_none());
    }

    #[test]
    fn cdp_new_tab_url_escaped() {
        // # 段会被 HTTP 层丢掉，?/& 会被 /json/new 的查询串吃掉——都必须转义
        assert_eq!(
            esc_cdp_url("https://a.com/x?tab=plan#/y/z"),
            "https://a.com/x%3Ftab=plan%23/y/z"
        );
        assert_eq!(
            esc_cdp_url("https://suoxie.codes/dashboard"),
            "https://suoxie.codes/dashboard"
        );
    }

    #[test]
    fn cookie_extract() {
        assert_eq!(
            extract_cookie(&json!({"cookie": "login_aliyunid_ticket=abc; cna=x"})).as_deref(),
            Some("login_aliyunid_ticket=abc; cna=x")
        );
        assert!(extract_cookie(&json!({"Cookie": ""})).is_none());
        assert!(extract_cookie(&json!({})).is_none());
    }
}
