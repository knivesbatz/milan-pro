//! Test plug-ins for Milan Pro's OFX host, written against the raw C API as a C plug-in would be:
//! 0 "Test Invert" (8-bit and float, rows split with the multithread suite), 1 "Test Gain" (a double parameter read at
//! the render time), 2 "Test Crash" (an access violation in render), 3 "Test Hang" (render never returns),
//! 4 "Test Dissolve" (Transition context), 5 "Test Solid" (Generator context, 16-bit only), 6 "Test Params" (a gain
//! that edits its own parameters: a starting seed, a linked value, a hidden parameter, push buttons, choice options
//! changed at run time, a value set while rendering, and an error message with printf arguments).
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

const LABELS: [&CStr; 7] = [c"Test Invert", c"Test Gain", c"Test Crash", c"Test Hang", c"Test Dissolve", c"Test Solid", c"Test Params"];
const DISSOLVE: usize = 4;
const SOLID: usize = 5;
const PARAMS_FX: usize = 6;

unsafe fn params() -> &'static OfxParameterSuiteV1 {
    unsafe { &*PARAMS.load(Ordering::Acquire) }
}

/// Parameter `name` of `fx` and its properties.
unsafe fn param(fx: OfxImageEffectHandle, name: &CStr) -> (OfxParamHandle, OfxPropertySetHandle) {
    unsafe {
        let (mut set, mut h, mut p) = (std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
        (effect().getParamSet)(fx, &mut set);
        (params().paramGetHandle)(set, name.as_ptr(), &mut h, &mut p);
        (h, p)
    }
}

unsafe fn get_d(fx: OfxImageEffectHandle, name: &CStr) -> f64 {
    let mut v = 0.0f64;
    unsafe { (params().paramGetValue)(param(fx, name).0, &mut v as *mut f64) };
    v
}

unsafe fn get_i(fx: OfxImageEffectHandle, name: &CStr) -> c_int {
    let mut v: c_int = 0;
    unsafe { (params().paramGetValue)(param(fx, name).0, &mut v as *mut c_int) };
    v
}

/// Define parameter `name` of `kind` under group "grp"; its properties.
unsafe fn define(fx: OfxImageEffectHandle, kind: &CStr, name: &CStr, label: &CStr, parent: bool) -> OfxPropertySetHandle {
    unsafe {
        let (mut set, mut p) = (std::ptr::null_mut(), std::ptr::null_mut());
        (effect().getParamSet)(fx, &mut set);
        (params().paramDefine)(set, kind.as_ptr(), name.as_ptr(), &mut p);
        (props().propSetString)(p, kOfxPropLabel.as_ptr(), 0, label.as_ptr());
        if parent {
            (props().propSetString)(p, kOfxParamPropParent.as_ptr(), 0, c"grp".as_ptr());
        }
        p
    }
}

/// "Test Params": Extra shows only in Advanced mode.
unsafe fn sync_params(fx: OfxImageEffectHandle) {
    unsafe {
        let hidden = (get_i(fx, c"mode") != 1) as c_int;
        (props().propSetInt)(param(fx, c"extra").1, kOfxParamPropSecret.as_ptr(), 0, hidden);
    }
}

unsafe fn create_instance(which: usize, fx: OfxImageEffectHandle) -> OfxStatus {
    if which == PARAMS_FX {
        unsafe {
            // A new instance gets a seed, as a plug-in with a random grain seed does.
            if get_i(fx, c"seed") == 0 {
                (params().paramSetValue)(param(fx, c"seed").0, 123_456_789 as c_int);
            }
            sync_params(fx);
        }
    }
    kOfxStatOK
}

unsafe fn instance_changed(which: usize, fx: OfxImageEffectHandle, args: OfxPropertySetHandle) -> OfxStatus {
    if which != PARAMS_FX {
        return kOfxStatReplyDefault;
    }
    unsafe {
        let (mut name, mut reason) = (std::ptr::null_mut(), std::ptr::null_mut());
        (props().propGetString)(args, kOfxPropName.as_ptr(), 0, &mut name);
        (props().propGetString)(args, kOfxPropChangeReason.as_ptr(), 0, &mut reason);
        if name.is_null() || reason.is_null() || CStr::from_ptr(reason) == kOfxChangePluginEdited {
            return kOfxStatReplyDefault;
        }
        let messages = (*HOST.load(Ordering::Acquire)).fetchSuite;
        let messages = messages((*HOST.load(Ordering::Acquire)).host, kOfxMessageSuite.as_ptr(), 1) as *const OfxMessageSuiteV1;
        match CStr::from_ptr(name).to_bytes() {
            b"mode" => sync_params(fx),
            b"amount" => {
                (params().paramSetValue)(param(fx, c"linked").0, get_d(fx, c"amount") * 2.0);
            }
            b"bump" => {
                let v = get_d(fx, c"amount") + 1.0;
                (params().paramSetValue)(param(fx, c"amount").0, v);
                ((*messages).message)(fx, kOfxMessageMessage.as_ptr(), c"bump".as_ptr(), c"%s to %d".as_ptr(), c"bumped".as_ptr(), v as c_int);
            }
            b"refresh" => {
                let (h, p) = param(fx, c"list");
                (props().propReset)(p, kOfxParamPropChoiceOption.as_ptr());
                let options = [c"One".as_ptr(), c"Two".as_ptr(), c"Three".as_ptr()];
                (props().propSetStringN)(p, kOfxParamPropChoiceOption.as_ptr(), 3, options.as_ptr());
                (props().propSetString)(p, kOfxPropLabel.as_ptr(), 0, c"List (3)".as_ptr());
                (props().propSetInt)(param(fx, c"note").1, kOfxParamPropEnabled.as_ptr(), 0, 0);
                (params().paramSetValue)(h, 2 as c_int);
            }
            b"broken" => {
                ((*messages).message)(fx, kOfxMessageError.as_ptr(), c"broken".as_ptr(), c"%s".as_ptr(), c"Could not do that.".as_ptr());
                return kOfxStatFailed;
            }
            _ => return kOfxStatReplyDefault,
        }
    }
    kOfxStatOK
}

unsafe fn describe(which: usize, fx: OfxImageEffectHandle) -> OfxStatus {
    unsafe {
        let mut p = std::ptr::null_mut();
        (effect().getPropertySet)(fx, &mut p);
        (props().propSetString)(p, kOfxPropLabel.as_ptr(), 0, LABELS[which].as_ptr());
        (props().propSetString)(p, kOfxImageEffectPluginPropGrouping.as_ptr(), 0, c"Milan Test".as_ptr());
        (props().propSetString)(p, kOfxPropPluginDescription.as_ptr(), 0, c"A test plug-in".as_ptr());
        let context = match which {
            DISSOLVE => kOfxImageEffectContextTransition,
            SOLID => kOfxImageEffectContextGenerator,
            _ => kOfxImageEffectContextFilter,
        };
        (props().propSetString)(p, kOfxImageEffectPropSupportedContexts.as_ptr(), 0, context.as_ptr());
        if which == SOLID {
            (props().propSetString)(p, kOfxImageEffectPropSupportedPixelDepths.as_ptr(), 0, kOfxBitDepthShort.as_ptr());
        } else {
            (props().propSetString)(p, kOfxImageEffectPropSupportedPixelDepths.as_ptr(), 0, kOfxBitDepthByte.as_ptr());
            (props().propSetString)(p, kOfxImageEffectPropSupportedPixelDepths.as_ptr(), 1, kOfxBitDepthFloat.as_ptr());
        }
    }
    kOfxStatOK
}

unsafe fn describe_in_context(which: usize, fx: OfxImageEffectHandle) -> OfxStatus {
    unsafe {
        let clips: &[&CStr] = match which {
            DISSOLVE => &[kOfxImageEffectTransitionSourceFromClipName, kOfxImageEffectTransitionSourceToClipName, kOfxImageEffectOutputClipName],
            SOLID => &[kOfxImageEffectOutputClipName],
            _ => &[kOfxImageEffectSimpleSourceClipName, kOfxImageEffectOutputClipName],
        };
        for clip in clips {
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
        if which == DISSOLVE {
            let p = define(fx, kOfxParamTypeDouble, kOfxImageEffectTransitionParamName, c"Transition", false);
            (props().propSetDouble)(p, kOfxParamPropDisplayMax.as_ptr(), 0, 1.0);
            let p = define(fx, kOfxParamTypeBoolean, c"swap", c"Swap", false);
            (props().propSetInt)(p, kOfxParamPropDefault.as_ptr(), 0, 0);
        }
        if which == SOLID {
            let p = define(fx, kOfxParamTypeRGBA, c"color", c"Colour", false);
            for (i, v) in [1.0, 0.5, 0.25, 1.0].into_iter().enumerate() {
                (props().propSetDouble)(p, kOfxParamPropDefault.as_ptr(), i as c_int, v);
            }
        }
        if which == PARAMS_FX {
            let p = define(fx, kOfxParamTypeGroup, c"grp", c"Controls", false);
            (props().propSetInt)(p, kOfxParamPropGroupOpen.as_ptr(), 0, 1);
            let p = define(fx, kOfxParamTypeChoice, c"mode", c"Mode", true);
            for (i, o) in [c"Simple", c"Advanced", c"Fail"].into_iter().enumerate() {
                (props().propSetString)(p, kOfxParamPropChoiceOption.as_ptr(), i as c_int, o.as_ptr());
            }
            let p = define(fx, kOfxParamTypeDouble, c"amount", c"Amount", true);
            (props().propSetDouble)(p, kOfxParamPropDefault.as_ptr(), 0, 1.0);
            (props().propSetDouble)(p, kOfxParamPropDisplayMin.as_ptr(), 0, 0.0);
            (props().propSetDouble)(p, kOfxParamPropDisplayMax.as_ptr(), 0, 4.0);
            let p = define(fx, kOfxParamTypeDouble, c"extra", c"Extra", true);
            (props().propSetInt)(p, kOfxParamPropSecret.as_ptr(), 0, 1);
            let p = define(fx, kOfxParamTypeDouble, c"linked", c"Linked", true);
            (props().propSetDouble)(p, kOfxParamPropDefault.as_ptr(), 0, 2.0);
            define(fx, kOfxParamTypeInteger, c"seed", c"Seed", false);
            let p = define(fx, kOfxParamTypeChoice, c"list", c"List", false);
            (props().propSetString)(p, kOfxParamPropChoiceOption.as_ptr(), 0, c"One".as_ptr());
            let p = define(fx, kOfxParamTypeString, c"note", c"Note", false);
            (props().propSetString)(p, kOfxParamPropDefault.as_ptr(), 0, c"hi".as_ptr());
            let p = define(fx, kOfxParamTypeDouble, c"sawScale", c"Render scale seen", false);
            (props().propSetInt)(p, kOfxParamPropSecret.as_ptr(), 0, 1);
            define(fx, kOfxParamTypePushButton, c"bump", c"Bump Amount", false);
            define(fx, kOfxParamTypePushButton, c"refresh", c"Refresh List", false);
            define(fx, kOfxParamTypePushButton, c"broken", c"Broken", false);
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
        let (mut data, mut row, mut b, mut depth) = (std::ptr::null_mut(), 0, [0 as c_int; 4], std::ptr::null_mut::<c_char>());
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
        if which == DISSOLVE {
            return dissolve(fx, t);
        }
        if which == SOLID {
            return solid(fx, t);
        }
        if which == PARAMS_FX {
            let mut scale = 0.0;
            (props().propGetDouble)(args, kOfxImageEffectPropRenderScale.as_ptr(), 0, &mut scale);
            (params().paramSetValue)(param(fx, c"sawScale").0, scale);
            if get_i(fx, c"mode") == 2 {
                let fetch = (*HOST.load(Ordering::Acquire)).fetchSuite;
                let messages = fetch((*HOST.load(Ordering::Acquire)).host, kOfxMessageSuite.as_ptr(), 1) as *const OfxMessageSuiteV1;
                ((*messages).message)(fx, kOfxMessageError.as_ptr(), c"renderer".as_ptr(), c"%s (%d)".as_ptr(), c"No Vulkan physical device is available.".as_ptr(), 7 as c_int);
                return kOfxStatFailed;
            }
        }
        let gain = if which == PARAMS_FX { Some(get_d(fx, c"amount")) } else { None };
        let gain = gain.or_else(|| (which == 1).then(|| {
            let (mut set, mut p, mut v) = (std::ptr::null_mut(), std::ptr::null_mut(), 1.0f64);
            (effect().getParamSet)(fx, &mut set);
            let ps = &*PARAMS.load(Ordering::Acquire);
            (ps.paramGetHandle)(set, c"gain".as_ptr(), &mut p, std::ptr::null_mut());
            (ps.paramGetValueAtTime)(p, t, &mut v as *mut f64);
            v
        }));
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

/// SourceFrom to SourceTo by the Transition parameter (Swap: the other way), 8-bit or float.
unsafe fn dissolve(fx: OfxImageEffectHandle, t: f64) -> OfxStatus {
    unsafe {
        let mut p = 0.0f64;
        (params().paramGetValueAtTime)(param(fx, kOfxImageEffectTransitionParamName).0, t, &mut p as *mut f64);
        if get_i(fx, c"swap") != 0 {
            p = 1.0 - p;
        }
        let imgs = [kOfxImageEffectTransitionSourceFromClipName, kOfxImageEffectTransitionSourceToClipName, kOfxImageEffectOutputClipName].map(|n| image(fx, n, t));
        let ok = if let [Some(a), Some(b), Some(d)] = &imgs {
            let n = d.w * 4;
            for y in 0..d.h {
                let row = |i: &Img| (i.data as isize + y as isize * i.row) as usize;
                if d.float {
                    let (a, b, o) = (row(a) as *const f32, row(b) as *const f32, row(d) as *mut f32);
                    for k in 0..n {
                        *o.add(k) = *a.add(k) * (1.0 - p as f32) + *b.add(k) * p as f32;
                    }
                } else {
                    let (a, b, o) = (row(a) as *const u8, row(b) as *const u8, row(d) as *mut u8);
                    for k in 0..n {
                        *o.add(k) = (*a.add(k) as f64 * (1.0 - p) + *b.add(k) as f64 * p).round() as u8;
                    }
                }
            }
            true
        } else {
            false
        };
        for i in imgs.into_iter().flatten() {
            (effect().clipReleaseImage)(i.handle);
        }
        if ok { kOfxStatOK } else { kOfxStatFailed }
    }
}

/// The colour parameter over the whole output, premultiplied, 16 bits a channel.
unsafe fn solid(fx: OfxImageEffectHandle, t: f64) -> OfxStatus {
    unsafe {
        let mut c = [0.0f64; 4];
        (params().paramGetValueAtTime)(param(fx, c"color").0, t, &mut c[0] as *mut f64, &mut c[1] as *mut f64, &mut c[2] as *mut f64, &mut c[3] as *mut f64);
        let Some(d) = image(fx, kOfxImageEffectOutputClipName, t) else { return kOfxStatFailed };
        let px = [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]].map(|v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16);
        for y in 0..d.h {
            let o = (d.data as isize + y as isize * d.row) as *mut u16;
            for x in 0..d.w {
                for (k, v) in px.iter().enumerate() {
                    *o.add(x * 4 + k) = *v;
                }
            }
        }
        (effect().clipReleaseImage)(d.handle);
        kOfxStatOK
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
        } else if a == kOfxActionCreateInstance {
            create_instance(which, fx)
        } else if a == kOfxActionInstanceChanged {
            instance_changed(which, fx, args)
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
entries!(entry0 = 0, entry1 = 1, entry2 = 2, entry3 = 3, entry4 = 4, entry5 = 5, entry6 = 6);

struct Plugins([OfxPlugin; 7]);
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
    plugin(c"com.meridiannle.test.dissolve", entry4),
    plugin(c"com.meridiannle.test.solid", entry5),
    plugin(c"com.meridiannle.test.params", entry6),
]);

#[unsafe(no_mangle)]
pub extern "C" fn OfxGetNumberOfPlugins() -> c_int {
    PLUGINS.0.len() as c_int
}

#[unsafe(no_mangle)]
pub extern "C" fn OfxGetPlugin(n: c_int) -> *const OfxPlugin {
    PLUGINS.0.get(n as usize).map_or(std::ptr::null(), |p| p as *const OfxPlugin)
}
