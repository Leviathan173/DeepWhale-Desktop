#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod assets;
mod balance;
mod claude;
mod config;
mod ledger;
mod login;
mod opencode;
mod pricing;

use std::time::Duration;

use serde_json::{json, Value};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager};

use app_state::AppState;

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let client = reqwest::Client::builder()
                .user_agent("dsh-whale-desktop/0.1")
                .connect_timeout(Duration::from_secs(10))
                .build()
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            app.manage(AppState {
                dir,
                client,
                cache: std::sync::Mutex::new(None),
                busy: tokio::sync::Mutex::new(()),
                cfg: std::sync::Mutex::new(()),
            });

            create_main_window(app)?;
            create_tray(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_balance,
            get_config,
            set_config,
            save_credentials,
            load_credentials,
            open_settings,
            capture_login_token,
            image_data_url,
            sound_data_url,
            get_window_bounds,
            screen_size,
            move_window,
            set_window_bounds
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, _event| {});
}

/// 鲸鱼小窗（透明置顶、无边框、可交互）。初始尺寸比照前端 base 公式给个合理值，
/// 前端启动后会按 scale 与屏幕尺寸精调（见 set_window_bounds 命令）。
fn create_main_window(app: &mut tauri::App) -> tauri::Result<()> {
    let cfg = config::read(&app.state::<AppState>().dir);
    let (sw, sh) = screen_logical(app)?;
    let base = whale_base(sw, sh, cfg.scale.clamp(1.0, 2.5));
    tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App("index.html".into()))
            .title("DSH 小鲸鱼余额")
            .transparent(true)
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .shadow(false)
            .resizable(false)
            .minimizable(false)
            .maximizable(false)
            .closable(false)
            .focused(false)
            .inner_size(base, base)
            .position(sw - base, sh - base)
            .build()?;
    Ok(())
}

/// 与前端 widget.js 的 --dshw-base 公式保持一致：短边 × 0.17 × scale，夹在 [110, 短边×0.5]。
fn whale_base(sw: f64, sh: f64, scale: f64) -> f64 {
    let min = sw.min(sh);
    (min * 0.17 * scale).clamp(110.0, min * 0.5)
}

/// 主显示器逻辑尺寸（物理像素 ÷ DPI）。
fn screen_logical(app: &tauri::App) -> tauri::Result<(f64, f64)> {
    match app.primary_monitor()? {
        Some(m) => {
            let s = m.size();
            let sf = m.scale_factor().max(0.1);
            Ok((s.width as f64 / sf, s.height as f64 / sf))
        }
        None => Ok((1280.0, 720.0)),
    }
}

/// 托盘：刷新余额 / 设置 / 退出。
fn create_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let handle = app.handle();
    let refresh = MenuItem::with_id(handle, "refresh", "刷新余额", true, None::<&str>)?;
    let settings = MenuItem::with_id(handle, "settings", "设置 API Key", true, None::<&str>)?;
    let quit = MenuItem::with_id(handle, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(handle, &[&refresh, &settings, &quit])?;

    TrayIconBuilder::with_id("main")
        .icon(
            app.default_window_icon()
                .expect("missing default icon")
                .clone(),
        )
        .tooltip("DSH 小鲸鱼余额")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "quit" => app.exit(0),
            "refresh" => {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.emit("refresh-balance", ());
                }
            }
            "settings" => {
                if let Err(e) = open_settings_window(app) {
                    eprintln!("open settings failed: {e}");
                }
            }
            _ => {}
        })
        .build(app)
        .map(|_| ())
}

fn open_settings_window(app: &tauri::AppHandle) -> tauri::Result<()> {
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.set_focus();
        return Ok(());
    }
    tauri::WebviewWindowBuilder::new(
        app,
        "settings",
        tauri::WebviewUrl::App("settings.html".into()),
    )
    .title("设置 API Key")
    .inner_size(500.0, 470.0)
    .resizable(false)
    .center()
    .build()?;
    Ok(())
}

#[tauri::command]
async fn get_balance(app: tauri::AppHandle) -> Result<Value, String> {
    let state = app.state::<AppState>();
    if let Some(p) = balance::cached_payload(&state) {
        return Ok(p);
    }
    let _guard = state.busy.lock().await;
    if let Some(p) = balance::cached_payload(&state) {
        return Ok(p);
    }
    Ok(balance::get_balance_payload(&state).await)
}

#[tauri::command]
fn get_config(app: tauri::AppHandle) -> Value {
    let st = app.state::<AppState>();
    let _g = st.cfg.lock().unwrap_or_else(|e| e.into_inner());
    let cfg = config::read(&st.dir);
    json!({
        "scale": cfg.scale,
        "sound": cfg.sound,
        "vol": cfg.vol,
        "soundSet": cfg.sound_set,
        "usageMode": cfg.usage_mode,
        "hasApiKey": cfg.api_key.is_some(),
    })
}

#[tauri::command]
fn set_config(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let get = |k: &str| payload.get(k);
    // IPC 是信任边界：scale 在前端有校验，这里再 clamp 一道（同 widget.js MIN/MAX）。
    let scale = get("scale")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0)
        .clamp(1.0, 2.5);
    let sound = get("sound").and_then(|v| v.as_bool()).unwrap_or(true);
    let vol = get("vol").and_then(|v| v.as_f64()).unwrap_or(0.9).clamp(0.0, 1.0);
    let sound_set = get("soundSet").and_then(|v| v.as_str()).unwrap_or("duck");
    let usage_mode = get("usageMode")
        .and_then(|v| v.as_str())
        .unwrap_or("ledger");
    let opencode_db = get("opencodeDb").and_then(|v| v.as_str()).unwrap_or("");
    let st = app.state::<AppState>();
    let _g = st.cfg.lock().unwrap_or_else(|e| e.into_inner());
    let dir = &st.dir;
    config::write_prefs(dir, scale, sound, vol, sound_set, usage_mode, opencode_db);
    Ok(json!({ "ok": true }))
}

#[tauri::command]
async fn open_settings(app: tauri::AppHandle) -> Result<(), String> {
    open_settings_window(&app).map_err(|e| e.to_string())
}

/// 设置窗内嵌的「自动获取令牌」：拉起浏览器等用户登录，抓到 platform
/// Authorization 直接写回凭据并返回，前端填进输入框。
#[tauri::command]
async fn capture_login_token(app: tauri::AppHandle) -> Result<String, String> {
    let token = login::capture_platform_token().await?;
    let st = app.state::<AppState>();
    let _g = st.cfg.lock().unwrap_or_else(|e| e.into_inner());
    config::write_credentials(&st.dir, None, Some(token.clone()));
    Ok(token)
}

#[tauri::command]
fn save_credentials(
    app: tauri::AppHandle,
    api_key: Option<String>,
    platform_token: Option<String>,
) -> Value {
    let st = app.state::<AppState>();
    let _g = st.cfg.lock().unwrap_or_else(|e| e.into_inner());
    config::write_credentials(&st.dir, api_key, platform_token);
    json!({ "ok": true })
}

#[tauri::command]
fn load_credentials(app: tauri::AppHandle) -> Value {
    let st = app.state::<AppState>();
    let _g = st.cfg.lock().unwrap_or_else(|e| e.into_inner());
    let cfg = config::read(&st.dir);
    json!({ "apiKey": cfg.api_key, "platformToken": cfg.platform_token })
}

#[tauri::command]
fn image_data_url() -> String {
    assets::image_data_url()
}

#[tauri::command]
fn sound_data_url(action: String, set: String) -> Option<String> {
    assets::sound_data_url(&action, &set)
}

/// 鲸鱼小窗逻辑坐标（相对主显示器左上角）。前端用它做拖拽/吸附计算。
/// 注意：tauri 的 outer_position/inner_size 返回物理像素，必须先按 DPI 转成逻辑像素，
/// 否则高 DPI 屏上窗口尺寸/位置与前端逻辑坐标对不上（鲸鱼被裁剪/跑偏）。
#[tauri::command]
fn get_window_bounds(app: tauri::AppHandle) -> Value {
    match app.get_webview_window("main") {
        Some(win) => {
            let sf = win.scale_factor().unwrap_or(1.0).max(0.1);
            let pos = win
                .outer_position()
                .map(|p| (p.x as f64 / sf, p.y as f64 / sf))
                .unwrap_or((0.0, 0.0));
            let size = win
                .inner_size()
                .map(|s| (s.width as f64 / sf, s.height as f64 / sf))
                .unwrap_or((0.0, 0.0));
            json!({ "x": pos.0, "y": pos.1, "width": size.0, "height": size.1 })
        }
        None => json!({ "x": 0.0, "y": 0.0, "width": 0.0, "height": 0.0 }),
    }
}

/// 主显示器逻辑尺寸（物理像素 ÷ DPI）。前端做吸附/越界 clamp 用。
#[tauri::command]
fn screen_size(app: tauri::AppHandle) -> Value {
    match app.primary_monitor() {
        Ok(Some(m)) => {
            let s = m.size();
            let sf = m.scale_factor().max(0.1);
            json!({ "width": s.width as f64 / sf, "height": s.height as f64 / sf })
        }
        _ => json!({ "width": 1280.0, "height": 720.0 }),
    }
}

/// 移动主窗口到逻辑坐标 (x, y)。
#[tauri::command]
fn move_window(app: tauri::AppHandle, x: f64, y: f64) -> Result<(), String> {
    let Some(win) = app.get_webview_window("main") else {
        return Ok(());
    };
    win.set_position(tauri::LogicalPosition::new(x, y))
        .map_err(|e| e.to_string())
}

/// 设置主窗口位置 + 尺寸（逻辑坐标）。前端在 scale 变化 / 菜单弹出 / 初始化时调用。
#[tauri::command]
fn set_window_bounds(
    app: tauri::AppHandle,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let Some(win) = app.get_webview_window("main") else {
        return Ok(());
    };
    win.set_position(tauri::LogicalPosition::new(x, y))
        .and_then(|()| win.set_size(tauri::LogicalSize::new(width, height)))
        .map_err(|e| e.to_string())
}
