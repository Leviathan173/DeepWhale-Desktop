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
            let Some(u) = resolve_unit(providers, &model, None) else {
                continue; // 套餐/未配置：不计金额
            };
            found = true;
            let pi = usize::from(crate::pricing::is_peak_time(ts_ms / 1000));
            cost += (cache_read as f64) / 1e6 * u.hit[pi]
                + (input + cache_create) as f64 / 1e6 * u.miss[pi]
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
    fn deepseek_and_unknown() {
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
            format!(r#"{{"type":"assistant","timestamp":"1999-01-01T00:00:00Z","message":{{"model":"deepseek-v4-flash","usage":{{"input_tokens":999999999,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}}}"#),
        ];
        fs::write(&f, lines.join("\n")).unwrap();

        // 默认表：deepseek/longcat 命中 bailian 套餐 → 全部不计金额。
        assert!(today_cost(Some(&dir), &crate::pricing::default_providers()).is_none());

        // 用户把 LongCat 配成按量 + 0.5 元/百万 → 2M tokens = 1.0 元；
        // deepseek 仍命中 bailian 套餐被排除。
        let providers = vec![ProviderCfg {
            name: "bailian".into(),
            metric: false,
            models: vec![ModelPriceCfg { pattern: "deepseek".into(), ppm: None }],
        }, ProviderCfg {
            name: "LongCat".into(),
            metric: true,
            models: vec![ModelPriceCfg { pattern: "longcat".into(), ppm: Some(0.5) }],
        }];
        let (cost, _) = today_cost(Some(&dir), &providers).unwrap();
        let expect = 2_000_000.0 / 1e6 * 0.5;
        assert!((cost - expect).abs() < 1e-9, "cost {cost} expect {expect}");

        // bailian 套餐（metric=false）命中所有事件时 → 无金额。
        let providers = vec![ProviderCfg {
            name: "bailian".into(),
            metric: false,
            models: vec![ModelPriceCfg { pattern: "deepseek".into(), ppm: None }],
        }];
        assert!(today_cost(Some(&dir), &providers).is_none());

        // 纯深求索（无 providerID 的 deepseek 兜底内置价目）。
        let providers = vec![ProviderCfg {
            name: "deepseek".into(),
            metric: true,
            models: vec![],
        }];
        let (cost, tokens) = today_cost(Some(&dir), &providers).unwrap();
        let p = crate::pricing::price_for("deepseek-v4-flash");
        let ts = time::OffsetDateTime::parse(&today, &Rfc3339).unwrap().unix_timestamp();
        let pi = usize::from(crate::pricing::is_peak_time(ts));
        assert!((cost - 1_000_000.0 / 1e6 * p.miss[pi]).abs() < 1e-9, "cost {cost}");
        // tokens 只累计计入金额的事件（LongCat 被跳过）。
        assert!((tokens - 1_000_000.0).abs() < 1.0);

        let _ = fs::remove_dir_all(&dir);
    }
}