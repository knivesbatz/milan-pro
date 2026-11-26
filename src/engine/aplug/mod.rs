//! R34a, R34b, R35: VST3 and CLAP audio effects. `ofx::scan` finds and describes them (in a host process, with the
//! OFX plug-ins), this module turns each described effect into a registry entry the Audio FX chooser, the FX window,
//! keyframes, automation and projects use like a built-in (`plugins::find`), and runs it through the plug-in host
//! (`client`: a host process per module, blocks of frames in shared memory) as an `afx::AudioFx` node of the mixer's
//! chains. A plug-in that crashes, hangs or fails is bypassed for the session with a message; its settings stay in
//! the project.
//!
//! Instances are stored as `vst3:<class id>` or `clap:<id>` with one key per parameter, `p<id>`: a VST3 value as the
//! API has it (normalized 0..=1; a stepped parameter as its step index, a switch as 0/1), a CLAP value plain. The
//! plug-in's own state (what is not a parameter) is kept as base64 in `Fx.text["state"]` (and `"cstate"` for a VST3
//! controller): a project that uses a plug-in that is missing keeps all of it and shows the FX as missing.
//! Latency is reported per plug-in (`latency`) and the mixer compensates it (`mix::Lat`), sample-exact.
pub mod preset;

pub use crate::engine::ofx::proto::EditorOp;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, RwLock};
use crate::engine::afx::AudioFx;
use crate::engine::ofx::client::{self, Conn, Shm, lock};
use crate::engine::ofx::proto::{AInstance, AOut, AParam, AProcess, AState, AudioDesc, EditorState, Reply, Req, b64, unb64};
use crate::engine::ofx::{Entry, OfxPrefs, ScanCache, Status};
use crate::engine::plugins::{FxEval, ParamKind, ParamSpec, PluginInfo, PluginKind, Preset};
use crate::project::Fx;

pub const VST3: &str = "vst3:";
pub const CLAP: &str = "clap:";
/// The block the host is set up for; longer buffers go through in pieces.
pub const BLOCK: usize = 1024;
/// `Fx.text` keys of the plug-in's own state.
pub const STATE_KEY: &str = "state";
pub const CSTATE_KEY: &str = "cstate";

pub fn is_ext(id: &str) -> bool {
    id.starts_with(VST3) || id.starts_with(CLAP)
}

/// One audio effect Milan Pro can run.
#[derive(Debug)]
pub struct Plugin {
    pub info: &'static PluginInfo,
    pub binary: PathBuf,
    pub desc: AudioDesc,
    /// The parameters shown and stored: (parameter, its control in `info.params`).
    specs: Vec<(AParam, usize)>,
}

impl Plugin {
    pub fn is_vst3(&self) -> bool {
        self.desc.format == "vst3"
    }

    /// The host's id (`AudioDesc::id`) of this effect.
    fn class(&self) -> &str {
        &self.desc.id
    }

    /// The plug-in's values of `e`.
    fn values(&self, e: &FxEval) -> Vec<(u32, f64)> {
        self.specs.iter().map(|(p, i)| (p.id, to_plug(p, e.f(self.info.params[*i].key)))).collect()
    }

    /// The values of `fx` that it holds itself (a parameter without a stored value stays as the plug-in's state has
    /// it: a loaded preset's, a saved project's).
    fn stored_values(&self, fx: &Fx) -> Vec<(u32, f64)> {
        let Some(e) = crate::engine::plugins::eval(fx, 0.0) else { return Vec::new() };
        self.specs.iter().filter(|(_, i)| fx.params.contains_key(self.info.params[*i].key)).map(|(p, i)| (p.id, to_plug(p, e.f(self.info.params[*i].key)))).collect()
    }

    /// Every parameter of `vals` as (control key, Milan value).
    pub fn to_keys(&self, vals: &[(u32, f64)]) -> Vec<(String, f32)> {
        vals.iter().filter_map(|(id, v)| self.specs.iter().find(|(p, _)| p.id == *id).map(|(p, i)| (self.info.params[*i].key.to_string(), to_milan(p, *v)))).collect()
    }
}

/// A plug-in value as Milan stores it.
fn to_milan(p: &AParam, v: f64) -> f32 {
    if p.steps >= 1 && !p.options.is_empty() || p.steps == 1 {
        // A step index: VST3 normalized steps, CLAP plain steps from min.
        if p.max <= 1.0 && p.min == 0.0 && p.steps > 1 && p.max - p.min < p.steps as f64 { ((v * p.steps as f64).round().clamp(0.0, p.steps as f64)) as f32 } else { ((v - p.min).round().clamp(0.0, p.steps as f64)) as f32 }
    } else {
        v as f32
    }
}

/// A Milan value as the plug-in takes it.
fn to_plug(p: &AParam, v: f32) -> f64 {
    if p.steps >= 1 && !p.options.is_empty() || p.steps == 1 {
        let i = (v as f64).round().clamp(0.0, p.steps as f64);
        if p.max <= 1.0 && p.min == 0.0 && p.steps > 1 && p.max - p.min < p.steps as f64 { i / p.steps as f64 } else { p.min + i }
    } else {
        (v as f64).clamp(p.min, p.max)
    }
}

#[derive(Default)]
struct Registry {
    usable: HashMap<String, Arc<Plugin>>,
    entries: Vec<Entry>,
}

static REG: LazyLock<RwLock<Arc<Registry>>> = LazyLock::new(Default::default);
static FAILED: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);
static EVENTS: LazyLock<Mutex<Vec<String>>> = LazyLock::new(Default::default);
static GEN: AtomicU64 = AtomicU64::new(1);
/// What plug-ins changed themselves (their editors), for the app to write into the project (`take_edits`).
static EDITS: LazyLock<Mutex<Vec<Edit>>> = LazyLock::new(Default::default);
/// The latency (samples) each plug-in reported last, by id: the scan's until an instance says otherwise.
static LATENCY: LazyLock<Mutex<HashMap<String, u32>>> = LazyLock::new(Default::default);
/// Per instance (FX id): its editor window as last reported.
static EDITORS: LazyLock<Mutex<HashMap<String, EditorState>>> = LazyLock::new(Default::default);
/// Per instance: the plug-in's last messages.
static NOTES: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);

/// Values a plug-in changed itself on instance `instance` (an FX id) of `plugin`, as Milan stores them.
#[derive(Clone, Debug, PartialEq)]
pub struct Edit {
    pub instance: String,
    pub plugin: String,
    pub values: Vec<(String, f32)>,
    /// Control gestures in the editor: (key, began).
    pub gestures: Vec<(String, bool)>,
    /// A new state blob (the editor closed, a preset loaded): `Fx.text` entries to set.
    pub state: Option<(String, String)>,
}

pub fn id_of(d: &AudioDesc) -> String {
    format!("{}:{}", d.format, d.id)
}

fn leak(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

static DEFAULT_ONLY: &[Preset] = &[Preset { name: "Default", vals: &[], text: &[] }];

/// The controls of `d`: every parameter that is not hidden, read-only or the plug-in's own bypass.
pub fn specs(d: &AudioDesc) -> Vec<(AParam, ParamSpec)> {
    let mut out = Vec::new();
    for p in d.params.iter().filter(|p| !p.hidden && !p.readonly && !p.bypass) {
        let key = leak(&format!("p{}", p.id));
        let fallback = format!("Parameter {}", p.id);
        let name = leak(if p.name.is_empty() { &fallback } else { &p.name });
        let kind = if p.steps == 1 && p.options.len() <= 2 {
            ParamKind::Bool { default: to_milan(p, p.default) >= 0.5 }
        } else if p.steps >= 1 && !p.options.is_empty() {
            let options: Vec<&'static str> = p.options.iter().map(|o| leak(o)).collect();
            ParamKind::Choice { options: Box::leak(options.into_boxed_slice()), default: to_milan(p, p.default) as usize }
        } else {
            ParamKind::Float { min: p.min as f32, max: p.max.max(p.min + f64::EPSILON) as f32, default: p.default.clamp(p.min, p.max) as f32 }
        };
        out.push((p.clone(), ParamSpec { key, name, section: "", kind }));
    }
    out
}

fn leak_info(d: &AudioDesc) -> (&'static PluginInfo, Vec<(AParam, usize)>) {
    static LEAKED: LazyLock<Mutex<HashMap<String, (&'static PluginInfo, Vec<(AParam, usize)>)>>> = LazyLock::new(Default::default);
    let key = serde_json::to_string(d).unwrap_or_default();
    lock(&LEAKED)
        .entry(key)
        .or_insert_with(|| {
            let sp = specs(d);
            let specs: Vec<(AParam, usize)> = sp.iter().enumerate().map(|(i, (p, _))| (p.clone(), i)).collect();
            let params: Vec<ParamSpec> = sp.into_iter().map(|x| x.1).collect();
            let info = Box::leak(Box::new(PluginInfo {
                id: leak(&id_of(d)),
                gpu: None, // audio: the GPU compositor (R21b) has nothing to run
                name: leak(if d.name.is_empty() { &d.id } else { &d.name }),
                kind: PluginKind::AudioFx,
                group: leak(if d.vendor.is_empty() { if d.format == "clap" { "CLAP" } else { "VST3" } } else { &d.vendor }),
                category: "Third Party",
                desc: leak(&format!("{} {}", d.format.to_uppercase(), d.category)),
                params: Box::leak(params.into_boxed_slice()),
                presets: DEFAULT_ONLY,
            }));
            (&*info, specs)
        })
        .clone()
}

/// The audio effects of the last scan, with the user's choices: what the chooser offers and what runs.
pub fn set_registry(cache: &ScanCache, prefs: &OfxPrefs) {
    let mut r = Registry::default();
    let mut lat = lock(&LATENCY);
    for b in cache.binaries.iter().filter(|b| !b.audio.is_empty() || super::ofx::scan::kind_of(&b.path) != "ofx") {
        let kind: &'static str = if super::ofx::scan::kind_of(&b.path) == "clap" { "CLAP" } else { "VST3" };
        let file = b.path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        for (_, e) in &b.errors {
            r.entries.push(Entry { id: String::new(), name: file.clone(), group: String::new(), version: String::new(), kind, binary: b.path.clone(), status: Status::Failed(e.clone()) });
        }
        for d in &b.audio {
            let id = id_of(d);
            let status = if prefs.disabled.contains(&id) {
                Status::Disabled
            } else if !d.error.is_empty() {
                Status::Failed(d.error.clone())
            } else {
                Status::Ok
            };
            if status == Status::Ok && !r.usable.contains_key(&id) {
                let (info, specs) = leak_info(d);
                lat.entry(id.clone()).or_insert(d.latency);
                r.usable.insert(id.clone(), Arc::new(Plugin { info, binary: b.path.clone(), desc: d.clone(), specs }));
            }
            r.entries.push(Entry { id, name: if d.name.is_empty() { d.id.clone() } else { d.name.clone() }, group: d.vendor.clone(), version: d.version.clone(), kind, binary: b.path.clone(), status });
        }
    }
    drop(lat);
    *REG.write().unwrap_or_else(|p| p.into_inner()) = Arc::new(r);
    GEN.fetch_add(1, Ordering::Relaxed);
}

fn reg() -> Arc<Registry> {
    REG.read().unwrap_or_else(|p| p.into_inner()).clone()
}

pub fn find(id: &str) -> Option<Arc<Plugin>> {
    if !is_ext(id) {
        return None;
    }
    reg().usable.get(id).cloned()
}

pub fn info(id: &str) -> Option<&'static PluginInfo> {
    find(id).map(|p| p.info)
}

/// The VST3 and CLAP effects the Audio FX chooser offers, by vendor and name.
pub fn catalog(kind: PluginKind) -> Vec<&'static PluginInfo> {
    if kind != PluginKind::AudioFx {
        return Vec::new();
    }
    let mut v: Vec<&'static PluginInfo> = reg().usable.values().map(|p| p.info).collect();
    v.sort_by(|a, b| (a.group.to_lowercase(), a.name.to_lowercase(), a.id).cmp(&(b.group.to_lowercase(), b.name.to_lowercase(), b.id)));
    v
}

/// Every effect found, with what failed this session.
pub fn entries() -> Vec<Entry> {
    let failed = lock(&FAILED).clone();
    let mut v = reg().entries.clone();
    for e in &mut v {
        if let (Status::Ok, Some(why)) = (&e.status, failed.get(&e.id)) {
            e.status = Status::Failed(why.clone());
        }
    }
    v
}

pub fn failure(id: &str) -> Option<String> {
    lock(&FAILED).get(id).cloned()
}

pub fn clear_failures() {
    lock(&FAILED).clear();
    GEN.fetch_add(1, Ordering::Relaxed);
}

pub fn take_events() -> Vec<String> {
    std::mem::take(&mut *lock(&EVENTS))
}

pub fn take_edits() -> Vec<Edit> {
    std::mem::take(&mut *lock(&EDITS))
}

pub fn note(instance: &str) -> Option<String> {
    lock(&NOTES).get(instance).cloned()
}

/// The latency (samples) of plug-in `id`: what its last instance reported, else the scan's; 0 for anything else.
pub fn latency(id: &str) -> usize {
    if !is_ext(id) {
        return 0;
    }
    lock(&LATENCY).get(id).copied().unwrap_or(0) as usize
}

/// The name of effect `id` when it cannot run: "Name (off)", "Name (failed)", "id (missing)".
pub fn label(id: &str) -> Option<String> {
    if !is_ext(id) {
        return None;
    }
    let r = reg();
    Some(match r.entries.iter().find(|e| e.id == id) {
        Some(e) if e.status == Status::Disabled => format!("{} (off)", e.name),
        Some(e) => format!("{} (failed)", e.name),
        None => format!("{} (missing)", id.split_once(':').map_or(id, |x| x.1)),
    })
}

/// An instance of `id` would be bypassed now: the timeline badge.
pub fn is_bypassed(id: &str) -> bool {
    is_ext(id) && (!reg().usable.contains_key(id) || lock(&FAILED).contains_key(id))
}

const KEPT: &str = "It is bypassed; its settings are kept in the project.";

/// Why an instance of `id` does not run (the FX window says it); None when it runs or is not a VST3/CLAP effect.
pub fn bypassed_because(id: &str) -> Option<String> {
    if !is_ext(id) {
        return None;
    }
    let r = reg();
    let entry = r.entries.iter().find(|e| e.id == id);
    let name = entry.map_or_else(|| id.split_once(':').map_or(id, |x| x.1).to_string(), |e| e.name.clone());
    if let Some(why) = failure(id) {
        return Some(format!("{name} failed in Milan Pro: {why}. {KEPT} Rescan in the Plug-In Manager (View › Window) tries it again."));
    }
    if r.usable.contains_key(id) {
        return None;
    }
    let fmt = if id.starts_with(CLAP) { "CLAP" } else { "VST3" };
    Some(match entry.map(|e| &e.status) {
        Some(Status::Disabled) => format!("{name} is turned off in the Plug-In Manager (View › Window › Plug-In Manager). {KEPT}"),
        Some(Status::Failed(why)) => format!("{name} cannot run in Milan Pro: {why}. {KEPT}"),
        _ => format!("The {fmt} plug-in \"{name}\" is not installed, or its folder is not searched (Preferences › VST Effects; C:\\Program Files\\Common Files\\{fmt} is searched). {KEPT}"),
    })
}

fn fail(p: &Plugin, why: String) {
    let msg = format!("The {} plug-in {} failed and is bypassed: {why}. Its settings are kept; Rescan in the Plug-In Manager tries it again.", p.desc.format.to_uppercase(), p.info.name);
    eprintln!("aplug: {} ({}): {why}", p.info.id, p.binary.display());
    lock(&FAILED).insert(p.info.id.to_string(), why);
    lock(&EVENTS).push(msg);
    GEN.fetch_add(1, Ordering::Relaxed);
}

// ---------------------------------------------------------------- hosts

/// The plug-in hosts, one per module, with the generation of the running one (a new host after a crash gets a new
/// generation: instances must be made again, with their state).
type Slot = Arc<Mutex<Option<(Conn, u64)>>>;
static HOSTS: LazyLock<Mutex<HashMap<PathBuf, Slot>>> = LazyLock::new(Default::default);
static HOST_GEN: AtomicU64 = AtomicU64::new(1);

/// Close every audio plug-in host (Rescan, or the OFX hosts closing).
pub fn close_hosts() {
    let all: Vec<Slot> = lock(&HOSTS).drain().map(|h| h.1).collect();
    for s in all {
        if let Ok(mut g) = s.try_lock() {
            if let Some((c, _)) = g.take() {
                c.quit();
            }
        }
    }
    lock(&EDITORS).clear();
}

/// `f` runs with the host of `binary` (started when needed) and its generation. A host that crashes or hangs is
/// dropped; an error the plug-in reports leaves it running.
fn with_host<T>(binary: &Path, f: impl FnOnce(&mut Conn, u64) -> Result<Result<T, String>, client::Fail>) -> Result<T, String> {
    let slot = lock(&HOSTS).entry(binary.to_path_buf()).or_default().clone();
    let mut g = lock(&slot);
    if g.is_none() {
        let (mut c, _) = Conn::start(binary).map_err(|e| e.to_string())?;
        c.shm = Some(Shm::create(1 << 20).map_err(|e| format!("cannot make audio memory: {e}"))?);
        *g = Some((c, HOST_GEN.fetch_add(1, Ordering::Relaxed)));
    }
    let (conn, generation) = g.as_mut().expect("started");
    match f(conn, *generation) {
        Ok(r) => r,
        Err(e) => {
            // The host is gone, and every editor window it owned with it (another host's come back with its next poll).
            *g = None;
            lock(&EDITORS).clear();
            Err(e.to_string())
        }
    }
}

/// The state of `fx` as stored (None when it holds none).
fn stored_state(fx: &Fx) -> Option<AState> {
    let component = fx.text.get(STATE_KEY).cloned().unwrap_or_default();
    let controller = fx.text.get(CSTATE_KEY).cloned().unwrap_or_default();
    (!component.is_empty() || !controller.is_empty()).then_some(AState { component, controller })
}

fn instance_of(p: &Plugin, fx_id: &str, state: Option<AState>, params: Vec<(u32, f64)>, reset: bool) -> AInstance {
    AInstance { id: p.class().to_string(), instance: fx_id.to_string(), rate: crate::audio::RATE as f64, block: BLOCK as u32, state, params, reset }
}

/// Take in what a reply says the instance did.
fn absorb(p: &Plugin, instance: &str, o: &AOut) {
    {
        let mut l = lock(&LATENCY);
        if l.get(p.info.id) != Some(&o.latency) {
            l.insert(p.info.id.to_string(), o.latency);
        }
    }
    if let Some(e) = &o.editor {
        lock(&EDITORS).insert(instance.to_string(), e.clone());
    }
    if !o.messages.is_empty() {
        lock(&NOTES).insert(instance.to_string(), o.messages.join("\n"));
    }
    let mut values = p.to_keys(&o.edits);
    if let Some(all) = &o.values {
        values.extend(p.to_keys(all));
    }
    let gestures: Vec<(String, bool)> = o.gestures.iter().filter_map(|(id, on)| p.to_keys(&[(*id, 0.0)]).pop().map(|(k, _)| (k, *on))).collect();
    if !values.is_empty() || !gestures.is_empty() {
        lock(&EDITS).push(Edit { instance: instance.to_string(), plugin: p.info.id.to_string(), values, gestures, state: None });
    }
}

fn a_out(reply: Reply) -> Result<AOut, String> {
    match reply {
        Reply::AOut(o) => Ok(o),
        Reply::Error(e) => Err(e),
        r => Err(format!("the plug-in host answered {r:?}")),
    }
}

// ---------------------------------------------------------------- the mixer node

/// One instance of an audio effect in a mixer chain: its blocks go through the host.
pub struct Node {
    p: Arc<Plugin>,
    instance: String,
    state: Option<AState>,
    /// The values the FX held when the node was made: what a fresh instance is set to (the rest is its state's).
    stored: Vec<(u32, f64)>,
    /// The host generation the instance was made in (0: not yet).
    made_in: u64,
    sent: Vec<(u32, f64)>,
    reset: bool,
    time: f64,
}

/// The mixer node for `fx` when it is a VST3 or CLAP effect Milan Pro can run; None otherwise.
pub fn node(fx: &Fx) -> Option<Box<dyn AudioFx>> {
    let p = find(&fx.plugin)?;
    let stored = p.stored_values(fx);
    Some(Box::new(Node { p, instance: fx.id.clone(), state: stored_state(fx), stored, made_in: 0, sent: Vec::new(), reset: true, time: 0.0 }))
}

impl Node {
    fn block(&mut self, vals: &[(u32, f64)], buf: &mut [f32]) -> Result<(), String> {
        let n = buf.len() / 2;
        let p = self.p.clone();
        let o = with_host(&p.binary, |conn, generation| {
            let fresh = self.made_in != generation;
            let params = if fresh { self.stored.clone() } else { vals.iter().filter(|v| !self.sent.contains(v)).copied().collect() };
            let inst = instance_of(&p, &self.instance, if fresh { self.state.clone() } else { None }, params, self.reset || fresh);
            let shm = conn.shm.as_mut().expect("made with the host");
            let bytes: &mut [u8] = shm.bytes();
            bytes[..n * 8].copy_from_slice(bytemuck::cast_slice(&buf[..n * 2]));
            let req = Req::AProcess(AProcess { inst, frames: n as u32, time: self.time, playing: true, shm: shm.name.clone(), shm_size: shm.size });
            let reply = conn.request(&req, Some(client::render_timeout()))?;
            if let Reply::AOut(_) = &reply {
                let shm = conn.shm.as_mut().expect("made");
                buf[..n * 2].copy_from_slice(bytemuck::cast_slice(&shm.bytes()[n * 8..n * 16]));
            }
            Ok(a_out(reply).map(|o| (o, generation)))
        })?;
        let (o, generation) = o;
        (self.made_in, self.reset) = (generation, false);
        self.sent = vals.to_vec();
        // Values the plug-in changed itself are what it holds now: Milan's next ones only count when they differ.
        for (id, v) in &o.edits {
            if let Some(s) = self.sent.iter_mut().find(|s| s.0 == *id) {
                s.1 = *v;
            }
        }
        absorb(&p, &self.instance, &o);
        self.time += n as f64 / crate::audio::RATE as f64;
        Ok(())
    }
}

impl AudioFx for Node {
    fn process(&mut self, e: &FxEval, buf: &mut [f32]) {
        if failure(self.p.info.id).is_some() {
            return;
        }
        let vals = self.p.values(e);
        for chunk in buf.chunks_mut(2 * BLOCK) {
            if let Err(why) = self.block(&vals, chunk) {
                fail(&self.p, why);
                return;
            }
        }
    }

    fn reset(&mut self) {
        self.reset = true;
    }

    fn neutral(&self, _: &FxEval) -> bool {
        false
    }

    fn at(&mut self, t: f64) {
        self.time = t;
    }
}

// ---------------------------------------------------------------- state, presets and the editor (R34b)

/// The instance of `fx` in its host (made with the project's state and values when it is not there).
fn with_instance<T>(fx: &Fx, f: impl FnOnce(&mut Conn, AInstance) -> Result<Reply, client::Fail>) -> Result<(Arc<Plugin>, T), String>
where
    T: TryFrom<Reply, Error = String>,
{
    let p = find(&fx.plugin).ok_or_else(|| bypassed_because(&fx.plugin).unwrap_or_else(|| "not an audio plug-in".into()))?;
    if let Some(why) = failure(&fx.plugin) {
        return Err(why);
    }
    let vals = p.stored_values(fx);
    let inst = instance_of(&p, &fx.id, stored_state(fx), vals, false);
    let r = with_host(&p.binary, |conn, _| f(conn, inst).map(|r| T::try_from(r)));
    match r {
        Ok(t) => Ok((p, t)),
        Err(why) => {
            fail(&p, why.clone());
            Err(why)
        }
    }
}

struct Out(AOut);

impl TryFrom<Reply> for Out {
    type Error = String;

    fn try_from(r: Reply) -> Result<Out, String> {
        a_out(r).map(Out)
    }
}

struct St(AState);

impl TryFrom<Reply> for St {
    type Error = String;

    fn try_from(r: Reply) -> Result<St, String> {
        match r {
            Reply::AStateIs(s) => Ok(St(s)),
            Reply::Error(e) => Err(e),
            r => Err(format!("the plug-in host answered {r:?}")),
        }
    }
}

/// The plug-in's state of `fx` now, as `Fx.text` entries (`STATE_KEY`, and `CSTATE_KEY` when there is one).
pub fn fetch_state(fx: &Fx) -> Result<BTreeMap<String, String>, String> {
    let (_, St(s)) = with_instance::<St>(fx, |conn, inst| conn.request(&Req::AGetState(inst), Some(client::render_timeout())))?;
    let mut m = BTreeMap::new();
    m.insert(STATE_KEY.to_string(), s.component);
    if !s.controller.is_empty() {
        m.insert(CSTATE_KEY.to_string(), s.controller);
    }
    Ok(m)
}

/// Load a state into `fx`'s instance: the plug-in's values afterwards come back as an edit (`take_edits`) with the
/// state itself, so the app writes both into the project.
pub fn load_state(fx: &Fx, state: AState) -> Result<(), String> {
    let (p, Out(o)) = with_instance::<Out>(fx, |conn, inst| conn.request(&Req::ASetState(inst, state.clone()), Some(client::render_timeout())))?;
    let mut values = p.to_keys(&o.edits);
    if let Some(all) = &o.values {
        values.extend(p.to_keys(all));
    }
    lock(&EDITS).push(Edit { instance: fx.id.clone(), plugin: p.info.id.to_string(), values, gestures: Vec::new(), state: Some((state.component, state.controller)) });
    absorb_quiet(&p, &fx.id, &o);
    Ok(())
}

/// `absorb` without the edits (the caller queued them with the state).
fn absorb_quiet(p: &Plugin, instance: &str, o: &AOut) {
    let quiet = AOut { edits: Vec::new(), values: None, gestures: Vec::new(), ..o.clone() };
    absorb(p, instance, &quiet);
}

/// Save `fx`'s state as a `.vstpreset` at `path` (a VST3 effect; a CLAP state is written the same way with its id).
pub fn save_preset(fx: &Fx, path: &Path) -> Result<(), String> {
    let p = find(&fx.plugin).ok_or("the plug-in is not available")?;
    let st = fetch_state(fx)?;
    let preset = preset::Preset { class_id: p.class().to_string(), component: unb64(st.get(STATE_KEY).map_or("", String::as_str)), controller: unb64(st.get(CSTATE_KEY).map_or("", String::as_str)) };
    std::fs::write(path, preset::encode(&preset, p.info.name)).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// Load the `.vstpreset` at `path` into `fx`'s instance (it must be a preset of the same plug-in).
pub fn load_preset(fx: &Fx, path: &Path) -> Result<(), String> {
    let p = find(&fx.plugin).ok_or("the plug-in is not available")?;
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let preset = preset::decode(&bytes).map_err(|e| e.to_string())?;
    if !preset.class_id.eq_ignore_ascii_case(p.class()) && p.is_vst3() {
        return Err(format!("{} is a preset of another plug-in (class {}), not of {}", path.display(), preset.class_id, p.info.name));
    }
    load_state(fx, AState { component: b64(&preset.component), controller: b64(&preset.controller) })
}

/// R34b: the plug-in's editor window for `fx`: open it (off-screen and inactive when `offscreen`), close it, or poll
/// it (edits made in it come through `take_edits`). The window's state, or why there is none.
pub fn editor(fx: &Fx, op: EditorOp) -> Result<EditorState, String> {
    let closing = op == EditorOp::Close;
    let (p, Out(o)) = with_instance::<Out>(fx, |conn, inst| {
        if matches!(op, EditorOp::Open { .. }) {
            conn.allow_foreground();
        }
        conn.request(&Req::AEditor(inst, op.clone()), Some(client::render_timeout()))
    })?;
    absorb(&p, &fx.id, &o);
    let st = o.editor.unwrap_or_default();
    if closing || !st.open {
        lock(&EDITORS).remove(&fx.id);
        // What the user did in the editor besides moving parameters lives in the state: keep it with the FX.
        if let Ok(m) = fetch_state(fx) {
            lock(&EDITS).push(Edit { instance: fx.id.clone(), plugin: p.info.id.to_string(), values: Vec::new(), gestures: Vec::new(), state: Some((m.get(STATE_KEY).cloned().unwrap_or_default(), m.get(CSTATE_KEY).cloned().unwrap_or_default())) });
        }
    }
    Ok(st)
}

/// The editor window of instance `instance` as last reported (None: none open).
pub fn editor_state(instance: &str) -> Option<EditorState> {
    lock(&EDITORS).get(instance).cloned().filter(|e| e.open)
}

/// Every open editor is closed (the project closes, Milan exits).
pub fn close_editors() {
    let open: Vec<String> = lock(&EDITORS).keys().cloned().collect();
    if open.is_empty() {
        return;
    }
    let all: Vec<Slot> = lock(&HOSTS).values().cloned().collect();
    for s in all {
        if let Ok(mut g) = s.try_lock() {
            if let Some((conn, _)) = g.as_mut() {
                for id in &open {
                    let inst = AInstance { instance: id.clone(), ..Default::default() };
                    let _ = conn.request(&Req::AEditor(inst, EditorOp::Close), Some(std::time::Duration::from_secs(2)));
                }
            }
        }
    }
    lock(&EDITORS).clear();
}

/// An instance's FX was removed: the host forgets it (its editor closes).
pub fn close_instance(fx: &Fx) {
    let Some(p) = find(&fx.plugin) else { return };
    let _ = with_host::<()>(&p.binary, |conn, _| conn.request(&Req::AClose(fx.id.clone()), Some(std::time::Duration::from_secs(2))).map(|_| Ok(())));
    lock(&EDITORS).remove(&fx.id);
}

/// R35 research spike: the DMO findings from an isolated host (`docs/compat/dx-audio.md`).
#[cfg(test)]
pub fn dmo_probe(any_binary: &Path) -> Result<Vec<String>, String> {
    let (mut conn, _) = Conn::start(any_binary).map_err(|e| e.to_string())?;
    let r = conn.request(&Req::DmoProbe, Some(std::time::Duration::from_secs(30))).map_err(|e| e.to_string())?;
    conn.quit();
    match r {
        Reply::Rendered(o) => Ok(o.messages),
        Reply::Error(e) => Err(e),
        r => Err(format!("unexpected reply {r:?}")),
    }
}

#[cfg(test)]
mod tests;
