//! 跨平台用户目录：macOS/Linux 用 $HOME，Windows 用 %USERPROFILE%。
//! HOME 可能被 MSYS/Git Bash 注入 Unix 风格路径（如 /c/Users/x），
//! 原生进程无法使用，因此逐个校验目录真实存在后再采纳。

pub fn home_dir() -> std::path::PathBuf {
    for key in ["HOME", "USERPROFILE"] {
        if let Some(v) = std::env::var_os(key) {
            if !v.is_empty() {
                let p = std::path::PathBuf::from(&v);
                if p.is_dir() {
                    return p;
                }
            }
        }
    }
    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
}

pub fn config_dir() -> std::path::PathBuf {
    // 逐段 join 产出纯本机分隔符：字面量 ".config/deskbuddy" 的正斜杠会被原样保留，
    // 混合分隔符路径 fs 能容忍，但 explorer.exe 解析失败会静默回退打开「文档」
    home_dir().join(".config").join("deskbuddy")
}

// 断言依据是 Windows 语义（正斜杠会让 explorer 失败）；Unix 的本机分隔符就是 /
#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn config_dir_uses_native_separators() {
        let dir = config_dir();
        let s = dir.to_string_lossy();
        assert!(!s.contains('/'), "混合分隔符路径 explorer 无法解析: {s}");
    }
}
