//! DeepSeek 接口调用：余额 + 平台用量，从 OLD lib/index.js 直译。
use std::time::Duration;
use time::format_description::well_known::Rfc3339;

use crate::app_state::{AppState, BalanceCache};
use crate::{config, ledger, pricing};
use serde_json::{json, Value};

pub const BALANCE_URL: &str = "https://api.deepseek.com/user/balance";
pub const USAGE_URL_BASE: &str = "https://platform.deepseek.com/api/v0/usage/by_api_key/amount";
pub const BALANCE_TTL_MS: u128 = 25_000;

fn now_secs() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

fn now_iso() -> String {
    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| String::new())
}

/// 命中缓存且还新 → 返回 payload（与 JS getBalance 的 25s TTL 一致）。
pub fn cached_payload(state: &AppState) -> Option<Value> {
    let c = state.cache.lock().ok()?.clone()?;
    let ok = c.payload.get("ok").and_then(|v| v.as_bool()) == Some(true);
    if ok && c.at.elapsed().as_millis() < BALANCE_TTL_MS {
        Some(c.payload.clone())
    } else {
        None
    }
}

async fn fetch_balance(client: &reqwest::Client, api_key: &str) -> Result<(f64, String), String> {
    let mut last_err: Option<String> = None;
    for attempt in 0..2 {
        let resp = client
            .get(BALANCE_URL)
            .header("Authorization", format!("Bearer {api_key}"))
            .timeout(Duration::from_secs(20))
            .send()
            .await;
        match resp {
            Ok(r) => {
                let status = r.status();
                if status.is_success() {
                    let body: Value = r
                        .json()
                        .await
                        .map_err(|e| format!("余额接口返回不是合法 JSON: {e}"))?;
                    let info = body
                        .get("balance_infos")
                        .and_then(|v| v.as_array())
                        .and_then(|a| a.first())
                        .ok_or("余额接口返回结构异常")?;
                    let total = info
                        .get("total_balance")
                        .and_then(|v| v.as_f64())
                        .ok_or("余额接口 total_balance 缺失")?;
                    let currency = info
                        .get("currency")
                        .and_then(|v| v.as_str())
                        .unwrap_or("CNY")
                        .to_string();
                    return Ok((total, currency));
                }
                last_err = Some(format!("HTTP {}", status.as_u16()));
                if status.as_u16() < 500 {
                    break;
                }
            }
            Err(e) => last_err = Some(e.to_string()),
        }
        if attempt == 0 {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
    Err(last_err.unwrap_or_else(|| "网络错误".to_string()))
}

/// 令牌实时用量模式：今日费用（元）。
async fn fetch_today_usage(client: &reqwest::Client, platform_token: &str) -> Result<f64, String> {
    let now = time::OffsetDateTime::now_local().map_err(|e| e.to_string())?;
    let midnight = now.replace_time(time::Time::MIDNIGHT);
    let start = midnight.unix_timestamp();
    let end = start + 86400;
    let tz = now.offset().whole_seconds();
    let url = format!("{USAGE_URL_BASE}?start={start}&end={end}&tz={tz}");

    let ress = client
        .get(&url)
        .header(
            "Authorization",
            format!(
                "Bearer {}",
                platform_token.trim_start_matches("Bearer ").trim()
            ),
        )
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !ress.status().is_success() {
        return Err(format!("http {}", ress.status().as_u16()));
    }
    let data: Value = ress.json().await.map_err(|e| e.to_string())?;
    pricing::compute_today_usage(&data)
        .map(|(cost, _)| cost)
        .ok_or_else(|| "no usage".to_string())
}

/// 组装完整 payload（记账/令牌双模式 + 峰谷标记 + 落缓存）。
pub async fn get_balance_payload(state: &AppState) -> Value {
    let cfg = config::read(&state.dir);
    let Some(key) = cfg.api_key.clone() else {
        return json!({
            "ok": false,
            "code": "NO_KEY",
            "error": "未配置 DEEPSEEK_API_KEY，请在托盘菜单「设置 API Key」"
        });
    };

    let (total, currency) = match fetch_balance(&state.client, &key).await {
        Ok(pair) => pair,
        Err(e) => {
            // 瞬时网络/接口抖动：沿用最近成功余额（与 JS transient 行为一致）
            if let Some(c) = state.cache.lock().unwrap().clone() {
                let mut v = c.payload.clone();
                v["stale"] = Value::Bool(true);
                v["error"] = json!(e);
                return v;
            }
            return json!({ "ok": false, "code": "HTTP", "error": format!("余额接口请求失败: {e}") });
        }
    };

    // 无论哪种模式都先把余额观测记入账本
    let led = ledger::record_usage(&state.dir, total);
    let mut payload = json!({
        "ok": true,
        "totalBalance": total,
        "currency": currency,
        "isPeak": pricing::is_peak_time(now_secs()),
        "updatedAt": now_iso(),
    });

    let mode = config::normalize(&cfg.usage_mode);
    if mode == "token" {
        if let Some(token) = cfg.platform_token.clone() {
            match fetch_today_usage(&state.client, &token).await {
                Ok(cost) => {
                    payload["todayUsage"] = json!(cost);
                    payload["usageMode"] = json!("token");
                }
                Err(_) => {
                    payload["todayUsage"] = json!(led.today_usage);
                    payload["usageMode"] = json!("ledger");
                }
            }
        } else {
            payload["todayUsage"] = json!(led.today_usage);
            payload["usageMode"] = json!("ledger");
        }
    } else {
        payload["todayUsage"] = json!(led.today_usage);
        payload["usageMode"] = json!("ledger");
    }

    *state.cache.lock().unwrap() = Some(BalanceCache {
        at: std::time::Instant::now(),
        payload: payload.clone(),
    });
    payload
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_ttl_respects_ok_flag() {
        // cached 只认「ok 且未过期」
        let state = AppState {
            dir: std::env::temp_dir(),
            client: reqwest::Client::new(),
            cache: std::sync::Mutex::new(Some(BalanceCache {
                at: std::time::Instant::now(),
                payload: json!({"ok": true}),
            })),
            busy: tokio::sync::Mutex::new(()),
        };
        assert!(cached_payload(&state).is_some());
        let expired = AppState {
            dir: std::env::temp_dir(),
            client: reqwest::Client::new(),
            cache: std::sync::Mutex::new(Some(BalanceCache {
                at: std::time::Instant::now() - Duration::from_secs(60),
                payload: json!({"ok": true}),
            })),
            busy: tokio::sync::Mutex::new(()),
        };
        assert!(cached_payload(&expired).is_none());
    }
}
