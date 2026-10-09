//! 本地事件服务器（127.0.0.1:17321 + token 鉴权）+ 悬停观察线程

use std::time::Duration;

use tauri::{Emitter, Manager};

use crate::notify::{check_frontmost_clear, NOTIFY_SOURCE};

/// 本地时间戳 HH:MM:SS（UTC+8）
fn chrono_like_now() -> String {
    let s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let l = (s % 86_400 + 8 * 3600) % 86_400;
    format!("{:02}:{:02}:{:02}", l / 3600, l % 3600 / 60, l % 60)
}

fn config_dir() -> std::path::PathBuf {
    crate::paths::config_dir()
}

fn token() -> String {
    let path = config_dir().join("token");
    if let Ok(t) = std::fs::read_to_string(&path) {
        return t.trim().to_string();
    }
    let t = format!(
        "{}{}",
        std::process::id(),
        std::time::Instant::now().elapsed().as_nanos()
    );
    let _ = std::fs::create_dir_all(config_dir());
    let _ = std::fs::write(&path, &t);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    t
}

/// 事件 → (动作, 气泡)。映射来自 ~/.config/deskbuddy/events.json（{title} 插值）
fn map_event(event: &str, title: &str, action_override: Option<&str>) -> (Option<String>, Option<String>) {
    let m = crate::config::event_mapping(event);
    let clean = title.replace('\n', " ").trim().to_string();
    let action = action_override.map(String::from).or(m.action);
    let bubble = m.bubble.map(|tpl| {
        let fallback = match event {
            "task.completed" => "任务完成",
            "task.failed" => "任务失败",
            _ => "",
        };
        let t = if clean.is_empty() { fallback.to_string() } else { clean.clone() };
        tpl.replace("{title}", &t)
    });
    (action, bubble)
}

pub fn start_event_server(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let server = match tiny_http::Server::http("127.0.0.1:17321") {
            Ok(s) => s,
            Err(e) => {
                eprintln!("事件服务器启动失败: {e}");
                return;
            }
        };
        let tok = token();
        for mut request in server.incoming_requests() {
            let method = request.method().clone();
            let url = request.url().trim_end_matches('/').to_string();
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);
            let authed = request.headers().iter().any(|h| {
                h.field.as_str().as_str().eq_ignore_ascii_case("authorization")
                    && h.value.as_str().contains(&tok)
            });
            if url != "/health" {
                fe_log("api", &format!("{method} {url} {body}"));
            }
            let (status, resp) = if url == "/health" && method.as_str() == "GET" {
                (200, "{\"ok\":true}".to_string())
            } else if url == "/fe-log" && method.as_str() == "POST" {
                // 前端黑匣子：无鉴权（仅本机回环）
                let v = serde_json::from_str::<serde_json::Value>(&body).unwrap_or_default();
                let k = v.get("kind").and_then(|x| x.as_str()).unwrap_or("?").to_string();
                let d = v.get("detail").and_then(|x| x.as_str()).unwrap_or("").to_string();
                fe_log(&k, &d);
                (200, "{\"ok\":true}".to_string())
            } else if !authed {
                (401, "{\"error\":\"unauthorized\"}".to_string())
            } else {
                match (method.as_str(), url.as_str()) {
                    ("POST", "/play") => {
                        let action = serde_json::from_str::<serde_json::Value>(&body)
                            .ok()
                            .and_then(|v| v.get("action").and_then(|a| a.as_str()).map(String::from));
                        match action {
                            Some(a) => {
                                let _ = app.emit("server-event", serde_json::json!({"action": a}));
                                (200, "{\"ok\":true}".to_string())
                            }
                            None => (400, "{\"error\":\"need action\"}".to_string()),
                        }
                    }
                    ("POST", "/say") => {
                        let text = serde_json::from_str::<serde_json::Value>(&body)
                            .ok()
                            .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(String::from));
                        match text {
                            Some(t) => {
                                let _ = app.emit("server-event", serde_json::json!({"bubble": t}));
                                (200, "{\"ok\":true}".to_string())
                            }
                            None => (400, "{\"error\":\"need text\"}".to_string()),
                        }
                    }
                    ("POST", "/status") => {
                        let v = serde_json::from_str::<serde_json::Value>(&body).unwrap_or_default();
                        // 结构化（icon/title/detail/tone）或纯文本（text）皆可
                        let rich = v.get("title").is_some();
                        let text = if rich {
                            Some(String::new())
                        } else {
                            v.get("text").and_then(|t| t.as_str()).map(String::from)
                        };
                        let source = v.get("source").and_then(|s| s.as_str()).map(String::from);
                        // Codex 逻辑：来源应用已是前台（用户正看着）→ 不弹通知
                        // 诊断魔法字（REDBOX/CLEARBOX）豁免抑制——它们就是用来看的
                        let magic = v.get("text").and_then(|t| t.as_str()).map(|t| t == "REDBOX" || t == "CLEARBOX").unwrap_or(false);
                        let suppressed = !magic && source.as_deref().map(crate::notify::source_is_frontmost).unwrap_or(false);
                        match (text, suppressed) {
                            (Some(_), false) => {
                                *NOTIFY_SOURCE.lock().unwrap() = source;
                                if rich {
                                    let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
                                    let _ = app.emit("server-event", serde_json::json!({
                                        "status": {"icon": g("icon"), "title": g("title"), "detail": g("detail"), "tone": g("tone")}
                                    }));
                                } else {
                                    let _ = app.emit("server-event", serde_json::json!({"status": v.get("text").and_then(|x| x.as_str()).unwrap_or("")}));
                                }
                                (200, "{\"ok\":true}".to_string())
                            }
                            (Some(_), true) => (200, "{\"ok\":true,\"suppressed\":true}".to_string()),
                            (None, _) => (400, "{\"error\":\"need text\"}".to_string()),
                        }
                    }
                    ("POST", "/move") => {
                        let v = serde_json::from_str::<serde_json::Value>(&body).unwrap_or_default();
                        let x = v.get("x").and_then(|x| x.as_f64()).unwrap_or(720.0);
                        let y = v.get("y").and_then(|y| y.as_f64()).unwrap_or(450.0);
                        if let Some(win) = app.get_webview_window("main") {
                            let scale = win.scale_factor().unwrap_or(2.0);
                            let _ = win.set_position(tauri::PhysicalPosition::new(
                                (x * scale) as i32,
                                (y * scale) as i32,
                            ));
                        }
                        (200, "{\"ok\":true}".to_string())
                    }
                    ("POST", "/settings") => {
                        crate::settings_cmd::open_settings(app.clone());
                        (200, "{\"ok\":true}".to_string())
                    }
                    ("POST", "/pet") => {
                        let id = serde_json::from_str::<serde_json::Value>(&body)
                            .ok()
                            .and_then(|v| v.get("id").and_then(|i| i.as_str()).map(String::from));
                        match id.as_deref().map(|i| crate::config::resolve_pet_base(&app, i)).flatten() {
                            Some(base) => {
                                if let Some(pid) = id.as_deref() {
                                    crate::config::set_current_pet(pid);
                                }
                                let _ = app.emit("switch-pet", serde_json::json!({"base": base}));
                                (200, "{\"ok\":true}".to_string())
                            }
                            None => (404, "{\"error\":\"pet not found\"}".to_string()),
                        }
                    }
                    ("POST", "/events") => {
                        let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                        let event = v.get("event").and_then(|e| e.as_str()).unwrap_or("").to_string();
                        let title = v.get("title").and_then(|t| t.as_str()).unwrap_or("").to_string();
                        let action = v.get("action").and_then(|a| a.as_str());
                        crate::mood::apply_event(&app, &event);
                        let (act, _legacy) = map_event(&event, &title, action);
                        let src = v.get("source").and_then(|x| x.as_str()).unwrap_or("");
                        let names = crate::session::load_source_names(src);
                        let rich = crate::session::event_status(&v, &names);
                        if src == "zcode" || src == "codex" {
                            // 有对应应用的来源：持久显示，切回该应用时清除（Codex unread 语义）
                            // 例外：处理中是实时状态，看着也显示
                            let live = event == "agent.working";
                            if live || !crate::notify::source_is_frontmost(src) {
                                *crate::notify::NOTIFY_SOURCE.lock().unwrap() = if live { None } else { Some(src.to_string()) };
                                let _ = app.emit("server-event", serde_json::json!({"action": act, "status": rich}));
                            } else if event == "task.completed" || event == "task.failed" {
                                let _ = app.emit("server-event", serde_json::json!({"clear":true, "action":act}));
                            }
                        } else {
                            let _ = app.emit("server-event", serde_json::json!({"action": act, "rich": rich}));
                        }
                        (200, "{\"ok\":true}".to_string())
                    }
                    _ => (404, "{\"error\":\"not found\"}".to_string()),
                }
            };
            let response = tiny_http::Response::from_string(resp)
                .with_status_code(status)
                .with_header(
                    tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                )
                .with_header(
                    tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap(),
                );
            let _ = request.respond(response);
        }
    });
}

/// 前端就绪标记（黑匣子 kind=ready 置位；看门狗据此自愈）
pub static FE_READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 前端黑匣子日志 + 就绪登记（IPC 命令与 HTTP 端点共用）
pub fn fe_log(kind: &str, detail: &str) {
    use std::io::Write;
    if kind == "ready" {
        FE_READY.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    let dir = crate::paths::config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("fe.log");
    // 轮转：超过 100KB 重写（对齐 watcher.log；/fe-log 端点无鉴权，可被本地进程刷盘）
    if std::fs::metadata(&path).map(|m| m.len() > 100_000).unwrap_or(false) {
        let _ = std::fs::File::create(&path);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "[{}] {} {}", chrono_like_now(), kind, detail);
    }
}

/// 悬停观察：非激活应用收不到鼠标移动事件，改为原生轮询全局鼠标位置做命中判断；
/// 顺带每 ~1 秒做一次"看了就消"前台检查；并推送视线方向（四向，仅近距且变化时）
pub fn start_hover_observer(win: tauri::WebviewWindow) {
    std::thread::spawn(move || {
        let mut inside = false;
        let mut look = String::new();
        let tick = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        loop {
            std::thread::sleep(Duration::from_millis(120));
            let w = win.clone();
            let tick_c = tick.clone();
            let (tx, rx) = std::sync::mpsc::channel::<(bool, String)>();
            let ok = win
                .run_on_main_thread(move || {
                    let (hit, dir) = probe_hover(&w);
                    let _ = tx.send((hit, dir));
                    let n = tick_c.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if n % 8 == 0 {
                        check_frontmost_clear(&w.app_handle().clone());
                    }
                    // 持续重申跨空间/全屏/层级标志（曾被外部重置→窗口消失于其他空间）
                    #[cfg(target_os = "macos")]
                    if n % 83 == 0 {
                        crate::window_patch::patch_mac_window(&w);
                    }
                })
                .is_ok();
            if !ok {
                continue;
            }
            let Ok((now_inside, dir)) = rx.recv_timeout(Duration::from_secs(1)) else {
                continue;
            };
            if now_inside != inside {
                inside = now_inside;
                let _ = win.emit(if inside { "hover-enter" } else { "hover-leave" }, ());
            }
            // 方向变化才推送（空=离开近距区，前端回正）
            if dir != look && !crate::drag::is_dragging() {
                look = dir.clone();
                let _ = win.emit("look", &dir);
            }
        }
    });
}

/// 全局鼠标相对主窗口的命中与视线方向（macOS：NS 坐标 y 向上）
#[cfg(target_os = "macos")]
fn probe_hover(win: &tauri::WebviewWindow) -> (bool, String) {
    use objc2_app_kit::{NSEvent, NSWindow};
    unsafe {
        let Ok(handle) = win.ns_window() else { return (false, String::new()) };
        let ns = &*(handle as *mut NSWindow);
        let loc = NSEvent::mouseLocation();
        let f = ns.frame();
        // 窗口加宽后：仅中心带（±95pt）算命中，两侧空白不触发悬停
        let cx = f.origin.x + f.size.width / 2.0;
        let inside = (loc.x - cx).abs() < 95.0
            && loc.y >= f.origin.y
            && loc.y <= f.origin.y + f.size.height;
        // 视线：鼠标在窗口中心 300pt 半径内 → 四向转头（NS 坐标 y 向上）
        let cy = f.origin.y + f.size.height / 2.0;
        let dx = loc.x - cx;
        let dy = loc.y - cy;
        let mut dir = String::new();
        if dx * dx + dy * dy < 300.0 * 300.0 {
            dir = if dx.abs() > dy.abs() * 1.2 {
                if dx > 0.0 { "right" } else { "left" }.to_string()
            } else if dy > 0.0 {
                "up".to_string()
            } else {
                "down".to_string()
            };
        }
        (inside, dir)
    }
}

/// 全局鼠标相对主窗口的命中与视线方向（Windows：物理像素，y 向下）
#[cfg(target_os = "windows")]
fn probe_hover(win: &tauri::WebviewWindow) -> (bool, String) {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let (Ok(pos), Ok(size)) = (win.outer_position(), win.outer_size()) else {
        return (false, String::new());
    };
    let mut point = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut point) } == 0 {
        return (false, String::new());
    }
    let scale = win.scale_factor().unwrap_or(1.0) as f64;
    let (w, h) = (size.width as f64, size.height as f64);
    let (mx, my) = (point.x as f64, point.y as f64);
    // 仅中心带（±95pt×scale）算命中，两侧空白不触发悬停
    let cx = pos.x as f64 + w / 2.0;
    let cy = pos.y as f64 + h / 2.0;
    let inside = (mx - cx).abs() < 95.0 * scale && my >= pos.y as f64 && my <= pos.y as f64 + h;
    // 视线：鼠标在窗口中心 300pt×scale 半径内 → 四向转头（屏幕坐标 y 向下）
    let (dx, dy) = (mx - cx, my - cy);
    let radius = 300.0 * scale;
    let mut dir = String::new();
    if dx * dx + dy * dy < radius * radius {
        dir = if dx.abs() > dy.abs() * 1.2 {
            if dx > 0.0 { "right" } else { "left" }.to_string()
        } else if dy > 0.0 {
            "down".to_string()
        } else {
            "up".to_string()
        };
    }
    (inside, dir)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn probe_hover(_win: &tauri::WebviewWindow) -> (bool, String) {
    (false, String::new())
}

#[cfg(test)]
mod tests {
    use super::map_event;
    use crate::config::{EventMapping, EVENTS};
    use std::collections::BTreeMap;
    static EVENT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    // 
    // {title} 丢了或没清洗换行都会直接上屏
    // 
    #[test]
    fn bubble_interpolates_title_and_cleans_newlines() {
        let _guard = EVENT_TEST_LOCK.lock().unwrap();
        let mut mood = BTreeMap::new();
        mood.insert("joy".to_string(), 15.0);
        *EVENTS.write().unwrap() = Some([(
            "task.completed".to_string(),
            EventMapping {
                mood,
                action: Some("jumping".into()),
                bubble: Some("✓ {title}".into()),
                enabled: true,
            },
        )].into_iter().collect());

        let (action, bubble) = map_event("task.completed", "自测\n第二行", None);
        assert_eq!(action.as_deref(), Some("jumping"));
        assert_eq!(bubble.as_deref(), Some("✓ 自测 第二行")); // 换行被清洗
    }

    #[test]
    fn empty_title_uses_event_fallback() {
        let _guard = EVENT_TEST_LOCK.lock().unwrap();
        *EVENTS.write().unwrap() = Some([(
            "task.failed".to_string(),
            EventMapping { mood: Default::default(), action: None, bubble: Some("✗ {title}".into()), enabled: true },
        )].into_iter().collect());
        let (_, bubble) = map_event("task.failed", "", None);
        assert_eq!(bubble.as_deref(), Some("✗ 任务失败")); // 空标题回退事件默认文案
    }

    #[test]
    fn action_override_wins_over_mapping() {
        let _guard = EVENT_TEST_LOCK.lock().unwrap();
        *EVENTS.write().unwrap() = Some([(
            "task.completed".to_string(),
            EventMapping { mood: Default::default(), action: Some("jumping".into()), bubble: None, enabled: true },
        )].into_iter().collect());
        let (action, _) = map_event("task.completed", "x", Some("waving".into()));
        assert_eq!(action.as_deref(), Some("waving"));
    }
}
