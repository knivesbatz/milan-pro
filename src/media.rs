use std::path::Path;
use std::process::Command;
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

    if let Ok(output) = Command::new("ffprobe")
        .args(["-v", "quiet", "-print_format", "json", "-show_format", "-show_streams"])
        .arg(path)
        .output()
    {
        if let Ok(json) = String::from_utf8(output.stdout) {
            parse_ffprobe(&json, &mut item);
        }
    }

    if item.media_type == MediaType::Image && item.duration == 0.0 {
        item.duration = 5.0;
    }

    Ok(item)
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
