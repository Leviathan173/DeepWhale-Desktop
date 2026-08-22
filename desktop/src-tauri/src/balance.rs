//! DeepSeek 接口调用：余额 + 平台用量，从 OLD lib/index.js 直译。
use std::time::Duration;
use time::format_description::well_known::Rfc3339;

use crate::app_state::{AppState, BalanceCache};
use crate::{claude, config, ledger, opencode, pricing};
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
    // ponytail: 前端 busy 会挡住手动刷新约整段时间；收紧到单次 15s 超时（放弃双尝试），
    // 保证一次完整请求 + 用量请求总耗时 < 25s。
    let resp = client
        .get(BALANCE_URL)
        .header("Authorization", format!("Bearer {api_key}"))
        .timeout(Duration::from_secs(15))
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
                    // 官方接口 total_balance 是字符串（"110.00"），as_f64 只认 JSON number
                    .and_then(|v| v.as_str().and_then(|s| s.parse::<f64>().ok()))
                    .ok_or("余额接口 total_balance 缺失")?;
                let currency = info
                    .get("currency")
                    .and_then(|v| v.as_str())
                    .unwrap_or("CNY")
                    .to_string();
                Ok((total, currency))
            } else {
                Err(format!("HTTP {}", status.as_u16()))
            }
        }
        Err(e) => Err(e.to_string()),
    }
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
    // ponytail: 只取配置快照就释放锁，避免 std MutexGuard 跨 await（async 命令要求 Send）
    let cfg = {
        let _g = state.cfg.lock().unwrap_or_else(|e| e.into_inner());
        config::read(&state.dir)
    };
    let Some(key) = cfg.api_key.clone() else {
        return json!({
            "ok": false,
            "code": "NO_KEY",
            "error": "未配置 API Key，请点鲸鱼菜单「设置 API Key」"
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
    } else if mode == "opencode" {
        let db = opencode::db_path(cfg.opencode_db.as_deref());
        let providers = cfg
            .usage_providers
            .clone()
            .unwrap_or_else(pricing::default_providers);
        // 本地总账：opencode.db + Claude Code jsonl 叠加；两者都读不到才回退 ledger。
        let cost = match (
            opencode::today_cost(&db, &providers),
            claude::today_cost(None, &providers),
        ) {
            (Some((a, _)), Some((b, _))) => Some(a + b),
            (Some((a, _)), None) => Some(a),
            (None, Some((b, _))) => Some(b),
            (None, None) => None,
        };
        match cost {
            Some(c) => {
                payload["todayUsage"] = json!(c);
                payload["usageMode"] = json!("opencode");
            }
            None => {
                payload["todayUsage"] = json!(led.today_usage);
                payload["usageMode"] = json!("ledger");
            }
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
            cfg: std::sync::Mutex::new(()),
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
            cfg: std::sync::Mutex::new(()),
        };
        assert!(cached_payload(&expired).is_none());
    }
}
