use eframe::egui;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, Sender};
use crate::project::{MediaItem, MediaType};

pub fn probe_media(path: &Path) -> anyhow::Result<MediaItem> {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let ext = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();

    let media_type = classify_extension(&ext);

    let mut item = MediaItem {
        id: uuid::Uuid::new_v4().to_string(),
        path: path.to_path_buf(),
        name,
        media_type,
        duration: 0.0,
        width: None,
        height: None,
        frame_rate: None,
        sample_rate: None,
        codec: None,
    };

    let mut cmd = Command::new("ffprobe");
    cmd.args(["-v", "quiet", "-print_format", "json", "-show_format", "-show_streams"])
        .arg(path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let output = cmd.output().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => anyhow::anyhow!("ffprobe not found on PATH"),
        _ => anyhow::anyhow!("ffprobe failed: {e}"),
    })?;
    if !output.status.success() && item.media_type != MediaType::Image {
        anyhow::bail!("unreadable media");
    }
    parse_ffprobe(&String::from_utf8_lossy(&output.stdout), &mut item);

    if item.media_type == MediaType::Image && item.duration == 0.0 {
        item.duration = 5.0;
    }
    // A zero-length clip would be invisible on the timeline.
    if item.duration <= 0.0 {
        anyhow::bail!("could not read duration");
    }

    Ok(item)
}

type ProbeBatch = Vec<Result<MediaItem, String>>;

/// Probes imported files on a worker thread so ffprobe never blocks the UI.
pub struct Importer {
    tx: Sender<ProbeBatch>,
    rx: Receiver<ProbeBatch>,
}

impl Importer {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { tx, rx }
    }

    pub fn import(&self, ctx: &egui::Context, paths: Vec<PathBuf>) {
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let batch = paths
                .iter()
                .map(|p| probe_media(p).map_err(|e| format!("{}: {e}", p.file_name().unwrap_or_default().to_string_lossy())))
                .collect();
            let _ = tx.send(batch);
            ctx.request_repaint();
        });
    }

    /// A finished batch of probe results, one per picked file.
    pub fn poll(&self) -> Option<ProbeBatch> {
        self.rx.try_recv().ok()
    }
}

fn classify_extension(ext: &str) -> MediaType {
    match ext {
        "mp4" | "mov" | "avi" | "mkv" | "wmv" | "flv" | "webm" | "mxf" | "mpg" | "mpeg"
        | "ts" | "m2ts" | "vob" | "3gp" | "m4v" => MediaType::Video,
        "mp3" | "wav" | "flac" | "aac" | "ogg" | "wma" | "aiff" | "opus" | "m4a" | "ac3"
        | "dts" | "pcm" => MediaType::Audio,
        "png" | "jpg" | "jpeg" | "tiff" | "tif" | "bmp" | "webp" | "gif" | "exr" | "tga"
        | "psd" => MediaType::Image,
        _ => MediaType::Unknown,
    }
}

fn parse_ffprobe(json: &str, item: &mut MediaItem) {
    let Ok(info) = serde_json::from_str::<serde_json::Value>(json) else {
        return;
    };

    if let Some(dur) = info["format"]["duration"].as_str() {
        item.duration = dur.parse().unwrap_or(0.0);
    }

    let Some(streams) = info["streams"].as_array() else {
        return;
    };

    for stream in streams {
        match stream["codec_type"].as_str() {
            Some("video") => {
                item.width = stream["width"].as_u64().map(|v| v as u32);
                item.height = stream["height"].as_u64().map(|v| v as u32);
                if let Some(r) = stream["r_frame_rate"].as_str() {
                    if let Some((n, d)) = r.split_once('/') {
                        let num: f64 = n.parse().unwrap_or(0.0);
                        let den: f64 = d.parse().unwrap_or(1.0);
                        if den > 0.0 {
                            item.frame_rate = Some(num / den);
                        }
                    }
                }
                item.codec = stream["codec_name"].as_str().map(String::from);
            }
            Some("audio") => {
                if let Some(sr) = stream["sample_rate"].as_str() {
                    item.sample_rate = sr.parse().ok();
                }
                if item.codec.is_none() {
                    item.codec = stream["codec_name"].as_str().map(String::from);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn unreadable_media_is_an_error() {
        // Err whether ffprobe is missing or fails on the file, never a 0-length Ok item.
        assert!(super::probe_media(std::path::Path::new("no-such-file.mp4")).is_err());
    }
}
