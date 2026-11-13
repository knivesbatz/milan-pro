use super::*;
use crate::engine::plugins::{self, preset_eval};
use crate::engine::vfx;
use crate::project::{Curve, Fx, Keyframe, Keyframes};
use proto::ParamDesc;
use std::sync::OnceLock;

fn param(name: &str, kind: &str, default: &[f64]) -> ParamDesc {
    ParamDesc { name: name.into(), label: name.to_uppercase(), kind: kind.into(), default: default.to_vec(), min: -f64::MAX, max: f64::MAX, ..Default::default() }
}

fn desc() -> PluginDesc {
    PluginDesc {
        id: "com.example.all".into(),
        major: 2,
        label: "All Kinds".into(),
        group: "Example".into(),
        contexts: vec!["OfxImageEffectContextFilter".into()],
        depths: vec!["OfxBitDepthFloat".into()],
        context: "OfxImageEffectContextFilter".into(),
        params: vec![
            param("page", "Page", &[]),
            param("grp", "Group", &[]),
            ParamDesc { parent: "grp".into(), min: 0.0, max: 4.0, ..param("amount", "Double", &[1.5]) },
            ParamDesc { parent: "grp".into(), ..param("count", "Integer", &[3.0]) },
            param("flag", "Boolean", &[1.0]),
            ParamDesc { options: vec!["A".into(), "B".into(), "C".into()], ..param("mode", "Choice", &[2.0]) },
            param("tint", "RGB", &[0.2, 0.4, 0.6]),
            param("pos", "Double2D", &[0.5, 0.25]),
            param("idx", "Integer3D", &[1.0, 2.0, 3.0]),
            ParamDesc { text: "hello".into(), ..param("note", "String", &[]) },
            ParamDesc { secret: true, ..param("hidden", "Double", &[9.0]) },
            param("go", "PushButton", &[]),
            param("blob", "Custom", &[]),
            param("empty", "Choice", &[0.0]),
        ],
        ..Default::default()
    }
}

/// R33a: OFX parameters map onto the FX window's controls (groups as headings; hidden, page, button, custom and empty
/// choices left out), and an evaluated instance maps back to the values the host sets, defaults included.
#[test]
fn parameters_map_both_ways() {
    let d = desc();
    let info = leak_info(&d);
    assert_eq!((info.id, info.name, info.group, info.kind), ("ofx:com.example.all@2", "All Kinds", "Example", PluginKind::VideoFx));
    let keys: Vec<&str> = info.params.iter().map(|p| p.key).collect();
    assert_eq!(keys, ["amount", "count", "flag", "mode", "tint", "pos.x", "pos.y", "idx.x", "idx.y", "idx.z", "note"]);
    assert_eq!((info.params[0].section, info.params[1].section, info.params[2].section), ("GRP", "", "Other"));
    assert!(matches!(info.params[0].kind, ParamKind::Float { min: 0.0, max: 4.0, default: 1.5 }));
    assert!(matches!(info.params[3].kind, ParamKind::Choice { default: 2, .. }));
    assert!(std::ptr::eq(info, leak_info(&d)), "an unchanged description is leaked once");
    let vals = values(&d, &preset_eval(info, None));
    let get = |k: &str| vals.iter().find(|v| v.0 == k).map(|v| v.1.clone());
    assert_eq!(get("amount"), Some(PVal::N(vec![1.5])));
    assert_eq!(get("count"), Some(PVal::N(vec![3.0])));
    assert_eq!(get("flag"), Some(PVal::N(vec![1.0])));
    assert_eq!(get("mode"), Some(PVal::N(vec![2.0])));
    assert_eq!(get("tint"), Some(PVal::N(vec![0.2f32 as f64, 0.4f32 as f64, 0.6f32 as f64])));
    assert_eq!(get("pos"), Some(PVal::N(vec![0.5, 0.25])));
    assert_eq!(get("idx"), Some(PVal::N(vec![1.0, 2.0, 3.0])));
    assert_eq!(get("note"), Some(PVal::S("hello".into())));
    assert!(get("hidden").is_none() && get("go").is_none() && get("blob").is_none());
    // A plug-in for Generators only, or 16-bit only, is listed and not run.
    assert!(unsupported(&PluginDesc { context: String::new(), contexts: vec!["OfxImageEffectContextGenerator".into()], ..d.clone() }).unwrap().contains("Generator"));
    assert!(unsupported(&PluginDesc { depths: vec!["OfxBitDepthHalf".into()], ..d.clone() }).unwrap().contains("OfxBitDepthHalf"));
    assert!(unsupported(&d).is_none());
}

/// Frames go to the host bottom row first and come back clamped to valid premultiplied colours, 8-bit exactly.
#[test]
fn frames_flip_and_convert() {
    let img = ColorImage { size: [3, 2], pixels: (0..6).map(|i| Color32::from_rgba_premultiplied(i * 10, i * 20, i * 30, 200)).collect() };
    for float in [false, true] {
        let mut buf = vec![0u8; 6 * bpp(float)];
        put(&img, float, &mut buf);
        let mut back = ColorImage::new([3, 2], Color32::TRANSPARENT);
        take(&buf, float, &mut back);
        assert_eq!(back.pixels, img.pixels, "float {float}");
        if float {
            let first = f32::from_le_bytes(buf[0..4].try_into().unwrap());
            assert_eq!(first, 30.0 / 255.0, "the bottom row (pixel 3) comes first");
        }
    }
    // Out of range floats are clamped; colour never exceeds alpha.
    let mut buf = Vec::new();
    for v in [2.0f32, -1.0, 0.9, 0.5] {
        buf.extend(v.to_le_bytes());
    }
    let mut one = ColorImage::new([1, 1], Color32::TRANSPARENT);
    take(&buf, true, &mut one);
    assert_eq!(one.pixels[0].to_array(), [128, 0, 128, 128]);
}

// ---- With the plug-in host and the test plug-in (tools/ofx-test-plugin). Run after `cargo build --release`:
// cargo test --release -- --ignored --test-threads=1 ofx::

/// The test bundle, built once, in a folder of its own under %TEMP%\milan-agent\ofx.
fn test_bundle() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        assert!(client::host_exe().is_some(), "build milan-plugin-host first: cargo build --release");
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let target = repo.join("target").join("ofx-test-plugin");
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let mut cmd = std::process::Command::new(cargo);
        cmd.args(["build", "--release", "--offline", "--quiet", "--manifest-path"]).arg(repo.join("tools").join("ofx-test-plugin").join("Cargo.toml")).arg("--target-dir").arg(&target);
        let st = crate::proc::output(&mut cmd).expect("cargo runs");
        assert!(st.status.success(), "{}", String::from_utf8_lossy(&st.stderr));
        let dir = std::env::temp_dir().join("milan-agent").join("ofx").join(format!("bundles-{}", std::process::id()));
        let win64 = dir.join("MilanTest.ofx.bundle").join("Contents").join("Win64");
        std::fs::create_dir_all(&win64).unwrap();
        std::fs::copy(target.join("release").join("milan_ofx_test.dll"), win64.join("MilanTest.ofx")).unwrap();
        dir
    })
    .clone()
}

fn registered() -> ScanCache {
    let cache = scan::scan(&[test_bundle()], &ScanCache::default(), false, &scan::describe_binary);
    set_registry(&cache, &Default::default());
    cache
}

const INVERT: &str = "ofx:com.meridiannle.test.invert@1";
const GAIN: &str = "ofx:com.meridiannle.test.gain@1";
const CRASH: &str = "ofx:com.meridiannle.test.crash@1";
const HANG: &str = "ofx:com.meridiannle.test.hang@1";

fn picture() -> ColorImage {
    let (w, h) = (64usize, 36usize);
    let pixels = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let a = if y < 4 { 64 + x as u8 * 2 } else { 255 };
            let c = |v: usize| ((v % 256) as u32 * a as u32 / 255) as u8;
            Color32::from_rgba_premultiplied(c(x * 4), c(y * 7), c(x + y * 3), a)
        })
        .collect();
    ColorImage { size: [w, h], pixels }
}

fn eval_at(id: &str, fx: Option<Fx>, t: f64, full_h: f64) -> FxEval {
    let fx = fx.unwrap_or_else(|| plugins::new_fx(plugins::find(id).expect("registered"), None));
    let mut e = plugins::eval(&fx, t).expect("evaluates");
    add_frame_vals(&mut e, &fx.id, t, full_h, 30.0, 1.0);
    e
}

fn max_diff(a: &ColorImage, b: &ColorImage) -> u8 {
    a.pixels.iter().zip(&b.pixels).flat_map(|(p, q)| (0..4).map(move |i| p.to_array()[i].abs_diff(q.to_array()[i]))).max().unwrap_or(0)
}

/// R33a acceptance: the test bundle is described through the plug-in host: four filters, their grouping, depths and
/// the gain parameter.
#[test]
#[ignore]
fn describe_through_the_host() {
    let cache = registered();
    let b = &cache.binaries[0];
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    let ids: Vec<String> = b.plugins.iter().map(id_of).collect();
    assert_eq!(ids, [INVERT, GAIN, CRASH, HANG]);
    let gain = &b.plugins[1];
    assert_eq!((gain.label.as_str(), gain.group.as_str(), gain.context.as_str()), ("Test Gain", "Milan Test", "OfxImageEffectContextFilter"));
    assert_eq!(gain.depths, ["OfxBitDepthByte", "OfxBitDepthFloat"]);
    assert_eq!(gain.params.len(), 1);
    assert_eq!((gain.params[0].name.as_str(), gain.params[0].kind.as_str(), gain.params[0].default.as_slice(), gain.params[0].min, gain.params[0].max), ("gain", "Double", &[1.0][..], 0.0, 4.0));
    let info = plugins::find(GAIN).unwrap();
    assert!(plugins::catalog(PluginKind::VideoFx).any(|i| i.id == GAIN), "offered in the Video FX chooser");
    assert_eq!((info.name, info.group, info.params[0].key), ("Test Gain", "Milan Test", "gain"));
    assert!(entries().iter().filter(|e| e.status == Status::Ok).count() >= 4);
    close_hosts();
}

/// R33b acceptance: the OFX invert matches the built-in invert within 1 LSB, in 32-bit float and 8-bit, through the
/// shared-memory render path; a preview-sized picture gets its render scale.
#[test]
#[ignore]
fn invert_matches_the_builtin_invert() {
    registered();
    let src = picture();
    let mut builtin = src.clone();
    vfx::apply(&mut builtin, &plugins::eval(&plugins::new_fx(plugins::find("milan.fx.invert").unwrap(), None), 0.0).unwrap(), 1);
    let mut float = src.clone();
    vfx::apply(&mut float, &eval_at(INVERT, None, 0.0, 72.0), 1);
    assert!(failure(INVERT).is_none(), "{:?}", failure(INVERT));
    assert!(max_diff(&float, &builtin) <= 1, "float: {}", max_diff(&float, &builtin));
    let p = find(INVERT).unwrap();
    assert!(p.float);
    let mut byte = src.clone();
    let r = Render { index: p.desc.index, instance: "byte".into(), w: 64, h: 36, float: false, scale: 1.0, par: 1.0, fps: 30.0, ..Default::default() };
    render(&p.binary, r, &mut byte).unwrap();
    assert!(max_diff(&byte, &builtin) <= 1, "8-bit: {}", max_diff(&byte, &builtin));
    close_hosts();
}

/// R33b acceptance: a keyframed OFX parameter is sent at the frame's time (gain 1 at 0 s to 3 at 2 s: 2 at 1 s).
#[test]
#[ignore]
fn gain_is_keyframed_at_t() {
    registered();
    let mut fx = plugins::new_fx(plugins::find(GAIN).unwrap(), None);
    fx.params.insert("gain".into(), Keyframes { keys: vec![Keyframe { t: 0.0, v: 1.0, curve: Curve::Linear }, Keyframe { t: 2.0, v: 3.0, curve: Curve::Linear }] });
    for (t, want) in [(0.0, 50), (1.0, 100), (2.0, 150)] {
        let mut img = ColorImage::new([8, 8], Color32::from_gray(50));
        vfx::apply(&mut img, &eval_at(GAIN, Some(fx.clone()), t, 8.0), 1);
        assert_eq!(img.pixels[0].to_array(), [want, want, want, 255], "t = {t}");
    }
    assert!(failure(GAIN).is_none());
    close_hosts();
}

/// R32/R33b acceptance: a plug-in that crashes or hangs is contained: the host is killed, the FX is bypassed with a
/// message, and the rest of the chain (another plug-in of the same binary in a new host, the built-ins) keeps
/// rendering.
#[test]
#[ignore]
fn crash_and_hang_are_contained() {
    registered();
    client::RENDER_TIMEOUT_MS.store(3000, Ordering::Relaxed);
    let _ = take_events();
    let src = picture();
    let mut expect = src.clone();
    vfx::apply(&mut expect, &plugins::eval(&plugins::new_fx(plugins::find("milan.fx.invert").unwrap(), None), 0.0).unwrap(), 1);
    for (bad, word) in [(CRASH, "crashed"), (HANG, "did not answer")] {
        let started = std::time::Instant::now();
        let mut img = src.clone();
        // The chain: the bad plug-in, then the OFX invert.
        for e in [eval_at(bad, None, 0.0, 36.0), eval_at(INVERT, None, 0.0, 36.0)] {
            vfx::apply(&mut img, &e, 1);
        }
        assert!(max_diff(&img, &expect) <= 1, "{bad} is bypassed, the invert still runs");
        let why = failure(bad).expect("recorded");
        assert!(why.contains(word), "{why}");
        assert!(take_events().iter().any(|m| m.contains("bypassed")));
        assert!(bypassed_because(bad).unwrap().contains("kept"));
        assert!(started.elapsed() < std::time::Duration::from_secs(20));
        // Bypassed from now on: no new host, no wait.
        let t = std::time::Instant::now();
        vfx::apply(&mut img, &eval_at(bad, None, 1.0, 36.0), 1);
        assert!(t.elapsed() < std::time::Duration::from_millis(200));
    }
    client::RENDER_TIMEOUT_MS.store(30_000, Ordering::Relaxed);
    clear_failures();
    close_hosts();
}

/// R32: a host that panics on request is reported as crashed; Milan carries on.
#[test]
#[ignore]
fn a_panicking_host_is_reported() {
    let bin = test_bundle().join("MilanTest.ofx.bundle").join("Contents").join("Win64").join("MilanTest.ofx");
    let (mut c, n) = Conn::start(&bin).unwrap();
    assert_eq!(n, 4);
    assert!(matches!(c.request(&Req::Panic, std::time::Duration::from_secs(5)), Err(client::Fail::Crashed(_))));
}

/// R33b acceptance, needs ffmpeg: a render runs OFX plug-ins as the preview does. A project with an OFX invert on its
/// event and an OFX gain on its track renders (lossless FFV1 RGB) to exactly the compositor's frames, and those show
/// both effects.
#[test]
#[ignore]
fn render_matches_preview_with_ofx_plugins() {
    use crate::engine::{plan, worker::Worker};
    use crate::project::{Clip, Project};
    registered();
    let dir = scan::tests::temp("render");
    let src = dir.join("color.mp4");
    let ok = crate::proc::ffmpeg().args(["-v", "error", "-y", "-f", "lavfi", "-i", "color=c=0x406080:s=320x240:r=30:d=1", "-c:v", "libx264", "-qp", "0", "-pix_fmt", "yuv444p"]).arg(&src).status().unwrap().success();
    assert!(ok);
    let m = crate::media::probe_media(&src).unwrap();
    let mut p = Project::new("ofx");
    (p.settings.width, p.settings.height, p.settings.frame_rate) = (320, 240, 30.0);
    let mut c = Clip::from_media(&m, 0.0);
    (c.duration, c.out_point) = (1.0, c.in_point + 1.0);
    c.fx.push(plugins::new_fx(plugins::find(INVERT).unwrap(), None));
    p.timeline.video_tracks[0].clips.push(c);
    let mut gain = plugins::new_fx(plugins::find(GAIN).unwrap(), None);
    gain.params.insert("gain".into(), Keyframes { keys: vec![Keyframe { t: 0.0, v: 1.5, curve: Curve::Linear }] });
    p.timeline.video_tracks[0].fx.push(gain);
    p.media_pool = vec![m];
    let t = crate::render::BUILTIN.iter().find(|t| t.name == "FFV1 MKV archive").unwrap();
    let out = dir.join("out.mkv");
    let job = crate::engine::render::start(&p, t, out.clone(), (0.0, 1.0), Default::default(), eframe::egui::Context::default());
    while job.status() == crate::render::Status::Running {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(job.status(), crate::render::Status::Done);
    let mut wk = Worker::for_render();
    wk.set_plan(std::sync::Arc::new(plan::compile(&p)));
    for k in [0i64, 15] {
        let (want, err) = wk.frame(k, 320, 240, p.settings.render_quality, None);
        assert!(err.is_none(), "{err:?}");
        let mut cmd = crate::proc::ffmpeg();
        cmd.args(["-v", "error", "-i"]).arg(&out).args(["-vf", &format!("select=eq(n\\,{k}),format=rgba"), "-frames:v", "1", "-f", "rawvideo", "-"]);
        let got = crate::proc::output(&mut cmd).unwrap().stdout;
        let want: &[u8] = bytemuck::cast_slice(&want.pixels);
        assert_eq!(got.len(), want.len());
        let diff = got.iter().zip(want).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
        assert!(diff <= 1, "frame {k}: the render differs from the preview by {diff}");
        // Inverted (0x40, 0x60, 0x80 -> 0xBF, 0x9F, 0x7F), then 1.5x, clamped.
        let mid = &want[(120 * 320 + 160) * 4..][..4];
        let expect = [255u8, (0x9F as f32 * 1.5).round() as u8, (0x7F as f32 * 1.5).round() as u8];
        assert!(mid[..3].iter().zip(expect).all(|(a, b)| a.abs_diff(b) <= 4), "{mid:?} vs {expect:?}");
    }
    assert!(failure(INVERT).is_none() && failure(GAIN).is_none());
    drop(wk);
    close_hosts();
    std::fs::remove_dir_all(&dir).unwrap();
}
