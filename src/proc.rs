//! Child processes: ffmpeg / ffprobe commands (spawned with `spawn` or `output`, never `Command::output`) without a console window, tied to a Job Object
//! so Windows kills them when Milan exits, crashes or is killed.
use std::io;
use std::process::{Child, Command, Stdio};

const CREATE_NO_WINDOW: u32 = 0x08000000;

/// `ffmpeg -nostdin` with null stdin and no console window.
pub fn ffmpeg() -> Command {
    let mut cmd = tool("ffmpeg");
    cmd.arg("-nostdin");
    cmd
}

/// `ffprobe` with null stdin and no console window (ffprobe has no `-nostdin`).
pub fn ffprobe() -> Command {
    tool("ffprobe")
}

fn tool(name: &str) -> Command {
    let mut cmd = Command::new(name);
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Spawn `cmd` and put the child in Milan's kill-on-close Job Object.
/// A failed assignment is logged and the child is still returned.
pub fn spawn(cmd: &mut Command) -> io::Result<Child> {
    let child = cmd.spawn()?;
    #[cfg(windows)]
    job::assign(&child);
    Ok(child)
}

/// Run `child` at below-normal priority (a render's encoder), so the editor stays responsive while it encodes.
pub fn below_normal(child: &Child) {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::Threading::{BELOW_NORMAL_PRIORITY_CLASS, SetPriorityClass};
        // SAFETY: the handle is the live child's own process handle.
        if unsafe { SetPriorityClass(child.as_raw_handle() as _, BELOW_NORMAL_PRIORITY_CLASS) } == 0 {
            eprintln!("proc: SetPriorityClass failed: {}", io::Error::last_os_error());
        }
    }
    #[cfg(not(windows))]
    let _ = child;
}

thread_local! {
    static LOW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// R19: run the calling thread (a render's) at below-normal priority, so its composing does not compete with the
/// editor; `composite::par_rows` stripes started from it follow it.
pub fn below_normal_thread() {
    LOW.set(true);
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};
        // SAFETY: the pseudo-handle of the calling thread.
        if unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) } == 0 {
            eprintln!("proc: SetThreadPriority failed: {}", io::Error::last_os_error());
        }
    }
}

/// The calling thread was lowered by [`below_normal_thread`].
pub fn is_below_normal() -> bool {
    LOW.get()
}

/// `cmd.output()` through [`spawn`], so a probe hung on a dead share still dies with Milan.
pub fn output(cmd: &mut Command) -> io::Result<std::process::Output> {
    spawn(cmd.stdout(Stdio::piped()).stderr(Stdio::piped()))?.wait_with_output()
}

/// User-facing text for a failed spawn of `tool`.
pub fn tool_error(e: &io::Error, tool: &str) -> String {
    match e.kind() {
        io::ErrorKind::NotFound => format!("{tool} not found next to Milan or on PATH"),
        _ => format!("{tool} failed to start: {e}"),
    }
}

#[cfg(windows)]
mod job {
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;
    use windows_sys::Win32::System::JobObjects::*;

    /// The job handle (as usize, since HANDLE is a raw pointer and not Sync). Never closed:
    /// Windows closes it when Milan exits, and KILL_ON_JOB_CLOSE then kills every child.
    static JOB: OnceLock<usize> = OnceLock::new();

    fn job() -> usize {
        *JOB.get_or_init(|| unsafe {
            let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if h.is_null() {
                eprintln!("proc: CreateJobObjectW failed: {}", std::io::Error::last_os_error());
                return 0;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let size = std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
            if SetInformationJobObject(h, JobObjectExtendedLimitInformation, &info as *const _ as *const _, size) == 0 {
                eprintln!("proc: SetInformationJobObject failed: {}", std::io::Error::last_os_error());
            }
            h as usize
        })
    }

    pub fn assign(child: &std::process::Child) {
        let h = job();
        if h != 0 && unsafe { AssignProcessToJobObject(h as _, child.as_raw_handle() as _) } == 0 {
            eprintln!("proc: AssignProcessToJobObject failed: {}", std::io::Error::last_os_error());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R19: a lowered thread's compose stripes run below normal priority too; other threads stay normal.
    #[cfg(windows)]
    #[test]
    fn render_threads_run_below_normal() {
        use eframe::egui::Color32;
        use windows_sys::Win32::System::Threading::{GetCurrentThread, GetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL, THREAD_PRIORITY_NORMAL};
        let prio = || unsafe { GetThreadPriority(GetCurrentThread()) };
        let stripes = |low: bool| {
            std::thread::spawn(move || {
                if low {
                    below_normal_thread();
                }
                let seen = std::sync::Mutex::new(Vec::new());
                let mut px = vec![Color32::BLACK; 256 * 256];
                crate::engine::composite::par_rows(&mut px, 256, 4, |_, _| seen.lock().unwrap().push((prio(), is_below_normal())));
                (prio(), seen.into_inner().unwrap())
            })
            .join()
            .unwrap()
        };
        let (own, rows) = stripes(true);
        assert_eq!(own, THREAD_PRIORITY_BELOW_NORMAL);
        assert!(rows.len() == 256 && rows.iter().all(|&r| r == (THREAD_PRIORITY_BELOW_NORMAL, true)));
        let (own, rows) = stripes(false);
        assert!(own == THREAD_PRIORITY_NORMAL && rows.iter().all(|&r| r == (THREAD_PRIORITY_NORMAL, false)));
    }

    #[test]
    fn tool_error_text() {
        let e = io::Error::from(io::ErrorKind::NotFound);
        assert_eq!(tool_error(&e, "ffprobe"), "ffprobe not found next to Milan or on PATH");
        let e = io::Error::from(io::ErrorKind::PermissionDenied);
        assert!(tool_error(&e, "ffmpeg").starts_with("ffmpeg failed to start: "));
    }

    /// Needs ffmpeg. `cargo test --release -- --ignored job_object_kills_children`
    /// Re-runs this test binary as a helper that spawns ffmpeg and then gets killed;
    /// the Job Object must take the ffmpeg child down with it.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn job_object_kills_children() {
        use std::io::{BufRead, BufReader, Write};
        use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
        use windows_sys::Win32::System::Threading::*;

        if std::env::var_os("MILAN_JOB_CHILD").is_some() {
            let mut cmd = ffmpeg();
            cmd.args(["-v", "error", "-f", "lavfi", "-i", "nullsrc", "-f", "null", "-"]).stdout(Stdio::null()).stderr(Stdio::null());
            let child = spawn(&mut cmd).expect("spawn ffmpeg");
            println!("PID {}", child.id());
            std::io::stdout().flush().unwrap();
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1)); // the parent kills us
            }
        }

        let mut helper = Command::new(std::env::current_exe().unwrap());
        helper
            .args(["--ignored", "--exact", "proc::tests::job_object_kills_children", "--nocapture", "--test-threads=1"])
            .env("MILAN_JOB_CHILD", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        {
            use std::os::windows::process::CommandExt;
            helper.creation_flags(CREATE_NO_WINDOW);
        }
        let mut helper = helper.spawn().unwrap();
        // libtest prints "test <name> ... " on the same line before the helper's output.
        let pid: Option<u32> = BufReader::new(helper.stdout.take().unwrap())
            .lines()
            .map_while(Result::ok)
            .find_map(|l| l.rsplit_once("PID ").and_then(|(_, p)| p.trim().parse().ok()));
        let _ = helper.kill();
        helper.wait().unwrap();
        let pid = pid.expect("helper printed no ffmpeg PID");

        // The ffmpeg PID is gone (OpenProcess fails) or exits within 5 s.
        let exited = unsafe {
            let h = OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, 0, pid);
            h.is_null() || {
                let ok = WaitForSingleObject(h, 5000) == WAIT_OBJECT_0;
                if !ok {
                    TerminateProcess(h, 1); // never leave the stray ffmpeg running
                }
                CloseHandle(h);
                ok
            }
        };
        assert!(exited, "ffmpeg {pid} outlived its parent");
    }
}
