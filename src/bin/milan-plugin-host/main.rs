//! milan-plugin-host.exe: runs OFX plug-ins (R32, R33a, R33b), AE-format effects (A2-A5, `aex`) and VST3 and CLAP audio
//! effects (R34a, R34b, R35) for
//! Milan Pro in a process of their own, so a plug-in that crashes or hangs never takes the editor down. Milan starts it with `--pipe <name>`; the host serves that named pipe
//! (requests in `proto.rs`, one reply each) and maps the frames Milan puts in shared memory. It has no window and no
//! console, and shows no error dialogs: a crash ends the process silently and Milan reports it.
#![windows_subsystem = "windows"]
// The OFX names (kOfxStatOK...) are kept as the headers spell them.
#![allow(non_upper_case_globals)]

#[cfg(not(all(windows, target_arch = "x86_64")))]
compile_error!("milan-plugin-host hosts 64-bit Windows OFX plug-ins (its variadic parameter functions rely on the Win64 ABI)");

mod aex;
mod clap;
mod dmo;
mod editor;
mod host;
mod ofx;
#[path = "../../engine/ofx/proto.rs"]
mod proto;
mod suites;
mod vst3;

use std::fs::File;
use std::os::windows::io::FromRawHandle;
use proto::{Reply, Req};

fn main() {
    quiet();
    probe_mode();
    let mut args = std::env::args().skip(1);
    let name = match (args.next().as_deref(), args.next()) {
        (Some("--pipe"), Some(n)) if n.starts_with(r"\\.\pipe\") => n,
        _ => std::process::exit(2),
    };
    let Some(mut pipe) = serve(&name) else { std::process::exit(3) };
    let mut host = host::Host::new();
    let mut audio = Audio::None;
    // A2: the AE-format effect file this host runs, when it was given one.
    let mut aexm: Option<aex::Module> = None;
    // Audio blocks travel in shared memory Milan names with each request (mapped once per name).
    let mut shm: Option<(String, host::Shm)> = None;
    loop {
        let Ok(req) = proto::read_msg::<Req>(&mut pipe) else { break };
        let reply = match req {
            // A VST3 or CLAP module by its extension; anything else is an OFX binary.
            Req::Load { binary } => match binary.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
                Some("vst3") => match vst3::Module::load(&binary) {
                    Ok(m) => {
                        audio = Audio::Vst3(m);
                        Reply::Loaded(1)
                    }
                    Err(e) => Reply::Error(e),
                },
                Some("clap") => match clap::Module::load(&binary) {
                    Ok(m) => {
                        audio = Audio::Clap(m);
                        Reply::Loaded(1)
                    }
                    Err(e) => Reply::Error(e),
                },
                Some("aex") => match aex::Module::load(&binary) {
                    Ok(m) => {
                        let n = m.count();
                        aexm = Some(m);
                        Reply::Loaded(n)
                    }
                    Err(e) => Reply::Error(e),
                },
                _ => host.load(&binary),
            },
            Req::Describe { index } => match &mut aexm {
                Some(m) => m.describe(index),
                None => host.describe(index),
            },
            Req::Render(r) => match &mut aexm {
                Some(m) => m.render(&r, &mut pipe),
                None => host.render(&r),
            },
            // An AE-format effect has no push buttons; making the instance waits for its first frame.
            Req::Button(r, name) => match &aexm {
                Some(_) => Reply::Rendered(Default::default()),
                None => host.button(&r, &name),
            },
            Req::AexFlatten { index, instance } => match &mut aexm {
                Some(m) => m.flatten(index, &instance),
                None => Reply::Error("no AE-format effect is loaded".into()),
            },
            Req::Frame(_) => Reply::Error("no frame was asked for".into()),
            Req::Panic => panic!("asked to panic"),
            Req::GpuCheck => host::gpu_check(),
            Req::AScan => match &mut audio {
                Audio::Vst3(m) => Reply::AScanned(m.scan()),
                Audio::Clap(m) => Reply::AScanned(m.scan()),
                Audio::None => Reply::Error("no VST3 or CLAP module is loaded".into()),
            },
            Req::AOpen(inst) => a_reply(match &mut audio {
                Audio::Vst3(m) => m.open(&inst),
                Audio::Clap(m) => m.open(&inst),
                Audio::None => Err("no VST3 or CLAP module is loaded".into()),
            }),
            Req::AProcess(p) => {
                let mem = match &mut shm {
                    Some((name, s)) if *name == p.shm && s.size >= p.shm_size => Ok(s),
                    _ => host::Shm::open(&p.shm, p.shm_size).map(|s| &mut shm.insert((p.shm.clone(), s)).1),
                };
                a_reply(mem.and_then(|s| match &mut audio {
                    Audio::Vst3(m) => m.process(&p, s.bytes()),
                    Audio::Clap(m) => m.process(&p, s.bytes()),
                    Audio::None => Err("no VST3 or CLAP module is loaded".into()),
                }))
            }
            Req::AGetState(inst) => match match &mut audio {
                Audio::Vst3(m) => m.get_state(&inst),
                Audio::Clap(m) => m.get_state(&inst),
                Audio::None => Err("no VST3 or CLAP module is loaded".into()),
            } {
                Ok(s) => Reply::AStateIs(s),
                Err(e) => Reply::Error(e),
            },
            Req::ASetState(inst, st) => a_reply(match &mut audio {
                Audio::Vst3(m) => m.set_state(&inst, &st),
                Audio::Clap(m) => m.set_state(&inst, &st),
                Audio::None => Err("no VST3 or CLAP module is loaded".into()),
            }),
            Req::AEditor(inst, op) => a_reply(match &mut audio {
                Audio::Vst3(m) => m.editor(&inst, &op),
                Audio::Clap(_) => Err("CLAP editors are not shown in this version of Milan Pro".into()),
                Audio::None => Err("no VST3 or CLAP module is loaded".into()),
            }),
            Req::AClose(instance) => {
                match &mut audio {
                    Audio::Vst3(m) => m.close(&instance),
                    Audio::Clap(m) => m.close(&instance),
                    Audio::None => {}
                }
                Reply::AOut(Default::default())
            }
            Req::DmoProbe => Reply::Rendered(proto::Outcome { messages: dmo::probe(), ..Default::default() }),
            Req::Quit => {
                host.shutdown();
                if let Some(m) = &mut aexm {
                    m.shutdown();
                }
                match &mut audio {
                    Audio::Vst3(m) => m.shutdown(),
                    Audio::Clap(m) => m.shutdown(),
                    Audio::None => {}
                }
                let _ = proto::write_msg(&mut pipe, &Reply::Rendered(Default::default()));
                break;
            }
        };
        if proto::write_msg(&mut pipe, &reply).is_err() {
            break;
        }
    }
    // Skip the plug-ins' unload and the DLL's detach: a hung or crashing unload must not keep the process alive.
    std::process::exit(0);
}

/// The audio module this host runs (one per host, as one OFX binary per host).
enum Audio {
    None,
    Vst3(vst3::Module),
    Clap(clap::Module),
}

fn a_reply(r: Result<proto::AOut, String>) -> Reply {
    match r {
        Ok(o) => Reply::AOut(o),
        Err(e) => Reply::Error(e),
    }
}

/// No crash dialog, no "cannot find DLL" box, no Windows Error Reporting window: the user must never see the host.
fn quiet() {
    use windows_sys::Win32::System::Diagnostics::Debug::{SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SEM_NOOPENFILEERRORBOX, SetErrorMode};
    use windows_sys::Win32::System::ErrorReporting::{WER_FAULT_REPORTING_NO_UI, WerSetFlags};
    // SAFETY: process-wide flags, set before any plug-in loads.
    unsafe {
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX);
        WerSetFlags(WER_FAULT_REPORTING_NO_UI);
    }
}

/// R33c compatibility probes (`MILAN_HOST_DESKTOP=<name>`): this thread moves to a desktop of that name, made if
/// needed, so a plug-in's licence or registration dialog opens where nobody sees it and cannot take focus; the probe's
/// watchdog ends the host when the dialog waits. With `MILAN_HOST_LOG=<file>`, a thread notes every window the plug-in
/// opens on this thread ("dialog: <title>") in that file, so the report can tell a dialog from a hang.
fn probe_mode() {
    use windows_sys::Win32::System::StationsAndDesktops::{CreateDesktopW, DESKTOP_CREATEWINDOW, DESKTOP_READOBJECTS, DESKTOP_WRITEOBJECTS, SetThreadDesktop};
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumThreadWindows, GetWindowTextW, IsWindowVisible};
    // The host's stderr goes nowhere (Milan opens it on a null handle), so a desktop failure is noted in the log file.
    let mut desktop_error = None;
    if let Some(name) = std::env::var_os("MILAN_HOST_DESKTOP").filter(|n| !n.is_empty()) {
        let w: Vec<u16> = name.to_string_lossy().encode_utf16().chain([0]).collect();
        // SAFETY: a NUL-terminated name; the desktop handle is kept for the life of the process.
        unsafe {
            let d = CreateDesktopW(w.as_ptr(), std::ptr::null(), std::ptr::null(), 0, DESKTOP_CREATEWINDOW | DESKTOP_READOBJECTS | DESKTOP_WRITEOBJECTS, std::ptr::null());
            if d.is_null() || SetThreadDesktop(d) == 0 {
                desktop_error = Some(format!("cannot use desktop {}: {}", name.to_string_lossy(), std::io::Error::last_os_error()));
            }
        }
    }
    let Some(log) = std::env::var_os("MILAN_HOST_LOG").filter(|n| !n.is_empty()) else { return };
    if let Some(e) = desktop_error {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).create(true).open(&log) {
            let _ = writeln!(f, "error: {e}");
        }
    }
    // SAFETY: a plain query.
    let main_thread = unsafe { GetCurrentThreadId() };
    unsafe extern "system" fn each(h: windows_sys::Win32::Foundation::HWND, titles: isize) -> windows_sys::Win32::Foundation::BOOL {
        // SAFETY: `titles` is the Vec the enumerating thread passed; the window handle is live for this call.
        unsafe {
            if IsWindowVisible(h) != 0 {
                let mut buf = [0u16; 256];
                let n = GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32);
                (*(titles as *mut Vec<String>)).push(String::from_utf16_lossy(&buf[..n.max(0) as usize]));
            }
        }
        1
    }
    let _ = std::thread::Builder::new().name("milan-host-window-watch".into()).spawn(move || {
        let mut seen = std::collections::BTreeSet::new();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let mut titles: Vec<String> = Vec::new();
            // SAFETY: the callback only reads the handle and writes into `titles`, which outlives the call.
            unsafe { EnumThreadWindows(main_thread, Some(each), &mut titles as *mut Vec<String> as isize) };
            for t in titles {
                if seen.insert(t.clone()) {
                    use std::io::Write;
                    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).create(true).open(&log) {
                        let _ = writeln!(f, "dialog: {t}");
                    }
                }
            }
        }
    });
}

/// Create pipe `name` (one instance, local clients only) and wait for Milan to open it.
fn serve(name: &str) -> Option<File> {
    use windows_sys::Win32::Foundation::{ERROR_PIPE_CONNECTED, GetLastError, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
    use windows_sys::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT};
    let w: Vec<u16> = name.encode_utf16().chain([0]).collect();
    // SAFETY: a NUL-terminated name; the handle is owned by the returned File.
    unsafe {
        let h = CreateNamedPipeW(w.as_ptr(), PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS, 1, 1 << 16, 1 << 16, 0, std::ptr::null());
        if h == INVALID_HANDLE_VALUE {
            return None;
        }
        if ConnectNamedPipe(h, std::ptr::null_mut()) == 0 && GetLastError() != ERROR_PIPE_CONNECTED {
            windows_sys::Win32::Foundation::CloseHandle(h);
            return None;
        }
        Some(File::from_raw_handle(h as _))
    }
}
