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
                    // 官方接口 total_balance 是字符串（"110.00"），但兼容 JSON number 两种形态
                    .and_then(num_or_str)
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

/// 百炼 TokenPlan 用量/订阅接口：原样重放登录时抓到的 form body（含 params/sec_token）。
async fn fetch_bailian_subscription(
    client: &reqwest::Client,
    cookie: &str,
    post_data: &str,
) -> Result<Value, String> {
    let url = format!(
        "{BAILIAN_API_URL}?action=BroadScopeAspnGateway&product=sfm_bailian&api=zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/usage&_v=undefined"
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

/// 解析百炼 TokenPlan 用量接口响应（实测结构）：
/// {
///   code, data: { success, DataV2: { data: { success, data: {
///     per1WeekResetTime: <ms>,      // 本周重置时间
///     per1WeekPercentage: 0.19      // 本周已用比例（0~1）
///   } } } }
/// }
/// 也兼容订阅接口（remainingDays / endTime），字段取哪个按抓到样本而定。
fn parse_bailian_subscription(body: &Value) -> Value {
    let data = body.get("data").unwrap_or(body);
    // 成功判断双保险：data.success 与顶层 successResponse 任一为 true 都视为成功；
    // 均缺失或任一显式 false → 失败。
    let data_success = data.get("success").and_then(|v| v.as_bool());
    let outer_success = body.get("successResponse").and_then(|v| v.as_bool());
    let success = matches!(
        (data_success, outer_success),
        (Some(true), None) | (None, Some(true)) | (Some(true), Some(true))
    );
    if !success {
        let msg = data
            .get("errorMsg")
            .or_else(|| body.get("errorMsg"))
            .and_then(|v| v.as_str())
            .unwrap_or("未登录或接口异常")
            .to_string();
        return json!({ "ok": false, "error": msg });
    }

    // 钻进 DataV2.data.data 取真实业务字段
    let inner = data
        .pointer("/DataV2/data/data")
        .or_else(|| data.pointer("/DataV2/data"))
        .or_else(|| data.get("data"));

    let mut reset_at: Option<f64> = None;
    let mut percent_str: Option<String> = None;
    let mut remaining_days: Option<f64> = None;
    let mut end_time: Option<f64> = None;

    if let Some(inner) = inner {
        if let Some(p) = inner.get("per1WeekPercentage").and_then(|v| v.as_f64()) {
            percent_str = Some(format!("{:.1}%", (1.0 - p) * 100.0));
            reset_at = inner.get("per1WeekResetTime").and_then(num_or_str);
        }
        if let Some(d) = inner.get("remainingDays").and_then(num_or_str) {
            remaining_days = Some(d);
        }
        if let Some(t) = inner.get("endTime").and_then(num_or_str) {
            end_time = Some(t);
        }
    }

    // 兜底：任意成功响应也算 ok（字段缺失时前端显示占位）。
    // 不塞 raw 原始响应：避免把敏感业务数据带进缓存/前端。
    json!({
        "ok": true,
        "remaining": percent_str,
        "resetAt": reset_at,
        "resetAtMs": reset_at,
        "remainingDays": remaining_days,
        "endTime": end_time,
    })
}

/// 数值字段可能是 JSON number 或数字字符串（毫秒时间戳尤其常见），统一转 f64。
fn num_or_str(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
}

/// 组装完整 payload（记账/令牌双模式 + 峰谷标记 + 落缓存）。
pub async fn get_balance_payload(state: &AppState) -> Value {
    // ponytail: 只取配置快照就释放锁，避免 std MutexGuard 跨 await（async 命令要求 Send）
    let cfg = {
        let _g = state.cfg.lock().unwrap_or_else(|e| e.into_inner());
        config::read(&state.dir)
    };

    // 百炼订阅（独立于 DeepSeek 的主流程）：有凭据才抓，全程并发跑，主余额不受拖累。
    let mut bailian_task = match (cfg.bailian_cookie.clone(), cfg.bailian_post_data.clone()) {
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
        // 已配置百炼却没配 DeepSeek key：先收百炼任务，前端仍能展示百炼数据
        let bailian = match bailian_task.take() {
            Some(t) => match t.await {
                Ok(v) => v,
                Err(e) => json!({ "ok": false, "error": format!("任务失败: {e}") }),
            },
            None => json!({ "ok": false, "configured": false }),
        };
        return json!({
            "ok": false,
            "code": "NO_KEY",
            "error": "未配置 API Key，请点鲸鱼菜单「设置 API Key」",
            "bailian": bailian,
        });
    };

    let (total, currency) = match fetch_balance(&state.client, &key).await {
        Ok(pair) => pair,
        Err(e) => {
            // 瞬时网络/接口抖动：沿用最近成功余额（与 JS transient 行为一致）
            // 先取快照释放锁，再收百炼任务（避免 std MutexGuard 跨 await 非 Send）
            let cached = state.cache.lock().unwrap().clone();
            if let Some(c) = cached {
                let best = match bailian_task.take() {
                    Some(t) => match t.await {
                        Ok(v) => v,
                        Err(e) => json!({ "ok": false, "error": format!("任务失败: {e}") }),
                    },
                    None => json!({ "ok": false, "configured": false }),
                };
                let mut v = c.payload.clone();
                v["bailian"] = best;
                v["stale"] = Value::Bool(true);
                v["error"] = json!(e);
                return v;
            }
            let bailian = match bailian_task.take() {
                Some(t) => match t.await {
                    Ok(v) => v,
                    Err(e) => json!({ "ok": false, "error": format!("任务失败: {e}") }),
                },
                None => json!({ "ok": false, "configured": false }),
            };
            return json!({
                "ok": false,
                "code": "HTTP",
                "error": format!("余额接口请求失败: {e}"),
                "bailian": bailian
            });
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
    payload["bailian"] = match bailian_task.take() {
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
        // 实测用量接口响应：per1WeekPercentage + per1WeekResetTime(ms)
        let body: Value = serde_json::from_str(r#"{
            "code": "200",
            "data": {
                "DataV2": {
                    "ret": ["SUCCESS::接口调用成功"],
                    "data": {
                        "msg": "Success.",
                        "code": "SUCCESS",
                        "data": {
                            "per1WeekResetTime": 1787728620000,
                            "per1WeekPercentage": 0.191642446
                        },
                        "requestId": "x",
                        "success": true
                    }
                },
                "success": true,
                "httpStatus": 200,
                "errorCode": "",
                "api": "zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/usage",
                "errorMsg": ""
            },
            "httpStatusCode": "200",
            "successResponse": true
        }"#)
        .unwrap();
        let out = parse_bailian_subscription(&body);
        assert_eq!(out["ok"], true);
        assert_eq!(out["remaining"].as_str().unwrap(), "80.8%");
        assert_eq!(out["resetAtMs"], 1787728620000.0);
    }

    #[test]
    fn bailian_sample_subscription_shape() {
        // 订阅接口：remainingDays / endTime 也应被提取
        let body: Value = serde_json::from_str(r#"{
            "code": "200",
            "data": {
                "DataV2": { "data": { "data": {
                    "remainingDays": 70,
                    "endTime": 1793462400000,
                    "status": "VALID"
                }, "success": true } },
                "success": true,
                "httpStatus": 200,
                "errorCode": "",
                "api": "zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/subscription",
                "errorMsg": ""
            },
            "httpStatusCode": "200",
            "successResponse": true
        }"#)
        .unwrap();
        let out = parse_bailian_subscription(&body);
        assert_eq!(out["ok"], true);
        assert_eq!(out["remainingDays"], 70.0);
        assert_eq!(out["endTime"], 1793462400000.0);
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
