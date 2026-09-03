//! 配置持久化：config.json（挂件偏好 + 凭据），存于 app data 目录。
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

pub fn normalize(m: &str) -> &'static str {
    if m == "token" {
        "token"
    } else {
        "opencode"
    }
}

/// 音效集：小黄鸭 / 音效1 / 鲸语（TTS 拟声）。
pub fn sound_set(m: &str) -> &'static str {
    if m == "fx1" {
        "fx1"
    } else if m == "whale" {
        "whale"
    } else {
        "duck"
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
    /// 梭子蟹中转站（suoxie.codes）登录 JWT（Bearer，约 24h 过期需重抓）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suoxie_token: Option<String>,
    /// 自动获取凭据时优先附加常驻调试浏览器（--remote-debugging-port 启动），失败再拉起新窗口。
    #[serde(default)]
    pub debug_attach: bool,
    /// 常驻调试浏览器 CDP 端口。
    #[serde(default = "default_debug_port")]
    pub debug_port: u16,
    /// 可选的 opencode.db 路径覆盖（留空 → 默认 ~/.local/share/opencode/opencode.db）。
    pub opencode_db: Option<String>,
    /// 用户自定义供应商/模型计价表；None → 用内置默认表（pricing::default_providers）。
    #[serde(default)]
    pub usage_providers: Option<Vec<ProviderCfg>>,
    /// 小鲸鱼当前展示的供应商（deepseek/bailian/suoxie）。
    #[serde(default = "default_provider")]
    pub provider: String,
    /// 通知阈值。None/≤0 = 关闭该项通知。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ds_hourly_limit: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ds_min_balance: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bl_hourly_pct: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bl_remaining_pct: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sx_hourly_limit: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sx_min_balance: Option<f64>,
}

fn positive(v: Option<f64>) -> Option<f64> {
    v.filter(|x| x.is_finite() && *x > 0.0)
}

/// 百分比阈值额外限制在 0..=100（百炼字段）。
fn positive_pct(v: Option<f64>) -> Option<f64> {
    v.filter(|x| x.is_finite() && *x > 0.0 && *x <= 100.0)
}

fn default_provider() -> String {
    "deepseek".to_string()
}

fn default_debug_port() -> u16 {
    9222
}

/// 展示供应商枚举归一：未知值一律回 deepseek。
pub fn norm_provider(p: &str) -> &'static str {
    match p {
        "bailian" => "bailian",
        "suoxie" => "suoxie",
        _ => "deepseek",
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            scale: 1.5,
            sound: true,
            vol: 0.9,
            sound_set: "duck".to_string(),
            usage_mode: "token".to_string(),
            api_key: None,
            platform_token: None,
            bailian_cookie: None,
            bailian_post_data: None,
            suoxie_token: None,
            debug_attach: false,
            debug_port: default_debug_port(),
            opencode_db: None,
            usage_providers: None,
            provider: default_provider(),
            ds_hourly_limit: None,
            ds_min_balance: None,
            bl_hourly_pct: None,
            bl_remaining_pct: None,
            sx_hourly_limit: None,
            sx_min_balance: None,
        }
    }
}

impl AppConfig {
    fn normalized(mut self) -> Self {
        self.sound_set = sound_set(&self.sound_set).to_string();
        self.usage_mode = normalize(&self.usage_mode).to_string();
        self.provider = norm_provider(&self.provider).to_string();
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
    cfg.sound_set = crate::config::sound_set(sound_set).to_string();
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
    cfg.bailian_cookie = cookie
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    cfg.bailian_post_data = post_data
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    write_file(dir, &cfg);
}

/// 只改梭子蟹 token。
pub fn write_suoxie_token(dir: &Path, token: Option<String>) {
    let mut cfg = read(dir);
    cfg.suoxie_token = token
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    write_file(dir, &cfg);
}

/// 只改「附加调试浏览器」偏好。port=0 归一为默认 9222。
pub fn write_debug_prefs(dir: &Path, attach: bool, port: u16) {
    let mut cfg = read(dir);
    cfg.debug_attach = attach;
    cfg.debug_port = if port == 0 {
        default_debug_port()
    } else {
        port
    };
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
    cfg.provider = norm_provider(provider).to_string();
    write_file(dir, &cfg.normalized());
}

/// 只改通知阈值。None/≤0 → 关闭该项通知（不改其他配置）。
pub fn write_notify_prefs(
    dir: &Path,
    ds_hourly_limit: Option<f64>,
    ds_min_balance: Option<f64>,
    bl_hourly_pct: Option<f64>,
    bl_remaining_pct: Option<f64>,
    sx_hourly_limit: Option<f64>,
    sx_min_balance: Option<f64>,
) {
    let mut cfg = read(dir);
    cfg.ds_hourly_limit = positive(ds_hourly_limit);
    cfg.ds_min_balance = positive(ds_min_balance);
    cfg.bl_hourly_pct = positive_pct(bl_hourly_pct);
    cfg.bl_remaining_pct = positive_pct(bl_remaining_pct);
    cfg.sx_hourly_limit = positive(sx_hourly_limit);
    cfg.sx_min_balance = positive(sx_min_balance);
    write_file(dir, &cfg.normalized());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn notify_prefs_roundtrip_and_positive_only() {
        let dir = tmp_dir("dshw-config-notify-test");
        write_notify_prefs(&dir, None, None, None, None, None, None);
        assert!(read(&dir).ds_hourly_limit.is_none());

        // 有效正数保存，0 / 负数 / NaN 归一为 None；百分比 >100 也归一
        write_notify_prefs(
            &dir,
            Some(20.0),
            Some(0.0),
            Some(-1.0),
            Some(150.0),
            Some(9.0),
            Some(0.0),
        );
        let c = read(&dir);
        assert_eq!(c.ds_hourly_limit, Some(20.0));
        assert!(c.ds_min_balance.is_none());
        assert!(c.bl_hourly_pct.is_none());
        assert!(c.bl_remaining_pct.is_none());
        assert_eq!(c.sx_hourly_limit, Some(9.0));
        assert!(c.sx_min_balance.is_none());

        write_notify_prefs(&dir, None, None, Some(50.0), Some(100.0), None, None);
        let c = read(&dir);
        assert_eq!(c.bl_hourly_pct, Some(50.0));
        assert_eq!(c.bl_remaining_pct, Some(100.0));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn notify_prefs_preserve_other_fields() {
        let dir = tmp_dir("dshw-config-notify-test2");
        write_prefs(&dir, 1.7, true, 0.6, "fx1", "token", "/tmp/x.db");
        write_notify_prefs(&dir, None, Some(5.0), Some(50.0), Some(30.0), None, None);
        let c = read(&dir);
        assert_eq!(c.scale, 1.7);
        assert_eq!(c.usage_mode, "token");
        assert_eq!(c.bl_remaining_pct, Some(30.0));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn suoxie_provider_and_token_roundtrip() {
        let dir = tmp_dir("dshw-config-suoxie-test");
        write_suoxie_token(&dir, Some("  jwt.abc  ".to_string()));
        assert_eq!(read(&dir).suoxie_token.as_deref(), Some("jwt.abc"));
        write_provider(&dir, "suoxie");
        assert_eq!(read(&dir).provider, "suoxie");
        // 未知供应商归一为 deepseek
        write_provider(&dir, "bogus");
        assert_eq!(read(&dir).provider, "deepseek");
        // 空 token → 清空
        write_suoxie_token(&dir, Some("   ".to_string()));
        assert!(read(&dir).suoxie_token.is_none());
        // debug prefs 往返 + port=0 归一 9222 + 不动凭据
        assert_eq!(
            (read(&dir).debug_attach, read(&dir).debug_port),
            (false, 9222)
        );
        write_debug_prefs(&dir, true, 9223);
        write_debug_prefs(&dir, true, 0);
        let c = read(&dir);
        assert!(c.debug_attach);
        assert_eq!(c.debug_port, 9222);
        write_debug_prefs(&dir, false, 1234);
        let c = read(&dir);
        assert!(!c.debug_attach);
        assert_eq!(c.debug_port, 1234);
        assert_eq!(norm_provider("suoxie"), "suoxie");
        assert_eq!(norm_provider("x"), "deepseek");
        let _ = fs::remove_dir_all(&dir);
    }
}
