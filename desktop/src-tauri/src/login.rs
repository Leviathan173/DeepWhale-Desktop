//! 登录令牌抓取：拉起带调试端口的浏览器，等用户登录后通过 CDP 嗅探
//! platform 用量接口的 Authorization 头，把小鲸鱼拿不到但页面会用的
//! Debug 令牌存下来。
use serde_json::{json, Value};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

pub const DEBUG_PORT: u16 = 9333;

fn json_url() -> String {
    format!("http://127.0.0.1:{DEBUG_PORT}/json")
}

/// 拉起 Edge/Chrome 到指定页面（独立临时 profile，不碰用户日常浏览器）。
fn spawn_browser(url: &str) -> Result<Child, String> {
    let mut path = std::env::temp_dir();
    path.push(format!("dshwl-{}", std::process::id()));
    std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    let profile = path.join("profile");
    std::fs::create_dir_all(&profile).map_err(|e| e.to_string())?;

    let candidates = [
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    ];
    let exe = candidates.iter().find(|p| std::path::Path::new(p).exists()).copied().ok_or(
        "未找到 Edge/Chrome，请手动复制 Cookie 填入百炼输入框",
    )?;

    Command::new(exe)
        .arg("--remote-debugging-port=9333")
        .arg("--remote-allow-origins=*")
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-features=Translate,msEdgeShoppingAssist,AutofillServerCommunication")
        .arg(url)
        .spawn()
        .map_err(|e| format!("浏览器启动失败: {e}"))
}

pub async fn capture_platform_token() -> Result<String, String> {
    let client = reqwest::Client::new();
    let mut child = spawn_browser("https://platform.deepseek.com/usage")?;
    let started = Instant::now();
    let deadline = started + Duration::from_secs(240);

    // 1. 等浏览器调试端口就绪 + 出现 platform 页 target
    let page_ws = loop {
        if let Some(ws) = try_find_page(&client, "platform.deepseek.com").await {
            break ws;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
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
            return Err(e);
        }
    };
    let _ = child.kill();
    Ok(token)
}

async fn try_find_page(client: &reqwest::Client, url_contains: &str) -> Option<String> {
    let list = client
        .get(json_url())
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
/// 订阅接口的请求 Cookie + 完整 form body，并抓一次响应体存为样本
/// （登录后该页会自动请求订阅接口）。
pub async fn capture_bailian_credentials() -> Result<BailianCreds, String> {
    let client = reqwest::Client::new();
    let mut child = spawn_browser(
        "https://bailian.console.aliyun.com/cn-beijing?tab=plan#/efm/subscription/token-plan/personal",
    )?;
    let started = Instant::now();
    let deadline = started + Duration::from_secs(240);

    let page_ws = loop {
        if let Some(ws) = try_find_page(&client, "bailian.console.aliyun.com").await {
            break ws;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            return Err("等待百炼登录超时（4 分钟）".to_string());
        }
        tokio::time::sleep(Duration::from_millis(600)).await;
    };

    let creds = match sniff_bailian(&page_ws, deadline).await {
        Ok(c) => c,
        Err(e) => {
            let _ = child.kill();
            return Err(e);
        }
    };
    let _ = child.kill();
    Ok(creds)
}

const BAILIAN_API: &str = "zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/subscription";

async fn sniff_bailian(ws_url: &str, deadline: Instant) -> Result<BailianCreds, String> {
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

    let mut cookie: Option<String> = None;
    let mut request_id: Option<String> = None;
    let mut post_data: Option<String> = None;
    let mut sample: Option<String> = None;
    let mut pending_body: Option<String> = None;

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
                let method = v.get("method").and_then(|m| m.as_str()).unwrap_or("");
                if method == "Network.requestWillBeSent" {
                    let url = v
                        .pointer("/params/request/url")
                        .and_then(|u| u.as_str())
                        .unwrap_or("");
                    // 订阅接口 URL：action=BroadScopeAspnGateway + api=.../tokenplan/personal/api/v2/subscription
                    if url.contains("data/api.json") && url.contains(BAILIAN_API) {
                        let rid = v
                            .pointer("/params/requestId")
                            .and_then(|r| r.as_str())
                            .map(str::to_string);
                        let ck = v
                            .pointer("/params/request/headers")
                            .and_then(extract_cookie);
                        // 登录后请求才有 cookie；抓到即去向
                        if let (Some(r), Some(c)) = (rid, ck) {
                            request_id = Some(r.clone());
                            cookie = Some(c.clone());
                            // 主动拿完整 form body（含 params/sec_token/region）
                            ws.send(Message::Text(
                                json!({ "id": 2, "method": "Network.getRequestPostData", "params": { "requestId": r } })
                                    .to_string()
                                    .into(),
                            ))
                            .await
                            .map_err(|e| e.to_string())?;
                        }
                    }
                } else if method == "Network.loadingFinished" && pending_body.is_none() {
                    let rid = v
                        .pointer("/params/requestId")
                        .and_then(|r| r.as_str())
                        .map(str::to_string);
                    if rid.is_some() && request_id.as_deref() == rid.as_deref() {
                        pending_body = rid.clone();
                        ws.send(Message::Text(
                            json!({ "id": 3, "method": "Network.getResponseBody", "params": { "requestId": rid } })
                                .to_string()
                                .into(),
                        ))
                        .await
                        .map_err(|e| e.to_string())?;
                    }
                }
                // 命令回执：id=2 post data，id=3 response body
                if let Some(id) = v.get("id").and_then(|i| i.as_u64()) {
                    if id == 2 {
                        if let Some(pd) = v.pointer("/result/postData").and_then(|p| p.as_str()) {
                            if pd.is_empty() {
                                return Err("抓取失败：订阅接口请求体为空，请检查登录态".to_string());
                            }
                            post_data = Some(pd.to_string());
                        }
                    } else if id == 3 {
                        if let Some(b) = v.get("result").and_then(|r| r.get("body")).and_then(|b| b.as_str()) {
                            sample = Some(b.to_string());
                        }
                    }
                }
                if let (Some(c), Some(p)) = (cookie.clone(), post_data.clone()) {
                    // 响应样本是探测用的，拿不到也不阻塞完成
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