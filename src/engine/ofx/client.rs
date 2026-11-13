//! Milan's side of the plug-in host (R32): start `milan-plugin-host.exe` in the Job Object with no window, connect to
//! its named pipe, send requests under a watchdog that kills a host which does not answer in time, and move frames
//! through shared memory. Nothing here loads plug-in code.
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use super::proto::{self, Reply, Req};

/// Loading a binary (its DLL load and every plug-in's load action): generous, a first load may be scanned by antivirus.
pub const LOAD_TIMEOUT: Duration = Duration::from_secs(10);
/// One plug-in's describe and describeInContext.
pub const DESCRIBE_TIMEOUT: Duration = Duration::from_secs(2);
/// One frame (a first render may set up a GPU): 30 s, shorter in the hang test.
pub static RENDER_TIMEOUT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(30_000);

pub fn render_timeout() -> Duration {
    Duration::from_millis(RENDER_TIMEOUT_MS.load(std::sync::atomic::Ordering::Relaxed))
}

pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Why a request got no answer.
#[derive(Clone, Debug, PartialEq)]
pub enum Fail {
    /// The host died (the plug-in crashed it).
    Crashed(String),
    /// No answer in time: the watchdog killed the host.
    Hung(Duration),
    /// The host could not start or talk.
    Io(String),
}

impl std::fmt::Display for Fail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fail::Crashed(s) => write!(f, "it crashed the plug-in host ({s})"),
            Fail::Hung(d) => write!(f, "it did not answer within {} s and was stopped", d.as_secs()),
            Fail::Io(s) => write!(f, "{s}"),
        }
    }
}

/// `milan-plugin-host.exe` beside Milan (in a test run: beside the test binary's `deps` folder).
pub fn host_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [dir.to_path_buf(), dir.parent()?.to_path_buf()].into_iter().map(|d| d.join("milan-plugin-host.exe")).find(|p| p.is_file())
}

/// The watchdog's view of one host: a deadline while a request waits, and the process to kill when it passes.
#[derive(Default)]
struct Watch {
    deadline: Option<Instant>,
    /// The process handle while the host lives (0 after drop).
    process: usize,
    fired: bool,
}

/// A running host.
pub struct Conn {
    child: Child,
    pipe: File,
    watch: Arc<Mutex<Watch>>,
    pub shm: Option<Shm>,
}

impl Conn {
    /// Start a host and load `binary` in it.
    pub fn start(binary: &Path) -> Result<(Conn, u32), Fail> {
        let exe = host_exe().ok_or_else(|| Fail::Io("milan-plugin-host.exe is missing next to Milan Pro: reinstall Milan Pro".into()))?;
        let name = format!(r"\\.\pipe\milan-ofx-{}-{}", std::process::id(), uuid::Uuid::new_v4().simple());
        let mut cmd = Command::new(exe);
        cmd.args(["--pipe", &name]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = crate::proc::spawn(&mut cmd).map_err(|e| Fail::Io(format!("the plug-in host failed to start: {e}")))?;
        let until = Instant::now() + Duration::from_secs(5);
        let pipe = loop {
            match std::fs::OpenOptions::new().read(true).write(true).open(&name) {
                Ok(f) => break f,
                Err(_) if Instant::now() < until && child.try_wait().ok().flatten().is_none() => std::thread::sleep(Duration::from_millis(5)),
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(Fail::Io(format!("the plug-in host did not open its pipe: {e}")));
                }
            }
        };
        #[cfg(windows)]
        let process = {
            use std::os::windows::io::AsRawHandle;
            child.as_raw_handle() as usize
        };
        #[cfg(not(windows))]
        let process = 0;
        let watch = Arc::new(Mutex::new(Watch { process, ..Default::default() }));
        let w = Arc::downgrade(&watch);
        // The watchdog lives as long as the connection.
        std::thread::Builder::new()
            .name("milan-ofx-watchdog".into())
            .spawn(move || {
                while let Some(w) = w.upgrade() {
                    {
                        let mut g = lock(&w);
                        if g.process != 0 && g.deadline.is_some_and(|d| Instant::now() >= d) {
                            kill(g.process);
                            (g.fired, g.deadline) = (true, None);
                        }
                    }
                    drop(w);
                    std::thread::sleep(Duration::from_millis(20));
                }
            })
            .map_err(|e| Fail::Io(format!("cannot start the plug-in watchdog: {e}")))?;
        let mut c = Conn { child, pipe, watch, shm: None };
        match c.request(&Req::Load { binary: binary.to_path_buf() }, LOAD_TIMEOUT)? {
            Reply::Loaded(n) => Ok((c, n)),
            Reply::Error(e) => Err(Fail::Io(e)),
            r => Err(Fail::Io(format!("unexpected reply {r:?}"))),
        }
    }

    /// Send `req`; the host is killed if it does not answer within `timeout`.
    pub fn request(&mut self, req: &Req, timeout: Duration) -> Result<Reply, Fail> {
        lock(&self.watch).deadline = Some(Instant::now() + timeout);
        let res = proto::write_msg(&mut self.pipe, req).and_then(|_| proto::read_msg::<Reply>(&mut self.pipe));
        let fired = {
            let mut g = lock(&self.watch);
            g.deadline = None;
            g.fired
        };
        match (res, fired) {
            (_, true) => Err(Fail::Hung(timeout)),
            (Ok(r), false) => Ok(r),
            (Err(e), false) => Err(self.why_gone(e)),
        }
    }

    fn why_gone(&mut self, e: io::Error) -> Fail {
        let until = Instant::now() + Duration::from_secs(2);
        while Instant::now() < until {
            if let Ok(Some(s)) = self.child.try_wait() {
                return Fail::Crashed(match s.code() {
                    Some(c) => format!("exit code 0x{:08X}", c as u32),
                    None => s.to_string(),
                });
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Fail::Io(format!("the plug-in host stopped answering: {e}"))
    }

    /// Ask the host to end (destroying its instances), else kill it.
    pub fn quit(mut self) {
        let _ = self.request(&Req::Quit, Duration::from_secs(2));
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        lock(&self.watch).process = 0;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn kill(process: usize) {
    #[cfg(windows)]
    // SAFETY: the live child's process handle (the watch is cleared under its lock before the child is dropped).
    unsafe {
        windows_sys::Win32::System::Threading::TerminateProcess(process as _, 0xDEAD);
    }
    #[cfg(not(windows))]
    let _ = process;
}

/// Shared memory for frames: Milan writes the source, the host the output.
pub struct Shm {
    pub name: String,
    pub size: u64,
    handle: usize,
    ptr: usize,
}

// SAFETY: an owned mapping; access is serialized by the connection's mutex.
unsafe impl Send for Shm {}

impl Shm {
    #[cfg(windows)]
    pub fn create(size: u64) -> io::Result<Shm> {
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Memory::{CreateFileMappingW, FILE_MAP_ALL_ACCESS, MapViewOfFile, PAGE_READWRITE};
        let name = format!(r"Local\milan-ofx-{}-{}", std::process::id(), uuid::Uuid::new_v4().simple());
        let w: Vec<u16> = name.encode_utf16().chain([0]).collect();
        // SAFETY: a paging-file mapping of `size` bytes; unmapped and closed on drop.
        unsafe {
            let h = CreateFileMappingW(INVALID_HANDLE_VALUE, std::ptr::null(), PAGE_READWRITE, (size >> 32) as u32, size as u32, w.as_ptr());
            if h.is_null() {
                return Err(io::Error::last_os_error());
            }
            let v = MapViewOfFile(h, FILE_MAP_ALL_ACCESS, 0, 0, size as usize);
            if v.Value.is_null() {
                let e = io::Error::last_os_error();
                CloseHandle(h);
                return Err(e);
            }
            Ok(Shm { name, size, handle: h as usize, ptr: v.Value as usize })
        }
    }

    #[cfg(not(windows))]
    pub fn create(_size: u64) -> io::Result<Shm> {
        Err(io::Error::other("OFX plug-ins run on Windows only"))
    }

    pub fn bytes(&mut self) -> &mut [u8] {
        // SAFETY: the whole view, owned by self.
        unsafe { std::slice::from_raw_parts_mut(self.ptr as *mut u8, self.size as usize) }
    }
}

impl Drop for Shm {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::System::Memory::{MEMORY_MAPPED_VIEW_ADDRESS, UnmapViewOfFile};
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: self.ptr as _ });
            windows_sys::Win32::Foundation::CloseHandle(self.handle as _);
        }
    }
}
