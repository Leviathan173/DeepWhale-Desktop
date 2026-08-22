//! opencode 本地记账：读 opencode 自己记录的 SQLite 数据库（每次对话/每段消息的
//! token 用量），按 pricing.rs 价目重算今天全部会话的「今日已用」。
//! ponytail: 依赖 opencode message.data 的固定布局（tokens.cache.read / input /
//! output / reasoning），opencode 若改版要同步跟进。
use rusqlite::Connection;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use time::PrimitiveDateTime;

use crate::config::ProviderCfg;
use crate::pricing::{is_peak_time, resolve_unit};

/// opencode.db 定位：配置 `opencode_db` > 环境变量 OPENCODE_DATA_DIR > 默认路径。
pub fn db_path(configured: Option<&str>) -> PathBuf {
    if let Some(p) = configured {
        let p = p.trim();
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    if let Ok(env) = std::env::var("OPENCODE_DATA_DIR") {
        if !env.is_empty() {
            return PathBuf::from(env).join("opencode.db");
        }
    }
    let mut base = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default();
    if base.as_os_str().is_empty() {
        base = PathBuf::from(".");
    }
    base.push(".local/share/opencode/opencode.db");
    base
}

/// 本地今天 0 点对应的 epoch 毫秒，作为查询下界（claude.rs 也用）。
pub(crate) fn today_start_ms() -> i64 {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    // 本地墙钟零点：必须带上本地时区偏移取 epoch，assume_utc 会按 UTC 解释导致窗口差 8h。
    let offset = now.offset();
    PrimitiveDateTime::new(now.date(), time::Time::MIDNIGHT)
        .assume_offset(offset)
        .unix_timestamp()
        * 1000
}

/// opencode 数据目录对偶：auth.json 所在目录（db_path 的上级）。
fn data_dir() -> PathBuf {
    let mut p = db_path(None);
    p.pop(); // opencode.db → opencode dir
    p
}

/// 自动发现 opencode 里已保存的各供应商 API Key：
/// auth.json（{provider: {type, key}}）+ 配置文件 provider.options.apiKey。
/// 返回去重的 (provider 小写, key)。损坏/缺失文件跳过错。
pub fn discover_api_keys() -> Vec<(String, String)> {
    let mut out = std::collections::BTreeMap::new();

    // 1) auth.json
    let auth = data_dir().join("auth.json");
    if let Ok(s) = fs::read_to_string(&auth) {
        if let Ok(v) = serde_json::from_str::<Value>(&s) {
            if let Some(map) = v.as_object() {
                for (prov, val) in map {
                    let key = val
                        .get("key")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .trim();
                    if !key.is_empty() {
                        out.entry(prov.to_ascii_lowercase()).or_insert_with(|| key.to_string());
                    }
                }
            }
        }
    }

    // 2) opencode.json 配置文件里 provider 显式 apiKey
    let mut cfg_dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let mut b = std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(PathBuf::from)
                .unwrap_or_default();
            b.push(".config");
            b
        });
    cfg_dir.push("opencode/opencode.json");
    if let Ok(s) = fs::read_to_string(&cfg_dir) {
        if let Ok(v) = serde_json::from_str::<Value>(&s) {
            if let Some(provs) = v.get("provider").and_then(|x| x.as_object()) {
                for (prov, conf) in provs {
                    if let Some(key) = conf
                        .get("options")
                        .and_then(|o| o.get("apiKey"))
                        .and_then(|x| x.as_str())
                    {
                        let key = key.trim();
                        if !key.is_empty() {
                            out.entry(prov.to_ascii_lowercase()).or_insert_with(|| key.to_string());
                        }
                    }
                }
            }
        }
    }
    out.into_iter().collect()
}

/// 自动发现：所有消息里去重的 (providerID, modelID) 小写对。
/// 用于「自动获取提供商与计费」按钮。
pub fn discover_models(db: &Path) -> Vec<(String, String)> {
    let mut set = std::collections::BTreeSet::new();
    if let Ok(conn) = Connection::open_with_flags(
        db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        if let Ok(mut stmt) = conn.prepare("SELECT data FROM message") {
            if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) {
                for row in rows.flatten() {
                    let Ok(v) = serde_json::from_str::<Value>(&row) else {
                        continue;
                    };
                    let pid = v
                        .get("providerID")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    let mid = v
                        .get("modelID")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    if !pid.is_empty() && !mid.is_empty() {
                        set.insert((pid, mid));
                    }
                }
            }
        }
    }
    set.into_iter().collect()
}

/// 累加「今天」所有消息的换算费用（CNY）与总 token 数。
/// 读失败 / 没有数据返回 None，上层回退到余额差值记账。
pub fn today_cost(db: &Path, providers: &[ProviderCfg]) -> Option<(f64, f64)> {
    let conn = Connection::open_with_flags(
        db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    let mut stmt = conn
        .prepare("SELECT time_created, data FROM message WHERE time_created >= ?1")
        .ok()?;
    let since = today_start_ms();
    let rows = stmt
        .query_map([since], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
        .ok()?;

    let mut cost = 0.0;
    let mut total_tokens = 0.0;
    let mut found = false;
    for row in rows {
        let Ok((ts, data)) = row else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&data) else {
            continue;
        };
        let Some(tokens) = v.get("tokens") else { continue };
        let raw = |k: &str| tokens.get(k).and_then(|x| x.as_i64()).unwrap_or(0);
        let cached_read = tokens
            .get("cache")
            .and_then(|c| c.get("read"))
            .and_then(|x| x.as_i64())
            .unwrap_or(0);
        let cache_created = tokens
            .get("cache")
            .and_then(|c| {
                c.get("creation").or_else(|| c.get("write"))
            })
            .and_then(|x| x.as_i64())
            .unwrap_or(0);
        let input = raw("input");
        let output = raw("output");
        let reasoning = raw("reasoning");
        // 实测 opencode 的 total = input + output + reasoning + cache.read，
        // reliability：cache read 命中时 input 只记缺水部分。
        let n = (cached_read + cache_created + input + output + reasoning) as f64;
        if n == 0.0 {
            continue;
        }
        // 缺 modelID 的消息无法确定归属供应商/定价，跳过而非默认按某个模型计价
        let Some(model) = v.get("modelID").and_then(|x| x.as_str()) else {
            continue;
        };
        let model = model.to_string();
        let provider = v.get("providerID").and_then(|x| x.as_str());
        let Some(u) = resolve_unit(providers, &model, provider) else {
            continue; // 套餐/未配置供应商：不计金额
        };
        found = true;
        let pi = if u.peak { usize::from(is_peak_time(ts / 1000)) } else { 0 };
        total_tokens += n;
        cost += (cached_read as f64) / 1e6 * u.hit[pi]
            + (cache_created as f64) / 1e6 * u.create[pi]
            + (input as f64) / 1e6 * u.miss[pi]
            + (output + reasoning) as f64 / 1e6 * u.out[pi];
    }
    if found {
        Some((cost, total_tokens))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(path: &Path) -> Connection {
        let c = Connection::open(path).expect("open fixture");
        c.execute_batch(
            "CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, \
             time_updated INTEGER, data TEXT);",
        )
        .expect("create table");
        c
    }

    fn msg(model: &str, read: i64, input: i64, output: i64, reasoning: i64) -> String {
        serde_json::json!({
            "role": "assistant",
            "modelID": model,
            "tokens": {
                "cache": {"read": read, "write": 0},
                "input": input, "output": output, "reasoning": reasoning,
                "total": read + input + output + reasoning
            }
        })
        .to_string()
    }

    fn insert(c: &Connection, id: &str, ts: i64, data: &str) {
        c.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data) \
             VALUES (?1, 's', ?2, ?2, ?3)",
            rusqlite::params![id, ts, data],
        )
        .unwrap();
    }

    #[test]
    fn sums_today_only_and_skips_other_models() {
        let dir = std::env::temp_dir().join("dshw-opencode-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("opencode.db");
        let c = fixture(&db);
        let now = today_start_ms();
        let p = crate::pricing::price_for("deepseek-v4-flash-0731");
        let pi = usize::from(is_peak_time(now / 1000));
        // 只有 deepseek 官方按量供应商：无 providerID 的 deepseek 模型走内置价目。
        let providers = vec![crate::config::ProviderCfg {
            name: "deepseek".into(),
            metric: true,
            peak: true,
            models: vec![],
        }];

        insert(&c, "a", now, &msg("deepseek-v4-flash-0731", 1_000_000, 0, 0, 0));
        insert(&c, "b", now + 1, &msg("deepseek-v4-flash-0731", 0, 0, 0, 0));
        insert(&c, "y", now - 86_400_000, &msg("deepseek-v4-flash-0731", 999_999_999, 0, 0, 0));
        insert(&c, "g", now, &msg("gpt-4o", 999_999_999, 0, 0, 0));
        insert(&c, "d", now + 2, &msg("deepseek-v4-flash-0731", 0, 500_000, 200_000, 100_000));

        let (cost, tokens) = today_cost(&db, &providers).unwrap();
        let expect = 1_000_000.0 / 1e6 * p.hit[pi]
            + 500_000.0 / 1e6 * p.miss[pi]
            + 300_000.0 / 1e6 * p.out[pi];
        assert!((cost - expect).abs() < 1e-9, "cost {cost} expect {expect}");
        assert!((tokens - 1_800_000.0).abs() < 1.0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn default_path_point() {
        let p = db_path(None);
        assert!(p.to_string_lossy().contains("opencode.db"));
        assert!(p.to_string_lossy().contains(".local"));
    }
}