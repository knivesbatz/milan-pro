//! The host suites a plug-in fetches: property, image effect, parameter, memory, multithread, message, progress and
//! time line. Every handle given to a plug-in is a pointer to one of the boxed objects below; they live as long as the
//! descriptor or instance that owns them (`host.rs`). Interact, parametric and GPU suites are not offered: `fetch_suite`
//! returns NULL for them, which OFX defines as "not supported".
//!
//! Variadic parameter functions: stable Rust cannot define C `...` functions, so `paramGetValue` and friends are fixed
//! arity functions that read four pointer-sized slots. On the Windows x64 calling convention a variadic call passes its
//! arguments exactly like a fixed one (a double in a variadic slot is also copied to the integer register), so reading
//! the slots as `usize`/`u64` is the documented ABI; only the slots the parameter's type needs are read. The host only
//! builds for x86_64 Windows (`main.rs`).
use std::alloc::Layout;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_void};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, LazyLock, Mutex, MutexGuard};
use std::thread::ThreadId;
use crate::ofx::*;

/// Lock, ignoring poisoning (a panic in the host aborts it anyway: `panic = abort` behaviour across `extern "C"`).
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

// ---------------------------------------------------------------- property sets

#[derive(Clone, Debug, PartialEq)]
pub enum V {
    I(Vec<c_int>),
    D(Vec<f64>),
    S(Vec<CString>),
    P(Vec<usize>),
}

impl V {
    fn len(&self) -> usize {
        match self {
            V::I(v) => v.len(),
            V::D(v) => v.len(),
            V::S(v) => v.len(),
            V::P(v) => v.len(),
        }
    }
}

#[derive(Debug, Default)]
pub struct Props {
    m: Mutex<HashMap<String, V>>,
}

impl Props {
    pub fn handle(&self) -> OfxPropertySetHandle {
        self as *const Props as OfxPropertySetHandle
    }

    pub fn copy(&self) -> Box<Props> {
        Box::new(Props { m: Mutex::new(lock(&self.m).clone()) })
    }

    pub fn set(&self, k: &CStr, v: V) {
        lock(&self.m).insert(k.to_string_lossy().into_owned(), v);
    }
    pub fn s(&self, k: &CStr, v: &CStr) {
        self.set(k, V::S(vec![v.to_owned()]));
    }
    pub fn st(&self, k: &CStr, v: &str) {
        self.set(k, V::S(vec![cstring(v)]));
    }
    pub fn i(&self, k: &CStr, v: c_int) {
        self.set(k, V::I(vec![v]));
    }
    pub fn d(&self, k: &CStr, v: &[f64]) {
        self.set(k, V::D(v.to_vec()));
    }
    pub fn p(&self, k: &CStr, v: usize) {
        self.set(k, V::P(vec![v]));
    }

    pub fn get_s(&self, k: &CStr) -> Vec<String> {
        match lock(&self.m).get(&*k.to_string_lossy()) {
            Some(V::S(v)) => v.iter().map(|s| s.to_string_lossy().into_owned()).collect(),
            _ => Vec::new(),
        }
    }
    pub fn s0(&self, k: &CStr) -> String {
        self.get_s(k).into_iter().next().unwrap_or_default()
    }
    pub fn get_d(&self, k: &CStr) -> Vec<f64> {
        match lock(&self.m).get(&*k.to_string_lossy()) {
            Some(V::D(v)) => v.clone(),
            Some(V::I(v)) => v.iter().map(|&x| x as f64).collect(),
            _ => Vec::new(),
        }
    }
    pub fn i0(&self, k: &CStr) -> Option<c_int> {
        self.get_d(k).first().map(|&x| x as c_int)
    }
}

pub fn cstring(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

unsafe fn name<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() { None } else { unsafe { CStr::from_ptr(p) }.to_str().ok() }
}

unsafe fn props<'a>(h: OfxPropertySetHandle) -> Option<&'a Props> {
    unsafe { (h as *const Props).as_ref() }
}

/// Set element `i` of property `k` to `x` (made `make` if absent or of another type; grown as needed).
unsafe fn prop_set<T: Clone>(h: OfxPropertySetHandle, k: *const c_char, i: c_int, x: T, fill: T, wrap: fn(Vec<T>) -> V, get: fn(&mut V) -> Option<&mut Vec<T>>) -> OfxStatus {
    let (Some(p), Some(k)) = (unsafe { props(h) }, unsafe { name(k) }) else { return kOfxStatErrBadHandle };
    if i < 0 {
        return kOfxStatErrBadIndex;
    }
    let mut m = lock(&p.m);
    let e = m.entry(k.to_string()).or_insert_with(|| wrap(Vec::new()));
    if get(e).is_none() {
        *e = wrap(Vec::new());
    }
    let v = get(e).expect("just made");
    let i = i as usize;
    if v.len() <= i {
        v.resize(i + 1, fill);
    }
    v[i] = x;
    kOfxStatOK
}

fn vi(v: &mut V) -> Option<&mut Vec<c_int>> {
    if let V::I(x) = v { Some(x) } else { None }
}
fn vd(v: &mut V) -> Option<&mut Vec<f64>> {
    if let V::D(x) = v { Some(x) } else { None }
}
fn vs(v: &mut V) -> Option<&mut Vec<CString>> {
    if let V::S(x) = v { Some(x) } else { None }
}
fn vp(v: &mut V) -> Option<&mut Vec<usize>> {
    if let V::P(x) = v { Some(x) } else { None }
}

unsafe extern "C" fn prop_set_pointer(h: OfxPropertySetHandle, k: *const c_char, i: c_int, x: *mut c_void) -> OfxStatus {
    unsafe { prop_set(h, k, i, x as usize, 0, V::P, vp) }
}
unsafe extern "C" fn prop_set_string(h: OfxPropertySetHandle, k: *const c_char, i: c_int, x: *const c_char) -> OfxStatus {
    let s = if x.is_null() { CString::default() } else { unsafe { CStr::from_ptr(x) }.to_owned() };
    unsafe { prop_set(h, k, i, s, CString::default(), V::S, vs) }
}
unsafe extern "C" fn prop_set_double(h: OfxPropertySetHandle, k: *const c_char, i: c_int, x: f64) -> OfxStatus {
    unsafe { prop_set(h, k, i, x, 0.0, V::D, vd) }
}
unsafe extern "C" fn prop_set_int(h: OfxPropertySetHandle, k: *const c_char, i: c_int, x: c_int) -> OfxStatus {
    unsafe { prop_set(h, k, i, x, 0, V::I, vi) }
}

/// Replace property `k` with `n` values read from `x`.
unsafe fn prop_set_n<T>(h: OfxPropertySetHandle, k: *const c_char, n: c_int, x: *const T, f: impl Fn(&T) -> V, join: fn(Vec<V>) -> V) -> OfxStatus {
    let (Some(p), Some(k)) = (unsafe { props(h) }, unsafe { name(k) }) else { return kOfxStatErrBadHandle };
    if n < 0 || (n > 0 && x.is_null()) {
        return kOfxStatErrBadIndex;
    }
    let vals: Vec<V> = (0..n as usize).map(|i| f(unsafe { &*x.add(i) })).collect();
    lock(&p.m).insert(k.to_string(), join(vals));
    kOfxStatOK
}

unsafe extern "C" fn prop_set_pointer_n(h: OfxPropertySetHandle, k: *const c_char, n: c_int, x: *const *mut c_void) -> OfxStatus {
    unsafe { prop_set_n(h, k, n, x, |p| V::P(vec![*p as usize]), |v| V::P(v.into_iter().flat_map(|x| if let V::P(p) = x { p } else { vec![] }).collect())) }
}
unsafe extern "C" fn prop_set_string_n(h: OfxPropertySetHandle, k: *const c_char, n: c_int, x: *const *const c_char) -> OfxStatus {
    let one = |p: &*const c_char| V::S(vec![if p.is_null() { CString::default() } else { unsafe { CStr::from_ptr(*p) }.to_owned() }]);
    unsafe { prop_set_n(h, k, n, x, one, |v| V::S(v.into_iter().flat_map(|x| if let V::S(p) = x { p } else { vec![] }).collect())) }
}
unsafe extern "C" fn prop_set_double_n(h: OfxPropertySetHandle, k: *const c_char, n: c_int, x: *const f64) -> OfxStatus {
    unsafe { prop_set_n(h, k, n, x, |p| V::D(vec![*p]), |v| V::D(v.into_iter().flat_map(|x| if let V::D(p) = x { p } else { vec![] }).collect())) }
}
unsafe extern "C" fn prop_set_int_n(h: OfxPropertySetHandle, k: *const c_char, n: c_int, x: *const c_int) -> OfxStatus {
    unsafe { prop_set_n(h, k, n, x, |p| V::I(vec![*p]), |v| V::I(v.into_iter().flat_map(|x| if let V::I(p) = x { p } else { vec![] }).collect())) }
}

/// Element `i` of property `k` through `f` (which writes it out); numbers convert between int and double.
unsafe fn prop_get(h: OfxPropertySetHandle, k: *const c_char, i: c_int, f: impl FnOnce(&V, usize) -> OfxStatus) -> OfxStatus {
    let (Some(p), Some(k)) = (unsafe { props(h) }, unsafe { name(k) }) else { return kOfxStatErrBadHandle };
    let m = lock(&p.m);
    let Some(v) = m.get(k) else { return kOfxStatErrUnknown };
    if i < 0 || i as usize >= v.len() {
        return kOfxStatErrBadIndex;
    }
    f(v, i as usize)
}

unsafe extern "C" fn prop_get_pointer(h: OfxPropertySetHandle, k: *const c_char, i: c_int, out: *mut *mut c_void) -> OfxStatus {
    if out.is_null() {
        return kOfxStatErrBadHandle;
    }
    unsafe {
        prop_get(h, k, i, |v, i| match v {
            V::P(p) => {
                *out = p[i] as *mut c_void;
                kOfxStatOK
            }
            _ => kOfxStatErrValue,
        })
    }
}
unsafe extern "C" fn prop_get_string(h: OfxPropertySetHandle, k: *const c_char, i: c_int, out: *mut *mut c_char) -> OfxStatus {
    if out.is_null() {
        return kOfxStatErrBadHandle;
    }
    unsafe {
        prop_get(h, k, i, |v, i| match v {
            // Valid until the property changes (OFX's rule).
            V::S(s) => {
                *out = s[i].as_ptr() as *mut c_char;
                kOfxStatOK
            }
            _ => kOfxStatErrValue,
        })
    }
}
unsafe extern "C" fn prop_get_double(h: OfxPropertySetHandle, k: *const c_char, i: c_int, out: *mut f64) -> OfxStatus {
    if out.is_null() {
        return kOfxStatErrBadHandle;
    }
    unsafe {
        prop_get(h, k, i, |v, i| match v {
            V::D(d) => {
                *out = d[i];
                kOfxStatOK
            }
            V::I(d) => {
                *out = d[i] as f64;
                kOfxStatOK
            }
            _ => kOfxStatErrValue,
        })
    }
}
unsafe extern "C" fn prop_get_int(h: OfxPropertySetHandle, k: *const c_char, i: c_int, out: *mut c_int) -> OfxStatus {
    if out.is_null() {
        return kOfxStatErrBadHandle;
    }
    unsafe {
        prop_get(h, k, i, |v, i| match v {
            V::I(d) => {
                *out = d[i];
                kOfxStatOK
            }
            V::D(d) => {
                *out = d[i] as c_int;
                kOfxStatOK
            }
            _ => kOfxStatErrValue,
        })
    }
}

/// The N getters: element by element through the single getters.
macro_rules! get_n {
    ($name:ident, $one:ident, $t:ty) => {
        unsafe extern "C" fn $name(h: OfxPropertySetHandle, k: *const c_char, n: c_int, out: *mut $t) -> OfxStatus {
            if n > 0 && out.is_null() {
                return kOfxStatErrBadHandle;
            }
            for i in 0..n.max(0) {
                let st = unsafe { $one(h, k, i, out.add(i as usize)) };
                if st != kOfxStatOK {
                    return st;
                }
            }
            kOfxStatOK
        }
    };
}
get_n!(prop_get_pointer_n, prop_get_pointer, *mut c_void);
get_n!(prop_get_string_n, prop_get_string, *mut c_char);
get_n!(prop_get_double_n, prop_get_double, f64);
get_n!(prop_get_int_n, prop_get_int, c_int);

unsafe extern "C" fn prop_reset(h: OfxPropertySetHandle, k: *const c_char) -> OfxStatus {
    let (Some(p), Some(k)) = (unsafe { props(h) }, unsafe { name(k) }) else { return kOfxStatErrBadHandle };
    // shortcut: no defaults are kept per property; reset empties it (what plug-ins use it for: choice options).
    match lock(&p.m).get_mut(k) {
        Some(V::I(v)) => v.clear(),
        Some(V::D(v)) => v.clear(),
        Some(V::S(v)) => v.clear(),
        Some(V::P(v)) => v.clear(),
        None => return kOfxStatErrUnknown,
    }
    kOfxStatOK
}

unsafe extern "C" fn prop_get_dimension(h: OfxPropertySetHandle, k: *const c_char, out: *mut c_int) -> OfxStatus {
    let (Some(p), Some(k)) = (unsafe { props(h) }, unsafe { name(k) }) else { return kOfxStatErrBadHandle };
    let Some(n) = lock(&p.m).get(k).map(V::len) else { return kOfxStatErrUnknown };
    if !out.is_null() {
        unsafe { *out = n as c_int };
    }
    kOfxStatOK
}

pub static PROPERTY_SUITE: OfxPropertySuiteV1 = OfxPropertySuiteV1 {
    propSetPointer: prop_set_pointer,
    propSetString: prop_set_string,
    propSetDouble: prop_set_double,
    propSetInt: prop_set_int,
    propSetPointerN: prop_set_pointer_n,
    propSetStringN: prop_set_string_n,
    propSetDoubleN: prop_set_double_n,
    propSetIntN: prop_set_int_n,
    propGetPointer: prop_get_pointer,
    propGetString: prop_get_string,
    propGetDouble: prop_get_double,
    propGetInt: prop_get_int,
    propGetPointerN: prop_get_pointer_n,
    propGetStringN: prop_get_string_n,
    propGetDoubleN: prop_get_double_n,
    propGetIntN: prop_get_int_n,
    propReset: prop_reset,
    propGetDimension: prop_get_dimension,
};

// ---------------------------------------------------------------- effects, clips, parameters

/// The picture being rendered, in shared memory: bottom row first (OFX's origin), RGBA.
#[derive(Clone, Copy, Debug, Default)]
pub struct Frame {
    pub live: bool,
    pub src: usize,
    pub dst: usize,
    pub w: u32,
    pub h: u32,
    pub float: bool,
    pub scale: f64,
    pub par: f64,
    pub time: f64,
}

impl Frame {
    /// The frame in canonical coordinates (project pixels, PAR applied).
    pub fn rod(&self) -> OfxRectD {
        let s = self.scale.max(1e-6);
        OfxRectD { x1: 0.0, y1: 0.0, x2: self.w as f64 * self.par / s, y2: self.h as f64 / s }
    }
}

pub struct Effect {
    pub props: Box<Props>,
    pub params: Box<ParamSet>,
    pub clips: Mutex<Vec<Box<Clip>>>,
    pub frame: Mutex<Frame>,
}

impl Effect {
    pub fn new(props: Box<Props>) -> Box<Effect> {
        Box::new(Effect { props, params: Box::new(ParamSet::default()), clips: Mutex::new(Vec::new()), frame: Mutex::new(Frame { scale: 1.0, par: 1.0, ..Default::default() }) })
    }

    pub fn handle(&self) -> OfxImageEffectHandle {
        self as *const Effect as OfxImageEffectHandle
    }

    pub fn add_clip(&self, name: &str, props: Box<Props>) -> *const Clip {
        let c = Box::new(Clip { name: name.to_string(), props, effect: self as *const Effect as usize });
        let p = &*c as *const Clip;
        lock(&self.clips).push(c);
        p
    }
}

pub struct Clip {
    pub name: String,
    pub props: Box<Props>,
    /// The `Effect` that owns this clip (it outlives it).
    effect: usize,
}

#[derive(Default)]
pub struct ParamSet {
    pub props: Box<Props>,
    pub params: Mutex<Vec<Box<Param>>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PV {
    N(Vec<f64>),
    S(CString),
}

pub struct Param {
    pub name: String,
    /// The full OFX type ("OfxParamTypeDouble").
    pub kind: String,
    pub props: Box<Props>,
    pub value: Mutex<PV>,
}

/// How a parameter type stores its value: ints, doubles (with their count) or a string.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Class {
    Int(usize),
    Dbl(usize),
    Str,
    None,
}

pub fn class(kind: &str) -> Class {
    match kind.strip_prefix("OfxParamType").unwrap_or(kind) {
        "Integer" | "Boolean" | "Choice" => Class::Int(1),
        "Integer2D" => Class::Int(2),
        "Integer3D" => Class::Int(3),
        "Double" => Class::Dbl(1),
        "Double2D" => Class::Dbl(2),
        "Double3D" | "RGB" => Class::Dbl(3),
        "RGBA" => Class::Dbl(4),
        "String" | "Custom" => Class::Str,
        _ => Class::None,
    }
}

impl Param {
    pub fn new(name: &str, kind: &str, props: Box<Props>) -> Box<Param> {
        let value = Mutex::new(Self::default_of(kind, &props));
        Box::new(Param { name: name.into(), kind: kind.into(), props, value })
    }

    pub fn default_of(kind: &str, props: &Props) -> PV {
        match class(kind) {
            Class::Str => PV::S(cstring(&props.s0(kOfxParamPropDefault))),
            Class::Int(n) | Class::Dbl(n) => {
                let mut d = props.get_d(kOfxParamPropDefault);
                d.resize(n, 0.0);
                PV::N(d)
            }
            Class::None => PV::N(Vec::new()),
        }
    }

    /// A copy for an instance: same properties, the default value.
    pub fn instance_copy(&self) -> Box<Param> {
        let props = self.props.copy();
        props.s(kOfxPropType, kOfxTypeParameterInstance);
        Param::new(&self.name, &self.kind, props)
    }
}

impl ParamSet {
    pub fn handle(&self) -> OfxParamSetHandle {
        self as *const ParamSet as OfxParamSetHandle
    }

    pub fn find(&self, name: &str) -> Option<*const Param> {
        lock(&self.params).iter().find(|p| p.name == name).map(|p| &**p as *const Param)
    }
}

unsafe fn effect<'a>(h: OfxImageEffectHandle) -> Option<&'a Effect> {
    unsafe { (h as *const Effect).as_ref() }
}

unsafe extern "C" fn get_property_set(h: OfxImageEffectHandle, out: *mut OfxPropertySetHandle) -> OfxStatus {
    let Some(e) = (unsafe { effect(h) }) else { return kOfxStatErrBadHandle };
    if !out.is_null() {
        unsafe { *out = e.props.handle() };
    }
    kOfxStatOK
}

unsafe extern "C" fn get_param_set(h: OfxImageEffectHandle, out: *mut OfxParamSetHandle) -> OfxStatus {
    let Some(e) = (unsafe { effect(h) }) else { return kOfxStatErrBadHandle };
    if !out.is_null() {
        unsafe { *out = e.params.handle() };
    }
    kOfxStatOK
}

unsafe extern "C" fn clip_define(h: OfxImageEffectHandle, n: *const c_char, out: *mut OfxPropertySetHandle) -> OfxStatus {
    let (Some(e), Some(n)) = (unsafe { effect(h) }, unsafe { name(n) }) else { return kOfxStatErrBadHandle };
    if lock(&e.clips).iter().any(|c| c.name == n) {
        return kOfxStatErrExists;
    }
    let p = Box::new(Props::default());
    p.s(kOfxPropType, kOfxTypeClip);
    p.st(kOfxPropName, n);
    p.st(kOfxPropLabel, n);
    p.i(kOfxImageClipPropOptional, 0);
    p.i(kOfxImageClipPropIsMask, 0);
    p.set(kOfxImageEffectPropSupportedComponents, V::S(Vec::new()));
    p.i(kOfxImageEffectPropTemporalClipAccess, 0);
    p.i(kOfxImageEffectPropSupportsTiles, 1);
    let c = e.add_clip(n, p);
    if !out.is_null() {
        unsafe { *out = (*c).props.handle() };
    }
    kOfxStatOK
}

unsafe extern "C" fn clip_get_handle(h: OfxImageEffectHandle, n: *const c_char, clip: *mut OfxImageClipHandle, out: *mut OfxPropertySetHandle) -> OfxStatus {
    let (Some(e), Some(n)) = (unsafe { effect(h) }, unsafe { name(n) }) else { return kOfxStatErrBadHandle };
    let clips = lock(&e.clips);
    let Some(c) = clips.iter().find(|c| c.name == n) else { return kOfxStatErrUnknown };
    unsafe {
        if !clip.is_null() {
            *clip = &**c as *const Clip as OfxImageClipHandle;
        }
        if !out.is_null() {
            *out = c.props.handle();
        }
    }
    kOfxStatOK
}

unsafe extern "C" fn clip_get_property_set(c: OfxImageClipHandle, out: *mut OfxPropertySetHandle) -> OfxStatus {
    let Some(c) = (unsafe { (c as *const Clip).as_ref() }) else { return kOfxStatErrBadHandle };
    if !out.is_null() {
        unsafe { *out = c.props.handle() };
    }
    kOfxStatOK
}

/// Image property sets handed out and not released yet (a release of anything else is refused, never freed twice).
static LIVE_IMAGES: LazyLock<Mutex<HashSet<usize>>> = LazyLock::new(Default::default);
static IMAGE_SERIAL: AtomicU64 = AtomicU64::new(1);

unsafe extern "C" fn clip_get_image(c: OfxImageClipHandle, _t: OfxTime, _region: *const OfxRectD, out: *mut OfxPropertySetHandle) -> OfxStatus {
    let Some(c) = (unsafe { (c as *const Clip).as_ref() }) else { return kOfxStatErrBadHandle };
    if out.is_null() {
        return kOfxStatErrBadHandle;
    }
    // SAFETY: the owning effect outlives its clips.
    let f = *lock(&unsafe { &*(c.effect as *const Effect) }.frame);
    // shortcut: every time gives the frame being rendered (no temporal access: Milan Pro says so in its host
    // properties); fetch other frames when a plug-in needs them.
    let data = match c.name.as_str() {
        "Output" => f.dst,
        "Source" => f.src,
        _ => 0,
    };
    if !f.live || data == 0 {
        return kOfxStatFailed;
    }
    let bpp = if f.float { 16 } else { 4 };
    let p = Box::new(Props::default());
    p.s(kOfxPropType, kOfxTypeImage);
    p.s(kOfxImageEffectPropPixelDepth, if f.float { kOfxBitDepthFloat } else { kOfxBitDepthByte });
    p.s(kOfxImageEffectPropComponents, kOfxImageComponentRGBA);
    p.s(kOfxImageEffectPropPreMultiplication, kOfxImagePreMultiplied);
    p.d(kOfxImageEffectPropRenderScale, &[f.scale, f.scale]);
    p.d(kOfxImagePropPixelAspectRatio, &[f.par]);
    p.p(kOfxImagePropData, data);
    p.set(kOfxImagePropBounds, V::I(vec![0, 0, f.w as c_int, f.h as c_int]));
    p.set(kOfxImagePropRegionOfDefinition, V::I(vec![0, 0, f.w as c_int, f.h as c_int]));
    p.i(kOfxImagePropRowBytes, (f.w as usize * bpp) as c_int);
    p.s(kOfxImagePropField, kOfxImageFieldNone);
    p.st(kOfxImagePropUniqueIdentifier, &format!("milan-{}", IMAGE_SERIAL.fetch_add(1, Ordering::Relaxed)));
    let raw = Box::into_raw(p);
    lock(&LIVE_IMAGES).insert(raw as usize);
    unsafe { *out = raw as OfxPropertySetHandle };
    kOfxStatOK
}

unsafe extern "C" fn clip_release_image(h: OfxPropertySetHandle) -> OfxStatus {
    if !lock(&LIVE_IMAGES).remove(&(h as usize)) {
        return kOfxStatErrBadHandle;
    }
    // SAFETY: made by Box::into_raw in clip_get_image and removed from the live set just now, so freed once.
    drop(unsafe { Box::from_raw(h as *mut Props) });
    kOfxStatOK
}

unsafe extern "C" fn clip_get_rod(c: OfxImageClipHandle, _t: OfxTime, out: *mut OfxRectD) -> OfxStatus {
    let Some(c) = (unsafe { (c as *const Clip).as_ref() }) else { return kOfxStatErrBadHandle };
    let f = *lock(&unsafe { &*(c.effect as *const Effect) }.frame);
    if !out.is_null() {
        unsafe { *out = f.rod() };
    }
    kOfxStatOK
}

unsafe extern "C" fn abort(_h: OfxImageEffectHandle) -> c_int {
    0
}

struct ImageMem {
    ptr: *mut u8,
    layout: Layout,
}

unsafe extern "C" fn image_memory_alloc(_h: OfxImageEffectHandle, n: usize, out: *mut OfxImageMemoryHandle) -> OfxStatus {
    if out.is_null() {
        return kOfxStatErrBadHandle;
    }
    let Ok(layout) = Layout::from_size_align(n.max(1), 64) else { return kOfxStatErrMemory };
    // SAFETY: a non-zero size.
    let ptr = unsafe { std::alloc::alloc(layout) };
    if ptr.is_null() {
        return kOfxStatErrMemory;
    }
    unsafe { *out = Box::into_raw(Box::new(ImageMem { ptr, layout })) as OfxImageMemoryHandle };
    kOfxStatOK
}

unsafe extern "C" fn image_memory_free(h: OfxImageMemoryHandle) -> OfxStatus {
    if h.is_null() {
        return kOfxStatErrBadHandle;
    }
    let m = unsafe { Box::from_raw(h as *mut ImageMem) };
    unsafe { std::alloc::dealloc(m.ptr, m.layout) };
    kOfxStatOK
}

unsafe extern "C" fn image_memory_lock(h: OfxImageMemoryHandle, out: *mut *mut c_void) -> OfxStatus {
    let Some(m) = (unsafe { (h as *const ImageMem).as_ref() }) else { return kOfxStatErrBadHandle };
    if !out.is_null() {
        unsafe { *out = m.ptr as *mut c_void };
    }
    kOfxStatOK
}

unsafe extern "C" fn image_memory_unlock(h: OfxImageMemoryHandle) -> OfxStatus {
    if h.is_null() { kOfxStatErrBadHandle } else { kOfxStatOK }
}

pub static IMAGE_EFFECT_SUITE: OfxImageEffectSuiteV1 = OfxImageEffectSuiteV1 {
    getPropertySet: get_property_set,
    getParamSet: get_param_set,
    clipDefine: clip_define,
    clipGetHandle: clip_get_handle,
    clipGetPropertySet: clip_get_property_set,
    clipGetImage: clip_get_image,
    clipReleaseImage: clip_release_image,
    clipGetRegionOfDefinition: clip_get_rod,
    abort,
    imageMemoryAlloc: image_memory_alloc,
    imageMemoryFree: image_memory_free,
    imageMemoryLock: image_memory_lock,
    imageMemoryUnlock: image_memory_unlock,
};

// ---- parameters

unsafe fn param_set<'a>(h: OfxParamSetHandle) -> Option<&'a ParamSet> {
    unsafe { (h as *const ParamSet).as_ref() }
}
unsafe fn param<'a>(h: OfxParamHandle) -> Option<&'a Param> {
    unsafe { (h as *const Param).as_ref() }
}

/// The OFX parameter types Milan Pro hosts.
const TYPES: [&CStr; 15] = [
    kOfxParamTypeInteger,
    kOfxParamTypeDouble,
    kOfxParamTypeBoolean,
    kOfxParamTypeChoice,
    kOfxParamTypeRGBA,
    kOfxParamTypeRGB,
    kOfxParamTypeDouble2D,
    kOfxParamTypeInteger2D,
    kOfxParamTypeDouble3D,
    kOfxParamTypeInteger3D,
    kOfxParamTypeString,
    kOfxParamTypeCustom,
    kOfxParamTypeGroup,
    kOfxParamTypePage,
    kOfxParamTypePushButton,
];

unsafe extern "C" fn param_define(set: OfxParamSetHandle, kind: *const c_char, n: *const c_char, out: *mut OfxPropertySetHandle) -> OfxStatus {
    let (Some(s), Some(kind), Some(n)) = (unsafe { param_set(set) }, unsafe { name(kind) }, unsafe { name(n) }) else { return kOfxStatErrBadHandle };
    if !TYPES.iter().any(|t| t.to_bytes() == kind.as_bytes()) {
        return kOfxStatErrUnsupported;
    }
    if s.find(n).is_some() {
        return kOfxStatErrExists;
    }
    let p = Box::new(Props::default());
    p.s(kOfxPropType, kOfxTypeParameter);
    p.st(kOfxParamPropType, kind);
    p.st(kOfxPropName, n);
    p.st(kOfxPropLabel, n);
    p.st(kOfxParamPropScriptName, n);
    p.st(kOfxParamPropHint, "");
    p.st(kOfxParamPropParent, "");
    p.i(kOfxParamPropSecret, 0);
    p.i(kOfxParamPropEnabled, 1);
    p.i(kOfxParamPropAnimates, 1);
    p.i(kOfxParamPropIsAnimating, 0);
    p.i(kOfxParamPropPersistant, 1);
    p.i(kOfxParamPropEvaluateOnChange, 1);
    match class(kind) {
        Class::Int(k) => {
            p.set(kOfxParamPropDefault, V::I(vec![0; k]));
            for (key, v) in [(kOfxParamPropMin, c_int::MIN), (kOfxParamPropMax, c_int::MAX), (kOfxParamPropDisplayMin, c_int::MIN), (kOfxParamPropDisplayMax, c_int::MAX)] {
                p.set(key, V::I(vec![v; k]));
            }
        }
        Class::Dbl(k) => {
            p.set(kOfxParamPropDefault, V::D(vec![if kind.ends_with("RGBA") { 1.0 } else { 0.0 }; k]));
            let rgb = kind.ends_with("RGB") || kind.ends_with("RGBA");
            let (lo, hi) = if rgb { (0.0, 1.0) } else { (-f64::MAX, f64::MAX) };
            for (key, v) in [(kOfxParamPropMin, lo), (kOfxParamPropMax, hi), (kOfxParamPropDisplayMin, lo), (kOfxParamPropDisplayMax, hi)] {
                p.set(key, V::D(vec![v; k]));
            }
            p.s(kOfxParamPropDoubleType, kOfxParamDoubleTypePlain);
        }
        Class::Str => {
            p.st(kOfxParamPropDefault, "");
            p.s(kOfxParamPropStringMode, kOfxParamStringIsSingleLine);
        }
        Class::None => {}
    }
    if kind == "OfxParamTypeChoice" {
        p.set(kOfxParamPropChoiceOption, V::S(Vec::new()));
    }
    let param = Param::new(n, kind, p);
    let ph = param.props.handle();
    lock(&s.params).push(param);
    if !out.is_null() {
        unsafe { *out = ph };
    }
    kOfxStatOK
}

unsafe extern "C" fn param_get_handle(set: OfxParamSetHandle, n: *const c_char, out: *mut OfxParamHandle, props: *mut OfxPropertySetHandle) -> OfxStatus {
    let (Some(s), Some(n)) = (unsafe { param_set(set) }, unsafe { name(n) }) else { return kOfxStatErrBadHandle };
    let Some(p) = s.find(n) else { return kOfxStatErrUnknown };
    unsafe {
        if !out.is_null() {
            *out = p as OfxParamHandle;
        }
        if !props.is_null() {
            *props = (*p).props.handle();
        }
    }
    kOfxStatOK
}

unsafe extern "C" fn param_set_get_property_set(set: OfxParamSetHandle, out: *mut OfxPropertySetHandle) -> OfxStatus {
    let Some(s) = (unsafe { param_set(set) }) else { return kOfxStatErrBadHandle };
    if !out.is_null() {
        unsafe { *out = s.props.handle() };
    }
    kOfxStatOK
}

unsafe extern "C" fn param_get_property_set(h: OfxParamHandle, out: *mut OfxPropertySetHandle) -> OfxStatus {
    let Some(p) = (unsafe { param(h) }) else { return kOfxStatErrBadHandle };
    if !out.is_null() {
        unsafe { *out = p.props.handle() };
    }
    kOfxStatOK
}

/// Write `h`'s value through the pointers in `out` (as many as its type has).
unsafe fn get_value(h: OfxParamHandle, out: [usize; 4]) -> OfxStatus {
    let Some(p) = (unsafe { param(h) }) else { return kOfxStatErrBadHandle };
    let v = lock(&p.value);
    match (class(&p.kind), &*v) {
        (Class::Int(n), PV::N(x)) => {
            for i in 0..n {
                if out[i] == 0 {
                    return kOfxStatErrBadHandle;
                }
                unsafe { *(out[i] as *mut c_int) = x.get(i).copied().unwrap_or(0.0).round() as c_int };
            }
        }
        (Class::Dbl(n), PV::N(x)) => {
            for i in 0..n {
                if out[i] == 0 {
                    return kOfxStatErrBadHandle;
                }
                unsafe { *(out[i] as *mut f64) = x.get(i).copied().unwrap_or(0.0) };
            }
        }
        (Class::Str, PV::S(s)) => {
            if out[0] == 0 {
                return kOfxStatErrBadHandle;
            }
            // Valid until the value changes.
            unsafe { *(out[0] as *mut *const c_char) = s.as_ptr() };
        }
        _ => return kOfxStatErrUnsupported,
    }
    kOfxStatOK
}

/// Set `h` from the variadic slots in `a` (ints in the low 32 bits, doubles as their bits, a string pointer).
unsafe fn set_value(h: OfxParamHandle, a: [u64; 4]) -> OfxStatus {
    let Some(p) = (unsafe { param(h) }) else { return kOfxStatErrBadHandle };
    let new = match class(&p.kind) {
        Class::Int(n) => PV::N((0..n).map(|i| a[i] as u32 as i32 as f64).collect()),
        Class::Dbl(n) => PV::N((0..n).map(|i| f64::from_bits(a[i])).collect()),
        Class::Str => {
            let s = a[0] as *const c_char;
            PV::S(if s.is_null() { CString::default() } else { unsafe { CStr::from_ptr(s) }.to_owned() })
        }
        Class::None => return kOfxStatErrUnsupported,
    };
    // shortcut: a plug-in's own change is kept in this host (not written back to the project) and does not call
    // instanceChanged with kOfxChangePluginEdited; do both if a plug-in needs to see its own edits echoed.
    *lock(&p.value) = new;
    kOfxStatOK
}

unsafe extern "C" fn param_get_value(h: OfxParamHandle, a: usize, b: usize, c: usize, d: usize) -> OfxStatus {
    unsafe { get_value(h, [a, b, c, d]) }
}
unsafe extern "C" fn param_get_value_at_time(h: OfxParamHandle, _t: OfxTime, a: usize, b: usize, c: usize, d: usize) -> OfxStatus {
    // shortcut: Milan sends the values at the frame being rendered; other times read the same values. Send the curves if
    // a plug-in reads parameters at other times.
    unsafe { get_value(h, [a, b, c, d]) }
}
unsafe extern "C" fn param_unsupported_t(_h: OfxParamHandle, _t: OfxTime) -> OfxStatus {
    kOfxStatErrUnsupported
}
unsafe extern "C" fn param_unsupported_tt(_h: OfxParamHandle, _t: OfxTime, _u: OfxTime) -> OfxStatus {
    kOfxStatErrUnsupported
}
unsafe extern "C" fn param_set_value(h: OfxParamHandle, a: u64, b: u64, c: u64, d: u64) -> OfxStatus {
    unsafe { set_value(h, [a, b, c, d]) }
}
unsafe extern "C" fn param_set_value_at_time(h: OfxParamHandle, _t: OfxTime, a: u64, b: u64, c: u64, d: u64) -> OfxStatus {
    unsafe { set_value(h, [a, b, c, d]) }
}
unsafe extern "C" fn param_get_num_keys(_h: OfxParamHandle, n: *mut c_uint) -> OfxStatus {
    if !n.is_null() {
        unsafe { *n = 0 };
    }
    kOfxStatOK
}
unsafe extern "C" fn param_get_key_time(_h: OfxParamHandle, _n: c_uint, _t: *mut OfxTime) -> OfxStatus {
    kOfxStatErrBadIndex
}
unsafe extern "C" fn param_get_key_index(_h: OfxParamHandle, _t: OfxTime, _d: c_int, _i: *mut c_int) -> OfxStatus {
    kOfxStatFailed
}
unsafe extern "C" fn param_delete_key(_h: OfxParamHandle, _t: OfxTime) -> OfxStatus {
    kOfxStatErrBadIndex
}
unsafe extern "C" fn param_delete_all_keys(_h: OfxParamHandle) -> OfxStatus {
    kOfxStatOK
}
unsafe extern "C" fn param_copy(to: OfxParamHandle, from: OfxParamHandle, _t: OfxTime, _r: *const OfxRangeD) -> OfxStatus {
    let (Some(to), Some(from)) = (unsafe { param(to) }, unsafe { param(from) }) else { return kOfxStatErrBadHandle };
    if to.kind != from.kind {
        return kOfxStatErrValue;
    }
    if !std::ptr::eq(to, from) {
        let v = lock(&from.value).clone();
        *lock(&to.value) = v;
    }
    kOfxStatOK
}
unsafe extern "C" fn param_edit_begin(_s: OfxParamSetHandle, _n: *const c_char) -> OfxStatus {
    kOfxStatOK
}
unsafe extern "C" fn param_edit_end(_s: OfxParamSetHandle) -> OfxStatus {
    kOfxStatOK
}

type GetFixed = unsafe extern "C" fn(OfxParamHandle, usize, usize, usize, usize) -> OfxStatus;
type GetFixedT = unsafe extern "C" fn(OfxParamHandle, OfxTime, usize, usize, usize, usize) -> OfxStatus;
type SetFixed = unsafe extern "C" fn(OfxParamHandle, u64, u64, u64, u64) -> OfxStatus;
type SetFixedT = unsafe extern "C" fn(OfxParamHandle, OfxTime, u64, u64, u64, u64) -> OfxStatus;
type Var = unsafe extern "C" fn(OfxParamHandle, ...) -> OfxStatus;
type VarT = unsafe extern "C" fn(OfxParamHandle, OfxTime, ...) -> OfxStatus;
type VarTT = unsafe extern "C" fn(OfxParamHandle, OfxTime, OfxTime, ...) -> OfxStatus;

// SAFETY (all transmutes below): function pointers of the same size; the Windows x64 ABI passes a variadic call's
// arguments in the same registers and stack slots as the fixed-arity functions read them (module comment).
pub static PARAMETER_SUITE: OfxParameterSuiteV1 = OfxParameterSuiteV1 {
    paramDefine: param_define,
    paramGetHandle: param_get_handle,
    paramSetGetPropertySet: param_set_get_property_set,
    paramGetPropertySet: param_get_property_set,
    paramGetValue: unsafe { std::mem::transmute::<GetFixed, Var>(param_get_value) },
    paramGetValueAtTime: unsafe { std::mem::transmute::<GetFixedT, VarT>(param_get_value_at_time) },
    paramGetDerivative: unsafe { std::mem::transmute::<unsafe extern "C" fn(OfxParamHandle, OfxTime) -> OfxStatus, VarT>(param_unsupported_t) },
    paramGetIntegral: unsafe { std::mem::transmute::<unsafe extern "C" fn(OfxParamHandle, OfxTime, OfxTime) -> OfxStatus, VarTT>(param_unsupported_tt) },
    paramSetValue: unsafe { std::mem::transmute::<SetFixed, Var>(param_set_value) },
    paramSetValueAtTime: unsafe { std::mem::transmute::<SetFixedT, VarT>(param_set_value_at_time) },
    paramGetNumKeys: param_get_num_keys,
    paramGetKeyTime: param_get_key_time,
    paramGetKeyIndex: param_get_key_index,
    paramDeleteKey: param_delete_key,
    paramDeleteAllKeys: param_delete_all_keys,
    paramCopy: param_copy,
    paramEditBegin: param_edit_begin,
    paramEditEnd: param_edit_end,
};

// ---------------------------------------------------------------- memory

const HEADER: usize = 16;

unsafe extern "C" fn memory_alloc(_h: *mut c_void, n: usize, out: *mut *mut c_void) -> OfxStatus {
    if out.is_null() {
        return kOfxStatErrBadHandle;
    }
    let Some(Ok(layout)) = n.checked_add(HEADER).map(|t| Layout::from_size_align(t, 16)) else { return kOfxStatErrMemory };
    // SAFETY: size >= 16; the size is kept in the header so the free can rebuild the layout.
    let p = unsafe { std::alloc::alloc(layout) };
    if p.is_null() {
        return kOfxStatErrMemory;
    }
    unsafe {
        (p as *mut usize).write(layout.size());
        *out = p.add(HEADER) as *mut c_void;
    }
    kOfxStatOK
}

unsafe extern "C" fn memory_free(p: *mut c_void) -> OfxStatus {
    if p.is_null() {
        return kOfxStatOK;
    }
    unsafe {
        let base = (p as *mut u8).sub(HEADER);
        let size = (base as *const usize).read();
        std::alloc::dealloc(base, Layout::from_size_align_unchecked(size, 16));
    }
    kOfxStatOK
}

pub static MEMORY_SUITE: OfxMemorySuiteV1 = OfxMemorySuiteV1 { memoryAlloc: memory_alloc, memoryFree: memory_free };

// ---------------------------------------------------------------- multithread

thread_local! {
    /// (index, spawned by multiThread)
    static THREAD: Cell<(u32, bool)> = const { Cell::new((0, false)) };
}

unsafe extern "C" fn multi_thread(f: Option<OfxThreadFunctionV1>, n: c_uint, arg: *mut c_void) -> OfxStatus {
    let Some(f) = f else { return kOfxStatErrBadHandle };
    let n = n.clamp(1, 256);
    let arg = arg as usize;
    std::thread::scope(|s| {
        let mut inline = Vec::new();
        for i in 0..n {
            let run = move || {
                THREAD.set((i, true));
                // SAFETY: the plug-in's thread function with its own argument.
                unsafe { f(i, n, arg as *mut c_void) };
            };
            // A thread that cannot start runs on this one after the others started.
            if std::thread::Builder::new().spawn_scoped(s, run).is_err() {
                inline.push(i);
            }
        }
        let was = THREAD.get();
        for i in inline {
            THREAD.set((i, true));
            unsafe { f(i, n, arg as *mut c_void) };
        }
        THREAD.set(was);
    });
    kOfxStatOK
}

unsafe extern "C" fn multi_thread_num_cpus(out: *mut c_uint) -> OfxStatus {
    if !out.is_null() {
        unsafe { *out = std::thread::available_parallelism().map_or(1, |n| n.get()) as c_uint };
    }
    kOfxStatOK
}

unsafe extern "C" fn multi_thread_index(out: *mut c_uint) -> OfxStatus {
    if !out.is_null() {
        unsafe { *out = THREAD.get().0 };
    }
    kOfxStatOK
}

unsafe extern "C" fn multi_thread_is_spawned() -> c_int {
    THREAD.get().1 as c_int
}

/// OFX mutexes are recursive.
#[derive(Default)]
struct RMutex {
    m: Mutex<(Option<ThreadId>, u32)>,
    cv: Condvar,
}

impl RMutex {
    fn try_lock(&self, wait: bool) -> bool {
        let me = std::thread::current().id();
        let mut g = lock(&self.m);
        loop {
            match g.0 {
                None => {
                    *g = (Some(me), 1);
                    return true;
                }
                Some(o) if o == me => {
                    g.1 += 1;
                    return true;
                }
                _ if !wait => return false,
                _ => g = self.cv.wait(g).unwrap_or_else(|p| p.into_inner()),
            }
        }
    }

    fn unlock(&self) -> bool {
        let mut g = lock(&self.m);
        if g.0 != Some(std::thread::current().id()) {
            return false;
        }
        g.1 -= 1;
        if g.1 == 0 {
            g.0 = None;
            self.cv.notify_one();
        }
        true
    }
}

unsafe extern "C" fn mutex_create(out: *mut OfxMutexHandle, count: c_int) -> OfxStatus {
    if out.is_null() {
        return kOfxStatErrBadHandle;
    }
    let m = Box::new(RMutex::default());
    if count > 0 {
        *lock(&m.m) = (Some(std::thread::current().id()), count as u32);
    }
    unsafe { *out = Box::into_raw(m) as OfxMutexHandle };
    kOfxStatOK
}

unsafe extern "C" fn mutex_destroy(m: OfxMutexHandle) -> OfxStatus {
    if m.is_null() {
        return kOfxStatErrBadHandle;
    }
    drop(unsafe { Box::from_raw(m as *mut RMutex) });
    kOfxStatOK
}

unsafe extern "C" fn mutex_lock(m: OfxMutexHandle) -> OfxStatus {
    match unsafe { (m as *const RMutex).as_ref() } {
        Some(m) => {
            m.try_lock(true);
            kOfxStatOK
        }
        None => kOfxStatErrBadHandle,
    }
}

unsafe extern "C" fn mutex_unlock(m: OfxMutexHandle) -> OfxStatus {
    match unsafe { (m as *const RMutex).as_ref() } {
        Some(m) if m.unlock() => kOfxStatOK,
        Some(_) => kOfxStatFailed,
        None => kOfxStatErrBadHandle,
    }
}

unsafe extern "C" fn mutex_try_lock(m: OfxMutexHandle) -> OfxStatus {
    match unsafe { (m as *const RMutex).as_ref() } {
        Some(m) if m.try_lock(false) => kOfxStatOK,
        Some(_) => kOfxStatFailed,
        None => kOfxStatErrBadHandle,
    }
}

pub static MULTITHREAD_SUITE: OfxMultiThreadSuiteV1 = OfxMultiThreadSuiteV1 {
    multiThread: multi_thread,
    multiThreadNumCPUs: multi_thread_num_cpus,
    multiThreadIndex: multi_thread_index,
    multiThreadIsSpawnedThread: multi_thread_is_spawned,
    mutexCreate: mutex_create,
    mutexDestroy: mutex_destroy,
    mutexLock: mutex_lock,
    mutexUnLock: mutex_unlock,
    mutexTryLock: mutex_try_lock,
};

// ---------------------------------------------------------------- messages, progress, time line

/// The plug-ins' last messages (logged, never shown as dialogs); an error reply carries them.
pub static MESSAGES: LazyLock<Mutex<Vec<String>>> = LazyLock::new(Default::default);

pub fn log(s: String) {
    let mut m = lock(&MESSAGES);
    m.push(s);
    let n = m.len();
    if n > 20 {
        m.drain(..n - 20);
    }
}

unsafe fn message_text(kind: *const c_char, id: *const c_char, fmt: *const c_char) -> String {
    // shortcut: the printf format is logged as it is (its arguments are not formatted).
    let t = |p| unsafe { name(p) }.unwrap_or("").to_string();
    let (kind, id, fmt) = (t(kind), t(id), t(fmt));
    format!("{}: {}{}", kind.trim_start_matches("OfxMessage"), if id.is_empty() { String::new() } else { format!("[{id}] ") }, fmt.chars().take(500).collect::<String>())
}

unsafe extern "C" fn message(_h: *mut c_void, kind: *const c_char, id: *const c_char, fmt: *const c_char) -> OfxStatus {
    log(unsafe { message_text(kind, id, fmt) });
    // No dialogs in a background host: a question is answered No.
    if unsafe { name(kind) } == Some("OfxMessageQuestion") { kOfxStatReplyNo } else { kOfxStatOK }
}

unsafe extern "C" fn clear_persistent(_h: *mut c_void) -> OfxStatus {
    kOfxStatOK
}

type MsgFixed = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, *const c_char) -> OfxStatus;
type MsgVar = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, *const c_char, ...) -> OfxStatus;

// SAFETY: as for the parameter suite (four register arguments, the variadic ones are not read).
pub static MESSAGE_SUITE_V1: OfxMessageSuiteV1 = OfxMessageSuiteV1 { message: unsafe { std::mem::transmute::<MsgFixed, MsgVar>(message) } };
pub static MESSAGE_SUITE_V2: OfxMessageSuiteV2 = OfxMessageSuiteV2 {
    message: unsafe { std::mem::transmute::<MsgFixed, MsgVar>(message) },
    setPersistentMessage: unsafe { std::mem::transmute::<MsgFixed, MsgVar>(message) },
    clearPersistentMessage: clear_persistent,
};

unsafe extern "C" fn progress_start(_h: *mut c_void, _l: *const c_char) -> OfxStatus {
    kOfxStatOK
}
unsafe extern "C" fn progress_start2(_h: *mut c_void, _l: *const c_char, _id: *const c_char) -> OfxStatus {
    kOfxStatOK
}
unsafe extern "C" fn progress_update(_h: *mut c_void, _p: f64) -> OfxStatus {
    kOfxStatOK
}
unsafe extern "C" fn progress_end(_h: *mut c_void) -> OfxStatus {
    kOfxStatOK
}

pub static PROGRESS_SUITE_V1: OfxProgressSuiteV1 = OfxProgressSuiteV1 { progressStart: progress_start, progressUpdate: progress_update, progressEnd: progress_end };
pub static PROGRESS_SUITE_V2: OfxProgressSuiteV2 = OfxProgressSuiteV2 { progressStart: progress_start2, progressUpdate: progress_update, progressEnd: progress_end };

/// The time of the frame being rendered (bits of an f64).
pub static NOW: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" fn get_time(_h: *mut c_void, t: *mut f64) -> OfxStatus {
    if !t.is_null() {
        unsafe { *t = f64::from_bits(NOW.load(Ordering::Relaxed)) };
    }
    kOfxStatOK
}
unsafe extern "C" fn goto_time(_h: *mut c_void, _t: f64) -> OfxStatus {
    kOfxStatFailed
}
unsafe extern "C" fn get_time_bounds(_h: *mut c_void, a: *mut f64, b: *mut f64) -> OfxStatus {
    unsafe {
        if !a.is_null() {
            *a = 0.0;
        }
        if !b.is_null() {
            *b = f64::from_bits(NOW.load(Ordering::Relaxed)).max(0.0);
        }
    }
    kOfxStatOK
}

pub static TIMELINE_SUITE: OfxTimeLineSuiteV1 = OfxTimeLineSuiteV1 { getTime: get_time, gotoTime: goto_time, getTimeBounds: get_time_bounds };

// ---------------------------------------------------------------- the host

pub unsafe extern "C" fn fetch_suite(_host: OfxPropertySetHandle, n: *const c_char, v: c_int) -> *const c_void {
    let p = |s: &'static dyn std::any::Any| s as *const dyn std::any::Any as *const c_void;
    match (unsafe { name(n) }.unwrap_or(""), v) {
        ("OfxPropertySuite", 1) => p(&PROPERTY_SUITE),
        ("OfxImageEffectSuite", 1) => p(&IMAGE_EFFECT_SUITE),
        ("OfxParameterSuite", 1) => p(&PARAMETER_SUITE),
        ("OfxMemorySuite", 1) => p(&MEMORY_SUITE),
        ("OfxMultiThreadSuite", 1) => p(&MULTITHREAD_SUITE),
        ("OfxMessageSuite", 1) => p(&MESSAGE_SUITE_V1),
        ("OfxMessageSuite", 2) => p(&MESSAGE_SUITE_V2),
        ("OfxProgressSuite", 1) => p(&PROGRESS_SUITE_V1),
        ("OfxProgressSuite", 2) => p(&PROGRESS_SUITE_V2),
        ("OfxTimeLineSuite", 1) => p(&TIMELINE_SUITE),
        // Interact (overlays), parametric parameters, OpenGL/CUDA/Metal/OpenCL, dialogs: not supported.
        _ => std::ptr::null(),
    }
}

/// The host's own properties: what Milan Pro supports.
pub fn host_props() -> Box<Props> {
    let p = Box::new(Props::default());
    p.s(kOfxPropType, kOfxTypeImageEffectHost);
    p.st(kOfxPropName, "com.meridiannle.milanpro");
    p.st(kOfxPropLabel, "Milan Pro");
    p.set(kOfxPropAPIVersion, V::I(vec![1, 4]));
    let ver: Vec<c_int> = env!("CARGO_PKG_VERSION").split('.').map(|x| x.parse().unwrap_or(0)).collect();
    p.set(kOfxPropVersion, V::I(ver));
    p.st(kOfxPropVersionLabel, env!("CARGO_PKG_VERSION"));
    p.i(kOfxImageEffectHostPropIsBackground, 0);
    p.i(kOfxImageEffectPropSupportsOverlays, 0);
    p.i(kOfxImageEffectPropSupportsMultiResolution, 0);
    p.i(kOfxImageEffectPropSupportsTiles, 0);
    p.i(kOfxImageEffectPropTemporalClipAccess, 0);
    p.set(kOfxImageEffectPropSupportedComponents, V::S(vec![kOfxImageComponentRGBA.to_owned()]));
    p.set(kOfxImageEffectPropSupportedContexts, V::S(vec![kOfxImageEffectContextFilter.to_owned(), kOfxImageEffectContextGeneral.to_owned()]));
    p.set(kOfxImageEffectPropSupportedPixelDepths, V::S(vec![kOfxBitDepthByte.to_owned(), kOfxBitDepthFloat.to_owned()]));
    p.i(kOfxImageEffectPropSupportsMultipleClipDepths, 0);
    p.i(kOfxImageEffectPropSupportsMultipleClipPARs, 0);
    p.i(kOfxImageEffectPropSetableFrameRate, 0);
    p.i(kOfxImageEffectPropSetableFielding, 0);
    p.i(kOfxImageEffectInstancePropSequentialRender, 0);
    p.s(kOfxImageEffectHostPropNativeOrigin, kOfxHostNativeOriginBottomLeft);
    p.i(kOfxParamHostPropSupportsCustomInteract, 0);
    p.i(kOfxParamHostPropSupportsCustomAnimation, 0);
    p.i(kOfxParamHostPropSupportsStringAnimation, 0);
    p.i(kOfxParamHostPropSupportsBooleanAnimation, 0);
    p.i(kOfxParamHostPropSupportsChoiceAnimation, 0);
    p.i(kOfxParamHostPropSupportsParametricAnimation, 0);
    p.i(kOfxParamHostPropMaxParameters, -1);
    p.i(kOfxParamHostPropMaxPages, 0);
    p.set(kOfxParamHostPropPageRowColumnCount, V::I(vec![0, 0]));
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The property suite through its C entry points: types convert, indices grow, strings stay valid, bad input is
    /// refused.
    #[test]
    fn property_suite() {
        let p = Props::default();
        let h = p.handle();
        let s = &PROPERTY_SUITE;
        unsafe {
            assert_eq!((s.propSetInt)(h, c"a".as_ptr(), 1, 7), kOfxStatOK);
            let mut n = 0;
            assert_eq!((s.propGetDimension)(h, c"a".as_ptr(), &mut n), kOfxStatOK);
            assert_eq!(n, 2);
            let mut d = 0.0;
            assert_eq!((s.propGetDouble)(h, c"a".as_ptr(), 1, &mut d), kOfxStatOK);
            assert_eq!(d, 7.0);
            assert_eq!((s.propGetDouble)(h, c"a".as_ptr(), 2, &mut d), kOfxStatErrBadIndex);
            assert_eq!((s.propGetDouble)(h, c"nope".as_ptr(), 0, &mut d), kOfxStatErrUnknown);
            let strs = [c"x".as_ptr(), c"yz".as_ptr()];
            assert_eq!((s.propSetStringN)(h, c"s".as_ptr(), 2, strs.as_ptr()), kOfxStatOK);
            let mut out = [std::ptr::null_mut(); 2];
            assert_eq!((s.propGetStringN)(h, c"s".as_ptr(), 2, out.as_mut_ptr()), kOfxStatOK);
            assert_eq!(CStr::from_ptr(out[1]).to_str().unwrap(), "yz");
            assert_eq!((s.propReset)(h, c"s".as_ptr()), kOfxStatOK);
            assert_eq!((s.propGetDimension)(h, c"s".as_ptr(), &mut n), kOfxStatOK);
            assert_eq!(n, 0);
            assert_eq!((s.propSetInt)(std::ptr::null_mut(), c"a".as_ptr(), 0, 1), kOfxStatErrBadHandle);
        }
    }

    /// The variadic parameter functions, called as a C plug-in calls them.
    #[test]
    fn variadic_parameter_values() {
        let set = ParamSet::default();
        let s = &PARAMETER_SUITE;
        unsafe {
            for (t, n) in [(kOfxParamTypeDouble, c"d"), (kOfxParamTypeRGBA, c"c"), (kOfxParamTypeInteger2D, c"i"), (kOfxParamTypeString, c"s")] {
                assert_eq!((s.paramDefine)(set.handle(), t.as_ptr(), n.as_ptr(), std::ptr::null_mut()), kOfxStatOK);
            }
            assert_eq!((s.paramDefine)(set.handle(), c"OfxParamTypeParametric".as_ptr(), c"p".as_ptr(), std::ptr::null_mut()), kOfxStatErrUnsupported);
            let mut h = std::ptr::null_mut();
            (s.paramGetHandle)(set.handle(), c"c".as_ptr(), &mut h, std::ptr::null_mut());
            assert_eq!((s.paramSetValue)(h, 0.25f64, 0.5f64, 0.75f64, 1.0f64), kOfxStatOK);
            let (mut r, mut g, mut b, mut a) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
            assert_eq!((s.paramGetValueAtTime)(h, 3.0, &mut r as *mut f64, &mut g as *mut f64, &mut b as *mut f64, &mut a as *mut f64), kOfxStatOK);
            assert_eq!((r, g, b, a), (0.25, 0.5, 0.75, 1.0));
            (s.paramGetHandle)(set.handle(), c"i".as_ptr(), &mut h, std::ptr::null_mut());
            assert_eq!((s.paramSetValueAtTime)(h, 1.0, -3 as c_int, 9 as c_int), kOfxStatOK);
            let (mut x, mut y) = (0 as c_int, 0 as c_int);
            assert_eq!((s.paramGetValue)(h, &mut x as *mut c_int, &mut y as *mut c_int), kOfxStatOK);
            assert_eq!((x, y), (-3, 9));
            (s.paramGetHandle)(set.handle(), c"s".as_ptr(), &mut h, std::ptr::null_mut());
            assert_eq!((s.paramSetValue)(h, c"hello".as_ptr()), kOfxStatOK);
            let mut p: *const c_char = std::ptr::null();
            assert_eq!((s.paramGetValue)(h, &mut p as *mut *const c_char), kOfxStatOK);
            assert_eq!(CStr::from_ptr(p).to_str().unwrap(), "hello");
        }
    }

    #[test]
    fn recursive_mutex_and_memory() {
        let s = &MULTITHREAD_SUITE;
        unsafe {
            let mut m = std::ptr::null_mut();
            assert_eq!((s.mutexCreate)(&mut m, 0), kOfxStatOK);
            assert_eq!((s.mutexLock)(m), kOfxStatOK);
            assert_eq!((s.mutexTryLock)(m), kOfxStatOK, "recursive");
            let other = m as usize;
            assert!(std::thread::spawn(move || (MULTITHREAD_SUITE.mutexTryLock)(other as OfxMutexHandle)).join().unwrap() == kOfxStatFailed);
            assert_eq!((s.mutexUnLock)(m), kOfxStatOK);
            assert_eq!((s.mutexUnLock)(m), kOfxStatOK);
            assert_eq!((s.mutexDestroy)(m), kOfxStatOK);
            let mut p = std::ptr::null_mut();
            assert_eq!((MEMORY_SUITE.memoryAlloc)(std::ptr::null_mut(), 1000, &mut p), kOfxStatOK);
            std::ptr::write_bytes(p as *mut u8, 7, 1000);
            assert_eq!((MEMORY_SUITE.memoryFree)(p), kOfxStatOK);
            // Every thread index once.
            static SEEN: Mutex<Vec<u32>> = Mutex::new(Vec::new());
            unsafe extern "C" fn f(i: c_uint, n: c_uint, _: *mut c_void) {
                assert_eq!(n, 4);
                lock(&SEEN).push(i);
            }
            assert_eq!((s.multiThread)(Some(f), 4, std::ptr::null_mut()), kOfxStatOK);
            let mut seen = lock(&SEEN).clone();
            seen.sort();
            assert_eq!(seen, [0, 1, 2, 3]);
        }
    }
}
