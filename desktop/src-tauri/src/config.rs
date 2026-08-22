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

/// 单个模型的计价项（本地记账用）。
/// pattern 对模型名做精确全名匹配。单价 = 元/百万 token：
/// 普通四类 + 高峰四类（peak_*）。None → 该项用内置价目。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelPriceCfg {
    pub pattern: String,
    /// 空闲价（非峰谷则同时用作高峰档）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation: Option<f64>,
    /// 高峰价（仅峰谷供应商生效）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_input: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_output: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_cache_read: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_cache_creation: Option<f64>,
}

impl ModelPriceCfg {
    /// 用户是否显式填了任一单价（auto 发现时据此保留旧条目）。
    pub fn priced(&self) -> bool {
        self.input.is_some()
            || self.output.is_some()
            || self.cache_read.is_some()
            || self.cache_creation.is_some()
            || self.peak_input.is_some()
            || self.peak_output.is_some()
            || self.peak_cache_read.is_some()
            || self.peak_cache_creation.is_some()
    }
}

/// 供应商：决定「按量计费 / 套餐」与这一组模型的定价。
/// opencode 消息带 providerID，按 name 精确匹配；没有 providerID 的来源（claude jsonl）
/// 按其 models.pattern 匹配。metric=false（套餐/订阅制，如 tokenplan）不计入今日金额。
/// peak=true 时按峰谷计价（DeepSeek 官方是典型：命中/输入/输出各自 [空闲,高峰] 两档）；
/// 模型没填任何单价也用内置峰谷价目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCfg {
    pub name: String,
    pub metric: bool,
    #[serde(default = "default_true")]
    pub peak: bool,
    #[serde(default)]
    pub models: Vec<ModelPriceCfg>,
}

fn default_true() -> bool {
    true
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
    /// 百炼 TokenPlan 控制台接口的登录 Cookie（自动抓取，会过期需重抓）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bailian_cookie: Option<String>,
    /// 订阅接口的完整 form body（含 params JSON / sec_token / region），原样重放最稳。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bailian_post_data: Option<String>,
    /// 可选的 opencode.db 路径覆盖（留空 → 默认 ~/.local/share/opencode/opencode.db）。
    pub opencode_db: Option<String>,
    /// 用户自定义供应商/模型计价表；None → 用内置默认表（pricing::default_providers）。
    #[serde(default)]
    pub usage_providers: Option<Vec<ProviderCfg>>,
    /// 小鲸鱼当前展示的供应商（deepseek/bailian）。
    #[serde(default = "default_provider")]
    pub provider: String,
}

fn default_provider() -> String {
    "deepseek".to_string()
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
            bailian_cookie: None,
            bailian_post_data: None,
            opencode_db: None,
            usage_providers: None,
            provider: default_provider(),
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
        self.provider = if self.provider == "bailian" { "bailian" } else { "deepseek" }.to_string();
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

/// 只改百炼凭据。
pub fn write_bailian_credentials(dir: &Path, cookie: Option<String>, post_data: Option<String>) {
    let mut cfg = read(dir);
    cfg.bailian_cookie = cookie.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    cfg.bailian_post_data = post_data.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    write_file(dir, &cfg);
}

/// 只改计价表。空表 → 存 None（走内置默认）。
pub fn write_usage_providers(dir: &Path, providers: Vec<ProviderCfg>) {
    let mut cfg = read(dir);
    cfg.usage_providers = (!providers.is_empty()).then_some(providers);
    write_file(dir, &cfg.normalized());
}

/// 只改展示供应商。
pub fn write_provider(dir: &Path, provider: &str) {
    let mut cfg = read(dir);
    cfg.provider = if provider == "bailian" {
        "bailian".to_string()
    } else {
        "deepseek".to_string()
    };
    write_file(dir, &cfg.normalized());
}
