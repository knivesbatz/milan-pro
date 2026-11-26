//! R44: Help › Create Diagnostic Report (the folder is written on a thread, then shown with Open Folder), and the
//! offer at the next start after a crash: Open crash report / Report on GitHub (the issue page opens in the browser
//! with the version filled in; the user attaches the report, nothing is uploaded by Milan Pro).
use eframe::egui::*;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use crate::diagnostics::{self, Report};

pub struct Diagnostics {
    rx: mpsc::Receiver<Result<Report, String>>,
    result: Option<Result<Report, String>>,
}

pub enum DiagAction {
    OpenFolder(PathBuf),
    Close,
}

impl Diagnostics {
    /// Start writing `report-<time>` under the diagnostics folder.
    pub fn start(ctx: &Context, privacy: bool) -> Self {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = diagnostics::reports_dir().ok_or_else(|| "no local data folder".to_string()).and_then(|root| {
                let dir = root.join(format!("report-{}", crate::logger::file_stamp(std::time::SystemTime::now())));
                diagnostics::create(&dir, crate::settings::Prefs::file().as_deref(), crate::logger::logs_dir().as_deref(), crate::logger::crashes_dir().as_deref(), privacy).map_err(|e| e.to_string())
            });
            let _ = tx.send(r);
            ctx.request_repaint();
        });
        Self { rx, result: None }
    }

    pub fn show(&mut self, ctx: &Context) -> Option<DiagAction> {
        if self.result.is_none() {
            self.result = self.rx.try_recv().ok();
        }
        let result = &self.result;
        let (hit, close) = super::modal(ctx, "diagnostic_report", "Diagnostic Report", vec2(520.0, 300.0), |ui| match result {
            None => {
                ui.label("Collecting system information, FFmpeg details, preferences, logs and crash reports...");
                ui.add_space(160.0);
                super::button_row(ui, &[("Close", false)])
            }
            Some(Err(e)) => {
                ui.label(format!("The report could not be written: {e}"));
                ui.add_space(160.0);
                super::button_row(ui, &[("Close", true)])
            }
            Some(Ok(r)) => {
                ui.label(RichText::new("The report was written to:").strong());
                ui.label(r.dir.display().to_string());
                ui.add_space(4.0);
                super::field_box(ui, 4, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_height(110.0);
                    for (name, res) in &r.sections {
                        match res {
                            Ok(()) => ui.label(format!("{name}: written")),
                            Err(e) => ui.label(format!("{name}: {e}")),
                        };
                    }
                });
                ui.add_space(4.0);
                ui.label("Nothing was sent anywhere. To report a problem, zip the folder and attach it to a GitHub issue.");
                ui.add_space(8.0);
                super::button_row(ui, &[("Open Folder", true), ("Close", true)])
            }
        });
        match (hit, &self.result) {
            (Some(0), Some(Ok(r))) => Some(DiagAction::OpenFolder(r.dir.clone())),
            (Some(_), _) => Some(DiagAction::Close),
            (None, _) if close => Some(DiagAction::Close),
            _ => None,
        }
    }
}

pub enum CrashChoice {
    OpenReport,
    ReportOnGitHub,
    Close,
}

/// The offer after a crash: `report` is the newest `crash-<time>.txt`.
pub fn crash_offer_window(ctx: &Context, report: &Path) -> Option<CrashChoice> {
    let when = std::fs::metadata(report).and_then(|m| m.modified()).ok().map(|t| crate::logger::format_time(t, false)).unwrap_or_default();
    let (hit, close) = super::modal(ctx, "crash_offer", "Milan Pro", vec2(500.0, 190.0), |ui| {
        ui.label(RichText::new("Milan Pro closed unexpectedly the last time it ran.").strong());
        ui.add_space(4.0);
        ui.label(format!("A crash report was written at {when}:"));
        ui.label(report.display().to_string());
        ui.add_space(4.0);
        ui.label("Report on GitHub opens the issue page in your browser with the version filled in; attach the report yourself. Nothing is sent by Milan Pro.");
        ui.add_space(12.0);
        super::button_row(ui, &[("Open crash report", true), ("Report on GitHub", true), ("Close", true)])
    });
    match hit {
        Some(0) => Some(CrashChoice::OpenReport),
        Some(1) => Some(CrashChoice::ReportOnGitHub),
        Some(_) => Some(CrashChoice::Close),
        None if close => Some(CrashChoice::Close),
        None => None,
    }
}

/// The new-issue page with the version and the report's name filled in.
pub fn issue_url(version: &str, report: &Path) -> String {
    let name = report.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let title = encode(&format!("Crash in Milan Pro {version}"));
    let body = encode(&format!("Milan Pro {version} closed unexpectedly ({name}).\n\nWhat I was doing:\n\n\n(Attach the crash report, or the folder from Help > Create Diagnostic Report.)\n"));
    format!("https://github.com/knivesbatz/milan-pro/issues/new?title={title}&body={body}")
}

/// Percent-encode for a URL query value.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Open `url` in the default browser.
pub fn open_url(url: &str) {
    if !url.starts_with("https://") {
        return;
    }
    #[cfg(windows)]
    // SAFETY: NUL-terminated wide strings that outlive the call.
    unsafe {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let w = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
        let (open, u) = (w("open"), w(url));
        ShellExecuteW(std::ptr::null_mut(), open.as_ptr(), u.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL);
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_url_carries_the_version_and_nothing_else() {
        let u = issue_url("0.8.0", Path::new(r"C:\Users\x\AppData\Local\MilanPro\crashes\crash-20261126-101010.txt"));
        assert!(u.starts_with("https://github.com/knivesbatz/milan-pro/issues/new?title=Crash%20in%20Milan%20Pro%200.8.0&body="), "{u}");
        assert!(u.contains("crash-20261126-101010.txt") && !u.contains("Users") && !u.contains(' '), "{u}");
        assert_eq!(encode("a b/c"), "a%20b%2Fc");
    }
}
