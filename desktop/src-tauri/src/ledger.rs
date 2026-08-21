//! 小鲸鱼记账：从 OLD lib/index.js 直译。
//! 余额差值记账，跨天自动归零并归档历史。
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use time::macros::format_description;
use time::OffsetDateTime;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Ledger {
    pub date: String,
    pub last_balance: Option<f64>,
    pub today_usage: f64,
    pub history: BTreeMap<String, f64>,
}

fn local_now() -> OffsetDateTime {
    OffsetDateTime::now_local().unwrap_or(OffsetDateTime::now_utc())
}

pub fn today_key() -> String {
    let now = local_now();
    let fmt = format_description!("[year]-[month]-[day]");
    now.format(&fmt).unwrap_or_default()
}

fn file_path(dir: &Path) -> std::path::PathBuf {
    dir.join("usage.json")
}

fn read(dir: &Path, today: &str) -> Ledger {
    let mut led: Ledger = fs::read(dir.join("usage.json"))
        .ok()
        .and_then(|s| serde_json::from_slice(&s).ok())
        .unwrap_or_default();
    if led.date.is_empty() {
        led.date = today.to_string();
    }
    if led.history.len() > 500 {
        led.history.clear();
    }
    led
}

fn write(dir: &Path, led: &Ledger) {
    if let Ok(s) = serde_json::to_string_pretty(led) {
        let _ = fs::write(file_path(dir), s);
    }
}

/// 每次观测余额后调用：按正差值累计当天用量（跨天归零并归档）。
pub fn record_usage(dir: &Path, current_balance: f64) -> Ledger {
    let t = today_key();
    let mut led = read(dir, &t);
    if led.date != t {
        if !led.date.is_empty() && led.today_usage > 0.0 {
            led.history.insert(led.date.clone(), led.today_usage);
        }
        let mut keys: Vec<String> = led.history.keys().cloned().collect();
        keys.sort();
        while keys.len() > 30 {
            if let Some(k) = keys.first() {
                led.history.remove(k);
            }
            keys.remove(0);
        }
        led.date = t;
        led.last_balance = Some(current_balance);
        led.today_usage = 0.0;
    } else {
        let prev = led.last_balance.unwrap_or(current_balance);
        if current_balance < prev {
            led.today_usage += prev - current_balance;
        }
        led.last_balance = Some(current_balance);
    }
    write(dir, &led);
    led
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_today() {
        let n = today_key();
        assert_eq!(n.len(), 10);
        assert!(n.chars().all(|c| c.is_ascii_digit() || c == '-'));
    }

    #[test]
    fn accumulate_positive_diff() {
        let dir = std::env::temp_dir().join("dshw-ledger-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let now = today_key();
        // 注入一个同一天的账本，last_balance=100
        fs::write(
            dir.join("usage.json"),
            serde_json::to_string_pretty(&Ledger {
                date: now.clone(),
                last_balance: Some(100.0),
                today_usage: 0.0,
                history: BTreeMap::new(),
            })
            .unwrap(),
        )
        .unwrap();

        let l1 = record_usage(&dir, 90.0);
        assert!((l1.today_usage - 10.0).abs() < 1e-9);

        let l2 = record_usage(&dir, 95.0); // 余额回升不扣账
        assert!((l2.today_usage - 10.0).abs() < 1e-9);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn archives_on_day_change() {
        let dir = std::env::temp_dir().join("dshw-ledger-test2");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        // 昨天记了 30
        fs::write(
            dir.join("usage.json"),
            serde_json::to_string_pretty(&Ledger {
                date: "2026-08-21".to_string(),
                last_balance: Some(100.0),
                today_usage: 30.0,
                history: BTreeMap::new(),
            })
            .unwrap(),
        )
        .unwrap();

        let l = record_usage(&dir, 95.0);
        assert_eq!(l.date, today_key());
        assert!((l.today_usage - 0.0).abs() < 1e-9);
        assert!((l.history.get("2026-08-21").copied().unwrap_or(0.0) - 30.0).abs() < 1e-9);

        let _ = fs::remove_dir_all(&dir);
    }
}
