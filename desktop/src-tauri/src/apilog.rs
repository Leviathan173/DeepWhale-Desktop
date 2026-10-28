//! 接口调用日志：写 `<profile>/api.log`，超过 10MB 只保留最近约 512KB。
//! 一行一次请求：时间、来源、method、URL（含 query 参数）、状态码/错误、耗时、响应片段。
//! 不记请求头（Authorization/cookie 不进日志），响应片段压成单行并截断。
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use time::format_description::well_known::Rfc3339;

const MAX_BYTES: u64 = 10 * 1024 * 1024;
const KEEP_BYTES: usize = 512 * 1024;
const SNIPPET_CHARS: usize = 300;

static PATH: OnceLock<PathBuf> = OnceLock::new();
static LOCK: Mutex<()> = Mutex::new(());

/// 启动时定下日志路径（profile 目录已由调用方建好）。
pub fn init(dir: &Path) {
    let _ = PATH.set(dir.join("api.log"));
}

/// 统一发请求 + 落日志；返回 (状态码, 响应文本)，JSON 由调用方解析。
/// 网络层失败返回 Err(错误文本)，同时也会记一行 ERR。
pub async fn send(tag: &str, req: reqwest::RequestBuilder) -> Result<(u16, String), String> {
    let (method, url) = describe(&req);
    let t0 = Instant::now();
    match req.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            let ms = t0.elapsed().as_millis();
            append(&format!(
                "{} [{tag}] {method} {url} -> {status} {ms}ms{}\n",
                stamp(),
                fmt_body(&body)
            ));
            Ok((status, body))
        }
        Err(e) => {
            append(&format!(
                "{} [{tag}] {method} {url} -> ERR {} {}ms\n",
                stamp(),
                flat(&err_chain(&e), SNIPPET_CHARS),
                t0.elapsed().as_millis()
            ));
            Err(e.to_string())
        }
    }
}

/// reqwest 的 Display 只有「error sending request for url」，真正原因在 source 链里
/// （connect timeout / connection closed before message completed / dns 等）。
fn err_chain(e: &reqwest::Error) -> String {
    let mut s = e.to_string();
    let mut cur = std::error::Error::source(e);
    while let Some(inner) = cur {
        s.push_str(" / ");
        s.push_str(&inner.to_string());
        cur = inner.source();
    }
    s
}

fn stamp() -> String {
    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

fn describe(req: &reqwest::RequestBuilder) -> (String, String) {
    // RequestBuilder 只能 send 一次，用 try_clone 造一份只为读 method/url（不执行）
    match req.try_clone().and_then(|b| b.build().ok()) {
        Some(r) => (r.method().to_string(), r.url().to_string()),
        None => ("?".to_string(), "?".to_string()),
    }
}

fn fmt_body(body: &str) -> String {
    if body.is_empty() {
        return String::new();
    }
    format!(" | {}", flat(body, SNIPPET_CHARS))
}

/// 单行化 + 截断：日志一行一次请求，便于 grep。
fn flat(s: &str, max: usize) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= max {
        one
    } else {
        let mut t: String = one.chars().take(max).collect();
        t.push('…');
        t
    }
}

fn append(s: &str) {
    let Some(p) = PATH.get() else { return };
    append_to(p, s, MAX_BYTES);
}

fn append_to(p: &Path, s: &str, max: u64) {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if std::fs::metadata(p).map(|m| m.len()).unwrap_or(0) > max {
        trim_tail(p, KEEP_BYTES);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(p)
    {
        let _ = f.write_all(s.as_bytes());
    }
}

/// 只留文件末尾 keep 字节，并从下一个换行开始，避免留下半行。
fn trim_tail(p: &Path, keep: usize) {
    let Ok(data) = std::fs::read(p) else { return };
    let tail = &data[data.len().saturating_sub(keep)..];
    let off = tail.iter().position(|b| *b == b'\n').map_or(0, |i| i + 1);
    let _ = std::fs::write(p, &tail[off..]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::create_dir_all(&dir);
        dir.join("api.log")
    }

    #[test]
    fn append_keeps_lines() {
        let p = tmp("dshw-apilog-append");
        let _ = std::fs::remove_file(&p);
        append_to(&p, "a\n", MAX_BYTES);
        append_to(&p, "b\n", MAX_BYTES);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "a\nb\n");

        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn trim_keeps_recent_full_lines() {
        let p = tmp("dshw-apilog-trim");
        let body: String = (0..50).map(|i| format!("line-{i:02}\n")).collect();
        std::fs::write(&p, &body).unwrap();
        trim_tail(&p, 40);
        let kept = std::fs::read_to_string(&p).unwrap();
        assert!(kept.starts_with("line-"));
        assert!(kept.ends_with("line-49\n"));
        assert!(kept.len() <= 48);

        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn oversized_file_gets_trimmed_before_append() {
        let p = tmp("dshw-apilog-cap");
        let body: String = (0..200)
            .map(|i| format!("old-{i:03} {}\n", "x".repeat(40)))
            .collect();
        std::fs::write(&p, &body).unwrap();
        append_to(&p, "new\n", 1024);
        let out = std::fs::read_to_string(&p).unwrap();
        // 触发过裁剪：旧内容留下的是尾巴，且新行在末尾、每行都完整
        assert!(out.ends_with("new\n"));
        assert!(out.len() < body.len());
        assert!(out.lines().all(|l| l == "new" || l.starts_with("old-")));

        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn flat_is_single_line_and_truncated() {
        assert_eq!(flat("a\n b\tc ", 99), "a b c");
        assert_eq!(flat("abcdef", 3), "abc…");
    }
}
