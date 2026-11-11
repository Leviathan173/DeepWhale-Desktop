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
    /// 防止轮询对同一失效凭证反复启动浏览器抓取。
    /// 存「上次尝试时间」而不是布尔开关：失败一次后冷却 10 分钟仍会重试，
    /// 不会像旧实现那样一次失败就锁死到进程退出。
    pub credential_retry_at: Mutex<Option<std::time::Instant>>,
    /// 已经提示过用户「凭据需要手动更新」——只在首次失败时发事件，成功后复位。
    pub credential_needs_user: std::sync::atomic::AtomicBool,
    /// 用户点了「更新凭据」正在跑：期间后台不要重复发起续期/弹浏览器。
    pub credential_busy: std::sync::atomic::AtomicBool,
    /// 序列化 config.json 的读改写，避免 set_config 与 save_credentials 并发互相覆盖。
    pub cfg: Mutex<()>,
    /// 托盘「检查更新」置位；设置页启动或收到事件后取走（一次性），避免监听器未就绪的事件竞态。
    pub check_update: std::sync::atomic::AtomicBool,
}
