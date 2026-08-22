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
        .arg(format!("--remote-allow-origins=http://127.0.0.1:{port},http://localhost:{port}"))
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

pub async fn capture_platform_token() -> Result<String, String> {
    let client = reqwest::Client::new();
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
    let token = match sniff_auth_header(&page_ws, deadline).await {
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

/// 百炼 TokenPlan：拉起独立浏览器到订阅页，等用户登录后嗅探
/// 订阅/用量接口的请求 Cookie + 完整 form body，并抓一次响应体存为样本
/// （登录后该页会自动请求这两个接口）。
pub async fn capture_bailian_credentials() -> Result<BailianCreds, String> {
    let client = reqwest::Client::new();
    let (mut child, port, dir) = spawn_browser(
        "https://bailian.console.aliyun.com/cn-beijing?tab=plan#/efm/subscription/token-plan/personal",
    )?;
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

    let creds = match sniff_bailian(&page_ws, deadline).await {
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

async fn sniff_bailian(ws_url: &str, deadline: Instant) -> Result<BailianCreds, String> {
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

    // 注意：Chrome 的 Cookie 头走 Network.requestWillBeSentExtraInfo，
    // 不在 requestWillBeSent 的请求头里。这里两种事件都收，按 requestId 关联。
    let mut cookie: Option<String> = None;
    let mut request_id: Option<String> = None;
    let mut post_data: Option<String> = None;
    let mut sample: Option<String> = None;
    // CDP 命令 id 必须单调递增；每个命令回执按 id 关联到对应请求。
    let mut cmd_id: u64 = 2;
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
                let method = v.get("method").and_then(|m| m.as_str()).unwrap_or("").to_string();
                match method.as_str() {
                    "Network.requestWillBeSentExtraInfo" => {
                        // 该事件的 headers 里有登录 Cookie；所有 balian 请求共用同一会话 Cookie。
                        // 始终取最新（登录后才有 login_aliyunid，登录前只有匿名 cna）。
                        if let Some(ck) = v
                            .pointer("/params/headers")
                            .and_then(extract_cookie)
                        {
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
                        if rid.is_some() && request_id.as_deref() == rid.as_deref() && body_cmd.is_none() {
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
                    if let Some(err) = v.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()) {
                        if post_cmd == Some(id) || body_cmd == Some(id) {
                            return Err(format!("抓取失败：CDP 拒绝命令: {err}"));
                        }
                    }
                    if post_cmd == Some(id) {
                        if let Some(pd) = v.pointer("/result/postData").and_then(|p| p.as_str()) {
                            if pd.is_empty() {
                                return Err("抓取失败：用量接口请求体为空，请检查登录态".to_string());
                            }
                            post_data = Some(pd.to_string());
                        }
                    } else if body_cmd == Some(id) {
                        if let Some(b) = v.get("result").and_then(|r| r.get("body")).and_then(|b| b.as_str()) {
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
    let s = v.as_str().map(str::to_string).or_else(|| {
        v.as_array()?
            .first()?
            .as_str()
            .map(str::to_string)
    })?;
    let t = s.trim();
    if t.is_empty() { None } else { Some(t.to_string()) }
}

async fn sniff_auth_header(ws_url: &str, deadline: Instant) -> Result<String, String> {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::Message;

    let (mut ws, _) = connect_async(ws_url).await.map_err(|e| e.to_string())?;
    // CDP：id 从 1 开始顺序发命令
    ws.send(Message::Text(
        json!({ "id": 1, "method": "Network.enable", "params": {} }).to_string().into(),
    ))
    .await
    .map_err(|e| e.to_string())?;

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
                    if url.contains("by_api_key/amount") || url.contains("/api/v0/usage/") {
                        if let Some(auth) = headers.and_then(extract_authorization) {
                            return Ok(auth);
                        }
                    }
                }
            }
        }
    }
    Err("等待登录超时（4 分钟）：请在打开的浏览器里登录 platform.deepseek.com".to_string())
}

fn extract_authorization(headers: &Value) -> Option<String> {
    let obj = headers.as_object()?;
    // CDP request.headers 的键是小写，这里不区分大小写找 Authorization
    let (_, v) = obj.iter().find(|(k, _)| k.eq_ignore_ascii_case("authorization"))?;
    let s = v.as_str().map(str::to_string).or_else(|| {
        v.as_array()?
            .first()?
            .as_str()
            .map(str::to_string)
    })?;
    let t = s.trim().trim_start_matches("Bearer").trim();
    if t.is_empty() { None } else { Some(t.to_string()) }
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
    fn cookie_extract() {
        assert_eq!(
            extract_cookie(&json!({"cookie": "login_aliyunid_ticket=abc; cna=x"})).as_deref(),
            Some("login_aliyunid_ticket=abc; cna=x")
        );
        assert!(extract_cookie(&json!({"Cookie": ""})).is_none());
        assert!(extract_cookie(&json!({})).is_none());
    }
}