//! R26: a Milan project written as the `.milan.json` (format "milan-vegas", version 2) that
//! `tools/vegas/Import from Milan Pro.cs` rebuilds inside VEGAS (File › Export › VEGAS Project (via script)...).
//! The file is the same schema the VEGAS export script writes, so `vegas::to_project` reads it back (the round-trip
//! test), and `vegas_map` runs in reverse: Milan built-ins go out as the VEGAS plug-ins they stand for, `ofx:`
//! placeholders as the plug-in they came from. Milan never writes a `.veg`.
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use serde_json::{Value, json};
use crate::interchange::vegas::*;
use crate::interchange::vegas_map;
use crate::project::*;

/// The export and what it could not carry (notes for the user, in the file's `unsupported` too).
pub fn export(p: &Project) -> (VegasExport, Vec<String>) {
    let mut notes: Vec<String> = Vec::new();
    let s = &p.settings;
    let tl = &p.timeline;
    let media = |id: &str| p.media_pool.iter().find(|m| m.id == id);
    let size = (s.width as f64, s.height as f64);
    let mut groups: HashMap<&str, i64> = HashMap::new();
    let mut tracks = Vec::new();
    for (video, list) in [(true, &tl.video_tracks), (false, &tl.audio_tracks)] {
        for (i, tr) in list.iter().enumerate() {
            let at = tr.name.clone();
            let mut t = VTrack {
                kind: if video { "video" } else { "audio" }.into(),
                name: tr.name.clone(),
                mute: tr.muted,
                solo: tr.solo,
                volume: if video { 1.0 } else { tr.volume as f64 },
                pan: if video { 0.0 } else { tr.pan as f64 },
                level: if video { tr.volume as f64 } else { 1.0 },
                ..Default::default()
            };
            if video {
                t.mode = composite_name(tr.composite, &at, &mut notes).into();
                t.fx = tr.fx.iter().filter_map(|f| fx_to_vegas(f, &format!("{at}: track FX"), &mut notes)).collect();
                t.child = tr.child;
                t.parent = list.get(i + 1).is_some_and(|n| n.child) && !tr.child;
                if t.parent {
                    t.parent_mode = composite_name(tr.parent_mode, &at, &mut notes).into();
                    t.parent_motion = motion_keys(&tr.parent_motion, size);
                }
                t.motion = motion_keys(&tr.motion, size);
                (t.shadow, t.glow) = (tr.shadow.on, tr.glow.on);
                if tr.compositor.is_some() {
                    notes.push(format!("{at}: the Custom compositing mode's compositor is not exported"));
                }
            } else {
                t.fx = tr.fx.iter().filter_map(|f| audio_fx_to_vegas(f, &format!("{at}: track FX"), &mut notes)).collect();
            }
            t.envelopes = tr.envelopes.iter().filter_map(|e| envelope_to_vegas(e, &at, &mut notes)).collect();
            for c in &tr.clips {
                let where_ = format!("{at}: the event at {:.3} s", c.position);
                let Some(m) = c.media_id.as_deref().and_then(media) else {
                    notes.push(format!("{where_} has no media (an adjustment or empty event) and was left out"));
                    continue;
                };
                let mut ev = VEvent {
                    start: c.position,
                    length: c.duration,
                    rate: c.rate,
                    reverse: c.reverse,
                    // A reversed event's offset is the end of the span it plays (vegas::to_project undoes this).
                    offset: c.in_point + if c.reverse { c.duration * c.rate } else { 0.0 },
                    stream: c.stream as i32,
                    name: c.name.clone(),
                    fade_in: c.fade_in,
                    fade_out: c.fade_out,
                    fade_in_curve: curve_name(c.fade_in_curve).into(),
                    fade_out_curve: curve_name(c.fade_out_curve).into(),
                    gain: if video { c.opacity() as f64 } else { 10f64.powf(c.gain_db as f64 / 20.0) },
                    mute: c.muted,
                    locked: c.locked,
                    looping: c.looping,
                    normalize: c.normalize,
                    group: c.group.as_deref().map(|g| {
                        let n = groups.len() as i64;
                        *groups.entry(g).or_insert(n)
                    }),
                    ..Default::default()
                };
                if let Some(g) = &m.generator {
                    ev.generator = fx_to_vegas(g, &format!("{where_}: generated media"), &mut notes);
                    if ev.generator.is_none() {
                        notes.push(format!("{where_}: the generated media was left out"));
                        continue;
                    }
                    ev.name = m.name.clone();
                } else {
                    ev.path = m.path.display().to_string();
                    if m.nested {
                        notes.push(format!("{where_}: a nested Milan Pro project is media VEGAS cannot open"));
                    }
                }
                // A subclip's event plays its parent file from the subclip's start on.
                if m.subclip.is_some() {
                    ev.offset += m.file_time(0.0);
                }
                let (sw, sh) = (m.width.map_or(size.0, f64::from), m.height.map_or(size.1, f64::from));
                ev.src_size = Some([sw, sh]);
                for tk in &c.takes {
                    let Some(tm) = media(&tk.media_id) else { continue };
                    if tm.generator.is_some() {
                        notes.push(format!("{where_}: a generated-media take was left out"));
                        continue;
                    }
                    ev.takes.push(VTake { path: tm.path.display().to_string(), offset: tk.in_point, reverse: false, name: tk.name.clone(), stream: 0 });
                }
                if video {
                    ev.pan_crop = c.pan_crop.keys.iter().map(|k| pan_key(k, (sw, sh))).collect();
                    if !c.masks.is_empty() {
                        notes.push(format!("{where_}: Pan/Crop masks are not exported"));
                    }
                    ev.fx = c.fx.iter().filter_map(|f| fx_to_vegas(f, &format!("{where_}: event FX"), &mut notes)).collect();
                    for e in &c.envelopes {
                        match e.kind {
                            EnvKind::Velocity => ev.envelopes.push(VEnvelope { kind: "Velocity".into(), neutral: 1.0, points: points(&e.points, |v| v as f64) }),
                            EnvKind::Opacity if e.points.keys.len() > 1 => notes.push(format!("{where_}: an Opacity envelope with several points is exported as its first value")),
                            _ => {}
                        }
                    }
                    ev.transition_in = c.transition.as_ref().and_then(|t| transition_to_vegas(t, &format!("{where_}: transition"), &mut notes));
                    ev.transition_out = c.transition_out.as_ref().and_then(|t| transition_to_vegas(t, &format!("{where_}: transition"), &mut notes));
                } else {
                    ev.fx = c.fx.iter().filter_map(|f| audio_fx_to_vegas(f, &format!("{where_}: event FX"), &mut notes)).collect();
                    if c.pitch_cents != 0.0 {
                        notes.push(format!("{where_}: the pitch change is not exported"));
                    }
                }
                t.events.push(ev);
            }
            tracks.push(t);
        }
    }
    let span = |a: f64, b: f64, name: &str| VSpan { t: a, length: b - a, name: name.into() };
    let e = VegasExport {
        format: "milan-vegas".into(),
        version: 2,
        name: p.name.clone(),
        project: VProject {
            w: s.width,
            h: s.height,
            fps: s.frame_rate,
            sample_rate: s.sample_rate,
            field_order: match s.field_order {
                FieldOrder::Progressive => "ProgressiveScan",
                FieldOrder::Upper => "UpperFieldFirst",
                FieldOrder::Lower => "LowerFieldFirst",
            }
            .into(),
            pixel_aspect: s.par,
            quality: format!("{:?}", s.render_quality),
            motion_blur: s.motion_blur.name().replace(' ', ""),
            deinterlace: match s.deinterlace {
                Deinterlace::None => "None",
                Deinterlace::Blend => "BlendFields",
                Deinterlace::Interpolate => "InterpolateFields",
            }
            .into(),
            bit_depth: s.bit_depth as u32,
            bus_mode: "Stereo".into(),
            // R40b: VEGAS's PixelFormat names (a VEGAS without the full range one keeps its own setting).
            pixel_format: match s.pixel.format {
                crate::project::pixel::PixelFormat::Bit8 => "Int8Bit",
                crate::project::pixel::PixelFormat::FloatVideo => "Float32Bit",
                crate::project::pixel::PixelFormat::FloatFull => "Float32BitFullRange",
            }
            .into(),
        },
        tracks,
        markers: tl.markers.iter().map(|m| VSpan { t: m.position, length: 0.0, name: m.name.clone() }).collect(),
        regions: tl.regions.iter().map(|r| span(r.start, r.end, &r.name)).collect(),
        loop_region: tl.loop_region.map(|(a, b)| span(a, b, "")),
        output_fx: tl.master.video_fx.iter().filter_map(|f| fx_to_vegas(f, "Video output FX", &mut notes)).collect(),
        master_fx: tl.master.audio_fx.iter().filter_map(|f| audio_fx_to_vegas(f, "Master bus FX", &mut notes)).collect(),
        media_fx: Vec::new(),
        unsupported: notes.clone(),
    };
    (e, notes)
}

/// Write `p` to `path` as pretty JSON; the notes come back for the message.
pub fn write(p: &Project, path: &Path) -> anyhow::Result<Vec<String>> {
    let (e, notes) = export(p);
    let text = serde_json::to_string_pretty(&e)?;
    std::fs::write(path, text)?;
    Ok(notes)
}

fn curve_name(c: Curve) -> &'static str {
    match c {
        Curve::Linear => "Linear",
        Curve::Fast => "Fast",
        Curve::Slow => "Slow",
        Curve::Smooth => "Smooth",
        Curve::Sharp => "Sharp",
        Curve::Hold => "None",
    }
}

fn composite_name(c: Composite, at: &str, notes: &mut Vec<String>) -> &'static str {
    match c {
        Composite::SourceAlpha => "SrcAlpha",
        Composite::ThreeD => "SrcAlpha3D",
        Composite::Add => "Add",
        Composite::Subtract => "Subtract",
        Composite::Cut => "Cut",
        Composite::Multiply => "Multiply",
        Composite::Screen => "Screen",
        Composite::Overlay => "Overlay",
        Composite::HardLight => "HardLight",
        Composite::Dodge => "Dodge",
        Composite::Burn => "Burn",
        Composite::Darken => "Darken",
        Composite::Lighten => "Lighten",
        Composite::Difference => "Difference",
        Composite::DifferenceSquared => "DifferenceSquared",
        Composite::Custom => "Custom",
        other => {
            notes.push(format!("{at}: compositing mode {other:?} is Milan Pro's own; Source Alpha is used in VEGAS"));
            "SrcAlpha"
        }
    }
}

fn points(k: &Keyframes<f32>, f: impl Fn(f32) -> f64) -> Vec<VPoint> {
    k.keys.iter().map(|k| VPoint { t: k.t, v: f(k.v), curve: curve_name(k.curve).into() }).collect()
}

/// A track envelope in VEGAS's units (Volume linear, Mute 1 = not muted).
fn envelope_to_vegas(e: &Envelope, at: &str, notes: &mut Vec<String>) -> Option<VEnvelope> {
    let (kind, neutral, f): (&str, f64, fn(f32) -> f64) = match e.kind {
        EnvKind::Volume => ("Volume", 1.0, |db| 10f64.powf(db as f64 / 20.0)),
        EnvKind::Pan => ("Pan", 0.0, |v| v as f64),
        EnvKind::Mute => ("Mute", 1.0, |v| v as f64),
        EnvKind::CompositeLevel => ("Composite", 1.0, |v| v as f64),
        EnvKind::FadeToColor => ("FadeToColor", 0.0, |v| v as f64),
        other => {
            notes.push(format!("{at}: the {other:?} envelope is not exported"));
            return None;
        }
    };
    Some(VEnvelope { kind: kind.into(), neutral, points: points(&e.points, f) })
}

/// Milan's Track Motion back to VEGAS's pixels (the inverse of `vegas::motion`).
fn motion_keys(k: &Keyframes<Motion>, (pw, ph): (f64, f64)) -> Vec<VMotionKey> {
    k.keys
        .iter()
        .map(|k| VMotionKey {
            t: k.t,
            x: k.v.x as f64 * pw,
            y: -k.v.y as f64 * ph,
            z: k.v.z as f64 * pw,
            w: k.v.w as f64 * pw,
            h: k.v.h as f64 * ph,
            rx: k.v.rx as f64,
            ry: k.v.ry as f64,
            rz: -k.v.rot as f64,
            curve: curve_name(k.curve).into(),
        })
        .collect()
}

/// A Pan/Crop key's three corners in source pixels (the inverse of `vegas::pan_crop`).
fn pan_key(k: &Keyframe<PanCrop>, (sw, sh): (f64, f64)) -> VPanKey {
    let pc = k.v;
    let (sin, cos) = (pc.rot as f64).to_radians().sin_cos();
    let (u, v) = ((pc.w as f64 * sw * cos, pc.w as f64 * sw * sin), (-(pc.h as f64) * sh * sin, pc.h as f64 * sh * cos));
    let (cx, cy) = (pc.cx as f64 * sw, pc.cy as f64 * sh);
    let tl = [cx - u.0 / 2.0 - v.0 / 2.0, cy - u.1 / 2.0 - v.1 / 2.0];
    VPanKey { t: k.t, tl, tr: [tl[0] + u.0, tl[1] + u.1], bl: [tl[0] + v.0, tl[1] + v.1], curve: curve_name(k.curve).into() }
}

/// `Fx.params` component keys (`Color.r`, `Center.x`) gathered back into one VEGAS parameter each, typed by their
/// suffixes; a parameter with keyframes carries them.
fn unflatten(params: &BTreeMap<String, Keyframes<f32>>, text: &BTreeMap<String, String>, kind_of: &dyn Fn(&str) -> Option<&'static str>) -> Vec<VParam> {
    let mut groups: BTreeMap<String, Vec<(&str, &Keyframes<f32>)>> = BTreeMap::new();
    for (k, kf) in params {
        let (base, sfx) = match k.rsplit_once('.') {
            Some((b, s)) if ["r", "g", "b", "a", "x", "y", "z"].contains(&s) => (b, s),
            _ => (k.as_str(), ""),
        };
        groups.entry(base.to_string()).or_default().push((sfx, kf));
    }
    let mut out = Vec::new();
    for (name, mut comps) in groups {
        let order = |s: &str| ["", "r", "g", "b", "a"].iter().position(|x| *x == s).or_else(|| ["x", "y", "z"].iter().position(|x| *x == s).map(|i| i + 10)).unwrap_or(99);
        comps.sort_by_key(|c| order(c.0));
        let colour = comps.iter().any(|c| ["r", "g", "b", "a"].contains(&c.0));
        let kind = kind_of(&name).unwrap_or(match comps.len() {
            1 => "Double",
            3 if colour => "RGB",
            4 if colour => "RGBA",
            2 => "Double2D",
            _ => "Double3D",
        });
        let val = |i: usize| -> Value {
            let vals: Vec<Value> = comps.iter().map(|c| json!(c.1.keys.get(i).or(c.1.keys.last()).map_or(0.0, |k| k.v as f64))).collect();
            if comps.len() == 1 { vals.into_iter().next().unwrap_or(Value::Null) } else { Value::Array(vals) }
        };
        let n = comps.iter().map(|c| c.1.keys.len()).max().unwrap_or(0);
        let v = match kind {
            "Boolean" => json!(comps[0].1.keys.first().is_some_and(|k| k.v >= 0.5)),
            "Choice" | "Integer" => json!(comps[0].1.keys.first().map_or(0, |k| k.v.round() as i64)),
            _ => val(0),
        };
        // Keys are per component in Milan; VEGAS keys hold the whole value, so each component's key times line up
        // by index (they do when the keys came from VEGAS or Milan's FX window, which keys all components at once).
        let keys = if n > 1 && !matches!(kind, "Boolean" | "Choice" | "Integer") {
            (0..n).map(|i| VKey { t: comps[0].1.keys.get(i).map_or(0.0, |k| k.t), v: val(i), curve: curve_name(comps[0].1.keys.get(i).map_or(Curve::Linear, |k| k.curve)).into() }).collect()
        } else {
            Vec::new()
        };
        out.push(VParam { name, kind: kind.into(), v, keys });
    }
    for (k, s) in text {
        out.push(VParam { name: k.clone(), kind: "String".into(), v: json!(s), keys: Vec::new() });
    }
    out
}

/// A Milan video FX / generator as the VEGAS plug-in it stands for: mapped built-ins through `vegas_map` in reverse,
/// `ofx:` placeholders as the plug-in they came from with every parameter; others are noted and left out.
pub fn fx_to_vegas(fx: &Fx, at: &str, notes: &mut Vec<String>) -> Option<VFx> {
    let info = crate::engine::plugins::find(&fx.plugin);
    let display = info.map_or(fx.plugin.as_str(), |i| i.name);
    if let Some(vid) = vegas_map::from_placeholder(&fx.plugin) {
        // A placeholder's Boolean / Choice parameters were stored as 0/1 and indexes; the importer sets them by the
        // parameter's own type.
        return Some(VFx { id: format!("{{Svfx:{vid}}}"), name: String::new(), bypass: !fx.enabled, preset: String::new(), params: unflatten(&fx.params, &fx.text, &|_| None) });
    }
    let Some(m) = vegas_map::to_vegas(&fx.plugin) else {
        notes.push(format!("{at}: {display} has no VEGAS counterpart and was left out"));
        return None;
    };
    let mut params: BTreeMap<String, Keyframes<f32>> = BTreeMap::new();
    let mut text: BTreeMap<String, String> = BTreeMap::new();
    for pm in m.params {
        if let Some(kf) = fx.params.get(pm.milan) {
            let keys = kf.keys.iter().map(|k| Keyframe { t: k.t, v: pm.to_vegas(k.v), curve: k.curve }).collect();
            params.insert(pm.vegas.to_string(), Keyframes { keys });
        }
    }
    // A colour with only some channels set (a preset sets the red alone): the other channels go out at the built-in's
    // defaults, so VEGAS gets a whole colour.
    for pm in m.params {
        let Some((base, sfx)) = pm.vegas.rsplit_once('.') else { continue };
        if !["r", "g", "b", "a"].contains(&sfx) || params.contains_key(pm.vegas) {
            continue;
        }
        let prefix = format!("{base}.");
        if !m.params.iter().any(|q| q.vegas.starts_with(&prefix) && params.contains_key(q.vegas)) {
            continue;
        }
        let (mkey, msfx) = pm.milan.rsplit_once('.').unwrap_or((pm.milan, ""));
        let default = info
            .and_then(|i| i.params.iter().find(|s| s.key == mkey))
            .and_then(|s| match s.kind {
                crate::engine::plugins::ParamKind::Color { default } => ["r", "g", "b", "a"].iter().position(|x| *x == msfx).map(|i| default[i]),
                _ => None,
            })
            .unwrap_or(if msfx == "a" { 1.0 } else { 0.0 });
        params.insert(pm.vegas.to_string(), Keyframes { keys: vec![Keyframe { t: 0.0, v: pm.to_vegas(default), curve: Curve::Linear }] });
    }
    for (vk, mk) in m.text {
        if let Some(s) = fx.text.get(*mk) {
            text.insert(vk.to_string(), s.clone());
        }
    }
    match m.milan {
        "milan.fx.crop" => {
            let at0 = |k: &str, d: f32| fx.params.get(k).and_then(|kf| kf.keys.first()).map_or(d, |k| k.v);
            let (l, r, t, b) = (at0("left", 0.0), at0("right", 0.0), at0("top", 0.0), at0("bottom", 0.0));
            let one = |v: f32| Keyframes { keys: vec![Keyframe { t: 0.0, v, curve: Curve::Linear }] };
            params.insert("XScale".into(), one(1.0 - l - r));
            params.insert("YScale".into(), one(1.0 - t - b));
            params.insert("Location.x".into(), one((l + (1.0 - r)) / 2.0));
            params.insert("Location.y".into(), one((b + (1.0 - t)) / 2.0));
        }
        "milan.gen.text" => {
            if let Some(s) = fx.text.get("text") {
                let font = fx.text.get("font").map_or("Arial", String::as_str);
                text.insert("Text".into(), rtf(s, font));
            }
        }
        _ => {}
    }
    // The VEGAS types the mapped parameters have, so the importer sets them the right way.
    let kind_of = |name: &str| -> Option<&'static str> {
        let pm = m.params.iter().find(|p| p.vegas == name)?;
        let spec = info?.params.iter().find(|s| s.key == pm.milan.rsplit_once('.').map_or(pm.milan, |x| x.0))?;
        Some(match spec.kind {
            crate::engine::plugins::ParamKind::Bool { .. } => "Boolean",
            crate::engine::plugins::ParamKind::Choice { .. } => "Choice",
            _ if name == "Shape" || name == "Direction" || name == "GradientType" || name == "TestPatternType" || name == "Method" => "Choice",
            _ => return None,
        })
    };
    Some(VFx { id: format!("{{Svfx:{}}}", m.vegas), name: display.to_string(), bypass: !fx.enabled, preset: String::new(), params: unflatten(&params, &text, &kind_of) })
}

/// Plain text as the RTF VEGAS's Titles & Text expects.
fn rtf(text: &str, font: &str) -> String {
    let mut body = String::new();
    for c in text.chars() {
        match c {
            '\\' | '{' | '}' => {
                body.push('\\');
                body.push(c);
            }
            '\n' => body.push_str("\\par\n"),
            '\r' => {}
            c if (c as u32) < 128 => body.push(c),
            c => body.push_str(&format!("\\u{}?", c as u32 as i32)),
        }
    }
    format!("{{\\rtf1\\ansi\\ansicpg1252\\deff0{{\\fonttbl{{\\f0\\fnil\\fcharset0 {font};}}}}\n\\viewkind4\\uc1\\pard\\f0\\fs96 {body}\\par\n}}")
}

fn transition_to_vegas(t: &Transition, at: &str, notes: &mut Vec<String>) -> Option<VFx> {
    let params = t.params.iter().map(|(k, v)| (k.clone(), Keyframes { keys: vec![Keyframe { t: 0.0, v: *v, curve: Curve::Linear }] })).collect();
    let fx = Fx { id: String::new(), plugin: t.plugin.clone(), enabled: true, params, text: BTreeMap::new(), auto: BTreeMap::new() };
    if !t.progress.is_empty() {
        notes.push(format!("{at}: the Transition Progress envelope is not exported"));
    }
    fx_to_vegas(&fx, at, notes)
}

/// A Milan audio FX as the VEGAS plug-in name it stands for (its settings do not travel: VEGAS's audio plug-ins
/// have no scriptable parameters).
fn audio_fx_to_vegas(fx: &Fx, at: &str, notes: &mut Vec<String>) -> Option<VFx> {
    let Some(name) = vegas_map::audio_to_vegas(&fx.plugin) else {
        let display = crate::engine::plugins::find(&fx.plugin).map_or(fx.plugin.as_str(), |i| i.name);
        notes.push(format!("{at}: audio FX {display} has no VEGAS counterpart and was left out"));
        return None;
    };
    Some(VFx { id: String::new(), name: format!("VEGAS {name}"), bypass: !fx.enabled, preset: String::new(), params: Vec::new() })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::PathBuf;
    use crate::engine::plugins::{find, new_fx};

    fn one(v: f32) -> Keyframes<f32> {
        Keyframes { keys: vec![Keyframe { t: 0.0, v, curve: Curve::Linear }] }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    /// A project with one of everything the export carries.
    pub(crate) fn sample() -> Project {
        let mut p = Project::new("Round Trip");
        p.settings = ProjectSettings { width: 1920, height: 1080, frame_rate: 25.0, sample_rate: 48000, field_order: FieldOrder::Upper, par: 1.2121, bit_depth: 24, ..Default::default() };
        let item = |name: &str, t, w| MediaItem { id: name.into(), path: format!(r"C:\m\{name}").into(), name: name.into(), media_type: t, duration: 60.0, width: w, height: w.map(|_| 1080), sample_rate: Some(48000), audio_streams: 2, ..Default::default() };
        let solid = new_fx(find("milan.gen.solid").unwrap(), find("milan.gen.solid").unwrap().presets.iter().find(|p| p.name == "Red"));
        let generated = MediaItem { id: "gen".into(), name: "Red".into(), media_type: MediaType::Image, duration: 2.0, generator: Some(solid), width: Some(1920), height: Some(1080), ..Default::default() };
        p.media_pool = vec![item("a.mp4", MediaType::Video, Some(1920)), item("b.mp4", MediaType::Video, Some(1920)), item("t.wav", MediaType::Audio, None), generated];
        let tl = &mut p.timeline;
        let mut v = Track::new("Parent", TrackKind::Video);
        (v.volume, v.composite, v.muted) = (0.5, Composite::Screen, true);
        let mut blur = new_fx(find("milan.fx.gaussian_blur").unwrap(), None);
        blur.params.insert("h_range".into(), Keyframes { keys: vec![Keyframe { t: 0.0, v: 0.012, curve: Curve::Linear }, Keyframe { t: 2.0, v: 0.0, curve: Curve::Hold }] });
        blur.params.insert("v_range".into(), one(0.002));
        let mut grain = Fx { plugin: "ofx:com.sonycreativesoftware:radialpixelate@1".into(), enabled: false, ..new_fx(find("milan.fx.invert").unwrap(), None) };
        grain.params = BTreeMap::from([("Amount".to_string(), one(0.3)), ("Tint.r".into(), one(1.0)), ("Tint.g".into(), one(0.5)), ("Tint.b".into(), one(0.25)), ("Tint.a".into(), one(1.0))]);
        grain.text.insert("Label".into(), "grainy".into());
        v.fx = vec![blur, grain, new_fx(find("milan.fx.exposure").unwrap(), None)];
        v.motion = Keyframes { keys: vec![Keyframe { t: 0.0, v: Motion::default(), curve: Curve::Smooth }, Keyframe { t: 2.0, v: Motion { x: 0.1, y: 0.1, w: 0.5, h: 0.5, rot: -10.0, ..Default::default() }, curve: Curve::Linear }] };
        v.parent_mode = Composite::Multiply;
        v.envelopes.push(Envelope { kind: EnvKind::CompositeLevel, points: Keyframes { keys: vec![Keyframe { t: 0.0, v: 1.0, curve: Curve::Linear }, Keyframe { t: 2.0, v: 0.25, curve: Curve::Hold }] } });
        let mut c = Clip::from_media(&p.media_pool[0], 1.0);
        (c.duration, c.in_point, c.rate, c.out_point) = (2.0, 3.5, 2.0, 7.5);
        (c.fade_in, c.fade_out, c.fade_in_curve, c.fade_out_curve) = (0.4, 0.2, Curve::Fast, Curve::Sharp);
        c.set_opacity(0.5);
        (c.muted, c.locked, c.looping, c.stream) = (true, true, false, 1);
        c.group = Some("g1".into());
        c.name = "Take A".into();
        c.takes.push(Take { media_id: "b.mp4".into(), in_point: 1.0, name: "B".into() });
        c.pan_crop = Keyframes { keys: vec![Keyframe { t: 0.0, v: PanCrop { w: 0.5, h: 0.5, ..Default::default() }, curve: Curve::Smooth }, Keyframe { t: 0.4, v: PanCrop { cx: 0.6, cy: 0.4, w: 0.3, h: 0.7, rot: 30.0 }, curve: Curve::Hold }] };
        c.envelopes.push(Envelope { kind: EnvKind::Velocity, points: Keyframes { keys: vec![Keyframe { t: 0.0, v: 1.0, curve: Curve::Linear }, Keyframe { t: 1.0, v: 0.0, curve: Curve::Hold }] } });
        c.transition = Some(Transition { plugin: "milan.tr.push".into(), params: BTreeMap::from([("direction".to_string(), 1.0)]), progress: Keyframes::default() });
        c.transition_out = Some(Transition { plugin: "milan.tr.fade_to_color".into(), params: BTreeMap::from([("additive".to_string(), 1.0), ("color.r".into(), 1.0), ("color.g".into(), 1.0), ("color.b".into(), 0.0)]), progress: Keyframes::default() });
        let mut crop = new_fx(find("milan.fx.crop").unwrap(), None);
        crop.params = BTreeMap::from([("left".to_string(), one(0.5)), ("top".into(), one(0.5))]);
        c.fx.push(crop);
        let mut g = Clip::from_media(&p.media_pool[3], 4.0);
        g.duration = 2.0;
        v.clips = vec![c, g];
        let mut child = Track::new("Child", TrackKind::Video);
        (child.child, child.composite) = (true, Composite::Add);
        tl.video_tracks = vec![v, child];
        let mut a = Track::new("Music", TrackKind::Audio);
        (a.volume, a.pan, a.solo) = (0.5, -0.25, true);
        a.fx = vec![new_fx(find("milan.afx.eq").unwrap(), None), Fx { enabled: false, ..new_fx(find("milan.afx.compressor").unwrap(), None) }];
        a.envelopes = vec![
            Envelope { kind: EnvKind::Volume, points: Keyframes { keys: vec![Keyframe { t: 0.0, v: -6.0, curve: Curve::Smooth }, Keyframe { t: 1.0, v: 6.0, curve: Curve::Linear }] } },
            Envelope { kind: EnvKind::Mute, points: Keyframes { keys: vec![Keyframe { t: 0.0, v: 1.0, curve: Curve::Hold }, Keyframe { t: 1.0, v: 0.0, curve: Curve::Hold }] } },
            Envelope { kind: EnvKind::Pan, points: Keyframes { keys: vec![Keyframe { t: 0.0, v: 0.5, curve: Curve::Linear }] } },
        ];
        let mut ac = Clip::from_media(&p.media_pool[2], 1.0);
        (ac.duration, ac.out_point, ac.gain_db, ac.normalize, ac.group) = (2.0, 2.0, -6.0, true, Some("g1".into()));
        ac.fx.push(new_fx(find("milan.afx.reverb").unwrap(), None));
        a.clips = vec![ac];
        tl.audio_tracks = vec![a];
        tl.markers.push(Marker { position: 0.4, name: "Intro".into(), color: [1, 2, 3] });
        tl.regions.push(Region { start: 8.0, end: 10.0, name: "R".into() });
        tl.loop_region = Some((1.0, 3.0));
        tl.master.video_fx.push(Fx { enabled: false, ..new_fx(find("milan.fx.brightness_contrast").unwrap(), None) });
        tl.master.audio_fx.push(new_fx(find("milan.afx.limiter").unwrap(), None));
        p
    }

    fn probed(p: &Project) -> HashMap<String, Result<MediaItem, String>> {
        p.media_pool.iter().filter(|m| m.generator.is_none()).map(|m| (m.path.display().to_string(), Ok(m.clone()))).collect()
    }

    /// R26: Milan → JSON → the R25 parser gives the same timeline.
    #[test]
    fn round_trip_through_the_vegas_json() {
        let p = sample();
        let (e, notes) = export(&p);
        let text = serde_json::to_string_pretty(&e).unwrap();
        let back = read_vegas_json(&text).unwrap();
        let (q, w) = to_project(&back, &probed(&p));
        assert_eq!(q.settings, p.settings);
        assert_eq!(q.name, "Round Trip");
        let (v, v0) = (&q.timeline.video_tracks[0], &p.timeline.video_tracks[0]);
        assert_eq!((v.name.as_str(), v.muted, v.volume, v.composite, v.parent_mode), ("Parent", true, 0.5, Composite::Screen, Composite::Multiply));
        // FX: the blur's keys, the placeholder as it was, Exposure left out and noted.
        assert_eq!(v.fx.len(), 2);
        let blur = &v.fx[0];
        assert_eq!(blur.plugin, "milan.fx.gaussian_blur");
        let h = &blur.params["h_range"].keys;
        assert!(h.len() == 2 && (h[0].v - 0.012).abs() < 1e-6 && h[1].t == 2.0 && h[1].curve == Curve::Hold, "{h:?}");
        assert!((blur.params["v_range"].keys[0].v - 0.002).abs() < 1e-6);
        assert_eq!((v.fx[1].plugin.as_str(), v.fx[1].enabled), ("ofx:com.sonycreativesoftware:radialpixelate@1", false));
        assert_eq!(v.fx[1].params, v0.fx[1].params);
        assert_eq!(v.fx[1].text, v0.fx[1].text);
        assert!(notes.iter().any(|n| n.contains("Exposure has no VEGAS counterpart")), "{notes:?}");
        assert_eq!(v.motion, v0.motion);
        assert_eq!(v.envelopes, v0.envelopes);
        assert!(q.timeline.video_tracks[1].child && q.timeline.video_tracks[1].composite == Composite::Add);
        let (c, c0) = (&v.clips[0], &v0.clips[0]);
        for (a, b) in [(c.position, c0.position), (c.duration, c0.duration), (c.in_point, c0.in_point), (c.rate, c0.rate), (c.out_point, c0.out_point), (c.fade_in, c0.fade_in), (c.fade_out, c0.fade_out)] {
            assert!(close(a, b), "{a} {b}");
        }
        assert_eq!((c.fade_in_curve, c.fade_out_curve, c.opacity(), c.muted, c.locked, c.looping, c.stream), (Curve::Fast, Curve::Sharp, 0.5, true, true, false, 1));
        assert_eq!((c.name.as_str(), c.takes.len(), c.takes[0].in_point, c.takes[0].name.as_str()), ("Take A", 1, 1.0, "B"));
        assert_eq!(q.media_pool.iter().find(|m| Some(&m.id) == c.takes.first().map(|t| &t.media_id)).unwrap().path, PathBuf::from(r"C:\m\b.mp4"));
        // Pan/Crop keys come back (through VEGAS's corner form) within float precision.
        assert_eq!(c.pan_crop.keys.len(), 2);
        for (k, k0) in c.pan_crop.keys.iter().zip(&c0.pan_crop.keys) {
            assert!(close(k.t, k0.t) && k.curve == k0.curve);
            for (a, b) in [(k.v.cx, k0.v.cx), (k.v.cy, k0.v.cy), (k.v.w, k0.v.w), (k.v.h, k0.v.h), (k.v.rot, k0.v.rot)] {
                assert!((a - b).abs() < 1e-4, "{:?} vs {:?}", k.v, k0.v);
            }
        }
        assert_eq!(c.envelopes.iter().find(|e| e.kind == EnvKind::Velocity).unwrap().points, c0.envelopes.iter().find(|e| e.kind == EnvKind::Velocity).unwrap().points);
        assert_eq!(c.transition, c0.transition);
        // The yellow Flash-like Fade to Color goes out as VEGAS's Flash and comes back as Milan's own Flash (MVFX), its
        // colour as the tint.
        let (t, t0) = (c.transition_out.as_ref().unwrap(), c0.transition_out.as_ref().unwrap());
        assert_eq!((t0.plugin.as_str(), t.plugin.as_str()), ("milan.tr.fade_to_color", "milan.tr.flash"));
        for ch in ["r", "g", "b"] {
            assert_eq!(t.params.get(&format!("tint.{ch}")), t0.params.get(&format!("color.{ch}")), "{ch}");
        }
        let crop = c.fx.iter().find(|f| f.plugin == "milan.fx.crop").unwrap();
        let at0 = |k: &str| crop.params[k].keys[0].v;
        assert!((at0("left") - 0.5).abs() < 1e-6 && (at0("top") - 0.5).abs() < 1e-6 && at0("right").abs() < 1e-6 && at0("bottom").abs() < 1e-6);
        assert_eq!(c.group.is_some(), true);
        // The generated event.
        let g = q.media_pool.iter().find(|m| Some(&m.id) == v.clips[1].media_id.as_ref()).unwrap();
        assert_eq!((g.name.as_str(), g.generator.as_ref().unwrap().plugin.as_str(), g.generator.as_ref().unwrap().params["color.r"].keys[0].v), ("Red", "milan.gen.solid", 1.0));
        // Audio.
        let (a, a0) = (&q.timeline.audio_tracks[0], &p.timeline.audio_tracks[0]);
        assert_eq!((a.name.as_str(), a.solo, a.volume, a.pan), ("Music", true, 0.5, -0.25));
        assert_eq!(a.fx.iter().map(|f| (f.plugin.as_str(), f.enabled)).collect::<Vec<_>>(), [("milan.afx.eq", true), ("milan.afx.compressor", false)]);
        for (e, e0) in a.envelopes.iter().zip(&a0.envelopes) {
            assert_eq!(e.kind, e0.kind);
            for (k, k0) in e.points.keys.iter().zip(&e0.points.keys) {
                assert!(close(k.t, k0.t) && (k.v - k0.v).abs() < 1e-3 && k.curve == k0.curve, "{e:?} {e0:?}");
            }
        }
        let ac = &a.clips[0];
        assert!((ac.gain_db + 6.0).abs() < 1e-3 && ac.normalize && ac.group == c.group);
        assert_eq!(ac.fx[0].plugin, "milan.afx.reverb");
        assert_eq!(q.timeline.markers.iter().map(|m| (m.position, m.name.as_str())).collect::<Vec<_>>(), [(0.4, "Intro")]);
        assert_eq!(q.timeline.regions.iter().map(|r| (r.start, r.end)).collect::<Vec<_>>(), [(8.0, 10.0)]);
        assert_eq!(q.timeline.loop_region, Some((1.0, 3.0)));
        assert_eq!((q.timeline.master.video_fx[0].plugin.as_str(), q.timeline.master.video_fx[0].enabled), ("milan.fx.brightness_contrast", false));
        assert_eq!(q.timeline.master.audio_fx[0].plugin, "milan.afx.limiter");
        // The only import warnings are the export's own notes plus the placeholder, still unmapped on the way back.
        assert_eq!(w.len(), notes.len() + 1, "{w:?}");
        assert!(w.iter().all(|x| notes.contains(x) || x.contains("radialpixelate is not available")), "{w:?}");
    }

    #[test]
    fn the_file_names_vegas_plugins() {
        // MILAN_R26_SAMPLE_DIR: also write the sample file there, for checking the VEGAS-side script's JSON reader.
        if let Some(dir) = std::env::var_os("MILAN_R26_SAMPLE_DIR") {
            write(&sample(), &Path::new(&dir).join("sample.milan.json")).unwrap();
        }
        let (e, _) = export(&sample());
        let v = &e.tracks[0];
        assert_eq!(v.fx[0].id, "{Svfx:com.sonycreativesoftware:gaussianblur}");
        let hr = v.fx[0].params.iter().find(|p| p.name == "HorizontalRange").unwrap();
        assert!((hr.v.as_f64().unwrap() - 0.06).abs() < 1e-6 && hr.keys.len() == 2 && hr.kind == "Double");
        assert_eq!(v.fx[1].id, "{Svfx:com.sonycreativesoftware:radialpixelate}");
        let tint = v.fx[1].params.iter().find(|p| p.name == "Tint").unwrap();
        assert_eq!((tint.kind.as_str(), tint.v.as_array().unwrap().len()), ("RGBA", 4));
        assert!(v.fx[1].params.iter().any(|p| p.name == "Label" && p.kind == "String"));
        let push = v.events[0].transition_in.as_ref().unwrap();
        let dir = push.params.iter().find(|p| p.name == "Direction").unwrap();
        // Milan's Right (1) is VEGAS's Right (3), a Choice.
        assert_eq!((push.id.as_str(), dir.kind.as_str(), dir.v.as_i64()), ("{Svfx:com.sonycreativesoftware:push}", "Choice", Some(3)));
        assert_eq!(e.tracks[2].fx[0].name, "VEGAS Track EQ");
        assert_eq!(e.project.field_order, "UpperFieldFirst");
        assert!(rtf("Hi {x}\nthere", "Georgia").contains("\\fcharset0 Georgia;") && rtf("Hi {x}\nthere", "Georgia").contains("Hi \\{x\\}\\par\nthere"));
        assert_eq!(rtf_plain(&rtf("Hi {x}\nthere é", "Georgia")).unwrap(), ("Hi {x}\nthere é".to_string(), Some("Georgia".to_string())));
    }
}
