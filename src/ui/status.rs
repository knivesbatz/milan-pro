//! Status bar (§5.11): hovered command / message on the left; background progress (video proxies) and free recording
//! time on the right.
use eframe::egui::*;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use crate::project::Project;
use super::{theme, toolbar};

/// `rect` is the status bar body (17 px VP16 / 20 px VP23); edge lines are painted by the frame.
/// `progress`: a background job's status ("Creating video proxy 2 of 5: 43%"), followed by a Cancel link.
/// Returns true when Cancel was clicked.
pub fn draw_status_bar(ui: &mut Ui, rect: Rect, project: &Project, message: Option<&str>, progress: Option<&str>) -> bool {
    let t = theme::get();
    let p = ui.painter();
    p.rect_filled(rect, 0.0, t.status_bg);
    let font = t.font(t.font.status);
    let hint = toolbar::take_hint(ui.ctx());
    let (left, color) = match (hint, message) {
        (Some(h), _) => (h, t.text),
        (None, Some(m)) => (m.to_owned(), t.warning_text),
        // VP16 leaves the last operation's status here (project open/render), "Complete: 00:00:00" (steam_vp16).
        (None, None) if t.look() == theme::UiLook::Vegas16 => ("Complete: 00:00:00".to_owned(), t.text),
        (None, None) => (String::new(), t.text),
    };
    let y = rect.center().y;
    // Right to left: record time, then the job's Cancel link and progress.
    let mut x = rect.max.x - 6.0;
    let mut put = |g: std::sync::Arc<Galley>, gap: f32| {
        x -= g.size().x;
        let at = pos2(x, (y - g.size().y * 0.5).round());
        p.galley(at, g.clone(), t.text);
        x -= gap;
        Rect::from_min_size(at, g.size())
    };
    if let Some(r) = record_time(project) {
        put(p.layout_no_wrap(format!("Record Time (2 channels): {r}"), font.clone(), t.text), 36.0);
    }
    let mut cancel = false;
    if let Some(progress) = progress {
        let link = put(p.layout_no_wrap("Cancel".into(), font.clone(), t.text), 10.0);
        let r = ui.interact(link, Id::new("status_job_cancel"), Sense::click()).on_hover_text("Stop creating video proxies");
        if r.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            p.hline(link.x_range(), link.max.y, Stroke::new(1.0_f32, t.text));
        }
        cancel = r.clicked();
        put(p.layout_no_wrap(progress.to_owned(), font.clone(), t.text), 0.0);
    }
    let left_room = (x - rect.min.x - 18.0).max(0.0);
    let lg = p.layout(left, font, color, f32::INFINITY);
    p.with_clip_rect(Rect::from_min_size(rect.min, vec2(left_room, rect.height())))
        .galley(pos2(rect.min.x + 6.0, (y - lg.size().y * 0.5).round()), lg, t.text);
    cancel
}

/// Free space on the record drive (project folder, else temp) as 48 kHz 16-bit stereo time, refreshed every 10 s.
fn record_time(project: &Project) -> Option<String> {
    static CACHE: Mutex<Option<(Instant, PathBuf, Option<String>)>> = Mutex::new(None);
    let dir = project.path.as_deref().and_then(Path::parent).map_or_else(std::env::temp_dir, Path::to_path_buf);
    let mut cache = CACHE.lock().ok()?;
    if let Some((at, d, s)) = cache.as_ref() {
        if at.elapsed() < Duration::from_secs(10) && *d == dir {
            return s.clone();
        }
    }
    let s = free_bytes(&dir).map(|b| format_hms(b / 192_000));
    *cache = Some((Instant::now(), dir, s.clone()));
    s
}

fn format_hms(secs: u64) -> String {
    format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

#[cfg(windows)]
fn free_bytes(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetDiskFreeSpaceExW(dir: *const u16, avail: *mut u64, total: *mut u64, free: *mut u64) -> i32;
    }
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut avail = 0u64;
    // SAFETY: `wide` is NUL-terminated and outlives the call; null out-pointers are allowed by the API.
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, std::ptr::null_mut(), std::ptr::null_mut()) };
    (ok != 0).then_some(avail)
}

#[cfg(not(windows))]
fn free_bytes(_dir: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn record_time_format() {
        assert_eq!(super::format_hms(135_645), "37:40:45");
        assert!(super::free_bytes(&std::env::temp_dir()).is_some_and(|b| b > 0) || cfg!(not(windows)));
    }
}
