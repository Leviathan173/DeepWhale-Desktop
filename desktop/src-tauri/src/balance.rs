//! DeepSeek 接口调用：余额 + 平台用量，从 OLD lib/index.js 直译。
use std::time::Duration;
use time::format_description::well_known::Rfc3339;

use crate::app_state::{AppState, BalanceCache};
use crate::{claude, config, ledger, opencode, pricing};
use serde_json::{json, Value};

pub const BALANCE_URL: &str = "https://api.deepseek.com/user/balance";
pub const USAGE_URL_BASE: &str = "https://platform.deepseek.com/api/v0/usage/by_api_key/amount";
pub const BALANCE_TTL_MS: u128 = 25_000;

pub const BAILIAN_API_URL: &str = "https://bailian-cs.console.aliyun.com/data/api.json";

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

/// 百炼 TokenPlan 订阅接口：原样重放登录时抓到的 form body（含 params/sec_token/region）。
async fn fetch_bailian_subscription(
    client: &reqwest::Client,
    cookie: &str,
    post_data: &str,
) -> Result<Value, String> {
    let url = format!(
        "{BAILIAN_API_URL}?action=BroadScopeAspnGateway&product=sfm_bailian&api=zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/subscription&_v=undefined"
    );
    let ress = client
        .post(&url)
        .header("content-type", "application/x-www-form-urlencoded")
        .header("cookie", cookie)
        .header("origin", "https://bailian.console.aliyun.com")
        .header("referer", "https://bailian.console.aliyun.com/cn-beijing?tab=plan")
        .body(post_data.to_string())
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !ress.status().is_success() {
        return Err(format!("http {}", ress.status().as_u16()));
    }
    ress.json::<Value>().await.map_err(|e| e.to_string())
}

/// 从订阅响应里宽泛提取 剩余额度/总额/已用/重置时间。
/// 字段名待拿到真实样本后定稿（见 bailian_sample.json）。用启发式扫描兜底：
/// 数值取 (remaining|surplus|available|left|quota) 类键，时间取 (expire|end|reset|cycle|nextRecharge)。
fn parse_bailian_subscription(body: &Value) -> Value {
    let data = body.get("data").unwrap_or(body);
    let success = data
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !success {
        let msg = data
            .get("errorMsg")
            .or_else(|| body.get("errorMsg"))
            .and_then(|v| v.as_str())
            .unwrap_or("未登录或接口异常")
            .to_string();
        return json!({ "ok": false, "error": msg });
    }

    let mut remaining: Option<f64> = None;
    let mut total: Option<f64> = None;
    let mut used: Option<f64> = None;
    let mut unit: Option<String> = None;
    let mut reset_at: Option<String> = None;
    scan_subscription(data, &mut remaining, &mut total, &mut used, &mut unit, &mut reset_at);

    json!({
        "ok": true,
        "remaining": remaining,
        "total": total,
        "used": used,
        "unit": unit,
        "resetAt": reset_at,
        "raw": body,
    })
}

fn scan_subscription(
    v: &Value,
    remaining: &mut Option<f64>,
    total: &mut Option<f64>,
    used: &mut Option<f64>,
    unit: &mut Option<String>,
    reset_at: &mut Option<String>,
) {
    match v {
        Value::Object(m) => {
            for (k, val) in m {
                let kl = k.to_ascii_lowercase();
                match val {
                    Value::String(s) => {
                        if reset_at.is_none()
                            && (kl.contains("expire")
                                || kl.contains("endtime")
                                || kl.contains("reset")
                                || kl.contains("cycleend")
                                || kl.contains("nextrecharge"))
                            && (s.contains('-') || s.contains(':') && s.chars().filter(|c| *c == ':').count() >= 2)
                        {
                            *reset_at = Some(s.clone());
                        } else if unit.is_none()
                            && (kl.contains("unit") || kl.contains("quotaunit") || kl.contains("billingmode"))
                            && !s.is_empty()
                        {
                            *unit = Some(s.clone());
                        }
                    }
                    Value::Number(_) => {
                        if let Some(n) = val.as_f64() {
                            if remaining.is_none()
                                && (kl.contains("remaining")
                                    || kl.contains("remain")
                                    || kl.contains("surplus")
                                    || kl.contains("available")
                                    || kl.contains("left"))
                            {
                                *remaining = Some(n);
                            } else if total.is_none()
                                && (kl.contains("total") || kl.contains("limit") || kl.contains("quota"))
                            {
                                *total = Some(n);
                            } else if used.is_none()
                                && (kl.contains("used") || kl.contains("consume") || kl.contains("cost"))
                            {
                                *used = Some(n);
                            }
                        }
                    }
                    _ => {}
                }
                scan_subscription(val, remaining, total, used, unit, reset_at);
            }
        }
        Value::Array(a) => {
            for e in a {
                scan_subscription(e, remaining, total, used, unit, reset_at);
            }
        }
        _ => {}
    }
}

/// 组装完整 payload（记账/令牌双模式 + 峰谷标记 + 落缓存）。
pub async fn get_balance_payload(state: &AppState) -> Value {
    // ponytail: 只取配置快照就释放锁，避免 std MutexGuard 跨 await（async 命令要求 Send）
    let cfg = {
        let _g = state.cfg.lock().unwrap_or_else(|e| e.into_inner());
        config::read(&state.dir)
    };

    // 百炼订阅（独立于 DeepSeek 的主流程）：有凭据才抓，全程并发跑，主余额不受拖累。
    let bailian_task = match (cfg.bailian_cookie.clone(), cfg.bailian_post_data.clone()) {
        (Some(ck), Some(pd)) => {
            let client = state.client.clone();
            Some(tokio::spawn(async move {
                match fetch_bailian_subscription(&client, &ck, &pd).await {
                    Ok(body) => parse_bailian_subscription(&body),
                    Err(e) => json!({ "ok": false, "error": format!("接口请求失败: {e}") }),
                }
            }))
        }
        _ => None,
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

    // 百炼 TokenPlan（订阅制），收并发任务的尾。
    payload["bailian"] = match bailian_task {
        Some(t) => match t.await {
            Ok(v) => v,
            Err(e) => json!({ "ok": false, "error": format!("任务失败: {e}") }),
        },
        None => json!({ "ok": false, "configured": false }),
    };

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

    #[test]
    fn bailian_sample_shape_parsed() {
        // 登录态响应：remaining/total/used/resetAt 启发式提取
        let out = parse_bailian_subscription(&json!({
            "code": "200",
            "data": {
                "success": true,
                "result": {
                    "remainingQuota": 860000,
                    "totalQuota": 1000000,
                    "usedQuota": 140000,
                    "cycleEndTime": "2026-08-29T00:00:00+08:00",
                    "unit": "tokens"
                }
            }
        }));
        assert_eq!(out["ok"], true);
        assert_eq!(out["remaining"], 860000.0);
        assert_eq!(out["total"], 1000000.0);
        assert_eq!(out["used"], 140000.0);
        assert!(out["resetAt"].as_str().unwrap().contains("2026-08-29"));
    }

    #[test]
    fn bailian_not_logined() {
        let out = parse_bailian_subscription(&json!({
            "code": "200",
            "data": {
                "success": false,
                "errorCode": "BailianGateway.Login.NotLogined",
                "errorMsg": "BailianGateway.Login.NotLogined"
            }
        }));
        assert_eq!(out["ok"], false);
        assert!(out["error"].as_str().unwrap().contains("NotLogined"));
    }
}
