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
            ParamDesc { parent: "grp".into(), min: 0.0, max: 4.0, vmin: 0.0, vmax: 4.0, ..param("amount", "Double", &[1.5]) },
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

/// R33a: every OFX parameter with a value maps onto a control (hidden and custom ones too: `layout` decides what shows;
/// pages, groups and buttons have no value), and an evaluated instance maps back to the values the host sets, defaults
/// included.
#[test]
fn parameters_map_both_ways() {
    let d = desc();
    let info = leak_info(&d);
    assert_eq!((info.id, info.name, info.group, info.kind), ("ofx:com.example.all@2", "All Kinds", "Example", PluginKind::VideoFx));
    let keys: Vec<&str> = info.params.iter().map(|p| p.key).collect();
    assert_eq!(keys, ["amount", "count", "flag", "mode", "tint", "pos.x", "pos.y", "idx.x", "idx.y", "idx.z", "note", "hidden", "blob", "empty"]);
    assert!(info.params.iter().all(|p| p.section.is_empty()), "the OFX layout places them, not sections");
    assert!(matches!(info.params[0].kind, ParamKind::Float { min: 0.0, max: 4.0, default: 1.5 }));
    // A parameter with no hard range keeps any value (the slider shows a range around the default: `Row::Param::slider`).
    assert!(matches!(info.params[1].kind, ParamKind::Float { min, max, default: 3.0 } if min == -f32::MAX && max == f32::MAX));

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
    assert_eq!((get("hidden"), get("blob"), get("empty")), (Some(PVal::N(vec![9.0])), Some(PVal::S(String::new())), Some(PVal::N(vec![0.0]))));
    assert!(get("go").is_none() && get("page").is_none() && get("grp").is_none());
    // A plug-in for Generators only, or 16-bit only, is listed and not run.
    assert!(unsupported(&PluginDesc { context: String::new(), contexts: vec!["OfxImageEffectContextGenerator".into()], ..d.clone() }).unwrap().contains("Generator"));
    assert!(unsupported(&PluginDesc { depths: vec!["OfxBitDepthWeird".into()], ..d.clone() }).unwrap().contains("OfxBitDepthWeird"));
    assert!(unsupported(&PluginDesc { depths: vec!["OfxBitDepthHalf".into()], ..d.clone() }).is_none(), "half is given");
    assert!(unsupported(&d).is_none());
}

/// Frames go to the host bottom row first and come back clamped to valid premultiplied colours, 8-bit exactly.
#[test]
fn frames_flip_and_convert() {
    let img = ColorImage { size: [3, 2], pixels: (0..6).map(|i| Color32::from_rgba_premultiplied(i * 10, i * 20, i * 30, 200)).collect() };
    for depth in [Depth::Byte, Depth::Short, Depth::Half, Depth::Float] {
        let mut buf = vec![0u8; 6 * bpp(depth)];
        put(&img, depth, &mut buf);
        let mut back = ColorImage::new([3, 2], Color32::TRANSPARENT);
        take(&buf, depth, &mut back);
        assert_eq!(back.pixels, img.pixels, "{depth:?}");
        if depth == Depth::Float {
            let first = f32::from_le_bytes(buf[0..4].try_into().unwrap());
            assert_eq!(first, 30.0 / 255.0, "the bottom row (pixel 3) comes first");
        }
        if depth == Depth::Short {
            assert_eq!(u16::from_le_bytes([buf[0], buf[1]]), 30 * 257);
        }
    }
    // Half floats round-trip 8-bit values exactly and keep sign, infinity and small numbers.
    for v in [0.0f32, 1.0, 0.5, 30.0 / 255.0, -2.0, 65504.0, 1e-5] {
        assert!((half_to_f32(f32_to_half(v)) - v).abs() <= v.abs() / 1024.0 + 1e-7, "{v}");
    }
    assert!(half_to_f32(f32_to_half(1e6)).is_infinite());
    // Out of range floats are clamped; colour never exceeds alpha.
    let mut buf = Vec::new();
    for v in [2.0f32, -1.0, 0.9, 0.5] {
        buf.extend(v.to_le_bytes());
    }
    let mut one = ColorImage::new([1, 1], Color32::TRANSPARENT);
    take(&buf, Depth::Float, &mut one);
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
        // One folder for every test run (a host from an earlier run may still hold the file: then a folder of this run's own).
        let built = target.join("release").join("milan_ofx_test.dll");
        let place = |dir: PathBuf| {
            let win64 = dir.join("MilanTest.ofx.bundle").join("Contents").join("Win64");
            std::fs::create_dir_all(&win64)?;
            std::fs::copy(&built, win64.join("MilanTest.ofx")).map(|_| dir)
        };
        let base = std::env::temp_dir().join("milan-agent").join("ofx");
        place(base.join("bundles")).or_else(|_| place(base.join(format!("bundles-{}", std::process::id())))).unwrap()
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
const DISSOLVE: &str = "ofx:com.meridiannle.test.dissolve@1";
const SOLID: &str = "ofx:com.meridiannle.test.solid@1";
const PARAMS: &str = "ofx:com.meridiannle.test.params@1";

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
    add_frame_vals(&mut e, &fx.id, restored(&fx), t, full_h, 30.0, 1.0);
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
    assert_eq!(ids, [INVERT, GAIN, CRASH, HANG, DISSOLVE, SOLID, PARAMS]);
    assert_eq!(b.plugins[4].context, "OfxImageEffectContextTransition");
    assert_eq!((b.plugins[5].context.as_str(), b.plugins[5].depths.as_slice()), ("OfxImageEffectContextGenerator", &["OfxBitDepthShort".to_string()][..]));
    assert!(plugins::catalog(PluginKind::Transition).any(|i| i.id == DISSOLVE) && plugins::catalog(PluginKind::Generator).any(|i| i.id == SOLID));
    let params = &b.plugins[6].params;
    assert_eq!(params.iter().map(|p| p.kind.as_str()).collect::<Vec<_>>(), ["Group", "Choice", "Double", "Double", "Double", "Integer", "Choice", "String", "Double", "PushButton", "PushButton", "PushButton"]);
    assert!(params[3].secret && params[1].parent == "grp" && params[0].open);
    let gain = &b.plugins[1];
    assert_eq!((gain.label.as_str(), gain.group.as_str(), gain.context.as_str()), ("Test Gain", "Milan Test", "OfxImageEffectContextFilter"));
    assert_eq!(gain.depths, ["OfxBitDepthByte", "OfxBitDepthFloat"]);
    assert_eq!(gain.params.len(), 1);
    assert_eq!((gain.params[0].name.as_str(), gain.params[0].kind.as_str(), gain.params[0].default.as_slice(), gain.params[0].min, gain.params[0].max), ("gain", "Double", &[1.0][..], 0.0, 4.0));
    let info = plugins::find(GAIN).unwrap();
    assert!(plugins::catalog(PluginKind::VideoFx).any(|i| i.id == GAIN), "offered in the Video FX chooser");
    assert_eq!((info.name, info.group, info.params[0].key), ("Test Gain", "Milan Test", "gain"));
    assert!(entries().iter().filter(|e| e.status == Status::Ok).count() >= 7);
    assert_eq!(entries().iter().find(|e| e.id == DISSOLVE).unwrap().kind, "OFX Transition");
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
    assert_eq!(p.depth, Depth::Float);
    let mut byte = src.clone();
    let r = Render { index: p.desc.index, instance: "byte".into(), w: 64, h: 36, depth: Depth::Byte, scale: 1.0, par: 1.0, fps: 30.0, restored: true, ..Default::default() };
    render(&p, r, &mut byte, None, true).unwrap();
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
    assert_eq!(n, 7);
    assert!(matches!(c.request(&Req::Panic, Some(std::time::Duration::from_secs(5))), Err(client::Fail::Crashed(_))));
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

/// R33a: the FX window's rows follow the plug-in's layout (pages in order, groups with their members, hidden
/// parameters left out, a transition's own progress parameter never shown) and the run-time state when there is one.
#[test]
fn layout_follows_pages_groups_and_state() {
    let mut d = desc();
    d.params[0].children = vec!["tint".into(), "grp".into(), "nope".into()];
    d.params.push(ParamDesc { children: vec!["flag".into()], ..param("page2", "Page", &[]) });
    let mut p = Plugin { info: leak_info(&d), binary: PathBuf::new(), desc: d.clone(), depth: Depth::Float, specs: HashMap::new() };
    for (i, s) in p.info.params.iter().enumerate() {
        p.specs.entry(param_of(&d, s.key).to_string()).or_default().push(i);
    }
    let names = |rows: &[Row]| {
        rows.iter()
            .map(|r| match r {
                Row::Group { label, open, .. } => format!("G:{label}{}", if *open { "" } else { "(closed)" }),
                Row::Heading(h) => format!("H:{h}"),
                Row::Param { label, enabled, options, .. } => format!("{label}{}{}", if *enabled { "" } else { "(off)" }, options.as_ref().map_or(String::new(), |o| format!("[{}]", o.join(",")))),
                Row::Button { label, .. } => format!("B:{label}"),
            })
            .collect::<Vec<_>>()
    };
    let rows = layout_of(&p, None, &|_, d| d);
    assert_eq!(names(&rows), ["H:PAGE", "TINT", "G:GRP", "AMOUNT", "COUNT", "H:PAGE2", "FLAG", "MODE[A,B,C]", "POS X", "POS Y", "IDX X", "IDX Y", "IDX Z", "NOTE", "B:GO", "EMPTY[]"]);
    assert!(rows.iter().any(|r| matches!(r, Row::Param { label, slider: Some((0.0, 4.0)), .. } if label == "AMOUNT")));
    assert!(rows.iter().any(|r| matches!(r, Row::Param { label, slider: Some((lo, hi)), .. } if label == "COUNT" && *lo == -100.0 && *hi == 103.0)));
    // A closed group hides its members; the plug-in's run-time state hides, greys, relabels and refills options.
    let rows = layout_of(&p, None, &|g, _| g != "grp");
    assert!(names(&rows).contains(&"G:GRP(closed)".to_string()) && !names(&rows).iter().any(|n| n == "AMOUNT"));
    let st: HashMap<String, ParamState> = [
        ("tint", ParamState { name: "tint".into(), secret: true, enabled: true, label: "TINT".into(), options: vec![] }),
        ("count", ParamState { name: "count".into(), secret: false, enabled: false, label: "Count!".into(), options: vec![] }),
        ("empty", ParamState { name: "empty".into(), secret: false, enabled: true, label: "EMPTY".into(), options: vec!["X".into(), "Y".into()] }),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    let rows = layout_of(&p, Some(&st), &|_, d| d);
    let n = names(&rows);
    assert!(!n.iter().any(|x| x == "TINT") && n.contains(&"Count!(off)".to_string()) && n.contains(&"EMPTY[X,Y]".to_string()), "{n:?}");
    // The colour hint: a camera log default on an input colour space choice, once.
    let mut c = desc();
    c.id = "com.example.log".into();
    c.params.push(ParamDesc { options: ["ARRI LogC4", "sRGB", "Rec.709 Gamma 2.4"].iter().map(|s| s.to_string()).collect(), ..param("inputColorSpace", "Choice", &[0.0]) });
    c.params.last_mut().unwrap().label = "Input Color Space".into();
    let info = leak_info(&c);
    let id = info.id;
    {
        let mut r = Registry::default();
        r.usable.insert(id.to_string(), Arc::new(Plugin { info, binary: PathBuf::new(), desc: c.clone(), depth: Depth::Float, specs: HashMap::new() }));
        *REG.write().unwrap() = Arc::new(r);
    }
    let fx = plugins::new_fx(info, None);
    let e = plugins::eval(&fx, 0.0).unwrap();
    let hint = colour_hint(id, &fx.id, &e).expect("a hint");
    assert!(hint.contains("Rec.709") && hint.contains("ARRI LogC4") && hint.contains("Input Color Space"), "{hint}");
    let mut srgb = fx.clone();
    srgb.params.insert("inputColorSpace".into(), Keyframes { keys: vec![Keyframe { t: 0.0, v: 1.0, curve: Curve::Linear }] });
    assert!(colour_hint(id, &fx.id, &plugins::eval(&srgb, 0.0).unwrap()).is_none(), "set to sRGB: no hint");
    dismiss_hint(id);
    assert!(colour_hint(id, &fx.id, &e).is_none() && hints_changed().unwrap().contains(id) && hints_changed().is_none());
    assert!(is_bypassed("ofx:nope@1") && !is_bypassed(id) && !is_bypassed("milan.fx.invert"));
    set_registry(&ScanCache::default(), &OfxPrefs::default());
}

/// R33b: an OFX transition and generator render through the host: the dissolve at p = 0.5 is the mean of A and B
/// (Swap turns it round); the 16-bit solid colour generator fills the frame with its (premultiplied) colour.
#[test]
#[ignore]
fn transition_and_generator_render_through_the_host() {
    registered();
    let a = ColorImage::new([16, 8], Color32::from_rgba_premultiplied(200, 100, 0, 255));
    let b = ColorImage::new([16, 8], Color32::from_rgba_premultiplied(0, 100, 200, 255));
    let mut tr = crate::project::Transition { plugin: DISSOLVE.into(), ..Default::default() };
    let mut e = plugins::eval_transition(&tr).expect("an OFX transition evaluates");
    add_frame_vals(&mut e, "ev.tr", true, 0.0, 8.0, 30.0, 1.0);
    let mid = crate::engine::transition::mix(&e, &a, &b, 0.5, 1);
    assert_eq!(mid.pixels[0].to_array(), [100, 100, 100, 255], "{:?}", failure(DISSOLVE));
    assert_eq!(crate::engine::transition::mix(&e, &a, &b, 0.25, 1).pixels[5].to_array(), [150, 100, 50, 255]);
    tr.params.insert("swap".into(), 1.0);
    let mut e = plugins::eval_transition(&tr).unwrap();
    add_frame_vals(&mut e, "ev.tr", true, 0.0, 8.0, 30.0, 1.0);
    assert_eq!(crate::engine::transition::mix(&e, &a, &b, 0.25, 1).pixels[5].to_array(), [50, 100, 150, 255]);
    assert!(failure(DISSOLVE).is_none());
    // The generator, with and without a frame plan (a thumbnail).
    let info = plugins::find(SOLID).unwrap();
    assert_eq!((info.kind, find(SOLID).unwrap().depth), (PluginKind::Generator, Depth::Short));
    let fx = plugins::new_fx(info, None);
    let g = plugins::eval(&fx, 0.0).unwrap();
    assert!(crate::engine::generate::animated(&g));
    let img = crate::engine::generate::render(&g, 8, 4, 0.0);
    assert_eq!(img.pixels[0].to_array(), [255, 128, 64, 255], "{:?}", failure(SOLID));
    let mut half = fx.clone();
    half.params.insert("color.a".into(), Keyframes { keys: vec![Keyframe { t: 0.0, v: 0.5, curve: Curve::Linear }] });
    let mut g = plugins::eval(&half, 0.0).unwrap();
    add_frame_vals(&mut g, &half.id, restored(&half), 0.0, 4.0, 30.0, 1.0);
    assert_eq!(crate::engine::generate::render(&g, 8, 4, 0.0).pixels[31].to_array(), [128, 64, 32, 128]);
    assert!(failure(SOLID).is_none());
    close_hosts();
}

fn wait_idle(id: &str) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while busy(id).is_some() && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(busy(id).is_none(), "the button never returned");
}

/// R33a: what a plug-in does to its own parameters reaches Milan: a new instance's starting values (the seed), a
/// value set while rendering (the render scale it saw), hidden and greyed parameters, labels and choice options
/// changed at run time, push buttons (with the plug-in's message, as the user's own edit), and a plug-in error shown
/// as the plug-in worded it.
#[test]
#[ignore]
fn plug_in_edits_buttons_state_and_messages() {
    registered();
    let _ = take_edits();
    let info = plugins::find(PARAMS).unwrap();
    let mut fx = plugins::new_fx(info, None);
    assert!(!restored(&fx), "a new instance holds no values yet");
    let gray = || ColorImage::new([8, 8], Color32::from_gray(100));
    let mut img = gray();
    vfx::apply(&mut img, &eval_at(PARAMS, Some(fx.clone()), 0.0, 8.0), 1);
    assert!(failure(PARAMS).is_none(), "{:?}", failure(PARAMS));
    assert_eq!(img.pixels[0].to_array(), [100, 100, 100, 255], "amount 1: unchanged");
    let eds = take_edits();
    assert_eq!(eds.len(), 1);
    let ed = &eds[0];
    assert!(ed.initial && !ed.by_user && ed.instance == fx.id && ed.plugin == PARAMS);
    // Milan stores 32-bit floats: a large integer is rounded to one (on both sides, so they agree).
    let seed = 123_456_789f32 as f64;
    assert!(ed.edits.iter().any(|e| e.0 == "seed" && e.1 == PVal::N(vec![seed])), "{:?}", ed.edits);
    assert!(ed.edits.iter().any(|e| e.0 == "note" && e.1 == PVal::S("hi".into())));
    // Values the user set first stand; the seed fills in.
    fx.params.insert("amount".into(), Keyframes { keys: vec![Keyframe { t: 0.0, v: 2.0, curve: Curve::Linear }] });
    assert!(apply_edits(&mut fx, &ed.edits, ed.initial));
    assert_eq!((fx.params["seed"].keys[0].v, fx.params["amount"].keys[0].v, fx.text["note"].as_str()), (seed as f32, 2.0, "hi"));
    assert!(!apply_edits(&mut fx, &ed.edits, true), "applied once");
    // The run-time state: Extra is hidden in Simple mode, shown in Advanced; the layout follows it.
    let rows = |fx: &Fx| layout(PARAMS, &fx.id, &|_, d| d).unwrap();
    let labels = |rows: &[Row]| rows.iter().filter_map(|r| if let Row::Param { label, .. } = r { Some(label.clone()) } else { None }).collect::<Vec<_>>();
    let st = state(&fx.id).expect("reported with the first frame");
    assert!(st["extra"].secret && st["sawScale"].secret);
    assert!(!labels(&rows(&fx)).contains(&"Extra".to_string()));
    assert!(matches!(&rows(&fx)[0], Row::Group { label, open: true, .. } if label == "Controls"));
    // The seed's slider spans a range around its default, the stored value stands outside it.
    assert!(rows(&fx).iter().any(|r| matches!(r, Row::Param { label, slider: Some((lo, hi)), .. } if label == "Seed" && *lo <= 0.0 && *hi <= 100.0)));
    assert_eq!(plugins::eval(&fx, 0.0).unwrap().f("seed"), seed as f32, "not clamped to the slider's range");
    assert_eq!(rows(&fx).iter().filter(|r| matches!(r, Row::Button { .. })).count(), 3);
    // Amount 2 now (restored): the picture doubles. The plug-in writes the render scale it saw (0.5: a preview-size
    // picture) into a parameter during the render: that stays its own, or every differently sized frame would change
    // the project (and render it again).
    let mut img = gray();
    vfx::apply(&mut img, &eval_at(PARAMS, Some(fx.clone()), 0.0, 16.0), 1);
    assert_eq!(img.pixels[0].to_array(), [200, 200, 200, 255]);
    let eds = take_edits();
    assert!(eds.iter().flat_map(|e| &e.edits).all(|e| e.0 != "sawScale"), "{eds:?}");
    for ed in &eds {
        apply_edits(&mut fx, &ed.edits, ed.initial);
    }
    assert!(!fx.params.contains_key("sawScale"));
    vfx::apply(&mut gray(), &eval_at(PARAMS, Some(fx.clone()), 0.0, 8.0), 1);
    assert!(take_edits().iter().flat_map(|e| &e.edits).all(|e| e.0 != "sawScale"), "nor set back and reported again");
    fx.params.insert("mode".into(), Keyframes { keys: vec![Keyframe { t: 0.0, v: 1.0, curve: Curve::Linear }] });
    vfx::apply(&mut gray(), &eval_at(PARAMS, Some(fx.clone()), 0.0, 8.0), 1);
    assert!(!state(&fx.id).unwrap()["extra"].secret, "Advanced shows Extra");
    assert!(labels(&rows(&fx)).contains(&"Extra".to_string()));
    let _ = take_edits();
    // Push buttons: Bump adds one to Amount with a message; Refresh refills the list, relabels it, greys Note and picks
    // "Three"; Broken fails with the plug-in's own error and does not bypass the plug-in.
    press(None, &fx, 0.0, "bump", (8, 8), 30.0, 1.0);
    wait_idle(&fx.id);
    let eds = take_edits();
    assert_eq!(eds.len(), 1);
    assert!(eds[0].by_user && !eds[0].initial);
    assert!(eds[0].edits.contains(&("amount".to_string(), PVal::N(vec![3.0]))), "{:?}", eds[0].edits);
    assert!(apply_edits(&mut fx, &eds[0].edits, false));
    assert_eq!(fx.params["amount"].keys[0].v, 3.0);
    assert_eq!(note(&fx.id).as_deref(), Some("Message: [bump] bumped to 3"));
    press(None, &fx, 0.0, "refresh", (8, 8), 30.0, 1.0);
    wait_idle(&fx.id);
    let eds = take_edits();
    assert!(eds[0].edits.contains(&("list".to_string(), PVal::N(vec![2.0]))));
    let st = state(&fx.id).unwrap();
    assert_eq!((st["list"].options.clone(), st["list"].label.as_str(), st["note"].enabled), (vec!["One".to_string(), "Two".into(), "Three".into()], "List (3)", false));
    let r = rows(&fx);
    assert!(r.iter().any(|r| matches!(r, Row::Param { label, options: Some(o), .. } if label == "List (3)" && o.len() == 3)));
    assert!(r.iter().any(|r| matches!(r, Row::Param { label, enabled: false, .. } if label == "Note")));
    assert!(note(&fx.id).is_none(), "a button without messages clears the last ones");
    press(None, &fx, 0.0, "broken", (8, 8), 30.0, 1.0);
    wait_idle(&fx.id);
    assert_eq!(note(&fx.id).as_deref(), Some("Error: [broken] Could not do that."));
    assert!(failure(PARAMS).is_none() && take_edits().is_empty());
    // A render error carries the plug-in's words, verbatim, into the failure the FX window and Plug-In Manager show.
    fx.params.insert("mode".into(), Keyframes { keys: vec![Keyframe { t: 0.0, v: 2.0, curve: Curve::Linear }] });
    vfx::apply(&mut gray(), &eval_at(PARAMS, Some(fx.clone()), 0.0, 8.0), 1);
    let why = failure(PARAMS).expect("bypassed");
    assert!(why.contains("Error: [renderer] No Vulkan physical device is available. (7)"), "{why}");
    assert!(bypassed_because(PARAMS).unwrap().contains("No Vulkan physical device is available."));
    assert!(entries().iter().any(|e| e.id == PARAMS && matches!(&e.status, Status::Failed(w) if w.contains("No Vulkan"))));
    clear_failures();
    close_hosts();
}

/// R33b bench: the plug-in host round trip at 1080p (shared memory both ways, the OFX invert) against the built-in
/// invert on the same picture. Prints BENCH lines for tools/bench.ps1.
#[test]
#[ignore]
fn bench_ofx_roundtrip_1080p() {
    registered();
    let (w, h) = (1920usize, 1080usize);
    let src = ColorImage { size: [w, h], pixels: (0..w * h).map(|i| Color32::from_rgba_premultiplied((i % 251) as u8, (i / w % 253) as u8, (i % 97) as u8, 255)).collect() };
    let builtin = plugins::eval(&plugins::new_fx(plugins::find("milan.fx.invert").unwrap(), None), 0.0).unwrap();
    let k = std::thread::available_parallelism().map_or(1, |n| n.get());
    let time = |e: &FxEval, k: usize| {
        let mut img = src.clone();
        vfx::apply(&mut img, e, k); // warm: the host starts, the instance is made
        let t = std::time::Instant::now();
        for _ in 0..20 {
            img = src.clone();
            vfx::apply(&mut img, e, k);
        }
        (t.elapsed().as_secs_f64() * 1000.0 / 20.0, img)
    };
    let (base, want) = time(&builtin, k);
    let (float, got) = time(&eval_at(INVERT, None, 0.0, h as f64), 1);
    assert!(max_diff(&got, &want) <= 1 && failure(INVERT).is_none());
    let p = find(INVERT).unwrap();
    let byte = {
        let r = Render { index: p.desc.index, instance: "bench8".into(), w: w as u32, h: h as u32, depth: Depth::Byte, scale: 1.0, par: 1.0, fps: 30.0, restored: true, ..Default::default() };
        let mut img = src.clone();
        render(&p, r.clone(), &mut img, None, true).unwrap();
        let t = std::time::Instant::now();
        for _ in 0..20 {
            render(&p, r.clone(), &mut img, None, true).unwrap();
        }
        t.elapsed().as_secs_f64() * 1000.0 / 20.0
    };
    println!("BENCH ofx_roundtrip_1080p float_ms={float:.2} byte_ms={byte:.2} builtin_invert_ms={base:.2} float_overhead_ms={:.2}", float - base);
    close_hosts();
}

/// A5: the plug-in host, started as Milan starts it (Job Object, no window), can make a Vulkan device of its own, as
/// Spektrafilm does. Strict where the system has a Vulkan loader; elsewhere it only has to answer.
#[test]
#[ignore]
fn the_host_can_make_a_vulkan_device() {
    let bin = test_bundle().join("MilanTest.ofx.bundle").join("Contents").join("Win64").join("MilanTest.ofx");
    let (mut c, _) = Conn::start(&bin).unwrap();
    let reply = c.request(&Req::GpuCheck, Some(std::time::Duration::from_secs(20))).expect("the host answers");
    println!("{reply:?}");
    let loader = std::env::var_os("SystemRoot").map(|r| PathBuf::from(r).join("System32").join("vulkan-1.dll")).is_some_and(|p| p.is_file());
    match reply {
        Reply::Rendered(o) => assert!(o.messages[0].contains("compute queue)") && !o.messages[0].contains(" 0 physical"), "{:?}", o.messages),
        Reply::Error(e) => assert!(!loader, "a Vulkan loader is installed but the host could not use it: {e}"),
        r => panic!("{r:?}"),
    }
    c.quit();
}

/// R33c: every OFX plug-in in the VEGAS Pro 15 bundles (`MILAN_VEGAS_OFX`, default `C:\Program Files\VEGAS\VEGAS Pro
/// 15.0\OFX Video Plug-Ins`; read only, never copied) is loaded, described and rendered once (96x54) in probe hosts:
/// background flag on, a hidden desktop, dialogs noted, a hang killed. Writes `ofx-vegas15.csv` and the dialog log to
/// `MILAN_COMPAT_OUT` (default %TEMP%\milan-agent\ofx2\compat). Nothing is asserted about the plug-ins: the CSV is the
/// result; the test only checks that the probe itself ran every binary and wrote a row per plug-in.
#[test]
#[ignore]
fn vegas15_compat_report() {
    let dir = std::env::var_os("MILAN_VEGAS_OFX").map_or_else(|| PathBuf::from(r"C:\Program Files\VEGAS\VEGAS Pro 15.0\OFX Video Plug-Ins"), PathBuf::from);
    if !dir.is_dir() {
        eprintln!("{} is not there: nothing to test", dir.display());
        return;
    }
    let out_dir = std::env::var_os("MILAN_COMPAT_OUT").map_or_else(|| std::env::temp_dir().join("milan-agent").join("ofx2").join("compat"), PathBuf::from);
    std::fs::create_dir_all(&out_dir).unwrap();
    let log = out_dir.join("host-dialogs.log");
    let _ = std::fs::remove_file(&log);
    *lock(&client::PROBE_LOG) = Some(log.clone());
    // `MILAN_COMPAT_DLL_DIRS` (;-separated): folders the probe hosts may load a plug-in's imports from (the VEGAS folder
    // holds ErrorReport.dll, OpenColorIO.dll and proDADMercalli20.dll); Milan Pro itself never adds them.
    let dll_dirs: Vec<PathBuf> = std::env::var_os("MILAN_COMPAT_DLL_DIRS").map(|v| std::env::split_paths(&v).filter(|p| !p.as_os_str().is_empty()).collect()).unwrap_or_default();
    *lock(&client::PROBE_DLL_DIRS) = dll_dirs.clone();
    client::DESCRIBE_TIMEOUT_MS.store(10_000, Ordering::Relaxed);
    client::RENDER_TIMEOUT_MS.store(20_000, Ordering::Relaxed);
    let started = std::time::Instant::now();
    let cache = scan::scan(&[dir.clone()], &ScanCache::default(), true, &scan::describe_binary);
    set_registry(&cache, &OfxPrefs::default());
    let describe_ms = started.elapsed().as_millis();
    let (w, h) = (96usize, 54usize);
    let src = ColorImage { size: [w, h], pixels: (0..w * h).map(|i| Color32::from_rgba_premultiplied((i % w * 255 / w) as u8, (i / w * 255 / h) as u8, 128, 255)).collect() };
    let b_img = ColorImage::new([w, h], Color32::from_rgba_premultiplied(20, 200, 40, 255));
    let csv_cell = |s: &str| {
        let s = s.replace('\n', " ");
        if s.contains([',', '"']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s }
    };
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut binaries = 0;
    for b in &cache.binaries {
        binaries += 1;
        let file = b.path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        for (i, e) in &b.errors {
            let idx = i.map_or(String::new(), |i| i.to_string());
            rows.push(vec![file.clone(), idx, String::new(), String::new(), String::new(), String::new(), String::new(), String::new(), "0".into(), if i.is_none() { "no".into() } else { "yes".into() }, "no".into(), "no".into(), String::new(), e.clone()]);
        }
        for d in &b.plugins {
            let id = id_of(d);
            let base = vec![file.clone(), d.index.to_string(), d.id.clone(), d.label.clone(), d.group.clone(), d.contexts.iter().map(|c| c.trim_start_matches("OfxImageEffectContext")).collect::<Vec<_>>().join(" "), d.context.trim_start_matches("OfxImageEffectContext").to_string(), d.depths.iter().map(|x| x.trim_start_matches("OfxBitDepth")).collect::<Vec<_>>().join(" "), d.params.len().to_string(), "yes".into(), "yes".into()];
            let mut row = base;
            if let Some(why) = unsupported(d) {
                row.extend(["no".into(), String::new(), why]);
                rows.push(row);
                continue;
            }
            let Some(info) = plugins::find(&id) else {
                row.extend(["no".into(), String::new(), "not registered".into()]);
                rows.push(row);
                continue;
            };
            let fx = plugins::new_fx(info, None);
            let mut e = plugins::eval(&fx, 0.0).expect("evaluates");
            add_frame_vals(&mut e, &fx.id, false, 0.0, h as f64, 30.0, 1.0);
            let t = std::time::Instant::now();
            let what = match info.kind {
                PluginKind::Transition => transition(&e, &src, &b_img, 0.5).map(|o| if o.pixels == src.pixels { "renders (SourceFrom passed through)" } else { "renders" }),
                PluginKind::Generator => {
                    let o = generate(&e, w as u32, h as u32, 0.0);
                    failure(&id).is_none().then(|| if o.pixels.iter().all(|p| p.a() == 0) { "renders (transparent)" } else { "renders" })
                }
                _ => {
                    let mut img = src.clone();
                    apply(&mut img, &e);
                    failure(&id).is_none().then(|| if img.pixels == src.pixels { "renders (identity at defaults)" } else { "renders" })
                }
            };
            let ms = t.elapsed().as_millis().to_string();
            match (what, failure(&id)) {
                (Some(w), None) => row.extend([w.to_string(), ms, String::new()]),
                (_, why) => row.extend(["no".into(), ms, why.unwrap_or_default()]),
            }
            rows.push(row);
        }
    }
    // Dialogs the hosts' window watcher saw.
    let dialogs = std::fs::read_to_string(&log).unwrap_or_default();
    let mut csv = String::from("binary,index,identifier,label,grouping,contexts,context_used,depths,params,loads,describes,renders,render_ms,error\n");
    for r in &rows {
        csv.push_str(&r.iter().map(|c| csv_cell(c)).collect::<Vec<_>>().join(","));
        csv.push('\n');
    }
    std::fs::write(out_dir.join("ofx-vegas15.csv"), &csv).unwrap();
    let plugins_n = rows.iter().filter(|r| !r[2].is_empty()).count();
    let rendered = rows.iter().filter(|r| r[11].starts_with("renders")).count();
    println!("COMPAT binaries={binaries} rows={} plugins={plugins_n} rendered={rendered} describe_ms={describe_ms} total_ms={} dll_dirs={dll_dirs:?} dialogs={:?}", rows.len(), started.elapsed().as_millis(), dialogs.lines().collect::<Vec<_>>());
    println!("{}", out_dir.join("ofx-vegas15.csv").display());
    *lock(&client::PROBE_LOG) = None;
    lock(&client::PROBE_DLL_DIRS).clear();
    client::DESCRIBE_TIMEOUT_MS.store(2_000, Ordering::Relaxed);
    client::RENDER_TIMEOUT_MS.store(30_000, Ordering::Relaxed);
    clear_failures();
    close_hosts();
    set_registry(&ScanCache::default(), &OfxPrefs::default());
    assert!(binaries > 0 && !rows.is_empty());
}
