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
    /// 序列化 config.json 的读改写，避免 set_config 与 save_credentials 并发互相覆盖。
    pub cfg: Mutex<()>,
    /// 托盘「检查更新」置位；设置页启动或收到事件后取走（一次性），避免监听器未就绪的事件竞态。
    pub check_update: std::sync::atomic::AtomicBool,
}
