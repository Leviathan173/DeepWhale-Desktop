//! DeepSeek 接口调用：余额 + 平台用量，从 OLD lib/index.js 直译。
use std::time::Duration;
use time::format_description::well_known::Rfc3339;

use crate::app_state::{AppState, BalanceCache};
use crate::{apilog, claude, config, opencode, pricing};
use serde_json::{json, Value};

pub const BALANCE_URL: &str = "https://api.deepseek.com/user/balance";
pub const USAGE_URL_BASE: &str = "https://platform.deepseek.com/api/v0/usage/by_api_key/amount";
/// 平台「消费金额」同源接口：直接给今日费用，不像 amount 接口要给 token 自己乘价目。
pub const USAGE_COST_URL_BASE: &str = "https://platform.deepseek.com/api/v0/usage/by_api_key/cost";
pub const BALANCE_TTL_MS: u128 = 25_000;

pub const BAILIAN_API_URL: &str = "https://bailian-cs.console.aliyun.com/data/api.json";

pub const SUOXIE_BASE: &str = "https://suoxie.codes";

/// 梭子蟹服务端 TTFB 实测 2–22s（chunked 流还会拖），connect 也常有 10s+ 卡顿：
/// 单请求放宽到 30s（连接阶段由 client 的 connect_timeout 兜底），别把「对方慢」误判成失败。
pub const SUOXIE_TIMEOUT: Duration = Duration::from_secs(30);

/// 收集一路并行抓取任务（None → 未配置）。统一三处早返回里的收尾样板。
async fn join_side(t: Option<tokio::task::JoinHandle<Value>>) -> Value {
    match t {
        Some(t) => match t.await {
            Ok(v) => v,
            Err(e) => json!({ "ok": false, "error": format!("任务失败: {e}") }),
        },
        None => json!({ "ok": false, "configured": false }),
    }
}

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
    let (status, text) = apilog::send(
        "deepseek",
        client
            .get(BALANCE_URL)
            .header("Authorization", format!("Bearer {api_key}"))
            .timeout(Duration::from_secs(15)),
    )
    .await?;
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}"));
    }
    let body: Value =
        serde_json::from_str(&text).map_err(|e| format!("余额接口返回不是合法 JSON: {e}"))?;
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
}

/// 平台用量接口的通用 GET：cost / amount 共用，返回解析后的 JSON。
async fn fetch_platform_usage(
    client: &reqwest::Client,
    platform_token: &str,
    url: &str,
) -> Result<Value, String> {
    let (status, text) = apilog::send(
        "platform",
        client
            .get(url)
            .header(
                "Authorization",
                format!(
                    "Bearer {}",
                    platform_token.trim_start_matches("Bearer ").trim()
                ),
            )
            .timeout(Duration::from_secs(15)),
    )
    .await?;
    if !(200..300).contains(&status) {
        return Err(format!("http {status}"));
    }
    let data: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    // 平台令牌失效时回的是 HTTP 200 + 业务码（实测 code=40003
    // "Authorization Failed (invalid token)"），并不会给 401。不识别就会静默回退
    // 本地记账，后台自动续期永远不触发——这是 DeepSeek 侧最隐蔽的一条漏网路径。
    if let Some(code) = data.get("code").and_then(|v| v.as_i64()) {
        if code != 0 {
            let msg = data.get("msg").and_then(|v| v.as_str()).unwrap_or("");
            return Err(format!("platform code {code}: {msg}"));
        }
    }
    Ok(data)
}

/// 令牌实时用量模式：今日费用（元）。
/// 优先平台 cost 接口（与页面「消费金额」同源，不受本地价目漂移影响）；
/// 失败回退 amount 接口按本地价目重算。
async fn fetch_today_usage(client: &reqwest::Client, platform_token: &str) -> Result<f64, String> {
    let now = time::OffsetDateTime::now_local().map_err(|e| e.to_string())?;
    let midnight = now.replace_time(time::Time::MIDNIGHT);
    let start = midnight.unix_timestamp();
    let end = start + 86400;
    let tz = now.offset().whole_seconds();

    let cost_url = format!("{USAGE_COST_URL_BASE}?start={start}&end={end}&tz={tz}");
    match fetch_platform_usage(client, platform_token, &cost_url).await {
        Ok(data) => {
            if let Some(cost) = pricing::compute_today_cost(&data) {
                return Ok(cost);
            }
        }
        Err(e) => {
            // 令牌失效时 amount 接口同样不会通，直接抛出触发续期，别白跑一趟。
            if platform_auth_error(&e) {
                return Err(e);
            }
            eprintln!("[balance] 平台 cost 接口失败，回退 amount: {e}");
        }
    }

    let amount_url = format!("{USAGE_URL_BASE}?start={start}&end={end}&tz={tz}");
    let data = fetch_platform_usage(client, platform_token, &amount_url).await?;
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
        // 控制台当前发送 `_v=` 空值；`undefined` 会被部分网关当成不同请求。
        "{BAILIAN_API_URL}?action=BroadScopeAspnGateway&product=sfm_bailian&api=zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/usage&_v="
    );
    let (status, text) = apilog::send(
        "bailian",
        client
            .post(&url)
            .header("content-type", "application/x-www-form-urlencoded")
            .header("cookie", cookie)
            .header("origin", "https://bailian.console.aliyun.com")
            .header(
                "referer",
                "https://bailian.console.aliyun.com/cn-beijing/subscription/token-plan/personal",
            )
            .body(post_data.to_string())
            .timeout(Duration::from_secs(10)),
    )
    .await?;
    if !(200..300).contains(&status) {
        return Err(format!("http {status}"));
    }
    serde_json::from_str::<Value>(&text).map_err(|e| e.to_string())
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
        let lower = msg.to_ascii_lowercase();
        let auth_expired = lower.contains("notlogined")
            || lower.contains("not logged")
            || lower.contains("未登录");
        return json!({ "ok": false, "error": msg, "credentialExpired": auth_expired });
    }

    // 钻进 DataV2.data.data 取真实业务字段
    let inner = data
        .pointer("/DataV2/data/data")
        .or_else(|| data.pointer("/DataV2/data"))
        .or_else(|| data.get("data"));

    let mut reset_at: Option<f64> = None;
    let mut percent_str: Option<String> = None;
    let mut period = "subscription";
    let mut remaining_days: Option<f64> = None;
    let mut end_time: Option<f64> = None;

    if let Some(inner) = inner {
        if let Some(p) = inner.get("per1MonthPercentage").and_then(|v| v.as_f64()) {
            percent_str = Some(format!("{:.1}%", (1.0 - p) * 100.0));
            reset_at = inner.get("per1MonthResetTime").and_then(num_or_str);
            period = "month";
        } else if let Some(p) = inner.get("per1WeekPercentage").and_then(|v| v.as_f64()) {
            percent_str = Some(format!("{:.1}%", (1.0 - p) * 100.0));
            reset_at = inner.get("per1WeekResetTime").and_then(num_or_str);
            period = "week";
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
        "period": period,
        "resetAt": reset_at,
        "resetAtMs": reset_at,
        "remainingDays": remaining_days,
        "endTime": end_time,
    })
}

/// 数值字段可能是 JSON number 或数字字符串（毫秒时间戳尤其常见），统一转 f64。
pub(crate) fn num_or_str(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
}

/// 用量接口一页的当日花费合计：actual_cost 求和（缺失回退 total_cost）。
fn page_cost(items: &[Value]) -> f64 {
    items
        .iter()
        .map(|it| {
            it.get("actual_cost")
                .filter(|v| !v.is_null())
                .or_else(|| it.get("total_cost").filter(|v| !v.is_null()))
                .and_then(num_or_str)
                .unwrap_or(0.0)
        })
        .sum()
}

/// 梭子蟹「今日」= 服务器按 Asia/Shanghai 判定，硬编码 +8 与看板对齐。
pub(crate) fn suoxie_today() -> String {
    let now = time::OffsetDateTime::now_utc() + time::Duration::hours(8);
    let d = now.date();
    format!("{:04}-{:02}-{:02}", d.year(), d.month() as u8, d.day())
}

pub(crate) fn suoxie_headers(req: reqwest::RequestBuilder, auth: &str) -> reqwest::RequestBuilder {
    req.header("authorization", auth)
        .header("x-user-ui-request", "1")
        .header("referer", "https://suoxie.codes/dashboard")
}

/// 梭子蟹中转站：剩余额度（auth/me data.balance）+ 当日花费（/api/v1/usage 分页累加 actual_cost）。
async fn fetch_suoxie(client: &reqwest::Client, token: &str) -> Result<Value, String> {
    let auth = format!("Bearer {}", token.trim_start_matches("Bearer ").trim());

    let (status, text) = apilog::send(
        "suoxie",
        suoxie_headers(
            client.get(format!(
                "{SUOXIE_BASE}/api/v1/auth/me?timezone=Asia%2FShanghai"
            )),
            &auth,
        )
        .timeout(SUOXIE_TIMEOUT),
    )
    .await?;
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}"));
    }
    let me: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    // 梭子蟹也存在「HTTP 200 + 业务 code」的失败形态（与 DeepSeek 平台一致），
    // 只看状态码会把 TOKEN_EXPIRED 当成结构异常，既不提示也不触发续期。
    if body_code_ok(&me) == Some(false) {
        let code = me.get("code").cloned().unwrap_or(Value::Null);
        let msg = me.get("message").and_then(|v| v.as_str()).unwrap_or("");
        return Err(format!("suoxie code {code}: {msg}"));
    }
    let balance = me
        .pointer("/data/balance")
        .and_then(num_or_str)
        .ok_or("梭子蟹 auth/me 结构异常")?;

    let day = suoxie_today();
    let mut spend = 0.0f64;
    let mut page = 1u32;
    loop {
        let url = format!(
            "{SUOXIE_BASE}/api/v1/usage?start_date={day}&end_date={day}&page={page}&page_size=100&timezone=Asia%2FShanghai"
        );
        let (status, text) = apilog::send(
            "suoxie",
            suoxie_headers(client.get(&url), &auth).timeout(SUOXIE_TIMEOUT),
        )
        .await?;
        if !(200..300).contains(&status) {
            return Err(format!("HTTP {status}"));
        }
        let body: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        if body_code_ok(&body) == Some(false) {
            let code = body.get("code").cloned().unwrap_or(Value::Null);
            let msg = body.get("message").and_then(|v| v.as_str()).unwrap_or("");
            return Err(format!("suoxie code {code}: {msg}"));
        }
        if let Some(items) = body.pointer("/data/items").and_then(|v| v.as_array()) {
            spend += page_cost(items);
        }
        // pages 可能是数字/字符串/浮点，兼容解析，缺失按 1
        let pages = body
            .pointer("/data/pages")
            .and_then(num_or_str)
            .map(|f| f.max(1.0) as u32)
            .unwrap_or(1);
        page += 1;
        // ponytail: 页上限兜底，防分页字段异常死循环
        if page > pages || page > 50 {
            break;
        }
    }

    Ok(json!({
        "ok": true,
        "balance": balance,
        "currency": "CNY",
        "todayUsage": spend,
    }))
}

/// 平台用量接口的登录态失效：HTTP 401/403，或 HTTP 200 + 业务码（实测 40003
/// 「Authorization Failed (invalid token)」）。后者最容易漏——漏了就永远不续期。
fn platform_auth_error(e: &str) -> bool {
    let l = e.to_ascii_lowercase();
    l.contains("http 401")
        || l.contains("http 403")
        || l.contains("40003")
        || l.contains("authorization failed")
        || l.contains("invalid token")
}

/// 业务码是否成功：`0` 或 `"0"` 算成功；返回 None 表示响应里没有 code 字段。
fn body_code_ok(body: &Value) -> Option<bool> {
    let c = body.get("code")?;
    Some(c.as_i64() == Some(0) || c.as_str().map(|s| s.trim() == "0").unwrap_or(false))
}

/// 梭子蟹登录态失效判断：错误串大小写不统一（fetch_suoxie 抛 "HTTP 401"，
/// 平台用量抛 "http 401"），统一转小写再比，别让一个大小写 bug 把自动续期关掉。
fn suoxie_auth_error(e: &str) -> bool {
    let l = e.to_ascii_lowercase();
    l.contains("http 401")
        || l.contains("http 403")
        || l.contains("code 401")
        || l.contains("code 403")
        || l.contains("code \"401\"")
        || l.contains("code \"403\"")
        || l.contains("token_expired")
        || l.contains("invalid_token")
        || l.contains("invalid token")
        || l.contains("unauthorized")
        || l.contains("token has expired")
}

/// 「当前保存的凭据还能不能用」的结论。Unknown 表示网络/接口异常——既不能
/// 宣称有效，也绝不能当成失效去拉浏览器。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CredentialState {
    Valid,
    Invalid,
    Unknown,
}

impl CredentialState {
    pub fn as_str(self) -> &'static str {
        match self {
            CredentialState::Valid => "valid",
            CredentialState::Invalid => "invalid",
            CredentialState::Unknown => "unknown",
        }
    }
}

/// 用真实接口判断已保存凭据的死活——不靠本地猜 JWT，也不靠「这次没抓到新凭据」。
/// 返回 (结论, 可读原因)。
pub async fn probe_credential(state: &AppState, provider: &str) -> (CredentialState, String) {
    let cfg = {
        let _g = state.cfg.lock().unwrap_or_else(|e| e.into_inner());
        config::read(&state.dir)
    };
    match provider {
        "suoxie" => match cfg.suoxie_token.as_deref() {
            Some(token) => probe_suoxie(&state.client, token).await,
            None => (CredentialState::Invalid, "未保存梭子蟹 token".to_string()),
        },
        "bailian" => match (
            cfg.bailian_cookie.as_deref(),
            cfg.bailian_post_data.as_deref(),
        ) {
            (Some(cookie), Some(post)) => {
                match fetch_bailian_subscription(&state.client, cookie, post).await {
                    Ok(body) => {
                        let parsed = parse_bailian_subscription(&body);
                        if parsed["ok"].as_bool() == Some(true) {
                            (CredentialState::Valid, "百炼订阅接口正常".to_string())
                        } else {
                            let msg = parsed["error"].as_str().unwrap_or("接口异常").to_string();
                            if parsed["credentialExpired"].as_bool() == Some(true) {
                                (CredentialState::Invalid, msg)
                            } else {
                                (CredentialState::Unknown, msg)
                            }
                        }
                    }
                    Err(e) => (CredentialState::Unknown, e),
                }
            }
            _ => (CredentialState::Invalid, "未保存百炼凭据".to_string()),
        },
        _ => {
            // 只有「实时·令牌」模式才需要平台令牌；本地记账模式下它不参与任何展示，
            // 不能因为它过期就弹「凭据失效」。
            if config::normalize(&cfg.usage_mode) != "token" {
                return (
                    CredentialState::Valid,
                    "当前不是令牌用量模式，平台令牌未参与取数".to_string(),
                );
            }
            match cfg.platform_token.as_deref() {
                Some(token) => match fetch_today_usage(&state.client, token).await {
                    Ok(_) => (CredentialState::Valid, "平台用量接口正常".to_string()),
                    Err(e) if platform_auth_error(&e) => (CredentialState::Invalid, e),
                    Err(e) => (CredentialState::Unknown, e),
                },
                None => (
                    CredentialState::Invalid,
                    "未保存 DeepSeek 平台令牌".to_string(),
                ),
            }
        }
    }
}

/// 只打 auth/me：最轻量、且正是余额用的那条接口，不会像分页用量那样慢。
async fn probe_suoxie(client: &reqwest::Client, token: &str) -> (CredentialState, String) {
    let auth = format!("Bearer {}", token.trim_start_matches("Bearer ").trim());
    let (status, text) = match apilog::send(
        "suoxie-probe",
        suoxie_headers(
            client.get(format!(
                "{SUOXIE_BASE}/api/v1/auth/me?timezone=Asia%2FShanghai"
            )),
            &auth,
        )
        .timeout(SUOXIE_TIMEOUT),
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return (CredentialState::Unknown, e),
    };
    if status == 401 || status == 403 {
        return (CredentialState::Invalid, format!("HTTP {status}"));
    }
    if !(200..300).contains(&status) {
        return (CredentialState::Unknown, format!("HTTP {status}"));
    }
    let body: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => return (CredentialState::Unknown, format!("响应非 JSON: {e}")),
    };
    if body_code_ok(&body) == Some(false) {
        let code = body.get("code").cloned().unwrap_or(Value::Null);
        let msg = body.get("message").and_then(|v| v.as_str()).unwrap_or("");
        let text = format!("suoxie code {code}: {msg}");
        return if suoxie_auth_error(&text) {
            (CredentialState::Invalid, text)
        } else {
            (CredentialState::Unknown, text)
        };
    }
    if body.pointer("/data/balance").and_then(num_or_str).is_none() {
        return (
            CredentialState::Invalid,
            "auth/me 未返回余额，登录态不可用".to_string(),
        );
    }
    (CredentialState::Valid, "梭子蟹登录态正常".to_string())
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
    // 梭子蟹中转站：有 token 才抓，同样并行跑。整体 40s 超时兜底（单请求 30s，
    // 再给分页留一轮余量），防分页退化时无限拖慢整个 payload（join 在主流程末尾等待）。
    let mut suoxie_task = cfg.suoxie_token.clone().map(|tok| {
        let client = state.client.clone();
        tokio::spawn(async move {
            match tokio::time::timeout(Duration::from_secs(40), fetch_suoxie(&client, &tok)).await {
                Err(_) => json!({ "ok": false, "error": "梭子蟹接口超时" }),
                Ok(Ok(v)) => v,
                Ok(Err(e)) => json!({ "ok": false, "error": format!("接口请求失败: {e}") }),
            }
        })
    });
    let Some(key) = cfg.api_key.clone() else {
        // 已配置百炼/梭子蟹却没配 DeepSeek key：先收尾并行任务，前端仍能展示它们的数据
        return json!({
            "ok": false,
            "code": "NO_KEY",
            "error": "未配置 API Key，请点鲸鱼菜单「设置 API Key」",
            "bailian": join_side(bailian_task.take()).await,
            "suoxie": join_side(suoxie_task.take()).await,
        });
    };

    let (total, currency) = match fetch_balance(&state.client, &key).await {
        Ok(pair) => pair,
        Err(e) => {
            // 瞬时网络/接口抖动：沿用最近成功余额（与 JS transient 行为一致）
            // 先取快照释放锁，再收并行任务（避免 std MutexGuard 跨 await 非 Send）
            let cached = state.cache.lock().unwrap().clone();
            let bailian = join_side(bailian_task.take()).await;
            let suoxie = join_side(suoxie_task.take()).await;
            if let Some(c) = cached {
                let mut v = c.payload.clone();
                v["bailian"] = bailian;
                v["suoxie"] = suoxie;
                v["stale"] = Value::Bool(true);
                v["error"] = json!(e);
                return v;
            }
            return json!({
                "ok": false,
                "code": "HTTP",
                "error": format!("余额接口请求失败: {e}"),
                "bailian": bailian,
                "suoxie": suoxie,
            });
        }
    };

    let mut payload = json!({
        "ok": true,
        "totalBalance": total,
        "currency": currency,
        "isPeak": pricing::is_peak_time(now_secs()),
        "updatedAt": now_iso(),
    });

    // 小鲸鱼记账（本地会话）：opencode.db + Claude Code jsonl 叠加，按计价表算今日金额。
    // 读不到 → None（前端显示 --）。不再有余额差回退。
    fn local_usage(cfg: &config::AppConfig) -> Option<f64> {
        let db = opencode::db_path(cfg.opencode_db.as_deref());
        let providers = cfg
            .usage_providers
            .clone()
            .unwrap_or_else(pricing::default_providers);
        match (
            opencode::today_cost(&db, &providers),
            claude::today_cost(None, &providers),
        ) {
            (Some((a, _)), Some((b, _))) => Some(a + b),
            (Some((a, _)), None) => Some(a),
            (None, Some((b, _))) => Some(b),
            (None, None) => None,
        }
    }

    // 实时·令牌（默认）：有令牌且抓取成功才用实时值；无令牌/失败 → 回退小鲸鱼记账。
    let mut usage_credential_expired = false;
    let realtime = if config::normalize(&cfg.usage_mode) == "token" {
        match cfg.platform_token.as_deref() {
            Some(token) => match fetch_today_usage(&state.client, token).await {
                Ok(cost) => {
                    payload["todayUsage"] = json!(cost);
                    payload["usageMode"] = json!("token");
                    true
                }
                Err(e) => {
                    eprintln!("[balance] 令牌用量抓取失败，回退小鲸鱼记账: {e}");
                    usage_credential_expired = platform_auth_error(&e);
                    false
                }
            },
            None => false,
        }
    } else {
        false
    };
    if !realtime {
        // local_usage 做阻塞式 SQLite/文件读取（opencode.db + Claude jsonl），挪出 async 执行器
        let local_cfg = cfg.clone();
        match tokio::task::spawn_blocking(move || local_usage(&local_cfg)).await {
            Ok(Some(cost)) => {
                payload["todayUsage"] = json!(cost);
                payload["usageMode"] = json!("opencode");
            }
            Ok(None) => {}
            Err(e) => eprintln!("[balance] 本地记账任务失败: {e}"),
        }
    }

    // 百炼 TokenPlan（订阅制）与梭子蟹（中转站），收并发任务的尾。
    payload["bailian"] = join_side(bailian_task.take()).await;
    payload["suoxie"] = join_side(suoxie_task.take()).await;
    let (credential_refresh_needed, credential_scope) = match cfg.provider.as_str() {
        // DeepSeek 余额靠 api_key，平台令牌只影响「实时用量」。把两者混成一句
        // 「凭据失效」正是误报的来源，所以这里单独标成 usage。
        "deepseek" => (usage_credential_expired, "usage"),
        "bailian" => (
            payload["bailian"]["credentialExpired"].as_bool() == Some(true),
            "login",
        ),
        "suoxie" => (
            payload["suoxie"]["error"]
                .as_str()
                .is_some_and(suoxie_auth_error),
            "login",
        ),
        _ => (false, "login"),
    };
    payload["credentialRefreshNeeded"] = json!(credential_refresh_needed);
    payload["credentialRefreshScope"] = json!(credential_scope);
    if credential_refresh_needed {
        apilog::note(
            "credential",
            &format!(
                "provider={} scope={} 判定需要更新：{}",
                cfg.provider,
                credential_scope,
                payload["suoxie"]["error"]
                    .as_str()
                    .or_else(|| payload["bailian"]["error"].as_str())
                    .unwrap_or("平台用量令牌失效")
            ),
        );
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
            credential_retry_at: std::sync::Mutex::new(None),
            credential_needs_user: std::sync::atomic::AtomicBool::new(false),
            credential_busy: std::sync::atomic::AtomicBool::new(false),
            cfg: std::sync::Mutex::new(()),
            check_update: std::sync::atomic::AtomicBool::new(false),
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
            credential_retry_at: std::sync::Mutex::new(None),
            credential_needs_user: std::sync::atomic::AtomicBool::new(false),
            credential_busy: std::sync::atomic::AtomicBool::new(false),
            cfg: std::sync::Mutex::new(()),
            check_update: std::sync::atomic::AtomicBool::new(false),
        };
        assert!(cached_payload(&expired).is_none());
    }

    #[test]
    fn platform_auth_error_detects_body_level_401() {
        // 实测：无效平台令牌 → HTTP 200 + {"code":40003,"msg":"Authorization Failed (invalid token)"}
        assert!(platform_auth_error(
            "platform code 40003: Authorization Failed (invalid token)"
        ));
        assert!(platform_auth_error("http 401"));
        assert!(platform_auth_error("HTTP 403"));
        assert!(!platform_auth_error("no usage"));
        assert!(!platform_auth_error("platform code 50000: internal error"));
        assert!(!platform_auth_error("余额接口请求失败: 超时"));
    }

    #[test]
    fn suoxie_auth_error_is_case_insensitive() {
        assert!(suoxie_auth_error("接口请求失败: HTTP 401"));
        assert!(suoxie_auth_error("接口请求失败: http 401"));
        assert!(suoxie_auth_error("HTTP 403"));
        assert!(suoxie_auth_error("TOKEN_EXPIRED"));
        assert!(suoxie_auth_error(
            "suoxie code \"TOKEN_EXPIRED\": Token has expired"
        ));
        assert!(suoxie_auth_error("suoxie code 401: unauthorized"));
        assert!(!suoxie_auth_error("梭子蟹接口超时"));
        assert!(!suoxie_auth_error("接口请求失败: HTTP 500"));
    }

    #[test]
    fn body_code_ok_handles_numbers_and_strings() {
        assert_eq!(body_code_ok(&json!({"code": 0})), Some(true));
        assert_eq!(body_code_ok(&json!({"code": "0"})), Some(true));
        assert_eq!(body_code_ok(&json!({"code": 40003})), Some(false));
        assert_eq!(body_code_ok(&json!({"code": "TOKEN_EXPIRED"})), Some(false));
        assert_eq!(body_code_ok(&json!({"data": {}})), None);
    }

    #[test]
    fn suoxie_page_cost_sums_actual_cost() {
        // 实测样本两笔：0.000725 + 0.00096；缺 actual_cost 的回退 total_cost；数字字符串兼容
        let items: Vec<Value> = serde_json::from_str(
            r#"[{"actual_cost":0.000725,"total_cost":0.000725},
                {"actual_cost":null,"total_cost":0.00096},
                {"actual_cost":"0.5"}]"#,
        )
        .unwrap();
        assert!(
            (page_cost(&items) - 0.501685).abs() < 1e-9,
            "got {}",
            page_cost(&items)
        );
        assert_eq!(page_cost(&[]), 0.0);
        // 今日日期串（+8）：YYYY-MM-DD
        let d = suoxie_today();
        assert_eq!(d.len(), 10);
        assert_eq!(&d[4..5], "-");
        assert_eq!(&d[7..8], "-");
    }

    #[test]
    fn bailian_sample_shape_parsed() {
        // 实测用量接口响应：per1WeekPercentage + per1WeekResetTime(ms)
        let body: Value = serde_json::from_str(
            r#"{
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
        }"#,
        )
        .unwrap();
        let out = parse_bailian_subscription(&body);
        assert_eq!(out["ok"], true);
        assert_eq!(out["remaining"].as_str().unwrap(), "80.8%");
        assert_eq!(out["resetAtMs"], 1787728620000.0);
    }

    #[test]
    fn bailian_sample_subscription_shape() {
        // 订阅接口：remainingDays / endTime 也应被提取
        let body: Value = serde_json::from_str(
            r#"{
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
        }"#,
        )
        .unwrap();
        let out = parse_bailian_subscription(&body);
        assert_eq!(out["ok"], true);
        assert_eq!(out["remainingDays"], 70.0);
        assert_eq!(out["endTime"], 1793462400000.0);
    }

    #[test]
    fn bailian_month_usage_shape_parsed() {
        let out = parse_bailian_subscription(&json!({
            "data": {
                "DataV2": { "data": { "data": {
                    "per1MonthPercentage": 1.0,
                    "per1MonthResetTime": 1793462400000i64
                }, "success": true } },
                "success": true
            },
            "successResponse": true
        }));
        assert_eq!(out["ok"], true);
        assert_eq!(out["remaining"], "0.0%");
        assert_eq!(out["period"], "month");
        assert_eq!(out["resetAtMs"], 1793462400000.0);
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
        assert_eq!(out["credentialExpired"], true);
        assert!(out["error"].as_str().unwrap().contains("NotLogined"));
    }

    #[test]
    fn bailian_request_uses_empty_version_parameter() {
        let url = format!(
            "{BAILIAN_API_URL}?action=BroadScopeAspnGateway&product=sfm_bailian&api=zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/usage&_v="
        );
        assert!(url.ends_with("&_v="));
        assert!(!url.ends_with("&_v=undefined"));
    }
}
