#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod autosave;
mod camera;
mod decode;
mod demo;
mod diagnostics;
mod edit;
mod engine;
mod history;
mod interchange;
mod logger;
mod media;
mod overlays;
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
    // R44: the log file and crash hooks first, so whatever fails later is on record. The harness writes nothing into
    // the user's AppData: without MILAN_APPDATA it logs to stderr and writes no crash files.
    let harness = launch.demo || launch.screenshot.is_some();
    // R37: the format capability table is cached per FFmpeg version; the harness never writes into AppData.
    engine::caps::set_write(!harness);
    let prefs = (!launch.demo).then(settings::Prefs::load);
    // R49: Preferences › General › Temporary files folder: TMP and TEMP point there for Milan Pro and its FFmpeg
    // children. Windows' own folder is kept for the Preferences page.
    settings::remember_system_temp();
    if let Some(dir) = prefs.as_ref().and_then(|p| p.temp_folder.clone()).filter(|d| d.is_dir()) {
        // SAFETY: no other thread exists yet (the logger, the audio and the GPU threads all start below).
        unsafe {
            std::env::set_var("TMP", &dir);
            std::env::set_var("TEMP", &dir);
        }
    }
    let files = !harness || std::env::var_os("MILAN_APPDATA").is_some_and(|d| !d.is_empty());
    logger::init(if files { logger::logs_dir() } else { None }, logger::level_from_name(prefs.as_ref().map_or("info", |p| p.log_level.as_str())));
    logger::install_crash_hooks(if files { logger::crashes_dir() } else { None });
    log::info!("Milan Pro {} starting ({})", updater::CURRENT, std::env::args().skip(1).collect::<Vec<_>>().join(" "));
    // R46: Setup's relaunch passes --after-update; a version that fails to start twice is offered a roll back.
    if files {
        if let Some(dir) = updater::updates_dir() {
            launch.rollback_offer = updater::start_marker(&dir, updater::CURRENT, std::env::args().any(|a| a == "--after-update"));
        }
    }
    // The OS title bar; the menu row is drawn under it.
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Untitled - Milan Pro")
        .with_inner_size([1600.0, 900.0])
        .with_min_inner_size([960.0, 540.0])
        .with_icon(icon());
    if offscreen_run(launch.screenshot.is_some()) {
        // The app resizes to 1920x1080 px on its first frame.
        viewport = offscreen(viewport).with_min_inner_size([100.0, 100.0]);
    }
    let mut options = eframe::NativeOptions { viewport, ..Default::default() };
    // R21a: the UI paints through wgpu (Vulkan), on the adapter Preferences › Video chose, and the compositor shares
    // that device (`engine::gpu::attach`). Built with `--features glow` only, eframe's OpenGL painter stays.
    #[cfg(feature = "wgpu")]
    {
        let gpu = if launch.demo { String::new() } else { settings::Prefs::load().gpu };
        options.renderer = eframe::Renderer::Wgpu;
        options.wgpu_options = engine::gpu::wgpu_options(&gpu);
    }
    #[cfg(not(feature = "wgpu"))]
    {
        options.renderer = eframe::Renderer::Glow;
    }
    eframe::run_native(
        "Milan Pro",
        options,
        Box::new(|cc| Ok(Box::new(app::MeridianApp::new(cc, launch)))),
    )
}

/// The window and taskbar icon of every Milan Pro window (the exe's own icon is assets/brand/milan-pro.ico, build.rs).
pub fn icon() -> std::sync::Arc<egui::IconData> {
    static ICON: std::sync::OnceLock<std::sync::Arc<egui::IconData>> = std::sync::OnceLock::new();
    ICON.get_or_init(|| {
        let png = include_bytes!("../assets/brand/milan-pro-256.png");
        std::sync::Arc::new(eframe::icon_data::from_png_bytes(png).expect("assets/brand/milan-pro-256.png is a valid PNG"))
    })
    .clone()
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
