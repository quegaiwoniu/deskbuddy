//! 心情状态机：joy/energy 0~100（中性 50），事件增减、随时间回归、分类推送前端

use std::sync::Mutex;
use std::time::Duration;

use tauri::Emitter;

use crate::config::{event_mapping, BEHAVIOR};

#[derive(Clone, Copy, Debug)]
struct MoodState {
    joy: f64,
    energy: f64,
}

static MOOD: Mutex<MoodState> = Mutex::new(MoodState { joy: 50.0, energy: 50.0 });

pub fn category_of(joy: f64) -> &'static str {
    if joy >= 70.0 {
        "happy"
    } else if joy <= 40.0 {
        "sad"
    } else {
        "normal"
    }
}

/// 事件影响心情（events.json 的 mood 增量），类别变化时推送前端
pub fn apply_event(app: &tauri::AppHandle, event: &str) {
    crate::sound::play(event);
    let m = event_mapping(event);
    let mut st = MOOD.lock().unwrap();
    let before = category_of(st.joy);
    for (key, delta) in &m.mood {
        let v = if key == "joy" { &mut st.joy } else { &mut st.energy };
        *v = (*v + delta).clamp(0.0, 100.0);
    }
    let after = category_of(st.joy);
    if before != after {
        let _ = app.emit("mood", serde_json::json!({"category": after, "joy": st.joy.round(), "energy": st.energy.round()}));
    }
}

/// 回归线程：每 10 秒向中性回归（regression_per_min 来自 behavior.json）
pub fn start(app: tauri::AppHandle) {
    // 启动时推送一次初始类别
    {
        let st = *MOOD.lock().unwrap();
        let _ = app.emit("mood", serde_json::json!({"category": category_of(st.joy), "joy": st.joy.round(), "energy": st.energy.round()}));
    }
    std::thread::spawn(move || {
        let mut last_cat = category_of(MOOD.lock().unwrap().joy);
        loop {
            std::thread::sleep(Duration::from_secs(10));
            let (reg, neutral) = {
                let guard = BEHAVIOR.read().unwrap();
                let b = guard.as_ref();
                (b.map(|c| c.mood.regression_per_min).unwrap_or(2.0) / 6.0,
                 b.map(|c| c.mood.neutral).unwrap_or(50.0))
            };
            let mut st = MOOD.lock().unwrap();
            st.joy += (neutral - st.joy).signum() * reg.min((neutral - st.joy).abs());
            st.energy += (neutral - st.energy).signum() * reg.min((neutral - st.energy).abs());
            let cat = category_of(st.joy);
            if cat != last_cat {
                last_cat = cat;
                let _ = app.emit("mood", serde_json::json!({"category": cat, "joy": st.joy.round(), "energy": st.energy.round()}));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::category_of;

    // 
    // 
    #[test]
    fn category_of_boundaries() {
        assert_eq!(category_of(70.0), "happy"); // 边界含 70
        assert_eq!(category_of(69.9), "normal");
        assert_eq!(category_of(50.0), "normal");
        assert_eq!(category_of(40.1), "normal");
        assert_eq!(category_of(40.0), "sad"); // 边界含 40
        assert_eq!(category_of(0.0), "sad");
        assert_eq!(category_of(100.0), "happy");
    }
}
