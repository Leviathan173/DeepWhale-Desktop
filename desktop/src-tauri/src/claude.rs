//! Claude Code 本地记账：扫 ~/.claude/projects/**/*.jsonl 的 assistant 事件，
//! 读 message.usage 的 input/output/cache_read/cache_creation token 数，
//! 按模型计价换算 CNY 累加为「今日已用」。与 opencode.db 一起构成小鲸鱼的本地总账。
//! ponytail: 依赖 Claude Code jsonl 事件固定结构（type=assistant + message.usage），
//! Claude Code 改版需同步跟进。
use serde_json::Value;
use std::fs;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use time::format_description::well_known::Rfc3339;

use crate::config::ProviderCfg;
use crate::pricing::resolve_unit;

/// `~/.claude/projects` 默认根目录（env HOME/USERPROFILE）。
fn projects_dir() -> PathBuf {
    let mut base = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default();
    if base.as_os_str().is_empty() {
        base = PathBuf::from(".");
    }
    base.push(".claude/projects");
    base
}

/// 递归收集目录下所有 *.jsonl。
fn scan_jsonl(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            scan_jsonl(&p, out);
        } else if p.extension().is_some_and(|x| x == "jsonl") {
            out.push(p);
        }
    }
}

/// settings.json 的 env 里显式配置的 API Key（ANTHROPIC_AUTH_TOKEN）。
/// Claude Code 固定归为提供商 "claude"（jsonl 里没有 providerID，无法区分更细）。
pub fn discover_api_key() -> Option<(String, String)> {
    let mut p = projects_dir();
    p.pop();
    p.push("settings.json");
    let s = fs::read_to_string(&p).ok()?;
    let v: Value = serde_json::from_str(&s).ok()?;
    let key = v
        .get("env")
        .and_then(|e| e.get("ANTHROPIC_AUTH_TOKEN"))
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())?;
    Some(("claude".to_string(), key))
}

/// settings.json 里显式指定的模型（ANTHROPIC_DEFAULT_*_MODEL），去重小写。
/// 跳过空 / `<...>` 这类非真实模型名。
pub fn configured_models() -> Vec<String> {
    let mut p = projects_dir();
    p.pop();
    p.push("settings.json");
    let mut out = std::collections::BTreeSet::new();
    if let Ok(s) = fs::read_to_string(&p) {
        if let Ok(v) = serde_json::from_str::<Value>(&s) {
            if let Some(map) = v.get("env").and_then(|e| e.as_object()) {
                for (k, val) in map {
                    if k.starts_with("ANTHROPIC_DEFAULT_") && k.ends_with("_MODEL") {
                        if let Some(m) = val.as_str() {
                            let m = m.to_ascii_lowercase();
                            if !m.is_empty() && !m.starts_with('<') {
                                out.insert(m);
                            }
                        }
                    }
                }
            }
        }
    }
    out.into_iter().collect()
}

/// 自动发现：只取 settings.json 声明的模型（供应商固定为 claude）。
/// jsonl 仅用于运行时计 token，不贡献模型条目。
pub fn discover_models(_dir: Option<&Path>) -> Vec<String> {
    configured_models()
}

/// 单条 assistant 事件 → (epoch_ms, model, input, output, cache_read, cache_create)。
fn parse_event(line: &str, today_start: i64) -> Option<(i64, String, i64, i64, i64, i64)> {
    let v: Value = serde_json::from_str(line).ok()?;
    if v.get("type").and_then(|x| x.as_str()) != Some("assistant") {
        return None;
    }
    let msg = v.get("message")?;
    let ts_ms = v
        .get("timestamp")
        .and_then(|x| x.as_str())
        .and_then(|s| time::OffsetDateTime::parse(s, &Rfc3339).ok())
        .map(|t| t.unix_timestamp() * 1000)?;
    if ts_ms < today_start {
        return None;
    }
    let model = msg.get("model").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let u = msg.get("usage")?;
    let n = |k: &str| u.get(k).and_then(|x| x.as_i64()).unwrap_or(0);
    Some((
        ts_ms,
        model,
        n("input_tokens"),
        n("output_tokens"),
        n("cache_read_input_tokens"),
        n("cache_creation_input_tokens"),
    ))
}

/// 累计「今天」全部 Claude Code 会话费用（CNY）与总 token。
/// 只算按量计费供应商（providers 表中 metric=true）；套餐/未配置模型不计金额。
/// 读不到数据返回 None（上层回退）。
pub fn today_cost(dir: Option<&Path>, providers: &[ProviderCfg]) -> Option<(f64, f64)> {
    let root = dir.map(|d| d.to_path_buf()).unwrap_or_else(projects_dir);
    let mut files = Vec::new();
    scan_jsonl(&root, &mut files);
    let today_start = crate::opencode::today_start_ms();

    let mut cost = 0.0;
    let mut total_tokens = 0.0;
    let mut found = false;
    for f in files {
        let Ok(fh) = fs::File::open(&f) else { continue };
        for line in std::io::BufReader::new(fh).lines().map_while(Result::ok) {
            let Some((ts_ms, model, input, output, cache_read, cache_create)) =
                parse_event(&line, today_start)
            else {
                continue;
            };
            let total_tok = input + output + cache_read + cache_create;
            if total_tok == 0 {
                continue;
            }
            let Some(u) = resolve_unit(providers, &model, Some("claude")) else {
                continue; // claude 套餐/未配置：不计金额
            };
            found = true;
            let pi = if u.peak { usize::from(crate::pricing::is_peak_time(ts_ms / 1000)) } else { 0 };
            cost += (cache_read as f64) / 1e6 * u.hit[pi]
                + (cache_create as f64) / 1e6 * u.create[pi]
                + (input as f64) / 1e6 * u.miss[pi]
                + (output as f64) / 1e6 * u.out[pi];
            total_tokens += total_tok as f64;
        }
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

    /// 「本地现在」的 ISO 时间戳（必定 >= 今天零点，测试数据都算今天）。
    fn today_iso() -> String {
        time::OffsetDateTime::now_local()
            .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
            .format(&Rfc3339)
            .unwrap()
    }

    #[test]
    fn claude_fixed_provider_exact() {
        use crate::config::{ModelPriceCfg, ProviderCfg};
        let dir = std::env::temp_dir().join("dshw-claude-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("skills.jsonl");
        let today = today_iso();
        let lines = [
            format!(r#"{{"type":"mode","mode":"plan","timestamp":"{today}"}}"#),
            format!(r#"{{"type":"assistant","timestamp":"{today}","message":{{"model":"deepseek-v4-flash","usage":{{"input_tokens":1000000,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}}}"#),
            format!(r#"{{"type":"assistant","timestamp":"{today}","message":{{"model":"LongCat-2.0","usage":{{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":2000000,"cache_creation_input_tokens":0}}}}}}"#),
            r#"{"type":"assistant","timestamp":"1999-01-01T00:00:00Z","message":{"model":"deepseek-v4-flash","usage":{"input_tokens":999999999,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#.to_string(),
        ];
        fs::write(&f, lines.join("\n")).unwrap();

        // 默认表含 claude 套餐提供器 → 全部不计金额。
        assert!(today_cost(Some(&dir), &crate::pricing::default_providers()).is_none());

        // claude 按量 + 精确模型名：deepseek 走内置价（平档 1.5），longcat-2.0 0.5 元/百万。
        let providers = vec![ProviderCfg {
            name: "claude".into(),
            metric: true,
            peak: false,
            models: vec![
                ModelPriceCfg { pattern: "deepseek-v4-flash".into(), ..Default::default() },
                ModelPriceCfg {
                    pattern: "longcat-2.0".into(),
                    input: Some(0.5),
                    output: Some(0.5),
                    cache_read: Some(0.5),
                    cache_creation: Some(0.5),
                },
            ],
        }];
        let (cost, _) = today_cost(Some(&dir), &providers).unwrap();
        let expect = 1_000_000.0 / 1e6 * 1.5 + 2_000_000.0 / 1e6 * 0.5;
        assert!((cost - expect).abs() < 1e-9, "cost {cost} expect {expect}");

        // 泛化 pattern "deepseek" 精确不命中 "deepseek-v4-flash"：
        // deepseek 兜底内置价 1.5，LongCat 未列出 → 0 元但 token 仍统计。
        let providers = vec![ProviderCfg {
            name: "claude".into(),
            metric: true,
            peak: false,
            models: vec![ModelPriceCfg { pattern: "deepseek".into(), ..Default::default() }],
        }];
        let (cost, tokens) = today_cost(Some(&dir), &providers).unwrap();
        assert!((cost - 1_000_000.0 / 1e6 * 1.5).abs() < 1e-9, "cost {cost}");
        assert!((tokens - 3_000_000.0).abs() < 1.0);

        // claude 套餐（metric=false）→ 无金额。
        let providers = vec![ProviderCfg {
            name: "claude".into(),
            metric: false,
            peak: false,
            models: vec![],
        }];
        assert!(today_cost(Some(&dir), &providers).is_none());

        let _ = fs::remove_dir_all(&dir);
    }
}