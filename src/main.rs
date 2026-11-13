#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod autosave;
mod decode;
mod demo;
mod edit;
mod engine;
mod history;
mod interchange;
mod media;
mod proc;
mod project;
mod proxy;
mod record;
mod render;
mod settings;
mod ui;
mod updater;
#[cfg(windows)]
mod wasapi;

use eframe::egui;

fn main() -> eframe::Result<()> {
    // Never closed, so it lives until Milan exits: installer/milan-pro.iss waits for it to go before updating.
    // A null DACL (as Inno's AppMutex docs advise) lets Setup see it when it is elevated as another user.
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Security::{InitializeSecurityDescriptor, SetSecurityDescriptorDacl, SECURITY_ATTRIBUTES, SECURITY_DESCRIPTOR};
        let mut sd: SECURITY_DESCRIPTOR = std::mem::zeroed();
        let sdp = &mut sd as *mut SECURITY_DESCRIPTOR as *mut std::ffi::c_void;
        InitializeSecurityDescriptor(sdp, 1); // SECURITY_DESCRIPTOR_REVISION
        SetSecurityDescriptorDacl(sdp, 1, std::ptr::null(), 0);
        let sa = SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: sdp, bInheritHandle: 0 };
        let name: Vec<u16> = "MilanProRunning\0".encode_utf16().collect();
        windows_sys::Win32::System::Threading::CreateMutexW(&sa, 0, name.as_ptr());
    }
    let mut launch = demo::Launch::from_env();
    let look = launch.look.unwrap_or_else(|| if launch.demo { Default::default() } else { settings::Prefs::load().look });
    launch.look = Some(look);
    // VP23 draws its own 32 px title bar (4.6); the app switches decorations live on look changes.
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Untitled - Milan Pro")
        .with_inner_size([1600.0, 900.0])
        .with_min_inner_size([960.0, 540.0])
        .with_decorations(look.look == ui::theme::UiLook::Vegas16);
    if offscreen_run(launch.screenshot.is_some()) {
        // The app resizes to 1920x1080 px on its first frame.
        viewport = offscreen(viewport).with_min_inner_size([100.0, 100.0]);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "Milan Pro",
        options,
        Box::new(|cc| Ok(Box::new(app::MeridianApp::new(cc, launch)))),
    )
}

/// The screenshot harness (`screenshot`) or an automatic update run: every window opens off-screen.
pub fn offscreen_run(screenshot: bool) -> bool {
    screenshot || std::env::var_os("MILAN_UPDATE_AUTO").is_some()
}

/// Never steal focus or show up: off every monitor, no taskbar button, clicks pass through. The main window and (R14)
/// every floating window and the external preview of an off-screen run.
pub fn offscreen(v: egui::ViewportBuilder) -> egui::ViewportBuilder {
    v.with_active(false).with_taskbar(false).with_mouse_passthrough(true).with_position([-20000.0, -20000.0])
}
