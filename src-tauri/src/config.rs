//! 行为与事件配置：~/.config/deskbuddy/behavior.json + events.json
//! 加载 → 缺省合并 → mtime 轮询热重载 → 推送前端

use std::path::PathBuf;
use std::sync::RwLock;
use std::time::Duration;

use tauri::{Emitter, Manager};

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug)]
#[serde(default)]
pub struct CarouselConfig {
    pub min_interval_sec: u64,
    pub trigger_probability: f64,
    pub weights: std::collections::BTreeMap<String, std::collections::BTreeMap<String, u32>>,
}

impl Default for CarouselConfig {
    fn default() -> Self {
        let mut weights = std::collections::BTreeMap::new();
        let mut normal = std::collections::BTreeMap::new();
        normal.insert("waving".to_string(), 3);
        normal.insert("jumping".to_string(), 1);
        // crawl（自主爬动）会挪动窗口位置，默认不参与轮播；想要的话可在 behavior.json 手动加回权重
        let mut happy = std::collections::BTreeMap::new();
        happy.insert("jumping".to_string(), 3);
        happy.insert("waving".to_string(), 2);
        let mut sad = std::collections::BTreeMap::new();
        sad.insert("waving".to_string(), 2);
        sad.insert("jumping".to_string(), 1);
        weights.insert("happy".to_string(), happy);
        weights.insert("normal".to_string(), normal);
        weights.insert("sad".to_string(), sad);
        Self {
            min_interval_sec: 20,
            trigger_probability: 0.3,
            weights,
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug)]
#[serde(default)]
pub struct HoverConfig {
    pub enabled: bool,
    pub action: String,
    pub guard_ms: u64,
}

impl Default for HoverConfig {
    fn default() -> Self {
        Self { enabled: true, action: "jumping".to_string(), guard_ms: 600 }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug)]
#[serde(default)]
pub struct MoodConfig {
    pub regression_per_min: f64,
    pub neutral: f64,
}

impl Default for MoodConfig {
    fn default() -> Self {
        Self { regression_per_min: 2.0, neutral: 50.0 }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Default)]
#[serde(default)]
pub struct BehaviorConfig {
    pub carousel: CarouselConfig,
    pub hover: HoverConfig,
    pub bubble: BubbleConfig,
    pub mood: MoodConfig,
    pub sound: SoundConfig,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug)]
#[serde(default)]
pub struct BubbleConfig {
    pub duration_sec: u64,
}

impl Default for BubbleConfig {
    fn default() -> Self {
        Self { duration_sec: 6 }
    }
}


#[derive(Clone, serde::Serialize, serde::Deserialize, Debug)]
#[serde(default)]
pub struct SoundConfig {
    pub enabled: bool,
}

impl Default for SoundConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// events.json 单条映射：mood 增量 + 动作 + 气泡模板（{title} 插值）；enabled=false 关闭该事件反应
#[derive(Clone, serde::Serialize, serde::Deserialize, Debug)]
#[serde(default)]
pub struct EventMapping {
    pub mood: std::collections::BTreeMap<String, f64>,
    pub action: Option<String>,
    pub bubble: Option<String>,
    pub enabled: bool,
}

impl Default for EventMapping {
    fn default() -> Self {
        Self { mood: Default::default(), action: None, bubble: None, enabled: true }
    }
}

pub type EventsConfig = std::collections::BTreeMap<String, EventMapping>;

pub static BEHAVIOR: RwLock<Option<BehaviorConfig>> = RwLock::new(None);
pub static EVENTS: RwLock<Option<EventsConfig>> = RwLock::new(None);

fn config_dir() -> PathBuf {
    crate::paths::config_dir()
}

/// 首次运行时把默认配置落盘（已存在则不动）
fn seed_defaults() {
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let b = dir.join("behavior.json");
    if !b.exists() {
        let _ = std::fs::write(&b, serde_json::to_string_pretty(&BehaviorConfig::default()).unwrap());
    }
    let e = dir.join("events.json");
    if !e.exists() {
        let mut ev: EventsConfig = Default::default();
        let mut m = std::collections::BTreeMap::new();
        m.insert("joy".to_string(), 15.0);
        m.insert("energy".to_string(), 10.0);
        ev.insert("task.completed".into(), EventMapping { mood: m, action: Some("jumping".into()), bubble: Some("✓ {title}".into()), enabled: true });
        let mut m = std::collections::BTreeMap::new();
        m.insert("joy".to_string(), -20.0);
        ev.insert("task.failed".into(), EventMapping { mood: m, action: Some("failed".into()), bubble: Some("✗ {title}".into()), enabled: true });
        let mut m = std::collections::BTreeMap::new();
        m.insert("energy".to_string(), 5.0);
        ev.insert("agent.working".into(), EventMapping { mood: m, action: Some("running".into()), bubble: None, enabled: true });
        ev.insert("agent.waiting".into(), EventMapping { mood: Default::default(), action: Some("waiting".into()), bubble: Some("在等你确认：{title}".into()), enabled: true });
        let _ = std::fs::write(&e, serde_json::to_string_pretty(&ev).unwrap());
    }
}

fn load_behavior() -> BehaviorConfig {
    match std::fs::read_to_string(config_dir().join("behavior.json")) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => BehaviorConfig::default(),
    }
}

fn load_events() -> EventsConfig {
    match std::fs::read_to_string(config_dir().join("events.json")) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => EventsConfig::default(),
    }
}

/// 初始化 + 热重载（mtime 轮询 2s），变更时推送前端
pub fn init_and_watch(app: tauri::AppHandle) {
    seed_defaults();
    *BEHAVIOR.write().unwrap() = Some(load_behavior());
    *EVENTS.write().unwrap() = Some(load_events());
    let _ = app.emit("behavior", BEHAVIOR.read().unwrap().clone().unwrap());
    std::thread::spawn(move || {
        let b_path = config_dir().join("behavior.json");
        let e_path = config_dir().join("events.json");
        let mut b_mtime = std::fs::metadata(&b_path).and_then(|m| m.modified()).ok();
        let mut e_mtime = std::fs::metadata(&e_path).and_then(|m| m.modified()).ok();
        loop {
            std::thread::sleep(Duration::from_millis(2000));
            let nb = std::fs::metadata(&b_path).and_then(|m| m.modified()).ok();
            if nb != b_mtime {
                b_mtime = nb;
                let cfg = load_behavior();
                let _ = app.emit("behavior", cfg.clone());
                *BEHAVIOR.write().unwrap() = Some(cfg);
                log_line("behavior.json 热重载".into());
            }
            let ne = std::fs::metadata(&e_path).and_then(|m| m.modified()).ok();
            if ne != e_mtime {
                e_mtime = ne;
                *EVENTS.write().unwrap() = Some(load_events());
                log_line("events.json 热重载".into());
            }
        }
    });
}

fn log_line(s: String) {
    use std::io::Write;
    let p = config_dir().join("watcher.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = writeln!(f, "{s}");
    }
}

/// 当前 events 映射（无配置时给等价默认）
pub fn event_mapping(event: &str) -> EventMapping {
    let guard = EVENTS.read().unwrap();
    if let Some(ev) = guard.as_ref() {
        return ev.get(event).cloned().unwrap_or_default();
    }
    EventMapping::default()
}

/// 外部宠物包扫描：~/.config/deskbuddy/pets/<id>/pet.json
pub fn scan_external_pets() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let dir = config_dir().join("pets");
    let Ok(entries) = std::fs::read_dir(&dir) else { return out };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.join("pet.json").is_file() {
            continue;
        }
        let id = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        if id.is_empty() {
            continue;
        }
        let name = std::fs::read_to_string(path.join("pet.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.get("displayName").and_then(|d| d.as_str()).map(String::from))
            .unwrap_or_else(|| id.clone());
        out.push((id, name));
    }
    out
}

/// 内置宠物包扫描：打包资源目录 pets/<id>/pet.json（无预置角色时为空）
pub fn scan_builtin_pets(app: &tauri::AppHandle) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(dir) = app.path().resource_dir().ok().map(|r| r.join("pets")) else {
        return out;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else { return out };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.join("pet.json").is_file() {
            continue;
        }
        let id = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        if id.is_empty() {
            continue;
        }
        let name = std::fs::read_to_string(path.join("pet.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.get("displayName").and_then(|d| d.as_str()).map(String::from))
            .unwrap_or_else(|| id.clone());
        out.push((id, name));
    }
    out.sort();
    out
}

/// 宠物 id → 加载基址（builtin: 前缀=打包内资源；ext: 前缀=外部目录绝对路径）
pub fn resolve_pet_base(app: &tauri::AppHandle, id: &str) -> Option<String> {
    if id.is_empty() {
        return None;
    }
    if let Ok(res) = app.path().resource_dir() {
        let p = res.join("pets").join(id).join("pet.json");
        if p.is_file() {
            return Some(format!("builtin:/pets/{id}"));
        }
    }
    let dir = config_dir().join("pets").join(id);
    if dir.join("pet.json").is_file() {
        Some(format!("ext:{}", dir.display()))
    } else {
        None
    }
}

/// 当前宠物（持久化于 current-pet 文件；无保存或失效时回退第一个内置→第一个外部→空）
pub fn current_pet_base(app: &tauri::AppHandle) -> String {
    let saved = std::fs::read_to_string(config_dir().join("current-pet")).unwrap_or_default();
    let id = saved.trim();
    if !id.is_empty() {
        if let Some(base) = resolve_pet_base(app, id) {
            return base;
        }
    }
    if let Some((id, _)) = scan_builtin_pets(app).into_iter().next() {
        return format!("builtin:/pets/{id}");
    }
    if let Some((id, _)) = scan_external_pets().into_iter().next() {
        return format!("ext:{}", config_dir().join("pets").join(id).display());
    }
    String::new()
}

pub fn set_current_pet(id: &str) {
    let _ = std::fs::write(config_dir().join("current-pet"), id);
}
