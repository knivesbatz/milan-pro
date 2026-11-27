//! R45b: Project Media › File Format Properties: every field and tag ffprobe reports for the file (format, each
//! stream, chapters) in a tree, with Copy as Text. ffprobe runs on a worker; the dialog fills in when it is done.
use eframe::egui::*;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

pub struct FileProps {
    pub name: String,
    path: PathBuf,
    rx: Option<Receiver<Result<Value, String>>>,
    info: Option<Result<Value, String>>,
}

/// ffprobe's full report on `path` as JSON.
pub fn probe(path: &Path) -> Result<Value, String> {
    let mut cmd = crate::proc::ffprobe();
    cmd.args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams", "-show_chapters", "-show_programs"]).arg(path);
    let out = crate::proc::output(&mut cmd).map_err(|e| crate::proc::tool_error(&e, "ffprobe"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or("ffprobe could not read the file").trim().to_string());
    }
    serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())
}

/// The sections of a report: ("Format", value), ("Stream #0: video (h264)", value), ("Chapter #1", value)...
pub fn sections(v: &Value) -> Vec<(String, &Value)> {
    let mut out = Vec::new();
    if let Some(f) = v.get("format") {
        out.push(("Format".to_string(), f));
    }
    for s in v["streams"].as_array().into_iter().flatten() {
        let kind = s["codec_type"].as_str().unwrap_or("data");
        let codec = s["codec_name"].as_str().map(|c| format!(" ({c})")).unwrap_or_default();
        out.push((format!("Stream #{}: {kind}{codec}", s["index"].as_u64().unwrap_or(0)), s));
    }
    for (k, c) in v["chapters"].as_array().into_iter().flatten().enumerate() {
        out.push((format!("Chapter #{}", k + 1), c));
    }
    for (k, p) in v["programs"].as_array().into_iter().flatten().enumerate() {
        out.push((format!("Program #{}", k + 1), p));
    }
    out
}

fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        v => v.to_string(),
    }
}

/// The report as indented text (Copy as Text): each section, its fields, and nested objects (tags, disposition,
/// side data) one level further in.
pub fn to_text(file: &str, v: &Value) -> String {
    fn walk(out: &mut String, v: &Value, depth: usize) {
        let pad = "  ".repeat(depth);
        match v {
            Value::Object(m) => {
                for (k, x) in m {
                    match x {
                        Value::Object(_) | Value::Array(_) => {
                            out.push_str(&format!("{pad}{k}:\n"));
                            walk(out, x, depth + 1);
                        }
                        x => out.push_str(&format!("{pad}{k}: {}\n", scalar(x))),
                    }
                }
            }
            Value::Array(a) => {
                for (k, x) in a.iter().enumerate() {
                    match x {
                        Value::Object(_) | Value::Array(_) => {
                            out.push_str(&format!("{pad}[{k}]\n"));
                            walk(out, x, depth + 1);
                        }
                        x => out.push_str(&format!("{pad}[{k}] {}\n", scalar(x))),
                    }
                }
            }
            x => out.push_str(&format!("{pad}{}\n", scalar(x))),
        }
    }
    let mut out = format!("{file}\n");
    for (title, s) in sections(v) {
        out.push_str(&format!("\n{title}\n"));
        walk(&mut out, s, 1);
    }
    out
}

impl FileProps {
    /// Probe `path` in the background.
    pub fn new(ctx: &Context, name: &str, path: &Path) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let (p, ctx) = (path.to_path_buf(), ctx.clone());
        std::thread::Builder::new()
            .name("milan-fileprops".into())
            .spawn(move || {
                let _ = tx.send(probe(&p));
                ctx.request_repaint();
            })
            .ok();
        Self { name: name.to_string(), path: path.to_path_buf(), rx: Some(rx), info: None }
    }

    /// The harness: probed at once, so the screenshot shows the report.
    pub fn new_now(name: &str, path: &Path) -> Self {
        Self { name: name.to_string(), path: path.to_path_buf(), rx: None, info: Some(probe(path)) }
    }

    /// True when closed.
    pub fn show(&mut self, ctx: &Context) -> bool {
        if let Some(r) = self.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            (self.info, self.rx) = (Some(r), None);
        }
        let title = format!("File Format Properties - {}", self.name);
        let (hit, close) = super::modal(ctx, "file_props", &title, vec2(600.0, 520.0), |ui| {
            let d = crate::ui::theme::get().dialog();
            ui.label(self.path.display().to_string());
            ui.add_space(4.0);
            super::field_box(ui, 4, |ui| {
                ui.set_width(ui.available_width());
                ui.set_height(400.0);
                ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.spacing_mut().interact_size.y = 16.0;
                    match &self.info {
                    None => {
                        ui.label(RichText::new("Reading the file...").color(d.text_disabled));
                    }
                    Some(Err(e)) => {
                        ui.label(RichText::new(format!("ffprobe could not read the file: {e}")).color(d.text_disabled));
                    }
                    Some(Ok(v)) => {
                        for (k, (title, s)) in sections(v).into_iter().enumerate() {
                            CollapsingHeader::new(RichText::new(title).strong()).id_salt(("fp_sec", k)).default_open(k < 3).show(ui, |ui| tree(ui, s, k));
                        }
                    }
                }});
            });
            ui.add_space(8.0);
            super::button_row(ui, &[("Close", true), ("Copy as Text", matches!(self.info, Some(Ok(_))))])
        });
        match hit {
            Some(1) => {
                if let Some(Ok(v)) = &self.info {
                    ctx.copy_text(to_text(&self.path.display().to_string(), v));
                }
                false
            }
            Some(_) => true,
            None => close,
        }
    }
}

/// One level of the tree: fields as "key: value" rows, objects and arrays as nodes.
fn tree(ui: &mut Ui, v: &Value, salt: usize) {
    let d = crate::ui::theme::get().dialog();
    let row = |ui: &mut Ui, k: &str, x: &Value, n: usize| match x {
        Value::Object(_) | Value::Array(_) => {
            CollapsingHeader::new(k).id_salt(("fp", salt, n, k)).default_open(k == "tags").show(ui, |ui| tree(ui, x, salt * 1000 + n));
        }
        x => {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{k}:")).color(d.text_disabled));
                ui.label(scalar(x));
            });
        }
    };
    match v {
        Value::Object(m) => {
            for (n, (k, x)) in m.iter().enumerate() {
                row(ui, k, x, n);
            }
        }
        Value::Array(a) => {
            for (n, x) in a.iter().enumerate() {
                row(ui, &format!("[{n}]"), x, n);
            }
        }
        x => {
            ui.label(scalar(x));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_field_and_tag_is_listed() {
        let v: Value = serde_json::from_str(r#"{"programs":[],"streams":[
            {"index":0,"codec_name":"h264","codec_type":"video","width":1920,"disposition":{"default":1},"tags":{"language":"und","handler_name":"VideoHandler"}},
            {"index":1,"codec_name":"aac","codec_type":"audio","sample_rate":"48000"}],
            "chapters":[{"id":0,"start_time":"0.000000","tags":{"title":"Intro"}}],
            "format":{"filename":"a.mp4","format_name":"mov,mp4","duration":"3.0","tags":{"encoder":"Lavf61","com.apple.quicktime.make":"Sony"}}}"#).unwrap();
        let titles: Vec<String> = sections(&v).into_iter().map(|s| s.0).collect();
        assert_eq!(titles, ["Format", "Stream #0: video (h264)", "Stream #1: audio (aac)", "Chapter #1"]);
        let text = to_text("a.mp4", &v);
        for want in ["format_name: mov,mp4", "  tags:\n", "    com.apple.quicktime.make: Sony", "handler_name: VideoHandler", "default: 1", "sample_rate: 48000", "title: Intro", "width: 1920"] {
            assert!(text.contains(want), "{want:?} missing from\n{text}");
        }
    }
}
