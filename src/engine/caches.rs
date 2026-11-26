//! R23: the cache sizes Preferences › Video sets, read by the video worker: the Dynamic RAM Preview's cap (VEGAS's
//! "Dynamic RAM Preview max (MB)") and the disk frame cache for heavy sources (Milan Pro's own, off by default).
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use crate::settings::Prefs;

static RAM_BYTES: AtomicU64 = AtomicU64::new(200 << 20);
static DISK_BYTES: AtomicU64 = AtomicU64::new(0);
static DISK_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Preferences changed (or were loaded).
pub fn configure(p: &Prefs) {
    RAM_BYTES.store((p.ram_preview_mb as u64) << 20, Relaxed);
    DISK_BYTES.store((p.disk_cache_mb as u64) << 20, Relaxed);
}

/// Bytes the Dynamic RAM Preview may hold.
pub fn ram_bytes() -> usize {
    RAM_BYTES.load(Relaxed) as usize
}

/// Bytes the disk frame cache may hold (0: off).
pub fn disk_bytes() -> u64 {
    DISK_BYTES.load(Relaxed)
}

/// Where the disk frame cache lives: `%LOCALAPPDATA%\MilanPro\framecache` unless a test set another folder.
pub fn disk_dir() -> Option<PathBuf> {
    if let Some(d) = DISK_DIR.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        return Some(d);
    }
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("MilanPro").join("framecache"))
}

/// Tests: the disk cache's folder and size.
#[cfg(test)]
pub fn set_disk(dir: Option<PathBuf>, bytes: u64) {
    *DISK_DIR.lock().unwrap_or_else(|e| e.into_inner()) = dir;
    DISK_BYTES.store(bytes, Relaxed);
}

/// Tests: the RAM preview's cap.
#[cfg(test)]
pub fn set_ram(bytes: u64) {
    RAM_BYTES.store(bytes, Relaxed);
}
