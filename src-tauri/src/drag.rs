//! 原生拖动：拖动循环在主线程逐帧跟随鼠标，中间零 IPC（丝滑的关键）

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::Emitter;

static DRAGGING: AtomicBool = AtomicBool::new(false);

/// 拖动进行中（视线跟随等据此让路）
pub fn is_dragging() -> bool {
    DRAGGING.load(Ordering::SeqCst)
}

struct DragState {
    offset_x: f64,
    offset_y: f64,
    prev_x: f64,
    last_dir: i8,
}

static DRAG_STATE: Mutex<Option<DragState>> = Mutex::new(None);

/// 前端爬行/自主爬动位移（逻辑像素 → 物理像素，由内核移动窗口）
#[tauri::command]
pub fn move_by(window: tauri::WebviewWindow, dx: f64, dy: f64) {
    let scale = window.scale_factor().unwrap_or(1.0);
    if let Ok(pos) = window.outer_position() {
        let _ = window.set_position(tauri::PhysicalPosition::new(
            pos.x + (dx * scale).round() as i32,
            pos.y + (dy * scale).round() as i32,
        ));
    }
}

#[tauri::command]
pub fn drag_begin(window: tauri::WebviewWindow) {
    if DRAGGING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        let started = Instant::now();
        while DRAGGING.load(Ordering::SeqCst) && started.elapsed() < Duration::from_secs(30) {
            let win = window.clone();
            let _ = window.run_on_main_thread(move || drag_tick(win));
            std::thread::sleep(Duration::from_millis(8));
        }
        DRAGGING.store(false, Ordering::SeqCst);
        *DRAG_STATE.lock().unwrap() = None;
        let _ = window.emit("drag-end", ());
    });
}

#[tauri::command]
pub fn drag_end() {
    DRAGGING.store(false, Ordering::SeqCst);
}

#[cfg(target_os = "macos")]
fn drag_tick(win: tauri::WebviewWindow) {
    use objc2_app_kit::{NSEvent, NSWindow};

    let mut guard = DRAG_STATE.lock().unwrap();
    if !DRAGGING.load(Ordering::SeqCst) {
        return;
    }
    // 首次进入：创建拖动状态（prev_x 置 NaN 触发偏移记录）
    if guard.is_none() {
        *guard = Some(DragState {
            offset_x: 0.0,
            offset_y: 0.0,
            prev_x: f64::NAN,
            last_dir: 0,
        });
    }
    let Some(st) = guard.as_mut() else { return };
    let Ok(handle) = win.ns_window() else { return };
    unsafe {
        let ns = &*(handle as *mut NSWindow);
        let loc = NSEvent::mouseLocation();
        let frame = ns.frame();
        if st.prev_x.is_nan() {
            // 首帧：记录鼠标相对窗口的偏移，之后窗口跟随鼠标
            st.offset_x = loc.x - frame.origin.x;
            st.offset_y = loc.y - frame.origin.y;
            st.prev_x = frame.origin.x;
        }
        let mut origin = objc2_foundation::NSPoint {
            x: loc.x - st.offset_x,
            y: loc.y - st.offset_y,
        };
        // 钳制：宝宝本体条带（窗口中线±56pt）保持屏内；窗口透明留白允许出屏
        if let Some((sx, sy, sw, sh)) = unsafe {
            let mtm = objc2_foundation::MainThreadMarker::new_unchecked();
            objc2_app_kit::NSScreen::mainScreen(mtm).map(|sc| {
                let f = sc.visibleFrame();
                (f.origin.x, f.origin.y, f.size.width, f.size.height)
            })
        } {
            let (w, h) = (frame.size.width, frame.size.height);
            let mid = w / 2.0;
            let (strip_l, strip_r) = (mid - 56.0, mid + 56.0);
            origin.x = origin.x.clamp(sx - strip_l, sx + sw - strip_r);
            origin.y = origin.y.clamp(sy, sy + sh - h);
        }
        ns.setFrameOrigin(origin);
        let dx = origin.x - st.prev_x;
        st.prev_x = origin.x;
        if dx.abs() > 6.0 {
            let dir: i8 = if dx < 0.0 { -1 } else { 1 };
            if dir != st.last_dir {
                st.last_dir = dir;
                let _ = win.emit("drag-dir", if dir < 0 { "left" } else { "right" });
            }
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn drag_tick(_win: tauri::WebviewWindow) {}

/// Windows：全局光标（物理像素，y 向下）驱动窗口跟随，工作区钳制 + 方向检测
#[cfg(target_os = "windows")]
fn drag_tick(win: tauri::WebviewWindow) {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let mut guard = DRAG_STATE.lock().unwrap();
    if !DRAGGING.load(Ordering::SeqCst) {
        return;
    }
    if guard.is_none() {
        *guard = Some(DragState {
            offset_x: 0.0,
            offset_y: 0.0,
            prev_x: f64::NAN,
            last_dir: 0,
        });
    }
    let Some(st) = guard.as_mut() else { return };
    let Ok(pos) = win.outer_position() else { return };
    let mut point = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut point) } == 0 {
        return;
    }
    let (mx, my) = (point.x as f64, point.y as f64);
    if st.prev_x.is_nan() {
        // 首帧：记录鼠标相对窗口的偏移，之后窗口跟随鼠标
        st.offset_x = mx - pos.x as f64;
        st.offset_y = my - pos.y as f64;
        st.prev_x = pos.x as f64;
    }
    let scale = win.scale_factor().unwrap_or(1.0) as f64;
    let (w, h) = match win.outer_size() {
        Ok(s) => (s.width as f64, s.height as f64),
        Err(_) => (352.0 * scale, 216.0 * scale),
    };
    let mut x = mx - st.offset_x;
    let mut y = my - st.offset_y;
    // 钳制：按光标所在屏选工作区（多屏拖拽的关键），宝宝本体条带（窗口中线±56pt）
    // 保持工作区内；窗口透明留白允许出屏。光标拖过屏幕边界 → 钳制区随之切换。
    if let Some((vx, vy, vw, vh)) = crate::window_patch::work_area_near(point.x, point.y) {
        let strip = 56.0 * scale;
        let mid = w / 2.0;
        let min_x = vx as f64 - (mid - strip);
        let max_x = vx as f64 + vw as f64 - (mid + strip);
        if max_x >= min_x {
            x = x.clamp(min_x, max_x);
        }
        y = y.clamp(vy as f64, vy as f64 + vh as f64 - h);
    }
    let x = x.round() as i32;
    let y = y.round() as i32;
    let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
    let dx = x as f64 - st.prev_x;
    st.prev_x = x as f64;
    if dx.abs() > 6.0 * scale {
        let dir: i8 = if dx < 0.0 { -1 } else { 1 };
        if dir != st.last_dir {
            st.last_dir = dir;
            let _ = win.emit("drag-dir", if dir < 0 { "left" } else { "right" });
        }
    }
}
