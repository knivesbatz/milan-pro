//! R31-R33b: OFX plug-ins. `scan` finds and describes them (in a host process), this module turns each described
//! effect (Filter or General: a video FX; Transition; Generator) into a registry entry the choosers, the FX window,
//! keyframes and projects use like a built-in (`plugins::find`), and renders it through `client` (a host process per
//! binary, frames in shared memory). A plug-in that crashes, hangs or fails is bypassed for the session with a
//! message; its settings stay in the project.
//!
//! Instances are stored as `ofx:<identifier>@<major version>` with the OFX parameter names as keys: a Double is one key,
//! an RGB/RGBA colour the usual `key.r`..`key.a`, a 2D/3D value `key.x`, `key.y`, `key.z`, a string in `Fx.text`.
//! Every parameter that has a value is stored, hidden ones too: what the FX window shows (hidden, greyed, labels, choice
//! options, groups) follows the plug-in at run time (`layout`). Values a plug-in sets itself come back with each
//! reply (`take_edits`) and are written to the project by the app.
pub mod client;
pub mod proto;
pub mod scan;

use eframe::egui::{Color32, ColorImage};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, RwLock};
use crate::engine::plugins::{FxEval, ParamKind, ParamSpec, PluginInfo, PluginKind, Preset, Val};
use crate::project::{Curve, Fx, Keyframe, Keyframes};
use client::{Conn, Shm, lock};
pub use proto::Depth;
use proto::{Outcome, PVal, ParamDesc, ParamState, PluginDesc, Render, Reply, Req};
pub use scan::{OfxPrefs, ScanCache};

pub const PREFIX: &str = "ofx:";

/// One OFX plug-in Milan Pro can run.
#[derive(Debug)]
pub struct Plugin {
    pub info: &'static PluginInfo,
    pub binary: PathBuf,
    pub desc: PluginDesc,
    /// The pixel depth its pictures travel in.
    pub depth: Depth,
    /// Where each OFX parameter's controls are in `info.params` (one; two or three for a 2D or 3D value).
    specs: HashMap<String, Vec<usize>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Ok,
    Failed(String),
    Disabled,
}

impl Status {
    pub fn name(&self) -> &'static str {
        match self {
            Status::Ok => "OK",
            Status::Failed(_) => "Failed",
            Status::Disabled => "Disabled",
        }
    }
}

/// A Plug-In Manager row: a plug-in, or a binary that could not be read.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// `Fx.plugin` of its instances; "" for a binary that failed as a whole.
    pub id: String,
    pub name: String,
    pub group: String,
    pub version: String,
    /// "OFX", "OFX Transition" or "OFX Generator".
    pub kind: &'static str,
    pub binary: PathBuf,
    pub status: Status,
}

#[derive(Default)]
struct Registry {
    usable: HashMap<String, Arc<Plugin>>,
    entries: Vec<Entry>,
}

static REG: LazyLock<RwLock<Arc<Registry>>> = LazyLock::new(Default::default);
/// Plug-ins that crashed, hung or failed this session (by id), and why.
static FAILED: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);
/// Messages for the status bar (a plug-in that failed while rendering).
static EVENTS: LazyLock<Mutex<Vec<String>>> = LazyLock::new(Default::default);
/// Changes on any registry change or failure: choosers list again, frames render again.
static GEN: AtomicU64 = AtomicU64::new(1);
/// What plug-ins changed themselves, for the app to write into the project (`take_edits`).
static EDITS: LazyLock<Mutex<Vec<Edit>>> = LazyLock::new(Default::default);
/// Per instance (FX id): its parameters' run-time state, by parameter name.
static STATE: LazyLock<Mutex<HashMap<String, Arc<HashMap<String, ParamState>>>>> = LazyLock::new(Default::default);
/// Per instance: the plug-in's last messages (after a button, or with a frame), as it worded them.
static NOTES: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);
/// Per instance: the button whose action is running (set before its thread starts), and the way to stop its host once
/// it is talking to one.
static BUSY: LazyLock<Mutex<HashMap<String, (String, Option<client::Killer>)>>> = LazyLock::new(Default::default);
/// Plug-ins (by id) whose input colour space hint was dismissed, and whether that changed since `hints_changed`.
static HINTS: LazyLock<Mutex<(BTreeSet<String>, bool)>> = LazyLock::new(Default::default);

/// Values a plug-in set itself on instance `instance` (an FX id) of `plugin`.
#[derive(Clone, Debug, PartialEq)]
pub struct Edit {
    pub instance: String,
    pub plugin: String,
    pub edits: Vec<(String, PVal)>,
    /// A new instance's starting values: they fill in only what the FX does not hold yet.
    pub initial: bool,
    /// After a button the user clicked: an undo step of its own.
    pub by_user: bool,
}

const TRANSITION: &str = "OfxImageEffectContextTransition";
const GENERATOR: &str = "OfxImageEffectContextGenerator";

fn kind_of(d: &PluginDesc) -> PluginKind {
    match d.context.as_str() {
        TRANSITION => PluginKind::Transition,
        GENERATOR => PluginKind::Generator,
        _ => PluginKind::VideoFx,
    }
}

pub fn id_of(d: &PluginDesc) -> String {
    format!("{PREFIX}{}@{}", d.id, d.major)
}

/// Why a described plug-in cannot run in Milan Pro; None when it can.
pub fn unsupported(d: &PluginDesc) -> Option<String> {
    if d.context.is_empty() {
        let has = d.contexts.iter().map(|c| c.trim_start_matches("OfxImageEffectContext")).collect::<Vec<_>>().join(", ");
        return Some(format!("it is a {has} effect: Milan Pro runs Filter, General, Transition and Generator effects"));
    }
    if Depth::pick(&d.depths).is_none() {
        return Some(format!("it takes only {} pixels: Milan Pro gives 8-bit, 16-bit, half or 32-bit float", if d.depths.is_empty() { "no".into() } else { d.depths.join(", ") }));
    }
    None
}

/// The plug-ins of the last scan, with the user's choices: what the choosers offer and what runs.
pub fn set_registry(cache: &ScanCache, prefs: &OfxPrefs) {
    let disabled = &prefs.disabled;
    {
        let mut h = lock(&HINTS);
        if !h.1 {
            h.0.clone_from(&prefs.hints_seen);
        }
    }
    let mut r = Registry::default();
    for b in &cache.binaries {
        let file = b.path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        for (i, e) in &b.errors {
            r.entries.push(Entry { id: String::new(), name: i.map_or_else(|| file.clone(), |i| format!("{file} #{i}")), group: String::new(), version: String::new(), kind: "OFX", binary: b.path.clone(), status: Status::Failed(e.clone()) });
        }
        for d in &b.plugins {
            let id = id_of(d);
            let status = if disabled.contains(&id) {
                Status::Disabled
            } else if let Some(why) = unsupported(d) {
                Status::Failed(why)
            } else {
                Status::Ok
            };
            if let (Status::Ok, false, Some(depth)) = (&status, r.usable.contains_key(&id), Depth::pick(&d.depths)) {
                let info = leak_info(d);
                let mut specs: HashMap<String, Vec<usize>> = HashMap::new();
                for (i, s) in info.params.iter().enumerate() {
                    specs.entry(param_of(d, s.key).to_string()).or_default().push(i);
                }
                r.usable.insert(id.clone(), Arc::new(Plugin { info, binary: b.path.clone(), desc: d.clone(), depth, specs }));
            }
            let name = if d.label.is_empty() { d.id.clone() } else { d.label.clone() };
            let kind = match kind_of(d) {
                PluginKind::Transition => "OFX Transition",
                PluginKind::Generator => "OFX Generator",
                _ => "OFX",
            };
            r.entries.push(Entry { id, name, group: d.group.clone(), version: format!("{}.{}", d.major, d.minor), kind, binary: b.path.clone(), status });
        }
    }
    *REG.write().unwrap_or_else(|p| p.into_inner()) = Arc::new(r);
    GEN.fetch_add(1, Ordering::Relaxed);
    // R34a/R35: the VST3 and CLAP effects of the same scan.
    super::aplug::set_registry(cache, prefs);
}

fn reg() -> Arc<Registry> {
    REG.read().unwrap_or_else(|p| p.into_inner()).clone()
}

pub fn generation() -> u64 {
    GEN.load(Ordering::Relaxed)
}


pub fn find(id: &str) -> Option<Arc<Plugin>> {
    if !id.starts_with(PREFIX) {
        return None;
    }
    reg().usable.get(id).cloned()
}

/// The registry entry of `id` (enabled, described and of a context Milan runs).
pub fn info(id: &str) -> Option<&'static PluginInfo> {
    find(id).map(|p| p.info)
}

/// The OFX plug-ins of `kind` the choosers offer, by grouping and name.
pub fn catalog(kind: PluginKind) -> Vec<&'static PluginInfo> {
    let mut v: Vec<&'static PluginInfo> = reg().usable.values().map(|p| p.info).filter(|i| i.kind == kind).collect();
    v.sort_by(|a, b| (a.group.to_lowercase(), a.name.to_lowercase(), a.id).cmp(&(b.group.to_lowercase(), b.name.to_lowercase(), b.id)));
    v
}

/// Every plug-in found, with what failed this session.
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

/// What failed this session for `id`.
pub fn failure(id: &str) -> Option<String> {
    lock(&FAILED).get(id).cloned()
}

/// Rescan and the Plug-In Manager try failed plug-ins again.
pub fn clear_failures() {
    lock(&FAILED).clear();
    super::aplug::clear_failures();
    GEN.fetch_add(1, Ordering::Relaxed);
}

/// Messages for the status bar since the last call.
pub fn take_events() -> Vec<String> {
    let mut v = std::mem::take(&mut *lock(&EVENTS));
    v.extend(super::aplug::take_events());
    v
}

/// What plug-ins changed themselves since the last call (the app writes it into the project).
pub fn take_edits() -> Vec<Edit> {
    std::mem::take(&mut *lock(&EDITS))
}

/// Instance `instance`'s parameters as the plug-in last set them up (hidden, enabled, label, choice options).
pub fn state(instance: &str) -> Option<Arc<HashMap<String, ParamState>>> {
    lock(&STATE).get(instance).cloned()
}

/// The plug-in's last messages for `instance`, as it worded them.
pub fn note(instance: &str) -> Option<String> {
    lock(&NOTES).get(instance).cloned()
}

/// The button of `instance` whose action is still running (the plug-in may be showing a window of its own).
pub fn busy(instance: &str) -> Option<String> {
    lock(&BUSY).get(instance).map(|b| b.0.clone())
}

/// Stop the plug-in host that is running `instance`'s button (it starts again with the next frame).
pub fn stop(instance: &str) {
    if let Some((_, Some(k))) = lock(&BUSY).get(instance) {
        k.kill();
    }
}

/// The input colour space hint of `plugin` was read: it is not shown again.
pub fn dismiss_hint(plugin: &str) {
    let mut h = lock(&HINTS);
    h.1 |= h.0.insert(plugin.to_string());
}

/// The dismissed hints when they changed since the last call (the app keeps them in its preferences).
pub fn hints_changed() -> Option<BTreeSet<String>> {
    let mut h = lock(&HINTS);
    std::mem::take(&mut h.1).then(|| h.0.clone())
}

/// The name of OFX plug-in `id` when it cannot run: "Name (off)", "Name (failed)", "identifier (missing)".
pub fn label(id: &str) -> Option<String> {
    let rest = id.strip_prefix(PREFIX)?;
    let r = reg();
    Some(match r.entries.iter().find(|e| e.id == id) {
        Some(e) if e.status == Status::Disabled => format!("{} (off)", e.name),
        Some(e) => format!("{} (failed)", e.name),
        None => format!("{} (missing)", rest.rsplit_once('@').map_or(rest, |x| x.0)),
    })
}

/// An instance of OFX plug-in `id` would be bypassed now (failed, crashed, turned off, unsupported or missing): the
/// timeline badge. Cheap enough for every event on every frame.
pub fn is_bypassed(id: &str) -> bool {
    id.starts_with(PREFIX) && (!reg().usable.contains_key(id) || lock(&FAILED).contains_key(id))
}

const KEPT: &str = "It is bypassed; its settings are kept in the project.";

/// Why an instance of `id` does not run (the FX window says it); None when it runs or is not an OFX plug-in.
pub fn bypassed_because(id: &str) -> Option<String> {
    if !id.starts_with(PREFIX) {
        return None;
    }
    let r = reg();
    let entry = r.entries.iter().find(|e| e.id == id);
    let name = entry.map_or_else(|| id.trim_start_matches(PREFIX).to_string(), |e| e.name.clone());
    if let Some(why) = failure(id) {
        return Some(format!("{name} failed in Milan Pro: {why}. {KEPT} Rescan in the Plug-In Manager (View › Window) tries it again."));
    }
    if r.usable.contains_key(id) {
        return None;
    }
    Some(match entry.map(|e| &e.status) {
        Some(Status::Disabled) => format!("{name} is turned off in the Plug-In Manager (View › Window › Plug-In Manager). {KEPT}"),
        Some(Status::Failed(why)) => format!("{name} cannot run in Milan Pro: {why}. {KEPT}"),
        _ => {
            let (ident, major) = id.trim_start_matches(PREFIX).rsplit_once('@').unwrap_or((id, "?"));
            format!(
                "The OFX plug-in \"{ident}\" (version {major}) is not installed, or its folder is not searched (Plug-In Manager › Add Folder; C:\\Program Files\\Common Files\\OFX\\Plugins and OFX_PLUGIN_PATH are searched). {KEPT}"
            )
        }
    })
}

fn fail(p: &Plugin, why: String) {
    let msg = format!("The OFX plug-in {} failed and is bypassed: {why}. Its settings are kept; Rescan in the Plug-In Manager tries it again.", p.info.name);
    log::warn!("ofx: {} ({}): {why}", p.info.id, p.binary.display());
    lock(&FAILED).insert(p.info.id.to_string(), why);
    lock(&EVENTS).push(msg);
    GEN.fetch_add(1, Ordering::Relaxed);
}

// ---------------------------------------------------------------- parameters

fn leak(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

static DEFAULT_ONLY: &[Preset] = &[Preset { name: "Default", vals: &[], text: &[] }];

/// A registry entry for `d`, made once per description (a rescan that finds it unchanged leaks nothing new).
fn leak_info(d: &PluginDesc) -> &'static PluginInfo {
    static LEAKED: LazyLock<Mutex<HashMap<String, &'static PluginInfo>>> = LazyLock::new(Default::default);
    let key = serde_json::to_string(d).unwrap_or_default();
    *lock(&LEAKED).entry(key).or_insert_with(|| {
        Box::leak(Box::new(PluginInfo {
            id: leak(&id_of(d)),
            name: leak(if d.label.is_empty() { &d.id } else { &d.label }),
            kind: kind_of(d),
            group: leak(if d.group.is_empty() { "OFX" } else { &d.group }),
            category: "Third Party",
            desc: leak(d.description.lines().next().unwrap_or_default()),
            params: Box::leak(specs(d).into_boxed_slice()),
            presets: DEFAULT_ONLY,
        }))
    })
}

const XYZ: [(&str, &str); 3] = [("x", "X"), ("y", "Y"), ("z", "Z")];

fn components(kind: &str) -> usize {
    match kind {
        "Double2D" | "Integer2D" => 2,
        "Double3D" | "Integer3D" => 3,
        _ => 1,
    }
}

/// A parameter's hard range as 32-bit floats (what `plugins::eval` keeps a value within).
fn hard(vmin: f64, vmax: f64) -> (f32, f32) {
    let f = |v: f64| v.clamp(-f32::MAX as f64, f32::MAX as f64) as f32;
    let (lo, hi) = (f(vmin), f(vmax));
    (lo, if hi > lo { hi } else { lo + 1.0 })
}

/// A finite slider range around the default (OFX display ranges are often ±DBL_MAX).
fn range(min: f64, max: f64, default: f64) -> (f32, f32) {
    let ok = |v: f64| v.is_finite() && v.abs() < 1e7;
    let lo = if ok(min) { min } else { default.min(0.0) - 100.0 };
    let hi = if ok(max) { max } else { default.max(0.0) + 100.0 };
    let (lo, hi) = (lo.min(default), hi.max(default));
    (lo as f32, if hi > lo { hi as f32 } else { lo as f32 + 1.0 })
}

/// The OFX parameter a control key belongs to: the key itself, or the name before `.x` / `.y` / `.z`.
fn param_of<'a>(d: &PluginDesc, key: &'a str) -> &'a str {
    if d.params.iter().any(|p| p.name == key) {
        return key;
    }
    key.rsplit_once('.').map_or(key, |x| x.0)
}

/// A transition's own progress parameter: the host sets it, Milan neither shows nor stores it.
fn is_progress(d: &PluginDesc, p: &ParamDesc) -> bool {
    d.context == TRANSITION && p.name == "Transition"
}

/// The controls of `d`, in the order it defined them: every parameter with a value (Milan stores them all, so what a
/// plug-in sets or hides at run time is kept). Groups, pages and push buttons have no value: `layout` places them.
/// A Custom parameter is kept as text and never shown.
pub fn specs(d: &PluginDesc) -> Vec<ParamSpec> {
    let mut out: Vec<ParamSpec> = Vec::new();
    for p in d.params.iter().filter(|p| !is_progress(d, p)) {
        let d0 = |i: usize| p.default.get(i).copied().unwrap_or(0.0);
        let mut add = |key: String, name: String, kind: ParamKind| out.push(ParamSpec { key: leak(&key), name: leak(&name), section: "", kind });
        match p.kind.as_str() {
            // The spec's range is the hard one (a value a plug-in or preset sets stays as it is); the slider shows
            // the display range (`Row::Param::slider`).
            "Double" | "Integer" => {
                let (min, max) = hard(p.vmin, p.vmax);
                add(p.name.clone(), p.label.clone(), ParamKind::Float { min, max, default: d0(0) as f32 });
            }
            "Double2D" | "Double3D" | "Integer2D" | "Integer3D" => {
                for (i, (k, l)) in XYZ.iter().enumerate().take(components(&p.kind)) {
                    let (min, max) = hard(p.vmin, p.vmax);
                    add(format!("{}.{k}", p.name), format!("{} {l}", p.label), ParamKind::Float { min, max, default: d0(i) as f32 });
                }
            }
            "Boolean" => add(p.name.clone(), p.label.clone(), ParamKind::Bool { default: d0(0) != 0.0 }),
            "Choice" => {
                let options: Vec<&'static str> = p.options.iter().map(|o| leak(o)).collect();
                add(p.name.clone(), p.label.clone(), ParamKind::Choice { options: Box::leak(options.into_boxed_slice()), default: d0(0).max(0.0) as usize });
            }
            "RGB" | "RGBA" => {
                let c = [d0(0), d0(1), d0(2), if p.kind == "RGBA" { p.default.get(3).copied().unwrap_or(1.0) } else { 1.0 }].map(|v| v.clamp(0.0, 1.0) as f32);
                add(p.name.clone(), p.label.clone(), ParamKind::Color { default: c });
            }
            "String" | "Custom" => add(p.name.clone(), p.label.clone(), ParamKind::Text { multiline: p.multiline, default: leak(&p.text) }),
            _ => {}
        }
    }
    out
}

/// The OFX values of `e` (an evaluated instance of `d`), as the host sets them.
pub fn values(d: &PluginDesc, e: &FxEval) -> Vec<(String, PVal)> {
    let has = |k: &str| e.vals.iter().any(|v| v.0 == k);
    let mut out = Vec::new();
    for p in d.params.iter().filter(|p| !is_progress(d, p)) {
        let int = p.kind.starts_with("Integer");
        let n = |v: f32| if int { (v as f64).round() } else { v as f64 };
        let v = match p.kind.as_str() {
            "Double" | "Integer" if has(&p.name) => PVal::N(vec![n(e.f(&p.name))]),
            "Double2D" | "Double3D" | "Integer2D" | "Integer3D" => {
                let keys: Vec<String> = XYZ.iter().take(components(&p.kind)).map(|(k, _)| format!("{}.{k}", p.name)).collect();
                if !keys.iter().all(|k| has(k)) {
                    continue;
                }
                PVal::N(keys.iter().map(|k| n(e.f(k))).collect())
            }
            "Boolean" if has(&p.name) => PVal::N(vec![e.on(&p.name) as u8 as f64]),
            "Choice" if has(&p.name) => PVal::N(vec![e.i(&p.name) as f64]),
            "RGB" if has(&p.name) => PVal::N(e.c(&p.name)[..3].iter().map(|&v| v as f64).collect()),
            "RGBA" if has(&p.name) => PVal::N(e.c(&p.name).iter().map(|&v| v as f64).collect()),
            "String" | "Custom" if has(&p.name) => PVal::S(e.t(&p.name).to_string()),
            _ => continue,
        };
        out.push((p.name.clone(), v));
    }
    out
}

/// Write values the plug-in set itself (`edits`, by OFX parameter name) into `fx`. `initial`: a new instance's
/// starting values, which never replace what `fx` already holds. True when `fx` changed.
pub fn apply_edits(fx: &mut Fx, edits: &[(String, PVal)], initial: bool) -> bool {
    let Some(p) = find(&fx.plugin) else { return false };
    let mut changed = false;
    let num = |fx: &mut Fx, key: String, v: f64| -> bool {
        let v = v as f32;
        match fx.params.get(&key) {
            Some(_) if initial => false,
            // shortcut: a keyframed parameter keeps the user's keys; key the plug-in's value at the frame's time if a
            // plug-in needs to drive an animated parameter.
            Some(k) if k.keys.len() > 1 => false,
            Some(k) if k.keys.first().is_some_and(|k| k.v == v) => false,
            _ => {
                fx.params.insert(key, Keyframes { keys: vec![Keyframe { t: 0.0, v, curve: Curve::Linear }] });
                true
            }
        }
    };
    for (name, v) in edits {
        let Some(d) = p.desc.params.iter().find(|d| d.name == *name && !is_progress(&p.desc, d)) else { continue };
        match (d.kind.as_str(), v) {
            ("Double" | "Integer" | "Boolean" | "Choice", PVal::N(x)) if !x.is_empty() => changed |= num(fx, name.clone(), x[0]),
            ("Double2D" | "Double3D" | "Integer2D" | "Integer3D", PVal::N(x)) => {
                for ((k, _), v) in XYZ.iter().zip(x) {
                    changed |= num(fx, format!("{name}.{k}"), *v);
                }
            }
            ("RGB" | "RGBA", PVal::N(x)) => {
                for (k, v) in ["r", "g", "b", "a"].iter().zip(x) {
                    changed |= num(fx, format!("{name}.{k}"), *v);
                }
            }
            ("String" | "Custom", PVal::S(t)) => {
                if !(initial && fx.text.contains_key(name)) && fx.text.get(name) != Some(t) {
                    fx.text.insert(name.clone(), t.clone());
                    changed = true;
                }
            }
            _ => {}
        }
    }
    changed
}

/// One line of an OFX plug-in's controls in the FX window.
#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    /// A group's heading (click to open or close); its rows follow one level deeper while it is open.
    Group { name: String, label: String, depth: usize, open: bool },
    /// A page's name (only when the plug-in has several pages).
    Heading(String),
    /// Control `index` of the plug-in's `PluginInfo::params`; `options`: a choice's options now; `slider`: a number's
    /// display range (the value may lie outside it: the slider widens to it).
    Param { index: usize, label: String, depth: usize, enabled: bool, options: Option<Vec<String>>, slider: Option<(f32, f32)>, hint: String },
    Button { name: String, label: String, depth: usize, enabled: bool, hint: String },
}

/// The FX window's rows for instance `instance` of OFX plug-in `plugin`, laid out as the plug-in defines: its pages'
/// children in page order (then whatever is on no page), groups with their members under them, in the order they were
/// defined; hidden (secret) parameters left out, disabled ones greyed, labels and choice options as the plug-in last
/// set them. `open(group, default)` says whether a group is open. None when `plugin` is not a usable OFX plug-in.
pub fn layout(plugin: &str, instance: &str, open: &dyn Fn(&str, bool) -> bool) -> Option<Vec<Row>> {
    let p = find(plugin)?;
    Some(layout_of(&p, state(instance).as_deref(), open))
}

fn layout_of(p: &Plugin, st: Option<&HashMap<String, ParamState>>, open: &dyn Fn(&str, bool) -> bool) -> Vec<Row> {
    let all = &p.desc.params;
    let groups: HashSet<&str> = all.iter().filter(|d| d.kind == "Group").map(|d| d.name.as_str()).collect();
    let top = |d: &ParamDesc| d.kind != "Page" && !groups.contains(d.parent.as_str());
    let now = |d: &ParamDesc| match st.and_then(|s| s.get(&d.name)) {
        Some(s) => (s.secret, s.enabled, s.label.clone(), s.options.clone()),
        None => (d.secret, d.enabled, d.label.clone(), d.options.clone()),
    };
    // The top level: (page heading, parameter).
    let pages: Vec<&ParamDesc> = all.iter().filter(|d| d.kind == "Page" && !d.children.is_empty() && !now(d).0).collect();
    let mut roots: Vec<(Option<String>, &ParamDesc)> = Vec::new();
    for page in &pages {
        let mut heading = (pages.len() > 1).then(|| now(page).2);
        for d in page.children.iter().filter_map(|c| all.iter().find(|d| d.name == *c && top(d))) {
            if !roots.iter().any(|r| r.1.name == d.name) {
                roots.push((heading.take(), d));
            }
        }
    }
    for d in all.iter().filter(|d| top(d)) {
        if !roots.iter().any(|r| r.1.name == d.name) {
            roots.push((None, d));
        }
    }
    fn emit(p: &Plugin, d: &ParamDesc, depth: usize, on: bool, now: &dyn Fn(&ParamDesc) -> (bool, bool, String, Vec<String>), open: &dyn Fn(&str, bool) -> bool, out: &mut Vec<Row>) {
        let (secret, enabled, label, options) = now(d);
        if secret || depth > 8 {
            return;
        }
        let enabled = on && enabled;
        match d.kind.as_str() {
            "Group" => {
                let is_open = open(&d.name, d.open);
                out.push(Row::Group { name: d.name.clone(), label, depth, open: is_open });
                if is_open {
                    for c in p.desc.params.iter().filter(|c| c.parent == d.name && c.kind != "Page") {
                        emit(p, c, depth + 1, enabled, now, open, out);
                    }
                }
            }
            "PushButton" => out.push(Row::Button { name: d.name.clone(), label, depth, enabled, hint: d.hint.clone() }),
            _ => {
                let idx = p.specs.get(&d.name).map_or(&[][..], Vec::as_slice);
                // A Custom parameter has a value and no control.
                for (k, &index) in idx.iter().enumerate().filter(|_| d.kind != "Custom") {
                    let label = if idx.len() > 1 { format!("{label} {}", XYZ[k.min(2)].1) } else { label.clone() };
                    let number = matches!(d.kind.as_str(), "Double" | "Integer" | "Double2D" | "Double3D" | "Integer2D" | "Integer3D");
                    let slider = number.then(|| range(d.min, d.max, d.default.get(k).copied().unwrap_or(0.0)));
                    out.push(Row::Param { index, label, depth, enabled, options: (d.kind == "Choice").then(|| options.clone()), slider, hint: d.hint.clone() });
                }
            }
        }
    }
    let mut out = Vec::new();
    for (heading, d) in roots {
        let before = out.len();
        emit(p, d, 0, true, &now, open, &mut out);
        if let (Some(h), true) = (heading, out.len() > before) {
            out.insert(before, Row::Heading(h));
        }
    }
    out
}

/// Milan Pro sends Rec.709 video and does not provide OFX colour management. When a plug-in's input colour space
/// choice is still at a default that is a camera log space, say so once (until dismissed): the text, None otherwise.
pub fn colour_hint(plugin: &str, instance: &str, e: &FxEval) -> Option<String> {
    if lock(&HINTS).0.contains(plugin) {
        return None;
    }
    let p = find(plugin)?;
    let st = state(instance);
    p.desc.params.iter().filter(|d| d.kind == "Choice").find_map(|d| {
        let now = st.as_ref().and_then(|s| s.get(&d.name));
        let (secret, label, options) = now.map_or((d.secret, &d.label, &d.options), |s| (s.secret, &s.label, &s.options));
        let named = format!("{} {label}", d.name).to_lowercase();
        let cur = e.i(&d.name);
        if secret || !named.contains("input") || !["color", "colour", "gamut"].iter().any(|w| named.contains(w)) || cur as f64 != d.default.first().copied().unwrap_or(0.0) {
            return None;
        }
        let option = options.get(cur)?;
        let o = option.to_lowercase();
        ["log", "acescc", "bmdfilm", "intermediate"].iter().any(|w| o.contains(w)).then(|| format!("Milan Pro sends Rec.709 video; set \"{label}\" to Rec.709 / sRGB. It is at its default, \"{option}\", which expects camera log footage."))
    })
}

/// R33b: what an OFX instance needs besides its parameters, added where a frame is planned: its instance (FX id), its
/// time in frames, the height its pictures have at full quality (for the render scale), the frame rate and pixel aspect.
/// `restored`: Milan holds values for it already (`restored`), so they win over the plug-in's own starting values.
pub fn add_frame_vals(e: &mut FxEval, instance: &str, restored: bool, t: f64, full_h: f64, fps: f64, par: f64) {
    if !e.id.starts_with(PREFIX) {
        return;
    }
    e.vals.push(("ofx.inst", Val::T(instance.to_string())));
    e.vals.push(("ofx.restored", Val::F(restored as u8 as f32)));
    e.vals.push(("ofx.time", Val::F((t * fps) as f32)));
    e.vals.push(("ofx.full_h", Val::F(full_h as f32)));
    e.vals.push(("ofx.fps", Val::F(fps as f32)));
    e.vals.push(("ofx.par", Val::F(par as f32)));
    // A plug-in tried again after a failure renders its frames again (they are cached by their plan).
    e.vals.push(("ofx.gen", Val::F(generation() as f32)));
}

/// `fx` holds values of its own (it was rendered, edited, loaded or made from a preset): they win over what the
/// plug-in sets up for a new instance.
pub fn restored(fx: &Fx) -> bool {
    !fx.params.is_empty() || !fx.text.is_empty()
}

// ---------------------------------------------------------------- rendering

/// The plug-in hosts, one per binary (a crash takes that binary's host only); None after one died.
type Slot = Arc<Mutex<Option<Conn>>>;
static HOSTS: LazyLock<Mutex<HashMap<PathBuf, Slot>>> = LazyLock::new(Default::default);

/// Close every plug-in host (Rescan: binaries may have changed). Never waits: a host busy with a frame is left to the
/// thread rendering it, and is killed when that thread lets go of it (it is no longer in the list).
pub fn close_hosts() {
    let all: Vec<Slot> = lock(&HOSTS).drain().map(|h| h.1).collect();
    for s in all {
        if let Ok(mut g) = s.try_lock() {
            drop(g.take());
        }
    }
    super::aplug::close_hosts();
}

/// The request for one frame (or a button) of `p` as `e` evaluates it, for pictures of `size`.
fn request(p: &Plugin, e: &FxEval, size: [usize; 2]) -> Render {
    let full_h = e.f("ofx.full_h") as f64;
    let par = e.f("ofx.par") as f64;
    Render {
        index: p.desc.index,
        instance: Some(e.t("ofx.inst")).filter(|s| !s.is_empty()).unwrap_or("milan").to_string(),
        time: e.f("ofx.time") as f64,
        w: size[0] as u32,
        h: size[1] as u32,
        depth: p.depth,
        scale: if full_h > 0.0 { (size[1] as f64 / full_h).clamp(1e-3, 1.0) } else { 1.0 },
        par: if par > 0.0 { par } else { 1.0 },
        fps: e.f("ofx.fps") as f64,
        params: values(&p.desc, e),
        restored: e.on("ofx.restored"),
        ..Default::default()
    }
}

/// `e`'s plug-in when it runs: known, of `kind`, and not failed this session.
fn runnable(e: &FxEval, kind: PluginKind, size: [usize; 2]) -> Option<Arc<Plugin>> {
    find(e.id).filter(|p| p.info.kind == kind && failure(e.id).is_none() && size[0] > 0 && size[1] > 0)
}

/// Run OFX effect `e` on `img` in place. A plug-in that is unknown, turned off or failed leaves it unchanged; one that
/// fails now is bypassed from here on, with a message (`take_events`).
pub fn apply(img: &mut ColorImage, e: &FxEval) {
    let Some(p) = runnable(e, PluginKind::VideoFx, img.size) else { return };
    if let Err(why) = render(&p, request(&p, e, img.size), img, None, true) {
        fail(&p, why);
    }
}

/// OFX transition `e` from `a` to `b` at progress `x` (0..=1). None when it cannot run (the caller dissolves).
pub fn transition(e: &FxEval, a: &ColorImage, b: &ColorImage, x: f32) -> Option<ColorImage> {
    let p = runnable(e, PluginKind::Transition, a.size).filter(|_| a.size == b.size)?;
    let mut r = request(&p, e, a.size);
    r.params.push(("Transition".into(), PVal::N(vec![x.clamp(0.0, 1.0) as f64])));
    let mut out = a.clone();
    match render(&p, r, &mut out, Some(b), true) {
        Ok(()) => Some(out),
        Err(why) => {
            fail(&p, why);
            None
        }
    }
}

/// OFX generator `e`'s picture of `w` x `h` at media time `t` (seconds); transparent when it cannot run.
pub fn generate(e: &FxEval, w: u32, h: u32, t: f64) -> ColorImage {
    let mut out = ColorImage::new([w as usize, h as usize], Color32::TRANSPARENT);
    let Some(p) = runnable(e, PluginKind::Generator, out.size) else { return out };
    let mut r = request(&p, e, out.size);
    // Outside a frame plan (a thumbnail, the trimmer) there are no frame values: the media time at 30 fps.
    if e.t("ofx.inst").is_empty() {
        (r.instance, r.time, r.fps, r.restored) = (format!("gen-{}", e.id), t * 30.0, 30.0, true);
    }
    if let Err(why) = render(&p, r, &mut out, None, false) {
        fail(&p, why);
        out.pixels.fill(Color32::TRANSPARENT);
    }
    out
}

/// A click on push button `button` of `fx` (an OFX instance) in the FX window, at `fx`'s time `at`: the plug-in's
/// instance changed action for it runs in its host on a thread of its own, without a time limit (the plug-in may show
/// a window of its own and wait for the user; `busy` says so and `stop` ends it). What the plug-in changed comes back
/// through `take_edits` (as the user's own step), its messages through `note`. `button` "" only makes the instance, so
/// a new FX learns the plug-in's starting values before it is first rendered.
pub fn press(ctx: Option<&eframe::egui::Context>, fx: &Fx, at: f64, button: &str, size: (u32, u32), fps: f64, par: f64) {
    let (Some(p), Some(mut e)) = (find(&fx.plugin), crate::engine::plugins::eval(fx, at)) else { return };
    if failure(&fx.plugin).is_some() || busy(&fx.id).is_some() {
        return;
    }
    add_frame_vals(&mut e, &fx.id, restored(fx), at, size.1 as f64, fps, par);
    let r = request(&p, &e, [size.0.max(1) as usize, size.1.max(1) as usize]);
    let (button, ctx) = (button.to_string(), ctx.cloned());
    lock(&BUSY).insert(r.instance.clone(), (button.clone(), None));
    let run = move || {
        let instance = r.instance.clone();
        match act(&p, r, &button) {
            Ok(o) => absorb(&p, &instance, o, !button.is_empty()),
            Err(why) => {
                lock(&NOTES).insert(instance.clone(), format!("Error: {why}"));
            }
        }
        lock(&BUSY).remove(&instance);
        GEN.fetch_add(1, Ordering::Relaxed);
        if let Some(c) = ctx {
            c.request_repaint();
        }
    };
    if let Err(e) = std::thread::Builder::new().name("milan-ofx-button".into()).spawn(run) {
        lock(&BUSY).remove(&fx.id);
        lock(&NOTES).insert(fx.id.clone(), format!("Error: cannot start the button's thread: {e}"));
    }
}

/// Bytes per pixel in shared memory.
fn bpp(depth: Depth) -> usize {
    depth.bpp()
}

fn f32_to_half(v: f32) -> u16 {
    let b = v.to_bits();
    let (s, e, m) = (((b >> 16) & 0x8000) as u16, ((b >> 23) & 0xff) as i32 - 112, b & 0x7f_ffff);
    if e >= 31 {
        return s | 0x7c00;
    }
    if e <= 0 {
        // Too small for a normal half: a subnormal, or zero.
        return if e < -10 { s } else { s | (((m | 0x80_0000) >> (1 - e)) + 0x1000 >> 13) as u16 };
    }
    // Rounded to nearest; a carry out of the mantissa rightly bumps the exponent.
    s | ((((e as u32) << 10) | (m >> 13)) + ((m >> 12) & 1)) as u16
}

fn half_to_f32(h: u16) -> f32 {
    let (e, m) = ((h >> 10) & 31, (h & 1023) as u32);
    let v = match e {
        0 => m as f32 * 2f32.powi(-24),
        31 => if m == 0 { f32::INFINITY } else { f32::NAN },
        _ => f32::from_bits(((e as u32 + 112) << 23) | (m << 13)),
    };
    if h & 0x8000 != 0 { -v } else { v }
}

/// Threads for converting a picture of `n` pixels: one for small ones, else up to 8 (a 1080p float frame is 33 MB).
fn convert_threads(n: usize) -> usize {
    if n < 65_536 { 1 } else { std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 8) }
}

/// `img` into OFX rows (bottom first) at `depth`, premultiplied as Milan keeps it.
fn put(img: &ColorImage, depth: Depth, out: &mut [u8]) {
    let [w, h] = img.size;
    let row = w * bpp(depth);
    static TO_F32: LazyLock<[f32; 256]> = LazyLock::new(|| std::array::from_fn(|i| i as f32 / 255.0));
    static TO_HALF: LazyLock<[u16; 256]> = LazyLock::new(|| std::array::from_fn(|i| f32_to_half(i as f32 / 255.0)));
    let one = |src: &[Color32], dst: &mut [u8]| match depth {
        Depth::Byte => dst.copy_from_slice(bytemuck::cast_slice(src)),
        Depth::Short => {
            for (d, v) in dst.chunks_exact_mut(2).zip(src.iter().flat_map(|s| s.to_array())) {
                d.copy_from_slice(&(v as u16 * 257).to_le_bytes());
            }
        }
        Depth::Half => {
            for (d, v) in dst.chunks_exact_mut(2).zip(src.iter().flat_map(|s| s.to_array())) {
                d.copy_from_slice(&TO_HALF[v as usize].to_le_bytes());
            }
        }
        Depth::Float => {
            for (d, v) in dst.chunks_exact_mut(4).zip(src.iter().flat_map(|s| s.to_array())) {
                d.copy_from_slice(&TO_F32[v as usize].to_le_bytes());
            }
        }
    };
    let k = convert_threads(w * h);
    let per = h.div_ceil(k).max(1);
    let px = &img.pixels;
    std::thread::scope(|sc| {
        // Byte rows in chunks, bottom first: byte row b holds picture row h - 1 - b.
        for (c, chunk) in out[..row * h].chunks_mut(per * row).enumerate() {
            let mut work = move || {
                for (b, dst) in (c * per..).zip(chunk.chunks_exact_mut(row)) {
                    one(&px[(h - 1 - b) * w..][..w], dst);
                }
            };
            if k > 1 {
                sc.spawn(work);
            } else {
                work();
            }
        }
    });
}

/// OFX rows back into `img`, clamped to valid premultiplied colours.
fn take(out: &[u8], depth: Depth, img: &mut ColorImage) {
    let [w, h] = img.size;
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    let mut px = std::mem::take(&mut img.pixels);
    let bytes = &out[..w * h * bpp(depth)];
    let k = convert_threads(w * h);
    let per = h.div_ceil(k).max(1);
    std::thread::scope(|sc| {
        for (c, chunk) in px.chunks_mut(per * w).enumerate() {
            let y0 = c * per;
            let mut work = move || {
                for (y, dst) in (y0..).zip(chunk.chunks_exact_mut(w)) {
                    let src = &bytes[(h - 1 - y) * w * bpp(depth)..][..w * bpp(depth)];
                    let mut c = [0u8; 4];
                    for (x, d) in dst.iter_mut().enumerate() {
                        match depth {
                            Depth::Byte => c.copy_from_slice(&src[x * 4..x * 4 + 4]),
                            Depth::Short => {
                                for (i, v) in c.iter_mut().enumerate() {
                                    *v = ((u16::from_le_bytes([src[x * 8 + i * 2], src[x * 8 + i * 2 + 1]]) as u32 + 128) / 257) as u8;
                                }
                            }
                            Depth::Half => {
                                for (i, v) in c.iter_mut().enumerate() {
                                    *v = q(half_to_f32(u16::from_le_bytes([src[x * 8 + i * 2], src[x * 8 + i * 2 + 1]])));
                                }
                            }
                            Depth::Float => {
                                for (i, v) in c.iter_mut().enumerate() {
                                    *v = q(f32::from_le_bytes(src[x * 16 + i * 4..x * 16 + i * 4 + 4].try_into().expect("4 bytes")));
                                }
                            }
                        }
                        let a = c[3];
                        *d = Color32::from_rgba_premultiplied(c[0].min(a), c[1].min(a), c[2].min(a), a);
                    }
                }
            };
            if k > 1 {
                sc.spawn(work);
            } else {
                work();
            }
        }
    });
    img.pixels = px;
}

/// Take in what a reply says the plug-in did: its parameters' state, its own edits, its messages.
fn absorb(p: &Plugin, instance: &str, o: Outcome, by_user: bool) {
    if let Some(s) = o.state {
        lock(&STATE).insert(instance.to_string(), Arc::new(s.into_iter().map(|s| (s.name.clone(), s)).collect()));
    }
    if !o.edits.is_empty() {
        lock(&EDITS).push(Edit { instance: instance.to_string(), plugin: p.info.id.to_string(), edits: o.edits, initial: o.initial, by_user });
    }
    if !o.messages.is_empty() {
        lock(&NOTES).insert(instance.to_string(), o.messages.join("\n"));
    } else if by_user {
        lock(&NOTES).remove(instance);
    }
}

/// The host of `binary`, started when needed; `f` runs with it. The host is dropped (killed) when it crashes or
/// hangs; an error the plug-in reports leaves it running.
fn with_host(binary: &Path, f: impl FnOnce(&mut Conn) -> Result<Reply, client::Fail>) -> Result<Outcome, String> {
    let slot = lock(&HOSTS).entry(binary.to_path_buf()).or_default().clone();
    let mut g = lock(&slot);
    if g.is_none() {
        *g = Some(Conn::start(binary).map_err(|e| e.to_string())?.0);
    }
    match f(g.as_mut().expect("started")) {
        Ok(Reply::Rendered(o)) => Ok(o),
        Ok(Reply::Error(e)) => Err(e),
        Ok(other) => {
            *g = None;
            Err(format!("the plug-in host answered {other:?}"))
        }
        Err(f) => {
            *g = None;
            Err(f.to_string())
        }
    }
}

/// One frame through `p`'s host: `img` is the first input when `source` (a filter's Source, a transition's
/// SourceFrom), `second` a transition's SourceTo; the output replaces `img`.
fn render(p: &Plugin, mut r: Render, img: &mut ColorImage, second: Option<&ColorImage>, source: bool) -> Result<(), String> {
    let depth = r.depth;
    let n = img.size[0] * img.size[1] * bpp(depth);
    r.inputs = source as u32 + second.is_some() as u32;
    let out = r.inputs as usize * n;
    let instance = r.instance.clone();
    let o = with_host(&p.binary, |conn| {
        let need = (out + n) as u64;
        if conn.shm.as_ref().is_none_or(|s| s.size < need) {
            conn.shm = None;
            // Grown in 16 MB steps, so a slightly larger frame does not remap every time.
            conn.shm = Some(Shm::create(need.div_ceil(16 << 20) * (16 << 20)).map_err(|e| client::Fail::Io(format!("cannot make frame memory: {e}")))?);
        }
        let shm = conn.shm.as_mut().expect("made");
        if source {
            put(img, depth, &mut shm.bytes()[..n]);
        }
        if let Some(b) = second {
            put(b, depth, &mut shm.bytes()[n..2 * n]);
        }
        (r.shm, r.shm_size, r.out) = (shm.name.clone(), shm.size, out as u64);
        let reply = conn.request(&Req::Render(r), Some(client::render_timeout()))?;
        if matches!(reply, Reply::Rendered(_)) {
            take(&conn.shm.as_mut().expect("made").bytes()[out..out + n], depth, img);
        }
        Ok(reply)
    })?;
    absorb(p, &instance, o, false);
    Ok(())
}

/// A button's action (or "" to make the instance) in `p`'s host, with no time limit: `busy` names it meanwhile.
fn act(p: &Plugin, r: Render, button: &str) -> Result<Outcome, String> {
    let instance = r.instance.clone();
    with_host(&p.binary, |conn| {
        lock(&BUSY).insert(instance.clone(), (button.to_string(), Some(conn.killer())));
        // The user's click lets the plug-in bring a window of its own to the front.
        conn.allow_foreground();
        conn.request(&Req::Button(r, button.to_string()), None)
    })
}

#[cfg(test)]
mod tests;
