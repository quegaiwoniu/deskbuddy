//! 桌伴（DeskBuddy）——应用组装层
//! 模块分工：window_patch 窗口魔法 / drag 原生拖动 / notify 通知状态
//!          server 事件服务器+悬停观察 / watcher 会话文件监视 / cli 命令行

mod cli;
mod config;
mod drag;
mod mood;
mod notify;
mod paths;
mod server;
mod session;
mod tail;
mod settings_cmd;
mod sound;
mod watcher;
mod window_patch;

pub use cli::cli as cli_main;

use tauri::{
    menu::{ContextMenu, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};

/// 右键动作菜单句柄（setup 时构建并 manage）
/// 右键动作菜单句柄（随宠物切换动态重建）
struct PetMenu(std::sync::Mutex<Menu<tauri::Wry>>);

/// 当前宠物基址（启动时前端询问；没有任何角色时返回空串）
#[tauri::command]
fn current_pet(app: tauri::AppHandle) -> String {
    config::current_pet_base(&app)
}

/// 前端黑匣子（IPC 通道，绕开混合内容拦截）
#[tauri::command]
fn fe_log(kind: String, detail: String) {
    server::fe_log(&kind, &detail);
}

/// 右键弹出原生动作菜单
#[tauri::command]
fn popup_menu(app: tauri::AppHandle, window: tauri::WebviewWindow) {
    let state = app.state::<PetMenu>();
    let guard = state.0.lock().unwrap();
    let _ = guard.popup(window.as_ref().window());
}

fn build_pet_menu(app: &tauri::AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let waving = MenuItem::with_id(app, "act-waving", "挥手", true, None::<&str>)?;
    let jumping = MenuItem::with_id(app, "act-jumping", "跳跃", true, None::<&str>)?;
    let crawl_l = MenuItem::with_id(app, "act-running-left", "向左爬", true, None::<&str>)?;
    let crawl_r = MenuItem::with_id(app, "act-running-right", "向右爬", true, None::<&str>)?;
    let sit = MenuItem::with_id(app, "act-idle", "坐下", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let hide = MenuItem::with_id(app, "win-hide", "隐藏桌伴", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "app-quit", "退出 DeskBuddy", true, None::<&str>)?;
    let open_cfg = MenuItem::with_id(app, "open-settings", "设置…", true, None::<&str>)?;
    let actions = Submenu::with_id_and_items(
        app,
        "actions",
        "动作",
        true,
        &[&waving, &jumping, &crawl_l, &crawl_r, &sit],
    )?;
    Menu::with_items(app, &[&actions, &sep, &open_cfg, &hide, &quit])
}

fn build_tray_menu(app: &tauri::AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let visible = app.get_webview_window("main").map(|w| w.is_visible().unwrap_or(true)).unwrap_or(true);
    let toggle = MenuItem::with_id(app, "tray-toggle", if visible { "隐藏桌伴" } else { "显示桌伴" }, true, None::<&str>)?;
    let status = notify::CURRENT_STATUS.lock().unwrap().clone();
    let status = if status.is_empty() { "空闲".to_string() } else { status };
    let status_item = MenuItem::with_id(app, "tray-status", &format!("当前状态：{status}"), false, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let settings = MenuItem::with_id(app, "open-settings", "设置…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "app-quit", "退出 DeskBuddy", true, None::<&str>)?;
    let entries = settings_cmd::list_pets(app.clone());
    let current = entries.iter().find(|p| p.current).map(|p| p.id.clone()).unwrap_or_default();
    let items: Vec<MenuItem<tauri::Wry>> = entries
        .into_iter()
        .filter(|p| !p.draft)
        .map(|pet| {
            let suffix = if pet.builtin { "（内置）" } else { "" };
            let label = if pet.id == current { format!("✓ {}{}", pet.name, suffix) } else { format!("{}{}", pet.name, suffix) };
            MenuItem::with_id(app, &format!("pet-{}", pet.id), &label, true, None::<&str>)
        })
        .collect::<tauri::Result<_>>()?;
    let mut all: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = vec![&toggle, &status_item, &sep];
    let pets = (!items.is_empty())
        .then(|| -> tauri::Result<Submenu<tauri::Wry>> {
            let refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> =
                items.iter().map(|i| i as &dyn tauri::menu::IsMenuItem<tauri::Wry>).collect();
            Submenu::with_id_and_items(app, "pets", "切换角色", true, &refs)
        })
        .transpose()?;
    if let Some(p) = pets.as_ref() {
        all.push(p);
    }
    all.push(&settings);
    all.push(&quit);
    Menu::with_items(app, &all)
}

fn action_label(action: &str) -> &'static str {
    match action {
        "idle" => "坐下",
        "waving" => "挥手",
        "jumping" => "跳跃",
        "running-left" => "向左爬",
        "running-right" => "向右爬",
        "running" => "跑步",
        "review" => "思考",
        "cover-ears" => "捂耳朵",
        "point-forward" => "指前方",
        "shy" => "害羞",
        "waiting" => "坐等",
        _ => "",
    }
}

/// 读取当前宠物的动作名列表（跳过 look-* 视线态，它们由悬停驱动）
fn current_pet_actions(app: &tauri::AppHandle) -> Vec<String> {
    let cfg = paths::config_dir();
    let saved = std::fs::read_to_string(cfg.join("current-pet"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if saved.is_empty() {
        return vec![];
    }
    // 内置角色在打包资源目录，外部角色在配置目录
    let path = app
        .path()
        .resource_dir()
        .ok()
        .map(|r| r.join("pets").join(&saved).join("pet.json"))
        .filter(|p| p.is_file())
        .or_else(|| {
            let p = cfg.join("pets").join(&saved).join("pet.json");
            p.is_file().then_some(p)
        });
    let Some(path) = path else { return vec![] };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            v.get("x-actions")
                .and_then(|a| a.as_object())
                .map(|o| o.keys().cloned().collect())
        })
        .unwrap_or_default()
}

/// 依据当前宠物动作重建右键菜单（宠物切换时调用）
pub(crate) fn rebuild_pet_menu(app: &tauri::AppHandle) -> tauri::Result<()> {
    let actions = current_pet_actions(app);
    let mut items: Vec<MenuItem<tauri::Wry>> = Vec::new();
    let known = ["idle", "waving", "jumping", "running-left", "running-right", "review", "cover-ears", "point-forward", "shy", "waiting"];
    for key in known {
        if actions.iter().any(|a| a == key) {
            items.push(MenuItem::with_id(app, &format!("act-{key}"), action_label(key), true, None::<&str>)?);
        }
    }
    for a in &actions {
        if !known.contains(&a.as_str()) && !a.starts_with("look-") {
            items.push(MenuItem::with_id(app, &format!("act-{a}"), a, true, None::<&str>)?);
        }
    }
    let refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> =
        items.iter().map(|i| i as &dyn tauri::menu::IsMenuItem<tauri::Wry>).collect();
    let menu = Menu::with_items(app, &refs)?;
    let state = app.state::<PetMenu>();
    *state.0.lock().unwrap() = menu;
    Ok(())
}

pub(crate) fn refresh_tray_menu(app: &tauri::AppHandle) {
    if let Some(tray) = app.tray_by_id("main-tray") {
        if let Ok(menu) = build_tray_menu(app) { let _ = tray.set_menu(Some(menu)); }
    }
}

#[tauri::command]
fn set_window_level(window: tauri::WebviewWindow, level: isize) -> Result<(), String> {
    if ![25, 101, 1000].contains(&level) { return Err("无效的窗口层级".into()); }
    let _ = window;
    #[cfg(target_os = "macos")]
    if let Some(main) = window.app_handle().get_webview_window("main") {
        window_patch::set_window_level(&main, level);
    }
    Ok(())
}


/// 读取宠物包文件返回 data URL（绕开 asset:// 跨域导致的 WebGL 画布污染）
#[tauri::command]
fn read_pet_file(app: tauri::AppHandle, rel: String) -> Result<String, String> {
    let clean = rel.trim_start_matches('/');
    if clean.contains("..") {
        return Err("非法路径".into());
    }
    // 打包内资源（builtin: /pets/<id>/... → 资源目录）
    if let Ok(base) = app.path().resource_dir() {
        let p = base.join(clean);
        if p.is_file() {
            return file_to_data_url(&p);
        }
    }
    // 外部宠物目录
    let p = paths::config_dir().join(clean);
    if p.is_file() {
        return file_to_data_url(&p);
    }
    // 缺失文件返回空串（前端据此判定"帧序列结束"），绝不抛错——
    // 帧探测依赖"加载失败=结束"，抛错会让整个宠物加载崩溃（间歇性消失的终极根因）
    Ok(String::new())
}

fn file_to_data_url(p: &std::path::Path) -> Result<String, String> {
    use std::io::Read;
    let mut f = std::fs::File::open(p).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let mime = match p.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "json" => "application/json",
        _ => "application/octet-stream",
    };
    Ok(format!("data:{mime};base64,{}", base64_encode(&buf)))
}

fn base64_encode(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

fn config_dir_has(name: &str) -> bool {
    paths::config_dir().join(name).exists()
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .on_window_event(|window, event| {
            if window.label() == "settings" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(|app| {
            let win = app.get_webview_window("main").expect("main 窗口");
            if let Some(settings) = app.get_webview_window("settings") {
                let _ = settings.set_size(tauri::LogicalSize::new(640.0, 620.0));
            }

            // 显示在所有空间（Spaces）
            let _ = win.set_visible_on_all_workspaces(true);
            let _ = win.set_always_on_top(true);

            // 首次启动放到主显示器右下角（之后由 window-state 插件记忆恢复）
            let data_dir = app.path().app_data_dir()?;
            let config_dir = app.path().app_config_dir()?;
            let restored = data_dir.join(".window-state.json").exists()
                || config_dir.join(".window-state.json").exists();
            if !restored {
                if let Ok(Some(mon)) = win.current_monitor() {
                    let size = mon.size();
                    let scale = mon.scale_factor();
                    let margin = (40.0 * scale) as i32;
                    let _ = win.set_position(tauri::PhysicalPosition::new(
                        size.width as i32 - (170.0 * scale) as i32 - margin,
                        size.height as i32 - (216.0 * scale) as i32 - margin,
                    ));
                }
            }

            // 固定窗口尺寸（位置由 window-state 记忆恢复，尺寸统一归一到 Codex petSize=112 档）
            let _ = win.set_size(tauri::LogicalSize::new(352.0, 216.0));

            // 钳制窗口入屏（window-state 异步恢复位置，三连发覆盖时序）
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            {
                window_patch::clamp_window(&win);
                let w1 = win.clone();
                let w1b = win.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(700));
                    let _ = w1.run_on_main_thread(move || window_patch::clamp_window(&w1b));
                });
                let w2 = win.clone();
                let w2b = win.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(2000));
                    let _ = w2.run_on_main_thread(move || window_patch::clamp_window(&w2b));
                });
            }

            // 安全模式：~/.config/deskbuddy/safe-mode 文件存在时跳过全部窗口魔法
            // （NSPanel/层级/跨空间），以普通窗口姿态排障
            let safe_mode = config_dir_has("safe-mode");
            #[cfg(target_os = "macos")]
            {
                if !safe_mode {
                    window_patch::patch_mac_window(&win);
                } else {
                    let _ = win.set_always_on_top(false);
                    eprintln!("[safe-mode] 跳过窗口补丁");
                }
            }
            // 悬停观察：非激活窗口收不到完整鼠标事件，双平台都用原生全局轮询
            let _ = safe_mode;
            server::start_hover_observer(win.clone());

            // M2：行为/事件配置（热重载）+ 心情状态机 + 本地事件服务器
            config::init_and_watch(app.handle().clone());
            sound::seed_sounds();
            mood::start(app.handle().clone());
            server::start_event_server(app.handle().clone());

            // ZCode 会话文件监视（免 hooks/免新会话，任何对话即时生效）
            watcher::start_zcode_watcher(app.handle().clone());
            watcher::start_codex_watcher(app.handle().clone());

            // 右键动作菜单（只通过 popup 弹出，不占系统菜单栏）
            app.manage(PetMenu(std::sync::Mutex::new(build_pet_menu(app.handle())?)));
            rebuild_pet_menu(app.handle())?;

            // 托盘：隐藏后唯一的召回入口 + 开机自启 + 宠物切换
            let tray_menu = build_tray_menu(app.handle())?;
            TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    handle_menu_event(app, event.id().as_ref());
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        if let Some(win) = tray.app_handle().get_webview_window("main") {
                            if win.is_visible().unwrap_or(true) { let _ = win.hide(); }
                            else { let _ = win.show(); let _ = win.set_focus(); }
                            refresh_tray_menu(tray.app_handle());
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .on_menu_event(|app, event| {
            handle_menu_event(app, event.id().as_ref());
        })
        .invoke_handler(tauri::generate_handler![
            drag::move_by,
            popup_menu,
            current_pet,
            read_pet_file,
            fe_log,
            settings_cmd::get_behavior,
            settings_cmd::save_behavior,
            settings_cmd::get_events,
            settings_cmd::save_events,
            settings_cmd::list_pets,
            settings_cmd::select_pet,
            settings_cmd::delete_pet,
            settings_cmd::show_pet_in_folder,
            settings_cmd::open_pet_raw_folder,
            settings_cmd::import_pet_folder,
            settings_cmd::get_autostart,
            settings_cmd::set_autostart,
            settings_cmd::create_static_pet,
            settings_cmd::create_draft_pet,
            settings_cmd::assemble_pet,
            settings_cmd::open_pets_folder,
            settings_cmd::open_settings,
            set_window_level,
            drag::drag_begin,
            drag::drag_end
        ])
        .run(tauri::generate_context!())
        .expect("deskbuddy 运行失败");
}

fn handle_menu_event(app: &tauri::AppHandle, id: &str) {
    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    match id {
        "app-quit" => app.exit(0),
        "win-hide" => {
            let _ = win.hide();
            refresh_tray_menu(app);
        }
        "tray-toggle" => {
            if win.is_visible().unwrap_or(true) { let _ = win.hide(); }
            else { let _ = win.show(); let _ = win.set_focus(); }
            refresh_tray_menu(app);
        }
        other => {
            if let Some(action) = other.strip_prefix("act-") {
                let _ = app.emit("play-action", action.to_string());
            } else if other == "open-settings" {
                settings_cmd::open_settings(app.clone());
            } else if let Some(pet_id) = other.strip_prefix("pet-") {
                let _ = settings_cmd::select_pet(app.clone(), pet_id.to_string());
            }
        }
    }
}
