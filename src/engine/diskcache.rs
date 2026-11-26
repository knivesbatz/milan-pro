//! R23: a disk cache for decoded frames of heavy sources (HEVC, AV1, 2560 px and up), Milan Pro's own (off by default;
//! Preferences › Video sets its size). A paused seek into such a source costs a decode from the keyframe before it,
//! hundreds of milliseconds on long-GOP media; a frame seen once comes back from `%LOCALAPPDATA%\MilanPro\framecache`
//! instead. Raw RGBA files (8 bytes of size, then the pixels), one per frame, with an `index.json` of what is there,
//! kept under the cap least recently used first. Writes go through a thread, so the worker never waits on the disk.
use eframe::egui::{Color32, ColorImage};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Entry {
    bytes: u64,
    used: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct Index {
    /// Entries by key (hex), with the tick they were last used at.
    entries: HashMap<String, Entry>,
}

enum Job {
    Write(PathBuf, Arc<ColorImage>),
    Remove(PathBuf),
    Index(PathBuf, String),
}

pub struct DiskCache {
    dir: PathBuf,
    cap: u64,
    used: u64,
    tick: u64,
    index: HashMap<u64, Entry>,
    writer: Sender<Job>,
    /// Lookups that found / missed a frame (the playback statistics count them with the RAM caches).
    pub hits: u64,
    pub misses: u64,
}

/// FNV-1a of a frame's identity, stable across builds (`DefaultHasher` is not).
pub fn key(media: &str, sf: i64, w: u32, h: u32, scaler: &str, proxy: bool, grid: u64) -> u64 {
    let s = format!("{media}\n{sf}\n{w}x{h}\n{scaler}\n{proxy}\n{grid}");
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

fn frame_path(dir: &Path, key: u64) -> PathBuf {
    dir.join(format!("{key:016x}.rgba"))
}

fn index_path(dir: &Path) -> PathBuf {
    dir.join("index.json")
}

fn load_index(dir: &Path) -> Index {
    std::fs::read_to_string(index_path(dir)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

impl DiskCache {
    /// Open the cache in `dir` with `cap` bytes: the index is read (entries whose file is gone are dropped) and the
    /// writer thread started.
    pub fn open(dir: PathBuf, cap: u64) -> DiskCache {
        let _ = std::fs::create_dir_all(&dir);
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("milan-framecache".into())
            .spawn(move || {
                for job in rx {
                    match job {
                        Job::Write(path, img) => {
                            let tmp = path.with_extension("tmp");
                            let write = || -> std::io::Result<()> {
                                let mut f = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
                                f.write_all(&(img.size[0] as u32).to_le_bytes())?;
                                f.write_all(&(img.size[1] as u32).to_le_bytes())?;
                                f.write_all(bytemuck::cast_slice(&img.pixels))?;
                                f.flush()?;
                                drop(f);
                                std::fs::rename(&tmp, &path)
                            };
                            if write().is_err() {
                                let _ = std::fs::remove_file(&tmp);
                            }
                        }
                        Job::Remove(path) => {
                            let _ = std::fs::remove_file(path);
                        }
                        Job::Index(path, text) => {
                            let _ = std::fs::write(path, text);
                        }
                    }
                }
            })
            .expect("spawn milan-framecache");
        let mut c = DiskCache { dir, cap, used: 0, tick: 0, index: HashMap::new(), writer: tx, hits: 0, misses: 0 };
        for (k, e) in load_index(&c.dir).entries {
            let Ok(key) = u64::from_str_radix(&k, 16) else { continue };
            if frame_path(&c.dir, key).exists() {
                c.tick = c.tick.max(e.used);
                c.used += e.bytes;
                c.index.insert(key, e);
            }
        }
        c.evict();
        c
    }

    pub fn set_cap(&mut self, cap: u64) {
        if cap != self.cap {
            self.cap = cap;
            self.evict();
            self.save_index();
        }
    }

    /// The frame under `key`, read from disk (None: not there, or the file is bad, which drops it).
    pub fn get(&mut self, key: u64) -> Option<Arc<ColorImage>> {
        let Some(e) = self.index.get_mut(&key) else {
            self.misses += 1;
            return None;
        };
        self.tick += 1;
        e.used = self.tick;
        let path = frame_path(&self.dir, key);
        let read = || -> std::io::Result<ColorImage> {
            let mut f = std::io::BufReader::new(std::fs::File::open(&path)?);
            let mut wh = [0u8; 8];
            f.read_exact(&mut wh)?;
            let (w, h) = (u32::from_le_bytes([wh[0], wh[1], wh[2], wh[3]]) as usize, u32::from_le_bytes([wh[4], wh[5], wh[6], wh[7]]) as usize);
            let mut px = vec![Color32::TRANSPARENT; w * h];
            f.read_exact(bytemuck::cast_slice_mut(&mut px))?;
            Ok(ColorImage { size: [w, h], pixels: px })
        };
        match read() {
            Ok(img) => {
                self.hits += 1;
                Some(Arc::new(img))
            }
            Err(_) => {
                self.misses += 1;
                self.remove(key);
                self.save_index();
                None
            }
        }
    }

    /// Keep `img` under `key` (the write happens on the writer thread), dropping the least recently used frames over the
    /// cap.
    pub fn put(&mut self, key: u64, img: Arc<ColorImage>) {
        let bytes = 8 + img.pixels.len() as u64 * 4;
        if self.cap == 0 || bytes > self.cap {
            return;
        }
        self.tick += 1;
        if let Some(old) = self.index.insert(key, Entry { bytes, used: self.tick }) {
            self.used -= old.bytes;
        }
        self.used += bytes;
        let _ = self.writer.send(Job::Write(frame_path(&self.dir, key), img));
        self.evict();
        self.save_index();
    }

    fn remove(&mut self, key: u64) {
        if let Some(e) = self.index.remove(&key) {
            self.used -= e.bytes;
            let _ = self.writer.send(Job::Remove(frame_path(&self.dir, key)));
        }
    }

    fn evict(&mut self) {
        while self.used > self.cap {
            // O(n) scan: a few thousand entries at most.
            let Some(&old) = self.index.iter().min_by_key(|e| e.1.used).map(|e| e.0) else { break };
            self.remove(old);
        }
    }

    fn save_index(&self) {
        let ix = Index { entries: self.index.iter().map(|(k, e)| (format!("{k:016x}"), e.clone())).collect() };
        if let Ok(text) = serde_json::to_string(&ix) {
            let _ = self.writer.send(Job::Index(index_path(&self.dir), text));
        }
    }

    /// Bytes the index holds.
    #[cfg(test)]
    pub fn used(&self) -> u64 {
        self.used
    }

    /// Tests: wait until the writer has written everything sent so far.
    #[cfg(test)]
    pub fn flush(&self) {
        let marker = self.dir.join("flush.tmp");
        let _ = self.writer.send(Job::Index(marker.clone(), String::new()));
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !marker.exists() && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let _ = std::fs::remove_file(marker);
    }
}

/// Preferences › Video › Clear: delete the cached frames the index lists (never anything else in the folder) and the
/// index. A cache open in the worker finds its files gone and drops them one by one.
pub fn clear() {
    let Some(dir) = crate::engine::caches::disk_dir() else { return };
    for k in load_index(&dir).entries.keys() {
        if let Ok(key) = u64::from_str_radix(k, 16) {
            let _ = std::fs::remove_file(frame_path(&dir, key));
        }
    }
    let _ = std::fs::remove_file(index_path(&dir));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(w: usize, h: usize, c: u8) -> Arc<ColorImage> {
        Arc::new(ColorImage { size: [w, h], pixels: vec![Color32::from_gray(c); w * h] })
    }

    /// Frames go to disk and come back equal; over the cap the least recently used go first (their files too); a get
    /// refreshes an entry; the index survives a reopen; Clear deletes only what the index lists.
    #[test]
    fn lru_on_disk() {
        let dir = std::env::temp_dir().join("milan-agent").join(format!("framecache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let frame = 8 + 16 * 16 * 4; // bytes of a 16x16 frame
        let mut c = DiskCache::open(dir.clone(), frame * 3);
        let keys: Vec<u64> = (0..5).map(|i| key("m.mp4", i, 16, 16, "bilinear", false, 0)).collect();
        assert!(keys.iter().all(|k| keys.iter().filter(|x| *x == k).count() == 1), "distinct keys");
        for (i, k) in keys.iter().enumerate().take(3) {
            c.put(*k, img(16, 16, i as u8 * 50));
        }
        c.flush();
        assert_eq!(c.used(), frame * 3);
        assert_eq!(c.get(keys[1]).unwrap().pixels[0], Color32::from_gray(50)); // refreshes 1: 0 is now the oldest
        c.put(keys[3], img(16, 16, 150));
        c.flush();
        assert!(c.get(keys[0]).is_none() && c.get(keys[1]).is_some() && c.get(keys[2]).is_some() && c.get(keys[3]).is_some());
        assert!(!frame_path(&dir, keys[0]).exists() && frame_path(&dir, keys[3]).exists());
        assert!(c.used() <= frame * 3);
        // A frame bigger than the cap is not kept; a zero cap keeps nothing.
        c.put(keys[4], img(64, 64, 1));
        assert!(c.get(keys[4]).is_none());
        let (hits, misses) = (c.hits, c.misses);
        assert!(hits >= 4 && misses >= 2, "{hits} {misses}");
        c.flush();
        drop(c);
        // Reopened: the index and files are what was left; a smaller cap evicts on open.
        let mut c = DiskCache::open(dir.clone(), frame * 3);
        assert_eq!(c.used(), frame * 3);
        assert_eq!(c.get(keys[3]).unwrap().pixels[5], Color32::from_gray(150));
        c.set_cap(frame);
        c.flush();
        assert_eq!(c.used(), frame);
        assert!(c.get(keys[3]).is_some(), "the most recently used stays");
        // Clear: the listed files and the index go, a stray file does not.
        std::fs::write(dir.join("keep.txt"), "x").unwrap();
        crate::engine::caches::set_disk(Some(dir.clone()), frame);
        drop(c);
        clear();
        crate::engine::caches::set_disk(None, 0);
        assert!(!frame_path(&dir, keys[3]).exists() && !index_path(&dir).exists() && dir.join("keep.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
