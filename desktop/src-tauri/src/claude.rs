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

use crate::pricing::{claude_usd, price_for, qwen_cny, USD_TO_CNY};

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
/// unknown_ppm: 未知名模型（代理别名如 LongCat-2.0）的 CNY/百万 token 折合价，
/// None 时这类模型不计钱。读不到数据返回 None（上层回退）。
pub fn today_cost(dir: Option<&Path>, unknown_ppm: Option<f64>) -> Option<(f64, f64)> {
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
            found = true;
            let pi = usize::from(crate::pricing::is_peak_time(ts_ms / 1000));
            let m = model.to_ascii_lowercase();
            let ev_cost = if m.contains("deepseek") {
                let p = price_for(&model);
                (cache_read as f64) / 1e6 * p.hit[pi]
                    + (input + cache_create) as f64 / 1e6 * p.miss[pi]
                    + (output as f64) / 1e6 * p.out[pi]
            } else if m.contains("claude") {
                match claude_usd(&model) {
                    Some(p) => {
                        ((input as f64) / 1e6 * p.input
                            + (output as f64) / 1e6 * p.output
                            + (cache_read as f64) / 1e6 * p.cache_read
                            + (cache_create as f64) / 1e6 * p.cache_write)
                            * USD_TO_CNY
                    }
                    None => unknown_model_cost(total_tok, unknown_ppm),
                }
            } else if m.contains("qwen") {
                match qwen_cny(&model) {
                    Some(p) => {
                        (cache_read as f64) / 1e6 * p.hit
                            + (input + cache_create) as f64 / 1e6 * p.miss
                            + (output as f64) / 1e6 * p.out
                    }
                    None => unknown_model_cost(total_tok, unknown_ppm),
                }
            } else {
                unknown_model_cost(total_tok, unknown_ppm)
            };
            total_tokens += total_tok as f64;
            cost += ev_cost;
        }
    }
    if found {
        Some((cost, total_tokens))
    } else {
        None
    }
}

fn unknown_model_cost(total_tok: i64, unknown_ppm: Option<f64>) -> f64 {
    match unknown_ppm {
        Some(ppm) => total_tok as f64 / 1e6 * ppm,
        None => 0.0,
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
    fn deepseek_and_unknown() {
        let dir = std::env::temp_dir().join("dshw-claude-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("skills.jsonl");
        let today = today_iso();
        let lines = [
            format!(r#"{{"type":"mode","mode":"plan","timestamp":"{today}"}}"#),
            format!(r#"{{"type":"assistant","timestamp":"{today}","message":{{"model":"deepseek-v4-flash","usage":{{"input_tokens":1000000,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}}}"#),
            format!(r#"{{"type":"assistant","timestamp":"{today}","message":{{"model":"LongCat-2.0","usage":{{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":2000000,"cache_creation_input_tokens":0}}}}}}"#),
            format!(r#"{{"type":"assistant","timestamp":"1999-01-01T00:00:00Z","message":{{"model":"deepseek-v4-flash","usage":{{"input_tokens":999999999,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}}}"#),
        ];
        fs::write(&f, lines.join("\n")).unwrap();

        // 未配置 unknown 单价 → LongCat 记 0，只算 deepseek 1M input。
        let (cost, tokens) = today_cost(Some(&dir), None).unwrap();
        let p = price_for("deepseek-v4-flash");
        let ts = time::OffsetDateTime::parse(&today, &Rfc3339).unwrap().unix_timestamp();
        let pi = usize::from(crate::pricing::is_peak_time(ts));
        assert!((cost - 1_000_000.0 / 1e6 * p.miss[pi]).abs() < 1e-9, "cost {cost}");
        assert!((tokens - 3_000_000.0).abs() < 1.0);

        // 配置 0.5 元/百万 → LongCat 2M tokens = 1.0 元。
        let (cost, _) = today_cost(Some(&dir), Some(0.5)).unwrap();
        let expect = 1_000_000.0 / 1e6 * p.miss[pi] + 2_000_000.0 / 1e6 * 0.5;
        assert!((cost - expect).abs() < 1e-9, "cost {cost} expect {expect}");

        let _ = fs::remove_dir_all(&dir);
    }
}