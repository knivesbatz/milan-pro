//! A2-A5: the host's AE-format personality: one `.aex` file (a DLL with one or more effect PiPLs), its effects' global
//! and parameter setup (describe), their instances with sequence setup, resetup, flatten and setdown, and the frame
//! flow (supervision, frame setup, render, frame setdown) over Milan's shared memory. Every call into the plug-in is
//! on this process's one thread, one at a time. Milan reports itself as `'PrMr'` (the documented convention for
//! Premiere Pro and other hosts), serial number 0, and never sends a command it cannot back (no events, no dialogs,
//! no SmartFX, no audio). Declarations: `abi.rs` (Milan's own, with provenance); callbacks: `suites.rs`.
pub mod abi;
pub mod convert;
#[path = "../../../engine/aex/pipl.rs"]
pub mod pipl;
pub mod suites;

use crate::proto::{self, AexInfo, Outcome, PVal, ParamDesc, ParamState, PluginDesc, Render, Reply, Req};
use abi::*;
use std::collections::{BTreeMap, HashMap};
use std::ffi::{c_char, c_void};
use std::fs::File;
use std::path::{Path, PathBuf};
use suites::Added;

/// At most this many instances live in one host; the least recently used is set down first.
const MAX_INSTANCES: usize = 16;
/// Flattened sequence data larger than this is not kept (plan §4.3).
const MAX_BLOB: usize = 16 << 20;
/// Time units per frame: `time_scale` is the frame rate times this, `time_step` this (only the ratio matters).
const STEP: i32 = 1000;

struct Setup {
    global: Handle,
    out_flags: u32,
    out_flags2: u32,
    /// The parameters as added (index 0: the input layer, made by the host).
    defs: Vec<Added>,
    /// Milan's names (`p<id>`) per parameter index ("" for ones Milan does not show or store).
    names: Vec<String>,
    /// A popup's plug-in values (1-based) per Milan choice index.
    popups: HashMap<usize, Vec<i32>>,
    desc: PluginDesc,
}

struct Effect {
    pipl: pipl::Effect,
    entry: Option<Entry>,
    setup: Option<Result<Setup, String>>,
}

struct Instance {
    seq: Handle,
    /// The parameters' values now (index 0: the input layer).
    params: Vec<ParamDef>,
    /// Layer parameters set to None.
    none_layers: Vec<i32>,
    /// What Milan sent last, per name.
    sent: HashMap<String, PVal>,
    /// The run-time state reported last (hidden, enabled), per name.
    state: Vec<ParamState>,
    used: u64,
}

pub struct Module {
    path: PathBuf,
    _lib: libloading::Library,
    fx: Vec<Effect>,
    instances: HashMap<(String, u32), Instance>,
    clock: u64,
    shm: Option<crate::host::Shm>,
}

fn cmd_name(c: i32) -> &'static str {
    match c {
        cmd::GLOBAL_SETUP => "global setup",
        cmd::GLOBAL_SETDOWN => "global setdown",
        cmd::PARAMS_SETUP => "parameter setup",
        cmd::SEQUENCE_SETUP => "sequence setup",
        cmd::SEQUENCE_RESETUP => "sequence resetup",
        cmd::SEQUENCE_FLATTEN => "sequence flatten",
        cmd::SEQUENCE_SETDOWN => "sequence setdown",
        cmd::FRAME_SETUP => "frame setup",
        cmd::RENDER => "render",
        cmd::FRAME_SETDOWN => "frame setdown",
        cmd::USER_CHANGED_PARAM => "user changed parameter",
        cmd::UPDATE_PARAMS_UI => "update parameters UI",
        _ => "a command",
    }
}

fn err_name(e: Err) -> String {
    match e {
        err::OUT_OF_MEMORY => "out of memory".into(),
        err::INTERNAL_STRUCT_DAMAGED => "internal structure damaged".into(),
        err::INVALID_INDEX => "invalid index".into(),
        err::UNRECOGNIZED_PARAM_TYPE => "unrecognized parameter type".into(),
        err::INVALID_CALLBACK => "invalid callback".into(),
        err::BAD_CALLBACK_PARAM => "bad callback parameter".into(),
        err::INTERRUPT_CANCEL => "cancelled".into(),
        e => format!("error {e}"),
    }
}

/// A C string in a fixed array (bounded).
fn fixed_str(b: &[c_char]) -> String {
    let b: Vec<u8> = b.iter().map(|&c| c as u8).take_while(|&c| c != 0).collect();
    String::from_utf8_lossy(&b).into_owned()
}

/// The frame a command is about, for `in_data`.
#[derive(Clone, Copy, Default)]
struct Frame {
    w: i32,
    h: i32,
    full_w: i32,
    full_h: i32,
    /// Current time in plug-in units, and the units per second.
    time: i32,
    scale: u32,
    par: f64,
}

impl Frame {
    fn of(r: &Render) -> Frame {
        let full = |v: u32| ((v as f64 / r.scale.max(1e-3)).round() as i32).max(1);
        let fps = if r.fps > 0.0 { r.fps } else { 30.0 };
        Frame { w: r.w as i32, h: r.h as i32, full_w: full(r.w), full_h: full(r.h), time: (r.time.round() as i32).saturating_mul(STEP), scale: (fps * STEP as f64).round() as u32, par: if r.par > 0.0 { r.par } else { 1.0 } }
    }
}

impl Module {
    pub fn load(binary: &Path) -> Result<Module, String> {
        let bytes = std::fs::read(binary).map_err(|e| format!("cannot read {}: {e}", binary.display()))?;
        let (machine, effects) = pipl::effects_of(&bytes)?;
        if machine != pipl::MACHINE_X64 {
            return Err(format!("not a 64-bit plug-in (machine 0x{machine:04X})"));
        }
        let effects: Vec<pipl::Effect> = effects.into_iter().filter_map(Result::ok).filter(|e| e.kind == "eFKT").collect();
        if effects.is_empty() {
            return Err("it has no After Effects effect PiPL".into());
        }
        // SAFETY: loading runs the plug-in's initialisers; that is what this isolated process is for. Its own folder is
        // searched for the DLLs it imports (never an Adobe application folder: Milan adds none).
        const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 8;
        let lib = match unsafe { libloading::os::windows::Library::load_with_flags(binary, if binary.is_absolute() { LOAD_WITH_ALTERED_SEARCH_PATH } else { 0 }) } {
            Ok(l) => libloading::Library::from(l),
            Err(e) => return Err(format!("cannot load {}: {e}{}", binary.display(), std::error::Error::source(&e).map_or(String::new(), |s| format!(": {s}")))),
        };
        let fx = effects
            .into_iter()
            .map(|e| {
                let name: Vec<u8> = e.entry.bytes().chain([0]).collect();
                // SAFETY: the PiPL names an exported function of this signature (the entry point page).
                let entry = unsafe { lib.get::<Entry>(&name) }.ok().map(|f| *f);
                Effect { pipl: e, entry, setup: None }
            })
            .collect();
        Ok(Module { path: binary.to_path_buf(), _lib: lib, fx, instances: HashMap::new(), clock: 0, shm: None })
    }

    pub fn count(&self) -> u32 {
        self.fx.len() as u32
    }

    fn in_data(&self, index: usize, seq: Handle, f: &Frame, num_params: usize) -> InData {
        let global = match &self.fx[index].setup {
            Some(Ok(s)) => s.global,
            _ => std::ptr::null_mut(),
        };
        let down = |v: i32, full: i32| RationalScale { num: v.max(1), den: full.max(1) as u32 };
        let par = RationalScale { num: (f.par * 1000.0).round() as i32, den: 1000 };
        InData {
            inter: suites::inter(),
            utils: suites::utils(),
            effect_ref: 1 as ProgPtr,
            quality: QUALITY_HI,
            version: SpecVersion { major: SPEC_MAJOR, minor: SPEC_MINOR },
            serial_num: 0,
            appl_id: APPL_ID_PRMR,
            num_params: num_params as i32,
            reserved: 0,
            what_cpu: 0,
            what_fpu: 0,
            current_time: f.time,
            time_step: STEP,
            total_time: (f.scale as i32).saturating_mul(3600),
            local_time_step: STEP,
            time_scale: f.scale,
            field: FIELD_FRAME,
            shutter_angle: 0,
            width: f.full_w,
            height: f.full_h,
            extent_hint: Rect { left: 0, top: 0, right: f.w, bottom: f.h },
            output_origin_x: 0,
            output_origin_y: 0,
            downsample_x: down(f.w, f.full_w),
            downsample_y: down(f.h, f.full_h),
            pixel_aspect_ratio: par,
            in_flags: 0,
            global_data: global,
            sequence_data: seq,
            frame_data: std::ptr::null_mut(),
            start_samp: 0,
            dur_samp: 0,
            total_samp: 0,
            src_snd: SoundWorld { rate: 0.0, num_channels: 0, format: 0, bytes_per_sample: 0, num_samples: 0, data: std::ptr::null_mut() },
            pica_basic: suites::basic(),
            pre_effect_source_origin_x: 0,
            pre_effect_source_origin_y: 0,
            shutter_phase: 0,
        }
    }

    /// Send `c` to effect `index`; its message (if any) goes to `messages`. Err: the plug-in's error, worded.
    #[allow(clippy::too_many_arguments)]
    fn call(&self, index: usize, c: i32, in_data: &mut InData, out: &mut OutData, params: &mut [*mut ParamDef], output: *mut World, extra: *mut c_void, messages: &mut Vec<String>) -> Result<(), String> {
        let Some(entry) = self.fx[index].entry else { return Err(format!("its entry point {} is not exported", self.fx[index].pipl.entry)) };
        out.return_msg = [0; 256];
        // SAFETY: the plug-in's entry point with structures that live across the call.
        let e = unsafe { entry(c, in_data, out, if params.is_empty() { std::ptr::null_mut() } else { params.as_mut_ptr() }, output, extra) };
        let msg = fixed_str(&out.return_msg);
        if !msg.is_empty() {
            log::info!("aex: {}: {}: {msg}", self.fx[index].pipl.match_name, cmd_name(c));
            messages.push(msg.clone());
        }
        if e != err::NONE {
            return Err(format!("its {} failed ({}){}", cmd_name(c), err_name(e), if msg.is_empty() { String::new() } else { format!(": {msg}") }));
        }
        Ok(())
    }

    fn out_data(&self, index: usize, seq: Handle) -> OutData {
        // SAFETY: all-zero is a valid OutData (null handles, empty strings).
        let mut o: OutData = unsafe { std::mem::zeroed() };
        if let Some(Ok(s)) = &self.fx[index].setup {
            o.global_data = s.global;
            o.out_flags = s.out_flags;
            o.out_flags2 = s.out_flags2;
        }
        o.sequence_data = seq;
        o.my_version = self.fx[index].pipl.version;
        o
    }

    /// Global setup and parameter setup, once per effect.
    fn setup(&mut self, index: usize) -> Result<(), String> {
        if index >= self.fx.len() {
            return Err(format!("no effect {index} in this file"));
        }
        if let Some(s) = &self.fx[index].setup {
            return s.as_ref().map(|_| ()).map_err(Clone::clone);
        }
        let r = self.setup_inner(index);
        let res = r.as_ref().map(|_| ()).map_err(Clone::clone);
        self.fx[index].setup = Some(r);
        res
    }

    fn setup_inner(&mut self, index: usize) -> Result<Setup, String> {
        suites::with(|c| *c = Default::default());
        let f = Frame { w: 1920, h: 1080, full_w: 1920, full_h: 1080, scale: 30 * STEP as u32, par: 1.0, ..Default::default() };
        let mut msgs = Vec::new();
        let mut ind = self.in_data(index, std::ptr::null_mut(), &f, 0);
        let mut out = self.out_data(index, std::ptr::null_mut());
        self.call(index, cmd::GLOBAL_SETUP, &mut ind, &mut out, &mut [], std::ptr::null_mut(), std::ptr::null_mut(), &mut msgs)?;
        let (global, out_flags, out_flags2) = (out.global_data, out.out_flags, out.out_flags2);
        let mut setup = Setup { global, out_flags, out_flags2, defs: Vec::new(), names: Vec::new(), popups: HashMap::new(), desc: PluginDesc::default() };
        self.fx[index].setup = Some(Ok(Setup { global, out_flags, out_flags2, defs: Vec::new(), names: Vec::new(), popups: HashMap::new(), desc: PluginDesc::default() }));
        suites::with(|c| c.adding = Some(Vec::new()));
        let mut ind = self.in_data(index, std::ptr::null_mut(), &f, 1);
        let mut out = self.out_data(index, std::ptr::null_mut());
        let r = self.call(index, cmd::PARAMS_SETUP, &mut ind, &mut out, &mut [], std::ptr::null_mut(), std::ptr::null_mut(), &mut msgs);
        let (added, refused) = suites::with(|c| (c.adding.take().unwrap_or_default(), std::mem::take(&mut c.refused)));
        self.fx[index].setup = None;
        r?;
        let mut input = ParamDef { param_type: param::LAYER, ..Default::default() };
        input.name[..5].copy_from_slice(&b"Input".map(|b| b as c_char));
        setup.defs = std::iter::once(Added { def: input, names: String::new() }).chain(added).collect();
        let e = &self.fx[index].pipl;
        let mut caps = crate_caps(e.out_flags, e.out_flags2);
        if (out_flags, out_flags2) != (e.out_flags, e.out_flags2) {
            caps.push(format!("its flags differ from its PiPL (0x{out_flags:08X}/0x{out_flags2:08X} at setup, 0x{:08X}/0x{:08X} in the PiPL)", e.out_flags, e.out_flags2));
        }
        let (params, names, popups) = describe_params(&setup.defs);
        if setup.defs.iter().skip(1).any(|a| a.def.param_type == param::ARBITRARY_DATA || a.def.param_type == param::CUSTOM) {
            caps.push("custom data parameters: kept at their defaults".into());
        }
        setup.names = names;
        setup.popups = popups;
        setup.desc = PluginDesc {
            index: index as u32,
            id: e.match_name.clone(),
            major: (e.version >> 19) & 0x1fff,
            minor: (e.version >> 15) & 0xf,
            label: e.name.clone(),
            group: e.category.clone(),
            depths: vec![proto::Depth::Byte.ofx().to_string()],
            params,
            aex: Some(AexInfo {
                match_name: e.match_name.clone(),
                entry: e.entry.clone(),
                pipl: index as u32,
                spec: e.spec_version,
                version: e.version,
                out_flags,
                out_flags2,
                described: true,
                caps,
                refused: refused.into_iter().collect(),
                message: msgs.join(" "),
                ..Default::default()
            }),
            ..Default::default()
        };
        Ok(setup)
    }

    pub fn describe(&mut self, index: u32) -> Reply {
        match self.setup(index as usize) {
            Ok(()) => match &self.fx[index as usize].setup {
                Some(Ok(s)) => Reply::Described(Box::new(s.desc.clone())),
                _ => Reply::Error("no description".into()),
            },
            Err(e) => Reply::Error(e),
        }
    }

    // ------------------------------------------------------------ instances

    fn instance(&mut self, r: &Render, msgs: &mut Vec<String>) -> Result<(), String> {
        let key = (r.instance.clone(), r.index);
        self.clock += 1;
        if let Some(i) = self.instances.get_mut(&key) {
            i.used = self.clock;
            return Ok(());
        }
        if self.instances.len() >= MAX_INSTANCES {
            if let Some(k) = self.instances.iter().min_by_key(|(_, i)| i.used).map(|(k, _)| k.clone()) {
                let i = self.instances.remove(&k).expect("listed");
                self.setdown(k.1 as usize, i);
            }
        }
        let index = r.index as usize;
        let Some(Ok(s)) = &self.fx[index].setup else { return Err("not set up".into()) };
        // After setup the id slot holds change flags (`uu`), cleared.
        let params: Vec<ParamDef> = s.defs.iter().map(|a| ParamDef { uu: 0, ..a.def }).collect();
        let f = Frame::of(r);
        let flat = r.blob.get("seq").map(|b| proto::unb64(b)).filter(|b| !b.is_empty());
        let (c, seq_in) = match &flat {
            Some(b) => (cmd::SEQUENCE_RESETUP, suites::handle_from(b)),
            None => (cmd::SEQUENCE_SETUP, std::ptr::null_mut()),
        };
        let mut ind = self.in_data(index, seq_in, &f, params.len());
        let mut out = self.out_data(index, seq_in);
        self.call(index, c, &mut ind, &mut out, &mut [], std::ptr::null_mut(), std::ptr::null_mut(), msgs)?;
        let seq = out.sequence_data;
        if !seq_in.is_null() && seq != seq_in {
            // The plug-in made its own data from the flat copy (and may have disposed of it already).
            suites::dispose_handle(seq_in);
        }
        self.instances.insert(key, Instance { seq, params, none_layers: Vec::new(), sent: HashMap::new(), state: Vec::new(), used: self.clock });
        Ok(())
    }

    fn setdown(&self, index: usize, i: Instance) {
        let mut ind = self.in_data(index, i.seq, &Frame { w: 1, h: 1, full_w: 1, full_h: 1, scale: 30 * STEP as u32, par: 1.0, ..Default::default() }, i.params.len());
        let mut out = self.out_data(index, i.seq);
        let _ = self.call(index, cmd::SEQUENCE_SETDOWN, &mut ind, &mut out, &mut [], std::ptr::null_mut(), std::ptr::null_mut(), &mut Vec::new());
        if suites::is_handle(out.sequence_data) {
            suites::dispose_handle(out.sequence_data);
        }
    }

    /// The instance's sequence data, flattened (plan §4.2): SEQUENCE_FLATTEN when the plug-in asks for it, then
    /// SEQUENCE_RESETUP from the flat copy so it keeps running; else the data as it is.
    pub fn flatten(&mut self, index: u32, instance: &str) -> Reply {
        let key = (instance.to_string(), index);
        let Some(inst) = self.instances.get(&key) else { return Reply::Rendered(Outcome::default()) };
        let i = index as usize;
        let Some(Ok(s)) = &self.fx[i].setup else { return Reply::Error("not set up".into()) };
        let mut msgs = Vec::new();
        let f = Frame { w: 1, h: 1, full_w: 1, full_h: 1, scale: 30 * STEP as u32, par: 1.0, ..Default::default() };
        let n = inst.params.len();
        let bytes = if s.out_flags & out_flag::SEQUENCE_DATA_NEEDS_FLATTENING != 0 {
            let mut ind = self.in_data(i, inst.seq, &f, n);
            let mut out = self.out_data(i, inst.seq);
            if let Err(e) = self.call(i, cmd::SEQUENCE_FLATTEN, &mut ind, &mut out, &mut [], std::ptr::null_mut(), std::ptr::null_mut(), &mut msgs) {
                return Reply::Error(e);
            }
            let flat = out.sequence_data;
            let bytes = suites::handle_bytes(flat).unwrap_or_default();
            // Running again from the flat copy.
            let mut ind = self.in_data(i, flat, &f, n);
            let mut out = self.out_data(i, flat);
            if let Err(e) = self.call(i, cmd::SEQUENCE_RESETUP, &mut ind, &mut out, &mut [], std::ptr::null_mut(), std::ptr::null_mut(), &mut msgs) {
                self.instances.remove(&key);
                return Reply::Error(e);
            }
            if out.sequence_data != flat && suites::is_handle(flat) {
                suites::dispose_handle(flat);
            }
            self.instances.get_mut(&key).expect("present").seq = out.sequence_data;
            bytes
        } else {
            suites::handle_bytes(inst.seq).unwrap_or_default()
        };
        let mut blob = BTreeMap::new();
        if !bytes.is_empty() && bytes.len() <= MAX_BLOB {
            blob.insert("seq".to_string(), proto::b64(&bytes));
        }
        Reply::Rendered(Outcome { blob: Some(blob), messages: msgs, ..Default::default() })
    }

    // ------------------------------------------------------------ frames

    pub fn render(&mut self, r: &Render, pipe: &mut File) -> Reply {
        match self.render_inner(r, pipe) {
            Ok(o) => Reply::Rendered(o),
            Err(e) => Reply::Error(e),
        }
    }

    fn render_inner(&mut self, r: &Render, pipe: &mut File) -> Result<Outcome, String> {
        let index = r.index as usize;
        self.setup(index)?;
        let mut msgs = Vec::new();
        suites::with(|c| *c = Default::default());
        self.instance(r, &mut msgs)?;
        let key = (r.instance.clone(), r.index);
        let f = Frame::of(r);
        let Some(Ok(s)) = &self.fx[index].setup else { return Err("not set up".into()) };
        let (out_flags, n_params) = (s.out_flags, s.defs.len());
        // Values: Milan's, into the definitions; a supervised one that changed is announced to the plug-in.
        let mut inst = self.instances.remove(&key).expect("made");
        let fresh = inst.sent.is_empty();
        let mut changed = Vec::new();
        for (name, v) in &r.params {
            let Some(i) = s.names.iter().position(|n| n == name && !n.is_empty()) else { continue };
            set_value(&mut inst.params[i], v, s.popups.get(&i), &f);
            if s.defs[i].def.param_type == param::LAYER {
                let none = matches!(v, PVal::N(x) if x.first().copied().unwrap_or(0.0) < 0.5);
                inst.none_layers.retain(|&k| k != i as i32);
                if none {
                    inst.none_layers.push(i as i32);
                }
            }
            if inst.sent.get(name) != Some(v) {
                if !fresh && s.defs[i].def.flags & param_flag::SUPERVISE != 0 {
                    changed.push(i);
                }
                inst.sent.insert(name.clone(), v.clone());
            }
        }
        // Frame worlds: the input and the output (separate buffers).
        let (w, h) = (r.w as usize, r.h as usize);
        let n = w * h * 4;
        let mem = match &mut self.shm {
            Some(m) if m.size >= r.shm_size && m.name == r.shm => m,
            _ => match crate::host::Shm::open(&r.shm, r.shm_size) {
                Ok(m) => self.shm.insert(m),
                Err(e) => {
                    self.instances.insert(key, inst);
                    return Err(e);
                }
            },
        };
        if mem.bytes().len() < r.out as usize + n {
            self.instances.insert(key, inst);
            return Err("the frame memory is too small".into());
        }
        let raw: Vec<u8> = if r.inputs > 0 { mem.bytes()[..n].to_vec() } else { vec![0; n] };
        let (shm_size, shm_ptr) = (mem.bytes().len(), mem.bytes().as_mut_ptr() as usize);
        let rb = suites::rowbytes(f.w) as usize;
        let mut input = convert::to_argb(&raw, w, h, rb);
        let mut output = vec![0u8; rb * h];
        let mut in_world = suites::world(input.as_mut_ptr(), f.w, f.h, rb as i32, false);
        let mut out_world = suites::world(output.as_mut_ptr(), f.w, f.h, rb as i32, true);
        inst.params[0].u.ld = in_world;
        let mut ptrs: Vec<*mut ParamDef> = inst.params.iter_mut().map(|p| p as *mut ParamDef).collect();
        let mut edits = Vec::new();
        // Supervision: one USER_CHANGED_PARAM per changed supervised parameter; values the plug-in changed come back.
        for &i in &changed {
            for p in inst.params.iter_mut() {
                p.uu = 0;
            }
            let mut extra = UserChangedParamExtra { param_index: i as i32 };
            let mut ind = self.in_data(index, inst.seq, &f, n_params);
            let mut out = self.out_data(index, inst.seq);
            self.call(index, cmd::USER_CHANGED_PARAM, &mut ind, &mut out, &mut ptrs, &mut out_world, &mut extra as *mut _ as *mut c_void, &mut msgs)?;
            inst.seq = out.sequence_data;
            for (k, p) in inst.params.iter().enumerate().skip(1) {
                if p.uu & CHANGED_VALUE != 0 && !s.names[k].is_empty() {
                    let v = get_value(p, s.popups.get(&k), &f);
                    inst.sent.insert(s.names[k].clone(), v.clone());
                    edits.push((s.names[k].clone(), v));
                }
            }
        }
        if out_flags & out_flag::SEND_UPDATE_PARAMS_UI != 0 || !changed.is_empty() || fresh {
            suites::with(|c| c.ui_updates.clear());
            let mut ind = self.in_data(index, inst.seq, &f, n_params);
            let mut out = self.out_data(index, inst.seq);
            if out_flags & out_flag::SEND_UPDATE_PARAMS_UI != 0 {
                self.call(index, cmd::UPDATE_PARAMS_UI, &mut ind, &mut out, &mut ptrs, &mut out_world, std::ptr::null_mut(), &mut msgs)?;
            }
            let updates = suites::with(|c| std::mem::take(&mut c.ui_updates));
            for (i, d) in updates {
                if let Some(slot) = inst.params.get_mut(i as usize) {
                    slot.ui_flags = d.ui_flags;
                }
            }
        }
        let state = param_state(s, &inst.params);
        let state_changed = state != inst.state;
        inst.state = state.clone();
        // The frame.
        suites::with(|c| {
            c.params = inst.params.clone();
            c.frame = Some((f.w, f.h, f.time, STEP));
            c.none_layers = inst.none_layers.clone();
            c.input = convert::to_argb(&raw, w, h, w * 4);
        });
        let offset = r.out as usize + n;
        let pipe_ptr = pipe as *mut File;
        let source: Box<suites::FrameSource<'static>> = Box::new(move |layer: i32, at: f64| {
            // A nested request to Milan: it writes that frame after the output in shared memory.
            if offset + n > shm_size {
                return None;
            }
            // SAFETY: the pipe outlives the render (cleared below before returning).
            let pipe = unsafe { &mut *pipe_ptr };
            proto::write_msg(pipe, &Reply::NeedFrame { layer, offset: at }).ok()?;
            match proto::read_msg::<Req>(pipe).ok()? {
                Req::Frame(true) => {
                    // SAFETY: the mapping holds `shm_size` bytes.
                    let mem = unsafe { std::slice::from_raw_parts(shm_ptr as *const u8, shm_size) };
                    Some(convert::to_argb(&mem[offset..offset + n], w, h, w * 4))
                }
                _ => None,
            }
        });
        suites::SOURCE.with(|s| *s.borrow_mut() = Some(source));
        let mut ind = self.in_data(index, inst.seq, &f, n_params);
        let mut out = self.out_data(index, inst.seq);
        let res = (|| {
            self.call(index, cmd::FRAME_SETUP, &mut ind, &mut out, &mut ptrs, &mut out_world, std::ptr::null_mut(), &mut msgs)?;
            self.call(index, cmd::RENDER, &mut ind, &mut out, &mut ptrs, &mut out_world, std::ptr::null_mut(), &mut msgs)?;
            self.call(index, cmd::FRAME_SETDOWN, &mut ind, &mut out, &mut ptrs, &mut out_world, std::ptr::null_mut(), &mut msgs)
        })();
        suites::SOURCE.with(|s| *s.borrow_mut() = None);
        inst.seq = out.sequence_data;
        let (refused, checkouts) = suites::with(|c| {
            c.checked.clear();
            (std::mem::take(&mut c.refused), c.checkouts)
        });
        self.instances.insert(key, inst);
        if !refused.is_empty() {
            msgs.push(format!("Milan Pro does not provide: {}", refused.into_iter().collect::<Vec<_>>().join(", ")));
        }
        res?;
        let _ = (&mut in_world, checkouts);
        let mem = self.shm.as_mut().expect("mapped");
        convert::from_argb(&output, w, h, rb, &mut mem.bytes()[r.out as usize..r.out as usize + n]);
        Ok(Outcome { edits, initial: false, state: state_changed.then_some(state), messages: msgs, blob: None })
    }

    pub fn shutdown(&mut self) {
        let all: Vec<((String, u32), Instance)> = self.instances.drain().collect();
        for (k, i) in all {
            self.setdown(k.1 as usize, i);
        }
        for index in 0..self.fx.len() {
            if let Some(Ok(s)) = &self.fx[index].setup {
                let n = s.defs.len();
                let mut ind = self.in_data(index, std::ptr::null_mut(), &Frame { w: 1, h: 1, full_w: 1, full_h: 1, scale: 30 * STEP as u32, par: 1.0, ..Default::default() }, n);
                let mut out = self.out_data(index, std::ptr::null_mut());
                let _ = self.call(index, cmd::GLOBAL_SETDOWN, &mut ind, &mut out, &mut [], std::ptr::null_mut(), std::ptr::null_mut(), &mut Vec::new());
            }
        }
        let _ = &self.path;
    }
}

/// The capability line from the flags (as Milan's scan words it; repeated here so the host's reply is complete).
fn crate_caps(out_flags: u32, out_flags2: u32) -> Vec<String> {
    let mut v = vec!["8-bit only".to_string()];
    if out_flags2 & out_flag2::SUPPORTS_SMART_RENDER != 0 {
        v.push("SmartFX (Milan Pro sends the classic render command only; a SmartFX-only effect will not render)".into());
    }
    if out_flags & out_flag::CUSTOM_UI != 0 {
        v.push("custom controls: generic controls only".into());
    }
    if out_flags & out_flag::I_DO_DIALOG != 0 {
        v.push("an options dialog Milan Pro does not open".into());
    }
    if out_flags2 & (out_flag2::I_USE_3D_CAMERA | out_flag2::I_USE_3D_LIGHTS) != 0 {
        v.push("composition camera or lights, which Milan Pro does not have".into());
    }
    if out_flags2 & out_flag2::SUPPORTS_GPU_RENDER_F32 != 0 {
        v.push("GPU rendering: its CPU path is used".into());
    }
    if out_flags & out_flag::I_EXPAND_BUFFER != 0 {
        v.push("may grow the frame: the output is cropped to it".into());
    }
    v
}

// ---------------------------------------------------------------- parameters (plan §4.3)

/// The ParamDesc kinds Milan's registry understands (the OFX names), per parameter, and Milan's names (`p<id>`, ""
/// for parameters Milan neither shows nor stores) and the popups' value maps.
fn describe_params(defs: &[Added]) -> (Vec<ParamDesc>, Vec<String>, HashMap<usize, Vec<i32>>) {
    let mut out = Vec::new();
    let mut names = vec![String::new(); defs.len()];
    let mut popups = HashMap::new();
    let mut groups: Vec<String> = Vec::new();
    let mut used = std::collections::HashSet::new();
    for (i, a) in defs.iter().enumerate().skip(1) {
        let d = &a.def;
        let id = d.uu;
        let name = if id > 0 && used.insert(id) { format!("p{id}") } else { format!("i{i}") };
        let label = fixed_str(&d.name);
        let parent = groups.last().cloned().unwrap_or_default();
        let secret = d.ui_flags & (pui::INVISIBLE | pui::NO_ECW_UI) != 0;
        let enabled = d.ui_flags & pui::DISABLED == 0;
        let base = ParamDesc { name: name.clone(), label: label.clone(), parent, secret, enabled, ..Default::default() };
        // SAFETY: the union member that `param_type` says is set.
        let desc = unsafe {
            match d.param_type {
                param::FLOAT_SLIDER => {
                    let s = d.u.fs_d;
                    let pct = s.display_flags & DISPLAY_PERCENT != 0;
                    Some(ParamDesc { kind: "Double".into(), label: if pct { format!("{label} (%)") } else { label }, default: vec![s.dephault as f64], min: s.slider_min as f64, max: s.slider_max as f64, vmin: s.valid_min as f64, vmax: s.valid_max as f64, ..base })
                }
                param::FIX_SLIDER => {
                    let s = d.u.fd;
                    Some(ParamDesc { kind: "Double".into(), default: vec![unfixed(s.dephault)], min: unfixed(s.slider_min), max: unfixed(s.slider_max), vmin: unfixed(s.valid_min), vmax: unfixed(s.valid_max), ..base })
                }
                param::SLIDER => {
                    let s = d.u.sd;
                    Some(ParamDesc { kind: "Integer".into(), default: vec![s.dephault as f64], min: s.slider_min as f64, max: s.slider_max as f64, vmin: s.valid_min as f64, vmax: s.valid_max as f64, ..base })
                }
                param::ANGLE => {
                    let s = d.u.ad;
                    let (lo, hi) = if s.valid_max > s.valid_min { (unfixed(s.valid_min), unfixed(s.valid_max)) } else { (-3600.0, 3600.0) };
                    Some(ParamDesc { kind: "Double".into(), label: format!("{label} (°)"), default: vec![unfixed(s.dephault)], min: lo.max(-360.0), max: hi.min(360.0), vmin: lo, vmax: hi, ..base })
                }
                param::CHECKBOX => Some(ParamDesc { kind: "Boolean".into(), default: vec![(d.u.bd.dephault != 0) as u8 as f64], ..base }),
                param::COLOR => {
                    let c = d.u.cd.dephault;
                    Some(ParamDesc { kind: "RGB".into(), default: vec![c.red as f64 / 255.0, c.green as f64 / 255.0, c.blue as f64 / 255.0], ..base })
                }
                param::POINT => {
                    let p = d.u.td;
                    Some(ParamDesc { kind: "Double2D".into(), label: format!("{label} (%)"), default: vec![unfixed(p.x_dephault), unfixed(p.y_dephault)], min: 0.0, max: 100.0, ..base })
                }
                param::POPUP => {
                    let p = d.u.pd;
                    let items: Vec<&str> = a.names.split('|').take(p.num_choices.max(0) as usize).collect();
                    let mut map = Vec::new();
                    let mut options = Vec::new();
                    for (k, it) in items.iter().enumerate() {
                        if it.trim() != "(-" {
                            map.push(k as i32 + 1);
                            options.push(it.trim().to_string());
                        }
                    }
                    let default = map.iter().position(|&v| v == p.dephault as i32).unwrap_or(0);
                    popups.insert(i, map);
                    Some(ParamDesc { kind: "Choice".into(), options, default: vec![default as f64], ..base })
                }
                param::LAYER => Some(ParamDesc { kind: "Choice".into(), options: vec!["None".into(), "This event".into()], default: vec![0.0], ..base }),
                param::GROUP_START => {
                    groups.push(name.clone());
                    Some(ParamDesc { kind: "Group".into(), open: d.flags & param_flag::START_COLLAPSED == 0, ..base })
                }
                param::GROUP_END => {
                    groups.pop();
                    None
                }
                // Buttons, custom data, paths and the rest: not shown, not stored (plan §4.3).
                _ => None,
            }
        };
        if let Some(p) = desc {
            if p.kind != "Group" {
                names[i] = name;
            }
            out.push(p);
        }
    }
    (out, names, popups)
}

/// Milan's value `v` into definition `d` (the frame's size for points, the popup's value map).
fn set_value(d: &mut ParamDef, v: &PVal, popup: Option<&Vec<i32>>, f: &Frame) {
    let PVal::N(x) = v else { return };
    let x0 = x.first().copied().unwrap_or(0.0);
    // Writing a union member is safe; `param_type` says which one the plug-in reads.
    {
        match d.param_type {
            param::FLOAT_SLIDER => d.u.fs_d.value = x0,
            param::FIX_SLIDER => d.u.fd.value = fixed(x0),
            param::SLIDER => d.u.sd.value = x0.round() as i32,
            param::ANGLE => d.u.ad.value = fixed(x0),
            param::CHECKBOX => d.u.bd.value = (x0 != 0.0) as i32,
            param::COLOR if x.len() >= 3 => {
                let c = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                d.u.cd.value = Pixel { alpha: 255, red: c(x[0]), green: c(x[1]), blue: c(x[2]) };
            }
            param::POINT if x.len() >= 2 => {
                // Percent of the frame to layer pixels at the render's size.
                d.u.td.x_value = fixed(x[0] / 100.0 * f.w as f64);
                d.u.td.y_value = fixed(x[1] / 100.0 * f.h as f64);
            }
            param::POPUP => d.u.pd.value = popup.and_then(|m| m.get(x0.max(0.0) as usize).copied()).unwrap_or(1),
            _ => {}
        }
    }
}

/// Definition `d`'s value as Milan stores it (the inverse of `set_value`).
fn get_value(d: &ParamDef, popup: Option<&Vec<i32>>, f: &Frame) -> PVal {
    // SAFETY: the union member that `param_type` says is set.
    unsafe {
        PVal::N(match d.param_type {
            param::FLOAT_SLIDER => vec![d.u.fs_d.value],
            param::FIX_SLIDER => vec![unfixed(d.u.fd.value)],
            param::SLIDER => vec![d.u.sd.value as f64],
            param::ANGLE => vec![unfixed(d.u.ad.value)],
            param::CHECKBOX => vec![(d.u.bd.value != 0) as u8 as f64],
            param::COLOR => {
                let c = d.u.cd.value;
                vec![c.red as f64 / 255.0, c.green as f64 / 255.0, c.blue as f64 / 255.0]
            }
            param::POINT => vec![unfixed(d.u.td.x_value) / f.w.max(1) as f64 * 100.0, unfixed(d.u.td.y_value) / f.h.max(1) as f64 * 100.0],
            param::POPUP => vec![popup.and_then(|m| m.iter().position(|&v| v == d.u.pd.value)).unwrap_or(0) as f64],
            _ => vec![0.0],
        })
    }
}

/// Every shown parameter's hidden and enabled state now (the FX window follows it).
fn param_state(s: &Setup, params: &[ParamDef]) -> Vec<ParamState> {
    s.desc
        .params
        .iter()
        .filter_map(|p| {
            let i = s.names.iter().position(|n| *n == p.name).or_else(|| (p.kind == "Group").then(|| s.defs.iter().position(|a| format!("p{}", a.def.uu) == p.name)).flatten())?;
            let d = params.get(i)?;
            Some(ParamState { name: p.name.clone(), secret: d.ui_flags & (pui::INVISIBLE | pui::NO_ECW_UI) != 0, enabled: d.ui_flags & pui::DISABLED == 0, label: p.label.clone(), options: p.options.clone() })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(kind: i32, id: i32, name: &str) -> ParamDef {
        let mut d = ParamDef { param_type: kind, uu: id, ..Default::default() };
        for (k, b) in name.bytes().enumerate() {
            d.name[k] = b as c_char;
        }
        d
    }

    /// A2: each parameter type maps to a Milan control and its values go both ways (popup separators dropped, points
    /// as percent of the frame, angles in degrees).
    #[test]
    fn parameters_map_both_ways_aex() {
        let mut fs = def(param::FLOAT_SLIDER, 3, "Gain");
        fs.u.fs_d = FloatSliderDef { value: 1.0, dephault: 1.0, valid_min: 0.0, valid_max: 4.0, slider_min: 0.0, slider_max: 2.0, display_flags: DISPLAY_PERCENT, ..Default::default() };
        let mut pd = def(param::POPUP, 4, "Mode");
        pd.u.pd = PopupDef { value: 3, num_choices: 4, dephault: 4, names: std::ptr::null() };
        let mut pt = def(param::POINT, 5, "Center");
        pt.u.td = PointDef { x_dephault: fixed(25.0), y_dephault: fixed(75.0), ..Default::default() };
        let mut an = def(param::ANGLE, 6, "Angle");
        an.u.ad = AngleDef { dephault: fixed(45.0), ..Default::default() };
        let mut cb = def(param::CHECKBOX, 7, "On");
        cb.u.bd = CheckBoxDef { value: 0, dephault: 1, reserved: 0, reserved1: 0, name: std::ptr::null() };
        let gs = def(param::GROUP_START, 8, "Group");
        let ge = def(param::GROUP_END, 9, "");
        let bt = def(param::BUTTON, 10, "Go");
        let lay = def(param::LAYER, 0, "Other");
        let a = |d: ParamDef, n: &str| Added { def: d, names: n.into() };
        let defs = vec![a(def(param::LAYER, 0, "Input"), ""), a(gs, ""), a(fs, ""), a(pd, "A|(-|B|C|ignored"), a(ge, ""), a(pt, ""), a(an, ""), a(cb, ""), a(bt, ""), a(lay, "")];
        let (descs, names, popups) = describe_params(&defs);
        let kinds: Vec<(&str, &str, &str)> = descs.iter().map(|d| (d.name.as_str(), d.kind.as_str(), d.parent.as_str())).collect();
        assert_eq!(kinds, [("p8", "Group", ""), ("p3", "Double", "p8"), ("p4", "Choice", "p8"), ("p5", "Double2D", ""), ("p6", "Double", ""), ("p7", "Boolean", ""), ("i9", "Choice", "")]);
        assert_eq!(names, ["", "", "p3", "p4", "", "p5", "p6", "p7", "", "i9"]);
        assert_eq!(descs[1].label, "Gain (%)");
        assert_eq!((descs[1].min, descs[1].max, descs[1].vmin, descs[1].vmax), (0.0, 2.0, 0.0, 4.0));
        assert_eq!((descs[2].options.clone(), descs[2].default.clone()), (vec!["A".to_string(), "B".into(), "C".into()], vec![2.0]));
        assert_eq!(descs[3].default, [25.0, 75.0]);
        assert_eq!((descs[4].default.clone(), descs[5].default.clone()), (vec![45.0], vec![1.0]));
        let f = Frame { w: 200, h: 100, ..Default::default() };
        let mut d = defs[3].def;
        set_value(&mut d, &PVal::N(vec![1.0]), popups.get(&3), &f);
        assert_eq!(unsafe { d.u.pd.value }, 3, "B is the third entry (the separator counts)");
        assert_eq!(get_value(&d, popups.get(&3), &f), PVal::N(vec![1.0]));
        let mut p = defs[5].def;
        set_value(&mut p, &PVal::N(vec![50.0, 10.0]), None, &f);
        assert_eq!(unsafe { (p.u.td.x_value, p.u.td.y_value) }, (fixed(100.0), fixed(10.0)));
        assert_eq!(get_value(&p, None, &f), PVal::N(vec![50.0, 10.0]));
        let mut g = defs[2].def;
        set_value(&mut g, &PVal::N(vec![1.75]), None, &f);
        assert_eq!(get_value(&g, None, &f), PVal::N(vec![1.75]));
    }
}
