//! A2-A5: what an AE-format effect can call back into Milan's host: the suite broker (`pica_basic`), the interaction
//! callbacks (`in_data.inter`), the utility callbacks (`in_data.utils`) and the suites whose tables a public page gives
//! (`abi.rs` says which). Every call runs on the host's one thread (calls to a plug-in are serialised, as Premiere does
//! them); the state of the command in progress is `CTX`. A suite or callback Milan does not serve is noted
//! (`Ctx::refused`), so the Plug-In Manager can say what a plug-in wanted, and returns an error: never a null slot.
use super::abi::*;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::ffi::{CStr, c_char, c_void};

/// A parameter added during PARAMS_SETUP, with copies of the strings its definition points to.
#[derive(Clone)]
pub struct Added {
    pub def: ParamDef,
    /// A popup's item names, or a checkbox's label.
    pub names: String,
}

/// A frame the plug-in checked out (`checkout_param` on a layer): kept until checked in or the command ends.
pub struct Checked {
    pub pixels: Vec<u8>,
}

/// Answers a layer checkout during a render: (layer parameter index, time offset in frames from the frame being
/// rendered) → an ARGB world's rows (top first, `rowbytes` = width * 4), or None for a transparent one.
pub type FrameSource<'a> = dyn FnMut(i32, f64) -> Option<Vec<u8>> + 'a;

/// The state of the command in progress.
#[derive(Default)]
pub struct Ctx {
    pub adding: Option<Vec<Added>>,
    /// Suites and callbacks the plug-in asked for that Milan does not serve.
    pub refused: BTreeSet<String>,
    /// `update_param_ui` calls: (parameter index, the definition as the plug-in wants it shown).
    pub ui_updates: Vec<(i32, ParamDef)>,
    /// During a render: the current parameters (index 0 the input), for checkouts of values at other times.
    pub params: Vec<ParamDef>,
    /// During a render: the frame size and the current time (plug-in units), for layer checkouts.
    pub frame: Option<(i32, i32, i32, i32)>,
    pub checked: Vec<Checked>,
    /// Layer checkouts during this command (the A5 echo test counts them).
    pub checkouts: u32,
    /// Layer parameters set to None (they check out transparent).
    pub none_layers: Vec<i32>,
    /// The input frame's rows (ARGB, top first, `w * 4` bytes each): a checkout of it at the current time.
    pub input: Vec<u8>,
}

thread_local! {
    pub static CTX: RefCell<Ctx> = RefCell::new(Ctx::default());
    /// The checkout answerer of the render in progress (set by the host around RENDER).
    pub static SOURCE: RefCell<Option<Box<FrameSource<'static>>>> = RefCell::new(None);
}

pub fn with<R>(f: impl FnOnce(&mut Ctx) -> R) -> R {
    CTX.with(|c| f(&mut c.borrow_mut()))
}

fn refuse(what: String) {
    log::debug!("aex: refused {what}");
    with(|c| c.refused.insert(what));
}

// ---------------------------------------------------------------- handles and memory

struct Block {
    /// The master pointer the handle points to.
    master: Box<*mut c_void>,
    size: usize,
}

thread_local! {
    static HANDLES: RefCell<HashMap<usize, Block>> = RefCell::new(HashMap::new());
}

fn layout(size: usize) -> std::alloc::Layout {
    std::alloc::Layout::from_size_align(size.max(1), 16).expect("a sane size")
}

/// A new zeroed handle of `size` bytes (null when the size is absurd).
pub fn new_handle(size: usize) -> Handle {
    if size > 1 << 31 {
        return std::ptr::null_mut();
    }
    // SAFETY: a non-zero layout.
    let data = unsafe { std::alloc::alloc_zeroed(layout(size)) } as *mut c_void;
    if data.is_null() {
        return std::ptr::null_mut();
    }
    let mut master = Box::new(data);
    let h = &mut *master as *mut *mut c_void;
    HANDLES.with(|m| m.borrow_mut().insert(h as usize, Block { master, size }));
    h
}

pub fn handle_size(h: Handle) -> Option<usize> {
    HANDLES.with(|m| m.borrow().get(&(h as usize)).map(|b| b.size))
}

/// The handle's bytes (None for a handle Milan did not make).
pub fn handle_bytes(h: Handle) -> Option<Vec<u8>> {
    HANDLES.with(|m| m.borrow().get(&(h as usize)).map(|b| unsafe { std::slice::from_raw_parts(*b.master as *const u8, b.size) }.to_vec()))
}

pub fn handle_from(bytes: &[u8]) -> Handle {
    let h = new_handle(bytes.len());
    if !h.is_null() {
        // SAFETY: a fresh block of `bytes.len()` bytes.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), *h as *mut u8, bytes.len()) };
    }
    h
}

pub fn dispose_handle(h: Handle) {
    if let Some(b) = HANDLES.with(|m| m.borrow_mut().remove(&(h as usize))) {
        // SAFETY: allocated by `new_handle` or `resize` with this size.
        unsafe { std::alloc::dealloc(*b.master as *mut u8, layout(b.size)) };
    }
}

pub fn is_handle(h: Handle) -> bool {
    HANDLES.with(|m| m.borrow().contains_key(&(h as usize)))
}

unsafe extern "C" fn host_new_handle(size: u64) -> Handle {
    new_handle(size as usize)
}

unsafe extern "C" fn host_lock_handle(h: Handle) -> *mut c_void {
    if is_handle(h) { unsafe { *h } } else { std::ptr::null_mut() }
}

unsafe extern "C" fn host_unlock_handle(_h: Handle) {}

unsafe extern "C" fn host_dispose_handle(h: Handle) {
    dispose_handle(h)
}

unsafe extern "C" fn host_get_handle_size(h: Handle) -> u64 {
    handle_size(h).unwrap_or(0) as u64
}

unsafe extern "C" fn host_resize_handle(size: u64, hp: *mut Handle) -> Err {
    if hp.is_null() || size > 1 << 31 {
        return err::BAD_CALLBACK_PARAM;
    }
    let h = unsafe { *hp };
    HANDLES.with(|m| {
        let mut m = m.borrow_mut();
        let Some(b) = m.get_mut(&(h as usize)) else { return err::BAD_CALLBACK_PARAM };
        // SAFETY: the block was allocated with `layout(b.size)`.
        let p = unsafe { std::alloc::realloc(*b.master as *mut u8, layout(b.size), size.max(1) as usize) };
        if p.is_null() {
            return err::OUT_OF_MEMORY;
        }
        if size as usize > b.size {
            // SAFETY: the grown tail of the new block.
            unsafe { std::ptr::write_bytes(p.add(b.size), 0, size as usize - b.size) };
        }
        *b.master = p as *mut c_void;
        b.size = size as usize;
        err::NONE
    })
}

// ---------------------------------------------------------------- worlds

thread_local! {
    /// Worlds the plug-in made (`new_world`): their pixels, by data pointer.
    static WORLDS: RefCell<HashMap<usize, Vec<u8>>> = RefCell::new(HashMap::new());
}

/// Rows padded to 16 bytes.
pub fn rowbytes(w: i32) -> i32 {
    (w * 4 + 15) / 16 * 16
}

/// A world over `data` (8-bit ARGB, top row first).
pub fn world(data: *mut u8, w: i32, h: i32, rb: i32, writeable: bool) -> World {
    World {
        world_flags: if writeable { WORLD_WRITEABLE } else { 0 },
        data,
        rowbytes: rb,
        width: w,
        height: h,
        extent_hint: Rect { left: 0, top: 0, right: w, bottom: h },
        pix_aspect_ratio: RationalScale { num: 1, den: 1 },
        ..Default::default()
    }
}

fn make_world(w: i32, h: i32, out: *mut World) -> Err {
    if out.is_null() || w < 0 || h < 0 || (w as i64) * (h as i64) > 1 << 28 {
        return err::BAD_CALLBACK_PARAM;
    }
    let rb = rowbytes(w);
    let mut buf = vec![0u8; (rb * h).max(16) as usize];
    let p = buf.as_mut_ptr();
    WORLDS.with(|m| m.borrow_mut().insert(p as usize, buf));
    // SAFETY: checked non-null; the plug-in's world struct.
    unsafe { *out = world(p, w, h, rb, true) };
    err::NONE
}

fn drop_world(wp: *mut World) -> Err {
    if wp.is_null() {
        return err::BAD_CALLBACK_PARAM;
    }
    let p = unsafe { (*wp).data } as usize;
    WORLDS.with(|m| m.borrow_mut().remove(&p));
    unsafe { (*wp).data = std::ptr::null_mut() };
    err::NONE
}

/// Pixel (x, y) of `w`, if inside it.
unsafe fn px(w: &World, x: i32, y: i32) -> Option<*mut Pixel> {
    (x >= 0 && y >= 0 && x < w.width && y < w.height && !w.data.is_null()).then(|| unsafe { w.data.add(y as usize * w.rowbytes as usize + x as usize * 4) as *mut Pixel })
}

fn whole(w: &World) -> Rect {
    Rect { left: 0, top: 0, right: w.width, bottom: w.height }
}

fn clip(r: Rect, w: &World) -> Rect {
    Rect { left: r.left.max(0), top: r.top.max(0), right: r.right.min(w.width), bottom: r.bottom.min(w.height) }
}

unsafe extern "C" fn new_world(_e: ProgPtr, w: i32, h: i32, _flags: i32, out: *mut World) -> Err {
    make_world(w, h, out)
}

unsafe extern "C" fn new_world2(_e: ProgPtr, w: i32, h: i32, _clear: u8, format: i32, out: *mut World) -> Err {
    if format != PIXEL_FORMAT_ARGB32 {
        refuse(format!("a world of pixel format 0x{format:08X}"));
        return err::BAD_CALLBACK_PARAM;
    }
    make_world(w, h, out)
}

unsafe extern "C" fn dispose_world(_e: ProgPtr, w: *mut World) -> Err {
    drop_world(w)
}

unsafe extern "C" fn get_pixel_format(w: *const World, f: *mut i32) -> Err {
    if w.is_null() || f.is_null() {
        return err::BAD_CALLBACK_PARAM;
    }
    unsafe { *f = PIXEL_FORMAT_ARGB32 };
    err::NONE
}

/// Copy `src_r` of `src` to `dst_r` of `dst` (whole worlds when null), scaled by nearest pixel when the sizes differ.
pub unsafe extern "C" fn copy(_e: ProgPtr, src: *mut World, dst: *mut World, src_r: *const Rect, dst_r: *const Rect) -> Err {
    let (Some(s), Some(d)) = (unsafe { src.as_ref() }, unsafe { dst.as_ref() }) else { return err::BAD_CALLBACK_PARAM };
    let sr = unsafe { src_r.as_ref() }.copied().unwrap_or_else(|| whole(s));
    let dr = unsafe { dst_r.as_ref() }.copied().unwrap_or_else(|| whole(d));
    let (sw, sh, dw, dh) = (sr.right - sr.left, sr.bottom - sr.top, dr.right - dr.left, dr.bottom - dr.top);
    if sw <= 0 || sh <= 0 || dw <= 0 || dh <= 0 {
        return err::NONE;
    }
    let c = clip(dr, d);
    for y in c.top..c.bottom {
        let sy = sr.top + ((y - dr.top) as i64 * sh as i64 / dh as i64) as i32;
        for x in c.left..c.right {
            let sx = sr.left + ((x - dr.left) as i64 * sw as i64 / dw as i64) as i32;
            unsafe {
                if let (Some(sp), Some(dp)) = (px(s, sx, sy), px(d, x, y)) {
                    *dp = *sp;
                }
            }
        }
    }
    err::NONE
}

pub unsafe extern "C" fn fill(_e: ProgPtr, color: *const Pixel, r: *const Rect, w: *mut World) -> Err {
    let Some(w) = (unsafe { w.as_ref() }) else { return err::BAD_CALLBACK_PARAM };
    let c = unsafe { color.as_ref() }.copied().unwrap_or_default();
    let r = clip(unsafe { r.as_ref() }.copied().unwrap_or_else(|| whole(w)), w);
    for y in r.top..r.bottom {
        for x in r.left..r.right {
            unsafe { *px(w, x, y).expect("clipped") = c };
        }
    }
    err::NONE
}

fn mul(c: u8, a: u8) -> u8 {
    ((c as u32 * a as u32 + 127) / 255) as u8
}

fn div(c: u8, a: u8) -> u8 {
    if a == 0 { 0 } else { ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 }
}

/// `forward`: premultiply; else divide alpha out. In place over the whole world.
pub unsafe extern "C" fn premultiply(_e: ProgPtr, forward: i32, w: *mut World) -> Err {
    unsafe { premultiply_color(_e, w, std::ptr::null(), forward, w) }
}

/// Premultiply with (`forward`) or out of a background colour (null: black), `src` into `dst`.
pub unsafe extern "C" fn premultiply_color(_e: ProgPtr, src: *mut World, color: *const Pixel, forward: i32, dst: *mut World) -> Err {
    let (Some(s), Some(d)) = (unsafe { src.as_ref() }, unsafe { dst.as_ref() }) else { return err::BAD_CALLBACK_PARAM };
    let bg = unsafe { color.as_ref() }.copied().unwrap_or_default();
    for y in 0..d.height.min(s.height) {
        for x in 0..d.width.min(s.width) {
            unsafe {
                let p = *px(s, x, y).expect("inside");
                let a = p.alpha;
                let f = |c: u8, b: u8| if forward != 0 { mul(c, a).saturating_add(mul(b, 255 - a)) } else { div(c.saturating_sub(mul(b, 255 - a)), a) };
                *px(d, x, y).expect("inside") = Pixel { alpha: a, red: f(p.red, bg.red), green: f(p.green, bg.green), blue: f(p.blue, bg.blue) };
            }
        }
    }
    err::NONE
}

/// `dst` = `src1` mixed with `src2` by `ratio` (16.16, 0 = all `src1`).
pub unsafe extern "C" fn blend(_e: ProgPtr, a: *const World, b: *const World, ratio: Fixed, dst: *mut World) -> Err {
    let (Some(a), Some(b), Some(d)) = (unsafe { a.as_ref() }, unsafe { b.as_ref() }, unsafe { dst.as_ref() }) else { return err::BAD_CALLBACK_PARAM };
    let r = ratio.clamp(0, 65536) as u32;
    let m = |x: u8, y: u8| ((x as u32 * (65536 - r) + y as u32 * r + 32768) >> 16) as u8;
    for y in 0..d.height.min(a.height).min(b.height) {
        for x in 0..d.width.min(a.width).min(b.width) {
            unsafe {
                let (p, q) = (*px(a, x, y).expect("inside"), *px(b, x, y).expect("inside"));
                *px(d, x, y).expect("inside") = Pixel { alpha: m(p.alpha, q.alpha), red: m(p.red, q.red), green: m(p.green, q.green), blue: m(p.blue, q.blue) };
            }
        }
    }
    err::NONE
}

/// The iterate family: `f` per pixel of `area` (of the output; whole when null), the input pixel at the same place
/// moved by `origin` (transparent outside the input when `clip_src` is false, else clamped... both read transparent:
/// Milan's input worlds are the frame). Single-threaded (shortcut: AE may split rows over threads).
unsafe fn iterate_any(src: *mut World, area: *const Rect, origin: Point, refcon: *mut c_void, f: Option<PixelFn8>, dst: *mut World) -> Err {
    let (Some(f), Some(d)) = (f, unsafe { dst.as_ref() }) else { return err::BAD_CALLBACK_PARAM };
    let s = unsafe { src.as_ref() };
    let r = clip(unsafe { area.as_ref() }.copied().unwrap_or_else(|| whole(d)), d);
    let mut none = Pixel::default();
    for y in r.top..r.bottom {
        for x in r.left..r.right {
            let ip = s.and_then(|s| unsafe { px(s, x + origin.h, y + origin.v) }).unwrap_or(&mut none as *mut Pixel);
            let e = unsafe { f(refcon, x, y, ip, px(d, x, y).expect("clipped")) };
            if e != err::NONE {
                return e;
            }
        }
    }
    err::NONE
}

pub unsafe extern "C" fn iterate(_i: *mut InData, _b: i32, _f: i32, src: *mut World, area: *const Rect, refcon: *mut c_void, f: Option<PixelFn8>, dst: *mut World) -> Err {
    unsafe { iterate_any(src, area, Point::default(), refcon, f, dst) }
}

pub unsafe extern "C" fn iterate_origin(_i: *mut InData, _b: i32, _f: i32, src: *mut World, area: *const Rect, origin: *const Point, refcon: *mut c_void, f: Option<PixelFn8>, dst: *mut World) -> Err {
    let o = unsafe { origin.as_ref() }.copied().unwrap_or_default();
    unsafe { iterate_any(src, area, o, refcon, f, dst) }
}

pub unsafe extern "C" fn iterate_lut(_i: *mut InData, _b: i32, _f: i32, src: *mut World, area: *const Rect, a: *mut u8, r: *mut u8, g: *mut u8, b: *mut u8, dst: *mut World) -> Err {
    let (Some(s), Some(d)) = (unsafe { src.as_ref() }, unsafe { dst.as_ref() }) else { return err::BAD_CALLBACK_PARAM };
    let rect = clip(unsafe { area.as_ref() }.copied().unwrap_or_else(|| whole(d)), d);
    let look = |lut: *mut u8, v: u8| if lut.is_null() { v } else { unsafe { *lut.add(v as usize) } };
    for y in rect.top..rect.bottom {
        for x in rect.left..rect.right {
            unsafe {
                let Some(sp) = px(s, x, y) else { continue };
                let p = *sp;
                *px(d, x, y).expect("clipped") = Pixel { alpha: look(a, p.alpha), red: look(r, p.red), green: look(g, p.green), blue: look(b, p.blue) };
            }
        }
    }
    err::NONE
}

pub unsafe extern "C" fn iterate_generic(n: i32, refcon: *mut c_void, f: Option<GenericFn>) -> Err {
    let Some(f) = f else { return err::BAD_CALLBACK_PARAM };
    for i in 0..n.max(0) {
        let e = unsafe { f(refcon, 0, i, n) };
        if e != err::NONE {
            return e;
        }
    }
    err::NONE
}

// ---------------------------------------------------------------- the C library for plug-ins

macro_rules! maths {
    ($($name:ident: $f:expr),*) => {$(
        unsafe extern "C" fn $name(x: f64) -> f64 {
            $f(x)
        }
    )*};
}
maths!(m_acos: f64::acos, m_asin: f64::asin, m_atan: f64::atan, m_ceil: f64::ceil, m_cos: f64::cos, m_exp: f64::exp, m_fabs: f64::abs, m_floor: f64::floor, m_log: f64::ln, m_log10: f64::log10, m_sin: f64::sin, m_sqrt: f64::sqrt, m_tan: f64::tan);
unsafe extern "C" fn m_atan2(y: f64, x: f64) -> f64 {
    y.atan2(x)
}
unsafe extern "C" fn m_fmod(x: f64, y: f64) -> f64 {
    x % y
}
unsafe extern "C" fn m_hypot(x: f64, y: f64) -> f64 {
    x.hypot(y)
}
unsafe extern "C" fn m_pow(x: f64, y: f64) -> f64 {
    x.powf(y)
}

/// sprintf: the Win64 variadic slots read as in the OFX message suite (`crate::suites::format_c`).
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn c_sprintf(buf: *mut c_char, fmt: *const c_char, a0: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64, a7: u64) -> i32 {
    if buf.is_null() || fmt.is_null() {
        return -1;
    }
    let s = unsafe { crate::suites::format_c(CStr::from_ptr(fmt).to_bytes(), &[a0, a1, a2, a3, a4, a5, a6, a7]) };
    unsafe {
        std::ptr::copy_nonoverlapping(s.as_ptr(), buf as *mut u8, s.len());
        *buf.add(s.len()) = 0;
    }
    s.len() as i32
}

unsafe extern "C" fn c_strcpy(d: *mut c_char, s: *const c_char) -> *mut c_char {
    if d.is_null() || s.is_null() {
        return d;
    }
    unsafe {
        let n = CStr::from_ptr(s).to_bytes_with_nul().len();
        std::ptr::copy(s, d, n);
    }
    d
}

type Sprintf = unsafe extern "C" fn(*mut c_char, *const c_char, ...) -> i32;

fn sprintf_ptr() -> Sprintf {
    // SAFETY: on Win64 a variadic call passes the same 8-byte slots a fixed-argument callee reads (the OFX host relies
    // on the same, `main.rs`).
    unsafe { std::mem::transmute::<unsafe extern "C" fn(*mut c_char, *const c_char, u64, u64, u64, u64, u64, u64, u64, u64) -> i32, Sprintf>(c_sprintf) }
}

// ---------------------------------------------------------------- stubs

/// A slot Milan does not serve: notes it and returns an error. One function per slot so the note names it.
macro_rules! stubs {
    ($($name:ident: $what:expr),*) => {$(
        unsafe extern "C" fn $name() -> Err {
            refuse(format!("utility callback {}", $what));
            err::INVALID_CALLBACK
        }
    )*};
}
stubs!(
    s_begin_sampling: "begin_sampling", s_subpixel: "subpixel_sample", s_area: "area_sample", s_batch: "get_batch_func", s_end_sampling: "end_sampling",
    s_composite: "composite_rect", s_convolve: "convolve", s_gaussian: "gaussian_kernel", s_transfer: "transfer_rect", s_transform: "transform_world",
    s_callback_addr: "get_callback_addr", s_app: "app", s_color: "color callbacks", s_platform: "get_platform_data", s_16: "16-bit callbacks",
    s_pixel_data: "get_pixel_data", s_ansi_procs: "ansi_procs", s_fill16: "fill16 / fill_float", s_premul16: "premultiply_color16 / float",
    s_wt_composite: "PF World Transform Suite composite_rect", s_wt_convolve: "PF World Transform Suite convolve", s_wt_transfer: "PF World Transform Suite transfer_rect",
    s_wt_transform: "PF World Transform Suite transform_world", s_pu: "PF Param Utils Suite function other than update_param_ui and get_keyframe_count"
);

fn v(f: unsafe extern "C" fn() -> Err) -> *const c_void {
    f as *const c_void
}

pub fn ansi_callbacks() -> AnsiCallbacks {
    AnsiCallbacks {
        atan: m_atan,
        atan2: m_atan2,
        ceil: m_ceil,
        cos: m_cos,
        exp: m_exp,
        fabs: m_fabs,
        floor: m_floor,
        fmod: m_fmod,
        hypot: m_hypot,
        log: m_log,
        log10: m_log10,
        pow: m_pow,
        sin: m_sin,
        sqrt: m_sqrt,
        tan: m_tan,
        sprintf: sprintf_ptr(),
        strcpy: c_strcpy,
        asin: m_asin,
        acos: m_acos,
        ansi_procs: [v(s_ansi_procs)],
    }
}

/// The utility callbacks every command gets (made once).
pub fn utils() -> *mut UtilCallbacks {
    static U: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *U.get_or_init(|| {
        Box::into_raw(Box::new(UtilCallbacks {
            begin_sampling: v(s_begin_sampling),
            subpixel_sample: v(s_subpixel),
            area_sample: v(s_area),
            get_batch_func: v(s_batch),
            end_sampling: v(s_end_sampling),
            composite_rect: v(s_composite),
            blend,
            convolve: v(s_convolve),
            copy,
            fill,
            gaussian_kernel: v(s_gaussian),
            iterate,
            premultiply,
            premultiply_color,
            new_world,
            dispose_world,
            iterate_origin,
            iterate_lut,
            transfer_rect: v(s_transfer),
            transform_world: v(s_transform),
            host_new_handle,
            host_lock_handle,
            host_unlock_handle,
            host_dispose_handle,
            get_callback_addr: v(s_callback_addr),
            app: v(s_app),
            ansi: ansi_callbacks(),
            color_cb: [v(s_color); 8],
            get_platform_data: v(s_platform),
            host_get_handle_size,
            iterate_origin_non_clip_src: iterate_origin,
            iterate_generic,
            host_resize_handle,
            subpixel_sample16: v(s_16),
            area_sample16: v(s_16),
            fill16: v(s_16),
            premultiply_color16: v(s_16),
            iterate16: v(s_16),
            iterate_origin16: v(s_16),
            iterate_origin_non_clip_src16: v(s_16),
            get_pixel_data8: v(s_pixel_data),
            get_pixel_data16: v(s_pixel_data),
            reserved: [v(s_16)],
        })) as usize
    }) as *mut UtilCallbacks
}

// ---------------------------------------------------------------- interaction callbacks

/// Copy the C string at `p` (bounded).
unsafe fn cstr(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    let s = unsafe { CStr::from_ptr(p) }.to_bytes();
    String::from_utf8_lossy(&s[..s.len().min(4096)]).into_owned()
}

unsafe extern "C" fn add_param(_e: ProgPtr, _index: i32, def: *mut ParamDef) -> Err {
    let Some(d) = (unsafe { def.as_ref() }) else { return err::BAD_CALLBACK_PARAM };
    let names = unsafe {
        match d.param_type {
            param::POPUP => cstr(d.u.pd.names),
            param::CHECKBOX => cstr(d.u.bd.name),
            param::BUTTON => cstr(d.u.button_d.names),
            _ => String::new(),
        }
    };
    with(|c| match &mut c.adding {
        Some(v) if v.len() < 1000 => {
            v.push(Added { def: *d, names });
            err::NONE
        }
        Some(_) => err::OUT_OF_MEMORY,
        None => err::BAD_CALLBACK_PARAM,
    })
}

unsafe extern "C" fn abort(_e: ProgPtr) -> Err {
    err::NONE
}

unsafe extern "C" fn progress(_e: ProgPtr, _cur: i32, _total: i32) -> Err {
    err::NONE
}

unsafe extern "C" fn register_ui(_e: ProgPtr, _info: *mut c_void) -> Err {
    // Recorded as accepted: no event is ever sent (generic controls only, plan §4.6).
    err::NONE
}

unsafe extern "C" fn no_audio() -> Err {
    refuse("layer audio".into());
    err::INVALID_CALLBACK
}

/// A value at another time is the value now (shortcut: Milan does not send parameter curves; a plug-in that reads a
/// parameter at another time gets the current one). A layer: the frame at that time from `SOURCE`, transparent for
/// "None" or when there is no source.
unsafe extern "C" fn checkout_param(_e: ProgPtr, index: i32, what_time: i32, _step: i32, _scale: u32, out: *mut ParamDef) -> Err {
    if out.is_null() {
        return err::BAD_CALLBACK_PARAM;
    }
    let (def, frame) = with(|c| (c.params.get(index as usize).copied(), c.frame));
    let Some(mut def) = def else { return err::INVALID_INDEX };
    if def.param_type == param::LAYER {
        let Some((w, h, now, step)) = frame else { return err::BAD_CALLBACK_PARAM };
        // Layer parameters other than the input: None (transparent) or this event.
        let none = with(|c| c.none_layers.contains(&index));
        let offset = if step != 0 { (what_time - now) as f64 / step as f64 } else { 0.0 };
        let mut pixels = if none {
            None
        } else if offset == 0.0 {
            with(|c| Some(c.input.clone()))
        } else {
            SOURCE.with(|s| s.borrow_mut().as_mut().and_then(|f| f(index, offset)))
        };
        let rb = w * 4;
        let mut buf = pixels.take().filter(|p| p.len() == (rb * h) as usize).unwrap_or_else(|| vec![0; (rb * h).max(4) as usize]);
        let data = buf.as_mut_ptr();
        def.u.ld = world(data, w, h, rb, false);
        unsafe { *out = def };
        with(|c| {
            c.checkouts += 1;
            c.checked.push(Checked { pixels: buf });
        });
        return err::NONE;
    }
    unsafe { *out = def };
    err::NONE
}

unsafe extern "C" fn checkin_param(_e: ProgPtr, def: *mut ParamDef) -> Err {
    if def.is_null() {
        return err::BAD_CALLBACK_PARAM;
    }
    let data = unsafe { (*def).u.ld.data } as usize;
    with(|c| c.checked.retain(|k| k.pixels.as_ptr() as usize != data));
    err::NONE
}

pub fn inter() -> InteractCallbacks {
    InteractCallbacks {
        checkout_param: Some(checkout_param),
        checkin_param: Some(checkin_param),
        add_param: Some(add_param),
        abort: Some(abort),
        progress: Some(progress),
        register_ui: Some(register_ui),
        checkout_layer_audio: Some(no_audio),
        checkin_layer_audio: Some(no_audio),
        get_audio_data: Some(no_audio),
        reserved_str: [std::ptr::null_mut(); 3],
        reserved: [std::ptr::null_mut(); 10],
    }
}

// ---------------------------------------------------------------- suites

unsafe extern "C" fn fm_fill(e: ProgPtr, c: *const Pixel, r: *const Rect, w: *mut World) -> Err {
    unsafe { fill(e, c, r, w) }
}

unsafe extern "C" fn wt_copy(e: ProgPtr, s: *mut World, d: *mut World, sr: *const Rect, dr: *const Rect) -> Err {
    unsafe { copy(e, s, d, sr, dr) }
}

unsafe extern "C" fn color_param(_e: ProgPtr, def: *const ParamDef, out: *mut PixelFloat) -> Err {
    let (Some(d), false) = (unsafe { def.as_ref() }, out.is_null()) else { return err::BAD_CALLBACK_PARAM };
    let c = unsafe { d.u.cd.value };
    unsafe { *out = PixelFloat { alpha: c.alpha as f32 / 255.0, red: c.red as f32 / 255.0, green: c.green as f32 / 255.0, blue: c.blue as f32 / 255.0 } };
    err::NONE
}

unsafe extern "C" fn point_param(_e: ProgPtr, def: *const ParamDef, out: *mut [f64; 2]) -> Err {
    let (Some(d), false) = (unsafe { def.as_ref() }, out.is_null()) else { return err::BAD_CALLBACK_PARAM };
    let p = unsafe { d.u.td };
    unsafe { *out = [unfixed(p.x_value), unfixed(p.y_value)] };
    err::NONE
}

unsafe extern "C" fn angle_param(_e: ProgPtr, def: *const ParamDef, out: *mut f64) -> Err {
    let (Some(d), false) = (unsafe { def.as_ref() }, out.is_null()) else { return err::BAD_CALLBACK_PARAM };
    unsafe { *out = unfixed(d.u.ad.value) };
    err::NONE
}

unsafe extern "C" fn update_param_ui(_e: ProgPtr, index: i32, def: *const ParamDef) -> Err {
    let Some(d) = (unsafe { def.as_ref() }) else { return err::BAD_CALLBACK_PARAM };
    if index <= 0 {
        return err::INVALID_INDEX;
    }
    with(|c| c.ui_updates.push((index, *d)));
    err::NONE
}

unsafe extern "C" fn keyframe_count(_e: ProgPtr, _index: i32, n: *mut i32) -> Err {
    if n.is_null() {
        return err::BAD_CALLBACK_PARAM;
    }
    // Milan evaluates keyframes itself; the plug-in sees one value per frame.
    unsafe { *n = 0 };
    err::NONE
}

fn leak<T>(t: T) -> usize {
    Box::into_raw(Box::new(t)) as usize
}

/// The suites served, by (name, version).
fn table(name: &CStr, version: i32) -> Option<*const c_void> {
    static T: std::sync::OnceLock<Vec<(&'static CStr, i32, usize)>> = std::sync::OnceLock::new();
    let t = T.get_or_init(|| {
        vec![
            (suite::HANDLE.0, suite::HANDLE.1, leak(HandleSuite1 { new_handle: host_new_handle, lock_handle: host_lock_handle, unlock_handle: host_unlock_handle, dispose_handle: host_dispose_handle, get_handle_size: host_get_handle_size, resize_handle: host_resize_handle })),
            (suite::ITERATE8.0, suite::ITERATE8.1, leak(Iterate8Suite1 { iterate, iterate_origin, iterate_lut, iterate_origin_non_clip_src: iterate_origin, iterate_generic })),
            (suite::WORLD.0, suite::WORLD.1, leak(WorldSuite2 { new_world: new_world2, dispose_world, get_pixel_format })),
            (suite::FILL_MATTE.0, suite::FILL_MATTE.1, leak(FillMatteSuite2 { fill: fm_fill, fill16: s_fill16, fill_float: s_fill16, premultiply, premultiply_color, premultiply_color16: s_premul16, premultiply_color_float: s_premul16 })),
            (suite::WORLD_TRANSFORM.0, suite::WORLD_TRANSFORM.1, leak(WorldTransformSuite1 { composite_rect: s_wt_composite, blend, convolve: s_wt_convolve, copy: wt_copy, copy_hq: wt_copy, transfer_rect: s_wt_transfer, transform_world: s_wt_transform })),
            (suite::ANSI.0, suite::ANSI.1, {
                let a = ansi_callbacks();
                leak(AnsiSuite1 { acos: a.acos, asin: a.asin, atan: a.atan, atan2: a.atan2, ceil: a.ceil, cos: a.cos, exp: a.exp, fabs: a.fabs, floor: a.floor, fmod: a.fmod, hypot: a.hypot, log: a.log, log10: a.log10, pow: a.pow, sin: a.sin, sqrt: a.sqrt, tan: a.tan, sprintf: a.sprintf, strcpy: a.strcpy })
            }),
            (suite::COLOR_PARAM.0, suite::COLOR_PARAM.1, leak(ColorParamSuite1 { get_float_color: color_param })),
            (suite::POINT_PARAM.0, suite::POINT_PARAM.1, leak(PointParamSuite1 { get_float_point: point_param })),
            (suite::ANGLE_PARAM.0, suite::ANGLE_PARAM.1, leak(AngleParamSuite1 { get_float_angle: angle_param })),
            (
                suite::PARAM_UTILS.0,
                suite::PARAM_UTILS.1,
                leak(ParamUtilsSuite3 { update_param_ui, get_current_state: s_pu, are_states_identical: s_pu, is_identical_checkout: s_pu, find_keyframe_time: s_pu, get_keyframe_count: keyframe_count, checkout_keyframe: s_pu, checkin_keyframe: s_pu, key_index_to_time: s_pu }),
            ),
        ]
    });
    t.iter().find(|(n, v, _)| *n == name && *v == version).map(|e| e.2 as *const c_void)
}

unsafe extern "C" fn acquire_suite(name: *const c_char, version: i32, out: *mut *const c_void) -> i32 {
    if name.is_null() || out.is_null() {
        return SUITE_NOT_FOUND;
    }
    let n = unsafe { CStr::from_ptr(name) };
    match table(n, version) {
        Some(p) => {
            unsafe { *out = p };
            0
        }
        None => {
            refuse(format!("{} v{version}", n.to_string_lossy()));
            unsafe { *out = std::ptr::null() };
            SUITE_NOT_FOUND
        }
    }
}

unsafe extern "C" fn release_suite(_name: *const c_char, _version: i32) -> i32 {
    0
}

unsafe extern "C" fn is_equal(a: *const c_char, b: *const c_char) -> i32 {
    if a.is_null() || b.is_null() {
        return 0;
    }
    (unsafe { CStr::from_ptr(a) } == unsafe { CStr::from_ptr(b) }) as i32
}

const BLOCK_HEADER: usize = 16;

unsafe extern "C" fn allocate_block(size: usize, out: *mut *mut c_void) -> i32 {
    if out.is_null() || size > 1 << 31 {
        return SUITE_NOT_FOUND;
    }
    // A header holds the size, so free and reallocate know the layout.
    let p = unsafe { std::alloc::alloc(layout(size + BLOCK_HEADER)) };
    if p.is_null() {
        return SUITE_NOT_FOUND;
    }
    unsafe {
        *(p as *mut usize) = size;
        *out = p.add(BLOCK_HEADER) as *mut c_void;
    }
    0
}

unsafe extern "C" fn free_block(b: *mut c_void) -> i32 {
    if !b.is_null() {
        unsafe {
            let p = (b as *mut u8).sub(BLOCK_HEADER);
            std::alloc::dealloc(p, layout(*(p as *mut usize) + BLOCK_HEADER));
        }
    }
    0
}

unsafe extern "C" fn reallocate_block(b: *mut c_void, size: usize, out: *mut *mut c_void) -> i32 {
    if b.is_null() {
        return unsafe { allocate_block(size, out) };
    }
    if out.is_null() || size > 1 << 31 {
        return SUITE_NOT_FOUND;
    }
    unsafe {
        let p = (b as *mut u8).sub(BLOCK_HEADER);
        let q = std::alloc::realloc(p, layout(*(p as *mut usize) + BLOCK_HEADER), size + BLOCK_HEADER);
        if q.is_null() {
            return SUITE_NOT_FOUND;
        }
        *(q as *mut usize) = size;
        *out = q.add(BLOCK_HEADER) as *mut c_void;
    }
    0
}

unsafe extern "C" fn undefined() -> i32 {
    SUITE_NOT_FOUND
}

/// The suite broker every command gets (made once).
pub fn basic() -> *mut BasicSuite {
    static B: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *B.get_or_init(|| leak(BasicSuite { acquire_suite, release_suite, is_equal, allocate_block, free_block, reallocate_block, undefined })) as *mut BasicSuite
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_world(w: i32, h: i32, f: impl Fn(i32, i32) -> Pixel) -> (Vec<u8>, World) {
        let rb = rowbytes(w);
        let mut buf = vec![0u8; (rb * h) as usize];
        for y in 0..h {
            for x in 0..w {
                let p = f(x, y);
                let i = (y * rb + x * 4) as usize;
                buf[i..i + 4].copy_from_slice(&[p.alpha, p.red, p.green, p.blue]);
            }
        }
        let wd = world(buf.as_mut_ptr(), w, h, rb, true);
        (buf, wd)
    }

    fn at(w: &World, x: i32, y: i32) -> Pixel {
        unsafe { *px(w, x, y).unwrap() }
    }

    unsafe extern "C" fn count(refcon: *mut c_void, x: i32, y: i32, inp: *mut Pixel, out: *mut Pixel) -> Err {
        unsafe {
            *(refcon as *mut u32) += 1;
            *out = Pixel { alpha: 255, red: x as u8, green: y as u8, blue: (*inp).red };
        }
        err::NONE
    }

    /// A3 acceptance: iterate visits every pixel of the area once, with the input pixel at the same place.
    #[test]
    fn iterate8_visits_every_pixel_once() {
        let (_a, mut src) = test_world(7, 5, |x, y| Pixel { alpha: 255, red: (x * 10 + y) as u8, ..Default::default() });
        let (_b, mut dst) = test_world(7, 5, |_, _| Pixel::default());
        let mut n = 0u32;
        let e = unsafe { iterate(std::ptr::null_mut(), 0, 0, &mut src, std::ptr::null(), &mut n as *mut u32 as *mut c_void, Some(count), &mut dst) };
        assert_eq!((e, n), (err::NONE, 35));
        assert_eq!(at(&dst, 6, 4), Pixel { alpha: 255, red: 6, green: 4, blue: 64 });
        let mut n = 0u32;
        let area = Rect { left: 2, top: 1, right: 4, bottom: 3 };
        unsafe { iterate(std::ptr::null_mut(), 0, 0, &mut src, &area, &mut n as *mut u32 as *mut c_void, Some(count), &mut dst) };
        assert_eq!(n, 4, "only the area");
        let mut n = 0u32;
        let big = Rect { left: -5, top: -5, right: 50, bottom: 50 };
        unsafe { iterate(std::ptr::null_mut(), 0, 0, &mut src, &big, &mut n as *mut u32 as *mut c_void, Some(count), &mut dst) };
        assert_eq!(n, 35, "clipped to the output");
        let mut n = 0u32;
        unsafe { iterate_generic(9, &mut n as *mut u32 as *mut c_void, Some(sum_up)) };
        assert_eq!(n, 0 + 1 + 2 + 3 + 4 + 5 + 6 + 7 + 8);
    }

    unsafe extern "C" fn sum_up(refcon: *mut c_void, _t: i32, i: i32, _n: i32) -> Err {
        unsafe { *(refcon as *mut u32) += i as u32 };
        err::NONE
    }

    /// A3 acceptance: a handle keeps its bytes when it grows or shrinks, reports its size, and is forgotten when disposed.
    #[test]
    fn handle_suite_resize_keeps_data() {
        unsafe {
            let mut h = host_new_handle(8);
            let p = host_lock_handle(h) as *mut u8;
            for i in 0..8 {
                *p.add(i) = i as u8 + 1;
            }
            assert_eq!(host_get_handle_size(h), 8);
            let before = h;
            assert_eq!(host_resize_handle(100_000, &mut h), err::NONE);
            assert_eq!(h, before, "the handle stays; its block may move");
            assert_eq!(host_get_handle_size(h), 100_000);
            let q = host_lock_handle(h) as *mut u8;
            assert_eq!(std::slice::from_raw_parts(q, 9), &[1, 2, 3, 4, 5, 6, 7, 8, 0]);
            assert_eq!(host_resize_handle(3, &mut h), err::NONE);
            assert_eq!(handle_bytes(h).unwrap(), [1, 2, 3]);
            host_dispose_handle(h);
            assert!(host_lock_handle(h).is_null() && host_get_handle_size(h) == 0, "a disposed handle is unknown");
            host_dispose_handle(h);
            let mut bogus = 12345usize as Handle;
            assert_eq!(host_resize_handle(4, &mut bogus), err::BAD_CALLBACK_PARAM);
        }
    }

    /// A3 acceptance: copy and fill touch only their rectangles (clipped to the world).
    #[test]
    fn copy_and_fill_respect_rects() {
        let red = Pixel { alpha: 255, red: 255, green: 0, blue: 0 };
        let (_a, mut w) = test_world(6, 4, |_, _| Pixel::default());
        let r = Rect { left: 1, top: 1, right: 3, bottom: 9 };
        unsafe { fill(std::ptr::null_mut(), &red, &r, &mut w) };
        assert_eq!((at(&w, 1, 1), at(&w, 2, 3), at(&w, 0, 1), at(&w, 3, 1), at(&w, 1, 0)), (red, red, Pixel::default(), Pixel::default(), Pixel::default()));
        let (_b, mut src) = test_world(6, 4, |x, y| Pixel { alpha: 255, red: x as u8, green: y as u8, blue: 9 });
        let (_c, mut dst) = test_world(6, 4, |_, _| Pixel::default());
        let (sr, dr) = (Rect { left: 0, top: 0, right: 2, bottom: 2 }, Rect { left: 4, top: 2, right: 6, bottom: 4 });
        unsafe { copy(std::ptr::null_mut(), &mut src, &mut dst, &sr, &dr) };
        assert_eq!(at(&dst, 5, 3), Pixel { alpha: 255, red: 1, green: 1, blue: 9 });
        assert_eq!(at(&dst, 3, 3), Pixel::default(), "outside the destination rectangle");
        unsafe { copy(std::ptr::null_mut(), &mut src, &mut dst, std::ptr::null(), std::ptr::null()) };
        assert_eq!(at(&dst, 3, 2), Pixel { alpha: 255, red: 3, green: 2, blue: 9 });
        // Premultiplying halves a half-transparent pixel and dividing restores it.
        let (_d, mut p) = test_world(1, 1, |_, _| Pixel { alpha: 128, red: 200, green: 100, blue: 0 });
        unsafe { premultiply(std::ptr::null_mut(), 1, &mut p) };
        assert_eq!(at(&p, 0, 0), Pixel { alpha: 128, red: 100, green: 50, blue: 0 });
        unsafe { premultiply(std::ptr::null_mut(), 0, &mut p) };
        assert_eq!(at(&p, 0, 0), Pixel { alpha: 128, red: 199, green: 100, blue: 0 });
        // A suite is found by name and version; others are refused and noted.
        let mut s: *const c_void = std::ptr::null();
        assert_eq!(unsafe { acquire_suite(suite::HANDLE.0.as_ptr(), 1, &mut s) }, 0);
        assert!(!s.is_null());
        assert_eq!(unsafe { acquire_suite(c"PF Made Up Suite".as_ptr(), 7, &mut s) }, SUITE_NOT_FOUND);
        assert!(s.is_null() && with(|c| c.refused.contains("PF Made Up Suite v7")));
        let mut buf = [0 as c_char; 64];
        let n = unsafe { c_sprintf(buf.as_mut_ptr(), c"%d-%s-%.2f".as_ptr(), 42, c"x".as_ptr() as u64, 1.5f64.to_bits(), 0, 0, 0, 0, 0) };
        assert_eq!(unsafe { CStr::from_ptr(buf.as_ptr()) }.to_str().unwrap(), "42-x-1.50");
        assert_eq!(n, 9);
    }
}
