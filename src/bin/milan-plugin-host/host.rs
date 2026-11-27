//! One loaded OFX binary: its plug-ins' load, describe and describeInContext actions, and their instances with the
//! render action sequence (instance changed, begin sequence, region of definition and interest, identity, render).
use std::collections::HashMap;
use std::ffi::{CStr, c_int};
use std::path::{Path, PathBuf};
use crate::ofx::*;
use crate::proto::{Depth, Outcome, PVal, ParamDesc, ParamState, PluginDesc, Render, Reply};
use crate::suites::{self, Class, Effect, Frame, PV, Props, V, class, cstring, lock};

const TRANSITION: &str = "OfxImageEffectContextTransition";
const GENERATOR: &str = "OfxImageEffectContextGenerator";
/// The contexts Milan runs, in the order it prefers them for a plug-in that has several.
const CONTEXTS: [&str; 4] = ["OfxImageEffectContextFilter", "OfxImageEffectContextGeneral", TRANSITION, GENERATOR];

/// At most this many plug-in instances live in one host; the least recently used goes first.
const MAX_INSTANCES: usize = 16;

struct Instance {
    effect: Box<Effect>,
    /// What Milan and the instance both hold, per parameter (as Milan stores it): a value Milan sends that differs is
    /// applied; a value the plug-in set that differs is reported.
    sent: HashMap<String, PVal>,
    /// The render scale of the sequence begun (None: none).
    began: Option<f64>,
    used: u64,
    /// The parameters' run-time state as last reported to Milan.
    state: Vec<ParamState>,
    /// Parameters the plug-in wrote during a render action: its own working values (the render scale it saw, a
    /// cache key...), which Milan neither stores nor sets back, or every frame would change the project.
    owned: std::collections::HashSet<String>,
}

pub struct Host {
    host: *mut OfxHost,
    lib: Option<(PathBuf, libloading::Library)>,
    plugins: Vec<*const OfxPlugin>,
    /// Per plug-in index: load action passed (Ok) or why not.
    loaded: Vec<Result<(), String>>,
    /// Per plug-in index: the describe and the context descriptor, kept for its instances.
    descs: HashMap<u32, (Box<Effect>, Box<Effect>, PluginDesc)>,
    instances: HashMap<(String, u32, Depth), Instance>,
    shm: Option<Shm>,
    clock: u64,
}

impl Host {
    pub fn new() -> Self {
        let props: &'static Props = Box::leak(suites::host_props());
        suites::HOST_PROPS.store(props.handle() as u64, std::sync::atomic::Ordering::Relaxed);
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
        // SAFETY: loading the plug-in runs its initialisers; that is what this isolated process is for. The altered
        // search path finds the DLLs a plug-in ships beside its binary (as other OFX hosts load them).
        const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 8;
        let lib = match unsafe { libloading::os::windows::Library::load_with_flags(binary, if binary.is_absolute() { LOAD_WITH_ALTERED_SEARCH_PATH } else { 0 }) } {
            Ok(l) => libloading::Library::from(l),
            // The OS error names the cause (a DLL it imports is missing, a wrong architecture...).
            Err(e) => return Reply::Error(format!("cannot load {}: {e}{}", binary.display(), std::error::Error::source(&e).map_or(String::new(), |s| format!(": {s}")))),
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
        let context = CONTEXTS.into_iter().find(|c| contexts.iter().any(|x| x == c)).unwrap_or("");
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
            aex: None,
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
        match self.with_instance(r, true, |h, i, f| h.frame(r, i, f)) {
            Ok(o) => Reply::Rendered(o),
            Err(e) => Reply::Error(e),
        }
    }

    /// A push button was clicked (`button` "": only make the instance, so Milan learns its starting values). The
    /// button's own failure is the plug-in's message, not a reason to bypass it.
    pub fn button(&mut self, r: &Render, button: &str) -> Reply {
        let index = r.index;
        let run = |h: &Host, i: &mut Instance, f: &Frame| {
            if button.is_empty() {
                return Ok(());
            }
            let e = &*i.effect;
            // SAFETY: parameters live as long as their effect.
            match e.params.find(button).map(|p| unsafe { &*p }) {
                Some(p) if p.kind == "OfxParamTypePushButton" => {}
                _ => return Err(format!("the plug-in has no button \"{button}\"")),
            }
            let st = h.changed(index, e, &[button.to_string()], f);
            if !matches!(st, kOfxStatOK | kOfxStatReplyDefault) && lock(&suites::MESSAGES).is_empty() {
                suites::log(format!("Error: the button did not work ({})", status(st)));
            }
            Ok(())
        };
        match self.with_instance(r, false, run) {
            Ok(o) => Reply::Rendered(o),
            Err(e) => Reply::Error(e),
        }
    }

    /// Run `run` on the instance `r` names (made if needed, with Milan's values applied), then report what the plug-in
    /// changed. `picture`: the request carries a frame in shared memory.
    fn with_instance(&mut self, r: &Render, picture: bool, run: impl FnOnce(&Host, &mut Instance, &Frame) -> Result<(), String>) -> Result<Outcome, String> {
        // Messages from before this request belong to nothing.
        lock(&suites::MESSAGES).clear();
        let n = r.w as u64 * r.h as u64 * r.depth.bpp() as u64;
        if r.w == 0 || r.h == 0 || r.inputs > 2 {
            return Err("bad frame size".into());
        }
        let mut base = 0usize;
        if picture {
            if n * r.inputs as u64 > r.out || r.out.checked_add(n).is_none_or(|end| end > r.shm_size) {
                return Err("bad frame size".into());
            }
            if self.shm.as_ref().is_none_or(|s| s.name != r.shm || s.size != r.shm_size) {
                self.shm = None; // unmap the old view first
                self.shm = Some(Shm::open(&r.shm, r.shm_size)?);
            }
            base = self.shm.as_ref().expect("mapped").ptr as usize;
        }
        let ctx = self.describe_inner(r.index)?;
        if ctx.context.is_empty() {
            return Err("no context Milan Pro runs".into());
        }
        let (scale, time) = (r.scale.clamp(1e-3, 1.0), r.time);
        suites::NOW.store(time.to_bits(), std::sync::atomic::Ordering::Relaxed);
        let src = [0, 1].map(|k| if picture && k < r.inputs as usize { base + k * n as usize } else { 0 });
        let frame = Frame { live: false, src, dst: if picture { base + r.out as usize } else { 0 }, w: r.w, h: r.h, depth: r.depth, scale, par: if r.par > 0.0 { r.par } else { 1.0 }, time };
        let key = (r.instance.clone(), r.index, r.depth);
        self.clock += 1;
        let (mut inst, fresh) = match self.instances.remove(&key) {
            Some(i) => (i, false),
            None => {
                self.evict();
                (self.create(r, &ctx.context, frame)?, true)
            }
        };
        inst.used = self.clock;
        set_frame_props(&inst.effect, &frame);
        *lock(&inst.effect.frame) = frame;
        self.apply_values(r, &ctx.context, &mut inst, &frame);
        let res = run(self, &mut inst, &frame);
        let out = self.outcome(&mut inst, &ctx.context, fresh, fresh && !r.restored);
        self.instances.insert(key, inst);
        res.map(|_| out)
    }

    /// Milan's values into the instance: those that differ from what it holds are set, then instance changed runs for
    /// them. Nothing for a new FX Milan holds no values of yet (`restored` false): the plug-in's starting values stand.
    fn apply_values(&self, r: &Render, context: &str, inst: &mut Instance, f: &Frame) {
        let e = &*inst.effect;
        let mut changed = Vec::new();
        for (k, v) in &r.params {
            // The host animates a transition's own parameter: no instance changed for it.
            if context == TRANSITION && k == "Transition" {
                set_param(e, k, v);
            } else if r.restored && !inst.owned.contains(k) && inst.sent.get(k).is_some_and(|s| s != v) {
                set_param(e, k, v);
                inst.sent.insert(k.clone(), v.clone());
                changed.push(k.clone());
            }
        }
        if !changed.is_empty() {
            self.changed(r.index, e, &changed, f);
        }
    }

    /// Begin, instance changed for each of `names` (as a user's edit), end. The last status of the changed actions.
    fn changed(&self, index: u32, e: &Effect, names: &[String], f: &Frame) -> OfxStatus {
        let reason = Props::default();
        reason.s(kOfxPropChangeReason, kOfxChangeUserEdited);
        self.call(index, kOfxActionBeginInstanceChanged, e.handle(), Some(&reason), None);
        let mut st = kOfxStatOK;
        for k in names {
            let a = Props::default();
            a.s(kOfxPropType, kOfxTypeParameter);
            a.st(kOfxPropName, k);
            a.s(kOfxPropChangeReason, kOfxChangeUserEdited);
            a.d(kOfxPropTime, &[f.time]);
            a.d(kOfxImageEffectPropRenderScale, &[f.scale, f.scale]);
            st = self.call(index, kOfxActionInstanceChanged, e.handle(), Some(&a), None);
        }
        self.call(index, kOfxActionEndInstanceChanged, e.handle(), Some(&reason), None);
        st
    }

    /// What the plug-in changed since Milan's values went in: its own parameter edits (every value for a new
    /// instance), the parameters' hidden / enabled / label / options when they changed (always for a new instance),
    /// and its messages. Values are
    /// kept as Milan stores them (32-bit floats), in the instance too, so both sides hold the same numbers.
    fn outcome(&self, inst: &mut Instance, context: &str, fresh: bool, initial: bool) -> Outcome {
        let mut out = Outcome { initial, messages: std::mem::take(&mut *lock(&suites::MESSAGES)), ..Default::default() };
        let params = lock(&inst.effect.params.params);
        let mut state = Vec::with_capacity(params.len());
        for p in params.iter() {
            state.push(ParamState {
                name: p.name.clone(),
                secret: p.props.i0(kOfxParamPropSecret).unwrap_or(0) != 0,
                enabled: p.props.i0(kOfxParamPropEnabled).unwrap_or(1) != 0,
                label: Some(p.props.s0(kOfxPropLabel)).filter(|l| !l.is_empty()).unwrap_or_else(|| p.name.clone()),
                options: p.props.get_s(kOfxParamPropChoiceOption),
            });
            if class(&p.kind) == Class::None || (context == TRANSITION && p.name == "Transition") || inst.owned.contains(&p.name) {
                continue;
            }
            let mut v = lock(&p.value);
            let cur = stored(&p.kind, &v);
            if let (PV::N(x), PVal::N(q)) = (&mut *v, &cur) {
                x.clone_from(q);
            }
            if initial || inst.sent.get(&p.name) != Some(&cur) {
                inst.sent.insert(p.name.clone(), cur.clone());
                out.edits.push((p.name.clone(), cur));
            }
        }
        // A new instance always reports its state, so Milan holds one for every instance it has rendered.
        if fresh || state != inst.state {
            out.state = Some(state.clone());
            inst.state = state;
        }
        out
    }

    /// The render action sequence for the frame in shared memory.
    fn frame(&self, r: &Render, inst: &mut Instance, f: &Frame) -> Result<(), String> {
        let e = &*inst.effect;
        // A sequence is one render scale: a new preview size ends it and begins another.
        if let Some(old) = inst.began.filter(|s| *s != f.scale) {
            self.end_sequence(r.index, e, old);
            inst.began = None;
        }
        let begin = inst.began.replace(f.scale).is_none();
        // The pictures are in place from here until the render returns.
        lock(&e.frame).live = true;
        // SAFETY: the output region is inside the mapping (checked by `with_instance`).
        unsafe { std::ptr::write_bytes(f.dst as *mut u8, 0, f.w as usize * f.h as usize * f.depth.bpp()) };
        let before: Vec<(String, PVal)> = lock(&e.params.params).iter().filter(|p| class(&p.kind) != Class::None).map(|p| (p.name.clone(), stored(&p.kind, &lock(&p.value)))).collect();
        let res = self.render_actions(r.index, e, f, begin);
        lock(&e.frame).live = false;
        // What the render wrote stays the plug-in's own: not reported as an edit, not set back next frame.
        for p in lock(&e.params.params).iter().filter(|p| class(&p.kind) != Class::None) {
            let cur = stored(&p.kind, &lock(&p.value));
            if before.iter().any(|b| b.0 == p.name && b.1 != cur) {
                inst.owned.insert(p.name.clone());
                inst.sent.insert(p.name.clone(), cur);
            }
        }
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
        for c in lock(&e.clips).iter().filter(|c| c.name != "Output") {
            out.d(&cstring(&format!("OfxImageClipPropRoI_{}", c.name)), &rod);
        }
        self.call(index, kOfxImageEffectActionGetRegionsOfInterest, e.handle(), Some(&a), Some(&out));
        // Identity: one of the inputs passes through.
        let a = Props::default();
        a.d(kOfxPropTime, &[f.time]);
        a.s(kOfxImageEffectPropFieldToRender, kOfxImageFieldNone);
        a.set(kOfxImageEffectPropRenderWindow, window.clone());
        a.d(kOfxImageEffectPropRenderScale, &scale);
        let out = Props::default();
        out.st(kOfxPropName, "");
        out.d(kOfxPropTime, &[f.time]);
        if self.call(index, kOfxImageEffectActionIsIdentity, e.handle(), Some(&a), Some(&out)) == kOfxStatOK {
            let name = out.s0(kOfxPropName);
            let from = f.data(&name);
            if from != 0 && name != "Output" {
                let n = f.w as usize * f.h as usize * f.depth.bpp();
                // SAFETY: both regions are inside the mapping and do not overlap.
                unsafe { std::ptr::copy_nonoverlapping(from as *const u8, f.dst as *mut u8, n) };
                return Ok(());
            }
        }
        a.i(kOfxImageEffectPropSequentialRenderStatus, 0);
        a.i(kOfxImageEffectPropInteractiveRenderStatus, 0);
        a.i(kOfxImageEffectPropRenderQualityDraft, (f.scale < 1.0) as c_int);
        match self.call(index, kOfxImageEffectActionRender, e.handle(), Some(&a), None) {
            kOfxStatOK => Ok(()),
            s => Err(format!("the render action failed ({}){}", status(s), messages())),
        }
    }

    /// A new instance of plug-in `r.index`, as its create instance action leaves it.
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
        let connected: &[&str] = match context {
            TRANSITION => &["SourceFrom", "SourceTo", "Output"],
            GENERATOR => &["Output"],
            // Only the source and the output (a General-context mask or extra input is not connected).
            _ => &["Source", "Output"],
        };
        for c in lock(&ctx.clips).iter() {
            let p = c.props.copy();
            p.d(kOfxImageEffectPropFrameRate, &[if r.fps > 0.0 { r.fps } else { 30.0 }]);
            p.d(kOfxImageEffectPropFrameRange, &[0.0, 1e6]);
            p.d(kOfxImageEffectPropUnmappedFrameRange, &[0.0, 1e6]);
            p.d(kOfxImageEffectPropUnmappedFrameRate, &[if r.fps > 0.0 { r.fps } else { 30.0 }]);
            p.s(kOfxImageClipPropFieldOrder, kOfxImageFieldNone);
            p.i(kOfxImageClipPropContinuousSamples, 0);
            p.i(kOfxImageClipPropConnected, connected.contains(&c.name.as_str()) as c_int);
            e.add_clip(&c.name, p);
        }
        {
            let mut params = lock(&e.params.params);
            for p in lock(&ctx.params.params).iter() {
                params.push(p.instance_copy());
            }
        }
        set_frame_props(&e, &f);
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
            out.st(&cstring(&format!("OfxImageClipPropDepth_{}", c.name)), f.depth.ofx());
            out.d(&cstring(&format!("OfxImageClipPropPAR_{}", c.name)), &[f.par]);
        }
        out.d(kOfxImageEffectPropFrameRate, &[if r.fps > 0.0 { r.fps } else { 30.0 }]);
        out.s(kOfxImageEffectPropPreMultiplication, kOfxImagePreMultiplied);
        out.i(kOfxImageClipPropContinuousSamples, 0);
        out.i(kOfxImageEffectFrameVarying, 0);
        self.call(index, kOfxImageEffectActionGetClipPreferences, e.handle(), None, Some(&out));
        // What the instance holds now: Milan's values are compared with these.
        let sent = lock(&e.params.params).iter().filter(|p| class(&p.kind) != Class::None).map(|p| (p.name.clone(), stored(&p.kind, &lock(&p.value)))).collect();
        // The parameters as described: a change the plug-in made while creating the instance is reported.
        let state = self.descs.get(&index).map(|d| d.2.params.iter().map(|p| ParamState { name: p.name.clone(), secret: p.secret, enabled: p.enabled, label: p.label.clone(), options: p.options.clone() }).collect()).unwrap_or_default();
        Ok(Instance { effect: e, sent, began: None, used: self.clock, state, owned: Default::default() })
    }

    fn end_sequence(&self, index: u32, e: &Effect, scale: f64) {
        let a = Props::default();
        a.d(kOfxImageEffectPropFrameRange, &[0.0, 0.0]);
        a.d(kOfxImageEffectPropFrameStep, &[1.0]);
        a.i(kOfxPropIsInteractive, 0);
        a.d(kOfxImageEffectPropRenderScale, &[scale, scale]);
        self.call(index, kOfxImageEffectActionEndSequenceRender, e.handle(), Some(&a), None);
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
        if let Some(scale) = i.began {
            self.end_sequence(index, &i.effect, scale);
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
    for c in lock(&e.clips).iter() {
        for k in [kOfxImageEffectPropPixelDepth, kOfxImageClipPropUnmappedPixelDepth] {
            c.props.st(k, f.depth.ofx());
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

/// A parameter's value as Milan stores it: numbers as 32-bit floats (integers rounded), finite; strings as they are.
fn stored(kind: &str, v: &PV) -> PVal {
    let int = matches!(class(kind), Class::Int(_));
    match v {
        PV::N(x) => PVal::N(x.iter().map(|&d| if d.is_finite() { (if int { d.round() } else { d }).clamp(-f32::MAX as f64, f32::MAX as f64) as f32 as f64 } else { 0.0 }).collect()),
        PV::S(s) => PVal::S(s.to_string_lossy().into_owned()),
    }
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
        vmin: first(kOfxParamPropMin).filter(|v| v.is_finite()).unwrap_or(-f64::MAX),
        vmax: first(kOfxParamPropMax).filter(|v| v.is_finite()).unwrap_or(f64::MAX),
        options: p.get_s(kOfxParamPropChoiceOption),
        parent: p.s0(kOfxParamPropParent),
        secret: p.i0(kOfxParamPropSecret).unwrap_or(0) != 0,
        multiline: p.s0(kOfxParamPropStringMode) == "OfxParamStringIsMultiLine",
        hint: p.s0(kOfxParamPropHint),
        enabled: p.i0(kOfxParamPropEnabled).unwrap_or(1) != 0,
        open: p.i0(kOfxParamPropGroupOpen).unwrap_or(1) != 0,
        children: p.get_s(kOfxParamPropPageChild),
    }
}

pub fn status(s: OfxStatus) -> String {
    let n = ["OK", "Failed", "ErrFatal", "ErrUnknown", "ErrMissingHostFeature", "ErrUnsupported", "ErrExists", "ErrFormat", "ErrMemory", "ErrBadHandle", "ErrBadIndex", "ErrValue", "ReplyYes", "ReplyNo", "ReplyDefault"];
    n.get(s as usize).map_or_else(|| format!("status {s}"), |n| format!("kOfxStat{n}"))
}

/// The plug-in's last messages, and the suites it asked for in vain, as a suffix for an error.
fn messages() -> String {
    let m = std::mem::take(&mut *lock(&suites::MESSAGES));
    let m = if m.is_empty() { String::new() } else { format!(": {}", m.join("; ")) };
    format!("{m}{}", suites::refused())
}

/// Can this process make a Vulkan device (as Spektrafilm does, compute only, no window)? The loader is taken from the
/// system; each device's name and whether it has a compute queue are reported.
pub fn gpu_check() -> Reply {
    use std::ffi::c_void;
    use std::os::raw::c_char;
    #[repr(C)]
    struct AppInfo {
        s_type: u32,
        next: *const c_void,
        app: *const c_char,
        app_version: u32,
        engine: *const c_char,
        engine_version: u32,
        api: u32,
    }
    #[repr(C)]
    struct CreateInfo {
        s_type: u32,
        next: *const c_void,
        flags: u32,
        app: *const AppInfo,
        layers: u32,
        layer_names: *const *const c_char,
        exts: u32,
        ext_names: *const *const c_char,
    }
    type CreateInstance = unsafe extern "system" fn(*const CreateInfo, *const c_void, *mut *mut c_void) -> i32;
    type Destroy = unsafe extern "system" fn(*mut c_void, *const c_void);
    type Enumerate = unsafe extern "system" fn(*mut c_void, *mut u32, *mut *mut c_void) -> i32;
    type Props = unsafe extern "system" fn(*mut c_void, *mut u8);
    type Queues = unsafe extern "system" fn(*mut c_void, *mut u32, *mut [u32; 6]);
    // SAFETY: the Vulkan loader's documented entry points, called with zeroed buffers larger than their structures
    // (VkPhysicalDeviceProperties is under 1 KB; a queue family property is 24 bytes).
    unsafe {
        let lib = match libloading::Library::new("vulkan-1.dll") {
            Ok(l) => l,
            Err(e) => return Reply::Error(format!("no Vulkan loader (vulkan-1.dll): {e}")),
        };
        let (Ok(create), Ok(destroy), Ok(enumerate), Ok(props), Ok(queues)) = (
            lib.get::<CreateInstance>(b"vkCreateInstance\0"),
            lib.get::<Destroy>(b"vkDestroyInstance\0"),
            lib.get::<Enumerate>(b"vkEnumeratePhysicalDevices\0"),
            lib.get::<Props>(b"vkGetPhysicalDeviceProperties\0"),
            lib.get::<Queues>(b"vkGetPhysicalDeviceQueueFamilyProperties\0"),
        ) else {
            return Reply::Error("the Vulkan loader lacks its entry points".into());
        };
        let app = AppInfo { s_type: 0, next: std::ptr::null(), app: c"Milan Pro plug-in host".as_ptr(), app_version: 1, engine: c"Milan Pro".as_ptr(), engine_version: 1, api: (1 << 22) | (1 << 12) };
        let info = CreateInfo { s_type: 1, next: std::ptr::null(), flags: 0, app: &app, layers: 0, layer_names: std::ptr::null(), exts: 0, ext_names: std::ptr::null() };
        let mut inst = std::ptr::null_mut();
        let r = create(&info, std::ptr::null(), &mut inst);
        if r != 0 {
            return Reply::Error(format!("vkCreateInstance failed with VkResult {r}"));
        }
        let mut n = 0u32;
        enumerate(inst, &mut n, std::ptr::null_mut());
        let mut devs = vec![std::ptr::null_mut(); n as usize];
        enumerate(inst, &mut n, devs.as_mut_ptr());
        let mut names = Vec::new();
        for d in devs.iter().take(n as usize) {
            let mut buf = vec![0u8; 8192];
            props(*d, buf.as_mut_ptr());
            let name = CStr::from_bytes_until_nul(&buf[20..276]).map_or_else(|_| "?".into(), |c| c.to_string_lossy().into_owned());
            let mut q = 0u32;
            queues(*d, &mut q, std::ptr::null_mut());
            let mut fam = vec![[0u32; 6]; q as usize];
            queues(*d, &mut q, fam.as_mut_ptr());
            let compute = fam.iter().take(q as usize).any(|f| f[0] & 2 != 0);
            names.push(format!("{name} ({}compute queue)", if compute { "" } else { "no " }));
        }
        destroy(inst, std::ptr::null());
        Reply::Rendered(Outcome { messages: vec![format!("Vulkan 1.1 instance made; {n} physical device(s): {}", names.join(", "))], ..Default::default() })
    }
}

/// Milan's shared memory, mapped (frames, or R34a audio blocks).
pub struct Shm {
    pub name: String,
    pub size: u64,
    handle: windows_sys::Win32::Foundation::HANDLE,
    ptr: *mut u8,
}

impl Shm {
    /// The whole view.
    pub fn bytes(&mut self) -> &mut [u8] {
        // SAFETY: the mapped view of `size` bytes, owned by self.
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.size as usize) }
    }

    pub fn open(name: &str, size: u64) -> Result<Shm, String> {
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
