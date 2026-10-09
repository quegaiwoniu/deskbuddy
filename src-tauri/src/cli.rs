//! CLI（同一二进制：deskbuddy emit/play/say/status/health）

use std::io::{Read, Write};

fn config_dir() -> std::path::PathBuf {
    crate::paths::config_dir()
}

fn token() -> String {
    std::fs::read_to_string(config_dir().join("token"))
        .map(|t| t.trim().to_string())
        .unwrap_or_default()
}

fn http_call(method: &str, path: &str, body: Option<&str>) -> Result<String, String> {
    let mut stream = std::net::TcpStream::connect("127.0.0.1:17321")
        .map_err(|_| "桌伴未在运行（请先启动 DeskBuddy）".to_string())?;
    let payload = body.unwrap_or("");
    let req = format!(
        "{} {} HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        method,
        path,
        token(),
        payload.len(),
        payload
    );
    stream.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut resp = String::new();
    stream.read_to_string(&mut resp).map_err(|e| e.to_string())?;
    Ok(resp
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default())
}

pub fn cli(args: &[String]) {
    let usage = "用法: deskbuddy emit <事件> [--title 标题] [--source 来源] [--action 动作] [--thread-id 会话ID] [--thread-name 会话名称] [--detail 正文] | play <动作> | say <文本> | status <文本> [--source 来源] | status";
    let cmd = args[0].as_str();
    let result = match cmd {
        "emit" => {
            let event = args.get(1).cloned().unwrap_or_default();
            let mut title = String::new();
            let mut source = String::from("manual");
            let mut action: Option<String> = None;
            let mut thread_id = String::new();
            let mut thread_name = String::new();
            let mut detail = String::new();
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--title" => title = args.get(i + 1).cloned().unwrap_or_default(),
                    "--source" => source = args.get(i + 1).cloned().unwrap_or_default(),
                    "--action" => action = args.get(i + 1).cloned(),
                    "--thread-id" => thread_id = args.get(i + 1).cloned().unwrap_or_default(),
                    "--thread-name" => thread_name = args.get(i + 1).cloned().unwrap_or_default(),
                    "--detail" => detail = args.get(i + 1).cloned().unwrap_or_default(),
                    _ => {}
                }
                i += 2;
            }
            http_call(
                "POST",
                "/events",
                Some(
                    &serde_json::json!({"event": event, "title": title, "source": source, "action": action, "threadId":thread_id, "threadName":thread_name, "detail":detail})
                        .to_string(),
                ),
            )
        }
        "play" => {
            let action = args.get(1).cloned().unwrap_or_default();
            http_call("POST", "/play", Some(&serde_json::json!({"action": action}).to_string()))
        }
        "say" => {
            let text = args.get(1).cloned().unwrap_or_default();
            http_call("POST", "/say", Some(&serde_json::json!({"text": text}).to_string()))
        }
        "status" => {
            if args.len() < 2 {
                http_call("GET", "/health", None)
            } else {
                let mut source: Option<String> = None;
                let mut texts: Vec<&String> = Vec::new();
                let mut i = 1;
                while i < args.len() {
                    if args[i] == "--source" {
                        source = args.get(i + 1).cloned();
                        i += 2;
                    } else {
                        texts.push(&args[i]);
                        i += 1;
                    }
                }
                http_call(
                    "POST",
                    "/status",
                    Some(
                        &serde_json::json!({
                            "text": texts.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" "),
                            "source": source
                        })
                        .to_string(),
                    ),
                )
            }
        }
        "settings" => http_call("POST", "/settings", None),
        "pet" => {
            let id = args.get(1).cloned().unwrap_or_default();
            http_call("POST", "/pet", Some(&serde_json::json!({"id": id}).to_string()))
        }
        _ => {
            println!("{usage}");
            std::process::exit(2);
        }
    };
    match result {
        Ok(body) => println!("{body}"),
        Err(e) => {
            eprintln!("错误: {e}");
            std::process::exit(1);
        }
    }
}
