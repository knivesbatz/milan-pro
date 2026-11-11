//! Periodic backup of modified projects to %APPDATA%\MilanPro\autosave (never the user's file).
use eframe::egui;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use crate::project::Project;

const INTERVAL: f64 = 120.0;

pub fn autosave_path(name: &str) -> Option<PathBuf> {
    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || " -_.".contains(c) { c } else { '_' }).collect();
    let base = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(base).join("MilanPro").join("autosave").join(format!("{}-autosave.mnle", safe.trim())))
}

#[derive(Default)]
pub struct Autosave {
    last: Option<f64>,
    error: Arc<Mutex<Option<String>>>,
}

impl Autosave {
    pub fn tick(&mut self, ctx: &egui::Context, project: &Project) {
        let now = ctx.input(|i| i.time);
        let last = *self.last.get_or_insert(now);
        if now - last < INTERVAL {
            ctx.request_repaint_after(Duration::from_secs_f64(INTERVAL - (now - last)));
            return;
        }
        self.last = Some(now);
        ctx.request_repaint_after(Duration::from_secs_f64(INTERVAL));
        if !project.modified {
            return;
        }
        // Saved projects are named after their file; unsaved ones get the pid so instances don't collide.
        let name = match project.path {
            Some(_) => project.name.clone(),
            None => format!("{}-{}", project.name, std::process::id()),
        };
        let Some(path) = autosave_path(&name).filter(|p| Some(p) != project.path.as_ref()) else { return };
        let json = match serde_json::to_string_pretty(project) {
            Ok(j) => j,
            Err(e) => return *self.error.lock().unwrap() = Some(format!("Autosave failed: {e}")),
        };
        let error = self.error.clone();
        std::thread::spawn(move || {
            // Write then rename, so a crash mid-write never destroys the previous good backup.
            let tmp = path.with_extension("mnle.tmp");
            let res = path
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|_| std::fs::write(&tmp, json))
                .and_then(|_| std::fs::rename(&tmp, &path));
            *error.lock().unwrap() = res.err().map(|e| format!("Autosave failed: {e}"));
        });
    }

    pub fn error(&self) -> Option<String> {
        self.error.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn path_is_sanitized_and_in_appdata() {
        let Some(p) = super::autosave_path("My: Project?") else { return };
        assert!(p.ends_with("MilanPro/autosave/My_ Project_-autosave.mnle"));
    }
}
