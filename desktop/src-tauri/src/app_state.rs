//! Tauri 全局应用状态（app data 目录、HTTP 客户端、余额缓存）。
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Clone)]
pub struct BalanceCache {
    pub at: std::time::Instant,
    pub payload: serde_json::Value,
}

pub struct AppState {
    pub dir: PathBuf,
    pub client: reqwest::Client,
    pub cache: Mutex<Option<BalanceCache>>,
    pub busy: tokio::sync::Mutex<()>,
}
