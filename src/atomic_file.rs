// atomic_file.rs — 상태 파일(config.json, API 키, 리플레이 기록)을 안전하게 바꿔 쓴다.
//
// 예전에는 임시 파일을 쓴 뒤 기존 파일을 지우고 이름을 바꿨다. 그 사이에 봇이 죽으면 파일이
// 없어져 다음 시작 때 기본값으로 덮였고(config.json은 예시 설정으로 복사), 동시에 두 번 저장하면
// 같은 임시 파일을 두고 다투다 파일이 사라질 수 있었다. 여기서는 같은 파일 쓰기를 한 번에 하나씩 하고,
// 임시 파일을 디스크에 내린 뒤 rename 한 번으로 바꾼다. rename은 기존 파일을 한 번에 바꿔치기하므로
// 언제 멈춰도 이전 파일이나 새 파일 중 하나가 온전히 남는다.

use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

/// 경로별 잠금과 그 경로에 마지막으로 쓴 스냅샷 번호. 같은 파일 쓰기는 한 번에 하나씩 하고,
/// 다른 파일(config.json과 큰 리플레이 파일 등)은 서로 기다리지 않는다.
static WRITTEN: Mutex<BTreeMap<PathBuf, Arc<Mutex<u64>>>> = Mutex::new(BTreeMap::new());
static NEXT_SEQ: AtomicU64 = AtomicU64::new(1);

/// 스냅샷 순서 번호. 상태 잠금 안에서 스냅샷을 뜰 때 받으면 번호가 클수록 최신이다.
pub fn next_seq() -> u64 {
    NEXT_SEQ.fetch_add(1, Ordering::Relaxed)
}

/// `path`를 `contents`로 바꾼다. `seq`를 주면 이 경로에 이미 쓴 것보다 오래된 스냅샷은
/// 쓰지 않고 `false`를 돌려준다 (잠금 밖에서 따로 저장하는 호출부가 순서를 지키게).
pub fn replace(path: &Path, contents: &[u8], seq: Option<u64>) -> Result<bool> {
    let slot = WRITTEN
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry(path.to_path_buf())
        .or_default()
        .clone();
    let mut last_written = slot.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(seq) = seq
        && *last_written > seq
    {
        return Ok(false);
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("폴더를 만들지 못했습니다: {}", parent.display()))?;
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state.json");
    let temp_path = path.with_file_name(format!("{file_name}.tmp"));
    {
        let mut file = fs::File::create(&temp_path)
            .with_context(|| format!("임시 파일을 만들지 못했습니다: {}", temp_path.display()))?;
        file.write_all(contents)
            .with_context(|| format!("임시 파일을 쓰지 못했습니다: {}", temp_path.display()))?;
        // 이름을 바꾸기 전에 내용을 디스크에 내려, 정전 뒤에도 빈 파일로 바뀌지 않게 한다.
        file.sync_all()
            .with_context(|| format!("임시 파일을 저장하지 못했습니다: {}", temp_path.display()))?;
    }
    fs::rename(&temp_path, path)
        .with_context(|| format!("파일을 교체하지 못했습니다: {}", path.display()))?;
    if let Some(seq) = seq {
        *last_written = seq;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mafia-atomic-{name}-{}-{}",
            std::process::id(),
            next_seq()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn replace_swaps_the_file_and_leaves_no_temp_file() {
        let dir = temp_dir("swap");
        let path = dir.join("config.json");
        assert!(replace(&path, b"{\"a\":1}\n", None).unwrap());
        assert!(replace(&path, b"{\"a\":2}\n", None).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"a\":2}\n");
        assert!(!dir.join("config.json.tmp").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn older_snapshots_never_overwrite_newer_ones() {
        let dir = temp_dir("order");
        let path = dir.join("replays.json");
        let older = next_seq();
        let newer = next_seq();
        assert!(replace(&path, b"new", Some(newer)).unwrap());
        assert!(!replace(&path, b"old", Some(older)).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn concurrent_writers_always_leave_a_complete_newest_file() {
        let dir = temp_dir("concurrent");
        let path = dir.join("state.json");
        let seqs = (0..32).map(|_| next_seq()).collect::<Vec<_>>();
        let newest = *seqs.last().unwrap();
        std::thread::scope(|scope| {
            for &seq in seqs.iter().rev() {
                let path = path.clone();
                scope.spawn(move || {
                    let body = format!("{{\"seq\":{seq},\"pad\":\"{}\"}}", "x".repeat(4096));
                    replace(&path, body.as_bytes(), Some(seq)).unwrap();
                    // 저장 도중 언제 읽어도 파일은 온전한 JSON이어야 한다.
                    let text = fs::read_to_string(&path).unwrap();
                    serde_json::from_str::<serde_json::Value>(&text).unwrap();
                });
            }
        });
        let text = fs::read_to_string(&path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["seq"].as_u64(), Some(newest));
        fs::remove_dir_all(&dir).unwrap();
    }
}
