//! MVFX: the looks, optical tricks, temporal effects and transitions of 2001-2009 emo / pop-punk and hip-hop music
//! videos (`docs/research/MUSIC_VIDEO_2000S.md`), and FX Packages (saved chains of FX). Where VEGAS Pro had a
//! built-in of the same purpose the plug-in carries VEGAS's name, parameters and ranges; the era's other techniques
//! have plain descriptive names.
//!
//! Every plug-in has a CPU kernel (the reference, here) and a GPU port (`engine/gpu/mvfx.rs` and its WGSL) that gives
//! the same bits: the kernels are written with `pm`'s portable maths. Temporal plug-ins are pure functions of the
//! frame: their randomness comes from a seed and the frame number (`frame_vals` adds the time to their values), and
//! the ones that look at earlier source frames (Echo, Stutter, Datamosh) read them through the frame's `Sources`
//! (`event_fx`), so seeking, RAM preview, prerendering and rendering agree.
use eframe::egui::ColorImage;
use std::sync::Arc;
use crate::engine::composite::Sources;
use crate::engine::plan::{EventPlan, SrcAt};
use crate::engine::plugins::{FxEval, GpuPort, ParamKind, ParamSpec, PluginInfo, PluginKind, Preset, Val};
use crate::engine::vfx::{self, ColorOp};

pub mod looks;
pub mod optics;
pub mod packages;
pub mod pm;
#[cfg(test)]
mod review;
pub mod temporal;
#[cfg(test)]
mod temporal_tests;
pub mod trans;

/// The registry rows, by category.
pub static SETS: [&[PluginInfo]; 4] = [looks::PLUGINS, optics::PLUGINS, temporal::PLUGINS, trans::PLUGINS];

/// Every MVFX plug-in.
pub fn all() -> impl Iterator<Item = &'static PluginInfo> {
    SETS.iter().flat_map(|s| s.iter())
}

pub fn find(id: &str) -> Option<&'static PluginInfo> {
    all().find(|p| p.id == id)
}

// ---- registry helpers (as `plugins.rs`'s own) ----
pub(crate) const fn f(key: &'static str, name: &'static str, min: f32, max: f32, default: f32) -> ParamSpec {
    ParamSpec { key, name, section: "", kind: ParamKind::Float { min, max, default } }
}
pub(crate) const fn color(key: &'static str, name: &'static str, default: [f32; 4]) -> ParamSpec {
    ParamSpec { key, name, section: "", kind: ParamKind::Color { default } }
}
pub(crate) const fn choice(key: &'static str, name: &'static str, options: &'static [&'static str], default: usize) -> ParamSpec {
    ParamSpec { key, name, section: "", kind: ParamKind::Choice { options, default } }
}
pub(crate) const fn flag(key: &'static str, name: &'static str, default: bool) -> ParamSpec {
    ParamSpec { key, name, section: "", kind: ParamKind::Bool { default } }
}
pub(crate) const fn text(key: &'static str, name: &'static str, default: &'static str) -> ParamSpec {
    ParamSpec { key, name, section: "", kind: ParamKind::Text { multiline: false, default } }
}
pub(crate) const fn preset(name: &'static str, vals: &'static [(&'static str, f32)]) -> Preset {
    Preset { name, vals, text: &[] }
}
pub(crate) const DEFAULT: Preset = preset("Default", &[]);
pub(crate) const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
pub(crate) const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// A video FX row.
pub(crate) const fn fx_row(id: &'static str, name: &'static str, group: &'static str, category: &'static str, desc: &'static str, gpu: GpuPort, params: &'static [ParamSpec], presets: &'static [Preset]) -> PluginInfo {
    PluginInfo { id, gpu: Some(gpu), name, kind: PluginKind::VideoFx, group, category, desc, params, presets }
}
/// A transition row.
pub(crate) const fn tr_row(id: &'static str, name: &'static str, group: &'static str, category: &'static str, desc: &'static str, params: &'static [ParamSpec], presets: &'static [Preset]) -> PluginInfo {
    PluginInfo { id, gpu: Some(GpuPort::Mvfx), name, kind: PluginKind::Transition, group, category, desc, params, presets }
}

// ---- dispatch (from `vfx::apply`, `vfx::color_op_for`, `transition::mix`, `plan`) ----

/// An MVFX video FX on `img`; false for an id that is not one.
pub fn apply(img: &mut ColorImage, e: &FxEval, k: usize) -> bool {
    if find(e.id).is_none() {
        return false;
    }
    let [w, h] = img.size;
    if w == 0 || h == 0 {
        return true;
    }
    if let Some(op) = color_op_for(e) {
        vfx::color_op(img, &op, k);
    } else if let Some(steps) = recipe(e, w, h) {
        *img = run(&steps, &[img], k);
    }
    true
}

/// The `ColorOp` of an MVFX colour plug-in (`GpuPort::ColorOp`).
pub fn color_op_for(e: &FxEval) -> Option<ColorOp> {
    looks::color_op_for(e)
}

/// An MVFX transition of A into B at 0 < p < 1 (both the same size); None for another id.
pub fn mix(e: &FxEval, a: &ColorImage, b: &ColorImage, p: f32, k: usize) -> Option<ColorImage> {
    let [w, h] = a.size;
    recipe_tr(e, w, h, p).map(|s| run(&s, &[a, b], k))
}

/// Keys the plan adds to a temporal plug-in's values: its time (s), frame number and frame rate.
pub const T: &str = "_t";
pub const N: &str = "_n";
pub const FPS: &str = "_fps";

/// The plug-ins that change with time without a keyframe (grain, flicker, shake, strobe...): the plan gives them
/// their time, so each frame is its own picture (and frames without them keep one key).
pub fn temporal(id: &str) -> bool {
    looks::TEMPORAL.contains(&id) || optics::TEMPORAL.contains(&id) || temporal::TEMPORAL.contains(&id) || trans::TEMPORAL.contains(&id)
}

/// `plan::evals`: `e` at time `t` (its own time: event-relative for event FX) on an `fps` grid.
pub fn frame_vals(e: &mut FxEval, t: f64, fps: f64) {
    if temporal(e.id) {
        let fps = if fps > 0.0 { fps } else { 30.0 };
        e.vals.push((T, Val::F(t as f32)));
        e.vals.push((N, Val::F((t * fps + 1e-6).floor() as f32)));
        e.vals.push((FPS, Val::F(fps as f32)));
    }
}

/// The frame number a temporal plug-in was evaluated at (0 without one: a chooser thumbnail, a test).
pub fn frame_no(e: &FxEval) -> i64 {
    e.f(N) as i64
}

/// Some FX in `fx` reads earlier source frames.
pub fn needs_sources(fx: &[FxEval]) -> bool {
    fx.iter().any(|f| temporal::reads_sources(f.id))
}

/// `composite::event_image`'s event FX: `fx` on `img` (event `e`'s source at `size`) in order; an FX that reads
/// earlier source frames gets each through the FX before it (and the flicker filter, when the event has it).
pub fn event_fx(img: &mut ColorImage, e: &EventPlan, src: &mut dyn Sources, size: (u32, u32), k: usize) {
    for (i, f) in e.fx.iter().enumerate() {
        match earlier(e, f) {
            Some(offsets) => {
                let raw: Vec<Option<Arc<ColorImage>>> = std::iter::once(0).chain(offsets).map(|o| raw_frame(e, o, src, size)).collect();
                let frames: Vec<Option<Arc<ColorImage>>> = raw[1..].iter().map(|r| r.clone().map(|r| through(e, &e.fx[..i], r, k))).collect();
                temporal::combine(img, f, &frames, &raw, k);
            }
            None if f.id.starts_with(crate::engine::aex::PREFIX) => {
                // A5: an AE-format effect may check out this event's source at other times (its raw source: the chain
                // before it is not run again, shortcut).
                let shape = img.size;
                let mut fetch = |_layer: i32, offset: f64| {
                    raw_frame(e, -(offset.round() as i64), src, size).filter(|i| i.size == shape)
                };
                crate::engine::ofx::apply_with(img, f, &mut fetch);
            }
            None => vfx::apply(img, f, k),
        }
    }
}

/// Source frame `sf - offset` of event `e` (clamped to the media's first frame) at `size`, as decoded.
pub fn raw_frame(e: &EventPlan, offset: i64, src: &mut dyn Sources, (w, h): (u32, u32)) -> Option<Arc<ColorImage>> {
    let sf = (e.src.sf? - offset).max(0);
    src.get(&SrcAt { m: e.src.m.clone(), sf: Some(sf), generator: None, nested: None }, w, h).filter(|i| i.size == [w as usize, h as usize])
}

/// A decoded frame through the flicker filter (when the event has it) and `before` (the FX ahead of the one asking).
fn through(e: &EventPlan, before: &[FxEval], mut img: Arc<ColorImage>, k: usize) -> Arc<ColorImage> {
    if e.flicker {
        crate::engine::composite::reduce_flicker(Arc::make_mut(&mut img), k);
    }
    for f in before {
        vfx::apply(Arc::make_mut(&mut img), f, k);
    }
    img
}

/// The source frames (offsets back from the current one) FX `f` of event `e` reads; None: it reads none (a still,
/// generated media or a nested timeline has no earlier frames: the FX then runs alone).
pub fn earlier(e: &EventPlan, f: &FxEval) -> Option<Vec<i64>> {
    if !temporal::reads_sources(f.id) || e.src.sf.is_none() || e.src.generator.is_some() || e.src.nested.is_some() {
        return None;
    }
    Some(temporal::offsets(f))
}


// ---- recipes: what both compositors run ----

/// A kernel: the pixel at (x, y) from settings `m`, tables `d` and pictures A and B (B is A when a step reads one).
pub type Kernel = fn(m: &pm::M, d: &[u32], x: i32, y: i32, a: pm::Img, b: pm::Img) -> eframe::egui::Color32;

/// Where a step reads a picture: input `n` (an FX's picture is input 0; a transition's A and B are 0 and 1; Echo's
/// earlier frames follow) or the result of an earlier step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Src {
    In(usize),
    Step(usize),
}

/// One step of a plug-in's recipe. The last step's picture is the result; every picture is the inputs' size.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// Kernel `entry` (`fs_<entry>` in the WGSL) over A, reading B too.
    K { entry: &'static str, m: pm::M, data: Vec<u32>, a: Src, b: Src },
    /// `vfx::blur`'s three box passes per axis.
    Blur { of: Src, rx: usize, ry: usize },
    /// `composite::draw` onto a transparent picture through `m` (pixels to pixels).
    Draw { of: Src, m: crate::engine::composite::Affine, bilinear: bool },
}

pub(crate) fn k(entry: &'static str, m: pm::M, data: Vec<u32>, a: Src, b: Src) -> Step {
    Step::K { entry, m, data, a, b }
}

/// Every kernel, by entry name.
pub fn kernel(entry: &str) -> Kernel {
    [looks::KERNELS, optics::KERNELS, temporal::KERNELS, trans::KERNELS].iter().flat_map(|s| s.iter()).find(|k| k.0 == entry).map(|k| k.1).unwrap_or_else(|| panic!("no MVFX kernel {entry}"))
}

/// A recipe on the CPU: the reference.
pub fn run(steps: &[Step], inputs: &[&ColorImage], k: usize) -> ColorImage {
    let mut done: Vec<ColorImage> = Vec::with_capacity(steps.len());
    for s in steps {
        let pick = |src: Src, done: &[ColorImage]| -> ColorImage {
            match src {
                Src::In(n) => inputs[n].clone(),
                Src::Step(n) => done[n].clone(),
            }
        };
        let out = match s {
            Step::K { entry, m, data, a, b } => {
                let (pa, pb) = (pick(*a, &done), pick(*b, &done));
                let f = kernel(entry);
                pm::kernel2(&pa, &pb, k, |x, y, ia, ib| f(m, data, x, y, ia, ib))
            }
            Step::Blur { of, rx, ry } => {
                let mut img = pick(*of, &done);
                vfx::blur(&mut img, *rx, *ry, k);
                img
            }
            Step::Draw { of, m, bilinear } => {
                let img = pick(*of, &done);
                let mut out = ColorImage::new(img.size, eframe::egui::Color32::TRANSPARENT);
                crate::engine::composite::draw(&mut out, &img, *m, 1.0, *bilinear, k);
                out
            }
        };
        done.push(out);
    }
    done.pop().unwrap_or_else(|| inputs[0].clone())
}

/// The recipe of video FX `e` on a `w` x `h` picture; None for an id that is not an MVFX kernel FX (the colour ones
/// are `ColorOp`s; Echo, Stutter and Datamosh take theirs from `temporal::recipe_with`).
pub fn recipe(e: &FxEval, w: usize, h: usize) -> Option<Vec<Step>> {
    looks::recipe(e, w, h).or_else(|| optics::recipe(e, w, h)).or_else(|| temporal::recipe(e, w, h))
}

/// The recipe of transition `e` at 0 < p < 1.
pub fn recipe_tr(e: &FxEval, w: usize, h: usize, p: f32) -> Option<Vec<Step>> {
    trans::recipe(e, w, h, p)
}

/// The GPU's parity tolerance for an MVFX plug-in: blurs 3 in 255, the rest 2.
#[cfg(test)]
pub fn tolerance(id: &str) -> u8 {
    if optics::BLURS.contains(&id) || looks::BLURS.contains(&id) || trans::BLURS.contains(&id) { 3 } else { 2 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Every row: a unique id (among all built-ins), a description, a "Default" preset first, valid defaults and
    /// preset keys, a GPU port; no artist, director, label or trademark in a name.
    #[test]
    fn registry_rows_are_valid() {
        let mut ids: HashSet<&str> = crate::engine::plugins::PLUGINS.iter().map(|p| p.id).collect();
        for p in all() {
            assert!(ids.insert(p.id), "duplicate id {}", p.id);
            assert!(!p.desc.is_empty() && p.gpu.is_some(), "{}", p.id);
            assert!(p.presets.first().is_some_and(|d| d.name == "Default" && d.vals.is_empty()), "{}: a Default preset first", p.id);
            let keys: HashSet<&str> = p.params.iter().map(|s| s.key).collect();
            assert_eq!(keys.len(), p.params.len(), "{}", p.id);
            for s in p.params {
                assert!(!s.key.starts_with('_'), "{} {}", p.id, s.key);
                match s.kind {
                    ParamKind::Float { min, max, default } => assert!(min < max && min <= default && default <= max, "{} {}", p.id, s.key),
                    ParamKind::Choice { options, default } => assert!(default < options.len(), "{} {}", p.id, s.key),
                    ParamKind::Color { default } => assert!(default.iter().all(|v| (0.0..=1.0).contains(v))),
                    _ => {}
                }
            }
            for pr in p.presets {
                for (k, v) in pr.vals {
                    let base = k.rsplit_once('.').filter(|(_, c)| ["r", "g", "b", "a"].contains(c)).map(|(b, _)| b);
                    let spec = p.params.iter().find(|s| s.key == *k || Some(s.key) == base);
                    assert!(spec.is_some(), "{} preset {}: {k}", p.id, pr.name);
                    if let Some(ParamSpec { kind: ParamKind::Float { min, max, .. }, .. }) = spec {
                        assert!((*min..=*max).contains(v), "{} preset {}: {k} = {v} outside {min}..{max}", p.id, pr.name);
                    }
                }
            }
            for word in packages::BANNED {
                let lower = |s: &str| s.to_lowercase();
                assert!(!lower(p.name).contains(word) && p.presets.iter().all(|pr| !lower(pr.name).contains(word)), "{}: {word}", p.id);
            }
            assert!(crate::engine::plugins::find(p.id).is_some(), "{} is found", p.id);
        }
    }

    /// Every plug-in and preset renders on the chooser's sample, the same on one stripe as on several; every
    /// transition is exactly A at 0 and B at 1 and moves between.
    #[test]
    fn every_entry_renders_striped_as_single() {
        use crate::engine::{thumbs, transition};
        let pic = crate::engine::generate::fill(160, 90, |x, y| eframe::egui::Color32::from_rgba_premultiplied((x * 3 % 256) as u8 / 2, (y * 5 % 256) as u8 / 2, ((x + y) % 128) as u8, if (x / 20 + y / 20) % 5 == 0 { 128 } else { 255 }));
        for info in all() {
            for pr in info.presets {
                let mut e = crate::engine::plugins::preset_eval(info, Some(pr));
                frame_vals(&mut e, 1.3, 30.0);
                assert_eq!(thumbs::preset_thumb(info, Some(pr), 0.4).size, [thumbs::W, thumbs::H], "{}", info.id);
                match info.kind {
                    PluginKind::VideoFx => {
                        let (mut one, mut many) = (pic.clone(), pic.clone());
                        vfx::apply(&mut one, &e, 1);
                        vfx::apply(&mut many, &e, 4);
                        assert!(one.pixels == many.pixels, "{} {}", info.id, pr.name);
                    }
                    _ => {
                        let mut b = pic.clone();
                        b.pixels.reverse();
                        assert!(transition::mix(&e, &pic, &b, 0.0, 2).pixels == pic.pixels, "{} {}", info.id, pr.name);
                        assert!(transition::mix(&e, &pic, &b, 1.0, 2).pixels == b.pixels, "{} {}", info.id, pr.name);
                        let mid = transition::mix(&e, &pic, &b, 0.5, 1);
                        assert!(mid.pixels == transition::mix(&e, &pic, &b, 0.5, 4).pixels, "{} {}", info.id, pr.name);
                        assert!(mid.pixels != pic.pixels || transition::mix(&e, &pic, &b, 0.25, 1).pixels != pic.pixels, "{} {} moves", info.id, pr.name);
                    }
                }
            }
        }
    }
}
