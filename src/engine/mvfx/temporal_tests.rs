//! MVFX acceptance: the temporal FX are deterministic (the same frame twice, and a seek against sequential play,
//! give identical pixels, on the CPU and on the GPU), and every FX and transition matches the CPU on the GPU at frames
//! other than the first, earlier source frames and all.
use eframe::egui::{Color32, ColorImage};
use std::collections::HashMap;
use std::sync::Arc;
use crate::engine::composite::{self, Compositor, Sources};
use crate::engine::plan::{self, SrcAt};
use crate::engine::plugins::{PluginKind, find, new_fx};
use crate::project::{Clip, MediaItem, MediaType, Project};

const W: usize = 320;
const H: usize = 180;

/// Footage whose content moves with the frame number: a gradient, a bright square travelling right and down, noise
/// so sampling has to be exact. Counts what it decodes.
#[derive(Default)]
struct Moving {
    made: HashMap<(i64, u32, u32), Arc<ColorImage>>,
    asked: usize,
}

impl Sources for Moving {
    fn get(&mut self, s: &SrcAt, w: u32, h: u32) -> Option<Arc<ColorImage>> {
        self.asked += 1;
        let n = s.sf.unwrap_or(0);
        Some(
            self.made
                .entry((n, w, h))
                .or_insert_with(|| {
                    let (bx, by) = ((n * 5) as u32 % w, (n * 2) as u32 % h);
                    Arc::new(crate::engine::generate::fill(w as usize, h as usize, |x, y| {
                        let (x, y) = (x as u32, y as u32);
                        let noise = crate::engine::mvfx::pm::hash(x * 7919 + y * 104_729) & 15;
                        if x.wrapping_sub(bx) < w / 6 && y.wrapping_sub(by) < h / 4 {
                            Color32::from_rgb(250, 240 - noise as u8, 90)
                        } else {
                            Color32::from_rgb((x * 255 / w + noise) as u8, (y * 255 / h) as u8, (120 + n * 3 % 100) as u8)
                        }
                    }))
                })
                .clone(),
        )
    }
}

/// A project of one 10 s clip at 30 fps carrying `fx` as event FX.
fn project(fx: Vec<crate::project::Fx>) -> Project {
    let mut p = Project::new("mvfx");
    p.settings.frame_rate = 30.0;
    (p.settings.width, p.settings.height) = (W as u32 * 6, H as u32 * 6);
    p.media_pool.push(MediaItem { id: "m".into(), media_type: MediaType::Video, duration: 10.0, width: Some(640), height: Some(360), frame_rate: Some(30.0), ..Default::default() });
    let mut c = Clip::from_media(&p.media_pool[0], 0.0);
    c.duration = 10.0;
    c.fx = fx;
    p.timeline.video_tracks[0].clips = vec![c];
    p
}

/// Every temporal FX at a preset that moves (its second preset), as event FX.
fn temporal_fx() -> Vec<(&'static str, crate::project::Fx)> {
    crate::engine::mvfx::all()
        .filter(|i| i.kind == PluginKind::VideoFx && crate::engine::mvfx::temporal(i.id))
        .map(|i| (i.id, new_fx(i, i.presets.get(1).or(i.presets.first()))))
        .collect()
}

fn frame(c: &dyn Compositor, vp: &plan::VideoPlan, n: i64, src: &mut Moving) -> ColorImage {
    (*c.compose(&plan::frame_plan(vp, n), src, W, H, true, 3).expect("composed").image()).clone()
}

/// Same frame twice, and frames played in order against the same frames reached by seeking (in another order, with
/// nothing decoded before), are identical, for each temporal FX on its own, on the CPU and (when there is one) on the
/// GPU; the GPU stays within the FX's tolerance of the CPU.
#[test]
fn temporal_fx_are_deterministic() {
    let gpu = crate::engine::gpu::tests::gpu_or_skip("temporal_fx_are_deterministic");
    let cpu: Arc<dyn Compositor> = Arc::new(composite::CpuCompositor);
    let mut comps = vec![("cpu", cpu)];
    if let Some(g) = gpu.clone() {
        comps.push(("gpu", g as Arc<dyn Compositor>));
    }
    let frames: Vec<i64> = (20..34).collect();
    for (id, fx) in temporal_fx() {
        let vp = plan::compile(&project(vec![fx]));
        let mut seen: Vec<Vec<ColorImage>> = Vec::new();
        for (name, c) in &comps {
            let mut src = Moving::default();
            let played: Vec<ColorImage> = frames.iter().map(|&n| frame(c.as_ref(), &vp, n, &mut src)).collect();
            let again = frame(c.as_ref(), &vp, frames[5], &mut src);
            assert!(again.pixels == played[5].pixels, "{id} on the {name}: the same frame twice differs");
            for (i, &n) in frames.iter().enumerate().rev().step_by(3) {
                let mut fresh = Moving::default();
                let sought = frame(c.as_ref(), &vp, n, &mut fresh);
                assert!(sought.pixels == played[i].pixels, "{id} on the {name}: frame {n} sought differs from played");
            }
            seen.push(played);
        }
        let changes = seen[0].windows(2).filter(|w| w[0].pixels != w[1].pixels).count();
        assert!(changes > 0, "{id} never changes over {} frames", frames.len());
        if seen.len() == 2 {
            let tol = super::tolerance(id);
            for (i, (a, b)) in seen[0].iter().zip(&seen[1]).enumerate() {
                let d = crate::engine::gpu::tests::diff(a, b);
                assert!(d.0 <= tol, "{id} frame {}: the GPU differs from the CPU by {} ({} channels)", frames[i], d.0, d.1);
            }
        }
    }
}

/// Echo, Stutter and Datamosh read the frames they say; Datamosh's smear is the plain frame outside its window and
/// different inside it; Stutter holds frames.
#[test]
fn source_readers_read_earlier_frames() {
    let cpu = composite::CpuCompositor;
    let plain = plan::compile(&project(Vec::new()));
    let datamosh = find("milan.fx.datamosh").unwrap();
    let mut fx = new_fx(datamosh, None);
    fx.params.insert("interval".into(), crate::project::Keyframes { keys: vec![crate::project::Keyframe { t: 0.0, v: 10.0, curve: crate::project::Curve::Linear }] });
    fx.params.insert("length".into(), crate::project::Keyframes { keys: vec![crate::project::Keyframe { t: 0.0, v: 5.0, curve: crate::project::Curve::Linear }] });
    let moshed = plan::compile(&project(vec![fx]));
    let mut src = Moving::default();
    for n in 20..32 {
        let (a, b) = (frame(&cpu, &plain, n, &mut src), frame(&cpu, &moshed, n, &mut src));
        let inside = (1..=5).contains(&(n % 10));
        assert_eq!(a.pixels != b.pixels, inside, "datamosh at frame {n}");
    }
    let stutter = find("milan.fx.stutter").unwrap();
    let held = plan::compile(&project(vec![new_fx(stutter, stutter.presets.iter().find(|p| p.name == "Choppy"))]));
    // Hold 4: frames 40..43 all show source frame 40.
    let shown: Vec<ColorImage> = (40..44).map(|n| frame(&cpu, &held, n, &mut src)).collect();
    assert!(shown.windows(2).all(|w| w[0].pixels == w[1].pixels));
    assert!(shown[0].pixels == frame(&cpu, &plain, 40, &mut src).pixels);
    let mut counted = Moving::default();
    let echo = find("milan.fx.echo").unwrap();
    let vp = plan::compile(&project(vec![new_fx(echo, None)]));
    frame(&cpu, &vp, 30, &mut counted);
    assert_eq!(counted.made.len(), 5, "Echo's default reads 4 earlier frames besides this one");
}

/// GPU parity for every MVFX video FX and transition at a frame other than the first (frame 41): the temporal ones'
/// grain, flicker, shake and strobe at that frame, the source readers with the frames before it.
#[test]
fn gpu_mvfx_match_cpu_later_frames() {
    let Some(g) = crate::engine::gpu::tests::gpu_or_skip("gpu_mvfx_match_cpu_later_frames") else { return };
    let mut n = 0;
    for info in crate::engine::mvfx::all() {
        for pr in info.presets {
            let fx = new_fx(info, Some(pr));
            let p = if info.kind == PluginKind::Transition {
                let mut p = project(Vec::new());
                p.media_pool.push(MediaItem { id: "m2".into(), media_type: MediaType::Video, duration: 10.0, width: Some(480), height: Some(270), frame_rate: Some(30.0), ..Default::default() });
                let c = &mut p.timeline.video_tracks[0].clips[0];
                c.duration = 2.0;
                let mut b = Clip::from_media(&p.media_pool[1], 1.0);
                b.duration = 3.0;
                b.transition = Some(crate::edit::transition_of(&fx));
                p.timeline.video_tracks[0].clips.push(b);
                p
            } else {
                project(vec![fx])
            };
            let vp = plan::compile(&p);
            let at = if info.kind == PluginKind::Transition { 39 } else { 41 };
            let fp = plan::frame_plan(&vp, at);
            let mut src = Moving::default();
            let cpu = composite::compose(&fp, &mut src, W, H, true, 3);
            let gpu = (*g.compose(&fp, &mut src, W, H, true, 3).expect("the GPU composed it").image()).clone();
            let d = crate::engine::gpu::tests::diff(&cpu, &gpu);
            let tol = super::tolerance(info.id);
            if d.0 > tol {
                let dir = std::env::temp_dir().join("milan-agent").join("mvfx").join("fail");
                std::fs::create_dir_all(&dir).unwrap();
                let name = format!("{}-{}", info.id, pr.name).replace(|c: char| !c.is_ascii_alphanumeric(), "_");
                crate::demo::save_png(&dir.join(format!("{name}-cpu.png")), &cpu).unwrap();
                crate::demo::save_png(&dir.join(format!("{name}-gpu.png")), &gpu).unwrap();
            }
            assert!(d.0 <= tol, "{} preset {}: the GPU differs from the CPU by {} ({} channels)", info.id, pr.name, d.0, d.1);
            n += 1;
        }
    }
    println!("gpu_mvfx_match_cpu_later_frames on {}: {n} presets", g.name);
}

/// Bench (`tools/bench.ps1`): the three heaviest MVFX on one 1920x1080 event at 1920x1080, composed on the CPU and on
/// the GPU (read back, as a render takes it), median of 9 after a warm-up: Datamosh twelve frames into its smear (twelve
/// motion searches and warps), Film Burn (six octaves of value noise per pixel), Radial Blur (24 bilinear taps).
#[test]
#[ignore]
fn bench_mvfx_1080p() {
    use std::time::Instant;
    let g = crate::engine::gpu::test_gpu();
    for (id, preset) in [("milan.fx.datamosh", "Datamosh 2009"), ("milan.fx.film_burn", "Burn Through"), ("milan.fx.radial_blur", "Strong Proportional")] {
        let info = find(id).unwrap();
        let mut p = project(vec![new_fx(info, info.presets.iter().find(|x| x.name == preset))]);
        (p.media_pool[0].width, p.media_pool[0].height) = (Some(1920), Some(1080));
        let vp = plan::compile(&p);
        let fp = plan::frame_plan(&vp, 60);
        let time = |c: &dyn Compositor| {
            let mut src = Moving::default();
            let _ = c.compose(&fp, &mut src, 1920, 1080, true, composite::stripes()).expect("composed").image();
            let mut v: Vec<f64> = (0..9)
                .map(|_| {
                    let t = Instant::now();
                    let _ = c.compose(&fp, &mut src, 1920, 1080, true, composite::stripes()).expect("composed").image();
                    t.elapsed().as_secs_f64() * 1000.0
                })
                .collect();
            v.sort_by(f64::total_cmp);
            v[4]
        };
        let cpu = time(&composite::CpuCompositor);
        let gpu = g.as_ref().map(|g| time(g.as_ref()));
        println!("BENCH mvfx fx={} cpu_ms={cpu:.1} gpu_ms={}", info.name.replace(' ', "_"), gpu.map_or("n/a".to_string(), |v| format!("{v:.1}")));
    }
}
