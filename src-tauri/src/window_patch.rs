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
