//! R34b: a plug-in's own editor in a top-level window the host owns, on a thread of its own that pumps its messages
//! (VST3 views expect every call on the thread that made their window). Milan asks for it to open, close or report;
//! the window closes with the host, and the host with Milan (Job Object). In tests and the screenshot harness the window
//! is made off every monitor, without activation or a taskbar button: nothing shows and no focus moves.
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::COLOR_WINDOW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;
use crate::proto::EditorState;
use crate::suites::lock;

/// What the window shows: a plug-in's view, driven from the window's thread only.
pub trait View: Send {
    /// The window exists: attach to it. The view's size (its own, or a default).
    fn attached(&mut self, hwnd: HWND) -> Result<(u32, u32), String>;
    /// The window's client area changed.
    fn resized(&mut self, w: u32, h: u32);
    /// The window is going away.
    fn removed(&mut self);
    /// A size the plug-in asked for since the last message (through its frame).
    fn wanted(&mut self) -> Option<(u32, u32)>;
}

#[derive(Default)]
struct Shared {
    state: EditorState,
    error: Option<String>,
}

pub struct Editor {
    shared: Arc<Mutex<Shared>>,
    thread_id: Arc<AtomicU32>,
    join: Option<std::thread::JoinHandle<()>>,
}

/// Per window, on its thread (the pointer is in GWLP_USERDATA).
struct Win {
    view: Box<dyn View>,
    shared: Arc<Mutex<Shared>>,
}

const CLASS: &[u16] = &[b'M' as u16, b'i' as u16, b'l' as u16, b'a' as u16, b'n' as u16, b'P' as u16, b'l' as u16, b'u' as u16, b'g' as u16, b'E' as u16, b'd' as u16, b'i' as u16, b't' as u16, b'o' as u16, b'r' as u16, 0];
/// Asked to close from the host's request thread.
const WM_HOST_CLOSE: u32 = WM_APP + 1;

impl Editor {
    /// Open the window (at `x`, `y`, or off-screen) and attach `view`; waits until the window is up (or failed).
    pub fn open(view: Box<dyn View>, title: &str, x: i32, y: i32, offscreen: bool) -> Result<Editor, String> {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let thread_id = Arc::new(AtomicU32::new(0));
        let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
        let (s2, t2, title) = (shared.clone(), thread_id.clone(), title.to_string());
        let join = std::thread::Builder::new()
            .name("milan-plugin-editor".into())
            .spawn(move || run(view, &title, x, y, offscreen, s2, t2, tx))
            .map_err(|e| format!("cannot start the editor thread: {e}"))?;
        match rx.recv_timeout(std::time::Duration::from_secs(10)) {
            Ok(Ok(())) => Ok(Editor { shared, thread_id, join: Some(join) }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("the plug-in's editor did not open within 10 s".into()),
        }
    }

    pub fn state(&self) -> EditorState {
        let mut s = lock(&self.shared).state.clone();
        if self.join.as_ref().is_some_and(|j| j.is_finished()) {
            s.open = false;
        }
        s
    }

    pub fn error(&self) -> Option<String> {
        lock(&self.shared).error.clone()
    }

    /// Close the window and wait (briefly) for its thread.
    pub fn close(mut self) {
        let id = self.thread_id.load(Ordering::Acquire);
        if id != 0 {
            // SAFETY: a thread id this process made; a dead thread makes the call fail harmlessly.
            unsafe { PostThreadMessageW(id, WM_HOST_CLOSE, 0, 0) };
        }
        if let Some(j) = self.join.take() {
            let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while !j.is_finished() && std::time::Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            if j.is_finished() {
                let _ = j.join();
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run(view: Box<dyn View>, title: &str, x: i32, y: i32, offscreen: bool, shared: Arc<Mutex<Shared>>, thread_id: Arc<AtomicU32>, ready: std::sync::mpsc::Sender<Result<(), String>>) {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    // SAFETY: plain Win32 calls on this thread; the window's user data points at a Box this function owns until the
    // window is destroyed.
    unsafe {
        thread_id.store(GetCurrentThreadId(), Ordering::Release);
        let hinst = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW { style: CS_DBLCLKS, lpfnWndProc: Some(wndproc), cbClsExtra: 0, cbWndExtra: 0, hInstance: hinst, hIcon: std::ptr::null_mut(), hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW), hbrBackground: (COLOR_WINDOW + 1) as usize as _, lpszMenuName: std::ptr::null(), lpszClassName: CLASS.as_ptr() };
        RegisterClassW(&wc); // fails harmlessly when registered already
        let mut win = Box::new(Win { view, shared: shared.clone() });
        let wtitle: Vec<u16> = title.encode_utf16().chain([0]).collect();
        let (ex, style) = if offscreen { (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW, WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN) } else { (WS_EX_APPWINDOW, WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN) };
        let (px, py) = if offscreen { (-20000, -20000) } else { (x, y) };
        let hwnd = CreateWindowExW(ex, CLASS.as_ptr(), wtitle.as_ptr(), style, px, py, 400, 300, std::ptr::null_mut(), std::ptr::null_mut(), hinst, std::ptr::null());
        if hwnd.is_null() {
            let _ = ready.send(Err(format!("cannot make the editor window: {}", std::io::Error::last_os_error())));
            return;
        }
        let (w, h) = match win.view.attached(hwnd) {
            Ok(s) => s,
            Err(e) => {
                let _ = ready.send(Err(e));
                DestroyWindow(hwnd);
                return;
            }
        };
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, &mut *win as *mut Win as isize);
        fit(hwnd, w, h);
        {
            let mut s = lock(&shared);
            s.state = EditorState { open: true, w, h, hwnd: hwnd as usize as u64 };
        }
        ShowWindow(hwnd, if offscreen { SW_SHOWNOACTIVATE } else { SW_SHOW });
        let _ = ready.send(Ok(()));
        let mut msg: MSG = std::mem::zeroed();
        loop {
            let r = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
            if r <= 0 {
                break;
            }
            if msg.message == WM_HOST_CLOSE && msg.hwnd.is_null() {
                DestroyWindow(hwnd);
                continue;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            // A size the plug-in asked for (IPlugFrame::resizeView) while that message ran.
            if let Some((w, h)) = win.view.wanted() {
                fit(hwnd, w, h);
            }
        }
        lock(&shared).state.open = false;
        // The Box outlives the window (the proc read it until WM_NCDESTROY).
        drop(win);
    }
}

/// Size the window so its client area is `w` x `h`.
unsafe fn fit(hwnd: HWND, w: u32, h: u32) {
    use windows_sys::Win32::Foundation::RECT;
    // SAFETY: a live window handle.
    unsafe {
        let mut r = RECT { left: 0, top: 0, right: w as i32, bottom: h as i32 };
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        AdjustWindowRectEx(&mut r, style, 0, ex);
        SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, r.right - r.left, r.bottom - r.top, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: the user data is the `Win` the window's thread owns while the window lives (null before it is set).
    unsafe {
        let win = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Win;
        match msg {
            WM_SIZE if !win.is_null() => {
                let (w, h) = ((lp & 0xffff) as u32, ((lp >> 16) & 0xffff) as u32);
                (*win).view.resized(w, h);
                let mut s = lock(&(*win).shared);
                (s.state.w, s.state.h) = (w, h);
                0
            }
            WM_CLOSE => {
                DestroyWindow(hwnd);
                0
            }
            WM_DESTROY => {
                if !win.is_null() {
                    (*win).view.removed();
                    lock(&(*win).shared).state.open = false;
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                }
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
