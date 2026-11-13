//! Render As › Customize Template (VEGAS's Custom Settings): the template's name and notes with Save and Delete, and
//! Video, Audio and Project pages. OK renders with the settings as they are; Save keeps them as a user template.
use eframe::egui::*;
use crate::decode::Quality;
use crate::project::{FieldOrder, PARS, ProjectSettings};
use crate::render::{ACODECS, AudioEnc, BUILTIN, CONTAINERS, Rate, Template, VCODECS, VideoEnc, acodec, container, vcodec};

/// Frame sizes offered besides the project's and a custom one.
const SIZES: [(u32, u32); 8] = [(3840, 2160), (2560, 1440), (1920, 1080), (1280, 720), (1080, 1920), (1080, 1080), (720, 480), (720, 576)];
const RATES: [f64; 9] = [23.976, 24.0, 25.0, 29.97, 30.0, 48.0, 50.0, 59.94, 60.0];
const SAMPLE_RATES: [u32; 6] = [22_050, 32_000, 44_100, 48_000, 88_200, 96_000];

pub struct Editor {
    pub draft: Template,
    tab: usize,
}

/// How the editor closed.
pub enum Closed {
    /// OK: render with this.
    Ok(Template),
    Cancel,
}

impl Editor {
    pub fn new(t: &Template) -> Self {
        Self { draft: t.clone(), tab: 0 }
    }

    /// Draws the dialog; `user` is the user templates, which Save and Delete change (`changed` says so).
    pub fn show(&mut self, ctx: &Context, s: &ProjectSettings, user: &mut Vec<Template>, changed: &mut bool) -> Option<Closed> {
        let d = &mut self.draft;
        let tab = &mut self.tab;
        let err = d.check().err();
        let builtin = BUILTIN.iter().any(|b| b.name == d.name.trim());
        let (hit, close) = super::modal(ctx, "render_template", "Custom Settings", vec2(520.0, 560.0), |ui| {
            let mut hit = None;
            ui.horizontal(|ui| {
                ui.label("Template:");
                ui.add(TextEdit::singleline(&mut d.name).desired_width(300.0));
                let why = if builtin { "A built-in template keeps its name: type a new one to save." } else { "Save Template" };
                if ui.add_enabled(!builtin && !d.name.trim().is_empty() && err.is_none(), Button::new("Save")).on_hover_text(why).on_disabled_hover_text(why).clicked() {
                    d.name = d.name.trim().to_string();
                    match user.iter_mut().find(|u| u.name == d.name) {
                        Some(u) => *u = d.clone(),
                        None => user.push(d.clone()),
                    }
                    *changed = true;
                }
                let mine = user.iter().position(|u| u.name == d.name.trim());
                if ui.add_enabled(mine.is_some(), Button::new("Delete")).on_hover_text("Delete this user template").clicked() {
                    if let Some(k) = mine {
                        user.remove(k);
                        *changed = true;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Description:");
                ui.add(TextEdit::singleline(&mut d.notes).desired_width(ui.available_width()));
            });
            super::row(ui, "Format:", 80.0, true, |ui| {
                ComboBox::from_id_salt("tpl_container").width(260.0).selected_text(container(&d.container).map_or(d.container.as_str(), |c| c.label)).show_ui(ui, |ui| {
                    for c in &CONTAINERS {
                        if ui.selectable_label(d.container == c.id, format!("{} (.{})", c.label, if c.ext.is_empty() { "png, tif, exr, dpx" } else { c.ext })).clicked() {
                            set_container(d, c.id);
                        }
                    }
                });
            });
            super::tabs(ui, &[&["Video", "Audio", "Project"]], tab);
            super::page(ui, 330.0, |ui| match *tab {
                0 => video_page(ui, d, s),
                1 => audio_page(ui, d, s),
                _ => project_page(ui, d),
            });
            if let Some(e) = &err {
                ui.colored_label(crate::ui::theme::get().warning_text, e);
            }
            ui.add_space(6.0);
            if let Some(k) = super::button_row(ui, &[("OK", err.is_none()), ("Cancel", true)]) {
                hit = Some(k);
            }
            hit
        });
        match hit {
            Some(0) => Some(Closed::Ok(self.draft.clone())),
            Some(_) => Some(Closed::Cancel),
            None if close => Some(Closed::Cancel),
            None => None,
        }
    }
}

/// A new container keeps the codecs it can hold and takes its first ones for the rest.
fn set_container(d: &mut Template, id: &str) {
    let Some(c) = container(id) else { return };
    d.container = id.into();
    match (&mut d.video, c.video.first()) {
        (Some(v), Some(first)) if !c.video.contains(&v.codec.as_str()) => *v = VideoEnc { codec: first.to_string(), ..VideoEnc::default() }.with_codec_defaults(),
        (None, Some(first)) => d.video = Some(VideoEnc { codec: first.to_string(), ..VideoEnc::default() }.with_codec_defaults()),
        (_, None) => d.video = None,
        _ => {}
    }
    match (&mut d.audio, c.audio.first()) {
        (Some(a), Some(first)) if !c.audio.contains(&a.codec.as_str()) => *a = AudioEnc { codec: first.to_string(), ..AudioEnc::default() }.with_codec_defaults(),
        (None, Some(first)) if !c.video.is_empty() || d.video.is_none() => d.audio = Some(AudioEnc { codec: first.to_string(), ..AudioEnc::default() }.with_codec_defaults()),
        (_, None) => d.audio = None,
        _ => {}
    }
    if d.video.is_none() && d.audio.is_none() {
        d.audio = c.audio.first().map(|a| AudioEnc { codec: a.to_string(), ..AudioEnc::default() }.with_codec_defaults());
    }
}

trait CodecDefaults {
    fn with_codec_defaults(self) -> Self;
}

impl CodecDefaults for VideoEnc {
    /// The codec's first pixel format, profile and preset, and a rate control it takes.
    fn with_codec_defaults(mut self) -> Self {
        if let Some(k) = vcodec(&self.codec) {
            (self.pix_fmt, self.profile, self.preset) = (String::new(), String::new(), String::new());
            self.rate = match k.rates.chars().next() {
                Some('Q') => Rate::Quality((k.q.0 + k.q.1) / 2),
                Some('V') => Rate::Vbr { avg: 16_000, max: 24_000 },
                Some('C') => Rate::Cbr(16_000),
                _ => Rate::Default,
            };
            if k.enc == "libx264" || k.enc == "libx265" {
                self.rate = Rate::Quality(20);
            }
        }
        self
    }
}

impl CodecDefaults for AudioEnc {
    /// A middle bit rate of the codec's (none for lossless ones).
    fn with_codec_defaults(mut self) -> Self {
        if let Some(k) = acodec(&self.codec) {
            self.kbps = k.kbps.get(k.kbps.len() / 2).copied().unwrap_or(0);
        }
        self
    }
}

fn video_page(ui: &mut Ui, d: &mut Template, s: &ProjectSettings) {
    let Some(c) = container(&d.container) else { return };
    let mut on = d.video.is_some();
    if ui.add_enabled(!c.video.is_empty(), Checkbox::new(&mut on, "Include video")).changed() {
        d.video = on.then(|| VideoEnc { codec: c.video[0].into(), ..VideoEnc::default() }.with_codec_defaults());
    }
    let Some(v) = &mut d.video else { return };
    super::row(ui, "Codec:", 120.0, true, |ui| {
        ComboBox::from_id_salt("tpl_vcodec").width(220.0).selected_text(vcodec(&v.codec).map_or(v.codec.as_str(), |k| k.label)).show_ui(ui, |ui| {
            for k in VCODECS.iter().filter(|k| c.video.contains(&k.enc)) {
                if ui.selectable_label(v.codec == k.enc, k.label).clicked() && v.codec != k.enc {
                    *v = VideoEnc { codec: k.enc.into(), size: v.size, fps: v.fps, field: v.field, par: v.par, ..VideoEnc::default() }.with_codec_defaults();
                }
            }
        });
    });
    let Some(k) = vcodec(&v.codec) else { return };
    super::row(ui, "Frame size:", 120.0, true, |ui| {
        let label = |z: Option<(u32, u32)>| z.map_or(format!("Project ({}x{})", s.width, s.height), |(w, h)| format!("{w}x{h}"));
        ComboBox::from_id_salt("tpl_size").width(160.0).selected_text(label(v.size)).show_ui(ui, |ui| {
            ui.selectable_value(&mut v.size, None, label(None));
            for z in SIZES {
                ui.selectable_value(&mut v.size, Some(z), label(Some(z)));
            }
        });
        if let Some((w, h)) = &mut v.size {
            ui.add(DragValue::new(w).range(16..=8192));
            ui.label("x");
            ui.add(DragValue::new(h).range(16..=8192));
        } else if ui.button("Custom").clicked() {
            v.size = Some((s.width, s.height));
        }
    });
    super::row(ui, "Frame rate:", 120.0, true, |ui| {
        let label = |f: Option<f64>| f.map_or(format!("Project ({:.3})", s.frame_rate), |f| format!("{f:.3}"));
        ComboBox::from_id_salt("tpl_fps").width(160.0).selected_text(label(v.fps)).show_ui(ui, |ui| {
            ui.selectable_value(&mut v.fps, None, label(None));
            for f in RATES {
                ui.selectable_value(&mut v.fps, Some(f), label(Some(f)));
            }
        });
    });
    super::row(ui, "Field order:", 120.0, k.interlace, |ui| {
        let label = |f: Option<FieldOrder>| f.map_or(format!("Project ({})", s.field_order.name()), |f| f.name().to_string());
        ComboBox::from_id_salt("tpl_field").width(220.0).selected_text(label(v.field)).show_ui(ui, |ui| {
            ui.selectable_value(&mut v.field, None, label(None));
            for f in FieldOrder::ALL {
                ui.selectable_value(&mut v.field, Some(f), label(Some(f)));
            }
        });
    });
    super::row(ui, "Pixel aspect ratio:", 120.0, true, |ui| {
        let label = |p: Option<f64>| p.map_or(format!("Project ({:.4})", s.par), |p| PARS.iter().find(|x| (x.0 - p).abs() < 1e-4).map_or(format!("{p:.4}"), |x| x.1.to_string()));
        ComboBox::from_id_salt("tpl_par").width(220.0).selected_text(label(v.par)).show_ui(ui, |ui| {
            ui.selectable_value(&mut v.par, None, label(None));
            for (p, _) in PARS {
                ui.selectable_value(&mut v.par, Some(p), label(Some(p)));
            }
        });
    });
    if !k.profile.1.is_empty() {
        super::row(ui, "Profile:", 120.0, true, |ui| {
            let cur = v.profile().map_or("", |p| p.1);
            ComboBox::from_id_salt("tpl_profile").width(220.0).selected_text(cur).show_ui(ui, |ui| {
                for p in k.profile.1 {
                    if ui.selectable_label(cur == p.1, p.1).clicked() {
                        v.profile = p.0.into();
                        // A profile that takes one pixel format gets it.
                        if let [only] = p.2 {
                            v.pix_fmt = only.to_string();
                        } else if !p.2.is_empty() && !p.2.contains(&v.pix_fmt()) {
                            v.pix_fmt = p.2[0].into();
                        }
                    }
                }
            });
        });
    }
    super::row(ui, "Pixel format:", 120.0, true, |ui| {
        let cur = v.pix_fmt().to_string();
        ComboBox::from_id_salt("tpl_pix").width(220.0).selected_text(&cur).show_ui(ui, |ui| {
            for p in k.pix_fmts {
                if ui.selectable_label(cur == *p, *p).clicked() {
                    v.pix_fmt = p.to_string();
                }
            }
        });
    });
    if !k.preset.0.is_empty() {
        super::row(ui, "Encoder speed:", 120.0, true, |ui| {
            let cur = if v.preset.is_empty() { k.preset.1[0].to_string() } else { v.preset.clone() };
            ComboBox::from_id_salt("tpl_preset").width(220.0).selected_text(&cur).show_ui(ui, |ui| {
                for p in k.preset.1 {
                    if ui.selectable_label(cur == *p, *p).clicked() {
                        v.preset = p.to_string();
                    }
                }
            });
        });
    }
    rate_controls(ui, v, k);
}

fn rate_controls(ui: &mut Ui, v: &mut VideoEnc, k: &crate::render::VCodec) {
    let names = |r: &Rate| match r {
        Rate::Default => "Set by the profile",
        Rate::Quality(_) => "Constant quality",
        Rate::Vbr { .. } => "Variable bit rate (VBR)",
        Rate::Cbr(_) => "Constant bit rate (CBR)",
    };
    let choices: Vec<Rate> = k
        .rates
        .chars()
        .map(|c| match c {
            'Q' => Rate::Quality((k.q.0 + k.q.1) / 2),
            'V' => Rate::Vbr { avg: 16_000, max: 24_000 },
            'C' => Rate::Cbr(16_000),
            _ => Rate::Default,
        })
        .collect();
    super::row(ui, "Rate control:", 120.0, choices.len() > 1, |ui| {
        ComboBox::from_id_salt("tpl_rate").width(220.0).selected_text(names(&v.rate)).show_ui(ui, |ui| {
            for c in choices {
                if ui.selectable_label(std::mem::discriminant(&v.rate) == std::mem::discriminant(&c), names(&c)).clicked() && std::mem::discriminant(&v.rate) != std::mem::discriminant(&c) {
                    v.rate = c;
                }
            }
        });
    });
    match &mut v.rate {
        Rate::Default => {}
        Rate::Quality(q) => {
            super::row(ui, "Quality (lower is better):", 160.0, true, |ui| ui.add(DragValue::new(q).range(k.q.0..=k.q.1)));
        }
        Rate::Vbr { avg, max } => {
            super::row(ui, "Average (kbps):", 120.0, true, |ui| ui.add(DragValue::new(avg).range(100..=800_000).speed(100)));
            super::row(ui, "Maximum (kbps):", 120.0, true, |ui| ui.add(DragValue::new(max).range(100..=800_000).speed(100)));
        }
        Rate::Cbr(r) => {
            super::row(ui, "Bit rate (kbps):", 120.0, true, |ui| ui.add(DragValue::new(r).range(100..=800_000).speed(100)));
        }
    }
}

fn audio_page(ui: &mut Ui, d: &mut Template, s: &ProjectSettings) {
    let Some(c) = container(&d.container) else { return };
    let mut on = d.audio.is_some();
    if ui.add_enabled(!c.audio.is_empty(), Checkbox::new(&mut on, "Include audio")).changed() {
        d.audio = on.then(|| AudioEnc { codec: c.audio[0].into(), ..AudioEnc::default() }.with_codec_defaults());
    }
    let container_id = d.container.clone();
    let Some(a) = &mut d.audio else { return };
    super::row(ui, "Codec:", 120.0, true, |ui| {
        ComboBox::from_id_salt("tpl_acodec").width(220.0).selected_text(acodec(&a.codec).map_or(a.codec.as_str(), |k| k.label)).show_ui(ui, |ui| {
            for k in ACODECS.iter().filter(|k| c.audio.contains(&k.enc)) {
                if ui.selectable_label(a.codec == k.enc, k.label).clicked() && a.codec != k.enc {
                    *a = AudioEnc { codec: k.enc.into(), rate: a.rate, channels: a.channels, ..AudioEnc::default() }.with_codec_defaults();
                }
            }
        });
    });
    let Some(k) = acodec(&a.codec) else { return };
    super::row(ui, "Sample rate (Hz):", 120.0, true, |ui| {
        let label = |r: Option<u32>| r.map_or(format!("Project ({})", s.sample_rate), |r| r.to_string());
        ComboBox::from_id_salt("tpl_ar").width(160.0).selected_text(label(a.rate)).show_ui(ui, |ui| {
            ui.selectable_value(&mut a.rate, None, label(None));
            for r in SAMPLE_RATES.iter().copied().filter(|r| k.rates.is_empty() || k.rates.contains(r)) {
                ui.selectable_value(&mut a.rate, Some(r), label(Some(r)));
            }
        });
        ui.label(format!("renders at {}", a.sample_rate(s.sample_rate, &container_id)));
    });
    super::row(ui, "Channels:", 120.0, true, |ui| {
        ui.radio_value(&mut a.channels, 2, "Stereo");
        ui.radio_value(&mut a.channels, 1, "Mono");
    });
    if !k.kbps.is_empty() {
        super::row(ui, "Bit rate (kbps):", 120.0, true, |ui| {
            ComboBox::from_id_salt("tpl_ab").width(160.0).selected_text(a.kbps.to_string()).show_ui(ui, |ui| {
                for r in k.kbps {
                    ui.selectable_value(&mut a.kbps, *r, r.to_string());
                }
            });
        });
    }
    if k.bits {
        super::row(ui, "Bit depth:", 120.0, true, |ui| {
            let label = |b: Option<u8>| b.map_or(format!("Project ({})", s.bit_depth), |b| if b == 32 { "32 (float)".into() } else { b.to_string() });
            ComboBox::from_id_salt("tpl_bits").width(160.0).selected_text(label(a.bits)).show_ui(ui, |ui| {
                ui.selectable_value(&mut a.bits, None, label(None));
                for b in [8, 16, 24, 32] {
                    ui.selectable_value(&mut a.bits, Some(b), label(Some(b)));
                }
            });
        });
    }
}

fn project_page(ui: &mut Ui, d: &mut Template) {
    super::row(ui, "Video rendering quality:", 160.0, d.video.is_some(), |ui| {
        let label = |q: Option<Quality>| q.map_or("Use project setting".to_string(), |q| format!("{q:?}"));
        ComboBox::from_id_salt("tpl_quality").width(180.0).selected_text(label(d.quality)).show_ui(ui, |ui| {
            ui.selectable_value(&mut d.quality, None, label(None));
            for q in Quality::ALL {
                ui.selectable_value(&mut d.quality, Some(q), label(Some(q)));
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Changing the format keeps the codecs it can hold and gives it valid ones for the rest.
    #[test]
    fn a_new_format_gets_codecs_it_holds() {
        let mut t = BUILTIN[0].clone();
        set_container(&mut t, "webm");
        assert_eq!((t.video.as_ref().unwrap().codec.as_str(), t.audio.as_ref().unwrap().codec.as_str()), ("libvpx-vp9", "libopus"));
        assert_eq!(t.check(), Ok(()));
        set_container(&mut t, "wav");
        assert!(t.video.is_none() && t.audio.as_ref().unwrap().codec == "pcm" && t.check().is_ok());
        set_container(&mut t, "image");
        assert!(t.audio.is_none() && t.video.as_ref().unwrap().codec == "png" && t.check().is_ok());
        set_container(&mut t, "mkv");
        assert!(t.audio.is_some() && t.check().is_ok(), "{t:?}");
        for c in &CONTAINERS {
            let mut t = BUILTIN[0].clone();
            set_container(&mut t, c.id);
            assert_eq!(t.check(), Ok(()), "{}", c.id);
        }
    }
}
