//! R40b: an 8-bit project composes bit for bit as the engine did before the float pipeline existed. The hashes below
//! were taken on the base engine (commit 5d0948e, before R40b) and must never move: a change to an 8-bit path shows
//! here as a new hash.
use eframe::egui::{Color32, ColorImage};
use std::sync::Arc;
use crate::engine::composite::{self, Sources};
use crate::engine::gpu::tests::{Noise, Rng, plan_n};
use crate::engine::plan::SrcAt;
use crate::engine::plugins::{self, PluginKind, preset_eval};
use crate::engine::{generate, transition, vfx};

/// FNV-1a over the pixels of `img`, folded into `h`.
fn fold(h: &mut u64, img: &ColorImage) {
    for b in bytemuck::cast_slice::<Color32, u8>(&img.pixels) {
        *h = (*h ^ *b as u64).wrapping_mul(0x0100_0000_01b3);
    }
}

const BASIS: u64 = 0xcbf2_9ce4_8422_2325;

/// The base engine's hash of 200 random frame plans (the GPU parity tests' generator: 1-4 layers in every blend mode,
/// opacity, Track Motion, Pan/Crop, dissolves, Fade to Color, compositing groups with Parent Motion, sources with
/// alpha, bilinear and nearest) composed by the CPU compositor.
const BASE_COMPOSE: u64 = 0x2020_8aa6_da23_06e2;
/// The base engine's hash of every built-in video FX, transition, generator and compositor at each of its presets on
/// one picture with alpha (text and credits generators left out: they draw with the system's fonts).
const BASE_PLUGINS: u64 = 0xb45a_1ba6_a8c5_6961;

#[test]
fn eight_bit_frames_are_bit_identical_to_the_base_engine() {
    let mut r = Rng(40);
    let mut h = BASIS;
    for _ in 0..200 {
        let (fp, [w, ht], bilinear) = plan_n(&mut r);
        fold(&mut h, &composite::compose(&fp, &mut Noise::default(), w, ht, bilinear, 3));
    }
    assert_eq!(h, BASE_COMPOSE, "8-bit composition changed: hash {h:#x}");
}

#[test]
fn eight_bit_plugins_are_bit_identical_to_the_base_engine() {
    let at = |id: &str, w: u32, h: u32| {
        let m = Arc::new(crate::engine::plan::MediaRef { id: id.into(), ..(*composite::tests::media(w, h)).clone() });
        Noise::default().get(&SrcAt { m, sf: Some(0), generator: None, nested: None }, w, h).unwrap()
    };
    let (a, b) = (at("a-r40b", 160, 90), at("m-r40b", 160, 90));
    let mut h = BASIS;
    for info in plugins::builtins() {
        for pr in info.presets {
            let mut e = preset_eval(info, Some(pr));
            crate::engine::mvfx::frame_vals(&mut e, 1.3, 30.0);
            match info.kind {
                PluginKind::VideoFx => {
                    let mut img = (*a).clone();
                    vfx::apply(&mut img, &e, 2);
                    fold(&mut h, &img);
                }
                PluginKind::Transition => {
                    for p in [0.25, 0.5, 0.8] {
                        fold(&mut h, &transition::mix(&e, &a, &b, p, 2));
                    }
                }
                PluginKind::Generator if info.id.contains("text") || info.id.contains("credits") => {}
                PluginKind::Generator => fold(&mut h, &generate::render(&e, 160, 90, 0.7)),
                PluginKind::Compositor => {
                    let mut acc = (*b).clone();
                    vfx::compositor(&mut acc, &a, &e, 0.8, 2);
                    fold(&mut h, &acc);
                }
                PluginKind::AudioFx => {}
            }
        }
    }
    assert_eq!(h, BASE_PLUGINS, "an 8-bit plug-in changed: hash {h:#x}");
}

/// The base engine's hash of the decoded frames of the 8-bit render below (FFV1, lossless RGB), with the bundled
/// FFmpeg 9.0.2 making the sources.
const BASE_RENDER: u64 = 0xd1ad_af97_e72f_b480;

/// Needs ffmpeg. `cargo test --release -- --ignored eight_bit_render_is_bit_identical`
/// R40b acceptance: `render_e2e`'s project (three layers of FFmpeg-made sources, a half level, Hard Light, a fade-in,
/// a velocity ramp, Fade to Color, a linear wipe out, a feathered mask, 2D Glow, Track Motion and motion blur), plus an
/// event FX, a track FX and a Video Output FX, rendered 8-bit to lossless FFV1: its frames hash as on the base engine.
#[test]
#[ignore]
fn eight_bit_render_is_bit_identical_to_the_base_engine() {
    use crate::project::{Clip, Curve, EnvKind, Envelope, Keyframe, Keyframes, MediaItem, Project, Track, TrackKind, Transition};
    use crate::render::{BUILTIN, Opts, Status};
    let dir = std::env::temp_dir().join("milan-agent").join("r40b").join(format!("bitexact-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let make = |name: &str, src: &str| {
        let path = dir.join(name);
        let ok = crate::proc::ffmpeg()
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", src, "-c:v", "libx264", "-qp", "0", "-pix_fmt", "yuv444p"])
            .arg(&path)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        crate::media::probe_media(&path).unwrap()
    };
    let clip = |m: &MediaItem, pos: f64, dur: f64| {
        let mut c = Clip::from_media(m, pos);
        (c.duration, c.out_point) = (dur, c.in_point + dur);
        c
    };
    let a = make("a.mp4", "testsrc2=d=2:s=320x240:r=30");
    let b = make("b.mp4", "smptebars=d=2:s=160x120:r=30");
    let c = make("c.mp4", "mandelbrot=s=320x240:r=30,trim=duration=2");
    let mut p = Project::new("bitexact");
    (p.settings.width, p.settings.height, p.settings.frame_rate) = (320, 240, 30.0);
    p.timeline.video_tracks = (0..3).map(|i| Track::new(&format!("Video {i}"), TrackKind::Video)).collect();
    p.timeline.video_tracks[2].clips.push(clip(&a, 0.0, 2.0));
    p.timeline.video_tracks[1].clips.push(clip(&b, 0.3, 1.7));
    p.timeline.video_tracks[1].volume = 0.5;
    let mut top = clip(&c, 0.6, 1.4);
    top.fade_in = 0.5;
    p.timeline.video_tracks[0].clips.push(top);
    let env = |kind, keys: &[(f64, f32, Curve)]| Envelope { kind, points: Keyframes { keys: keys.iter().map(|&(t, v, curve)| Keyframe { t, v, curve }).collect() } };
    p.timeline.video_tracks[2].clips[0].envelopes.push(env(EnvKind::Velocity, &[(0.0, 1.0, Curve::Linear), (1.0, 0.5, Curve::Hold)]));
    p.timeline.video_tracks[1].envelopes.push(env(EnvKind::FadeToColor, &[(0.3, 0.0, Curve::Linear), (2.0, -1.0, Curve::Linear)]));
    p.timeline.video_tracks[1].fade_colors[1] = [0, 128, 255];
    let fx = |id: &str| plugins::new_fx(plugins::find(id).unwrap(), None);
    {
        let t = &mut p.timeline.video_tracks[0].clips[0];
        (t.fade_out, t.transition_out) = (0.5, Some(Transition { plugin: "milan.tr.linear_wipe".into(), ..Default::default() }));
        let path = vec![[0.0, 0.0, 0.0, 0.0, 0.0, 0.0], [0.4, 0.0, 0.0, 0.0, 0.0, 0.0], [0.3, 1.0, 0.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0, 0.0, 0.0]];
        let mask = crate::project::Mask { path: Keyframes { keys: vec![Keyframe { t: 0.0, v: path, curve: Curve::Linear }] }, mode: crate::project::MaskMode::Negative, opacity: 1.0, feather: 0.05 };
        t.masks = vec![mask];
        t.fx.push(fx("milan.fx.sepia"));
    }
    p.timeline.video_tracks[0].glow.on = true;
    p.timeline.video_tracks[1].composite = crate::project::Composite::HardLight;
    p.timeline.video_tracks[1].fx.push(fx("milan.fx.gaussian_blur"));
    let m = |x| crate::project::Motion { x, w: 0.8, h: 0.8, ..Default::default() };
    p.timeline.video_tracks[2].motion = Keyframes { keys: vec![Keyframe { t: 0.0, v: m(-0.1), curve: Curve::Linear }, Keyframe { t: 2.0, v: m(0.1), curve: Curve::Linear }] };
    p.timeline.master.video_envelopes = vec![env(EnvKind::MotionBlur, &[(0.0, 0.1, Curve::Linear)])];
    p.timeline.master.video_fx.push(fx("milan.fx.vignette"));
    p.media_pool = vec![a, b, c];
    let t = BUILTIN.iter().find(|t| t.name == "FFV1 MKV archive").unwrap();
    let out = dir.join("out.mkv");
    let job = super::render::start(&p, t, out.clone(), (0.0, 2.0), Opts::default(), eframe::egui::Context::default());
    while job.status() == Status::Running {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(job.status(), Status::Done);
    let o = crate::proc::ffmpeg().args(["-v", "error", "-i"]).arg(&out).args(["-f", "rawvideo", "-pix_fmt", "rgb24", "-"]).output().unwrap();
    assert!(o.status.success() && o.stdout.len() == 320 * 240 * 3 * 60, "{} bytes", o.stdout.len());
    let mut h = BASIS;
    for b in &o.stdout {
        h = (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(h, BASE_RENDER, "the 8-bit render changed: hash {h:#x}");
}
