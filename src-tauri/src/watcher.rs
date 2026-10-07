//! 本地会话日志监视：独立状态、完整行缓冲、真实名称与最新内容。
use crate::{session::SessionState, tail::FileTail};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};

fn publish(app: &tauri::AppHandle, source: &str, status: Value) {
    let event = if status["tone"] == "work" {
        "agent.working"
    } else {
        "task.completed"
    };
    if crate::config::event_mapping(event).enabled {
        crate::notify::notify_session(app, source, status);
    }
}
fn heartbeat(path: &Path, message: &str) {
    use std::io::Write;
    if std::fs::metadata(path)
        .map(|m| m.len() > 100_000)
        .unwrap_or(false)
    {
        let _ = std::fs::write(path, "");
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(file, "{stamp} {message}");
    }
}
#[derive(Default)]
struct WatchedSession {
    tail: FileTail,
    state: SessionState,
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
                out.push(path);
            }
        }
    }
}
pub fn start_codex_watcher(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let root = PathBuf::from(home).join(".codex/sessions");
        let mut sessions: HashMap<PathBuf, WatchedSession> = HashMap::new();
        let mut startup = true;
        let mut displayed: Option<(String, Value)> = None;
        loop {
            let names = crate::session::load_names();
            let mut paths = Vec::new();
            walk(&root, &mut paths);
            paths.sort();
            let mut latest: Option<(String, Value)> = None;
            for path in paths {
                let new_file = !sessions.contains_key(&path);
                let session = sessions.entry(path.clone()).or_default();
                if std::fs::metadata(&path)
                    .map(|m| m.len() < session.tail.offset)
                    .unwrap_or(false)
                {
                    session.state = SessionState::default();
                }
                let records = session.tail.read(&path);
                let had_records = !records.is_empty();
                let mut changed = false;
                for v in records {
                    changed |= session.state.ingest(&v);
                }
                if startup && new_file {
                    // 初始化名称/任务上下文，但不重放历史通知。
                    session.state.working = false;
                    continue;
                }
                if had_records {
                    session.state.quiet_polls = 0;
                } else if session.state.working && !session.state.explicit_lifecycle {
                    session.state.quiet_polls += 1;
                    if session.state.quiet_polls >= 5 {
                        session.state.working = false;
                        changed = true;
                    }
                }
                if changed {
                    let status = session.state.status(&names);
                    let candidate = (session.state.timestamp.clone(), status);
                    if latest
                        .as_ref()
                        .map(|old| candidate.0 >= old.0)
                        .unwrap_or(true)
                    {
                        latest = Some(candidate);
                    }
                }
            }
            startup = false;
            if let Some((timestamp, status)) = latest {
                // 静默回退的旧会话完成不得覆盖更新会话的工作状态。
                if displayed
                    .as_ref()
                    .map(|(ts, _)| timestamp >= *ts)
                    .unwrap_or(true)
                {
                    publish(&app, "codex", status.clone());
                    displayed = Some((timestamp, status));
                }
            } else if let Some((_, status)) = displayed.as_mut() {
                // 会话重命名时原地更新，不把任务摘要当真实名称。
                if let Some(name) = status["threadId"].as_str().and_then(|id| names.get(id)) {
                    if status["title"].as_str() != Some(name) {
                        status["title"] = Value::String(name.clone());
                        publish(&app, "codex", status.clone());
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}
pub fn start_zcode_watcher(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let root = PathBuf::from(&home).join(".zcode/cli/rollout");
        let log = PathBuf::from(&home).join(".config/deskbuddy/watcher.log");
        heartbeat(&log, "watcher 启动");
        let mut poll = 0u32;
        let mut sessions: HashMap<PathBuf, WatchedSession> = HashMap::new();
        let mut startup = true;
        let mut displayed: Option<Value> = None;
        loop {
            poll += 1;
            if poll % 30 == 0 {
                heartbeat(&log, "watcher 心跳");
            }
            let names = crate::session::load_source_names("zcode");
            let mut latest: Option<(String, Value)> = None;
            let mut paths = Vec::new();
            walk(&root, &mut paths);
            paths.sort();
            for path in paths {
                if !path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .starts_with("model-io-sess_")
                {
                    continue;
                }
                let new_file = !sessions.contains_key(&path);
                let session = sessions.entry(path.clone()).or_default();
                if startup && new_file {
                    // ZCode 每次模型记录含消息上下文，无需读取全部历史。
                    session.tail.skip_history(&path);
                    continue;
                }
                if std::fs::metadata(&path)
                    .map(|m| m.len() < session.tail.offset)
                    .unwrap_or(false)
                {
                    session.state = SessionState::default();
                }
                for v in session.tail.read(&path) {
                    let finish = v
                        .pointer("/response/finishReason")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let event = match finish {
                        "tool-calls" => "agent.working",
                        "stop" => "task.completed",
                        _ => continue,
                    };
                    if !crate::config::event_mapping(event).enabled {
                        continue;
                    }
                    let state = &mut session.state;
                    state.ingest_zcode(&v);
                    let mut status = state.status(&names);
                    if state.first_task.is_empty() && !names.contains_key(&state.id) {
                        status["title"] = Value::String("ZCode 会话".into());
                    }
                    crate::mood::apply_event(&app, event);
                    let candidate = (state.timestamp.clone(), status);
                    if latest
                        .as_ref()
                        .map(|old| candidate.0 >= old.0)
                        .unwrap_or(true)
                    {
                        latest = Some(candidate);
                    }
                }
            }
            startup = false;
            if let Some((_, status)) = latest {
                publish(&app, "zcode", status.clone());
                displayed = Some(status);
            } else if let Some(status) = displayed.as_mut() {
                if let Some(name) = status["threadId"].as_str().and_then(|id| names.get(id)) {
                    if status["title"].as_str() != Some(name) {
                        status["title"] = Value::String(name.clone());
                        publish(&app, "zcode", status.clone());
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}
