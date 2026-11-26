//! R44: Milan Pro's own file logger behind the `log` crate, the crash hooks, and the time formatting the dialogs
//! share. Named `logger`, not `log`, so `log::warn!` keeps meaning the `log` crate everywhere in the tree.
//!
//! - `%LOCALAPPDATA%\MilanPro\logs\milan-<date>.log`, 2 MB a file and 5 files kept (`MAX_BYTES`, `MAX_FILES`); the
//!   level comes from Preferences (`set_level`). Other crates' records are kept from Warn up unless the level is Trace.
//!   The last 500 lines are also kept in memory for crash reports. `MILAN_APPDATA=<folder>` moves the logs and crash
//!   files there (tests, the harness); the harness without it logs to stderr only.
//! - A panic hook writes `crashes\crash-<time>.txt`: version, the panic, the backtrace, the project summary (counts
//!   only when "privacy" is on) and the last log lines. The unhandled-exception filter (an access violation in plug-in
//!   or driver code, which a Rust panic never is) writes `crash-<time>.dmp` with `MiniDumpWriteDump` and a `.txt` beside
//!   it. The next start offers the newest report (`crash_reports`); nothing is ever sent anywhere by itself.
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use log::{LevelFilter, Log, Metadata, Record};

pub const MAX_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_FILES: usize = 5;
pub const RING: usize = 500;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The per-user local data folder: `MILAN_APPDATA` when set, else %LOCALAPPDATA%\MilanPro.
pub fn local_root() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("MILAN_APPDATA").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("MilanPro"))
}

pub fn logs_dir() -> Option<PathBuf> {
    Some(local_root()?.join("logs"))
}

pub fn crashes_dir() -> Option<PathBuf> {
    Some(local_root()?.join("crashes"))
}

struct Inner {
    dir: Option<PathBuf>,
    file: Option<File>,
    /// The date the open file is named after; a new day opens a new file.
    date: String,
    written: u64,
    max_bytes: u64,
    max_files: usize,
    ring: VecDeque<String>,
    stderr: bool,
}

pub struct FileLogger {
    inner: Mutex<Inner>,
    /// `LevelFilter` as usize (`LevelFilter::Off` = 0 ... `Trace` = 5).
    level: AtomicUsize,
}

static LOGGER: OnceLock<FileLogger> = OnceLock::new();
static PROJECT: Mutex<Option<(String, String)>> = Mutex::new(None);
static PRIVACY: AtomicUsize = AtomicUsize::new(0);

/// Install the logger once: files in `dir` (None: stderr only), records at `level` and up.
pub fn init(dir: Option<PathBuf>, level: LevelFilter) {
    init_with(dir, level, MAX_BYTES, MAX_FILES);
}

pub fn init_with(dir: Option<PathBuf>, level: LevelFilter, max_bytes: u64, max_files: usize) {
    let logger = LOGGER.get_or_init(|| FileLogger {
        inner: Mutex::new(Inner { dir: None, file: None, date: String::new(), written: 0, max_bytes, max_files, ring: VecDeque::with_capacity(RING), stderr: cfg!(debug_assertions) }),
        level: AtomicUsize::new(0),
    });
    {
        let mut i = logger.inner.lock().unwrap_or_else(|e| e.into_inner());
        (i.dir, i.file, i.max_bytes, i.max_files) = (dir, None, max_bytes, max_files);
        i.stderr |= i.dir.is_none();
    }
    set_level(level);
    let _ = log::set_logger(logger);
    log::set_max_level(LevelFilter::Trace);
}

/// Preferences › Update › Log level.
pub fn set_level(level: LevelFilter) {
    if let Some(l) = LOGGER.get() {
        l.level.store(level as usize, Ordering::Relaxed);
    }
}

/// "error" | "warn" | "info" | "debug" | "trace" (anything else: info).
pub fn level_from_name(name: &str) -> LevelFilter {
    match name.trim().to_ascii_lowercase().as_str() {
        "off" => LevelFilter::Off,
        "error" => LevelFilter::Error,
        "warn" | "warning" => LevelFilter::Warn,
        "debug" => LevelFilter::Debug,
        "trace" => LevelFilter::Trace,
        _ => LevelFilter::Info,
    }
}

pub const LEVEL_NAMES: [&str; 5] = ["error", "warn", "info", "debug", "trace"];

/// The last `RING` lines logged (newest last).
pub fn recent_lines() -> Vec<String> {
    LOGGER.get().map(|l| l.inner.lock().unwrap_or_else(|e| e.into_inner()).ring.iter().cloned().collect()).unwrap_or_default()
}

/// What a crash report says about the project: `(with paths, without paths)`.
pub fn set_project(with_paths: String, without_paths: String) {
    let mut p = PROJECT.lock().unwrap_or_else(|e| e.into_inner());
    if p.as_ref().is_none_or(|(a, b)| *a != with_paths || *b != without_paths) {
        *p = Some((with_paths, without_paths));
    }
}

/// Preferences › Update › "Crash reports leave out file names and paths".
pub fn set_privacy(on: bool) {
    PRIVACY.store(usize::from(on), Ordering::Relaxed);
}

fn project_summary() -> String {
    let p = PROJECT.lock().unwrap_or_else(|e| e.into_inner());
    match &*p {
        Some((full, bare)) => if PRIVACY.load(Ordering::Relaxed) != 0 { bare.clone() } else { full.clone() },
        None => "(no project)".into(),
    }
}

impl Log for FileLogger {
    fn enabled(&self, m: &Metadata) -> bool {
        let level = self.level.load(Ordering::Relaxed);
        let own = m.target().starts_with("milan_pro") || m.target().starts_with("milan_plugin_host");
        // The GPU stack warns every frame about the driver ("Unrecognized present mode", "Suboptimal present"), which
        // would turn the 2 MB files over in minutes: errors only, unless everything is asked for.
        let chatty = ["wgpu", "naga", "egui_wgpu"].iter().any(|c| m.target().starts_with(c));
        let cap = if own || level >= LevelFilter::Trace as usize {
            level
        } else if chatty {
            level.min(LevelFilter::Error as usize)
        } else {
            level.min(LevelFilter::Warn as usize)
        };
        m.level() as usize <= cap
    }

    fn log(&self, r: &Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let now = SystemTime::now();
        let ms = now.duration_since(UNIX_EPOCH).unwrap_or_default().subsec_millis();
        let target = r.target().strip_prefix("milan_pro::").unwrap_or(r.target());
        let line = format!("{}.{ms:03} {:<5} {target}: {}", format_time(now, true), r.level(), r.args());
        let mut i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if i.ring.len() >= RING {
            i.ring.pop_front();
        }
        i.ring.push_back(line.clone());
        if i.stderr {
            let _ = writeln!(std::io::stderr(), "{line}");
        }
        i.write_line(&line);
    }

    fn flush(&self) {
        if let Some(f) = &mut self.inner.lock().unwrap_or_else(|e| e.into_inner()).file {
            let _ = f.flush();
        }
    }
}

impl Inner {
    fn write_line(&mut self, line: &str) {
        let Some(dir) = self.dir.clone() else { return };
        let date = date_stamp(SystemTime::now());
        if self.file.is_some() && (date != self.date || self.written >= self.max_bytes) {
            self.file = None;
            if date == self.date {
                rotate_full(&dir, &self.date, self.max_files);
            }
        }
        if self.file.is_none() {
            let path = dir.join(format!("milan-{date}.log"));
            let _ = std::fs::create_dir_all(&dir);
            match OpenOptions::new().append(true).create(true).open(&path) {
                Ok(f) => {
                    self.written = f.metadata().map(|m| m.len()).unwrap_or(0);
                    self.file = Some(f);
                    self.date = date;
                    prune(&dir, self.max_files);
                }
                Err(_) => return,
            }
        }
        if let Some(f) = &mut self.file {
            if writeln!(f, "{line}").is_ok() {
                self.written += line.len() as u64 + 1;
            }
        }
    }
}

/// `milan-<date>.log` is full: it becomes `milan-<date>.1.log` (an older `.1` moves to `.2`, and so on).
fn rotate_full(dir: &Path, date: &str, max_files: usize) {
    let name = |k: usize| if k == 0 { dir.join(format!("milan-{date}.log")) } else { dir.join(format!("milan-{date}.{k}.log")) };
    let _ = std::fs::remove_file(name(max_files.max(1)));
    for k in (1..max_files.max(1)).rev() {
        let _ = std::fs::rename(name(k), name(k + 1));
    }
    let _ = std::fs::rename(name(0), name(1));
}

/// Every `milan-*.log` in `dir`, newest first (by modification time).
pub fn log_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<(SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("milan-") && n.ends_with(".log")))
        .map(|p| (std::fs::metadata(&p).and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH), p))
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    files.into_iter().map(|(_, p)| p).collect()
}

/// Keep the newest `max_files` log files.
fn prune(dir: &Path, max_files: usize) {
    for p in log_files(dir).into_iter().skip(max_files.max(1)) {
        let _ = std::fs::remove_file(p);
    }
}

// ---- Crash reports ----

/// Every `crash-*.txt` in the crashes folder, newest first.
pub fn crash_reports(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("crash-") && n.ends_with(".txt")))
        .collect();
    v.sort();
    v.reverse();
    v
}

/// Write `crash-<time>.txt` into `dir`: `what` (the panic or exception), the backtrace, the project and the log tail.
pub fn write_crash_report(dir: &Path, what: &str, backtrace: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stamp = file_stamp(SystemTime::now());
    let mut path = dir.join(format!("crash-{stamp}.txt"));
    for n in 2.. {
        if !path.exists() {
            break;
        }
        path = dir.join(format!("crash-{stamp}-{n}.txt"));
    }
    let mut f = File::create(&path)?;
    writeln!(f, "Milan Pro {VERSION} crash report, {}", format_time(SystemTime::now(), true))?;
    writeln!(f, "OS: {} {}", std::env::consts::OS, std::env::consts::ARCH)?;
    writeln!(f, "Thread: {}", std::thread::current().name().unwrap_or("?"))?;
    writeln!(f)?;
    writeln!(f, "{what}")?;
    writeln!(f)?;
    writeln!(f, "Project: {}", project_summary())?;
    writeln!(f)?;
    writeln!(f, "Backtrace (release builds are stripped: addresses only):")?;
    writeln!(f, "{backtrace}")?;
    writeln!(f)?;
    writeln!(f, "Last log lines:")?;
    for l in recent_lines() {
        writeln!(f, "{l}")?;
    }
    f.flush()?;
    Ok(path)
}

static CRASH_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

/// The panic hook and, on Windows, the unhandled-exception filter. `dir`: the crashes folder (None: log only).
pub fn install_crash_hooks(dir: Option<PathBuf>) {
    let _ = CRASH_DIR.set(dir.clone());
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let what = format!("Panic: {info}");
        log::error!("{what}");
        if let Some(dir) = &dir {
            let bt = std::backtrace::Backtrace::force_capture().to_string();
            match write_crash_report(dir, &what, &bt) {
                Ok(p) => log::error!("crash report written to {}", p.display()),
                Err(e) => log::error!("could not write the crash report: {e}"),
            }
        }
        if let Some(l) = LOGGER.get() {
            l.flush();
        }
        previous(info);
    }));
    #[cfg(windows)]
    // SAFETY: a plain registration of a filter that lives for the whole process.
    unsafe {
        windows_sys::Win32::System::Diagnostics::Debug::SetUnhandledExceptionFilter(Some(exception_filter));
    }
}

/// A native crash (not a Rust panic): the minidump and a report beside it, then the process ends.
#[cfg(windows)]
unsafe extern "system" fn exception_filter(ep: *const windows_sys::Win32::System::Diagnostics::Debug::EXCEPTION_POINTERS) -> i32 {
    // SAFETY: the pointer comes from the system for this very exception.
    let (code, at) = unsafe {
        let rec = (*ep).ExceptionRecord;
        if rec.is_null() { (0, 0usize) } else { ((*rec).ExceptionCode as u32, (*rec).ExceptionAddress as usize) }
    };
    if let Some(dir) = CRASH_DIR.get().and_then(|d| d.clone()) {
        // SAFETY: as above.
        let dump = unsafe { write_minidump(&dir, ep) };
        let what = format!("Unhandled exception 0x{code:08X} at 0x{at:016X}\nMinidump: {}", dump.as_deref().map_or("(not written)".to_string(), |p| p.display().to_string()));
        let _ = write_crash_report(&dir, &what, "(see the minidump)");
    }
    1 // EXCEPTION_EXECUTE_HANDLER: the process ends
}

/// `crash-<time>.dmp` with the exception context: what a debugger opens to see where a native crash happened.
#[cfg(windows)]
unsafe fn write_minidump(dir: &Path, ep: *const windows_sys::Win32::System::Diagnostics::Debug::EXCEPTION_POINTERS) -> Option<PathBuf> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Diagnostics::Debug::{MINIDUMP_EXCEPTION_INFORMATION, MiniDumpWithIndirectlyReferencedMemory, MiniDumpWithThreadInfo, MiniDumpWriteDump};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId};
    std::fs::create_dir_all(dir).ok()?;
    let path = dir.join(format!("crash-{}.dmp", file_stamp(SystemTime::now())));
    let file = File::create(&path).ok()?;
    // SAFETY: the handles are this process's own; `ep` is the live exception for this call.
    let ok = unsafe {
        let info = MINIDUMP_EXCEPTION_INFORMATION { ThreadId: GetCurrentThreadId(), ExceptionPointers: ep as *mut _, ClientPointers: 0 };
        MiniDumpWriteDump(GetCurrentProcess(), GetCurrentProcessId(), file.as_raw_handle() as _, MiniDumpWithIndirectlyReferencedMemory | MiniDumpWithThreadInfo, &info, std::ptr::null(), std::ptr::null()) != 0
    };
    drop(file);
    if ok {
        Some(path)
    } else {
        let _ = std::fs::remove_file(&path);
        None
    }
}

// ---- Time ----

/// Minutes the local clock is ahead of UTC right now (a time from another DST period is off by the DST hour).
pub fn local_offset_minutes() -> i64 {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Foundation::SYSTEMTIME;
        use windows_sys::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime};
        let (mut local, mut utc): (SYSTEMTIME, SYSTEMTIME) = (std::mem::zeroed(), std::mem::zeroed());
        GetSystemTime(&mut utc);
        GetLocalTime(&mut local);
        let minutes = |t: &SYSTEMTIME| days_from_civil(t.wYear as i64, t.wMonth as u32, t.wDay as u32) * 1440 + i64::from(t.wHour) * 60 + i64::from(t.wMinute);
        minutes(&local) - minutes(&utc)
    }
    #[cfg(not(windows))]
    {
        0
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// (year, month, day) of a day count since 1970-01-01.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `t` in local time: "YYYY-MM-DD HH:MM" (`seconds`: "YYYY-MM-DD HH:MM:SS").
pub fn format_time(t: SystemTime, seconds: bool) -> String {
    format_at(t, local_offset_minutes(), seconds)
}

/// `format_time` for a file name: "YYYYMMDD-HHMMSS".
pub fn file_stamp(t: SystemTime) -> String {
    let s = format_at(t, local_offset_minutes(), true);
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    format!("{}-{}", &digits[..8], &digits[8..])
}

/// The local date of `t`: "YYYY-MM-DD" (the daily log file's name).
pub fn date_stamp(t: SystemTime) -> String {
    format_at(t, local_offset_minutes(), false)[..10].to_string()
}

fn format_at(t: SystemTime, offset_minutes: i64, seconds: bool) -> String {
    let secs = t.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO).as_secs() as i64 + offset_minutes * 60;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    let (hh, mm, ss) = (rem / 3600, rem % 3600 / 60, rem % 60);
    if seconds { format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02}") } else { format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::Level;

    #[test]
    fn civil_dates_round_trip() {
        for (y, m, d, days) in [(1970, 1, 1, 0), (2000, 2, 29, 11016), (2026, 11, 26, 20783), (1969, 12, 31, -1)] {
            assert_eq!(days_from_civil(y, m, d), days, "{y}-{m}-{d}");
            assert_eq!(civil_from_days(days), (y, m, d));
        }
        let t = UNIX_EPOCH + Duration::from_secs(20783 * 86400 + 13 * 3600 + 5 * 60 + 9);
        assert_eq!(format_at(t, 0, true), "2026-11-26 13:05:09");
        assert_eq!(format_at(t, 60, false), "2026-11-26 14:05");
        assert_eq!(format_at(t, -14 * 60, false), "2026-11-25 23:05");
        assert_eq!(file_stamp(UNIX_EPOCH).len(), "YYYYMMDD-HHMMSS".len());
        assert_eq!(date_stamp(t).len(), 10);
        assert_eq!(level_from_name("Debug"), LevelFilter::Debug);
        assert_eq!(level_from_name("?"), LevelFilter::Info);
    }

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join("milan-agent").join("r42").join(format!("{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// The logger is process-wide, so one test drives it: rotation at the size limit keeps `max_files` files, and
    /// the ring keeps the last lines for crash reports.
    #[test]
    fn log_rotation_keeps_n_files_and_the_ring_the_last_lines() {
        let dir = temp("logs");
        init_with(Some(dir.clone()), LevelFilter::Debug, 4096, 3);
        for n in 0..600 {
            log::info!("rotation-test line {n} {}", "x".repeat(60));
        }
        log::logger().flush();
        let files = log_files(&dir);
        assert_eq!(files.len(), 3, "{files:?}");
        assert!(files.iter().any(|f| f.file_name().unwrap().to_str().unwrap().ends_with(".1.log")), "{files:?}");
        let total: u64 = files.iter().map(|f| std::fs::metadata(f).unwrap().len()).sum();
        assert!(total <= 3 * (4096 + 256), "{total}");
        // Other tests of this process may log in between: only this test's lines are counted.
        let ring = recent_lines();
        assert_eq!(ring.len(), RING);
        let mine: Vec<&String> = ring.iter().filter(|l| l.contains("rotation-test line ")).collect();
        assert!(mine.last().unwrap().contains("line 599"), "{:?}", mine.last());
        assert!(mine.len() >= 400 && mine.len() <= RING, "{}", mine.len());
        // Levels: a lowered level drops records; other crates are capped at warn.
        set_level(LevelFilter::Error);
        log::warn!("dropped");
        assert!(!recent_lines().last().unwrap().contains("dropped"));
        set_level(LevelFilter::Debug);
        assert!(!log::logger().enabled(&Metadata::builder().level(Level::Info).target("ureq::x").build()));
        assert!(log::logger().enabled(&Metadata::builder().level(Level::Warn).target("ureq::x").build()));
        // The GPU stack's per-frame driver warnings stay out; its errors get in.
        assert!(!log::logger().enabled(&Metadata::builder().level(Level::Warn).target("wgpu_hal::vulkan::conv").build()));
        assert!(log::logger().enabled(&Metadata::builder().level(Level::Error).target("wgpu_core::device").build()));
        // A crash report carries the version, the panic text, the project and the log tail.
        set_project("Show.mnle at C:/x (3 events)".into(), "3 events".into());
        set_privacy(true);
        let p = write_crash_report(&dir, "Panic: boom", "bt").unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains(VERSION) && text.contains("Panic: boom") && text.contains("Project: 3 events") && !text.contains("C:/x") && text.contains("rotation-test line 599"), "{text}");
        set_privacy(false);
        let p2 = write_crash_report(&dir, "Panic: again", "bt").unwrap();
        assert!(std::fs::read_to_string(&p2).unwrap().contains("Project: Show.mnle at C:/x"));
        assert_eq!(crash_reports(&dir).len(), 2);
        assert!(crash_reports(&dir)[0] >= crash_reports(&dir)[1], "newest first");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Run in a child process by `panic_hook_writes_a_crash_file`: installs the hooks into MILAN_PANIC_TEST_DIR and panics.
    #[test]
    fn panic_hook_child() {
        let Ok(dir) = std::env::var("MILAN_PANIC_TEST_DIR") else { return };
        let dir = PathBuf::from(dir);
        init(Some(dir.join("logs")), LevelFilter::Info);
        install_crash_hooks(Some(dir.join("crashes")));
        log::info!("the line before the crash");
        set_project("P".into(), "P".into());
        let boom: Option<u8> = None;
        let _ = boom.expect("crash test panic");
    }

    /// The panic hook writes crash-<time>.txt (checked in a child process, as a panic in this one would fail the test).
    #[test]
    fn panic_hook_writes_a_crash_file() {
        let dir = temp("panic");
        let exe = std::env::current_exe().unwrap();
        let out = std::process::Command::new(&exe)
            .args(["--exact", "logger::tests::panic_hook_child", "--nocapture", "--test-threads=1"])
            .env("MILAN_PANIC_TEST_DIR", &dir)
            .output()
            .unwrap();
        assert!(!out.status.success(), "the child's test panics");
        let reports = crash_reports(&dir.join("crashes"));
        assert_eq!(reports.len(), 1, "{reports:?}\n{}", String::from_utf8_lossy(&out.stderr));
        let text = std::fs::read_to_string(&reports[0]).unwrap();
        assert!(text.contains("crash test panic") && text.contains(VERSION) && text.contains("the line before the crash") && text.contains("Project: P") && text.contains("Backtrace"), "{text}");
        assert!(!log_files(&dir.join("logs")).is_empty(), "the log file was written too");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// R44: every stderr print went to the logger (the release exe has no console).
    #[test]
    fn no_eprintln_left_in_the_tree() {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        let mut files = Vec::new();
        walk(Path::new(env!("CARGO_MANIFEST_DIR")).join("src").as_path(), &mut files);
        assert!(files.len() > 50);
        let needle = concat!("eprint", "ln!");
        let hits: Vec<String> = files.iter().filter(|f| std::fs::read_to_string(f).unwrap_or_default().contains(needle)).map(|f| f.display().to_string()).collect();
        assert!(hits.is_empty(), "{needle} in {hits:?}");
    }
}
