//! Test plug-ins for Milan Pro's OFX host, written against the raw C API as a C plug-in would be:
//! 0 "Test Invert" (8-bit and float, rows split with the multithread suite), 1 "Test Gain" (a double parameter read at
//! the render time), 2 "Test Crash" (an access violation in render), 3 "Test Hang" (render never returns).
#![allow(non_upper_case_globals, clippy::missing_safety_doc)]
#[path = "../../../src/bin/milan-plugin-host/ofx.rs"]
mod ofx;

use ofx::*;
use std::ffi::{CStr, c_char, c_int, c_uint, c_void};
use std::sync::atomic::{AtomicPtr, Ordering};

static HOST: AtomicPtr<OfxHost> = AtomicPtr::new(std::ptr::null_mut());
static PROPS: AtomicPtr<OfxPropertySuiteV1> = AtomicPtr::new(std::ptr::null_mut());
static EFFECT: AtomicPtr<OfxImageEffectSuiteV1> = AtomicPtr::new(std::ptr::null_mut());
static PARAMS: AtomicPtr<OfxParameterSuiteV1> = AtomicPtr::new(std::ptr::null_mut());
static THREADS: AtomicPtr<OfxMultiThreadSuiteV1> = AtomicPtr::new(std::ptr::null_mut());

unsafe fn props() -> &'static OfxPropertySuiteV1 {
    unsafe { &*PROPS.load(Ordering::Acquire) }
}
unsafe fn effect() -> &'static OfxImageEffectSuiteV1 {
    unsafe { &*EFFECT.load(Ordering::Acquire) }
}

unsafe extern "C" fn set_host(h: *mut OfxHost) {
    HOST.store(h, Ordering::Release);
}

unsafe fn load() -> OfxStatus {
    let h = HOST.load(Ordering::Acquire);
    if h.is_null() {
        return kOfxStatErrMissingHostFeature;
    }
    let h = unsafe { &*h };
    let fetch = |n: &CStr| unsafe { (h.fetchSuite)(h.host, n.as_ptr(), 1) } as *mut c_void;
    PROPS.store(fetch(kOfxPropertySuite) as _, Ordering::Release);
    EFFECT.store(fetch(kOfxImageEffectSuite) as _, Ordering::Release);
    PARAMS.store(fetch(kOfxParameterSuite) as _, Ordering::Release);
    THREADS.store(fetch(kOfxMultiThreadSuite) as _, Ordering::Release);
    if PROPS.load(Ordering::Acquire).is_null() || EFFECT.load(Ordering::Acquire).is_null() || PARAMS.load(Ordering::Acquire).is_null() {
        return kOfxStatErrMissingHostFeature;
    }
    kOfxStatOK
}

const LABELS: [&CStr; 4] = [c"Test Invert", c"Test Gain", c"Test Crash", c"Test Hang"];

unsafe fn describe(which: usize, fx: OfxImageEffectHandle) -> OfxStatus {
    unsafe {
        let mut p = std::ptr::null_mut();
        (effect().getPropertySet)(fx, &mut p);
        (props().propSetString)(p, kOfxPropLabel.as_ptr(), 0, LABELS[which].as_ptr());
        (props().propSetString)(p, kOfxImageEffectPluginPropGrouping.as_ptr(), 0, c"Milan Test".as_ptr());
        (props().propSetString)(p, kOfxPropPluginDescription.as_ptr(), 0, c"A test plug-in".as_ptr());
        (props().propSetString)(p, kOfxImageEffectPropSupportedContexts.as_ptr(), 0, kOfxImageEffectContextFilter.as_ptr());
        (props().propSetString)(p, kOfxImageEffectPropSupportedPixelDepths.as_ptr(), 0, kOfxBitDepthByte.as_ptr());
        (props().propSetString)(p, kOfxImageEffectPropSupportedPixelDepths.as_ptr(), 1, kOfxBitDepthFloat.as_ptr());
    }
    kOfxStatOK
}

unsafe fn describe_in_context(which: usize, fx: OfxImageEffectHandle) -> OfxStatus {
    unsafe {
        for clip in [kOfxImageEffectSimpleSourceClipName, kOfxImageEffectOutputClipName] {
            let mut p = std::ptr::null_mut();
            (effect().clipDefine)(fx, clip.as_ptr(), &mut p);
            (props().propSetString)(p, kOfxImageEffectPropSupportedComponents.as_ptr(), 0, kOfxImageComponentRGBA.as_ptr());
        }
        if which == 1 {
            let (mut set, mut p) = (std::ptr::null_mut(), std::ptr::null_mut());
            (effect().getParamSet)(fx, &mut set);
            ((*PARAMS.load(Ordering::Acquire)).paramDefine)(set, kOfxParamTypeDouble.as_ptr(), c"gain".as_ptr(), &mut p);
            (props().propSetString)(p, kOfxPropLabel.as_ptr(), 0, c"Gain".as_ptr());
            (props().propSetDouble)(p, kOfxParamPropDefault.as_ptr(), 0, 1.0);
            (props().propSetDouble)(p, kOfxParamPropDisplayMin.as_ptr(), 0, 0.0);
            (props().propSetDouble)(p, kOfxParamPropDisplayMax.as_ptr(), 0, 4.0);
        }
    }
    kOfxStatOK
}

/// One image's pixels.
struct Img {
    data: usize,
    row: isize,
    w: usize,
    h: usize,
    float: bool,
    handle: OfxPropertySetHandle,
}

unsafe fn image(fx: OfxImageEffectHandle, name: &CStr, t: f64) -> Option<Img> {
    unsafe {
        let (mut clip, mut img) = (std::ptr::null_mut(), std::ptr::null_mut());
        if (effect().clipGetHandle)(fx, name.as_ptr(), &mut clip, std::ptr::null_mut()) != kOfxStatOK || (effect().clipGetImage)(clip, t, std::ptr::null(), &mut img) != kOfxStatOK {
            return None;
        }
        let (mut data, mut row, mut b, mut depth) = (std::ptr::null_mut(), 0, [0 as c_int; 4], std::ptr::null_mut());
        (props().propGetPointer)(img, kOfxImagePropData.as_ptr(), 0, &mut data);
        (props().propGetInt)(img, kOfxImagePropRowBytes.as_ptr(), 0, &mut row);
        (props().propGetIntN)(img, kOfxImagePropBounds.as_ptr(), 4, b.as_mut_ptr());
        (props().propGetString)(img, kOfxImageEffectPropPixelDepth.as_ptr(), 0, &mut depth);
        let float = CStr::from_ptr(depth) == kOfxBitDepthFloat;
        Some(Img { data: data as usize, row: row as isize, w: (b[2] - b[0]) as usize, h: (b[3] - b[1]) as usize, float, handle: img })
    }
}

struct Job {
    src: *const Img,
    dst: *const Img,
    gain: Option<f64>,
}

/// Rows `i` of `n` of the job: invert (gain None) or gain, premultiplied (the colour never exceeds alpha in 8 bits).
unsafe extern "C" fn rows(i: c_uint, n: c_uint, arg: *mut c_void) {
    unsafe {
        let j = &*(arg as *const Job);
        let (s, d) = (&*j.src, &*j.dst);
        let (h, n, i) = (s.h, n as usize, i as usize);
        for y in (h * i / n)..(h * (i + 1) / n) {
            for x in 0..s.w {
                if s.float {
                    let sp = (s.data as isize + y as isize * s.row) as *const f32;
                    let dp = (d.data as isize + y as isize * d.row) as *mut f32;
                    let a = *sp.add(x * 4 + 3);
                    for c in 0..3 {
                        let v = *sp.add(x * 4 + c);
                        *dp.add(x * 4 + c) = match j.gain {
                            None => a - v,
                            Some(g) => v * g as f32,
                        };
                    }
                    *dp.add(x * 4 + 3) = a;
                } else {
                    let sp = (s.data as isize + y as isize * s.row) as *const u8;
                    let dp = (d.data as isize + y as isize * d.row) as *mut u8;
                    let a = *sp.add(x * 4 + 3);
                    for c in 0..3 {
                        let v = *sp.add(x * 4 + c);
                        *dp.add(x * 4 + c) = match j.gain {
                            None => a.saturating_sub(v),
                            Some(g) => ((v as f64 * g).round().min(a as f64)) as u8,
                        };
                    }
                    *dp.add(x * 4 + 3) = a;
                }
            }
        }
    }
}

unsafe fn render(which: usize, fx: OfxImageEffectHandle, args: OfxPropertySetHandle) -> OfxStatus {
    match which {
        2 => unsafe { std::ptr::null_mut::<u32>().write_volatile(1) },
        3 => loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        },
        _ => {}
    }
    unsafe {
        let mut t = 0.0;
        (props().propGetDouble)(args, kOfxPropTime.as_ptr(), 0, &mut t);
        let gain = (which == 1).then(|| {
            let (mut set, mut p, mut v) = (std::ptr::null_mut(), std::ptr::null_mut(), 1.0f64);
            (effect().getParamSet)(fx, &mut set);
            let ps = &*PARAMS.load(Ordering::Acquire);
            (ps.paramGetHandle)(set, c"gain".as_ptr(), &mut p, std::ptr::null_mut());
            (ps.paramGetValueAtTime)(p, t, &mut v as *mut f64);
            v
        });
        let (Some(src), Some(dst)) = (image(fx, kOfxImageEffectSimpleSourceClipName, t), image(fx, kOfxImageEffectOutputClipName, t)) else { return kOfxStatFailed };
        let ok = src.w == dst.w && src.h == dst.h && src.float == dst.float;
        if ok {
            let job = Job { src: &src, dst: &dst, gain };
            let mt = THREADS.load(Ordering::Acquire);
            if mt.is_null() {
                rows(0, 1, &job as *const Job as *mut c_void);
            } else {
                ((*mt).multiThread)(Some(rows), 4, &job as *const Job as *mut c_void);
            }
        }
        (effect().clipReleaseImage)(src.handle);
        (effect().clipReleaseImage)(dst.handle);
        if ok { kOfxStatOK } else { kOfxStatErrFormat }
    }
}

unsafe fn main_entry(which: usize, action: *const c_char, handle: *const c_void, args: OfxPropertySetHandle) -> OfxStatus {
    let a = unsafe { CStr::from_ptr(action) };
    let fx = handle as OfxImageEffectHandle;
    unsafe {
        if a == kOfxActionLoad {
            load()
        } else if a == kOfxActionDescribe {
            describe(which, fx)
        } else if a == kOfxImageEffectActionDescribeInContext {
            describe_in_context(which, fx)
        } else if a == kOfxImageEffectActionRender {
            render(which, fx, args)
        } else {
            kOfxStatReplyDefault
        }
    }
}

macro_rules! entries {
    ($($f:ident = $i:literal),*) => {
        $(unsafe extern "C" fn $f(a: *const c_char, h: *const c_void, i: OfxPropertySetHandle, _o: OfxPropertySetHandle) -> OfxStatus {
            unsafe { main_entry($i, a, h, i) }
        })*
    };
}
entries!(entry0 = 0, entry1 = 1, entry2 = 2, entry3 = 3);

struct Plugins([OfxPlugin; 4]);
// SAFETY: read-only after construction; the pointers are to static strings.
unsafe impl Sync for Plugins {}

const fn plugin(id: &'static CStr, entry: OfxPluginEntryPoint) -> OfxPlugin {
    OfxPlugin { pluginApi: kOfxImageEffectPluginApi.as_ptr(), apiVersion: 1, pluginIdentifier: id.as_ptr(), pluginVersionMajor: 1, pluginVersionMinor: 0, setHost: Some(set_host), mainEntry: Some(entry) }
}

static PLUGINS: Plugins = Plugins([
    plugin(c"com.meridiannle.test.invert", entry0),
    plugin(c"com.meridiannle.test.gain", entry1),
    plugin(c"com.meridiannle.test.crash", entry2),
    plugin(c"com.meridiannle.test.hang", entry3),
]);

#[unsafe(no_mangle)]
pub extern "C" fn OfxGetNumberOfPlugins() -> c_int {
    4
}

#[unsafe(no_mangle)]
pub extern "C" fn OfxGetPlugin(n: c_int) -> *const OfxPlugin {
    PLUGINS.0.get(n as usize).map_or(std::ptr::null(), |p| p as *const OfxPlugin)
}
