//! 设置面板命令层：GUI ↔ 配置文件。
//! 保存 = 写文件，热重载管线自动广播到桌伴前端（改即生效，无重启）。

use crate::config::{self, BehaviorConfig, EventsConfig};
use tauri::{Emitter, Manager};

fn config_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    std::path::PathBuf::from(home).join(".config/deskbuddy")
}

#[tauri::command]
pub fn get_behavior() -> BehaviorConfig {
    crate::config::BEHAVIOR
        .read()
        .unwrap()
        .clone()
        .unwrap_or_default()
}

#[tauri::command]
pub fn save_behavior(app: tauri::AppHandle, cfg: BehaviorConfig) -> Result<(), String> {
    let path = config_dir().join("behavior.json");
    let json = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    // 热重载管线会在 mtime 变化时广播；这里直接也广播一次，绕过 2s 轮询延迟
    *crate::config::BEHAVIOR.write().unwrap() = Some(cfg.clone());
    let _ = app.emit("behavior", cfg);
    Ok(())
}

#[tauri::command]
pub fn get_events() -> EventsConfig {
    crate::config::EVENTS
        .read()
        .unwrap()
        .clone()
        .unwrap_or_default()
}

#[tauri::command]
pub fn save_events(_app: tauri::AppHandle, cfg: EventsConfig) -> Result<(), String> {
    let path = config_dir().join("events.json");
    let json = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    *crate::config::EVENTS.write().unwrap() = Some(cfg);
    Ok(())
}

#[derive(serde::Serialize)]
pub struct PetEntry {
    pub id: String,
    pub name: String,
    pub current: bool,
    pub draft: bool,
}

#[tauri::command]
pub fn list_pets() -> Vec<PetEntry> {
    let cur = std::fs::read_to_string(config_dir().join("current-pet"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "baby".into());
    let mut out = vec![PetEntry { id: "baby".into(), name: "宝宝".into(), current: cur == "baby", draft: false }];
    for (id, name) in config::scan_external_pets() {
        let draft = config_dir().join("pets").join(&id).join("raw").is_dir()
            && std::fs::read_to_string(config_dir().join("pets").join(&id).join("pet.json"))
                .map(|s| s.contains("\"draft\": true")).unwrap_or(false);
        out.push(PetEntry { current: cur == id, id, name, draft });
    }
    out
}

#[tauri::command]
pub fn select_pet(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let entry = list_pets().into_iter().find(|p| p.id == id).ok_or("角色不存在")?;
    if entry.draft {
        return Err("草稿尚未组装，不能使用".into());
    }
    match config::resolve_pet_base(&id) {
        Some(base) => {
            config::set_current_pet(&id);
            let _ = app.emit("switch-pet", serde_json::json!({"base": base}));
            crate::refresh_tray_menu(&app);
            crate::rebuild_pet_menu(&app).map_err(|e| e.to_string())?;
            Ok(())
        }
        None => Err("角色不存在".into()),
    }
}

#[tauri::command]
pub fn get_autostart(app: tauri::AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
pub fn set_autostart(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let al = app.autolaunch();
    if enabled {
        al.enable().map_err(|e| e.to_string())
    } else {
        al.disable().map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub fn open_settings(app: tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.show();
        let _ = win.set_focus();
    }
}

/// 在访达中打开外部宠物包目录
#[tauri::command]
pub fn open_pets_folder() {
    let dir = config_dir().join("pets");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::process::Command::new("open").arg(dir).spawn();
}

fn deletable_pet_dir(root: &std::path::Path, id: &str) -> Result<std::path::PathBuf, String> {
    if id == "baby" || id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("不可删除此角色".into());
    }
    let dir = root.join(id);
    let meta = std::fs::symlink_metadata(&dir).map_err(|_| "角色不存在".to_string())?;
    if !meta.file_type().is_dir() || meta.file_type().is_symlink() || !dir.join("pet.json").is_file() {
        return Err("角色目录无效".into());
    }
    Ok(dir)
}

#[tauri::command]
pub fn delete_pet(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let dir = deletable_pet_dir(&pets_dir(), &id)?;
    let cur = std::fs::read_to_string(config_dir().join("current-pet")).unwrap_or_default();
    let was_current = cur.trim() == id;
    if was_current {
        config::set_current_pet("baby");
        let _ = app.emit("switch-pet", serde_json::json!({"base": "builtin:/pets/baby"}));
    }
    if let Err(e) = std::fs::remove_dir_all(dir) {
        if was_current {
            config::set_current_pet(&id);
            if let Some(base) = config::resolve_pet_base(&id) {
                let _ = app.emit("switch-pet", serde_json::json!({"base": base}));
            }
        }
        return Err(format!("删除失败：{e}"));
    }
    let _ = app.emit("pets-changed", ());
    crate::refresh_tray_menu(&app);
    Ok(())
}

#[tauri::command]
pub fn show_pet_in_folder(id: String) -> Result<(), String> {
    let dir = deletable_pet_dir(&pets_dir(), &id)?;
    std::process::Command::new("open").arg("-R").arg(dir).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn open_pet_raw_folder(id: String) -> Result<(), String> {
    let dir = deletable_pet_dir(&pets_dir(), &id)?.join("raw");
    if !dir.is_dir() { return Err("该角色没有待组装素材文件夹".into()); }
    std::process::Command::new("open").arg(dir).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

// Resolve only relative assets, rejecting symlinks in both the file and its parents.
fn safe_asset_path(root: &std::path::Path, rel: &str) -> Result<std::path::PathBuf, String> {
    let path = std::path::Path::new(rel);
    if rel.is_empty() || !path.components().all(|c| matches!(c, std::path::Component::Normal(_))) {
        return Err(format!("角色素材路径无效：{rel}"));
    }
    let mut current = root.to_path_buf();
    for part in path.components() {
        current.push(part);
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => return Err(format!("角色素材不能使用符号链接：{rel}")),
            Ok(_) => {},
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(e) => return Err(format!("无法读取角色素材 {rel}：{e}")),
        }
    }
    Ok(root.join(path))
}

// Match the desktop loader's priority and sequential PNG frame probing.
fn runtime_pet_assets(root: &std::path::Path, manifest: &serde_json::Value) -> Result<(serde_json::Value, std::collections::BTreeSet<String>), String> {
    let mut imported = manifest.clone();
    let mut files = std::collections::BTreeSet::new();
    if let Some(actions) = manifest.get("x-actions").and_then(|v| v.as_object()).filter(|a| !a.is_empty()) {
        if !actions.contains_key("idle") { return Err("角色包缺少 idle 待机动作".into()); }
        for (name, action) in actions {
            let dir = action.get("dir").and_then(|v| v.as_str()).ok_or_else(|| format!("动作 {name} 缺少素材目录"))?;
            // The renderer reads 00.png through 41.png, stopping at the first gap.
            for i in 0..=41 {
                let rel = format!("{dir}/{i:02}.png");
                let path = safe_asset_path(root, &rel)?;
                if !path.is_file() {
                    if i == 0 { return Err(format!("动作 {name} 缺少首帧：{rel}")); }
                    break;
                }
                files.insert(rel);
            }
        }
        imported.as_object_mut().unwrap().remove("spritesheetPath");
        imported.as_object_mut().unwrap().remove("staticImage");
    } else {
        let (key, path) = if let Some(path) = manifest.get("spritesheetPath").and_then(|v| v.as_str()) {
            if manifest.get("gridColumns").and_then(|v| v.as_u64()).unwrap_or(0) == 0 {
                return Err("动画图集缺少有效的 gridColumns".into());
            }
            ("spritesheetPath", path)
        } else if let Some(path) = manifest.get("staticImage").and_then(|v| v.as_str()) {
            ("staticImage", path)
        } else { return Err("角色包缺少可用的待机图片或动作帧".into()); };
        if !safe_asset_path(root, path)?.is_file() { return Err(format!("角色素材不存在：{path}")); }
        files.insert(path.to_string());
        imported.as_object_mut().unwrap().remove("x-actions");
        imported.as_object_mut().unwrap().remove(if key == "staticImage" { "spritesheetPath" } else { "staticImage" });
    }
    Ok((imported, files))
}

fn copy_pet_files(src: &std::path::Path, dst: &std::path::Path, manifest: &serde_json::Value) -> Result<serde_json::Value, String> {
    let (imported, files) = runtime_pet_assets(src, manifest)?;
    let mut bytes = serde_json::to_vec(&imported).map_err(|e| e.to_string())?.len() as u64;
    if files.len() + 1 > 512 { return Err("运行素材超过 512 个文件".into()); }
    for rel in &files {
        bytes += std::fs::metadata(safe_asset_path(src, rel)?).map_err(|e| e.to_string())?.len();
        if bytes > 100 * 1024 * 1024 { return Err("运行素材超过 100 MB".into()); }
    }
    for rel in files {
        let from = safe_asset_path(src, &rel)?;
        let to = dst.join(&rel);
        std::fs::create_dir_all(to.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::copy(from, to).map_err(|e| e.to_string())?;
    }
    Ok(imported)
}

fn validate_pet_manifest(root: &std::path::Path, manifest: &serde_json::Value) -> Result<String, String> {
    let name = manifest.get("displayName").and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty()).ok_or("pet.json 缺少 displayName")?;
    runtime_pet_assets(root, manifest)?;
    Ok(name.to_string())
}

#[tauri::command]
pub fn import_pet_folder(app: tauri::AppHandle, source: String) -> Result<String, String> {
    let src = std::path::PathBuf::from(source);
    let meta = std::fs::symlink_metadata(&src).map_err(|e| e.to_string())?;
    if !meta.is_dir() || meta.file_type().is_symlink() { return Err("请选择角色包文件夹".into()); }
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(safe_asset_path(&src, "pet.json")?).map_err(|_| "文件夹根目录缺少 pet.json")?
    ).map_err(|_| "pet.json 格式无效")?;
    let name = validate_pet_manifest(&src, &manifest)?;
    std::fs::create_dir_all(pets_dir()).map_err(|e| e.to_string())?;
    let id = available_pet_id(&pets_dir(), &name);
    let dst = pets_dir().join(&id);
    std::fs::create_dir(&dst).map_err(|e| e.to_string())?;
    let mut imported = match copy_pet_files(&src, &dst, &manifest) {
        Ok(imported) => imported,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dst);
            return Err(e);
        }
    };
    imported["id"] = serde_json::Value::String(id.clone());
    if let Err(e) = std::fs::write(dst.join("pet.json"), serde_json::to_string_pretty(&imported).map_err(|e| e.to_string())?) {
        let _ = std::fs::remove_dir_all(&dst);
        return Err(e.to_string());
    }
    let _ = app.emit("pets-changed", ());
    crate::refresh_tray_menu(&app);
    Ok(id)
}

// ---- 新建宠物向导 ----

fn pets_dir() -> std::path::PathBuf {
    config_dir().join("pets")
}

fn pet_id_from(name: &str) -> String {
    let id: String = name
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .collect();
    let id = id.trim_matches('-').to_string();
    if id.is_empty() {
        format!("pet-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis())
    } else {
        id
    }
}

fn available_pet_id(root: &std::path::Path, name: &str) -> String {
    let base = pet_id_from(name);
    let mut id = base.clone();
    let mut suffix = 2;
    while id == "baby" || root.join(&id).exists() {
        id = format!("{base}-{suffix}");
        suffix += 1;
    }
    id
}

/// 🖼 一张图变宠物：拷图 + 写静态 pet.json
#[tauri::command]
pub fn create_static_pet(app: tauri::AppHandle, name: String, image_path: String) -> Result<String, String> {
    if name.trim().is_empty() { return Err("请填写角色名称".into()); }
    let src = std::path::PathBuf::from(&image_path);
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("png").to_lowercase();
    if !["png", "webp", "jpg", "jpeg"].contains(&ext.as_str()) || !src.is_file() {
        return Err("请选择 PNG、WebP 或 JPEG 图片".into());
    }
    std::fs::create_dir_all(pets_dir()).map_err(|e| e.to_string())?;
    let id = available_pet_id(&pets_dir(), &name);
    let dir = pets_dir().join(&id);
    std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
    let img_name = format!("image.{ext}");
    if let Err(e) = std::fs::copy(&src, dir.join(&img_name)) {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(e.to_string());
    }
    let manifest = serde_json::json!({
        "id": id, "displayName": name, "staticImage": img_name,
        "description": "静态图角色（程序化动效）"
    });
    if let Err(e) = std::fs::write(dir.join("pet.json"), serde_json::to_string_pretty(&manifest).unwrap()) {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(e.to_string());
    }
    let _ = app.emit("pets-changed", ());
    crate::refresh_tray_menu(&app);
    Ok(id)
}

/// ✨ AI 生成向导：目录脚手架 + 完整提示词包（PROMPTS.md）
#[tauri::command]
pub fn create_draft_pet(app: tauri::AppHandle, name: String, description: String) -> Result<String, String> {
    if name.trim().is_empty() || description.trim().is_empty() { return Err("请填写角色名称和描述".into()); }
    std::fs::create_dir_all(pets_dir()).map_err(|e| e.to_string())?;
    let id = available_pet_id(&pets_dir(), &name);
    let dir = pets_dir().join(&id);
    std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::create_dir(dir.join("raw")) {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(e.to_string());
    }
    let manifest = serde_json::json!({
        "id": id, "displayName": name, "draft": true,
        "description": description
    });
    if let Err(e) = std::fs::write(dir.join("pet.json"), serde_json::to_string_pretty(&manifest).unwrap()) {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(e.to_string());
    }
    if let Err(e) = std::fs::write(dir.join("PROMPTS.md"), prompt_pack(&name, &description)) {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(e.to_string());
    }
    let _ = app.emit("pets-changed", ());
    Ok(id)
}

fn prompt_pack(name: &str, desc: &str) -> String {
    format!(r#"# {name} · 角色生成提示词包

> 用法：把下面的提示词逐条喂给 Codex（或任意图像生成工具），
> 生成的 GIF/PNG 放进本目录的 raw/ 子目录（命名含动作名，如 idle.gif、jumping.gif），
> 然后回到 设置 → 陪伴 → 继续制作 → 检查并组装。

## 角色设定（每条提示词都要带上这段）

生成 Q 版桌面角色「{name}」：{desc}。
要求：透明背景 PNG 或 GIF、角色居中、脚底对齐、同一角色在所有图中保持完全一致的造型
（脸型/配色/服饰细节不变）、软萌贴纸风格、适合缩到 112pt 高仍清晰。

## 逐动作提示词（9 个动作，每条独立生成）

1. **idle**（6 帧循环）：{name} 坐姿待机，轻微呼吸起伏，偶尔眨眼微笑
2. **waiting**（6 帧）：{name} 坐着等待，手放腿上，眼神期待地看向观众
3. **running**（6 帧）：{name} 朝正前方小步跑动
4. **running-left**（8 帧）：{name} 向画面左侧爬行/跑动，身体朝左
5. **running-right**（8 帧）：同上但朝右
6. **review**（6 帧）：{name} 专注地低头看（像在看代码），偶尔点头
7. **failed**（8 帧）：{name} 垂头丧气，肩膀垮下，头顶冒小乌云
8. **waving**（4 帧，不循环）：{name} 站起来挥手打招呼，挥完回到自然姿态
9. **jumping**（5 帧，不循环）：{name} 蹲下→起跳→空中张开手臂→落地

## 规格

- 每帧建议 192×208 或等比更大（高清源更佳，组装时会自适应缩放）
- GIF 或逐帧 PNG 皆可；GIF 命名 `<动作>.gif`，PNG 放 `raw/<动作>/00.png` 起编号
"#)
}

/// 组装：raw/ 里的 GIF 拆帧 → 写完整 x-actions → 撤销草稿标记
#[tauri::command]
pub fn assemble_pet(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let dir = pets_dir().join(&id);
    let raw = dir.join("raw");
    if !raw.is_dir() || std::fs::read_dir(&raw).map(|m| m.count()).unwrap_or(0) == 0 {
        return Err("raw/ 目录为空：请先把生成的 GIF/PNG 放进去".into());
    }
    // GIF → 帧
    let out = std::process::Command::new("bash")
        .arg("-c")
        .arg(include_str!("../../scripts/split_pet.sh"))
        .arg("split_pet.sh")
        .arg(&dir)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("拆帧失败: {}", String::from_utf8_lossy(&out.stderr)));
    }
    // PNG 直放 raw/<动作>/ 的情况：搬到 frames/
    for entry in std::fs::read_dir(&raw).into_iter().flatten().flatten() {
        let p = entry.path();
        if p.is_dir() {
            let action = p.file_name().unwrap_or_default().to_string_lossy().to_string();
            let dst = dir.join("frames").join(&action);
            std::fs::create_dir_all(&dst).ok();
            for f in std::fs::read_dir(&p).into_iter().flatten().flatten() {
                std::fs::copy(f.path(), dst.join(f.file_name())).ok();
            }
        }
    }
    // 生成 manifest
    let mut actions = serde_json::Map::new();
    let fps = [("idle",5),("waiting",5),("running",8),("running-left",10),("running-right",10),("review",5),("failed",6),("waving",7),("jumping",10)];
    let one_shot = ["waving", "jumping"];
    if let Ok(entries) = std::fs::read_dir(dir.join("frames")) {
        for e in entries.flatten() {
            let a = e.file_name().to_string_lossy().to_string();
            if !e.path().is_dir() { continue; }
            actions.insert(a.clone(), serde_json::json!({
                "dir": format!("frames/{a}"),
                "fps": fps.iter().find(|(k,_)| *k == a).map(|(_,v)| v).unwrap_or(&6),
                "loop": !one_shot.contains(&a.as_str())
            }));
        }
    }
    if actions.is_empty() {
        return Err("未发现任何动作帧".into());
    }
    let old: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("pet.json")).unwrap_or_default())
            .unwrap_or_default();
    let mut manifest = serde_json::Map::new();
    manifest.insert("id".into(), old.get("id").cloned().unwrap_or(serde_json::json!(id)));
    manifest.insert("displayName".into(), old.get("displayName").cloned().unwrap_or(serde_json::json!(id)));
    manifest.insert("description".into(), old.get("description").cloned().unwrap_or("".into()));
    manifest.insert("x-actions".into(), serde_json::Value::Object(actions));
    let _ = std::fs::remove_file(dir.join("draft"));
    std::fs::write(
        dir.join("pet.json"),
        serde_json::to_string_pretty(&serde_json::Value::Object(manifest)).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    let _ = app.emit("pets-changed", ());
    crate::refresh_tray_menu(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{pet_id_from, deletable_pet_dir, available_pet_id, validate_pet_manifest};

    // 
    // 清洗不当时会生成非法目录名或互相覆盖
    // 
    #[test]
    fn pet_id_sanitizes_and_trims() {
        assert_eq!(pet_id_from("小白cat!"), "cat");
        assert_eq!(pet_id_from("My-Pet_2"), "My-Pet-2");
        assert_eq!(pet_id_from("  --haha--  "), "haha");
    }

    #[test]
    fn pet_id_empty_gets_timestamp_fallback() {
        let id = pet_id_from("!!!");
        assert!(id.starts_with("pet-"), "空清洗结果应回退 pet-<毫秒>，实际 {id}");
        assert!(id.len() > "pet-".len());
    }

    #[test]
    fn deletion_only_accepts_real_direct_children() {
        let root = std::env::temp_dir().join(format!("deskbuddy-delete-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("cat")).unwrap();
        std::fs::write(root.join("cat/pet.json"), "{}").unwrap();
        assert_eq!(deletable_pet_dir(&root, "cat").unwrap(), root.join("cat"));
        assert!(deletable_pet_dir(&root, "baby").is_err());
        assert!(deletable_pet_dir(&root, "../cat").is_err());
        assert!(deletable_pet_dir(&root, "absent").is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn new_pet_ids_never_reuse_existing_directory() {
        let root = std::env::temp_dir().join(format!("deskbuddy-id-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("cat")).unwrap();
        assert_eq!(available_pet_id(&root, "cat"), "cat-2");
        assert_eq!(available_pet_id(&root, "baby"), "baby-2");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn imported_pet_requires_real_safe_asset() {
        let root = std::env::temp_dir().join(format!("deskbuddy-import-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("idle.png"), "png").unwrap();
        assert!(validate_pet_manifest(&root, &serde_json::json!({"displayName":"Cat","staticImage":"idle.png"})).is_ok());
        assert!(validate_pet_manifest(&root, &serde_json::json!({"displayName":"Cat","staticImage":"../idle.png"})).is_err());
        assert!(validate_pet_manifest(&root, &serde_json::json!({"displayName":"Cat","x-actions":{}})).is_err());
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn import_copies_only_runtime_assets() {
        let root = std::env::temp_dir().join(format!("deskbuddy-runtime-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let src = root.join("source");
        let dst = root.join("imported");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        let manifest = serde_json::json!({"displayName":"Photo","staticImage":"image.png"});
        std::fs::write(src.join("pet.json"), manifest.to_string()).unwrap();
        std::fs::write(src.join("image.png"), "runtime image").unwrap();
        std::fs::write(src.join("creation-record.json"), "private record").unwrap();
        std::fs::write(src.join("preview.mp4"), "preview").unwrap();
        super::copy_pet_files(&src, &dst, &manifest).unwrap();
        assert!(dst.join("image.png").is_file());
        assert!(!dst.join("creation-record.json").exists(), "制作记录不应被导入");
        assert!(!dst.join("preview.mp4").exists(), "预览视频不应被导入");
        std::fs::remove_dir_all(root).unwrap();
    }

    fn import_fixture(label: &str) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("deskbuddy-import-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let src = root.join("source");
        let dst = root.join("imported");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        (root, src, dst)
    }

    #[test]
    fn animated_import_keeps_declared_contiguous_frames_and_ignores_records() {
        let (root, src, dst) = import_fixture("frames");
        std::fs::create_dir_all(src.join("frames/idle")).unwrap();
        std::fs::create_dir_all(src.join("frames/waving")).unwrap();
        for rel in ["frames/idle/00.png", "frames/idle/01.png", "frames/idle/03.png", "frames/waving/00.png", "frames/idle/preview.gif", "spritesheet.png"] {
            std::fs::write(src.join(rel), "image").unwrap();
        }
        // Unused production files neither consume the runtime file limit nor get copied.
        std::fs::create_dir(src.join("creation-record")).unwrap();
        for i in 0..600 { std::fs::write(src.join(format!("creation-record/{i}.txt")), "private").unwrap(); }
        let manifest = serde_json::json!({"displayName":"小宝","spritesheetPath":"spritesheet.png","gridColumns":8,"x-actions":{
            "idle":{"dir":"frames/idle","fps":5,"loop":true},"waving":{"dir":"frames/waving","fps":7,"loop":false}
        }});
        let imported = super::copy_pet_files(&src, &dst, &manifest).unwrap();
        assert!(dst.join("frames/idle/00.png").is_file());
        assert!(dst.join("frames/idle/01.png").is_file());
        assert!(dst.join("frames/waving/00.png").is_file());
        assert!(!dst.join("frames/idle/03.png").exists());
        assert!(!dst.join("frames/idle/preview.gif").exists());
        assert!(!dst.join("creation-record").exists());
        assert!(!dst.join("spritesheet.png").exists());
        assert!(imported.get("spritesheetPath").is_none());
        assert!(validate_pet_manifest(&dst, &imported).is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn spritesheet_import_keeps_only_selected_sheet() {
        let (root, src, dst) = import_fixture("sheet");
        std::fs::write(src.join("sheet.png"), "sheet").unwrap();
        std::fs::write(src.join("photo.png"), "unused").unwrap();
        let manifest = serde_json::json!({"displayName":"Sheet","spritesheetPath":"sheet.png","gridColumns":8,"staticImage":"photo.png"});
        let imported = super::copy_pet_files(&src, &dst, &manifest).unwrap();
        assert!(dst.join("sheet.png").is_file());
        assert!(!dst.join("photo.png").exists());
        assert!(imported.get("staticImage").is_none());
        assert!(validate_pet_manifest(&dst, &imported).is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_action_frames_report_the_exact_required_asset() {
        let (root, src, dst) = import_fixture("missing");
        let manifest = serde_json::json!({"displayName":"Missing","x-actions":{"idle":{"dir":"frames/idle"}}});
        let err = super::copy_pet_files(&src, &dst, &manifest).unwrap_err();
        assert!(err.contains("frames/idle/00.png"));
        assert_eq!(std::fs::read_dir(&dst).unwrap().count(), 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn referenced_assets_cannot_traverse_or_use_symlinked_parents() {
        let (root, src, dst) = import_fixture("unsafe");
        std::fs::write(root.join("secret.png"), "private").unwrap();
        let manifest = serde_json::json!({"displayName":"Unsafe","staticImage":"../secret.png"});
        assert!(super::copy_pet_files(&src, &dst, &manifest).is_err());
        #[cfg(unix)] {
            std::os::unix::fs::symlink(&root, src.join("linked")).unwrap();
            let manifest = serde_json::json!({"displayName":"Unsafe","staticImage":"linked/secret.png"});
            assert!(super::copy_pet_files(&src, &dst, &manifest).is_err());
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_size_limit_is_checked_before_copying() {
        let (root, src, dst) = import_fixture("size");
        std::fs::File::create(src.join("large.png")).unwrap().set_len(100 * 1024 * 1024 + 1).unwrap();
        let manifest = serde_json::json!({"displayName":"Large","staticImage":"large.png"});
        assert!(super::copy_pet_files(&src, &dst, &manifest).unwrap_err().contains("100 MB"));
        assert_eq!(std::fs::read_dir(&dst).unwrap().count(), 0);
        std::fs::remove_dir_all(root).unwrap();
    }

}
