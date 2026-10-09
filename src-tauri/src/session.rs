//! 会话身份、内容与回合状态（与窗口无关，便于验证）。
use serde_json::{json, Value};
use std::collections::HashMap;

pub fn clean(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
pub fn parse_names(text: &str) -> HashMap<String, String> {
    let mut names = HashMap::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let (Some(id), Some(name)) = (v["id"].as_str(), v["thread_name"].as_str()) {
            let name = clean(name);
            if !name.is_empty() {
                names.insert(id.to_string(), name);
            }
        }
    }
    names
}
pub fn load_names() -> HashMap<String, String> {
    let path = crate::paths::home_dir().join(".codex/session_index.jsonl");
    parse_names(&std::fs::read_to_string(path).unwrap_or_default())
}
/// ZCode 的真实标题位于本机 SQLite；只读查询失败时回退到任务摘要。
pub fn load_source_names(source: &str) -> HashMap<String, String> {
    if source != "zcode" {
        return load_names();
    }
    let path = crate::paths::home_dir().join(".zcode/cli/db/db.sqlite");
    if !path.is_file() {
        return HashMap::new();
    }
    // macOS 自带 /usr/bin/sqlite3；Windows 无内置 CLI，尝试 PATH 中的 sqlite3
    let exe = if cfg!(target_os = "windows") { "sqlite3" } else { "/usr/bin/sqlite3" };
    let Ok(output) = std::process::Command::new(exe)
        .args(["-readonly", "-json"])
        .arg(path)
        .arg("SELECT id, title FROM session;")
        .output()
    else {
        return HashMap::new();
    };
    if !output.status.success() {
        return HashMap::new();
    }
    let Ok(rows) = serde_json::from_slice::<Vec<Value>>(&output.stdout) else {
        return HashMap::new();
    };
    rows.into_iter()
        .filter_map(|v| {
            let id = v["id"].as_str()?.to_string();
            let title = clean(v["title"].as_str()?);
            if title.is_empty() {
                None
            } else {
                Some((id, title))
            }
        })
        .collect()
}
fn valid_task(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('<')
        && !s.starts_with('[')
        && !s.starts_with("The following is the Codex agent history")
}
fn message_text(p: &Value) -> String {
    let Some(content) = p["content"].as_array() else {
        return String::new();
    };
    clean(
        &content
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join(" "),
    )
}
#[derive(Default)]
pub struct SessionState {
    pub id: String,
    pub internal: bool,
    pub first_task: String,
    pub task: String,
    pub turn_id: String,
    pub reply: String,
    pub working: bool,
    pub explicit_lifecycle: bool,
    pub quiet_polls: u32,
    pub timestamp: String,
}
impl SessionState {
    pub fn ingest_zcode(&mut self, v: &Value) {
        self.id = v["sessionId"].as_str().unwrap_or("").into();
        let turn = v["turnId"].as_str().unwrap_or("");
        if !turn.is_empty() && turn != self.turn_id {
            self.reply.clear();
            self.turn_id = turn.into();
        }
        if let Some(messages) = v.pointer("/request/messages").and_then(Value::as_array) {
            for message in messages.iter().filter(|m| m["role"] == "user") {
                let text = message["content"]
                    .as_str()
                    .map(String::from)
                    .unwrap_or_else(|| message_text(message));
                self.set_task(&text);
            }
        }
        self.working =
            v.pointer("/response/finishReason").and_then(Value::as_str) == Some("tool-calls");
        let text = v
            .pointer("/response/text")
            .and_then(Value::as_str)
            .map(clean)
            .unwrap_or_default();
        if !text.is_empty() {
            self.reply = text;
        }
        self.timestamp = v["completedAt"].as_str().unwrap_or("").into();
    }
    pub fn set_task(&mut self, text: &str) {
        let text = clean(text);
        if valid_task(&text) {
            if self.first_task.is_empty() {
                self.first_task = text.clone();
            }
            self.task = text;
        }
    }
    /// true 表示可展示状态变化；元数据与 token/工具日志不覆盖正文。
    pub fn ingest(&mut self, v: &Value) -> bool {
        if let Some(ts) = v["timestamp"].as_str() {
            self.timestamp = ts.into();
        }
        let p = &v["payload"];
        if v["type"] == "session_meta" {
            self.internal =
                p["thread_source"] == "guardian_review" || p["source"].get("subagent").is_some();
        }
        if self.internal {
            return false;
        }
        match v["type"].as_str().unwrap_or("") {
            "session_meta" => {
                self.id = p["id"]
                    .as_str()
                    .or_else(|| p["session_id"].as_str())
                    .unwrap_or("")
                    .into();
                false
            }
            "event_msg" => match p["type"].as_str().unwrap_or("") {
                "task_started" => {
                    self.explicit_lifecycle = true;
                    self.working = true;
                    self.reply.clear();
                    self.quiet_polls = 0;
                    true
                }
                "task_complete" | "turn_aborted" => {
                    self.explicit_lifecycle = true;
                    self.working = false;
                    if let Some(text) = p["last_agent_message"].as_str() {
                        let text = clean(text);
                        if !text.is_empty() {
                            self.reply = text;
                        }
                    }
                    if p["type"] == "turn_aborted" {
                        self.reply = "任务已中断".into();
                    }
                    true
                }
                // 兼容旧版会话日志。
                "user_message" => {
                    if let Some(text) = p["message"].as_str() {
                        self.set_task(text);
                        self.reply.clear();
                        self.working = true;
                        return true;
                    }
                    false
                }
                "agent_message" => {
                    if let Some(text) = p["message"].as_str() {
                        self.reply = clean(text);
                        if p["phase"] == "final_answer" {
                            self.working = false;
                        }
                        return !self.reply.is_empty();
                    }
                    false
                }
                _ => false,
            },
            "response_item" if p["type"] == "message" => {
                let text = message_text(p);
                if text.is_empty() {
                    return false;
                }
                match p["role"].as_str().unwrap_or("") {
                    "user" if valid_task(&text) => {
                        self.set_task(&text);
                        self.reply.clear();
                        self.working = true;
                        true
                    }
                    "assistant" => {
                        self.reply = text;
                        if p["phase"] == "final_answer" {
                            self.working = false;
                        } else {
                            self.working = true;
                        }
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }
    pub fn status(&self, names: &HashMap<String, String>) -> Value {
        let title = names
            .get(&self.id)
            .filter(|s| !s.is_empty())
            .map(String::as_str)
            .unwrap_or(if self.first_task.is_empty() {
                "Codex 会话"
            } else {
                &self.first_task
            });
        let detail = if !self.reply.is_empty() {
            &self.reply
        } else if self.working && !self.task.is_empty() {
            &self.task
        } else if self.working {
            "正在处理…"
        } else {
            "任务完成，等待查看"
        };
        json!({"icon":if self.working { "" } else { "✓" },
            "title":title, "detail":detail,
            "tone":if self.working { "work" } else { "good" }, "threadId":self.id})
    }
}
/// 兼容旧事件：title 仍表示事件说明，新字段显式区分会话名称与正文。
pub fn event_status(v: &Value, names: &HashMap<String, String>) -> Value {
    let event = v["event"].as_str().unwrap_or("");
    let source = v["source"].as_str().unwrap_or("");
    let id = v["threadId"].as_str().unwrap_or("");
    let (icon, tone, fallback) = match event {
        "task.completed" => ("✓", "good", "任务完成，等待查看"),
        "task.failed" => ("✗", "bad", "任务失败"),
        "agent.waiting" => ("⏸", "wait", "需要输入"),
        "agent.working" => ("", "work", "正在处理…"),
        _ => ("", "plain", "新消息"),
    };
    let name = v["threadName"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| names.get(id).map(String::as_str));
    let title = name.unwrap_or(match source {
        "codex" => "Codex 会话",
        "zcode" => "ZCode 会话",
        "git" => "Git",
        _ => "桌伴",
    });
    let detail = v["detail"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| v["title"].as_str().filter(|s| !s.trim().is_empty()))
        .unwrap_or(fallback);
    json!({"icon":icon,"title":clean(title),"detail":clean(detail),"tone":tone,"threadId":id})
}

#[cfg(test)]

mod tests {
    use super::*;
    fn message(role: &str, text: &str, phase: &str) -> Value {
        json!({"type":"response_item","payload":{"type":"message","role":role,"phase":phase,"content":[{"text":text}]}})
    }
    #[test]
    fn zcode_tool_only_calls_keep_progress_until_new_turn() {
        let mut s = SessionState::default();
        s.ingest_zcode(&json!({"sessionId":"sess-a","turnId":"turn-1","request":{"messages":[{"role":"user","content":"优化宠物"}]},"response":{"finishReason":"tool-calls","text":"正在检查两行布局"}}));
        s.ingest_zcode(&json!({"sessionId":"sess-a","turnId":"turn-1","request":{"messages":[{"role":"user","content":"优化宠物"}]},"response":{"finishReason":"tool-calls","text":""}}));
        assert_eq!(s.status(&HashMap::new())["detail"], "正在检查两行布局");
        s.ingest_zcode(&json!({"sessionId":"sess-a","turnId":"turn-2","request":{"messages":[{"role":"user","content":"优化宠物"},{"role":"user","content":"检查动画"}]},"response":{"finishReason":"tool-calls","text":""}}));
        assert_eq!(s.status(&HashMap::new())["detail"], "检查动画");
    }
    #[test]
    fn internal_approval_sessions_never_become_pet_notifications() {
        let mut s = SessionState::default();
        s.ingest(&json!({"type":"session_meta","payload":{"id":"guardian","thread_source":"guardian_review","source":{"subagent":{"other":"guardian"}}}}));
        assert!(!s.ingest(&json!({"type":"event_msg","payload":{"type":"task_started"}})));
        assert!(!s.ingest(&message(
            "user",
            "The following is the Codex agent history whose request action you are assessing.",
            ""
        )));
        assert!(!s.ingest(&message("assistant", "allow", "final_answer")));
        assert!(s.first_task.is_empty());
    }
    #[test]
    fn names_use_latest_valid_record() {
        let names = parse_names("{\"id\":\"a\",\"thread_name\":\"旧名\"}\ninvalid\n{\"id\":\"a\",\"thread_name\":\"新名\"}\n");
        assert_eq!(names.get("a").map(String::as_str), Some("新名"));
    }
    #[test]
    fn progress_and_completion_keep_real_name() {
        let mut s = SessionState::default();
        s.ingest(&json!({"type":"session_meta","payload":{"id":"a"}}));
        s.ingest(&json!({"type":"event_msg","payload":{"type":"task_started"}}));
        assert!(s.ingest(&message("assistant", "正在检查布局", "commentary")));
        let names = HashMap::from([("a".into(), "优化设置面板".into())]);
        assert_eq!(s.status(&names)["title"], "优化设置面板");
        assert_eq!(s.status(&names)["detail"], "正在检查布局");
        s.ingest(&json!({"type":"event_msg","payload":{"type":"task_complete","last_agent_message":"布局已调整"}}));
        assert_eq!(s.status(&names)["tone"], "good");
        assert_eq!(s.status(&names)["detail"], "布局已调整");
    }
    #[test]
    fn interleaved_sessions_and_turns_do_not_share_replies() {
        let mut a = SessionState::default();
        let mut b = SessionState::default();
        a.ingest(&message("user", "任务A", ""));
        b.ingest(&message("user", "任务B", ""));
        a.ingest(&message("assistant", "A完成", "final_answer"));
        assert_eq!(b.status(&HashMap::new())["detail"], "任务B");
        a.ingest(&json!({"type":"event_msg","payload":{"type":"task_started"}}));
        a.ingest(&message("user", "A新任务", ""));
        assert_eq!(a.status(&HashMap::new())["detail"], "A新任务");
        assert_eq!(a.status(&HashMap::new())["title"], "任务A");
    }
    #[test]
    fn hooks_keep_name_and_reply_separate_and_legacy_events_work() {
        let names = HashMap::from([("a".into(), "会话A".into())]);
        let st = event_status(
            &json!({"event":"task.completed","source":"codex","threadId":"a","detail":"真正的回复"}),
            &names,
        );
        assert_eq!(st["title"], "会话A");
        assert_eq!(st["detail"], "真正的回复");
        let legacy = event_status(
            &json!({"event":"task.completed","source":"git","title":"提交成功"}),
            &HashMap::new(),
        );
        assert_eq!(legacy["detail"], "提交成功");
    }
    #[test]
    fn ignores_injected_user_messages_and_final_phase_finishes_turn() {
        let mut s = SessionState::default();
        s.ingest(&message(
            "user",
            "<environment_context>注入</environment_context>",
            "",
        ));
        s.ingest(&message("user", "真实任务", ""));
        s.ingest(&message("assistant", "最终回复", "final_answer"));
        assert_eq!(s.status(&HashMap::new())["title"], "真实任务");
        assert_eq!(s.status(&HashMap::new())["tone"], "good");
    }
}
