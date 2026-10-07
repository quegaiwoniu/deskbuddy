use serde_json::Value;
use std::path::Path;
#[derive(Default)]
pub struct FileTail {
    pub offset: u64,
    partial: Vec<u8>,
}
impl FileTail {
    /// 跳过历史完整记录，但保留正在写入的尾行，包括半个 UTF-8 字符。
    pub fn skip_history(&mut self, path: &Path) {
        use std::io::{Read, Seek, SeekFrom};
        let Ok(mut file) = std::fs::File::open(path) else {
            return;
        };
        let Ok(len) = file.metadata().map(|m| m.len()) else {
            return;
        };
        let mut end = len;
        let mut trailing = Vec::new();
        while end > 0 {
            let start = end.saturating_sub(4096);
            if file.seek(SeekFrom::Start(start)).is_err() {
                return;
            }
            let mut chunk = vec![0; (end - start) as usize];
            if file.read_exact(&mut chunk).is_err() {
                return;
            }
            if let Some(pos) = chunk.iter().rposition(|b| *b == b'\n') {
                self.partial = chunk[pos + 1..].to_vec();
                self.partial.extend(trailing);
                self.offset = len;
                return;
            }
            chunk.extend(trailing);
            trailing = chunk;
            end = start;
        }
        self.offset = len;
        self.partial = trailing;
    }
    pub fn read(&mut self, path: &Path) -> Vec<Value> {
        use std::io::{Read, Seek, SeekFrom};
        let Ok(mut file) = std::fs::File::open(path) else {
            return Vec::new();
        };
        let Ok(len) = file.metadata().map(|m| m.len()) else {
            return Vec::new();
        };
        if len < self.offset {
            self.offset = 0;
            self.partial.clear();
        }
        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            return Vec::new();
        }
        let mut chunk = Vec::new();
        if file.read_to_end(&mut chunk).is_err() {
            return Vec::new();
        }
        self.offset += chunk.len() as u64;
        self.partial.extend(chunk);
        let Some(last) = self.partial.iter().rposition(|b| *b == b'\n') else {
            return Vec::new();
        };
        let records = self.partial[..=last]
            .split(|b| *b == b'\n')
            .filter_map(|line| serde_json::from_slice(line).ok())
            .collect();
        self.partial.drain(..=last);
        records
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn startup_retains_partial_record_without_replaying_history() {
        let path = std::env::temp_dir().join(format!(
            "deskbuddy-tail-bootstrap-{}.jsonl",
            std::process::id()
        ));
        let pending = "{\"text\":\"宝宝\"}\n".as_bytes();
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(b"{\"old\":true}\n").unwrap();
        file.write_all(&pending[..10]).unwrap();
        let mut tail = FileTail::default();
        tail.skip_history(&path);
        assert!(tail.read(&path).is_empty());
        file.write_all(&pending[10..]).unwrap();
        let records = tail.read(&path);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["text"], "宝宝");
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn buffers_partial_utf8_and_resets_after_truncation() {
        let path =
            std::env::temp_dir().join(format!("deskbuddy-tail-{}.jsonl", std::process::id()));
        let mut f = std::fs::File::create(&path).unwrap();
        let bytes = "{\"text\":\"宝宝\"}\n".as_bytes();
        let split = 10;
        f.write_all(&bytes[..split]).unwrap();
        let mut tail = FileTail::default();
        assert!(tail.read(&path).is_empty());
        f.write_all(&bytes[split..]).unwrap();
        assert_eq!(tail.read(&path)[0]["text"], "宝宝");
        std::fs::write(&path, b"{\"n\":1}\n").unwrap();
        assert_eq!(tail.read(&path)[0]["n"], 1);
        std::fs::remove_file(path).unwrap();
    }
}
