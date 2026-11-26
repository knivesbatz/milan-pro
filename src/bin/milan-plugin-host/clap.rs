//! R35: one loaded CLAP module (a `.clap` DLL) through `clap-sys`: its factory's audio effects, instances with the
//! params, state, latency, tail and audio-ports extensions, 32-bit float stereo processing at the project rate.
//! Parameter values are plain (the plug-in's own units). The host object a plug-in sees is below: it logs, answers
//! thread checks, and notes latency changes and rescans; it never shows anything.
#![allow(clippy::missing_safety_doc)]
use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_void};
use std::path::Path;
use std::sync::Mutex;
use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::entry::clap_plugin_entry;
use clap_sys::events::*;
use clap_sys::ext::audio_ports::*;
use clap_sys::ext::latency::*;
use clap_sys::ext::log::*;
use clap_sys::ext::params::*;
use clap_sys::ext::state::*;
use clap_sys::ext::tail::*;
use clap_sys::ext::thread_check::*;
use clap_sys::factory::plugin_factory::*;
use clap_sys::host::clap_host;
use clap_sys::plugin::*;
use clap_sys::process::*;
use clap_sys::stream::*;
use clap_sys::version::CLAP_VERSION;
use crate::proto::{AInstance, AOut, AParam, AProcess, AState, AudioDesc, b64, unb64};
use crate::suites::lock;

fn cstr(p: *const c_char) -> String {
    if p.is_null() { String::new() } else { unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned() }
}

fn chars(s: &[c_char]) -> String {
    let n = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    String::from_utf8_lossy(&s[..n].iter().map(|&c| c as u8).collect::<Vec<_>>()).into_owned()
}

/// Per instance: what the plug-in asked of the host.
#[derive(Default)]
struct HostState {
    restart: bool,
    latency_changed: bool,
    rescan: bool,
    callback: bool,
    log: Vec<String>,
}

/// The `clap_host` a plug-in gets (boxed: its address must not move), with the host's state behind `host_data`.
struct Host {
    raw: clap_host,
    state: Mutex<HostState>,
}

const NAME: &CStr = c"Milan Pro";
const VENDOR: &CStr = c"MeridianNLE";
const URL: &CStr = c"https://github.com/MeridianNLE";
const VERSION: &CStr = c"0.6";

unsafe fn host_of<'a>(h: *const clap_host) -> Option<&'a Host> {
    if h.is_null() { None } else { unsafe { ((*h).host_data as *const Host).as_ref() } }
}

unsafe extern "C" fn host_get_extension(h: *const clap_host, id: *const c_char) -> *const c_void {
    let _ = h;
    let id = unsafe { CStr::from_ptr(id) };
    if id == CLAP_EXT_PARAMS {
        &HOST_PARAMS as *const _ as *const c_void
    } else if id == CLAP_EXT_LATENCY {
        &HOST_LATENCY as *const _ as *const c_void
    } else if id == CLAP_EXT_STATE {
        &HOST_STATE as *const _ as *const c_void
    } else if id == CLAP_EXT_LOG {
        &HOST_LOG as *const _ as *const c_void
    } else if id == CLAP_EXT_THREAD_CHECK {
        &HOST_THREAD as *const _ as *const c_void
    } else if id == CLAP_EXT_TAIL {
        &HOST_TAIL as *const _ as *const c_void
    } else if id == CLAP_EXT_AUDIO_PORTS {
        &HOST_PORTS as *const _ as *const c_void
    } else {
        std::ptr::null()
    }
}

unsafe extern "C" fn host_request_restart(h: *const clap_host) {
    if let Some(host) = unsafe { host_of(h) } {
        lock(&host.state).restart = true;
    }
}

unsafe extern "C" fn host_request_process(_h: *const clap_host) {}

unsafe extern "C" fn host_request_callback(h: *const clap_host) {
    if let Some(host) = unsafe { host_of(h) } {
        lock(&host.state).callback = true;
    }
}

unsafe extern "C" fn params_rescan(h: *const clap_host, _flags: clap_param_rescan_flags) {
    if let Some(host) = unsafe { host_of(h) } {
        lock(&host.state).rescan = true;
    }
}

unsafe extern "C" fn params_clear(_h: *const clap_host, _id: clap_sys::id::clap_id, _flags: clap_param_clear_flags) {}

unsafe extern "C" fn params_request_flush(_h: *const clap_host) {}

unsafe extern "C" fn latency_changed(h: *const clap_host) {
    if let Some(host) = unsafe { host_of(h) } {
        lock(&host.state).latency_changed = true;
    }
}

unsafe extern "C" fn state_mark_dirty(_h: *const clap_host) {}

unsafe extern "C" fn log_log(h: *const clap_host, severity: clap_log_severity, msg: *const c_char) {
    if let Some(host) = unsafe { host_of(h) } {
        let level = match severity {
            CLAP_LOG_DEBUG => "debug",
            CLAP_LOG_INFO => "info",
            CLAP_LOG_WARNING => "warning",
            CLAP_LOG_ERROR => "error",
            CLAP_LOG_FATAL => "fatal",
            _ => "host misbehaviour",
        };
        let mut s = lock(&host.state);
        if s.log.len() < 64 {
            s.log.push(format!("{level}: {}", cstr(msg)));
        }
    }
}

unsafe extern "C" fn thread_is_main(_h: *const clap_host) -> bool {
    true
}

unsafe extern "C" fn thread_is_audio(_h: *const clap_host) -> bool {
    true
}

unsafe extern "C" fn tail_changed(_h: *const clap_host) {}

unsafe extern "C" fn ports_flag_supported(_h: *const clap_host, _flag: u32) -> bool {
    false
}

unsafe extern "C" fn ports_rescan(_h: *const clap_host, _flags: u32) {}

static HOST_PARAMS: clap_host_params = clap_host_params { rescan: Some(params_rescan), clear: Some(params_clear), request_flush: Some(params_request_flush) };
static HOST_LATENCY: clap_host_latency = clap_host_latency { changed: Some(latency_changed) };
static HOST_STATE: clap_host_state = clap_host_state { mark_dirty: Some(state_mark_dirty) };
static HOST_LOG: clap_host_log = clap_host_log { log: Some(log_log) };
static HOST_THREAD: clap_host_thread_check = clap_host_thread_check { is_main_thread: Some(thread_is_main), is_audio_thread: Some(thread_is_audio) };
static HOST_TAIL: clap_host_tail = clap_host_tail { changed: Some(tail_changed) };
static HOST_PORTS: clap_host_audio_ports = clap_host_audio_ports { is_rescan_flag_supported: Some(ports_flag_supported), rescan: Some(ports_rescan) };

impl Host {
    fn new() -> Box<Host> {
        let mut h = Box::new(Host {
            raw: clap_host {
                clap_version: CLAP_VERSION,
                host_data: std::ptr::null_mut(),
                name: NAME.as_ptr(),
                vendor: VENDOR.as_ptr(),
                url: URL.as_ptr(),
                version: VERSION.as_ptr(),
                get_extension: Some(host_get_extension),
                request_restart: Some(host_request_restart),
                request_process: Some(host_request_process),
                request_callback: Some(host_request_callback),
            },
            state: Mutex::new(HostState::default()),
        });
        h.raw.host_data = &*h as *const Host as *mut c_void;
        h
    }
}

// ---------------------------------------------------------------- streams and event lists

unsafe extern "C" fn ostream_write(s: *const clap_ostream, buf: *const c_void, size: u64) -> i64 {
    unsafe {
        let v = &mut *((*s).ctx as *mut Vec<u8>);
        v.extend_from_slice(std::slice::from_raw_parts(buf as *const u8, size as usize));
    }
    size as i64
}

unsafe extern "C" fn istream_read(s: *const clap_istream, buf: *mut c_void, size: u64) -> i64 {
    unsafe {
        let (v, pos) = &mut *((*s).ctx as *mut (Vec<u8>, usize));
        let n = (size as usize).min(v.len().saturating_sub(*pos));
        std::ptr::copy_nonoverlapping(v.as_ptr().add(*pos), buf as *mut u8, n);
        *pos += n;
        n as i64
    }
}

unsafe extern "C" fn in_size(l: *const clap_input_events) -> u32 {
    unsafe { (*((*l).ctx as *const Vec<clap_event_param_value>)).len() as u32 }
}

unsafe extern "C" fn in_get(l: *const clap_input_events, i: u32) -> *const clap_event_header {
    unsafe {
        let v = &*((*l).ctx as *const Vec<clap_event_param_value>);
        v.get(i as usize).map_or(std::ptr::null(), |e| &e.header)
    }
}

unsafe extern "C" fn out_push(l: *const clap_output_events, e: *const clap_event_header) -> bool {
    unsafe {
        let out = &mut *((*l).ctx as *mut Vec<(u16, u32, f64)>);
        if (*e).space_id == CLAP_CORE_EVENT_SPACE_ID {
            match (*e).type_ {
                CLAP_EVENT_PARAM_VALUE => {
                    let p = &*(e as *const clap_event_param_value);
                    out.push((CLAP_EVENT_PARAM_VALUE, p.param_id, p.value));
                }
                CLAP_EVENT_PARAM_GESTURE_BEGIN | CLAP_EVENT_PARAM_GESTURE_END => {
                    let g = &*(e as *const clap_event_param_gesture);
                    out.push(((*e).type_, g.param_id, 0.0));
                }
                _ => {}
            }
        }
    }
    true
}

fn param_event(id: u32, value: f64) -> clap_event_param_value {
    clap_event_param_value { header: clap_event_header { size: std::mem::size_of::<clap_event_param_value>() as u32, time: 0, space_id: CLAP_CORE_EVENT_SPACE_ID, type_: CLAP_EVENT_PARAM_VALUE, flags: 0 }, param_id: id, cookie: std::ptr::null_mut(), note_id: -1, port_index: -1, channel: -1, key: -1, value }
}

// ---------------------------------------------------------------- the module

struct Instance {
    plugin: *const clap_plugin,
    host: Box<Host>,
    params: Vec<AParam>,
    ext_params: *const clap_plugin_params,
    ext_state: *const clap_plugin_state,
    ext_latency: *const clap_plugin_latency,
    ext_tail: *const clap_plugin_tail,
    sent: HashMap<u32, f64>,
    pending: Vec<(u32, f64)>,
    latency: u32,
    tail: u32,
    active: bool,
    rate: f64,
    block: u32,
    bufs: [Vec<f32>; 4],
    steady: i64,
}

pub struct Module {
    _lib: libloading::Library,
    entry: *const clap_plugin_entry,
    factory: *const clap_plugin_factory,
    instances: HashMap<String, Instance>,
}

impl Module {
    pub fn load(path: &Path) -> Result<Module, String> {
        // SAFETY: loading a plug-in binary runs its code; that is this process's purpose.
        unsafe {
            let lib = libloading::Library::new(path).map_err(|e| format!("cannot load {}: {e}", path.display()))?;
            let entry: libloading::Symbol<*const clap_plugin_entry> = lib.get(b"clap_entry\0").map_err(|e| format!("no clap_entry export: {e}"))?;
            let entry = *entry;
            if entry.is_null() {
                return Err("clap_entry is NULL".into());
            }
            if !clap_sys::version::clap_version_is_compatible((*entry).clap_version) {
                return Err(format!("CLAP version {}.{}.{} is not supported", (*entry).clap_version.major, (*entry).clap_version.minor, (*entry).clap_version.revision));
            }
            let cpath = std::ffi::CString::new(path.to_string_lossy().as_bytes()).map_err(|_| "bad path")?;
            if let Some(init) = (*entry).init {
                if !init(cpath.as_ptr()) {
                    return Err("clap_entry.init returned false".into());
                }
            }
            let get = (*entry).get_factory.ok_or("clap_entry has no get_factory")?;
            let factory = get(CLAP_PLUGIN_FACTORY_ID.as_ptr()) as *const clap_plugin_factory;
            if factory.is_null() {
                return Err("the module has no plug-in factory".into());
            }
            Ok(Module { _lib: lib, entry, factory, instances: HashMap::new() })
        }
    }

    fn descriptors(&self) -> Vec<*const clap_plugin_descriptor> {
        unsafe {
            let f = &*self.factory;
            let n = f.get_plugin_count.map_or(0, |g| g(self.factory));
            (0..n).filter_map(|i| f.get_plugin_descriptor.map(|g| g(self.factory, i))).filter(|d| !d.is_null()).collect()
        }
    }

    pub fn scan(&mut self) -> Vec<AudioDesc> {
        let mut out = Vec::new();
        for d in self.descriptors() {
            let d = unsafe { &*d };
            let mut features = Vec::new();
            unsafe {
                let mut p = d.features;
                while !p.is_null() && !(*p).is_null() {
                    features.push(cstr(*p));
                    p = p.add(1);
                }
            }
            let mut desc = AudioDesc { format: "clap".into(), id: cstr(d.id), name: cstr(d.name), vendor: cstr(d.vendor), version: cstr(d.version), category: features.join("|"), sdk: format!("CLAP {}.{}.{}", d.clap_version.major, d.clap_version.minor, d.clap_version.revision), ..Default::default() };
            if features.iter().any(|f| f == "instrument" || f == "note-effect" || f == "note-detector") {
                desc.error = format!("it is a {} plug-in: Milan Pro runs audio effects", features.first().map_or("", String::as_str));
                out.push(desc);
                continue;
            }
            let probe = AInstance { id: desc.id.clone(), instance: format!("scan-{}", desc.id), rate: 48_000.0, block: 1024, ..Default::default() };
            match self.make(&probe) {
                Ok(mut inst) => {
                    desc.params = inst.params.clone();
                    desc.latency = inst.latency;
                    desc.tail = inst.tail;
                    desc.has_editor = unsafe { inst.extension::<c_void>(clap_sys::ext::gui::CLAP_EXT_GUI) }.is_some();
                    inst.destroy();
                }
                Err(e) => desc.error = e,
            }
            out.push(desc);
        }
        out
    }

    fn make(&self, inst: &AInstance) -> Result<Instance, String> {
        let d = self.descriptors().into_iter().find(|d| cstr(unsafe { (**d).id }) == inst.id).ok_or_else(|| format!("the module has no plug-in {}", inst.id))?;
        let host = Host::new();
        unsafe {
            let create = (*self.factory).create_plugin.ok_or("the factory cannot create plug-ins")?;
            let plugin = create(self.factory, &host.raw, (*d).id);
            if plugin.is_null() {
                return Err("create_plugin returned NULL".into());
            }
            let mut me = Instance { plugin, host, params: Vec::new(), ext_params: std::ptr::null(), ext_state: std::ptr::null(), ext_latency: std::ptr::null(), ext_tail: std::ptr::null(), sent: HashMap::new(), pending: Vec::new(), latency: 0, tail: 0, active: false, rate: inst.rate, block: inst.block.max(1), bufs: Default::default(), steady: 0 };
            if !(*plugin).init.ok_or("no init")?(plugin) {
                (*plugin).destroy.map(|d| d(plugin));
                return Err("the plug-in's init returned false".into());
            }
            me.ext_params = me.extension::<clap_plugin_params>(CLAP_EXT_PARAMS).map_or(std::ptr::null(), |p| p as *const _);
            me.ext_state = me.extension::<clap_plugin_state>(CLAP_EXT_STATE).map_or(std::ptr::null(), |p| p as *const _);
            me.ext_latency = me.extension::<clap_plugin_latency>(CLAP_EXT_LATENCY).map_or(std::ptr::null(), |p| p as *const _);
            me.ext_tail = me.extension::<clap_plugin_tail>(CLAP_EXT_TAIL).map_or(std::ptr::null(), |p| p as *const _);
            // Stereo main ports in and out.
            let ports = me.extension::<clap_plugin_audio_ports>(CLAP_EXT_AUDIO_PORTS).ok_or("it has no audio ports")?;
            let count = |input: bool| ports.count.map_or(0, |c| c(plugin, input));
            if count(true) < 1 || count(false) < 1 {
                me.destroy();
                return Err(format!("it has {} audio input and {} output ports: Milan Pro runs stereo effects", count(true), count(false)));
            }
            let channels = |input: bool| {
                let mut info: clap_audio_port_info = std::mem::zeroed();
                ports.get.is_some_and(|g| g(plugin, 0, input, &mut info)).then_some(info.channel_count)
            };
            if channels(true) != Some(2) || channels(false) != Some(2) {
                me.destroy();
                return Err(format!("its main ports are {} in, {} out: Milan Pro runs stereo effects", channels(true).unwrap_or(0), channels(false).unwrap_or(0)));
            }
            me.params = me.read_params();
            if let Some(st) = &inst.state {
                me.load_state(st);
            }
            // What the plug-in holds now counts as sent: Milan's values reach it only where they differ.
            me.values();
            for &(id, v) in &inst.params {
                me.set_param(id, v);
            }
            me.activate(true)?;
            Ok(me)
        }
    }

    fn get(&mut self, inst: &AInstance) -> Result<&mut Instance, String> {
        if !self.instances.contains_key(&inst.instance) {
            let made = self.make(inst)?;
            self.instances.insert(inst.instance.clone(), made);
        } else if inst.reset {
            // (A state sent along is only for making the instance; `ASetState` replaces a living one's.)
            self.instances.get_mut(&inst.instance).expect("present").reset();
        }
        let me = self.instances.get_mut(&inst.instance).expect("present");
        for &(id, v) in &inst.params {
            me.set_param(id, v);
        }
        Ok(me)
    }

    pub fn open(&mut self, inst: &AInstance) -> Result<AOut, String> {
        let fresh = !self.instances.contains_key(&inst.instance);
        let me = self.get(inst)?;
        let mut out = me.out()?;
        if fresh {
            out.values = Some(me.values());
        }
        Ok(out)
    }

    pub fn process(&mut self, p: &AProcess, shm: &mut [u8]) -> Result<AOut, String> {
        let me = self.get(&p.inst)?;
        me.process(p, shm)
    }

    pub fn get_state(&mut self, inst: &AInstance) -> Result<AState, String> {
        let me = self.get(inst)?;
        Ok(me.state())
    }

    pub fn set_state(&mut self, inst: &AInstance, st: &AState) -> Result<AOut, String> {
        let me = self.get(inst)?;
        me.load_state(st);
        let mut out = me.out()?;
        out.values = Some(me.values());
        Ok(out)
    }

    pub fn close(&mut self, instance: &str) {
        if let Some(mut i) = self.instances.remove(instance) {
            i.destroy();
        }
    }

    pub fn shutdown(&mut self) {
        for (_, mut i) in self.instances.drain() {
            i.destroy();
        }
        unsafe {
            if let Some(deinit) = (*self.entry).deinit {
                deinit();
            }
        }
    }
}

impl Instance {
    unsafe fn extension<T>(&self, id: &CStr) -> Option<&'static T> {
        unsafe {
            let p = (*self.plugin).get_extension?(self.plugin, id.as_ptr()) as *const T;
            p.as_ref()
        }
    }

    fn activate(&mut self, on: bool) -> Result<(), String> {
        unsafe {
            let p = &*self.plugin;
            if on && !self.active {
                if !p.activate.map_or(false, |a| a(self.plugin, self.rate, 1, self.block)) {
                    return Err(format!("activate({} Hz, {} frames) failed", self.rate, self.block));
                }
                self.latency = self.ext_latency.as_ref().and_then(|l| l.get).map_or(0, |g| g(self.plugin));
                self.tail = self.ext_tail.as_ref().and_then(|t| t.get).map_or(0, |g| g(self.plugin));
                if !p.start_processing.map_or(true, |s| s(self.plugin)) {
                    return Err("start_processing failed".into());
                }
                self.active = true;
            } else if !on && self.active {
                if let Some(s) = p.stop_processing {
                    s(self.plugin);
                }
                if let Some(d) = p.deactivate {
                    d(self.plugin);
                }
                self.active = false;
            }
        }
        Ok(())
    }

    fn reset(&mut self) {
        unsafe {
            if let Some(r) = (*self.plugin).reset {
                r(self.plugin);
            }
        }
    }

    fn destroy(&mut self) {
        let _ = self.activate(false);
        unsafe {
            if let Some(d) = (*self.plugin).destroy {
                d(self.plugin);
            }
        }
        self.plugin = std::ptr::null();
    }

    fn read_params(&self) -> Vec<AParam> {
        let mut out = Vec::new();
        let Some(ext) = (unsafe { self.ext_params.as_ref() }) else { return out };
        unsafe {
            let n = ext.count.map_or(0, |c| c(self.plugin));
            for i in 0..n {
                let mut info: clap_param_info = std::mem::zeroed();
                if !ext.get_info.is_some_and(|g| g(self.plugin, i, &mut info)) {
                    continue;
                }
                let f = |bit: u32| info.flags & bit != 0;
                let stepped = f(CLAP_PARAM_IS_STEPPED) || f(CLAP_PARAM_IS_ENUM);
                let span = (info.max_value - info.min_value).max(0.0);
                let mut p = AParam {
                    id: info.id,
                    name: chars(&info.name),
                    units: String::new(),
                    steps: if stepped && span <= 4096.0 { span.round() as i32 } else { 0 },
                    min: info.min_value,
                    max: info.max_value,
                    default: info.default_value.clamp(info.min_value, info.max_value),
                    automatable: f(CLAP_PARAM_IS_AUTOMATABLE),
                    hidden: f(CLAP_PARAM_IS_HIDDEN),
                    bypass: f(CLAP_PARAM_IS_BYPASS),
                    readonly: f(CLAP_PARAM_IS_READONLY),
                    options: Vec::new(),
                    program: false,
                };
                let module = chars(&info.module);
                if !module.is_empty() {
                    p.units = module;
                }
                if (1..=64).contains(&p.steps) && (f(CLAP_PARAM_IS_ENUM) || p.steps <= 16) {
                    for k in 0..=p.steps {
                        let mut buf = [0 as c_char; 128];
                        let v = info.min_value + k as f64;
                        let text = if ext.value_to_text.is_some_and(|t| t(self.plugin, info.id, v, buf.as_mut_ptr(), buf.len() as u32)) { chars(&buf) } else { String::new() };
                        p.options.push(if text.is_empty() { format!("{}", v) } else { text });
                    }
                }
                out.push(p);
            }
        }
        out
    }

    fn set_param(&mut self, id: u32, v: f64) {
        if self.sent.get(&id).is_some_and(|x| *x == v) {
            return;
        }
        self.sent.insert(id, v);
        self.pending.push((id, v));
    }

    fn values(&mut self) -> Vec<(u32, f64)> {
        let Some(ext) = (unsafe { self.ext_params.as_ref() }) else { return Vec::new() };
        let mut v = Vec::new();
        for p in &self.params {
            let mut x = 0.0;
            if unsafe { ext.get_value.is_some_and(|g| g(self.plugin, p.id, &mut x)) } {
                v.push((p.id, x));
                self.sent.insert(p.id, x);
            }
        }
        v
    }

    /// Values set since the last block reach the plug-in (`params.flush`), so its state holds them.
    fn flush(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let Some(ext) = (unsafe { self.ext_params.as_ref() }) else { return };
        let events: Vec<clap_event_param_value> = self.pending.drain(..).map(|(id, v)| param_event(id, v)).collect();
        let in_events = clap_input_events { ctx: &events as *const Vec<clap_event_param_value> as *mut c_void, size: Some(in_size), get: Some(in_get) };
        let mut got: Vec<(u16, u32, f64)> = Vec::new();
        let out_events = clap_output_events { ctx: &mut got as *mut Vec<(u16, u32, f64)> as *mut c_void, try_push: Some(out_push) };
        unsafe {
            if let Some(f) = ext.flush {
                f(self.plugin, &in_events, &out_events);
            }
        }
    }

    fn state(&mut self) -> AState {
        self.flush();
        let mut bytes: Vec<u8> = Vec::new();
        if let Some(ext) = unsafe { self.ext_state.as_ref() } {
            let s = clap_ostream { ctx: &mut bytes as *mut Vec<u8> as *mut c_void, write: Some(ostream_write) };
            unsafe {
                if let Some(save) = ext.save {
                    save(self.plugin, &s);
                }
            }
        }
        AState { component: b64(&bytes), controller: String::new() }
    }

    fn load_state(&mut self, st: &AState) {
        let bytes = unb64(&st.component);
        if bytes.is_empty() {
            return;
        }
        if let Some(ext) = unsafe { self.ext_state.as_ref() } {
            let mut src = (bytes, 0usize);
            let s = clap_istream { ctx: &mut src as *mut (Vec<u8>, usize) as *mut c_void, read: Some(istream_read) };
            unsafe {
                if let Some(load) = ext.load {
                    load(self.plugin, &s);
                }
            }
        }
        self.sent.clear();
        self.pending.clear();
    }

    fn out(&mut self) -> Result<AOut, String> {
        let (restart, latency_changed, rescan, callback, log) = {
            let mut s = lock(&self.host.state);
            (std::mem::take(&mut s.restart), std::mem::take(&mut s.latency_changed), std::mem::take(&mut s.rescan), std::mem::take(&mut s.callback), std::mem::take(&mut s.log))
        };
        if callback {
            unsafe {
                if let Some(cb) = (*self.plugin).on_main_thread {
                    cb(self.plugin);
                }
            }
        }
        if restart || latency_changed {
            self.activate(false)?;
            self.activate(true)?;
        }
        let mut out = AOut { latency: self.latency, tail: self.tail, messages: log, ..Default::default() };
        if rescan {
            out.values = Some(self.values());
        }
        Ok(out)
    }

    fn process(&mut self, p: &AProcess, shm: &mut [u8]) -> Result<AOut, String> {
        let n = p.frames as usize;
        if shm.len() < n * 16 {
            return Err("the audio memory is too small for the block".into());
        }
        if n as u32 > self.block {
            return Err(format!("a block of {n} frames exceeds the {} the plug-in was set up for", self.block));
        }
        let input: &[f32] = bytemuck::cast_slice(&shm[..n * 8]);
        for b in &mut self.bufs {
            b.clear();
            b.resize(n, 0.0);
        }
        for (k, fr) in input.chunks_exact(2).enumerate() {
            (self.bufs[0][k], self.bufs[1][k]) = (fr[0], fr[1]);
        }
        let events: Vec<clap_event_param_value> = self.pending.drain(..).map(|(id, v)| param_event(id, v)).collect();
        let in_events = clap_input_events { ctx: &events as *const Vec<clap_event_param_value> as *mut c_void, size: Some(in_size), get: Some(in_get) };
        let mut got: Vec<(u16, u32, f64)> = Vec::new();
        let out_events = clap_output_events { ctx: &mut got as *mut Vec<(u16, u32, f64)> as *mut c_void, try_push: Some(out_push) };
        let mut in_ch = [self.bufs[0].as_mut_ptr(), self.bufs[1].as_mut_ptr()];
        let mut out_ch = [self.bufs[2].as_mut_ptr(), self.bufs[3].as_mut_ptr()];
        let inputs = clap_audio_buffer { data32: in_ch.as_mut_ptr(), data64: std::ptr::null_mut(), channel_count: 2, latency: 0, constant_mask: 0 };
        let mut outputs = clap_audio_buffer { data32: out_ch.as_mut_ptr(), data64: std::ptr::null_mut(), channel_count: 2, latency: 0, constant_mask: 0 };
        let mut transport: clap_event_transport = unsafe { std::mem::zeroed() };
        transport.header = clap_event_header { size: std::mem::size_of::<clap_event_transport>() as u32, time: 0, space_id: CLAP_CORE_EVENT_SPACE_ID, type_: CLAP_EVENT_TRANSPORT, flags: 0 };
        transport.flags = CLAP_TRANSPORT_HAS_SECONDS_TIMELINE | CLAP_TRANSPORT_HAS_TEMPO | if p.playing { CLAP_TRANSPORT_IS_PLAYING } else { 0 };
        transport.song_pos_seconds = (p.time * (1u64 << 31) as f64) as i64;
        transport.tempo = 120.0;
        let proc_ = clap_process { steady_time: self.steady, frames_count: n as u32, transport: &transport, audio_inputs: &inputs, audio_outputs: &mut outputs, audio_inputs_count: 1, audio_outputs_count: 1, in_events: &in_events, out_events: &out_events };
        let status = unsafe { (*self.plugin).process.ok_or("no process")?(self.plugin, &proc_) };
        if status == CLAP_PROCESS_ERROR {
            return Err("process returned CLAP_PROCESS_ERROR".into());
        }
        self.steady += n as i64;
        let output: &mut [f32] = bytemuck::cast_slice_mut(&mut shm[n * 8..n * 16]);
        for (k, fr) in output.chunks_exact_mut(2).enumerate() {
            (fr[0], fr[1]) = (self.bufs[2][k], self.bufs[3][k]);
        }
        let mut out = self.out()?;
        for (kind, id, v) in got {
            match kind {
                CLAP_EVENT_PARAM_VALUE => {
                    if self.sent.get(&id) != Some(&v) {
                        self.sent.insert(id, v);
                        out.edits.push((id, v));
                    }
                }
                CLAP_EVENT_PARAM_GESTURE_BEGIN => out.gestures.push((id, true)),
                CLAP_EVENT_PARAM_GESTURE_END => out.gestures.push((id, false)),
                _ => {}
            }
        }
        Ok(out)
    }
}
