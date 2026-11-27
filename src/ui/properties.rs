//! Properties panel: selected clip, selected media, or project settings.
use eframe::egui::{self, *};
use crate::project::*;
use super::{ClipRef, theme};

pub fn draw_properties(
    ui: &mut Ui,
    project: &Project,
    selected_clip: &Option<ClipRef>,
    selected_media: &Option<usize>,
) {
    let accent = theme::get().accent;
    ui.heading(RichText::new("Properties").size(13.0));
    ui.separator();

    if let Some(clip_ref) = selected_clip {
        let tracks = if clip_ref.is_video {
            &project.timeline.video_tracks
        } else {
            &project.timeline.audio_tracks
        };
        if let Some(track) = tracks.get(clip_ref.track_idx) {
            if let Some(clip) = track.clips.get(clip_ref.clip_idx) {
                ui.label(RichText::new("Clip Properties").color(accent).size(12.0));
                ui.add_space(4.0);
                egui::Grid::new("clip_props").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    prop_row(ui, "Name", &clip.name);
                    prop_row(ui, "Position", &format!("{:.3}s", clip.position));
                    prop_row(ui, "Duration", &format!("{:.3}s", clip.duration));
                    prop_row(ui, "In Point", &format!("{:.3}s", clip.in_point));
                    prop_row(ui, "Out Point", &format!("{:.3}s", clip.out_point));
                    prop_row(ui, "Track", &track.name);
                });
                return;
            }
        }
    }

    if let Some(idx) = selected_media {
        if let Some(item) = project.media_pool.get(*idx) {
            ui.label(RichText::new("Media Properties").color(accent).size(12.0));
            ui.add_space(4.0);
            egui::Grid::new("media_props").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                prop_row(ui, "Name", &item.name);
                prop_row(ui, "Type", &format!("{:?}", item.media_type));
                prop_row(ui, "Duration", &format!("{:.3}s", item.duration));
                if let Some(w) = item.width {
                    prop_row(ui, "Width", &format!("{w}"));
                }
                if let Some(h) = item.height {
                    prop_row(ui, "Height", &format!("{h}"));
                }
                if let Some(fps) = item.frame_rate {
                    prop_row(ui, "Frame Rate", &format!("{fps:.2}"));
                }
                if let Some(sr) = item.sample_rate {
                    prop_row(ui, "Sample Rate", &format!("{sr} Hz"));
                }
                if let Some(ref codec) = item.codec {
                    prop_row(ui, "Codec", codec);
                }
                // R38: a still image sequence; R41: a spanned clip's parts.
                if let Some(seq) = &item.sequence {
                    prop_row(ui, "Sequence", &seq.describe());
                }
                if !item.spanned.is_empty() {
                    prop_row(ui, "Spanned parts", &format!("{} files joined", item.spanned.len()));
                }
                // R40a: bit depth, colour space, HDR metadata and how the source reaches the timeline.
                if item.generator.is_none() && !item.nested && item.media_type != MediaType::Audio && item.unsupported.is_none() {
                    if let Some(b) = item.bit_depth {
                        prop_row(ui, "Bit depth", &format!("{b}-bit"));
                    }
                    prop_row(ui, "Color space", &item.color_space_name());
                    if let Some(t) = &item.transfer {
                        prop_row(ui, "Transfer", t);
                    }
                    if let Some(p) = &item.primaries {
                        prop_row(ui, "Primaries", p);
                    }
                    if let Some(m) = &item.color_matrix {
                        prop_row(ui, "Matrix", m);
                    }
                    if let Some(md) = item.mastering {
                        if md.max_luminance > 0.0 {
                            prop_row(ui, "Mastering display", &format!("{:.4} - {:.0} cd/m²", md.min_luminance, md.max_luminance));
                        }
                        if md.max_cll > 0 || md.max_fall > 0 {
                            prop_row(ui, "Content light level", &format!("MaxCLL {} cd/m², MaxFALL {} cd/m²", md.max_cll, md.max_fall));
                        }
                    }
                    let cm = project.settings.color_management;
                    let handling = match crate::engine::color::badge(item, cm) {
                        Some("HDR → SDR") => match cm {
                            crate::project::ColorManagement::SdrClip => "HDR → SDR: Rec.709, highlights clipped (Project Properties › Color management)",
                            _ => "HDR → SDR: tone-mapped to Rec.709, reference white 203 cd/m² (Project Properties › Color management)",
                        },
                        Some("BT.2020 → Rec.709") => "BT.2020 → Rec.709 (Project Properties › Color management)",
                        Some(_) => "Linear light shown with a simple sRGB transfer (R40b brings a float pipeline)",
                        None if item.hdr().is_some() => "None: decoded as Rec.709 (Color management is Off)",
                        None => "SDR, used as it is",
                    };
                    prop_row(ui, "View transform", handling);
                }
                // R17a/R41: timecode, camera make/model and reel.
                if let Some(tc) = item.timecode {
                    prop_row(ui, "Timecode", &format_timecode(tc, item.frame_rate.filter(|f| *f > 0.0).unwrap_or(project.settings.frame_rate)));
                }
                if let Some(c) = &item.camera {
                    prop_row(ui, "Camera", c);
                }
                if let Some(r) = &item.reel {
                    prop_row(ui, "Reel", r);
                }
                // R37: the capability verdict (decode, proxy, render) from the FFmpeg table.
                let caps = crate::engine::caps::try_get();
                for (k, v) in crate::engine::caps::verdict(item, caps.as_deref()) {
                    prop_row(ui, &k, &v);
                }
            });
            return;
        }
    }

    ui.label(RichText::new("Project Settings").color(accent).size(12.0));
    ui.add_space(4.0);
    egui::Grid::new("proj_props").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        prop_row(ui, "Resolution", &format!("{}x{}", project.settings.width, project.settings.height));
        prop_row(ui, "Frame Rate", &format!("{:.2} fps", project.settings.frame_rate));
        prop_row(ui, "Sample Rate", &format!("{} Hz", project.settings.sample_rate));
        prop_row(ui, "Duration", &format_duration_short(project.timeline.duration()));
    });
}

fn prop_row(ui: &mut Ui, label: &str, value: &str) {
    let t = theme::get();
    ui.label(RichText::new(label).color(t.text_label).size(11.0));
    ui.label(RichText::new(value).color(t.text_value).size(11.0));
    ui.end_row();
}

fn format_duration_short(seconds: f64) -> String {
    let total = seconds as u64;
    let s = total % 60;
    let m = (total / 60) % 60;
    let h = total / 3600;
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}
