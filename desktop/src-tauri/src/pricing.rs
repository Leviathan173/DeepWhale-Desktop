//! DeepSeek 价目换算：从 OLD lib/index.js 直译。
//! 高峰时段：每日 9:00–12:00 和 14:00–18:00（北京时间）。
//! ponytail: 价格表按官方价目硬编码（2025 年），改价需同步更新；若定价漂移会导致
//! token 实时用量模式的费用/余额换算不准。记账模式不受影响。
use serde_json::Value;

pub const PEAK_HOURS: [[i32; 2]; 2] = [[9, 12], [14, 18]];

/// CNY 每百万 token 价格：[空闲时段价, 高峰时段价]
/// （deepseek-chat/v4-flash/v4-pro/reasoner 目前共享；分模型定价后再拆。）
pub struct Prices {
    pub hit: [f64; 2],
    pub miss: [f64; 2],
    pub out: [f64; 2],
}

const BASE_PRICE: Prices = Prices {
    hit: [0.05, 0.1],
    miss: [1.5, 3.0],
    out: [4.5, 9.0],
};

const MODELS: [&str; 4] = [
    "deepseek-chat",
    "deepseek-reasoner",
    "deepseek-v4-flash",
    "deepseek-v4-pro",
];

pub fn price_for(model: &str) -> &'static Prices {
    let m = model.to_ascii_lowercase();
    for key in MODELS {
        if m.contains(key) {
            return &BASE_PRICE;
        }
    }
    &BASE_PRICE
}

/// Anthropic 官方模型 USD 单价（每百万 token）。Claude Code 走官方直连时用；
/// 当前配置是路由到百炼代理，这些一般不出现。hit= cache read, miss=未命中输入。
/// ponytail: 汇率与价目硬编码，漂移需手动同步。
pub const USD_TO_CNY: f64 = 7.2;

pub struct UsdPrices {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
}

macro_rules! usd_prices {
    ($input:expr, $output:expr, $cache_read:expr) => {
        UsdPrices {
            input: $input,
            output: $output,
            cache_read: $cache_read,
        }
    };
}

pub fn claude_usd(model: &str) -> Option<&'static UsdPrices> {
    let m = model.to_ascii_lowercase();
    if m.contains("opus") {
        Some(&usd_prices![15.0, 75.0, 1.5])
    } else if m.contains("haiku") {
        if m.contains("3-5") {
            Some(&usd_prices![0.8, 4.0, 0.1])
        } else {
            Some(&usd_prices![1.0, 5.0, 0.1])
        }
    } else if m.contains("sonnet") {
        Some(&usd_prices![3.0, 15.0, 0.3])
    } else {
        None
    }
}

/// 阿里云百炼 qwen 模型的 CNY 单价（每百万 token），qwen*=按 tier 匹配；
/// preview 模型按同代 max 档计价。
/// ponytail: 按 2025–2026 公开价目，漂移需手动同步。
pub struct CnyPrices {
    pub hit: f64,
    pub miss: f64,
    pub out: f64,
}

pub fn qwen_cny(model: &str) -> Option<&'static CnyPrices> {
    let m = model.to_ascii_lowercase();
    if !m.contains("qwen") {
        return None;
    }
    // preview 模型按同代 max 档计价（见函数注释）
    if m.contains("max") || m.contains("preview") {
        Some(&CnyPrices {
            hit: 0.405,
            miss: 4.05,
            out: 9.45,
        })
    } else if m.contains("plus") {
        Some(&CnyPrices {
            hit: 0.15,
            miss: 1.5,
            out: 4.5,
        })
    } else {
        Some(&CnyPrices {
            hit: 0.05,
            miss: 0.5,
            out: 2.0,
        })
    }
}

/// bucket time 为 epoch 秒；换算北京时间小时判断峰谷。
pub fn is_peak_time(time_sec: i64) -> bool {
    let hour = time::OffsetDateTime::from_unix_timestamp(time_sec)
        .map(|t| i32::from((t + time::Duration::hours(8)).hour()))
        .unwrap_or(0);
    for [start, end] in PEAK_HOURS {
        if hour >= start && hour < end {
            return true;
        }
    }
    false
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

fn token_of(usage: &Value, key: &str) -> f64 {
    num(usage.get(key).unwrap_or(&Value::Null))
}

/// 本地记账的单位价（CNY/百万 token，[空闲, 高峰] 两档）。
#[derive(Debug, Clone, Copy)]
pub struct UnitPrice {
    pub hit: [f64; 2],
    pub miss: [f64; 2],
    pub out: [f64; 2],
    /// 缓存创建/写入（cache.creation）。内置价目暂无独立档 → 沿用 miss。
    pub create: [f64; 2],
    /// 该单是否参与峰谷计价（false 时调用方一律取 0 档即空闲价）。
    pub peak: bool,
}

/// 内置默认供应商表，反映常见事实：
/// - bailian / tokenplan（百炼套餐制）→ 按量=false，不计入今日金额；
/// - deepseek 官方、anthropic 官方 → 按量=true，用内置价目。
pub fn default_providers() -> Vec<crate::config::ProviderCfg> {
    use crate::config::{ModelPriceCfg, ProviderCfg};
    vec![
        ProviderCfg {
            name: "bailian".into(),
            metric: false,
            peak: false,
            models: vec![
                ModelPriceCfg {
                    pattern: "deepseek".into(),
                    ..Default::default()
                },
                ModelPriceCfg {
                    pattern: "longcat".into(),
                    ..Default::default()
                },
                ModelPriceCfg {
                    pattern: "qwen".into(),
                    ..Default::default()
                },
            ],
        },
        ProviderCfg {
            name: "tokenplan".into(),
            metric: false,
            peak: false,
            models: vec![],
        },
        ProviderCfg {
            name: "deepseek".into(),
            metric: true,
            peak: true,
            models: vec![],
        },
        ProviderCfg {
            name: "claude".into(),
            metric: false,
            peak: false,
            models: vec![],
        },
    ]
}

/// 内置价目（deepseek 官方 / claude / qwen），按价对象为单价表；未知返回 None。
/// deepseek 官方峰谷两档；claude/qwen 用平档（不峰谷）。
pub fn builtin_unit(model: &str) -> Option<UnitPrice> {
    let m = model.to_ascii_lowercase();
    if m.contains("deepseek") {
        let p = price_for(model);
        Some(UnitPrice {
            hit: p.hit,
            miss: p.miss,
            out: p.out,
            create: p.miss,
            peak: true,
        })
    } else if m.contains("claude") {
        claude_usd(model).map(|p| UnitPrice {
            hit: [p.cache_read * USD_TO_CNY; 2],
            miss: [(p.input) * USD_TO_CNY; 2],
            out: [p.output * USD_TO_CNY; 2],
            create: [(p.input) * USD_TO_CNY; 2],
            peak: false,
        })
    } else if m.contains("qwen") {
        qwen_cny(model).map(|p| UnitPrice {
            hit: [p.hit; 2],
            miss: [p.miss; 2],
            out: [p.out; 2],
            create: [p.miss; 2],
            peak: false,
        })
    } else {
        None
    }
}

const ZERO_UNIT: UnitPrice = UnitPrice {
    hit: [0.0; 2],
    miss: [0.0; 2],
    out: [0.0; 2],
    create: [0.0; 2],
    peak: false,
};

/// 模型名精确匹配（忽略大小写）。不用子串：pattern 即模型全名，避免泛化误归。
fn model_matches(patterns: &[crate::config::ModelPriceCfg], model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    patterns.iter().any(|c| m == c.pattern.to_ascii_lowercase())
}

/// 解析一笔本地用量应套用的单价。
/// 返回 None = 套餐/订阅（非按量计费）或未配置模型 → 不计入今日金额。
/// provider 为 opencode 的 providerID（无则 None，如 claude jsonl）。
pub fn resolve_unit(
    providers: &[crate::config::ProviderCfg],
    model: &str,
    provider: Option<&str>,
) -> Option<UnitPrice> {
    use crate::config::ProviderCfg;
    let clamp_peak = |u: UnitPrice, peak: bool| UnitPrice {
        peak: u.peak && peak,
        ..u
    };
    // 1) 有 providerID：按供应商名精确匹配（opencode 数据）
    if let Some(pid) = provider {
        if let Some(p) = providers.iter().find(|p| p.name.eq_ignore_ascii_case(pid)) {
            if !p.metric {
                return None;
            }
            // 供应商已匹配但模型无内置价：0 元仍统计（与 claude jsonl 缺失
            // providerID 时的行为不同，但那属数据源差异，这里保留统计语义）
            let u = model_unit(p, model)
                .or_else(|| builtin_unit(model))
                .unwrap_or(ZERO_UNIT);
            return Some(clamp_peak(u, p.peak));
        }
    }
    // 2) 无 providerID：全表按模型 pattern 匹配（claude jsonl 数据）
    if let Some(p) = providers
        .iter()
        .find(|p: &&ProviderCfg| model_matches(&p.models, model))
    {
        if !p.metric {
            return None;
        }
        let u = model_unit(p, model)
            .or_else(|| builtin_unit(model))
            .unwrap_or(ZERO_UNIT);
        return Some(clamp_peak(u, p.peak));
    }
    // 3) 兜底：deepseek/claude/qwen 系仍按内置价目计（按量）；其余不计。
    builtin_unit(model)
}

fn model_unit(p: &crate::config::ProviderCfg, model: &str) -> Option<UnitPrice> {
    let m = model.to_ascii_lowercase();
    let c = p
        .models
        .iter()
        .find(|c| m == c.pattern.to_ascii_lowercase())?;
    if !c.priced() {
        return None;
    }
    // 每个类别两组价：空闲=用户空闲值或内置空闲档；高峰=用户高峰值，未填时
    // 峰谷供应商按空闲×2（老行为），非峰谷供应商与空闲同价；全空回退内置两档。
    let base = builtin_unit(model).unwrap_or(ZERO_UNIT);
    let tier = |off: Option<f64>, peak: Option<f64>, b: [f64; 2]| {
        let offv = off.unwrap_or(b[0]);
        let peakv = match peak {
            Some(x) => x,
            None if p.peak => off.map_or(b[1], |x| x * 2.0),
            None => offv,
        };
        [offv, peakv]
    };
    Some(UnitPrice {
        hit: tier(c.cache_read, c.peak_cache_read, base.hit),
        miss: tier(c.input, c.peak_input, base.miss),
        out: tier(c.output, c.peak_output, base.out),
        create: tier(c.cache_creation, c.peak_cache_creation, base.create),
        peak: p.peak,
    })
}

/// 平台用量响应 → (今日费用, 今日token数)。结构与 OLD JS computeTodayUsage 一致。
pub fn compute_today_usage(data: &Value) -> Option<(f64, f64)> {
    let mut d = data;
    if let Some(inner) = data.get("data") {
        if let Some(biz) = inner.get("biz_data") {
            if biz.get("series").is_some() {
                d = biz;
            }
        } else if inner.get("series").is_some() {
            d = inner;
        }
    }
    let series = d.get("series")?.as_array()?;
    if series.is_empty() {
        return None;
    }
    let mut cost = 0.0;
    let mut tokens = 0.0;
    let mut found = false;
    for s in series {
        let model = s.get("model").and_then(|v| v.as_str()).unwrap_or("");
        let p = price_for(model);
        let buckets = s.get("buckets").and_then(|v| v.as_array());
        let Some(buckets) = buckets else { continue };
        for b in buckets {
            let Some(usage) = b.get("usage") else {
                continue;
            };
            let hit = token_of(usage, "PROMPT_CACHE_HIT_TOKEN");
            let miss = token_of(usage, "PROMPT_CACHE_MISS_TOKEN");
            let out = token_of(usage, "RESPONSE_TOKEN");
            if hit + miss + out == 0.0 {
                continue;
            }
            found = true;
            tokens += hit + miss + out;
            let pi = usize::from(is_peak_time(
                b.get("time").and_then(|v| v.as_i64()).unwrap_or(0),
            ));
            cost += (hit / 1e6) * p.hit[pi] + (miss / 1e6) * p.miss[pi] + (out / 1e6) * p.out[pi];
        }
    }
    if found {
        Some((cost, tokens))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// UTC 时刻（2026-08-22）→ epoch。北京 = UTC + 8h。
    fn ts(h: u8, m: u8) -> i64 {
        time::OffsetDateTime::new_utc(
            time::Date::from_calendar_date(2026, time::Month::August, 22).unwrap(),
            time::Time::from_hms(h, m, 0).unwrap(),
        )
        .unix_timestamp()
    }

    #[test]
    fn beijing_hour_boundaries() {
        assert!(!is_peak_time(ts(0, 59))); // 北京 8:59
        assert!(is_peak_time(ts(1, 0))); // 北京 9:00 高峰
        assert!(is_peak_time(ts(3, 59))); // 北京 11:59 高峰
        assert!(!is_peak_time(ts(4, 0))); // 北京 12:00
        assert!(is_peak_time(ts(6, 0))); // 北京 14:00 高峰
        assert!(is_peak_time(ts(9, 59))); // 北京 17:59 高峰
        assert!(!is_peak_time(ts(10, 0))); // 北京 18:00
    }

    #[test]
    fn compute_usage_off_peak() {
        let json = serde_json::json!({
            "data": {
                "biz_data": {
                    "series": [{
                        "model": "deepseek-chat",
                        "buckets": [{
                            "time": ts(15, 0), // 北京 23:00，空闲
                            "usage": {
                                "PROMPT_CACHE_HIT_TOKEN": 1000000,
                                "PROMPT_CACHE_MISS_TOKEN": 0,
                                "RESPONSE_TOKEN": 0
                            }
                        }]
                    }]
                }
            }
        });
        let (cost, tokens) = compute_today_usage(&json).unwrap();
        // 空闲 0.05/百万 → 1M hit = 0.05 元
        assert!((cost - 0.05).abs() < 1e-9);
        assert!((tokens - 1_000_000.0).abs() < 1.0);
    }

    #[test]
    fn resolve_provider_flag() {
        use crate::config::{ModelPriceCfg, ProviderCfg};
        let plan = vec![ProviderCfg {
            name: "bailian".into(),
            metric: false, // 套餐制
            peak: false,
            models: vec![ModelPriceCfg {
                pattern: "deepseek".into(),
                ..Default::default()
            }],
        }];
        // 按供应商名精确匹配 → 套餐不计（None）。
        assert!(resolve_unit(&plan, "deepseek-v4-flash-0731", Some("bailian")).is_none());
        // 无 providerID 且泛化 pattern "deepseek" 不再精确匹配 "deepseek-v4-flash-0731"
        // → 落到内置兜底（而非被套餐排除）。
        let u = resolve_unit(&plan, "deepseek-v4-flash-0731", None).unwrap();
        assert!((u.miss[0] - 1.5).abs() < 1e-9);

        let metric = vec![ProviderCfg {
            name: "longcat".into(),
            metric: true,
            peak: true,
            models: vec![ModelPriceCfg {
                pattern: "longcat-2.0".into(),
                input: Some(0.5),
                output: Some(0.5),
                cache_read: Some(0.5),
                cache_creation: Some(0.5),
                ..Default::default()
            }],
        }];
        // 精确全名才命中；泛化片段 "longcat" 不命中。
        let u = resolve_unit(&metric, "longcat-2.0", None).unwrap();
        assert!((u.miss[0] - 0.5).abs() < 1e-9);
        assert!((u.create[1] - 1.0).abs() < 1e-9); // 峰谷 ×2
        assert!(resolve_unit(&metric, "longcat-2.0-extra", None).is_none());

        // 高峰显式填价：miss=[0.3,0.8]，其余类别回退内置。
        let metric = vec![ProviderCfg {
            name: "longcat".into(),
            metric: true,
            peak: true,
            models: vec![ModelPriceCfg {
                pattern: "deepseek-v4-flash".into(),
                input: Some(0.3),
                peak_input: Some(0.8),
                ..Default::default()
            }],
        }];
        let u = resolve_unit(&metric, "deepseek-v4-flash", None).unwrap();
        assert!((u.miss[0] - 0.3).abs() < 1e-9);
        assert!((u.miss[1] - 0.8).abs() < 1e-9);
        // output 未填 → 内置高峰档
        assert!((u.out[1] - 9.0).abs() < 1e-9);

        // 只填 input 时，其余类别回退内置价目。
        let metric = vec![ProviderCfg {
            name: "longcat".into(),
            metric: true,
            peak: false,
            models: vec![ModelPriceCfg {
                pattern: "longcat-2.0".into(),
                input: Some(2.0),
                ..Default::default()
            }],
        }];
        let u = resolve_unit(&metric, "longcat-2.0", None).unwrap();
        assert!((u.miss[0] - 2.0).abs() < 1e-9);
        assert!((u.out[0] - 0.0).abs() < 1e-9); // longcat 无内置 → 该项 0

        // 未配置的 deepseek 兜底走内置价目。
        let u = resolve_unit(&[], "deepseek-v4-flash", None).unwrap();
        assert!((u.miss[0] - 1.5).abs() < 1e-9);
        // 完全未知模型不计。
        assert!(resolve_unit(&[], "gpt-4o-x", None).is_none());
    }
}
