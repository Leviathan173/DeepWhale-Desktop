#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod assets;
mod balance;
mod config;
mod ledger;
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
            image_data_url,
            sound_data_url,
            set_input_enabled
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, _event| {});
}

/// 全屏透明置顶窗口，铺满主显示器。
fn create_main_window(app: &mut tauri::App) -> tauri::Result<()> {
    let (w, h, x, y) = match app.primary_monitor()? {
        Some(m) => {
            let s = m.size();
            let p = m.position();
            (s.width as f64, s.height as f64, p.x as f64, p.y as f64)
        }
        None => (1280.0, 720.0, 0.0, 0.0),
    };
    let win =
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
            .inner_size(w, h)
            .position(x, y)
            .build()?;
    win.set_ignore_cursor_events(true)?;
    Ok(())
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
                if let Err(e) = open_settings(app) {
                    eprintln!("open settings failed: {e}");
                }
            }
            _ => {}
        })
        .build(app)
        .map(|_| ())
}

fn open_settings(app: &tauri::AppHandle) -> tauri::Result<()> {
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
    .inner_size(460.0, 360.0)
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
    let cfg = config::read(&app.state::<AppState>().dir);
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
    let scale = get("scale").and_then(|v| v.as_f64()).unwrap_or(1.5);
    let sound = get("sound").and_then(|v| v.as_bool()).unwrap_or(true);
    let vol = get("vol").and_then(|v| v.as_f64()).unwrap_or(0.9);
    let sound_set = get("soundSet").and_then(|v| v.as_str()).unwrap_or("duck");
    let usage_mode = get("usageMode")
        .and_then(|v| v.as_str())
        .unwrap_or("ledger");
    let dir = &app.state::<AppState>().dir;
    config::write_prefs(dir, scale, sound, vol, sound_set, usage_mode);
    Ok(json!({ "ok": true }))
}

#[tauri::command]
fn save_credentials(
    app: tauri::AppHandle,
    api_key: Option<String>,
    platform_token: Option<String>,
) -> Value {
    config::write_credentials(&app.state::<AppState>().dir, api_key, platform_token);
    json!({ "ok": true })
}

#[tauri::command]
fn load_credentials(app: tauri::AppHandle) -> Value {
    let cfg = config::read(&app.state::<AppState>().dir);
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

/// 点击穿透开关：true → 鲸鱼/菜单可交互；false → 其余区域鼠标穿透到桌面。
#[tauri::command]
fn set_input_enabled(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("main") {
        win.set_ignore_cursor_events(enabled)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
