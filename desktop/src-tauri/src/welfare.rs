//! 梭子蟹每日福利自动化：凑一次中转调用（满足签到条件）→ 签到 → 抽奖 → 补签漏签日。
//! 福利接口走 suoxie.codes/api/v1（登录 JWT）；凑调用走 /v1/chat/completions（sk- API Key）。
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::{Emitter, Manager};

use crate::{balance, config};

const API: &str = "https://suoxie.codes/api/v1";
const CHAT_URL: &str = "https://suoxie.codes/v1/chat/completions";

fn bearer(jwt: &str) -> String {
    format!("Bearer {}", jwt.trim().trim_start_matches("Bearer ").trim())
}

/// 统一 JSON 请求：HTTP 非 2xx 或业务 code 非 0 都算失败（取 message）。
async fn call(req: reqwest::RequestBuilder) -> Result<Value, String> {
    let resp = req
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: Value = resp
        .json()
        .await
        .unwrap_or_else(|_| json!({ "message": "响应非 JSON" }));
    let code = body.get("code").and_then(|v| v.as_i64());
    if !status.is_success() || code.is_some_and(|c| c != 0) {
        let msg = body
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("请求失败");
        return Err(format!("HTTP {} {msg}", status.as_u16()));
    }
    Ok(body)
}

async fn get_status(client: &reqwest::Client, auth: &str) -> Result<Value, String> {
    let body = call(balance::suoxie_headers(
        client.get(format!("{API}/welfare/status")),
        auth,
    ))
    .await?;
    body.get("data")
        .cloned()
        .ok_or("welfare/status 结构异常".to_string())
}

/// 最小 chat 请求：只为凑「当日一次成功调用」，输入输出都压到最低。
async fn chat_min(client: &reqwest::Client, api_key: &str, model: &str) -> Result<(), String> {
    let req = client
        .post(CHAT_URL)
        .bearer_auth(api_key.trim().trim_start_matches("Bearer ").trim())
        .json(&json!({
            "model": model,
            "messages": [{ "role": "user", "content": "hi" }],
            "max_tokens": 1,
            "stream": false
        }))
        .timeout(Duration::from_secs(30));
    let resp = req.send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    let text = resp.text().await.unwrap_or_default();
    let snippet: String = text.chars().take(160).collect();
    Err(format!("chat HTTP {status}: {snippet}"))
}

fn bool_at(s: &Value, p: &str) -> bool {
    s.pointer(p).and_then(|v| v.as_bool()) == Some(true)
}

/// 是否需要先凑一次 API 调用（签到启用、未签到、要求用量但今日未达标）。
pub fn need_api_call(s: &Value) -> bool {
    bool_at(s, "/checkin/enabled")
        && !bool_at(s, "/checkin/checked_in")
        && bool_at(s, "/checkin/require_api_success")
        && !bool_at(s, "/checkin/api_requirement_met")
}

/// 抽奖条件：启用 + (本身可抽 或 刚签到解锁) + 未超每日次数。limit<=0 视为不限。
pub fn should_draw(s: &Value, just_checked_in: bool) -> bool {
    let limit = s
        .pointer("/lottery/daily_limit")
        .and_then(balance::num_or_str)
        .unwrap_or(0.0);
    let used = s
        .pointer("/lottery/daily_used")
        .and_then(balance::num_or_str)
        .unwrap_or(0.0);
    bool_at(s, "/lottery/enabled")
        && (bool_at(s, "/lottery/can_draw") || just_checked_in)
        && (limit <= 0.0 || used < limit)
}

/// 可补签的日期（streak.makeup.eligible_dates → "YYYY-MM-DD" 列表）。
pub fn makeup_dates(s: &Value) -> Vec<String> {
    if !bool_at(s, "/streak/makeup/enabled") {
        return vec![];
    }
    s.pointer("/streak/makeup/eligible_dates")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|d| d.get("date").and_then(|v| v.as_str()))
                .map(|d| d.chars().take(10).collect())
                .collect()
        })
        .unwrap_or_default()
}

/// 跑一遍完整流程，返回 {ok, date, message, reward}；date 非空表示今日可记完成。
pub async fn run_once(
    client: &reqwest::Client,
    jwt: &str,
    api_key: &str,
    model: &str,
) -> Result<Value, String> {
    let auth = bearer(jwt);
    let mut s = get_status(client, &auth).await?;
    if !bool_at(&s, "/enabled") {
        return Ok(json!({ "ok": true, "date": null, "message": "福利未启用" }));
    }
    let date = s
        .pointer("/checkin/date")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mut parts: Vec<String> = vec![];
    let mut reward: Option<f64> = None;

    // 1. 凑签到条件：发最小请求后轮询等服务端记账（实测几秒内生效）。
    if need_api_call(&s) {
        chat_min(client, api_key, model).await?;
        for _ in 0..5 {
            tokio::time::sleep(Duration::from_secs(3)).await;
            s = get_status(client, &auth).await?;
            if bool_at(&s, "/checkin/api_requirement_met") {
                break;
            }
        }
        if !bool_at(&s, "/checkin/api_requirement_met") {
            return Err("API 调用成功但福利端未确认用量，稍后自动重试".to_string());
        }
    }

    // 2. 签到。
    let mut just_in = false;
    if bool_at(&s, "/checkin/enabled") && !bool_at(&s, "/checkin/checked_in") {
        let body = call(balance::suoxie_headers(
            client.post(format!("{API}/welfare/check-in")),
            &auth,
        ))
        .await?;
        reward = body
            .pointer("/data/reward_balance")
            .or_else(|| body.pointer("/data/amount"))
            .and_then(balance::num_or_str);
        parts.push(match reward {
            Some(r) => format!("签到 +{r} 元"),
            None => "已签到".to_string(),
        });
        just_in = true;
    } else {
        parts.push("签到已完成".to_string());
    }

    // 3. 抽奖（免费，服务端自己校验签到/次数，被拒即报错、下小时重试）。
    if should_draw(&s, just_in) {
        let idem = format!("{:x}", now_nanos());
        let body = call(
            balance::suoxie_headers(client.post(format!("{API}/welfare/lottery/draw")), &auth)
                .header("Idempotency-Key", idem),
        )
        .await?;
        let prize = body
            .pointer("/data/prize_name_zh")
            .or_else(|| body.pointer("/data/prize_name"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        parts.push(format!(
            "抽奖：{}",
            if prize.is_empty() { "已完成" } else { prize }
        ));
    }

    // 4. 补签漏签日（恢复连签宝箱；配额耗尽即停，不影响当日结果）。
    let mut made: Vec<String> = vec![];
    for day in makeup_dates(&s).into_iter().take(5) {
        let req =
            balance::suoxie_headers(client.post(format!("{API}/welfare/check-in/makeup")), &auth)
                .json(&json!({ "date": day }));
        if let Err(e) = call(req).await {
            // 配额耗尽/无权限/网络问题都在此终止补签，当日结果不受影响
            eprintln!("[welfare] 补签 {day} 停止: {e}");
            break;
        }
        made.push(day);
    }
    if !made.is_empty() {
        parts.push(format!("补签 x{}", made.len()));
    }

    Ok(json!({
        "ok": true,
        "date": date,
        "message": parts.join("，"),
        "reward": reward,
        "makeup": made,
    }))
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// 同一进程内串行化：防后台循环与手动「立即执行」重叠（重复凑调用/抽奖、补签耗配额）。
static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 执行 + 落「今日已完成」标记 + 通知鲸鱼气泡。手动按钮与后台循环共用。
pub async fn execute(app: &tauri::AppHandle) -> Result<Value, String> {
    if RUNNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err("福利流程正在执行中，稍后再试".to_string());
    }
    let r = execute_inner(app).await;
    RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
    r
}

async fn execute_inner(app: &tauri::AppHandle) -> Result<Value, String> {
    let st = app.state::<crate::app_state::AppState>();
    let cfg = {
        let _g = st.cfg.lock().unwrap_or_else(|e| e.into_inner());
        config::read(&st.dir)
    };
    let (Some(jwt), Some(key)) = (cfg.suoxie_token.clone(), cfg.suoxie_api_key.clone()) else {
        return Err("缺梭子蟹 JWT 或 API Key，请到设置页补齐".to_string());
    };
    let res = run_once(&st.client, &jwt, &key, &cfg.suoxie_welfare_model).await;
    let payload = match &res {
        Ok(v) => {
            if let Some(d) = v.get("date").and_then(|x| x.as_str()) {
                if !d.is_empty() {
                    let _g = st.cfg.lock().unwrap_or_else(|e| e.into_inner());
                    config::write_suoxie_welfare_date(&st.dir, Some(d.to_string()));
                }
            }
            v.clone()
        }
        Err(e) => json!({ "ok": false, "error": e, "date": balance::suoxie_today() }),
    };
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.emit("suoxie-welfare", &payload);
    }
    res
}

/// 后台循环：启动即查一次，之后每小时一次。按上海时区日期防当日重复；
/// token 过期/网络抖动不记完成，下一小时自动重试。
pub async fn daily_loop(app: tauri::AppHandle) {
    loop {
        let today_due = {
            let st = app.state::<crate::app_state::AppState>();
            let _g = st.cfg.lock().unwrap_or_else(|e| e.into_inner());
            let cfg = config::read(&st.dir);
            cfg.suoxie_welfare
                && cfg.suoxie_token.is_some()
                && cfg.suoxie_api_key.is_some()
                && cfg.suoxie_welfare_date.as_deref() != Some(balance::suoxie_today().as_str())
        };
        if today_due {
            if let Err(e) = execute(&app).await {
                eprintln!("[welfare] 梭子蟹福利自动化失败: {e}");
            }
        }
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today_status() -> Value {
        // 2026-09-04 实测样本（节选）：未签到、API 用量未达标、抽奖锁定。
        json!({
            "enabled": true,
            "checkin": {
                "enabled": true, "checked_in": false, "date": "2026-09-04",
                "require_api_success": true, "api_requirement_met": false,
                "reward_balance": 0.1
            },
            "streak": { "makeup": { "enabled": true, "opportunity_state": "available", "eligible_dates": null } },
            "lottery": { "enabled": true, "can_draw": false, "daily_limit": 1, "daily_used": 0,
                          "unlock": { "unlocked": false, "reason": "checkin_required" } }
        })
    }

    #[test]
    fn real_sample_gates_flow() {
        let s = today_status();
        assert!(need_api_call(&s));
        assert!(!should_draw(&s, false));
        assert!(makeup_dates(&s).is_empty());
        // 刚签到应解锁抽奖（daily_used 仍 0）
        assert!(should_draw(&s, true));
        // 今日已签到且抽完 → 都不做
        let done = json!({
            "checkin": { "enabled": true, "checked_in": true, "require_api_success": true, "api_requirement_met": true },
            "lottery": { "enabled": true, "can_draw": false, "daily_limit": 1, "daily_used": 1 },
            "streak": { "makeup": { "enabled": true, "eligible_dates": [{ "date": "2026-09-02T10:00:00+08:00" }] } }
        });
        assert!(!need_api_call(&done));
        assert!(!should_draw(&done, false));
        assert_eq!(makeup_dates(&done), vec!["2026-09-02"]);
    }
}
