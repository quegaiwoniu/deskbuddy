//! 通知状态：来源应用跟踪 + "看了就消"（Codex 的 unread-until-read 逻辑）

use std::sync::Mutex;
use tauri::Emitter;

pub static NOTIFY_SOURCE: Mutex<Option<String>> = Mutex::new(None);
pub static FRONTMOST_BID: Mutex<String> = Mutex::new(String::new());
pub static CURRENT_STATUS: Mutex<String> = Mutex::new(String::new());

pub fn source_bundle_id(source: &str) -> Option<&'static str> {
    match source {
        "zcode" => Some("dev.zcode.app"),
        "codex" => Some("com.openai.codex"),
        _ => None,
    }
}

/// 通知来源应用是否为当前前台（用户正看着 → 不该弹通知）
pub fn source_is_frontmost(source: &str) -> bool {
    let front = FRONTMOST_BID.lock().unwrap().clone();
    !front.is_empty() && source_bundle_id(source) == Some(front.as_str())
}

/// 前台应用监测：来源应用成为前台 → 清除气泡
#[cfg(target_os = "macos")]
pub fn check_frontmost_clear(app: &tauri::AppHandle) {
    use objc2_app_kit::NSWorkspace;
    let front = unsafe { NSWorkspace::sharedWorkspace().frontmostApplication() };
    let Some(front) = front else { return };
    let bid = unsafe { front.bundleIdentifier() };
    let Some(bid) = bid else { return };
    let bid = bid.to_string();
    *FRONTMOST_BID.lock().unwrap() = bid.clone();
    let mut guard = NOTIFY_SOURCE.lock().unwrap();
    if let Some(src) = guard.as_ref() {
        if source_bundle_id(src) == Some(bid.as_str()) {
            *guard = None;
            *CURRENT_STATUS.lock().unwrap() = "空闲".into();
            let _ = app.emit("server-event", serde_json::json!({"clear": true}));
            crate::refresh_tray_menu(app);
        }
    }
}

/// 结构化会话通知：实时工作状态持续显示，结束消息保留前台抑制。
pub fn notify_session(app: &tauri::AppHandle, source: &str, status: serde_json::Value) {
    let working = status["tone"] == "work";
    if working {
        NOTIFY_SOURCE.lock().unwrap().take();
    } else if source_is_frontmost(source) {
        notify_action(app, "jumping");
        *CURRENT_STATUS.lock().unwrap() = "空闲".into();
        let _ = app.emit("server-event", serde_json::json!({"clear":true}));
        crate::refresh_tray_menu(app);
        return;
    } else {
        *NOTIFY_SOURCE.lock().unwrap() = Some(source.into());
    }
    let title = status["title"].as_str().unwrap_or("会话");
    let state = match status["tone"].as_str().unwrap_or("") {
        "work" => "处理中", "wait" => "等待处理", "bad" => "需要关注", "good" => "已完成", _ => "更新",
    };
    *CURRENT_STATUS.lock().unwrap() = format!("{state} · {}", title.chars().take(24).collect::<String>());
    let _ = app.emit("server-event", serde_json::json!({"status":status}));
    crate::refresh_tray_menu(app);
}

/// 动作通知
pub fn notify_action(app: &tauri::AppHandle, action: &str) {
    let _ = app.emit("server-event", serde_json::json!({"action": action}));
}
