//! A1-A5 (docs/AE_PREMIERE_PLUGINS.md): AE-format effect plug-ins (`.aex`, written for the Adobe After Effects effect
//! API), an experimental feature that is off by default (Plug-In Manager › Folders). This module finds them and reads
//! their PiPL resources in Milan's process as data (`pipl`, bounds-checked: no plug-in code runs here), classifies them
//! before anything is loaded (Adobe's own effects are Blocked; audio effects and other machines are not run), and has
//! the rest described, rendered and flattened by `milan-plugin-host.exe` (its `aex` personality), one host per file,
//! through the same registry, pipe and watchdog as OFX plug-ins (`engine::ofx`).
//!
//! Instances are stored as `aex:<match name>`, with each parameter under `p<id>` (the plug-in's own parameter id, so a
//! plug-in version that reorders its parameters keeps projects valid). Flattened sequence data is kept in `Fx.blob`.
//!
//! Adobe, After Effects and Premiere Pro are trademarks of Adobe Inc. Milan Pro is not affiliated with, endorsed or
//! sponsored by Adobe.
pub mod pipl;

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use super::ofx::client::{Conn, describe_timeout, lock};
use super::ofx::proto::{AexInfo, PluginDesc, Reply, Req};
use super::ofx::scan::Binary;
use crate::project::{Fx, Project};

pub const PREFIX: &str = "aex:";
/// The plug-in kind as the Plug-In Manager labels it (plan §2.5).
pub const KIND: &str = "AE-format effect";
/// The trademark notice (plan §2.5): README, About and the Plug-In Manager.
pub const NOTICE: &str = "Adobe, After Effects and Premiere Pro are trademarks of Adobe Inc. Milan Pro is not affiliated with, endorsed or sponsored by Adobe.";
/// Said next to the switch (plan §2.6 (8)).
pub const NO_SUPPORT: &str = "Adobe does not support third-party hosts of its plug-in interface.";
/// The switch's label.
pub const SWITCH: &str = "Experimental: AE-format effects";

/// The AE-format preferences, kept with the OFX ones (`OfxPrefs::aex`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AexPrefs {
    /// Off by default: nothing is searched, listed or run until the user turns it on.
    pub enabled: bool,
    /// The user's folders, searched besides the shared plug-in folders.
    pub folders: Vec<PathBuf>,
}

/// Match names of plug-ins with a recorded compatibility test (`docs/compat/aex.csv`): only these show OK; every other
/// plug-in that loads shows Untested (plan §2.6 (7)). Milan's own test plug-in is the only one so far.
pub const TESTED_PREFIX: &str = "MLNT ";

pub fn tested(match_name: &str) -> bool {
    match_name.starts_with(TESTED_PREFIX)
}

// ---------------------------------------------------------------- where plug-ins are

/// The folders searched, in order, each once: none while the feature is off; else the shared plug-in folders the
/// registry names (After Effects' and Premiere Pro's `CommonPluginInstallPath`), the standard shared folders, then the
/// user's. Adobe application folders are never searched by default.
pub fn folders(prefs: &AexPrefs) -> Vec<PathBuf> {
    folders_with(prefs, &|k| std::env::var_os(k).filter(|v| !v.is_empty()), &registry_paths)
}

fn folders_with(prefs: &AexPrefs, env: &dyn Fn(&str) -> Option<std::ffi::OsString>, reg: &dyn Fn() -> Vec<PathBuf>) -> Vec<PathBuf> {
    if !prefs.enabled {
        return Vec::new();
    }
    let pf = env("ProgramFiles").map_or_else(|| PathBuf::from(r"C:\Program Files"), PathBuf::from);
    let mut v = reg();
    for p in ["Plug-ins", "Plugins"] {
        v.push(pf.join("Adobe").join("Common").join(p).join("7.0").join("MediaCore"));
    }
    v.extend(prefs.folders.iter().cloned());
    let mut seen = BTreeSet::new();
    v.retain(|p| seen.insert(p.to_string_lossy().trim_end_matches('\\').to_lowercase()));
    v
}

/// The shared plug-in folders named in the registry (plan §1.5), read-only.
#[cfg(windows)]
fn registry_paths() -> Vec<PathBuf> {
    use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
    let mut out = Vec::new();
    for key in [r"SOFTWARE\Adobe\After Effects\7.0", r"SOFTWARE\Adobe\Premiere Pro\10.0"] {
        let k: Vec<u16> = key.encode_utf16().chain([0]).collect();
        let v: Vec<u16> = "CommonPluginInstallPath".encode_utf16().chain([0]).collect();
        let mut buf = vec![0u16; 1024];
        let mut len = (buf.len() * 2) as u32;
        // SAFETY: NUL-terminated names and a buffer of `len` bytes.
        let r = unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, k.as_ptr(), v.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut len) };
        if r == 0 {
            buf.truncate((len as usize / 2).saturating_sub(1));
            let s = String::from_utf16_lossy(&buf);
            if !s.trim().is_empty() {
                out.push(PathBuf::from(s.trim()));
            }
        }
    }
    out
}

#[cfg(not(windows))]
fn registry_paths() -> Vec<PathBuf> {
    Vec::new()
}

/// The `.aex` files under `dirs`, as After Effects searches (plan §1.5): up to 10 folders deep, skipping folders whose
/// names end in ')' or start with '~', never following links; sorted, each once.
pub fn find(dirs: &[PathBuf]) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut all: Vec<(PathBuf, std::fs::FileType)> = rd.flatten().filter_map(|e| Some((e.path(), e.file_type().ok()?))).collect();
        all.sort_by(|a, b| a.0.cmp(&b.0));
        for (p, ft) in all {
            let name = p.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
            if ft.is_symlink() {
                continue;
            }
            if ft.is_file() && name.to_lowercase().ends_with(".aex") {
                out.push(p);
            } else if ft.is_dir() && depth > 1 && !name.ends_with(')') && !name.starts_with('~') {
                walk(&p, depth - 1, out);
            }
        }
    }
    let mut out = Vec::new();
    for d in dirs {
        walk(d, 10, &mut out);
    }
    let mut seen = BTreeSet::new();
    out.retain(|p| seen.insert(p.to_string_lossy().to_lowercase()));
    out
}

/// Why Milan does not load this effect: Adobe's own effects (match names starting "ADBE ", or a file inside an Adobe
/// application's folder) are licensed with Adobe's applications (plan §2.3, §2.6 (6)). None when it may be loaded.
pub fn blocked(path: &Path, match_name: &str) -> Option<String> {
    const WHY: &str = "Adobe's own effect, licensed for Adobe applications";
    if match_name.starts_with("ADBE ") {
        return Some(WHY.into());
    }
    let parts: Vec<String> = path.components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect();
    let in_app = parts.windows(2).any(|w| w[0] == "adobe" && w[1].starts_with("adobe "));
    in_app.then(|| format!("{WHY} (it is inside an Adobe application's folder)"))
}

/// Flag bits read from the PiPL (`docs/compat/aex-provenance.md`; the host's `abi.rs` has the same values).
mod flags {
    pub const I_USE_AUDIO: u32 = 1 << 20;
    pub const AUDIO_EFFECT_TOO: u32 = 1 << 30;
    pub const AUDIO_EFFECT_ONLY: u32 = 1 << 31;
    pub const CUSTOM_UI: u32 = 1 << 15;
    pub const DEEP_COLOR_AWARE: u32 = 1 << 25;
    pub const I_DO_DIALOG: u32 = 1 << 5;
    pub const SEQUENCE_DATA_NEEDS_FLATTENING: u32 = 1 << 4;
    pub const WIDE_TIME_INPUT: u32 = 1 << 1;
    pub const I_EXPAND_BUFFER: u32 = 1 << 9;
    pub const SUPPORTS_SMART_RENDER: u32 = 1 << 10;
    pub const FLOAT_COLOR_AWARE: u32 = 1 << 12;
    pub const I_USE_3D_CAMERA: u32 = 1 << 1;
    pub const I_USE_3D_LIGHTS: u32 = 1 << 2;
    pub const SUPPORTS_GPU_RENDER_F32: u32 = 1 << 25;
}

/// Why an effect with these flags is not run at all (plan §4.1): None when it may be. Premiere does not load audio
/// effects either.
pub fn unsupported(out_flags: u32, out_flags2: u32) -> Option<String> {
    let _ = out_flags2;
    if out_flags & (flags::I_USE_AUDIO | flags::AUDIO_EFFECT_TOO | flags::AUDIO_EFFECT_ONLY) != 0 {
        return Some("it processes audio, which Milan Pro does not give AE-format effects".into());
    }
    None
}

/// The capability line of an effect (plan §4.7): what it uses that Milan gives only in part, from its flags.
pub fn capabilities(out_flags: u32, out_flags2: u32) -> Vec<String> {
    let mut v = vec!["8-bit only".to_string()];
    if out_flags2 & flags::SUPPORTS_SMART_RENDER != 0 {
        v.push("SmartFX (Milan Pro sends the classic render command only; a SmartFX-only effect will not render)".into());
    }
    if out_flags & flags::CUSTOM_UI != 0 {
        v.push("custom controls: generic controls only".into());
    }
    if out_flags & flags::I_DO_DIALOG != 0 {
        v.push("an options dialog Milan Pro does not open".into());
    }
    if out_flags2 & (flags::I_USE_3D_CAMERA | flags::I_USE_3D_LIGHTS) != 0 {
        v.push("composition camera or lights, which Milan Pro does not have".into());
    }
    if out_flags2 & flags::SUPPORTS_GPU_RENDER_F32 != 0 {
        v.push("GPU rendering: its CPU path is used".into());
    }
    if out_flags & flags::I_EXPAND_BUFFER != 0 {
        v.push("may grow the frame: the output is cropped to it".into());
    }
    let _ = (flags::DEEP_COLOR_AWARE, flags::FLOAT_COLOR_AWARE, flags::SEQUENCE_DATA_NEEDS_FLATTENING, flags::WIDE_TIME_INPUT);
    v
}

/// What the PiPLs of `path` say, before anything runs: one description per effect PiPL (in file order, `AexInfo::pipl`
/// its position among them), and the file's errors. A file that is not a 64-bit Windows DLL, or has no effect PiPL, is
/// one error.
pub fn read_static(path: &Path) -> Binary {
    let mut out = Binary { path: path.to_path_buf(), ..Default::default() };
    let (machine, effects) = match pipl::effects_of_file(path) {
        Ok(x) => x,
        Err(e) => {
            out.errors.push((None, e));
            return out;
        }
    };
    if machine != pipl::MACHINE_X64 {
        out.errors.push((None, format!("not a 64-bit plug-in (machine 0x{machine:04X}): Milan Pro runs 64-bit plug-ins")));
        return out;
    }
    let mut n = 0u32;
    for (i, e) in effects.into_iter().enumerate() {
        let e = match e {
            Ok(e) => e,
            Err(why) => {
                out.errors.push((Some(i as u32), why));
                continue;
            }
        };
        if e.kind != "eFKT" {
            continue;
        }
        let blocked = blocked(path, &e.match_name).unwrap_or_default();
        let unsupported = if blocked.is_empty() { unsupported(e.out_flags, e.out_flags2).unwrap_or_default() } else { String::new() };
        let unsupported = if unsupported.is_empty() && e.entry.is_empty() { "its PiPL names no 64-bit entry point".to_string() } else { unsupported };
        let unsupported = if unsupported.is_empty() && e.match_name.is_empty() { "its PiPL has no match name".to_string() } else { unsupported };
        out.plugins.push(PluginDesc {
            index: n,
            id: e.match_name.clone(),
            label: e.name.clone(),
            group: e.category.clone(),
            major: e.version >> 19 & 0xf,
            minor: e.version >> 15 & 0xf,
            aex: Some(AexInfo {
                match_name: e.match_name,
                entry: e.entry,
                pipl: n,
                spec: e.spec_version,
                version: e.version,
                out_flags: e.out_flags,
                out_flags2: e.out_flags2,
                caps: capabilities(e.out_flags, e.out_flags2),
                blocked,
                unsupported,
                ..Default::default()
            }),
            ..Default::default()
        });
        n += 1;
    }
    if out.plugins.is_empty() && out.errors.is_empty() {
        out.errors.push((None, "it has no After Effects effect PiPL".into()));
    }
    out
}

/// Describe the effects of `path`: the PiPLs (in Milan, as data), then each effect Milan may run through global and
/// parameter setup in a plug-in host of its own (A2). Blocked and unsupported effects are never loaded. A crash or a
/// hang in an effect's setup costs that effect only (a new host describes the rest), recorded as why it cannot run.
pub fn describe_binary(path: &Path) -> Binary {
    let mut out = read_static(path);
    let runnable: Vec<usize> = (0..out.plugins.len()).filter(|&i| out.plugins[i].aex.as_ref().is_some_and(|a| a.blocked.is_empty() && a.unsupported.is_empty())).collect();
    let mut conn: Option<Conn> = None;
    for i in runnable {
        let index = out.plugins[i].aex.as_ref().map_or(0, |a| a.pipl);
        if conn.is_none() {
            match Conn::start(path) {
                Ok((c, _)) => conn = Some(c),
                Err(e) => {
                    set_unsupported(&mut out.plugins[i], format!("it could not be loaded: {e}"));
                    continue;
                }
            }
        }
        let c = conn.as_mut().expect("started");
        match c.request(&Req::Describe { index }, Some(describe_timeout())) {
            Ok(Reply::Described(d)) => {
                let mut d = *d;
                if let Some(a) = d.aex.as_mut() {
                    // Its flags as its global setup set them decide, as they do for its PiPL.
                    a.unsupported = unsupported(a.out_flags, a.out_flags2).unwrap_or_default();
                }
                out.plugins[i] = d;
            }
            Ok(Reply::Error(e)) => set_unsupported(&mut out.plugins[i], e),
            Ok(r) => set_unsupported(&mut out.plugins[i], format!("unexpected reply {r:?}")),
            Err(e) => {
                set_unsupported(&mut out.plugins[i], format!("its setup: {e}"));
                // A crashed or killed host: the rest are described in a new one.
                conn = None;
            }
        }
    }
    if let Some(c) = conn {
        c.quit();
    }
    out
}

fn set_unsupported(d: &mut PluginDesc, why: String) {
    if let Some(a) = d.aex.as_mut() {
        a.unsupported = why;
    }
}

/// What the FX window says about an AE-format effect besides its controls (plan §4.6): its custom controls are not
/// shown; it works in 8 bits. None for other plug-ins.
pub fn fx_note(id: &str) -> Option<String> {
    let p = super::ofx::find(id)?;
    let a = p.desc.aex.as_ref()?;
    let mut v = vec![format!("{KIND}: 8-bit only (a float project is quantised through it); generic controls.")];
    if a.out_flags & flags::CUSTOM_UI != 0 {
        v.push("This plug-in has custom controls that Milan Pro cannot show.".into());
    }
    Some(v.join(" "))
}

// ---------------------------------------------------------------- sequence data (A4)

/// Per instance (FX id): its saved data (`Fx.blob`), sent to a host that does not have the instance yet.
static BLOBS: LazyLock<Mutex<HashMap<String, BTreeMap<String, String>>>> = LazyLock::new(Default::default);

/// What `fx` holds for its plug-in host, noted where its frames are planned.
pub fn note_blob(fx: &Fx) {
    if fx.plugin.starts_with(PREFIX) && !fx.blob.is_empty() {
        let mut b = lock(&BLOBS);
        if b.get(&fx.id) != Some(&fx.blob) {
            b.insert(fx.id.clone(), fx.blob.clone());
        }
    }
}

/// The saved data of instance `instance`.
pub fn blob_for(instance: &str) -> BTreeMap<String, String> {
    lock(&BLOBS).get(instance).cloned().unwrap_or_default()
}

/// Before a save: every AE-format instance a plug-in host is running flattens its sequence data into its `Fx.blob`
/// (plan §4.2). An instance no host runs keeps what it had.
pub fn flatten_into(p: &mut Project) {
    let mut each = |fx: &mut Fx| {
        if !fx.plugin.starts_with(PREFIX) {
            return;
        }
        if let Some(b) = super::ofx::flatten(fx) {
            fx.blob = b;
            lock(&BLOBS).insert(fx.id.clone(), fx.blob.clone());
        }
    };
    let tl = &mut p.timeline;
    for t in &mut tl.video_tracks {
        t.fx.iter_mut().for_each(&mut each);
        for c in &mut t.clips {
            c.fx.iter_mut().for_each(&mut each);
        }
    }
    tl.master.video_fx.iter_mut().for_each(&mut each);
}

/// The id of an AE-format effect.
pub fn id_of(d: &PluginDesc) -> String {
    format!("{PREFIX}{}", d.aex.as_ref().map_or(d.id.as_str(), |a| a.match_name.as_str()))
}

const KEPT: &str = "It is bypassed; its settings are kept in the project.";

/// Why an instance of AE-format effect `id` does not run, for the FX window: Some(None) when it runs, None when `id` is
/// not an AE-format effect.
pub fn bypassed_because(id: &str) -> Option<Option<String>> {
    let name = id.strip_prefix(PREFIX)?;
    use super::ofx::Status;
    let entry = super::ofx::entries().into_iter().find(|e| e.id == id);
    let shown = entry.as_ref().map_or_else(|| name.to_string(), |e| e.name.clone());
    if let Some(why) = super::ofx::failure(id) {
        return Some(Some(format!("{shown} failed in Milan Pro: {why}. {KEPT} Rescan in the Plug-In Manager (View › Window) tries it again.")));
    }
    if super::ofx::find(id).is_some() {
        return Some(None);
    }
    Some(Some(match entry.map(|e| e.status) {
        Some(Status::Disabled) => format!("{shown} is turned off in the Plug-In Manager (View › Window › Plug-In Manager). {KEPT}"),
        Some(Status::Blocked(why)) => format!("{shown} is not loaded: {why}. {KEPT}"),
        Some(Status::Failed(why)) => format!("{shown} cannot run in Milan Pro: {why}. {KEPT}"),
        _ => format!(
            "The AE-format effect \"{name}\" is not installed, its folder is not searched, or AE-format effects are turned off (Plug-In Manager › Folders › {SWITCH}). {KEPT}"
        ),
    }))
}

#[cfg(test)]
mod tests;
