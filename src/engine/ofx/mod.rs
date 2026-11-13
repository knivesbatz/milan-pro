//! R31-R33b: OFX plug-ins. `scan` finds and describes them (in a host process), this module turns each described
//! Filter or General effect into a registry entry the choosers, the FX window, keyframes and projects use like a
//! built-in (`plugins::find`), and renders it through `client` (a host process per binary, frames in shared memory).
//! A plug-in that crashes, hangs or fails is bypassed for the session with a message; its settings stay in the project.
//!
//! Instances are stored as `ofx:<identifier>@<major version>` with the OFX parameter names as keys: a Double is one key,
//! an RGB/RGBA colour the usual `key.r`..`key.a`, a 2D/3D value `key.x`, `key.y`, `key.z`, a string in `Fx.text`.
pub mod client;
pub mod proto;
pub mod scan;

use eframe::egui::{Color32, ColorImage};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, RwLock};
use crate::engine::plugins::{FxEval, ParamKind, ParamSpec, PluginInfo, PluginKind, Preset, Val};
use client::{Conn, Shm, lock};
use proto::{PVal, PluginDesc, Render, Reply, Req};
pub use scan::{OfxPrefs, ScanCache};

pub const PREFIX: &str = "ofx:";

/// One OFX plug-in Milan Pro can run.
#[derive(Debug)]
pub struct Plugin {
    pub info: &'static PluginInfo,
    pub binary: PathBuf,
    pub desc: PluginDesc,
    /// Rendered in 32-bit float (else 8-bit).
    pub float: bool,
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

pub fn id_of(d: &PluginDesc) -> String {
    format!("{PREFIX}{}@{}", d.id, d.major)
}

/// Why a described plug-in cannot run in Milan Pro; None when it can.
pub fn unsupported(d: &PluginDesc) -> Option<String> {
    if d.context.is_empty() {
        let has = d.contexts.iter().map(|c| c.trim_start_matches("OfxImageEffectContext")).collect::<Vec<_>>().join(", ");
        return Some(format!("it is a {has} effect: Milan Pro runs Filter and General video effects"));
    }
    if !d.depths.iter().any(|x| x == "OfxBitDepthFloat" || x == "OfxBitDepthByte") {
        return Some(format!("it takes only {} pixels: Milan Pro gives 8-bit or 32-bit float", d.depths.join(", ")));
    }
    None
}

/// The plug-ins of the last scan, with the user's choices: what the choosers offer and what runs.
pub fn set_registry(cache: &ScanCache, disabled: &BTreeSet<String>) {
    let mut r = Registry::default();
    for b in &cache.binaries {
        let file = b.path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        for (i, e) in &b.errors {
            r.entries.push(Entry { id: String::new(), name: i.map_or_else(|| file.clone(), |i| format!("{file} #{i}")), group: String::new(), version: String::new(), binary: b.path.clone(), status: Status::Failed(e.clone()) });
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
            if status == Status::Ok && !r.usable.contains_key(&id) {
                let float = d.depths.iter().any(|x| x == "OfxBitDepthFloat");
                r.usable.insert(id.clone(), Arc::new(Plugin { info: leak_info(d), binary: b.path.clone(), desc: d.clone(), float }));
            }
            let name = if d.label.is_empty() { d.id.clone() } else { d.label.clone() };
            r.entries.push(Entry { id, name, group: d.group.clone(), version: format!("{}.{}", d.major, d.minor), binary: b.path.clone(), status });
        }
    }
    *REG.write().unwrap_or_else(|p| p.into_inner()) = Arc::new(r);
    GEN.fetch_add(1, Ordering::Relaxed);
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

/// The OFX video FX the choosers offer, by grouping and name.
pub fn catalog() -> Vec<&'static PluginInfo> {
    let mut v: Vec<&'static PluginInfo> = reg().usable.values().map(|p| p.info).collect();
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
    GEN.fetch_add(1, Ordering::Relaxed);
}

/// Messages for the status bar since the last call.
pub fn take_events() -> Vec<String> {
    std::mem::take(&mut *lock(&EVENTS))
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
    eprintln!("ofx: {} ({}): {why}", p.info.id, p.binary.display());
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
            kind: PluginKind::VideoFx,
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

/// A finite slider range around the default (OFX ranges are often ±DBL_MAX).
fn range(min: f64, max: f64, default: f64) -> (f32, f32) {
    let ok = |v: f64| v.is_finite() && v.abs() < 1e7;
    let lo = if ok(min) { min } else { default.min(0.0) - 100.0 };
    let hi = if ok(max) { max } else { default.max(0.0) + 100.0 };
    let (lo, hi) = (lo.min(default), hi.max(default));
    (lo as f32, if hi > lo { hi as f32 } else { lo as f32 + 1.0 })
}

/// The FX window's controls for `d`: what Milan Pro can show and store. Hidden (secret) parameters, pages, push
/// buttons, custom parameters and choices without options are left to the plug-in's defaults; groups become headings.
pub fn specs(d: &PluginDesc) -> Vec<ParamSpec> {
    let groups: HashMap<&str, &str> = d.params.iter().filter(|p| p.kind == "Group").map(|p| (p.name.as_str(), p.label.as_str())).collect();
    let mut out: Vec<ParamSpec> = Vec::new();
    let mut section: Option<&str> = None;
    for p in d.params.iter().filter(|p| !p.secret) {
        let d0 = |i: usize| p.default.get(i).copied().unwrap_or(0.0);
        let mut add = |key: String, name: String, kind: ParamKind| {
            let heading = groups.get(p.parent.as_str()).copied().unwrap_or(if out.is_empty() { "" } else { "Other" });
            let sec = if section != Some(heading) { heading } else { "" };
            section = Some(heading);
            out.push(ParamSpec { key: leak(&key), name: leak(&name), section: leak(sec), kind });
        };
        match p.kind.as_str() {
            "Double" | "Integer" => {
                let (min, max) = range(p.min, p.max, d0(0));
                add(p.name.clone(), p.label.clone(), ParamKind::Float { min, max, default: d0(0) as f32 });
            }
            "Double2D" | "Double3D" | "Integer2D" | "Integer3D" => {
                for (i, (k, l)) in XYZ.iter().enumerate().take(components(&p.kind)) {
                    let (min, max) = range(p.min, p.max, d0(i));
                    add(format!("{}.{k}", p.name), format!("{} {l}", p.label), ParamKind::Float { min, max, default: d0(i) as f32 });
                }
            }
            "Boolean" => add(p.name.clone(), p.label.clone(), ParamKind::Bool { default: d0(0) != 0.0 }),
            "Choice" if !p.options.is_empty() => {
                let options: Vec<&'static str> = p.options.iter().map(|o| leak(o)).collect();
                let default = (d0(0).max(0.0) as usize).min(options.len() - 1);
                add(p.name.clone(), p.label.clone(), ParamKind::Choice { options: Box::leak(options.into_boxed_slice()), default });
            }
            "RGB" | "RGBA" => {
                let c = [d0(0), d0(1), d0(2), if p.kind == "RGBA" { p.default.get(3).copied().unwrap_or(1.0) } else { 1.0 }].map(|v| v.clamp(0.0, 1.0) as f32);
                add(p.name.clone(), p.label.clone(), ParamKind::Color { default: c });
            }
            "String" => add(p.name.clone(), p.label.clone(), ParamKind::Text { multiline: p.multiline, default: leak(&p.text) }),
            _ => {}
        }
    }
    out
}

/// The OFX values of `e` (an evaluated instance of `d`), as the host sets them.
pub fn values(d: &PluginDesc, e: &FxEval) -> Vec<(String, PVal)> {
    let has = |k: &str| e.vals.iter().any(|v| v.0 == k);
    let mut out = Vec::new();
    for p in d.params.iter().filter(|p| !p.secret) {
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
            "String" if has(&p.name) => PVal::S(e.t(&p.name).to_string()),
            _ => continue,
        };
        out.push((p.name.clone(), v));
    }
    out
}

/// R33b: what an OFX instance needs besides its parameters, added where a frame is planned: its instance (FX id), its
/// time in frames, the height its pictures have at full quality (for the render scale), the frame rate and pixel aspect.
pub fn add_frame_vals(e: &mut FxEval, instance: &str, t: f64, full_h: f64, fps: f64, par: f64) {
    if !e.id.starts_with(PREFIX) {
        return;
    }
    e.vals.push(("ofx.inst", Val::T(instance.to_string())));
    e.vals.push(("ofx.time", Val::F((t * fps) as f32)));
    e.vals.push(("ofx.full_h", Val::F(full_h as f32)));
    e.vals.push(("ofx.fps", Val::F(fps as f32)));
    e.vals.push(("ofx.par", Val::F(par as f32)));
    // A plug-in tried again after a failure renders its frames again (they are cached by their plan).
    e.vals.push(("ofx.gen", Val::F(generation() as f32)));
}

// ---------------------------------------------------------------- rendering

/// The plug-in hosts, one per binary (a crash takes that binary's host only); None after one died.
type Slot = Arc<Mutex<Option<Conn>>>;
static HOSTS: LazyLock<Mutex<HashMap<PathBuf, Slot>>> = LazyLock::new(Default::default);

/// Close every plug-in host (Rescan: binaries may have changed).
pub fn close_hosts() {
    let all: Vec<Slot> = lock(&HOSTS).drain().map(|h| h.1).collect();
    for s in all {
        if let Some(c) = lock(&s).take() {
            c.quit();
        }
    }
}

/// Run OFX effect `e` on `img` in place. A plug-in that is unknown, turned off or failed leaves it unchanged; one that
/// fails now is bypassed from here on, with a message (`take_events`).
pub fn apply(img: &mut ColorImage, e: &FxEval) {
    let Some(p) = find(e.id) else { return };
    if failure(e.id).is_some() || img.size[0] == 0 || img.size[1] == 0 {
        return;
    }
    let full_h = e.f("ofx.full_h") as f64;
    let par = e.f("ofx.par") as f64;
    let r = Render {
        index: p.desc.index,
        instance: Some(e.t("ofx.inst")).filter(|s| !s.is_empty()).unwrap_or("milan").to_string(),
        time: e.f("ofx.time") as f64,
        w: img.size[0] as u32,
        h: img.size[1] as u32,
        float: p.float,
        scale: if full_h > 0.0 { (img.size[1] as f64 / full_h).clamp(1e-3, 1.0) } else { 1.0 },
        par: if par > 0.0 { par } else { 1.0 },
        fps: e.f("ofx.fps") as f64,
        params: values(&p.desc, e),
        ..Default::default()
    };
    if let Err(why) = render(&p.binary, r, img) {
        fail(&p, why);
    }
}

/// Bytes per pixel in shared memory.
fn bpp(float: bool) -> usize {
    if float { 16 } else { 4 }
}

/// `img` into OFX rows (bottom first), 8-bit or float, premultiplied as Milan keeps it.
fn put(img: &ColorImage, float: bool, out: &mut [u8]) {
    let [w, h] = img.size;
    let row = w * bpp(float);
    for (y, src) in img.pixels.chunks_exact(w).enumerate() {
        let dst = &mut out[(h - 1 - y) * row..][..row];
        if float {
            for (d, s) in dst.chunks_exact_mut(16).zip(src) {
                for (c, v) in s.to_array().into_iter().enumerate() {
                    d[c * 4..c * 4 + 4].copy_from_slice(&(v as f32 / 255.0).to_le_bytes());
                }
            }
        } else {
            dst.copy_from_slice(bytemuck::cast_slice(src));
        }
    }
}

/// OFX rows back into `img`, clamped to valid premultiplied colours.
fn take(out: &[u8], float: bool, img: &mut ColorImage) {
    let [w, h] = img.size;
    let row = w * bpp(float);
    for (y, dst) in img.pixels.chunks_exact_mut(w).enumerate() {
        let src = &out[(h - 1 - y) * row..][..row];
        for (x, d) in dst.iter_mut().enumerate() {
            let c: [u8; 4] = if float {
                let f = |c: usize| f32::from_le_bytes(src[x * 16 + c * 4..x * 16 + c * 4 + 4].try_into().expect("4 bytes"));
                let a = f(3).clamp(0.0, 1.0);
                let q = |v: f32| (v * 255.0).round() as u8;
                [q(f(0).clamp(0.0, a)), q(f(1).clamp(0.0, a)), q(f(2).clamp(0.0, a)), q(a)]
            } else {
                [src[x * 4], src[x * 4 + 1], src[x * 4 + 2], src[x * 4 + 3]]
            };
            let a = c[3];
            *d = Color32::from_rgba_premultiplied(c[0].min(a), c[1].min(a), c[2].min(a), a);
        }
    }
}

/// One frame through the host of `binary` (started when needed). The host is dropped (killed) when it crashes or
/// hangs; an error the plug-in reports leaves it running.
fn render(binary: &Path, mut r: Render, img: &mut ColorImage) -> Result<(), String> {
    let slot = lock(&HOSTS).entry(binary.to_path_buf()).or_default().clone();
    let mut g = lock(&slot);
    if g.is_none() {
        *g = Some(Conn::start(binary).map_err(|e| e.to_string())?.0);
    }
    let conn = g.as_mut().expect("started");
    let float = r.float;
    let n = img.size[0] * img.size[1] * bpp(float);
    let need = 2 * n as u64;
    if conn.shm.as_ref().is_none_or(|s| s.size < need) {
        conn.shm = None;
        // Grown in 16 MB steps, so a slightly larger frame does not remap every time.
        conn.shm = Some(Shm::create(need.div_ceil(16 << 20) * (16 << 20)).map_err(|e| format!("cannot make frame memory: {e}"))?);
    }
    let shm = conn.shm.as_mut().expect("made");
    put(img, float, &mut shm.bytes()[..n]);
    (r.shm, r.shm_size, r.out) = (shm.name.clone(), shm.size, n as u64);
    match conn.request(&Req::Render(r), client::render_timeout()) {
        Ok(Reply::Rendered) => {
            let shm = conn.shm.as_mut().expect("made");
            take(&shm.bytes()[n..2 * n], float, img);
            Ok(())
        }
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

#[cfg(test)]
mod tests;
