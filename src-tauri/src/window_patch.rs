//! macOS 窗口补丁：NSPanel 转换 + 全屏可见 + 非激活 + 层级诊断

/// 转 NSPanel + 全屏辅助标志 + 弹出菜单层级（悬浮于全屏 App 之上的完整配方）
#[cfg(target_os = "macos")]
pub fn patch_mac_window(win: &tauri::WebviewWindow) {
    use objc2::ClassType;
    use objc2_app_kit::{
        NSPanel, NSPopUpMenuWindowLevel, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
    };
    if let Ok(handle) = win.ns_window() {
        let ns = unsafe { &*(handle as *mut NSWindow) };
        unsafe {
            // NSWindow → NSPanel：浮在其他 App 全屏空间之上的必要条件
            // 
            extern "C" {
                fn object_setClass(obj: *mut std::ffi::c_void, cls: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
            }
            let panel_class = NSPanel::class() as *const _ as *mut std::ffi::c_void;
            object_setClass(handle as *mut std::ffi::c_void, panel_class);
            ns.setLevel(NSPopUpMenuWindowLevel);
            // NSPanel 上 NonactivatingPanel 真正生效：点击不激活本应用
            ns.setStyleMask(ns.styleMask() | NSWindowStyleMask::NonactivatingPanel);
            // 整体替换而非叠加：避免与 MoveToActiveSpace 冲突（全屏可见的关键）
            ns.setCollectionBehavior(
                NSWindowCollectionBehavior::CanJoinAllSpaces
                    | NSWindowCollectionBehavior::FullScreenAuxiliary,
            );
            ns.setAcceptsMouseMovedEvents(true);
            ns.orderFrontRegardless();
        }
    }
}

/// 诊断用：切换窗口层级（找全屏可见的最低档）
#[cfg(target_os = "macos")]
pub fn set_window_level(win: &tauri::WebviewWindow, level: isize) {
    use objc2_app_kit::NSWindow;
    if let Ok(handle) = win.ns_window() {
        unsafe {
            (&*(handle as *mut NSWindow)).setLevel(level);
            (&*(handle as *mut NSWindow)).orderFrontRegardless();
        }
    }
}

/// 窗口钳制入屏（物理像素计算；尺寸变化/旧位置记忆/拖拽都可能出界）
#[cfg(target_os = "macos")]
pub fn clamp_window(win: &tauri::WebviewWindow) {
    use objc2_app_kit::NSScreen;
    let Ok(handle) = win.ns_window() else { return };
    let ns = unsafe { &*(handle as *mut objc2_app_kit::NSWindow) };
    unsafe {
        let mtm = objc2_foundation::MainThreadMarker::new_unchecked();
        let Some(screen) = NSScreen::mainScreen(mtm) else { return };
        let f = ns.frame();
        let vf = screen.visibleFrame();
        // NS 坐标（点，原点左下）——宝宝本体条带（中线±56pt）入屏即可，
        // 窗口透明留白允许出屏（否则宝宝到不了真正的屏幕边缘）
        let mid = f.size.width / 2.0;
        let min_x = vf.origin.x - (mid - 56.0);
        let max_x = vf.origin.x + vf.size.width - (mid + 56.0);
        let min_y = vf.origin.y;
        let max_y = vf.origin.y + vf.size.height - f.size.height;
        let (x, y) = if max_x >= min_x && max_y >= min_y {
            (f.origin.x.clamp(min_x, max_x), f.origin.y.clamp(min_y, max_y))
        } else {
            (min_x, min_y) // 窗口比屏幕还大：贴左下角
        };
        if x != f.origin.x || y != f.origin.y {
            ns.setFrameOrigin(objc2_foundation::NSPoint { x, y });
        }
    }
}

/// 光标所在显示器的工作区（物理像素；排除任务栏）。
/// 多屏拖拽的关键：跟随光标而非窗口选屏，宠物才能被拖过屏幕边界；
/// 用联合矩形会把宠物放进某块屏不存在的坐标区（如主屏下方）。
#[cfg(target_os = "windows")]
pub fn work_area_near(cx: i32, cy: i32) -> Option<(i32, i32, i32, i32)> {
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    let monitor = unsafe { MonitorFromPoint(POINT { x: cx, y: cy }, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_null() {
        return None;
    }
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        rcMonitor: RECT { left: 0, top: 0, right: 0, bottom: 0 },
        rcWork: RECT { left: 0, top: 0, right: 0, bottom: 0 },
        dwFlags: 0,
    };
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return None;
    }
    let r = info.rcWork;
    Some((r.left, r.top, r.right - r.left, r.bottom - r.top))
}

/// 窗口钳制入屏（Windows：宝宝本体条带中线±56pt 保持光标所在屏工作区内，透明留白可出屏）
#[cfg(target_os = "windows")]
pub fn clamp_window(win: &tauri::WebviewWindow) {
    let (Ok(pos), Ok(size)) = (win.outer_position(), win.outer_size()) else {
        return;
    };
    let center = (pos.x + size.width as i32 / 2, pos.y + size.height as i32 / 2);
    let Some((vx, vy, vw, vh)) = work_area_near(center.0, center.1) else { return };
    let scale = win.scale_factor().unwrap_or(1.0) as f64;
    let (w, h) = (size.width as f64, size.height as f64);
    let strip = 56.0 * scale;
    let mid = w / 2.0;
    let min_x = vx as f64 - (mid - strip);
    let max_x = vx as f64 + vw as f64 - (mid + strip);
    let (x, y) = if max_x >= min_x {
        (
            (pos.x as f64).clamp(min_x, max_x),
            (pos.y as f64).clamp(vy as f64, vy as f64 + vh as f64 - h),
        )
    } else {
        (min_x, vy as f64) // 窗口比工作区还宽：贴左上角
    };
    if x != pos.x as f64 || y != pos.y as f64 {
        let _ = win.set_position(tauri::PhysicalPosition::new(
            x.round() as i32,
            y.round() as i32,
        ));
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::work_area_near;

    #[test]
    fn cursor_monitor_work_area_is_valid() {
        let (x, y, w, h) = work_area_near(0, 0).expect("应能取到光标所在屏的工作区");
        assert!(w > 0 && h > 0, "工作区尺寸应为正: {x},{y} {w}x{h}");
    }
}
