//! R44: Help › Create Diagnostic Report: a folder under `%LOCALAPPDATA%\MilanPro\diagnostics\report-<time>\` with
//! `system.txt` (Windows build, CPU, memory, GPUs and drivers through DXGI, displays), `ffmpeg.txt` (`ffmpeg -version`
//! and `-hwaccels`), `prefs.json`, the last log files and the crash reports. The user attaches the folder to an issue;
//! nothing is sent anywhere by itself. With the privacy preference on, paths and the recent-file list are left out.
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// What the report holds, section by section, with what went wrong for the ones that could not be written.
pub struct Report {
    pub dir: PathBuf,
    pub sections: Vec<(&'static str, Result<(), String>)>,
}

/// The sections every report has, in order.
pub const SECTIONS: [&str; 6] = ["README.txt", "system.txt", "ffmpeg.txt", "prefs.json", "logs", "crashes"];

pub fn reports_dir() -> Option<PathBuf> {
    Some(crate::logger::local_root()?.join("diagnostics"))
}

/// Write a report into `dir` (made here). `prefs`: the preferences file to copy; `logs` and `crashes`: the folders.
pub fn create(dir: &Path, prefs: Option<&Path>, logs: Option<&Path>, crashes: Option<&Path>, privacy: bool) -> std::io::Result<Report> {
    std::fs::create_dir_all(dir)?;
    let mut sections = Vec::new();
    let text = |name: &'static str, body: String| -> (&'static str, Result<(), String>) { (name, std::fs::write(dir.join(name), body).map_err(|e| e.to_string())) };
    sections.push(text("README.txt", readme(privacy)));
    sections.push(text("system.txt", system_info(privacy)));
    sections.push(text("ffmpeg.txt", ffmpeg_info()));
    sections.push(("prefs.json", copy_prefs(prefs, &dir.join("prefs.json"), privacy)));
    sections.push(("logs", copy_logs(logs, &dir.join("logs"))));
    sections.push(("crashes", copy_crashes(crashes, &dir.join("crashes"))));
    debug_assert!(sections.iter().map(|(n, _)| *n).eq(SECTIONS));
    for (name, r) in &sections {
        match r {
            Ok(()) => log::info!("diagnostic report: {name} written"),
            Err(e) => log::warn!("diagnostic report: {name}: {e}"),
        }
    }
    Ok(Report { dir: dir.to_path_buf(), sections })
}

fn readme(privacy: bool) -> String {
    format!(
        "Milan Pro {} diagnostic report, {}\n\n\
         system.txt   Windows build, CPU, memory, graphics adapters and drivers, displays\n\
         ffmpeg.txt   the FFmpeg build Milan Pro found and its hardware decoders\n\
         prefs.json   the preferences{}\n\
         logs\\        the last log files (Preferences > Update sets the level)\n\
         crashes\\     crash reports (crash-<time>.txt) and minidumps (.dmp) from earlier sessions\n\n\
         Nothing in this folder was sent anywhere. To report a problem, zip the folder and attach it to a GitHub issue:\n\
         https://github.com/knivesbatz/milan-pro/issues\n{}",
        crate::logger::VERSION,
        crate::logger::format_time(std::time::SystemTime::now(), true),
        if privacy { " (recent files and folders left out)" } else { "" },
        if privacy { "Paths were left out of this report (Preferences > Update > Crash reports leave out file names and paths).\n" } else { "" }
    )
}

/// `system.txt`.
pub fn system_info(privacy: bool) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "Milan Pro {}", crate::logger::VERSION);
    if !privacy {
        let _ = writeln!(s, "Executable: {}", std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default());
    }
    let _ = writeln!(s, "Time: {}", crate::logger::format_time(std::time::SystemTime::now(), true));
    let _ = writeln!(s);
    let _ = writeln!(s, "[Windows]");
    let _ = writeln!(s, "{}", windows_version());
    let _ = writeln!(s);
    let _ = writeln!(s, "[CPU]");
    let _ = writeln!(s, "{}", std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_else(|_| "unknown".into()));
    let _ = writeln!(s, "Logical processors: {}", std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0));
    let _ = writeln!(s);
    let _ = writeln!(s, "[Memory]");
    let _ = writeln!(s, "{}", memory());
    let _ = writeln!(s);
    let _ = writeln!(s, "[GPU]");
    for line in gpus() {
        let _ = writeln!(s, "{line}");
    }
    let _ = writeln!(s);
    let _ = writeln!(s, "[Displays]");
    for m in crate::ui::monitors::list() {
        let _ = writeln!(s, "{} {} {}x{} at {}%{}", m.name, m.model, m.rect.width(), m.rect.height(), (m.scale * 100.0).round(), if m.primary { " (primary)" } else { "" });
    }
    s
}

fn windows_version() -> String {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;
        #[link(name = "ntdll")]
        unsafe extern "system" {
            fn RtlGetVersion(info: *mut OSVERSIONINFOW) -> i32;
        }
        // SAFETY: a zeroed struct with its size set, as the call expects.
        let mut v: OSVERSIONINFOW = unsafe { std::mem::zeroed() };
        v.dwOSVersionInfoSize = size_of::<OSVERSIONINFOW>() as u32;
        if unsafe { RtlGetVersion(&mut v) } == 0 {
            return format!("Windows {}.{} build {} ({})", v.dwMajorVersion, v.dwMinorVersion, v.dwBuildNumber, std::env::consts::ARCH);
        }
    }
    format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)
}

fn memory() -> String {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut m: MEMORYSTATUSEX = std::mem::zeroed();
        m.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut m) != 0 {
            let gb = |b: u64| b as f64 / (1u64 << 30) as f64;
            return format!("Physical: {:.1} GB, available {:.1} GB ({}% in use)", gb(m.ullTotalPhys), gb(m.ullAvailPhys), m.dwMemoryLoad);
        }
    }
    "unknown".into()
}

/// Every DXGI adapter: name, dedicated memory, driver version (the user-mode driver's, as DXGI reports it), outputs.
pub fn gpus() -> Vec<String> {
    #[cfg(windows)]
    {
        use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIDevice, IDXGIFactory1};
        use windows::core::Interface;
        let mut out = Vec::new();
        // SAFETY: plain COM calls on objects this function owns; descriptions are copied out before release.
        unsafe {
            let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else { return vec!["DXGI factory could not be created".into()] };
            for i in 0.. {
                let Ok(adapter) = factory.EnumAdapters1(i) else { break };
                let mut desc = std::mem::zeroed();
                if adapter.GetDesc1(&mut desc).is_err() {
                    continue;
                }
                let name = String::from_utf16_lossy(&desc.Description).trim_end_matches('\0').to_string();
                let software = desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0;
                let driver = adapter.CheckInterfaceSupport(&IDXGIDevice::IID).map(|v| format!("{}.{}.{}.{}", (v >> 48) & 0xffff, (v >> 32) & 0xffff, (v >> 16) & 0xffff, v & 0xffff)).unwrap_or_else(|_| "unknown".into());
                let outputs = (0..).take_while(|&j| adapter.EnumOutputs(j).is_ok()).count();
                let vram = desc.DedicatedVideoMemory as f64 / (1u64 << 30) as f64;
                out.push(format!("{name}{}: {vram:.1} GB dedicated, driver {driver}, {outputs} output(s), vendor 0x{:04X} device 0x{:04X}", if software { " (software)" } else { "" }, desc.VendorId, desc.DeviceId));
            }
        }
        if out.is_empty() {
            out.push("no adapters".into());
        }
        out
    }
    #[cfg(not(windows))]
    {
        vec!["(DXGI is Windows only)".into()]
    }
}

/// `ffmpeg.txt`: the build and its hardware decoders (what the capability rows in docs/FORMAT_SUPPORT.md rest on).
pub fn ffmpeg_info() -> String {
    let mut s = String::new();
    for args in [&["-version"][..], &["-hide_banner", "-hwaccels"][..]] {
        let _ = writeln!(s, "> ffmpeg {}", args.join(" "));
        let mut cmd = crate::proc::ffmpeg();
        cmd.args(args);
        match crate::proc::output(&mut cmd) {
            Ok(o) => {
                let _ = writeln!(s, "{}", String::from_utf8_lossy(&o.stdout).trim());
                let err = String::from_utf8_lossy(&o.stderr);
                if !err.trim().is_empty() {
                    let _ = writeln!(s, "{}", err.trim());
                }
            }
            Err(e) => {
                let _ = writeln!(s, "{}", crate::proc::tool_error(&e, "ffmpeg"));
            }
        }
        let _ = writeln!(s);
    }
    s
}

/// The preferences file, with the recent list and folders left out when `privacy`.
fn copy_prefs(prefs: Option<&Path>, to: &Path, privacy: bool) -> Result<(), String> {
    let Some(prefs) = prefs else { return Err("no preferences file".into()) };
    let text = std::fs::read_to_string(prefs).map_err(|e| format!("{}: {e}", prefs.display()))?;
    let text = if privacy {
        let mut v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        if let Some(o) = v.as_object_mut() {
            for k in ["recent", "overlay_library", "audio_editor", "render_recent", "layouts"] {
                o.remove(k);
            }
        }
        serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?
    } else {
        text
    };
    std::fs::write(to, text).map_err(|e| e.to_string())
}

fn copy_logs(logs: Option<&Path>, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    let Some(logs) = logs else { return Err("no log folder".into()) };
    log::logger().flush();
    let files = crate::logger::log_files(logs);
    for f in &files {
        if let Some(name) = f.file_name() {
            std::fs::copy(f, to.join(name)).map_err(|e| format!("{}: {e}", f.display()))?;
        }
    }
    if files.is_empty() {
        std::fs::write(to.join("no-logs.txt"), "No log files were found.\n").map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Every crash report and the three newest minidumps (they can be large).
fn copy_crashes(crashes: Option<&Path>, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    let Some(crashes) = crashes else { return Err("no crash folder".into()) };
    let mut files: Vec<PathBuf> = std::fs::read_dir(crashes).into_iter().flatten().flatten().map(|e| e.path()).collect();
    files.sort();
    files.reverse();
    let mut dumps = 0;
    for f in &files {
        let is_dump = f.extension().is_some_and(|x| x == "dmp");
        if is_dump {
            dumps += 1;
            if dumps > 3 {
                continue;
            }
        }
        if let Some(name) = f.file_name() {
            std::fs::copy(f, to.join(name)).map_err(|e| format!("{}: {e}", f.display()))?;
        }
    }
    if files.is_empty() {
        std::fs::write(to.join("no-crashes.txt"), "No crash reports were found.\n").map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every section is there, and system.txt has its headings.
    #[test]
    fn diagnostic_report_has_every_section() {
        let root = std::env::temp_dir().join("milan-agent").join("r42").join(format!("diag-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("logs")).unwrap();
        std::fs::create_dir_all(root.join("crashes")).unwrap();
        std::fs::write(root.join("logs").join("milan-2026-01-01.log"), "a line\n").unwrap();
        std::fs::write(root.join("crashes").join("crash-20260101-000000.txt"), "boom\n").unwrap();
        std::fs::write(root.join("prefs.json"), r#"{"recent":["C:/secret.mnle"],"look":{"look":"Vegas16","variant":"Dark"}}"#).unwrap();
        let dir = root.join("report");
        let r = create(&dir, Some(&root.join("prefs.json")), Some(&root.join("logs")), Some(&root.join("crashes")), true).unwrap();
        assert_eq!(r.sections.iter().map(|(n, _)| *n).collect::<Vec<_>>(), SECTIONS);
        for (name, res) in &r.sections {
            assert!(res.is_ok(), "{name}: {res:?}");
            assert!(dir.join(name).exists(), "{name} missing");
        }
        let system = std::fs::read_to_string(dir.join("system.txt")).unwrap();
        for heading in ["[Windows]", "[CPU]", "[Memory]", "[GPU]", "[Displays]", crate::logger::VERSION] {
            assert!(system.contains(heading), "{heading} missing in {system}");
        }
        assert!(!system.contains("Executable:"), "privacy leaves the exe path out");
        let prefs = std::fs::read_to_string(dir.join("prefs.json")).unwrap();
        assert!(!prefs.contains("secret") && prefs.contains("Vegas16"), "{prefs}");
        assert!(dir.join("logs").join("milan-2026-01-01.log").exists());
        assert!(dir.join("crashes").join("crash-20260101-000000.txt").exists());
        assert!(std::fs::read_to_string(dir.join("README.txt")).unwrap().contains("system.txt"));
        let ff = std::fs::read_to_string(dir.join("ffmpeg.txt")).unwrap();
        assert!(ff.contains("> ffmpeg -version"), "{ff}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn gpu_list_names_an_adapter() {
        let g = gpus();
        assert!(!g.is_empty());
        assert!(g[0].contains("driver"), "{g:?}");
    }
}
