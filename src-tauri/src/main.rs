#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// 单实例探测：事件服务器 /health 有应答 = 已有一只完整的桌伴在跑
fn deskbuddy_alive() -> bool {
    use std::io::{Read, Write};
    std::net::TcpStream::connect("127.0.0.1:17321")
        .ok()
        .map(|mut s| {
            let _ = s.write_all(b"GET /health HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n");
            let mut buf = String::new();
            let _ = s.read_to_string(&mut buf);
            buf.contains("\"ok\":true")
        })
        .unwrap_or(false)
}

fn main() {
    // --hidden 是自启参数（当前等价于正常启动），过滤掉避免误入 CLI 分支
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| a != "--hidden")
        .collect();
    if !args.is_empty() {
        deskbuddy_lib::cli_main(&args);
        return;
    }
    // 双开守卫：自启 + 手动双击会各起一只，第二只抢不到事件服务器端口，
    // 变成"活着但没脑子"的僵尸宠物（QA 实测复现）。已有实例应答时直接退出。
    if deskbuddy_alive() {
        eprintln!("DeskBuddy 已在运行，本次启动退出。");
        return;
    }
    deskbuddy_lib::run()
}
