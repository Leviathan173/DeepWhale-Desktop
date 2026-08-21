//! DeepSeek 价目换算：从 OLD lib/index.js 直译。
//! 高峰时段：每日 9:00–12:00 和 14:00–18:00（北京时间）。
use serde_json::Value;

pub const PEAK_HOURS: [[i32; 2]; 2] = [[9, 12], [14, 18]];

/// CNY 每百万 token 价格：[空闲时段价, 高峰时段价]
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
}
