//! 配置持久化：config.json（挂件偏好 + 凭据），存于 app data 目录。
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

pub fn normalize(m: &str) -> &'static str {
    if m == "token" {
        "token"
    } else if m == "opencode" {
        "opencode"
    } else {
        "ledger"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub scale: f64,
    pub sound: bool,
    pub vol: f64,
    pub sound_set: String,
    pub usage_mode: String,
    pub api_key: Option<String>,
    pub platform_token: Option<String>,
    /// 可选的 opencode.db 路径覆盖（留空 → 默认 ~/.local/share/opencode/opencode.db）。
    pub opencode_db: Option<String>,
    /// 本地记账里「未知名别名模型」（如 LongCat-2.0 代理别名）的单折价：
    /// CNY / 百万 token，作用于该类模型的全部 token。None → 这类模型不计钱。
    pub unknown_price_per_m: Option<f64>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            scale: 1.5,
            sound: true,
            vol: 0.9,
            sound_set: "duck".to_string(),
            usage_mode: "ledger".to_string(),
            api_key: None,
            platform_token: None,
            opencode_db: None,
            unknown_price_per_m: None,
        }
    }
}

impl AppConfig {
    fn normalized(mut self) -> Self {
        self.sound_set = if self.sound_set == "fx1" {
            "fx1"
        } else {
            "duck"
        }
        .to_string();
        self.usage_mode = normalize(&self.usage_mode).to_string();
        self
    }
}

fn file(dir: &Path) -> std::path::PathBuf {
    dir.join("config.json")
}

pub fn read(dir: &Path) -> AppConfig {
    let cfg: AppConfig = fs::read(dir.join("config.json"))
        .ok()
        .and_then(|s| serde_json::from_slice::<AppConfig>(&s).ok())
        .unwrap_or_default()
        .normalized();
    cfg
}

fn write_file(dir: &Path, cfg: &AppConfig) {
    match serde_json::to_string_pretty(cfg) {
        Ok(s) => {
            if let Err(e) = fs::write(file(dir), s) {
                eprintln!("config write failed: {e}");
            }
        }
        Err(e) => eprintln!("config serialize failed: {e}"),
    }
}

/// 覆盖挂件偏好（不改凭据）。
pub fn write_prefs(
    dir: &Path,
    scale: f64,
    sound: bool,
    vol: f64,
    sound_set: &str,
    usage_mode: &str,
    opencode_db: &str,
) {
    let mut cfg = read(dir);
    cfg.scale = scale;
    cfg.sound = sound;
    cfg.vol = vol;
    cfg.sound_set = if sound_set == "fx1" { "fx1" } else { "duck" }.to_string();
    cfg.usage_mode = normalize(usage_mode).to_string();
    cfg.opencode_db = Some(opencode_db.trim().to_string()).filter(|s| !s.is_empty());
    write_file(dir, &cfg.normalized());
}

/// 只改凭据。
pub fn write_credentials(dir: &Path, api_key: Option<String>, platform_token: Option<String>) {
    let mut cfg = read(dir);
    cfg.api_key = api_key
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty());
    cfg.platform_token = platform_token
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty());
    write_file(dir, &cfg);
}
