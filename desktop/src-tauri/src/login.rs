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

/// 拉起 Edge/Chrome 到 platform 用量页（独立临时 profile，不碰用户日常浏览器）。
fn spawn_browser() -> Result<Child, String> {
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
        "未找到 Edge/Chrome，请手动把 Authorization 复制到「平台令牌」输入框",
    )?;

    Command::new(exe)
        .arg("--remote-debugging-port=9333")
        .arg("--remote-allow-origins=*")
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-features=Translate,msEdgeShoppingAssist,AutofillServerCommunication")
        .arg("https://platform.deepseek.com/usage")
        .spawn()
        .map_err(|e| format!("浏览器启动失败: {e}"))
}

pub async fn capture_platform_token() -> Result<String, String> {
    let client = reqwest::Client::new();
    let mut child = spawn_browser()?;
    let started = Instant::now();
    let deadline = started + Duration::from_secs(240);

    // 1. 等浏览器调试端口就绪 + 出现 platform 页 target
    let page_ws = loop {
        if let Some(ws) = try_find_page(&client).await {
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

async fn try_find_page(client: &reqwest::Client) -> Option<String> {
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
        if url.contains("platform.deepseek.com") && p.get("type").and_then(|v| v.as_str()) == Some("page") {
            return Some(ws.to_string());
        }
    }
    None
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
}