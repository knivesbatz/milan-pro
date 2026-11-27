use super::*;
use crate::engine::composite::{self, Compositor};
use crate::engine::gpu::tests::{Noise, Rng, gpu_or_skip, plan_n};
use crate::engine::plugins::{find, preset_eval};
use crate::project::pixel::{AcesSpace, PixelFormat, PixelSettings, ViewTransform};
use crate::project::ProjectSettings;
use std::sync::Arc;

fn working(format: PixelFormat, gamma: Gamma, view: ViewTransform, aces_space: AcesSpace) -> Working {
    let s = ProjectSettings { pixel: PixelSettings { format, gamma, view, aces_space, ..Default::default() }, ..Default::default() };
    Working::of(&s).expect("a float project")
}

/// The working spaces the tests walk: video-coded, linear, and the ACES spaces under each kind of view.
fn spaces() -> Vec<(&'static str, Working)> {
    use AcesSpace::*;
    use PixelFormat::*;
    use ViewTransform::*;
    vec![
        ("video levels, 2.222", working(FloatVideo, Gamma::Video, Off, Aces2065)),
        ("full range, linear", working(FloatFull, Gamma::Linear, Off, Aces2065)),
        ("ACES2065-1, sRGB view", working(FloatFull, Gamma::Linear, Srgb, Aces2065)),
        ("ACEScg, Rec.709 view", working(FloatFull, Gamma::Linear, Rec709, AcesCg)),
        ("ACEScct, Rec.709 view", working(FloatFull, Gamma::Linear, Rec709, AcesCct)),
        ("ACEScc, sRGB view", working(FloatFull, Gamma::Linear, Srgb, AcesCc)),
        ("ACEScg, HDR10", working(FloatFull, Gamma::Linear, Pq1000, AcesCg)),
        ("ACES2065-1, HLG", working(FloatFull, Gamma::Linear, Hlg1000, Aces2065)),
    ]
}

fn close(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol * a.abs().max(b.abs()).max(1.0)
}

#[test]
fn transfers_and_primaries_round_trip() {
    // PQ: BT.2408's reference white (203 cd/m²) is 58 % PQ; 100, 1000 and 10000 cd/m² as ST 2084 tabulates them.
    for (nits, code) in [(100.0, 0.508), (203.0, 0.581), (1000.0, 0.752), (10_000.0, 1.0)] {
        assert!((pq_oetf(nits) - code).abs() < 1.5e-3, "PQ {nits}: {}", pq_oetf(nits));
    }
    // HLG: 75 % is 203 cd/m² on a 1000 cd/m² display (BT.2408).
    let d = hlg_to_display([0.75; 3])[1] * HDR_PEAK;
    assert!((d - 203.0).abs() < 203.0 * 0.03, "HLG 75 %: {d} cd/m²");
    for i in 0..=50 {
        let v = i as f32 / 50.0;
        assert!((pq_oetf(pq_eotf(v)) - v).abs() < 2e-4, "PQ {v}");
        let h = hlg_from_display(hlg_to_display([v, v * 0.5, v * 0.25]));
        assert!(close(h[0], v, 2e-3) && close(h[2], v * 0.25, 2e-3), "HLG {v}: {h:?}");
        assert!(close(lin_to_video(video_to_lin(v)), v, 1e-5));
    }
    for l in [0.0001f32, 0.0078, 0.18, 1.0, 16.0] {
        assert!(close(cc_to_lin(lin_to_cc(l)), l, 1e-4) && close(cct_to_lin(lin_to_cct(l)), l, 1e-4), "ACEScc/cct {l}");
        assert!(close(rrt_odt_inv(rrt_odt(l)), l, 1e-3) || !(0.01..=10.0).contains(&l), "RRT {l}");
    }
    assert!((lin_to_cct(0.18) - 0.4135).abs() < 1e-3, "ACEScct mid grey {}", lin_to_cct(0.18));
    // Primaries: there and back; the white point stays white (Bradford D60 <-> D65).
    for (a, b) in [(Prim::Rec709, Prim::Rec2020), (Prim::Rec709, Prim::Ap0), (Prim::Ap1, Prim::Rec2020), (Prim::Ap0, Prim::Ap1)] {
        let c = [0.3, 0.6, 0.1];
        let back = mul(conv_m(b, a), mul(conv_m(a, b), c));
        assert!((0..3).all(|i| (back[i] - c[i]).abs() < 1e-5), "{a:?} -> {b:?}: {back:?}");
        let white = mul(conv_m(a, b), [1.0; 3]);
        assert!(white.iter().all(|v| (v - 1.0).abs() < 2e-3), "{a:?} -> {b:?} white {white:?}");
    }
    assert_eq!(mobius(0.3, 0.5, 4.9), 0.3);
    assert!((mobius(4.9, 0.5, 4.9) - 1.0).abs() < 1e-4 && mobius(2.0, 0.5, 4.9) < 1.0);
}

/// An SDR source shows as it is coded in every float working space (the ACES SDR views undo their own input
/// transform; the HDR views tone-map it back for the preview), and its white lands at 203 cd/m² in an HDR10 render.
#[test]
fn sdr_media_round_trips_through_every_working_space() {
    let colours = [[0.5, 0.5, 0.5], [0.9, 0.2, 0.1], [0.05, 0.3, 0.8], [0.18, 0.18, 0.18], [0.75, 0.75, 0.6]];
    for (name, wk) in spaces() {
        let input = wk.sdr_input(Depth::W16);
        for c in colours {
            let back = wk.preview_px(input.pixel(c));
            // An HDR view's preview rolls off above half of reference white: compare the darker colours there.
            let tol = if wk.out.hdr() && c.iter().any(|v| *v > 0.7) { 0.06 } else { 2.0 / 1023.0 };
            assert!((0..3).all(|i| (back[i] - c[i]).abs() <= tol), "{name}: {c:?} came back as {back:?}");
        }
    }
    let hdr10 = working(PixelFormat::FloatFull, Gamma::Linear, ViewTransform::Pq1000, AcesSpace::AcesCg);
    let white = hdr10.view_px(hdr10.sdr_input(Depth::B8).pixel([1.0; 3]), Out::Pq);
    assert!(white.iter().all(|v| (v - pq_oetf(REF_WHITE)).abs() < 2e-3), "SDR white in HDR10: {white:?}");
    // 8-bit: no working space, nothing changes.
    assert!(Working::of(&ProjectSettings::default()).is_none());
}

/// The float compositor follows the 8-bit one: 150 random plans (every blend mode, opacity, Track Motion, Pan/Crop,
/// dissolves, Fade to Color, groups) composed in a video-coded float space and shown in 8 bits are the 8-bit
/// compositor's frames but for its rounding.
#[test]
fn float_compose_follows_the_8bit_compose() {
    let wk = working(PixelFormat::FloatVideo, Gamma::Video, ViewTransform::Off, AcesSpace::Aces2065);
    let mut r = Rng(11);
    let (mut sum, mut n, mut far) = (0u64, 0u64, 0u64);
    for _ in 0..150 {
        let (fp, [w, h], bilinear) = plan_n(&mut r);
        let eight = composite::compose(&fp, &mut Noise::default(), w, h, bilinear, 2);
        let float = wk.to_preview(&compose::compose(&fp, &mut Noise::default(), w, h, bilinear, 2, &wk), 2);
        for (a, b) in eight.pixels.iter().zip(&float.pixels) {
            for (x, y) in a.to_array().into_iter().zip(b.to_array()) {
                let d = x.abs_diff(y) as u64;
                (sum, n, far) = (sum + d, n + 1, far + (d > 2) as u64);
            }
        }
    }
    let mean = sum as f64 / n as f64;
    println!("float_compose_follows_the_8bit_compose: mean difference {mean:.3} in 255, {far} of {n} channels over 2");
    assert!(mean < 0.5 && (far as f64) < n as f64 * 0.002, "mean {mean:.3}, {far} of {n} channels over 2");
}

/// R40b acceptance: float compositing on the GPU (`Rgba16Float`) agrees with the CPU's: 120 random plans in each of a
/// video-coded, a linear and an ACEScg working space. Video-coded and linear: within 2e-3 (relative above 1: half-float
/// steps) on all but 0.1 % of the channels (Hard Mix, Burn and Dodge flip where a value sits on their threshold) and
/// the previews within 2 in 255 on 99 % of the pixels. ACEScg, whose SDR media come in as scene light up to about 60
/// times reference white, puts the separable modes' cancelling formulas (Screen, Overlay...) far outside 0..1, where
/// half floats' steps grow: within 2e-3 on 97 % of the channels there.
#[test]
fn gpu_float_matches_cpu_float() {
    let Some(g) = gpu_or_skip("gpu_float_matches_cpu_float") else { return };
    for (name, wk, tol) in spaces().into_iter().filter_map(|(n, wk)| [("video levels, 2.222", 0.001), ("full range, linear", 0.001), ("ACEScg, Rec.709 view", 0.03)].into_iter().find(|t| t.0 == n).map(|t| (n, wk, t.1))) {
        let wk = Arc::new(wk);
        let mut r = Rng(23);
        let (mut worst, mut far, mut n, mut bad, mut pixels) = (0.0f32, 0usize, 0usize, 0usize, 0usize);
        for i in 0..120 {
            let (fp, [w, h], bilinear) = plan_n(&mut r);
            let cpu = compose::compose(&fp, &mut Noise::default(), w, h, bilinear, 2, &wk);
            let pic = g.compose_wide(&fp, &mut Noise::default(), w, h, bilinear, 2, &wk).unwrap_or_else(|| panic!("{name} plan {i}: the GPU did not compose it"));
            let gpu = pic.wide_image().expect("a float frame").0;
            for (a, b) in cpu.px.iter().zip(&gpu.px) {
                for (x, y) in a.iter().zip(b) {
                    let d = (x - y).abs() / x.abs().max(1.0);
                    worst = worst.max(d);
                    far += (d > 2e-3) as usize;
                    n += 1;
                }
            }
            // The preview the GPU made through the view against the CPU's (in a linear space the view's encoding is
            // steep near black, where half floats' steps show).
            let (pv, cv) = (pic.image(), wk.to_preview(&cpu, 2));
            bad += pv.pixels.iter().zip(&cv.pixels).filter(|(a, b)| a.to_array().iter().zip(b.to_array()).any(|(x, y)| x.abs_diff(y) > 2)).count();
            pixels += pv.pixels.len();
        }
        println!("gpu_float_matches_cpu_float on {} ({name}): largest difference {worst:.2e}, {far} of {n} channels over 2e-3, {bad} of {pixels} preview pixels over 2 in 255", g.name);
        assert!((far as f64) <= n as f64 * tol, "{name}: {far} of {n} channels over 2e-3 (largest {worst})");
        assert!(bad * 100 <= pixels || tol > 0.01, "{name}: {bad} of {pixels} preview pixels over 2 in 255");
    }
}
/// FX keep float precision: a colour FX reads its LUT between entries (a 10-bit gradient stays 10-bit), Gaussian
/// Blur and masks run in float, and an FX without a float kernel (`via8`) leaves the pixels it does not touch exactly
/// as they were and keeps the rest within half an 8-bit step.
#[test]
fn fx_keep_float_precision() {
    let ramp = |w: usize| WImage { size: [w, 2], px: (0..2 * w).map(|i| { let v = (i % w) as f32 / (w - 1) as f32; [v, v, v, 1.0] }).collect() };
    let unique = |img: &WImage| {
        let mut v: Vec<u32> = img.px[..img.size[0]].iter().map(|p| (p[0] * 1023.0).round() as u32).collect();
        v.dedup();
        v.len()
    };
    let fx = |id: &str, vals: &[(&'static str, f32)]| {
        let mut e = preset_eval(find(id).unwrap(), None);
        for (k, v) in vals {
            e.vals.retain(|(key, _)| key != k);
            e.vals.push((k, crate::engine::plugins::Val::F(*v)));
        }
        e
    };
    let mut img = ramp(1024);
    compose::apply_fx(&mut img, &fx("milan.fx.invert", &[]), 2);
    assert!(img.px.iter().enumerate().all(|(i, p)| (p[0] - (1.0 - (i % 1024) as f32 / 1023.0)).abs() < 1e-5), "invert is exact");
    let mut img = ramp(1024);
    compose::apply_fx(&mut img, &fx("milan.fx.brightness_contrast", &[("brightness", 0.1)]), 2);
    assert!(unique(&img) > 900, "brightness keeps a 10-bit ramp: {} levels", unique(&img));
    // Mirror has no float kernel: the half it keeps is exact, the mirrored half within half a step.
    let src = ramp(1024);
    let mut img = src.clone();
    compose::apply_fx(&mut img, &fx("milan.fx.mirror", &[]), 2);
    let kept = img.px.iter().zip(&src.px).filter(|(a, b)| a == b).count();
    assert!(kept >= src.px.len() / 2 - 4, "{kept} pixels kept exactly");
    assert!(img.px.iter().zip(&to8(&img).pixels).all(|(p, q)| (p[0] - q.r() as f32 / 255.0).abs() <= 0.5 / 255.0 + 1e-6));
}

/// A project file without the block is 8-bit; a float one saves and reads its settings.
#[test]
fn pixel_settings_save_and_load() {
    let mut p = crate::project::Project::new("f");
    assert!(!serde_json::to_string(&p.settings).unwrap().contains("pixel"));
    p.settings.pixel.set_hdr(crate::project::pixel::HdrMode::Hdr10);
    p.settings.pixel.aces_space = AcesSpace::AcesCct;
    let back: ProjectSettings = serde_json::from_str(&serde_json::to_string(&p.settings).unwrap()).unwrap();
    assert_eq!(back.pixel, p.settings.pixel);
    let wk = Working::of(&back).unwrap();
    assert_eq!((wk.enc, wk.prim, wk.out, wk.hdr_in), (Enc::Cct, Prim::Ap1, Out::Pq, true));
}

// ---- end to end, with FFmpeg ----

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join("milan-agent").join("r40b").join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn ff(args: &[&str]) {
    let mut cmd = crate::proc::ffmpeg();
    cmd.args(args);
    let out = crate::proc::output(&mut cmd).unwrap();
    assert!(out.status.success(), "ffmpeg {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr));
}

/// The raw frames of `path` as `pix_fmt` (16-bit words for 10-bit formats).
fn raw(path: &std::path::Path, pix_fmt: &str) -> Vec<u8> {
    let mut cmd = crate::proc::ffmpeg();
    cmd.args(["-v", "error", "-i"]).arg(path).args(["-f", "rawvideo", "-pix_fmt", pix_fmt, "-"]);
    let out = crate::proc::output(&mut cmd).unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    out.stdout
}

fn words(b: &[u8]) -> Vec<u16> {
    b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
}

/// A project `w` x `h` at 25 fps showing `m` for `secs`, with pixel settings `px`.
fn project(m: &crate::project::MediaItem, (w, h): (u32, u32), secs: f64, px: PixelSettings) -> crate::project::Project {
    let mut p = crate::project::Project::new("r40b");
    (p.settings.width, p.settings.height, p.settings.frame_rate, p.settings.pixel) = (w, h, 25.0, px);
    let mut c = crate::project::Clip::from_media(m, 0.0);
    (c.duration, c.out_point) = (secs, c.in_point + secs);
    p.timeline.video_tracks[0].clips.push(c);
    p.media_pool.push(m.clone());
    p
}

fn render(p: &crate::project::Project, t: &crate::render::Template, out: &std::path::Path, secs: f64) {
    use crate::render::{Opts, Status};
    let job = crate::engine::render::start(p, t, out.to_path_buf(), (0.0, secs), Opts::default(), eframe::egui::Context::default());
    while job.status() == Status::Running {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let log = job.log().and_then(|l| std::fs::read_to_string(l).ok()).unwrap_or_default();
    assert_eq!(job.status(), Status::Done, "{}: {log}", t.name);
}

fn template(name: &str) -> crate::render::Template {
    crate::render::BUILTIN.iter().find(|t| t.name == name).unwrap_or_else(|| panic!("no template {name}")).clone()
}

fn float(format: PixelFormat) -> PixelSettings {
    let mut px = PixelSettings::default();
    px.set_format(format);
    px
}

/// Needs ffmpeg. `cargo test --release -- --ignored ten_bit`
/// R40b acceptance: a 10-bit 4:4:4 source (lossless FFV1, a smooth gradient in all three channels) renders through
/// a 32-bit floating point project to a 10-bit 4:4:4 file within 1 code value of the source in every sample; the same
/// render from an 8-bit project is off by more (the check would see 8 bits).
#[test]
#[ignore]
fn ten_bit_source_round_trips_through_a_ten_bit_render() {
    let d = dir("roundtrip");
    let src = d.join("src.mkv");
    // A 16-bit RGB gradient (in gamut by construction), coded 10-bit 4:4:4 Rec.709.
    ff(&["-v", "error", "-y", "-f", "lavfi", "-i", "nullsrc=s=512x64:r=25:d=0.4,format=gbrp16le,geq=r='X*128':g='Y*1000+X*8':b='65535-X*120',scale=out_color_matrix=bt709:out_range=tv,format=yuv444p10le", "-c:v", "ffv1", "-color_primaries", "bt709", "-color_trc", "bt709", "-colorspace", "bt709", "-color_range", "tv", &src.to_string_lossy()]);
    let m = crate::media::probe_media(&src).unwrap();
    assert_eq!(m.bit_depth, Some(10));
    let mut t = template("FFV1 MKV archive");
    (t.video.as_mut().unwrap().pix_fmt, t.audio) = ("yuv444p10le".into(), None);
    let want = words(&raw(&src, "yuv444p10le"));
    let mut worst = Vec::new();
    for (name, px) in [("float", float(PixelFormat::FloatVideo)), ("8-bit", PixelSettings::default())] {
        let out = d.join(format!("{name}.mkv"));
        render(&project(&m, (512, 64), 0.4, px), &t, &out, 0.4);
        let got = words(&raw(&out, "yuv444p10le"));
        assert_eq!(got.len(), want.len(), "{name}: frames");
        worst.push(got.iter().zip(&want).map(|(a, b)| a.abs_diff(*b)).max().unwrap());
    }
    println!("ten_bit_source_round_trips_through_a_ten_bit_render: largest difference {} (float), {} (8-bit)", worst[0], worst[1]);
    assert!(worst[0] <= 1, "float: off by {}", worst[0]);
    assert!(worst[1] >= 2, "8-bit: off by only {}", worst[1]);
    let _ = std::fs::remove_dir_all(&d);
}

/// Needs ffmpeg. R40b acceptance (spec: "unique values ≥ 900 of 1024"): a 10-bit grey ramp over the 1920 columns of
/// a ProRes 422 HQ render has 850 or more of the 877 luma codes a 10-bit video-range file can hold (900 of 1024 is
/// beyond video range's 877); the 8-bit project's render has at most 256.
#[test]
#[ignore]
fn ten_bit_gradient_renders_without_banding() {
    let d = dir("banding");
    let src = d.join("ramp.mkv");
    ff(&["-v", "error", "-y", "-f", "lavfi", "-i", "nullsrc=s=1920x16:r=25:d=0.2,format=gbrp16le,geq=r='X*34.15':g='X*34.15':b='X*34.15',scale=out_color_matrix=bt709:out_range=tv,format=yuv444p10le", "-c:v", "ffv1", "-colorspace", "bt709", "-color_range", "tv", &src.to_string_lossy()]);
    let m = crate::media::probe_media(&src).unwrap();
    let t = crate::render::Template { audio: None, ..template("ProRes 422 HQ") };
    let mut levels = Vec::new();
    for (name, px) in [("float", float(PixelFormat::FloatVideo)), ("8-bit", PixelSettings::default())] {
        let out = d.join(format!("{name}.mov"));
        render(&project(&m, (1920, 16), 0.2, px), &t, &out, 0.2);
        let y = words(&raw(&out, "yuv422p10le"));
        let mut row: Vec<u16> = y[8 * 1920..9 * 1920].to_vec();
        row.sort_unstable();
        row.dedup();
        levels.push(row.len());
    }
    println!("ten_bit_gradient_renders_without_banding: {} luma codes (float), {} (8-bit)", levels[0], levels[1]);
    assert!(levels[0] >= 850, "float: {} levels", levels[0]);
    assert!(levels[1] <= 256, "8-bit: {} levels", levels[1]);
    let _ = std::fs::remove_dir_all(&d);
}

fn probe(path: &std::path::Path, args: &[&str]) -> String {
    let mut cmd = crate::proc::ffprobe();
    cmd.args(["-v", "error"]).args(args).arg(path);
    let out = crate::proc::output(&mut cmd).unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Needs ffmpeg. R40b acceptance: an HDR10 project (PQ source, Rec.2020) renders the HEVC HDR10 template to a file
/// ffprobe reads as Rec.2020 PQ 10-bit with the mastering display and content light level side data, its picture
/// within a few codes of the source's (below the roll-off); an HLG project renders HLG; an 8-bit SDR project into the
/// HDR10 template puts SDR white at 203 cd/m² (58 % PQ).
#[test]
#[ignore]
fn hdr_projects_render_hdr10_and_hlg() {
    use crate::project::pixel::HdrMode;
    let d = dir("hdr");
    let bars = "smptehdbars=s=640x360:r=25:d=0.4,setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv";
    let make = |name: &str, vf: &str, trc: &str| {
        let p = d.join(name);
        ff(&["-v", "error", "-y", "-f", "lavfi", "-i", bars, "-vf", vf, "-c:v", "libx265", "-preset", "ultrafast", "-crf", "4", "-x265-params", "log-level=error", "-color_primaries", "bt2020", "-color_trc", trc, "-colorspace", "bt2020nc", &p.to_string_lossy()]);
        crate::media::probe_media(&p).unwrap()
    };
    let pq = make("pq.mp4", "zscale=t=linear:npl=203,format=gbrpf32le,zscale=p=bt2020,zscale=t=smpte2084:m=bt2020nc:r=tv:npl=203,format=yuv420p10le", "smpte2084");
    let hlg = make("hlg.mp4", "zscale=t=linear,format=gbrpf32le,zscale=p=bt2020,exposure=exposure=-2.300,zscale=t=arib-std-b67:m=bt2020nc:r=tv:npl=1000,format=yuv420p10le", "arib-std-b67");
    let sdr = {
        let p = d.join("sdr.mp4");
        ff(&["-v", "error", "-y", "-f", "lavfi", "-i", "color=c=white:s=640x360:r=25:d=0.4", "-vf", "format=yuv420p", "-c:v", "libx264", "-qp", "0", "-colorspace", "bt709", &p.to_string_lossy()]);
        crate::media::probe_media(&p).unwrap()
    };
    let hdr = |m| {
        let mut px = PixelSettings::default();
        px.set_hdr(m);
        px
    };
    let side = |p: &std::path::Path| probe(p, &["-select_streams", "v:0", "-read_intervals", "%+#1", "-show_frames"]);
    let stream = |p: &std::path::Path| probe(p, &["-select_streams", "v:0", "-show_entries", "stream=pix_fmt,color_transfer,color_primaries,color_space", "-of", "default=nw=1"]);
    // HDR10.
    let out = d.join("hdr10.mp4");
    render(&project(&pq, (640, 360), 0.4, hdr(HdrMode::Hdr10)), &template("MP4 HEVC HDR10 (PQ)"), &out, 0.4);
    let s = stream(&out);
    assert!(s.contains("pix_fmt=yuv420p10le") && s.contains("color_transfer=smpte2084") && s.contains("color_primaries=bt2020") && s.contains("color_space=bt2020nc"), "{s}");
    let sd = side(&out);
    assert!(sd.contains("side_data_type=Mastering display metadata") && sd.contains("max_luminance=10000000/10000") && sd.contains("side_data_type=Content light level metadata") && sd.contains("max_content=1000"), "{sd}");
    // The picture: the 40 % grey bar's luma, source against render (PQ in, PQ out, under the roll-off).
    let luma = |p: &std::path::Path| {
        let y = words(&raw(p, "yuv420p10le"));
        y[100 * 640 + 640 / 16]
    };
    let (a, b) = (luma(&d.join("pq.mp4")), luma(&out));
    assert!(a.abs_diff(b) <= 4, "PQ 40 % grey: source {a}, render {b}");
    // HLG.
    let out = d.join("hlg.mp4");
    render(&project(&hlg, (640, 360), 0.4, hdr(HdrMode::Hlg)), &template("MP4 HEVC HLG"), &out, 0.4);
    let s = stream(&out);
    assert!(s.contains("color_transfer=arib-std-b67") && s.contains("color_primaries=bt2020"), "{s}");
    let (a, b) = (luma(&d.join("hlg.mp4")), luma(&out));
    assert!(a.abs_diff(b) <= 6, "HLG 40 % grey: source {a}, render {b}");
    // SDR white into HDR10 from an 8-bit project: 58 % PQ (video range: 64 + 0.581 x 876).
    let out = d.join("sdr-in-hdr10.mp4");
    render(&project(&sdr, (640, 360), 0.4, PixelSettings::default()), &template("MP4 HEVC HDR10 (PQ)"), &out, 0.4);
    let w = luma(&out) as f32;
    assert!((w - (64.0 + pq_oetf(REF_WHITE) * 876.0)).abs() < 4.0, "SDR white at {w}");
    let _ = std::fs::remove_dir_all(&d);
}

/// Needs ffmpeg. R40b acceptance, linear EXR: an OpenEXR still with values up to 4.0 (scene light over reference
/// white) in a full-range linear project renders to OpenEXR with its highlights and its ramp (within half-float
/// precision of the encoder); an 8-bit project clips them at 1.
#[test]
#[ignore]
fn linear_exr_keeps_its_highlights() {
    let d = dir("exr");
    let src = d.join("hot.exr");
    let (w, h) = (64usize, 8usize);
    // Planar G, B, R float: a ramp 0..4 across in every channel.
    let mut planes = Vec::with_capacity(w * h * 3 * 4);
    for _ in 0..3 {
        for _y in 0..h {
            for x in 0..w {
                planes.extend((x as f32 / (w - 1) as f32 * 4.0).to_le_bytes());
            }
        }
    }
    let raw_in = d.join("ramp.gbrpf32");
    std::fs::write(&raw_in, &planes).unwrap();
    ff(&["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "gbrpf32le", "-s", &format!("{w}x{h}"), "-i", &raw_in.to_string_lossy(), "-frames:v", "1", "-c:v", "exr", "-compression", "0", &src.to_string_lossy()]);
    let m = crate::media::probe_media(&src).unwrap();
    let t = template("OpenEXR sequence");
    let mut maxima = Vec::new();
    for (name, px) in [("float", float(PixelFormat::FloatFull)), ("8-bit", PixelSettings::default())] {
        let first = d.join(name).join("out_000000.exr");
        std::fs::create_dir_all(first.parent().unwrap()).unwrap();
        render(&project(&m, (w as u32, h as u32), 0.04, px), &t, &first, 0.04);
        let b = raw(&first, "gbrpf32le");
        let f: Vec<f32> = b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        // The green plane's middle row.
        let row = &f[(h / 2) * w..(h / 2 + 1) * w];
        if name == "float" {
            for (x, v) in row.iter().enumerate() {
                let want = x as f32 / (w - 1) as f32 * 4.0;
                assert!((v - want).abs() <= want * 2e-3 + 1e-3, "x {x}: {v}, wanted {want}");
            }
        }
        maxima.push(row.iter().cloned().fold(0.0f32, f32::max));
    }
    println!("linear_exr_keeps_its_highlights: brightest {} (float), {} (8-bit)", maxima[0], maxima[1]);
    assert!((maxima[0] - 4.0).abs() < 0.01 && maxima[1] <= 1.0 + 1e-3, "{maxima:?}");
    let _ = std::fs::remove_dir_all(&d);
}
