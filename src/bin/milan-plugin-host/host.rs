//! One loaded OFX binary: its plug-ins' load, describe and describeInContext actions, and their instances with the
//! render action sequence (instance changed, begin sequence, region of definition and interest, identity, render).
use std::collections::HashMap;
use std::ffi::{CStr, c_int};
use std::path::{Path, PathBuf};
use crate::ofx::*;
use crate::proto::{PVal, ParamDesc, PluginDesc, Render, Reply};
use crate::suites::{self, Class, Effect, Frame, PV, Props, V, class, cstring, lock};

/// At most this many plug-in instances live in one host; the least recently used goes first.
const MAX_INSTANCES: usize = 16;

struct Instance {
    effect: Box<Effect>,
    /// The values Milan last sent: only a change from these is applied (a plug-in's own edits stay otherwise).
    sent: HashMap<String, PVal>,
    began: bool,
    used: u64,
}

pub struct Host {
    host: *mut OfxHost,
    lib: Option<(PathBuf, libloading::Library)>,
    plugins: Vec<*const OfxPlugin>,
    /// Per plug-in index: load action passed (Ok) or why not.
    loaded: Vec<Result<(), String>>,
    /// Per plug-in index: the describe and the context descriptor, kept for its instances.
    descs: HashMap<u32, (Box<Effect>, Box<Effect>, PluginDesc)>,
    instances: HashMap<(String, u32, bool), Instance>,
    shm: Option<Shm>,
    clock: u64,
}

impl Host {
    pub fn new() -> Self {
        let props: &'static Props = Box::leak(suites::host_props());
        let host = Box::into_raw(Box::new(OfxHost { host: props.handle(), fetchSuite: suites::fetch_suite }));
        Self { host, lib: None, plugins: Vec::new(), loaded: Vec::new(), descs: HashMap::new(), instances: HashMap::new(), shm: None, clock: 0 }
    }

    fn entry(&self, index: u32) -> Option<OfxPluginEntryPoint> {
        // SAFETY: pointers OfxGetPlugin returned; the library stays loaded while `self` lives.
        self.plugins.get(index as usize).and_then(|p| unsafe { p.as_ref() }).and_then(|p| p.mainEntry)
    }

    fn call(&self, index: u32, action: &CStr, handle: *const std::ffi::c_void, inp: Option<&Props>, out: Option<&Props>) -> OfxStatus {
        let Some(f) = self.entry(index) else { return kOfxStatErrBadHandle };
        let h = |p: Option<&Props>| p.map_or(std::ptr::null_mut(), Props::handle);
        // SAFETY: the plug-in's entry point with handles that live across the call.
        unsafe { f(action.as_ptr(), handle, h(inp), h(out)) }
    }

    pub fn load(&mut self, binary: &Path) -> Reply {
        if let Some((p, _)) = &self.lib {
            return if p == binary { Reply::Loaded(self.plugins.len() as u32) } else { Reply::Error("this host has another binary loaded".into()) };
        }
        // SAFETY: loading the plug-in runs its initialisers; that is what this isolated process is for.
        let lib = match unsafe { libloading::Library::new(binary) } {
            Ok(l) => l,
            Err(e) => return Reply::Error(format!("cannot load {}: {e}", binary.display())),
        };
        unsafe {
            // OFX 1.5: a binary may take the host before it lists its plug-ins.
            if let Ok(set) = lib.get::<unsafe extern "C" fn(*const OfxHost) -> OfxStatus>(b"OfxSetHost\0") {
                set(self.host);
            }
            let count = match lib.get::<unsafe extern "C" fn() -> c_int>(b"OfxGetNumberOfPlugins\0") {
                Ok(f) => f(),
                Err(_) => return Reply::Error("not an OFX plug-in: OfxGetNumberOfPlugins is missing".into()),
            };
            let Ok(get) = lib.get::<unsafe extern "C" fn(c_int) -> *mut OfxPlugin>(b"OfxGetPlugin\0") else {
                return Reply::Error("not an OFX plug-in: OfxGetPlugin is missing".into());
            };
            let get = *get;
            self.plugins = (0..count.clamp(0, 10_000)).map(|i| get(i) as *const OfxPlugin).collect();
        }
        self.lib = Some((binary.to_path_buf(), lib));
        let loaded = (0..self.plugins.len() as u32).map(|i| self.load_one(i)).collect();
        self.loaded = loaded;
        Reply::Loaded(self.plugins.len() as u32)
    }

    fn load_one(&self, i: u32) -> Result<(), String> {
        // SAFETY: as in `entry`.
        let Some(p) = (unsafe { self.plugins[i as usize].as_ref() }) else {
            return Err("OfxGetPlugin returned nothing".into());
        };
        let api = if p.pluginApi.is_null() { String::new() } else { unsafe { CStr::from_ptr(p.pluginApi) }.to_string_lossy().into_owned() };
        if api != "OfxImageEffectPluginAPI" || p.apiVersion != 1 {
            return Err(format!("not an image effect (API {api} version {})", p.apiVersion));
        }
        if let Some(set) = p.setHost {
            unsafe { set(self.host) };
        }
        match self.call(i, kOfxActionLoad, std::ptr::null(), None, None) {
            kOfxStatOK | kOfxStatReplyDefault => Ok(()),
            s => Err(format!("its load action failed ({}){}", status(s), messages())),
        }
    }

    pub fn describe(&mut self, index: u32) -> Reply {
        match self.describe_inner(index) {
            Ok(d) => Reply::Described(Box::new(d)),
            Err(e) => Reply::Error(e),
        }
    }

    fn describe_inner(&mut self, index: u32) -> Result<PluginDesc, String> {
        if let Some(d) = self.descs.get(&index) {
            return Ok(d.2.clone());
        }
        match self.loaded.get(index as usize) {
            Some(Ok(())) => {}
            Some(Err(e)) => return Err(e.clone()),
            None => return Err(format!("no plug-in {index} in this binary")),
        }
        // SAFETY: checked by load_one.
        let p = unsafe { &*self.plugins[index as usize] };
        if p.pluginIdentifier.is_null() {
            return Err("the plug-in has no identifier".into());
        }
        let id = unsafe { CStr::from_ptr(p.pluginIdentifier) }.to_string_lossy().into_owned();
        let props = Box::new(Props::default());
        props.s(kOfxPropType, kOfxTypeImageEffect);
        props.st(kOfxPropLabel, &id);
        props.st(kOfxImageEffectPluginPropGrouping, "");
        props.set(kOfxImageEffectPropSupportedContexts, V::S(Vec::new()));
        props.set(kOfxImageEffectPropSupportedPixelDepths, V::S(Vec::new()));
        props.i(kOfxImageEffectPropSupportsMultiResolution, 1);
        props.i(kOfxImageEffectPropSupportsTiles, 1);
        props.i(kOfxImageEffectPropTemporalClipAccess, 0);
        props.i(kOfxImageEffectPropSupportsMultipleClipDepths, 0);
        props.i(kOfxImageEffectPropSupportsMultipleClipPARs, 0);
        // The bundle folder (…\Name.ofx.bundle) the binary sits in.
        if let Some(bundle) = self.lib.as_ref().and_then(|l| l.0.ancestors().nth(3)) {
            props.st(kOfxPluginPropFilePath, &bundle.display().to_string());
        }
        let desc = Effect::new(props);
        match self.call(index, kOfxActionDescribe, desc.handle(), None, None) {
            kOfxStatOK | kOfxStatReplyDefault => {}
            s => return Err(format!("its describe action failed ({}){}", status(s), messages())),
        }
        let contexts = desc.props.get_s(kOfxImageEffectPropSupportedContexts);
        let context = ["OfxImageEffectContextFilter", "OfxImageEffectContextGeneral"].into_iter().find(|c| contexts.iter().any(|x| x == c)).unwrap_or("");
        let mut out = PluginDesc {
            index,
            id,
            major: p.pluginVersionMajor,
            minor: p.pluginVersionMinor,
            label: desc.props.s0(kOfxPropLabel),
            group: desc.props.s0(kOfxImageEffectPluginPropGrouping),
            description: desc.props.s0(kOfxPropPluginDescription),
            contexts,
            depths: desc.props.get_s(kOfxImageEffectPropSupportedPixelDepths),
            context: context.into(),
            params: Vec::new(),
        };
        if context.is_empty() {
            return Ok(out);
        }
        let ctx = Effect::new(desc.props.copy());
        let inp = Props::default();
        inp.st(kOfxImageEffectPropContext, context);
        match self.call(index, kOfxImageEffectActionDescribeInContext, ctx.handle(), Some(&inp), None) {
            kOfxStatOK | kOfxStatReplyDefault => {}
            s => return Err(format!("its describeInContext action ({context}) failed ({}){}", status(s), messages())),
        }
        // describeInContext may set the depths too.
        let depths = ctx.props.get_s(kOfxImageEffectPropSupportedPixelDepths);
        if !depths.is_empty() {
            out.depths = depths;
        }
        out.params = lock(&ctx.params.params).iter().map(|p| param_desc(&p.name, &p.kind, &p.props)).collect();
        self.descs.insert(index, (desc, ctx, out.clone()));
        Ok(out)
    }

    pub fn render(&mut self, r: &Render) -> Reply {
        match self.render_inner(r) {
            Ok(()) => Reply::Rendered,
            Err(e) => Reply::Error(e),
        }
    }

    fn render_inner(&mut self, r: &Render) -> Result<(), String> {
        let px = r.w as u64 * r.h as u64 * if r.float { 16 } else { 4 };
        if r.w == 0 || r.h == 0 || px > r.out || r.out.checked_add(px).is_none_or(|end| end > r.shm_size) {
            return Err("bad frame size".into());
        }
        if self.shm.as_ref().is_none_or(|s| s.name != r.shm || s.size != r.shm_size) {
            self.shm = None; // unmap the old view first
            self.shm = Some(Shm::open(&r.shm, r.shm_size)?);
        }
        let base = self.shm.as_ref().expect("mapped").ptr;
        let ctx = self.describe_inner(r.index)?;
        if ctx.context.is_empty() {
            return Err("no Filter or General context".into());
        }
        let (scale, time) = (r.scale.clamp(1e-3, 1.0), r.time);
        suites::NOW.store(time.to_bits(), std::sync::atomic::Ordering::Relaxed);
        let frame = Frame { live: false, src: base as usize, dst: base as usize + r.out as usize, w: r.w, h: r.h, float: r.float, scale, par: if r.par > 0.0 { r.par } else { 1.0 }, time };
        let key = (r.instance.clone(), r.index, r.float);
        self.clock += 1;
        let fresh = !self.instances.contains_key(&key);
        if fresh {
            self.evict();
            let inst = self.create(r, &ctx.context, frame)?;
            self.instances.insert(key.clone(), inst);
        }
        let index = r.index;
        let clock = self.clock;
        let inst = self.instances.get_mut(&key).expect("just made");
        inst.used = clock;
        let e = &*inst.effect as *const Effect;
        // SAFETY: the instance's effect lives in `self.instances` for this whole call.
        let e = unsafe { &*e };
        set_frame_props(e, &frame);
        *lock(&e.frame) = frame;
        // Parameters Milan changed since it last sent them: set, then instance changed (not for a new instance).
        let changed: Vec<String> = r.params.iter().filter(|(k, v)| inst.sent.get(k) != Some(v)).map(|(k, _)| k.clone()).collect();
        for (k, v) in &r.params {
            if changed.contains(k) {
                set_param(e, k, v);
                inst.sent.insert(k.clone(), v.clone());
            }
        }
        let began = std::mem::replace(&mut inst.began, true);
        if !fresh && !changed.is_empty() {
            let reason = Props::default();
            reason.s(kOfxPropChangeReason, kOfxChangeUserEdited);
            self.call(index, kOfxActionBeginInstanceChanged, e.handle(), Some(&reason), None);
            for k in &changed {
                let a = Props::default();
                a.s(kOfxPropType, kOfxTypeParameter);
                a.st(kOfxPropName, k);
                a.s(kOfxPropChangeReason, kOfxChangeUserEdited);
                a.d(kOfxPropTime, &[time]);
                a.d(kOfxImageEffectPropRenderScale, &[scale, scale]);
                self.call(index, kOfxActionInstanceChanged, e.handle(), Some(&a), None);
            }
            self.call(index, kOfxActionEndInstanceChanged, e.handle(), Some(&reason), None);
        }
        // The pictures are in place from here until the render returns.
        lock(&e.frame).live = true;
        // SAFETY: the output region is inside the mapping (checked above).
        unsafe { std::ptr::write_bytes(frame.dst as *mut u8, 0, px as usize) };
        let res = self.render_actions(index, e, &frame, !began);
        lock(&e.frame).live = false;
        res
    }

    fn render_actions(&self, index: u32, e: &Effect, f: &Frame, begin: bool) -> Result<(), String> {
        let scale = [f.scale, f.scale];
        let window = V::I(vec![0, 0, f.w as c_int, f.h as c_int]);
        let rod = f.rod();
        let rod = [rod.x1, rod.y1, rod.x2, rod.y2];
        if begin {
            let a = Props::default();
            a.d(kOfxImageEffectPropFrameRange, &[f.time, f.time]);
            a.d(kOfxImageEffectPropFrameStep, &[1.0]);
            a.i(kOfxPropIsInteractive, 0);
            a.d(kOfxImageEffectPropRenderScale, &scale);
            a.i(kOfxImageEffectPropSequentialRenderStatus, 0);
            a.i(kOfxImageEffectPropInteractiveRenderStatus, 0);
            self.call(index, kOfxImageEffectActionBeginSequenceRender, e.handle(), Some(&a), None);
        }
        // Region of definition and of interest: Milan always gives and takes the whole frame; asked as OFX wants.
        let a = Props::default();
        a.d(kOfxPropTime, &[f.time]);
        a.d(kOfxImageEffectPropRenderScale, &scale);
        let out = Props::default();
        out.d(kOfxImageEffectPropRegionOfDefinition, &rod);
        self.call(index, kOfxImageEffectActionGetRegionOfDefinition, e.handle(), Some(&a), Some(&out));
        a.d(c"OfxImageEffectPropRegionOfInterest", &rod);
        let out = Props::default();
        out.d(c"OfxImageClipPropRoI_Source", &rod);
        self.call(index, kOfxImageEffectActionGetRegionsOfInterest, e.handle(), Some(&a), Some(&out));
        // Identity: the source passes through.
        let a = Props::default();
        a.d(kOfxPropTime, &[f.time]);
        a.s(kOfxImageEffectPropFieldToRender, kOfxImageFieldNone);
        a.set(kOfxImageEffectPropRenderWindow, window.clone());
        a.d(kOfxImageEffectPropRenderScale, &scale);
        let out = Props::default();
        out.st(kOfxPropName, "");
        out.d(kOfxPropTime, &[f.time]);
        if self.call(index, kOfxImageEffectActionIsIdentity, e.handle(), Some(&a), Some(&out)) == kOfxStatOK && out.s0(kOfxPropName) == "Source" {
            let n = f.w as usize * f.h as usize * if f.float { 16 } else { 4 };
            // SAFETY: both regions are inside the mapping and do not overlap.
            unsafe { std::ptr::copy_nonoverlapping(f.src as *const u8, f.dst as *mut u8, n) };
            return Ok(());
        }
        a.i(kOfxImageEffectPropSequentialRenderStatus, 0);
        a.i(kOfxImageEffectPropInteractiveRenderStatus, 0);
        a.i(kOfxImageEffectPropRenderQualityDraft, (f.scale < 1.0) as c_int);
        match self.call(index, kOfxImageEffectActionRender, e.handle(), Some(&a), None) {
            kOfxStatOK => Ok(()),
            s => Err(format!("the render action failed ({}){}", status(s), messages())),
        }
    }

    /// A new instance of plug-in `r.index` with Milan's values (no instance changed for them, as for a loaded project).
    fn create(&mut self, r: &Render, context: &str, f: Frame) -> Result<Instance, String> {
        let (_, ctx, _) = self.descs.get(&r.index).ok_or("not described")?;
        let props = ctx.props.copy();
        props.s(kOfxPropType, kOfxTypeImageEffectInstance);
        props.st(kOfxImageEffectPropContext, context);
        props.i(kOfxPropIsInteractive, 0);
        props.p(kOfxPropInstanceData, 0);
        props.d(kOfxImageEffectPropFrameRate, &[if r.fps > 0.0 { r.fps } else { 30.0 }]);
        props.d(kOfxImageEffectInstancePropEffectDuration, &[1e6]);
        props.i(kOfxImageEffectInstancePropSequentialRender, 0);
        let e = Effect::new(props);
        for c in lock(&ctx.clips).iter() {
            let p = c.props.copy();
            p.d(kOfxImageEffectPropFrameRate, &[if r.fps > 0.0 { r.fps } else { 30.0 }]);
            p.d(kOfxImageEffectPropFrameRange, &[0.0, 1e6]);
            p.d(kOfxImageEffectPropUnmappedFrameRange, &[0.0, 1e6]);
            p.d(kOfxImageEffectPropUnmappedFrameRate, &[if r.fps > 0.0 { r.fps } else { 30.0 }]);
            p.s(kOfxImageClipPropFieldOrder, kOfxImageFieldNone);
            p.i(kOfxImageClipPropContinuousSamples, 0);
            // Only the source and the output are connected (a General-context mask or extra input is not).
            p.i(kOfxImageClipPropConnected, matches!(c.name.as_str(), "Source" | "Output") as c_int);
            e.add_clip(&c.name, p);
        }
        {
            let mut params = lock(&e.params.params);
            for p in lock(&ctx.params.params).iter() {
                params.push(p.instance_copy());
            }
        }
        set_frame_props(&e, &f);
        let mut sent = HashMap::new();
        for (k, v) in &r.params {
            set_param(&e, k, v);
            sent.insert(k.clone(), v.clone());
        }
        let index = r.index;
        match self.call(index, kOfxActionCreateInstance, e.handle(), None, None) {
            kOfxStatOK | kOfxStatReplyDefault => {}
            s => return Err(format!("its create instance action failed ({}){}", status(s), messages())),
        }
        // Clip preferences: Milan gives every clip the same depth and RGBA, premultiplied; the plug-in's wishes are read
        // and not needed (it said it takes these when it listed them).
        let out = Props::default();
        for c in lock(&e.clips).iter() {
            out.s(&cstring(&format!("OfxImageClipPropComponents_{}", c.name)), kOfxImageComponentRGBA);
            out.s(&cstring(&format!("OfxImageClipPropDepth_{}", c.name)), if f.float { kOfxBitDepthFloat } else { kOfxBitDepthByte });
            out.d(&cstring(&format!("OfxImageClipPropPAR_{}", c.name)), &[f.par]);
        }
        out.d(kOfxImageEffectPropFrameRate, &[if r.fps > 0.0 { r.fps } else { 30.0 }]);
        out.s(kOfxImageEffectPropPreMultiplication, kOfxImagePreMultiplied);
        out.i(kOfxImageClipPropContinuousSamples, 0);
        out.i(kOfxImageEffectFrameVarying, 0);
        self.call(index, kOfxImageEffectActionGetClipPreferences, e.handle(), None, Some(&out));
        Ok(Instance { effect: e, sent, began: false, used: self.clock })
    }

    fn evict(&mut self) {
        while self.instances.len() >= MAX_INSTANCES {
            let Some(k) = self.instances.iter().min_by_key(|i| i.1.used).map(|i| i.0.clone()) else { return };
            if let Some(i) = self.instances.remove(&k) {
                self.destroy(k.1, i);
            }
        }
    }

    fn destroy(&self, index: u32, i: Instance) {
        if i.began {
            let a = Props::default();
            a.d(kOfxImageEffectPropFrameRange, &[0.0, 0.0]);
            a.d(kOfxImageEffectPropFrameStep, &[1.0]);
            a.i(kOfxPropIsInteractive, 0);
            a.d(kOfxImageEffectPropRenderScale, &[1.0, 1.0]);
            self.call(index, kOfxImageEffectActionEndSequenceRender, i.effect.handle(), Some(&a), None);
        }
        self.call(index, kOfxActionDestroyInstance, i.effect.handle(), None, None);
    }

    /// Destroy every instance (Quit). The library is left to the process exit: some plug-ins do not survive an unload.
    pub fn shutdown(&mut self) {
        for (k, i) in std::mem::take(&mut self.instances) {
            self.destroy(k.1, i);
        }
    }
}

/// The frame size, depth and scale on an instance and its clips.
fn set_frame_props(e: &Effect, f: &Frame) {
    let rod = f.rod();
    e.props.d(kOfxImageEffectPropProjectSize, &[rod.x2, rod.y2]);
    e.props.d(kOfxImageEffectPropProjectExtent, &[rod.x2, rod.y2]);
    e.props.d(kOfxImageEffectPropProjectOffset, &[0.0, 0.0]);
    e.props.d(kOfxImageEffectPropProjectPixelAspectRatio, &[f.par]);
    let depth = if f.float { kOfxBitDepthFloat } else { kOfxBitDepthByte };
    for c in lock(&e.clips).iter() {
        for k in [kOfxImageEffectPropPixelDepth, kOfxImageClipPropUnmappedPixelDepth] {
            c.props.s(k, depth);
        }
        for k in [kOfxImageEffectPropComponents, kOfxImageClipPropUnmappedComponents] {
            c.props.s(k, kOfxImageComponentRGBA);
        }
        c.props.s(kOfxImageEffectPropPreMultiplication, kOfxImagePreMultiplied);
        c.props.d(kOfxImagePropPixelAspectRatio, &[f.par]);
    }
}

fn set_param(e: &Effect, k: &str, v: &PVal) {
    let Some(p) = e.params.find(k) else { return };
    // SAFETY: parameters live as long as their effect.
    let p = unsafe { &*p };
    let new = match (class(&p.kind), v) {
        (Class::Int(n) | Class::Dbl(n), PVal::N(x)) => {
            let mut x = x.clone();
            x.resize(n, 0.0);
            PV::N(x)
        }
        (Class::Str, PVal::S(s)) => PV::S(cstring(s)),
        _ => return,
    };
    *lock(&p.value) = new;
}

fn param_desc(name: &str, kind: &str, p: &Props) -> ParamDesc {
    let first = |k: &CStr| p.get_d(k).first().copied();
    let lo = first(kOfxParamPropDisplayMin).or(first(kOfxParamPropMin)).unwrap_or(0.0);
    let hi = first(kOfxParamPropDisplayMax).or(first(kOfxParamPropMax)).unwrap_or(1.0);
    ParamDesc {
        name: name.into(),
        label: Some(p.s0(kOfxPropLabel)).filter(|l| !l.is_empty()).unwrap_or_else(|| name.into()),
        kind: kind.strip_prefix("OfxParamType").unwrap_or(kind).into(),
        default: p.get_d(kOfxParamPropDefault),
        text: if class(kind) == Class::Str { p.s0(kOfxParamPropDefault) } else { String::new() },
        min: lo,
        max: hi,
        options: p.get_s(kOfxParamPropChoiceOption),
        parent: p.s0(kOfxParamPropParent),
        secret: p.i0(kOfxParamPropSecret).unwrap_or(0) != 0,
        multiline: p.s0(kOfxParamPropStringMode) == "OfxParamStringIsMultiLine",
        hint: p.s0(kOfxParamPropHint),
    }
}

pub fn status(s: OfxStatus) -> String {
    let n = ["OK", "Failed", "ErrFatal", "ErrUnknown", "ErrMissingHostFeature", "ErrUnsupported", "ErrExists", "ErrFormat", "ErrMemory", "ErrBadHandle", "ErrBadIndex", "ErrValue", "ReplyYes", "ReplyNo", "ReplyDefault"];
    n.get(s as usize).map_or_else(|| format!("status {s}"), |n| format!("kOfxStat{n}"))
}

/// The plug-in's last messages, as a suffix for an error.
fn messages() -> String {
    let m = std::mem::take(&mut *lock(&suites::MESSAGES));
    if m.is_empty() { String::new() } else { format!(": {}", m.join("; ")) }
}

/// Milan's shared memory, mapped.
struct Shm {
    name: String,
    size: u64,
    handle: windows_sys::Win32::Foundation::HANDLE,
    ptr: *mut u8,
}

impl Shm {
    fn open(name: &str, size: u64) -> Result<Shm, String> {
        use windows_sys::Win32::System::Memory::{FILE_MAP_ALL_ACCESS, MapViewOfFile, OpenFileMappingW};
        let w: Vec<u16> = name.encode_utf16().chain([0]).collect();
        // SAFETY: a NUL-terminated name; the view is unmapped and the handle closed on drop.
        unsafe {
            let handle = OpenFileMappingW(FILE_MAP_ALL_ACCESS, 0, w.as_ptr());
            if handle.is_null() {
                return Err(format!("cannot open the frame memory: {}", std::io::Error::last_os_error()));
            }
            let view = MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, size as usize);
            if view.Value.is_null() {
                windows_sys::Win32::Foundation::CloseHandle(handle);
                return Err(format!("cannot map the frame memory: {}", std::io::Error::last_os_error()));
            }
            Ok(Shm { name: name.into(), size, handle, ptr: view.Value as *mut u8 })
        }
    }
}

impl Drop for Shm {
    fn drop(&mut self) {
        use windows_sys::Win32::System::Memory::{MEMORY_MAPPED_VIEW_ADDRESS, UnmapViewOfFile};
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: self.ptr as _ });
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}
