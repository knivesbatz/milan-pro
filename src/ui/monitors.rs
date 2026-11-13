//! R14: the displays (Windows `EnumDisplayMonitors`) with their work areas and scaling, where a floating window or the
//! external preview goes on them, and the global cursor and buttons (to dock a floating window the OS moved). Every
//! rectangle here is in physical screen pixels; egui sizes a window in points of the monitor it is on.
use eframe::egui::{Context, Pos2, Rect, ViewportCommand, pos2, vec2};

/// A display, as Windows numbers it.
#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    /// `\\.\DISPLAY2`: what layouts and the preferences keep.
    pub name: String,
    /// The monitor's model ("Generic PnP Monitor").
    pub model: String,
    pub rect: Rect,
    /// Without the taskbar.
    pub work: Rect,
    pub primary: bool,
    /// Windows scaling (1.5 = 150 %).
    pub scale: f32,
}

impl Monitor {
    /// The number Windows shows for it (Settings › Display, Identify).
    pub fn number(&self) -> u32 {
        let digits = self.name.trim_start_matches(|c: char| !c.is_ascii_digit());
        digits.parse().unwrap_or(0)
    }

    /// "2 - Generic PnP Monitor (2560x1440)", the Display adapter list's row.
    pub fn label(&self) -> String {
        let model = if self.model.is_empty() { self.name.as_str() } else { self.model.as_str() };
        let primary = if self.primary { ", primary" } else { "" };
        format!("{} - {model} ({}x{}{primary})", self.number(), self.rect.width() as i32, self.rect.height() as i32)
    }
}

/// Where a floating window saved at `rect` on `monitor` reopens: on that display if it is still connected, else on the
/// primary one, moved and shrunk to fit the work area. The rect and the display's index; None without displays.
pub fn place(rect: Rect, monitor: &str, mons: &[Monitor]) -> Option<(Rect, usize)> {
    let k = mons.iter().position(|m| m.name == monitor).or_else(|| mons.iter().position(|m| m.primary)).or((!mons.is_empty()).then_some(0))?;
    Some((clamp_into(rect, mons[k].work), k))
}

/// `r` moved (and shrunk if it must) to lie inside `area`.
pub fn clamp_into(r: Rect, area: Rect) -> Rect {
    let size = r.size().max(vec2(1.0, 1.0)).min(area.size());
    let min = r.min.clamp(area.min, area.max - size);
    Rect::from_min_size(min, size)
}

/// The external preview's display: the one chosen in Preferences › Preview Device if it is connected, else the first
/// that is not the primary, else the primary.
pub fn external<'a>(choice: Option<&str>, mons: &'a [Monitor]) -> Option<&'a Monitor> {
    choice.and_then(|c| mons.iter().find(|m| m.name == c)).or_else(|| mons.iter().find(|m| !m.primary)).or_else(|| mons.first())
}

/// A physical rect in the points of a window drawn at `ppp` pixels per point (egui's zoom × the display's scaling).
pub fn to_points(r: Rect, ppp: f32) -> Rect {
    Rect::from_min_size((r.min.to_vec2() / ppp).to_pos2(), r.size() / ppp)
}

/// A point in physical pixels as the points of the window whose client area starts at `origin` (points) at `ppp`.
pub fn to_local(p: Pos2, origin: Pos2, ppp: f32) -> Pos2 {
    (p.to_vec2() / ppp).to_pos2() - origin.to_vec2()
}

/// Move this frame's viewport to `want` (outer position, inner size), in its own pixels per point, which change as it
/// crosses displays: Windows first places a new window in the primary display's scaling. True once it is there.
pub fn settle(ctx: &Context, want: Rect) -> bool {
    let (outer, inner, ppp) = ctx.input(|i| (i.viewport().outer_rect, i.viewport().inner_rect, i.pixels_per_point));
    let (Some(outer), Some(inner)) = (outer, inner) else { return false };
    let at = outer.min.to_vec2() * ppp;
    let (pos_ok, size_ok) = ((at - want.min.to_vec2()).length() <= 1.5, (inner.size() * ppp - want.size()).length() <= 1.5);
    if !pos_ok {
        ctx.send_viewport_cmd(ViewportCommand::OuterPosition((want.min.to_vec2() / ppp).to_pos2()));
    }
    if !size_ok {
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(want.size() / ppp));
    }
    pos_ok && size_ok
}

/// This frame's viewport as a physical rect (outer position, inner size); None before Windows has placed it.
pub fn current(ctx: &Context) -> Option<Rect> {
    let (outer, inner, ppp) = ctx.input(|i| (i.viewport().outer_rect, i.viewport().inner_rect, i.pixels_per_point));
    Some(Rect::from_min_size((outer?.min.to_vec2() * ppp).to_pos2(), inner?.size() * ppp))
}

#[cfg(windows)]
mod win {
    use super::*;
    use windows_sys::Win32::Foundation::{BOOL, LPARAM, POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{DISPLAY_DEVICEW, EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW, MonitorFromPoint};
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_LBUTTON};
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    fn rect(r: RECT) -> Rect {
        Rect::from_min_max(pos2(r.left as f32, r.top as f32), pos2(r.right as f32, r.bottom as f32))
    }

    fn text(w: &[u16]) -> String {
        String::from_utf16_lossy(&w[..w.iter().position(|&c| c == 0).unwrap_or(w.len())])
    }

    fn info(h: HMONITOR) -> Option<MONITORINFOEXW> {
        // SAFETY: a zeroed MONITORINFOEXW with its size set is what GetMonitorInfoW fills.
        unsafe {
            let mut mi: MONITORINFOEXW = std::mem::zeroed();
            mi.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
            (GetMonitorInfoW(h, &mut mi as *mut MONITORINFOEXW as *mut MONITORINFO) != 0).then_some(mi)
        }
    }

    unsafe extern "system" fn collect(h: HMONITOR, _: HDC, _: *mut RECT, out: LPARAM) -> BOOL {
        // SAFETY: `out` is the Vec passed to EnumDisplayMonitors below, alive for the whole call.
        unsafe { (*(out as *mut Vec<HMONITOR>)).push(h) };
        1
    }

    pub fn list() -> Vec<Monitor> {
        let mut handles: Vec<HMONITOR> = Vec::new();
        // SAFETY: the callback only pushes into `handles`, which outlives the call.
        unsafe { EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(collect), &mut handles as *mut _ as LPARAM) };
        let mut out: Vec<Monitor> = handles
            .into_iter()
            .filter_map(|h| {
                let mi = info(h)?;
                let name = text(&mi.szDevice);
                let (mut dx, mut dy) = (96u32, 96u32);
                // SAFETY: plain out-parameters; a failure leaves 96 dpi (100 %).
                unsafe { GetDpiForMonitor(h, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) };
                // SAFETY: a zeroed DISPLAY_DEVICEW with its size set; `dev` is NUL-terminated.
                let model = unsafe {
                    let mut dd: DISPLAY_DEVICEW = std::mem::zeroed();
                    dd.cb = size_of::<DISPLAY_DEVICEW>() as u32;
                    let dev: Vec<u16> = mi.szDevice.iter().copied().take_while(|&c| c != 0).chain([0]).collect();
                    if EnumDisplayDevicesW(dev.as_ptr(), 0, &mut dd, 0) != 0 { text(&dd.DeviceString) } else { String::new() }
                };
                let m = &mi.monitorInfo;
                Some(Monitor { name, model, rect: rect(m.rcMonitor), work: rect(m.rcWork), primary: m.dwFlags & 1 != 0, scale: dx.max(1) as f32 / 96.0 })
            })
            .collect();
        out.sort_by_key(Monitor::number);
        out
    }

    pub fn name_at(p: Pos2) -> Option<String> {
        // SAFETY: MonitorFromPoint has no out-parameters; 0 = MONITOR_DEFAULTTONULL.
        let h = unsafe { MonitorFromPoint(POINT { x: p.x.round() as i32, y: p.y.round() as i32 }, 0) };
        (!h.is_null()).then(|| info(h)).flatten().map(|mi| text(&mi.szDevice))
    }

    pub fn scale_at(p: Pos2) -> f32 {
        let (mut dx, mut dy) = (96u32, 96u32);
        // SAFETY: no pointers but plain out-parameters; 2 = MONITOR_DEFAULTTONEAREST; a failure leaves 96 dpi.
        unsafe {
            let h = MonitorFromPoint(POINT { x: p.x.round() as i32, y: p.y.round() as i32 }, 2);
            GetDpiForMonitor(h, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        }
        dx.max(1) as f32 / 96.0
    }

    pub fn cursor() -> Option<Pos2> {
        let mut p = POINT { x: 0, y: 0 };
        // SAFETY: plain out-parameter.
        (unsafe { GetCursorPos(&mut p) } != 0).then(|| pos2(p.x as f32, p.y as f32))
    }

    pub fn down(vk: u16) -> bool {
        // SAFETY: no pointers; the high bit is "down now".
        unsafe { GetAsyncKeyState(vk as i32) < 0 }
    }

    pub const LEFT: u16 = VK_LBUTTON;
    pub const CTRL: u16 = VK_CONTROL;
}

/// The connected displays in Windows' order (none on other systems).
pub fn list() -> Vec<Monitor> {
    #[cfg(windows)]
    return win::list();
    #[cfg(not(windows))]
    Vec::new()
}

/// The display holding physical point `p`.
pub fn name_at(p: Pos2) -> Option<String> {
    #[cfg(windows)]
    return win::name_at(p);
    #[cfg(not(windows))]
    {
        let _ = p;
        None
    }
}

/// The Windows scaling of the display nearest physical point `p` (1.0 elsewhere).
pub fn scale_at(p: Pos2) -> f32 {
    #[cfg(windows)]
    return win::scale_at(p);
    #[cfg(not(windows))]
    {
        let _ = p;
        1.0
    }
}

/// The mouse pointer in physical screen pixels, wherever it is.
pub fn cursor() -> Option<Pos2> {
    #[cfg(windows)]
    return win::cursor();
    #[cfg(not(windows))]
    None
}

/// The left mouse button is held (also over another window, or during the OS's own window move).
pub fn left_down() -> bool {
    #[cfg(windows)]
    return win::down(win::LEFT);
    #[cfg(not(windows))]
    false
}

pub fn ctrl_down() -> bool {
    #[cfg(windows)]
    return win::down(win::CTRL);
    #[cfg(not(windows))]
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(name: &str, x: f32, w: f32, h: f32, primary: bool, scale: f32) -> Monitor {
        let rect = Rect::from_min_size(pos2(x, 0.0), vec2(w, h));
        Monitor { name: name.into(), model: String::new(), rect, work: rect.with_max_y(h - 40.0), primary, scale }
    }

    /// A floating window on a removed monitor reopens on the primary one, inside its work area; on a monitor still
    /// there it stays, pulled back in if the resolution shrank.
    #[test]
    fn floating_window_on_a_removed_monitor_reopens_on_the_primary() {
        let mons = [mon(r"\\.\DISPLAY1", 0.0, 1920.0, 1080.0, true, 1.0), mon(r"\\.\DISPLAY2", 1920.0, 2560.0, 1440.0, false, 1.5)];
        let on2 = Rect::from_min_size(pos2(3000.0, 200.0), vec2(800.0, 600.0));
        assert_eq!(place(on2, r"\\.\DISPLAY2", &mons), Some((on2, 1)));
        let (r, k) = place(on2, r"\\.\DISPLAY3", &mons[..1]).unwrap();
        assert_eq!((k, r), (0, Rect::from_min_size(pos2(1120.0, 200.0), vec2(800.0, 600.0))));
        // Larger than the work area: shrunk to it; above or left of it: moved in.
        let big = Rect::from_min_size(pos2(-500.0, -50.0), vec2(4000.0, 900.0));
        assert_eq!(place(big, "", &mons).unwrap().0, Rect::from_min_size(pos2(0.0, 0.0), vec2(1920.0, 900.0)));
        assert_eq!(place(on2, "", &[]), None);
        // No primary flagged: the first display.
        assert_eq!(place(on2, "x", &mons[1..]).map(|p| p.1), Some(0));
    }

    #[test]
    fn external_display_choice_and_point_conversions() {
        let mons = [mon(r"\\.\DISPLAY1", 0.0, 1920.0, 1080.0, true, 1.0), mon(r"\\.\DISPLAY2", 1920.0, 2560.0, 1440.0, false, 1.5)];
        assert_eq!(external(None, &mons).unwrap().number(), 2);
        assert_eq!(external(Some(r"\\.\DISPLAY1"), &mons).unwrap().number(), 1);
        assert_eq!(external(Some("gone"), &mons[..1]).unwrap().number(), 1);
        assert!(external(None, &[]).is_none());
        // A 150 % display at x 1920: its window's points.
        let p = to_points(mons[1].rect, 1.5);
        assert!(p.min == pos2(1280.0, 0.0) && (p.size() - vec2(2560.0 / 1.5, 960.0)).length() < 1e-3, "{p:?}");
        assert_eq!(to_local(pos2(300.0, 150.0), pos2(100.0, 50.0), 1.5), pos2(100.0, 50.0));
        assert_eq!(mons[1].label(), r"2 - \\.\DISPLAY2 (2560x1440)");
    }
}
