//! milan-plugin-host.exe: runs OFX plug-ins for Milan Pro in a process of their own, so a plug-in that crashes or hangs
//! never takes the editor down (R32, R33a, R33b). Milan starts it with `--pipe <name>`; the host serves that named pipe
//! (requests in `proto.rs`, one reply each) and maps the frames Milan puts in shared memory. It has no window and no
//! console, and shows no error dialogs: a crash ends the process silently and Milan reports it.
#![windows_subsystem = "windows"]
// The OFX names (kOfxStatOK...) are kept as the headers spell them.
#![allow(non_upper_case_globals)]

#[cfg(not(all(windows, target_arch = "x86_64")))]
compile_error!("milan-plugin-host hosts 64-bit Windows OFX plug-ins (its variadic parameter functions rely on the Win64 ABI)");

mod host;
mod ofx;
#[path = "../../engine/ofx/proto.rs"]
mod proto;
mod suites;

use std::fs::File;
use std::os::windows::io::FromRawHandle;
use proto::{Reply, Req};

fn main() {
    quiet();
    let mut args = std::env::args().skip(1);
    let name = match (args.next().as_deref(), args.next()) {
        (Some("--pipe"), Some(n)) if n.starts_with(r"\\.\pipe\") => n,
        _ => std::process::exit(2),
    };
    let Some(mut pipe) = serve(&name) else { std::process::exit(3) };
    let mut host = host::Host::new();
    loop {
        let Ok(req) = proto::read_msg::<Req>(&mut pipe) else { break };
        let reply = match req {
            Req::Load { binary } => host.load(&binary),
            Req::Describe { index } => host.describe(index),
            Req::Render(r) => host.render(&r),
            Req::Panic => panic!("asked to panic"),
            Req::Quit => {
                host.shutdown();
                let _ = proto::write_msg(&mut pipe, &Reply::Rendered);
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
