//! A test plug-in for Milan Pro's CLAP host, written against the raw CLAP C API: "Milan Test CLAP Gain", an audio
//! effect with a Gain parameter (plain 0..2) and a Mute switch, a 32-sample delay line whose latency is reported, and
//! state (the two values).
#![allow(non_upper_case_globals, clippy::missing_safety_doc)]
use std::ffi::{CStr, c_char, c_void};
use std::sync::Mutex;
use clap_sys::entry::clap_plugin_entry;
use clap_sys::events::*;
use clap_sys::ext::audio_ports::*;
use clap_sys::ext::latency::*;
use clap_sys::ext::params::*;
use clap_sys::ext::state::*;
use clap_sys::factory::plugin_factory::*;
use clap_sys::host::clap_host;
use clap_sys::id::clap_id;
use clap_sys::plugin::*;
use clap_sys::process::*;
use clap_sys::stream::*;
use clap_sys::version::CLAP_VERSION;

const DELAY: usize = 32;

const ID: &CStr = c"com.meridiannle.test.clapgain";
const NAME: &CStr = c"Milan Test CLAP Gain";
const VENDOR: &CStr = c"Milan Test";
const URL: &CStr = c"https://github.com/MeridianNLE";
const VERSION: &CStr = c"1.0.0";
const DESC: &CStr = c"A gain with a 32-sample delay, for Milan Pro's host tests";
/// The NUL-terminated feature list (a `static` of raw pointers needs a Sync wrapper).
struct Features([*const c_char; 3]);
unsafe impl Sync for Features {}
static FEATURES: Features = Features([c"audio-effect".as_ptr(), c"utility".as_ptr(), std::ptr::null()]);

static DESCRIPTOR: clap_plugin_descriptor = clap_plugin_descriptor {
    clap_version: CLAP_VERSION,
    id: ID.as_ptr(),
    name: NAME.as_ptr(),
    vendor: VENDOR.as_ptr(),
    url: URL.as_ptr(),
    manual_url: URL.as_ptr(),
    support_url: URL.as_ptr(),
    version: VERSION.as_ptr(),
    description: DESC.as_ptr(),
    features: FEATURES.0.as_ptr(),
};

struct State {
    gain: f64,
    mute: f64,
    lines: [Vec<f32>; 2],
    pos: usize,
}

/// The `clap_plugin` the host holds, with our data behind `plugin_data`.
#[repr(C)]
struct Plugin {
    raw: clap_plugin,
    state: Mutex<State>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

unsafe fn me<'a>(p: *const clap_plugin) -> &'a Plugin {
    unsafe { &*((*p).plugin_data as *const Plugin) }
}

unsafe extern "C" fn init(_p: *const clap_plugin) -> bool {
    true
}

unsafe extern "C" fn destroy(p: *const clap_plugin) {
    unsafe { drop(Box::from_raw((*p).plugin_data as *mut Plugin)) };
}

unsafe extern "C" fn activate(p: *const clap_plugin, _rate: f64, _min: u32, _max: u32) -> bool {
    let mut s = lock(&unsafe { me(p) }.state);
    for l in &mut s.lines {
        l.fill(0.0);
    }
    s.pos = 0;
    true
}

unsafe extern "C" fn deactivate(_p: *const clap_plugin) {}

unsafe extern "C" fn start_processing(_p: *const clap_plugin) -> bool {
    true
}

unsafe extern "C" fn stop_processing(_p: *const clap_plugin) {}

unsafe extern "C" fn reset(p: *const clap_plugin) {
    let mut s = lock(&unsafe { me(p) }.state);
    for l in &mut s.lines {
        l.fill(0.0);
    }
    s.pos = 0;
}

unsafe extern "C" fn process(p: *const clap_plugin, pr: *const clap_process) -> clap_process_status {
    unsafe {
        let pr = &*pr;
        let mut s = lock(&me(p).state);
        if let Some(ev) = pr.in_events.as_ref() {
            let n = ev.size.map_or(0, |f| f(pr.in_events));
            for i in 0..n {
                let h = ev.get.map_or(std::ptr::null(), |g| g(pr.in_events, i));
                if !h.is_null() && (*h).space_id == CLAP_CORE_EVENT_SPACE_ID && (*h).type_ == CLAP_EVENT_PARAM_VALUE {
                    let e = &*(h as *const clap_event_param_value);
                    match e.param_id {
                        0 => s.gain = e.value.clamp(0.0, 2.0),
                        1 => s.mute = e.value.clamp(0.0, 1.0),
                        _ => {}
                    }
                }
            }
        }
        if pr.audio_inputs_count < 1 || pr.audio_outputs_count < 1 {
            return CLAP_PROCESS_CONTINUE;
        }
        let (inp, out) = (&*pr.audio_inputs, &*pr.audio_outputs);
        if inp.channel_count < 2 || out.channel_count < 2 || inp.data32.is_null() || out.data32.is_null() {
            return CLAP_PROCESS_CONTINUE;
        }
        let n = pr.frames_count as usize;
        let g = if s.mute >= 0.5 { 0.0 } else { s.gain as f32 };
        let ic = std::slice::from_raw_parts(inp.data32, 2);
        let oc = std::slice::from_raw_parts(out.data32, 2);
        for k in 0..n {
            for c in 0..2 {
                let x = *ic[c].add(k);
                let pos = s.pos;
                let d = s.lines[c][pos];
                s.lines[c][pos] = x;
                *oc[c].add(k) = d * g;
            }
            s.pos = (s.pos + 1) % DELAY;
        }
    }
    CLAP_PROCESS_CONTINUE
}

unsafe extern "C" fn get_extension(_p: *const clap_plugin, id: *const c_char) -> *const c_void {
    let id = unsafe { CStr::from_ptr(id) };
    if id == CLAP_EXT_PARAMS {
        &PARAMS as *const _ as *const c_void
    } else if id == CLAP_EXT_AUDIO_PORTS {
        &PORTS as *const _ as *const c_void
    } else if id == CLAP_EXT_LATENCY {
        &LATENCY as *const _ as *const c_void
    } else if id == CLAP_EXT_STATE {
        &STATE as *const _ as *const c_void
    } else {
        std::ptr::null()
    }
}

unsafe extern "C" fn on_main_thread(_p: *const clap_plugin) {}

// ---- params

unsafe extern "C" fn params_count(_p: *const clap_plugin) -> u32 {
    2
}

fn put(s: &str, dst: &mut [c_char]) {
    for (d, c) in dst.iter_mut().zip(s.bytes().chain([0])) {
        *d = c as c_char;
    }
    if let Some(l) = dst.last_mut() {
        *l = 0;
    }
}

unsafe extern "C" fn params_info(_p: *const clap_plugin, index: u32, info: *mut clap_param_info) -> bool {
    let info = unsafe { &mut *info };
    info.cookie = std::ptr::null_mut();
    put("", &mut info.module);
    match index {
        0 => {
            info.id = 0;
            info.flags = CLAP_PARAM_IS_AUTOMATABLE;
            put("Gain", &mut info.name);
            (info.min_value, info.max_value, info.default_value) = (0.0, 2.0, 1.0);
        }
        1 => {
            info.id = 1;
            info.flags = CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_IS_STEPPED;
            put("Mute", &mut info.name);
            (info.min_value, info.max_value, info.default_value) = (0.0, 1.0, 0.0);
        }
        _ => return false,
    }
    true
}

unsafe extern "C" fn params_value(p: *const clap_plugin, id: clap_id, out: *mut f64) -> bool {
    let s = lock(&unsafe { me(p) }.state);
    unsafe {
        match id {
            0 => *out = s.gain,
            1 => *out = s.mute,
            _ => return false,
        }
    }
    true
}

unsafe extern "C" fn params_text(_p: *const clap_plugin, id: clap_id, v: f64, buf: *mut c_char, cap: u32) -> bool {
    let s = match id {
        0 => format!("{v:.2} x"),
        1 => (if v >= 0.5 { "Muted" } else { "On" }).to_string(),
        _ => return false,
    };
    let dst = unsafe { std::slice::from_raw_parts_mut(buf, cap as usize) };
    put(&s, dst);
    true
}

unsafe extern "C" fn params_parse(_p: *const clap_plugin, _id: clap_id, _text: *const c_char, _out: *mut f64) -> bool {
    false
}

unsafe extern "C" fn params_flush(_p: *const clap_plugin, _in: *const clap_input_events, _out: *const clap_output_events) {}

static PARAMS: clap_plugin_params = clap_plugin_params { count: Some(params_count), get_info: Some(params_info), get_value: Some(params_value), value_to_text: Some(params_text), text_to_value: Some(params_parse), flush: Some(params_flush) };

// ---- audio ports

unsafe extern "C" fn ports_count(_p: *const clap_plugin, _input: bool) -> u32 {
    1
}

unsafe extern "C" fn ports_get(_p: *const clap_plugin, index: u32, input: bool, info: *mut clap_audio_port_info) -> bool {
    if index != 0 {
        return false;
    }
    let info = unsafe { &mut *info };
    info.id = 0;
    put(if input { "Input" } else { "Output" }, &mut info.name);
    info.flags = CLAP_AUDIO_PORT_IS_MAIN;
    info.channel_count = 2;
    info.port_type = CLAP_PORT_STEREO.as_ptr();
    info.in_place_pair = clap_sys::id::CLAP_INVALID_ID;
    true
}

static PORTS: clap_plugin_audio_ports = clap_plugin_audio_ports { count: Some(ports_count), get: Some(ports_get) };

// ---- latency and state

unsafe extern "C" fn latency_get(_p: *const clap_plugin) -> u32 {
    DELAY as u32
}

static LATENCY: clap_plugin_latency = clap_plugin_latency { get: Some(latency_get) };

unsafe extern "C" fn state_save(p: *const clap_plugin, stream: *const clap_ostream) -> bool {
    let s = lock(&unsafe { me(p) }.state);
    let mut bytes = Vec::new();
    bytes.extend(s.gain.to_le_bytes());
    bytes.extend(s.mute.to_le_bytes());
    unsafe {
        let w = (*stream).write.expect("write");
        w(stream, bytes.as_ptr() as *const c_void, bytes.len() as u64) == bytes.len() as i64
    }
}

unsafe extern "C" fn state_load(p: *const clap_plugin, stream: *const clap_istream) -> bool {
    let mut bytes = [0u8; 16];
    let n = unsafe { (*stream).read.expect("read")(stream, bytes.as_mut_ptr() as *mut c_void, 16) };
    if n != 16 {
        return false;
    }
    let mut s = lock(&unsafe { me(p) }.state);
    s.gain = f64::from_le_bytes(bytes[0..8].try_into().unwrap());
    s.mute = f64::from_le_bytes(bytes[8..16].try_into().unwrap());
    true
}

static STATE: clap_plugin_state = clap_plugin_state { save: Some(state_save), load: Some(state_load) };

// ---- factory and entry

unsafe extern "C" fn plugin_count(_f: *const clap_plugin_factory) -> u32 {
    1
}

unsafe extern "C" fn plugin_descriptor(_f: *const clap_plugin_factory, index: u32) -> *const clap_plugin_descriptor {
    if index == 0 { &DESCRIPTOR } else { std::ptr::null() }
}

unsafe extern "C" fn create_plugin(_f: *const clap_plugin_factory, _host: *const clap_host, id: *const c_char) -> *const clap_plugin {
    if id.is_null() || unsafe { CStr::from_ptr(id) } != ID {
        return std::ptr::null();
    }
    let mut p = Box::new(Plugin {
        raw: clap_plugin {
            desc: &DESCRIPTOR,
            plugin_data: std::ptr::null_mut(),
            init: Some(init),
            destroy: Some(destroy),
            activate: Some(activate),
            deactivate: Some(deactivate),
            start_processing: Some(start_processing),
            stop_processing: Some(stop_processing),
            reset: Some(reset),
            process: Some(process),
            get_extension: Some(get_extension),
            on_main_thread: Some(on_main_thread),
        },
        state: Mutex::new(State { gain: 1.0, mute: 0.0, lines: [vec![0.0; DELAY], vec![0.0; DELAY]], pos: 0 }),
    });
    p.raw.plugin_data = &*p as *const Plugin as *mut c_void;
    let raw: *const clap_plugin = &p.raw;
    std::mem::forget(p);
    raw
}

static FACTORY: clap_plugin_factory = clap_plugin_factory { get_plugin_count: Some(plugin_count), get_plugin_descriptor: Some(plugin_descriptor), create_plugin: Some(create_plugin) };

unsafe extern "C" fn entry_init(_path: *const c_char) -> bool {
    true
}

unsafe extern "C" fn entry_deinit() {}

unsafe extern "C" fn entry_factory(id: *const c_char) -> *const c_void {
    if !id.is_null() && unsafe { CStr::from_ptr(id) } == CLAP_PLUGIN_FACTORY_ID { &FACTORY as *const _ as *const c_void } else { std::ptr::null() }
}

#[allow(non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static clap_entry: clap_plugin_entry = clap_plugin_entry { clap_version: CLAP_VERSION, init: Some(entry_init), deinit: Some(entry_deinit), get_factory: Some(entry_factory) };
