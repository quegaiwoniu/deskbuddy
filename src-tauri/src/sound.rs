//! 声音提示：首次运行合成两枚轻量提示音（无素材依赖），事件经 afplay 播放

use std::path::PathBuf;

fn sounds_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".config/deskbuddy/sounds")
}

/// 写 16bit PCM 单声道 WAV
fn write_wav(path: &std::path::Path, samples: &[f32], rate: u32) {
    let n = samples.len() as u32;
    let mut data = Vec::with_capacity(44 + (n * 2) as usize);
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&(36 + n * 2).to_le_bytes());
    data.extend_from_slice(b"WAVEfmt ");
    data.extend_from_slice(&16u32.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes()); // PCM
    data.extend_from_slice(&1u16.to_le_bytes()); // mono
    data.extend_from_slice(&rate.to_le_bytes());
    data.extend_from_slice(&(rate * 2).to_le_bytes());
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&16u16.to_le_bytes());
    data.extend_from_slice(b"data");
    data.extend_from_slice(&(n * 2).to_le_bytes());
    for s in samples {
        let v = ((*s).clamp(-1.0, 1.0) * 32767.0) as i16;
        data.extend_from_slice(&v.to_le_bytes());
    }
    let _ = std::fs::write(path, data);
}

/// 简易音符：正弦 + 指数衰减
fn note(freq: f32, dur: f32, rate: u32) -> Vec<f32> {
    let n = (dur * rate as f32) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / rate as f32;
            (2.0 * std::f32::consts::PI * freq * t).sin() * (-6.0 * t).exp() * 0.6
        })
        .collect()
}

fn concat(parts: &[Vec<f32>]) -> Vec<f32> {
    let mut out = Vec::new();
    for p in parts {
        out.extend_from_slice(p);
    }
    out
}

/// 首次运行合成提示音（完成=上行双音；失败=下行低鸣）
pub fn seed_sounds() {
    let dir = sounds_dir();
    let _ = std::fs::create_dir_all(&dir);
    let rate = 44100u32;
    let complete = dir.join("complete.wav");
    if !complete.exists() {
        let gap = vec![0.0; (0.03 * rate as f32) as usize];
        let wav = concat(&[note(880.0, 0.10, rate), gap.clone(), note(1174.7, 0.16, rate)]);
        write_wav(&complete, &wav, rate);
    }
    let failed = dir.join("failed.wav");
    if !failed.exists() {
        // 下行扫频（220→150Hz）
        let n = (0.22 * rate as f32) as usize;
        let wav: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / rate as f32;
                let f = 220.0 - (220.0 - 150.0) * (t / 0.22);
                (2.0 * std::f32::consts::PI * f * t).sin() * (-5.0 * t).exp() * 0.55
            })
            .collect();
        write_wav(&failed, &wav, rate);
    }
}

fn sound_enabled() -> bool {
    crate::config::BEHAVIOR
        .read()
        .unwrap()
        .as_ref()
        .map(|b| b.sound.enabled)
        .unwrap_or(true)
}

/// 事件音（task.completed/task.failed 有声，其余静默；非阻塞）
pub fn play(event: &str) {
    if !sound_enabled() {
        return;
    }
    let name = match event {
        "task.completed" => "complete.wav",
        "task.failed" => "failed.wav",
        _ => return,
    };
    let path = sounds_dir().join(name);
    if path.exists() {
        let _ = std::process::Command::new("afplay")
            .arg(&path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}
