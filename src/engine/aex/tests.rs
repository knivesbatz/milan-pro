use super::*;
use crate::engine::ofx::{self, ScanCache, Status, scan};
use crate::engine::plugins::{self, FxEval};
use crate::engine::vfx;
use crate::project::{Curve, Keyframe, Keyframes};
use eframe::egui::{Color32, ColorImage};
use std::sync::OnceLock;

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join("milan-agent").join("aex").join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn effect(name: &str, match_name: &str, out_flags: u32) -> pipl::Effect {
    pipl::Effect { kind: "eFKT".into(), name: name.into(), category: "Cat".into(), entry: "Main".into(), match_name: match_name.into(), out_flags, ..Default::default() }
}

/// A1 acceptance: nothing is searched while the feature is off (the default); when on, the shared folders the registry
/// names come first, then the standard shared folders, then the user's, each once. Adobe application folders are not
/// among them.
#[test]
fn scan_is_off_by_default() {
    let off = AexPrefs::default();
    assert!(!off.enabled);
    assert!(folders(&off).is_empty(), "the real folders too");
    let env = |k: &str| (k == "ProgramFiles").then(|| r"C:\PF".into());
    assert!(folders_with(&off, &env, &|| vec![PathBuf::from(r"C:\Reg")]).is_empty());
    let on = AexPrefs { enabled: true, folders: vec![r"D:\mine".into(), r"c:\pf\adobe\common\plug-ins\7.0\mediacore\".into()] };
    assert_eq!(
        folders_with(&on, &env, &|| vec![PathBuf::from(r"C:\Reg\MediaCore")]),
        [r"C:\Reg\MediaCore", r"C:\PF\Adobe\Common\Plug-ins\7.0\MediaCore", r"C:\PF\Adobe\Common\Plugins\7.0\MediaCore", r"D:\mine"].map(PathBuf::from)
    );
    // The preference is kept with the OFX ones and an old preferences file (no "aex") reads as off.
    let p: ofx::OfxPrefs = serde_json::from_str(r#"{"folders":[]}"#).unwrap();
    assert_eq!(p.aex, AexPrefs::default());
    // A scan with the feature off lists no AE-format file even in a searched folder.
    let d = temp("off");
    std::fs::write(d.join("x.aex"), pipl::tests_support::fake_dll(&[effect("X", "VEND X", 0)])).unwrap();
    let c = scan::scan_all(&[d.clone()], &folders(&off), &ScanCache::default(), false, &scan::describe_binary);
    assert!(c.binaries.is_empty());
    std::fs::remove_dir_all(&d).unwrap();
}

/// A1: the search follows After Effects' rules: 10 folders deep, not into folders ending in ')' or starting with '~'.
#[test]
fn aex_files_are_found_as_after_effects_finds_them() {
    let d = temp("find");
    let mut deep = d.clone();
    for i in 0..11 {
        deep = deep.join(format!("l{i}"));
    }
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(deep.join("too-deep.aex"), b"x").unwrap();
    std::fs::write(deep.parent().unwrap().parent().unwrap().join("deep-enough.aex"), b"x").unwrap();
    for skip in ["Old (disabled)", "~temp"] {
        std::fs::create_dir_all(d.join(skip)).unwrap();
        std::fs::write(d.join(skip).join("skipped.aex"), b"x").unwrap();
    }
    std::fs::write(d.join("Top.AEX"), b"x").unwrap();
    std::fs::write(d.join("readme.txt"), b"x").unwrap();
    let names: Vec<String> = find(&[d.clone(), d.clone()]).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["Top.AEX", "deep-enough.aex"]);
    std::fs::remove_dir_all(&d).unwrap();
}

/// A1 acceptance: Adobe's own effects are Blocked (by match name, or inside an Adobe application's folder; the shared
/// MediaCore folder is not one), whatever the user turned on; audio effects and other machines are not run; nothing
/// is loaded to find this out.
#[test]
fn adobe_match_names_are_blocked() {
    assert!(blocked(Path::new(r"D:\fx\a.aex"), "ADBE Gaussian Blur 2").is_some());
    assert!(blocked(Path::new(r"C:\Program Files\Adobe\Adobe After Effects 2025\Support Files\Plug-ins\Effects\x.aex"), "VEND X").unwrap().contains("application"));
    assert!(blocked(Path::new(r"C:\Program Files\Adobe\Adobe Premiere Pro 2025\Plug-ins\Common\x.aex"), "VEND X").is_some());
    assert!(blocked(Path::new(r"C:\Program Files\Adobe\Common\Plug-ins\7.0\MediaCore\Vendor\x.aex"), "VEND X").is_none());
    assert!(blocked(Path::new(r"D:\fx\a.aex"), "ADBEX not adobe's").is_none());
    let d = temp("blocked");
    let dll = pipl::tests_support::fake_dll(&[effect("Blur", "ADBE Blur", 0), effect("Sound", "VEND Sound", 1 << 20), effect("Fine", "VEND Fine", 0)]);
    std::fs::write(d.join("mixed.aex"), &dll).unwrap();
    std::fs::write(d.join("x86.aex"), pipl::tests_support::fake_dll_for(0x14c, &[effect("Old", "VEND Old", 0)])).unwrap();
    std::fs::write(d.join("junk.aex"), b"MZ not really").unwrap();
    // Classification only: nothing is loaded (`read_static`).
    let cache = scan::scan_all(&[], &[d.clone()], &ScanCache::default(), false, &|p| read_static(p));
    let prefs = ofx::OfxPrefs { disabled: ["aex:ADBE Blur".to_string()].into(), ..Default::default() };
    ofx::set_registry(&cache, &prefs);
    let es: Vec<ofx::Entry> = ofx::entries().into_iter().filter(|e| e.kind == KIND && e.binary.starts_with(&d)).collect();
    let st = |id: &str| es.iter().find(|e| e.id == id).map(|e| e.status.clone());
    assert!(matches!(st("aex:ADBE Blur"), Some(Status::Blocked(w)) if w.contains("Adobe")), "blocked even when also turned off: {es:?}");
    assert!(matches!(st("aex:VEND Sound"), Some(Status::Failed(w)) if w.contains("audio")));
    assert_eq!(st("aex:VEND Fine"), Some(Status::Ok));
    assert!(es.iter().any(|e| e.id.is_empty() && matches!(&e.status, Status::Failed(w) if w.contains("64-bit"))));
    assert!(es.iter().any(|e| e.id.is_empty() && e.binary.ends_with("junk.aex")));
    assert_eq!(ofx::label("aex:ADBE Blur").as_deref(), Some("Blur (blocked)"));
    assert!(ofx::bypassed_because("aex:ADBE Blur").unwrap().contains("Adobe"));
    assert!(ofx::bypassed_because("aex:Gone Thing").unwrap().contains(SWITCH));
    assert!(ofx::is_bypassed("aex:VEND Fine"), "known from its PiPL only: not run");
    ofx::set_registry(&ScanCache::default(), &Default::default());
    std::fs::remove_dir_all(&d).unwrap();
}

// ---- With the test plug-in (tools/aex-test-plugin). Run after `cargo build --release`:
// cargo test --release -- --ignored --test-threads=1 aex::

/// The test plug-in, built once, as `MilanTest.aex` in a folder of its own under %TEMP%\milan-agent\aex.
pub fn test_plugin_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let target = repo.join("target").join("aex-test-plugin");
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let mut cmd = std::process::Command::new(cargo);
        cmd.args(["build", "--release", "--offline", "--quiet", "--manifest-path"]).arg(repo.join("tools").join("aex-test-plugin").join("Cargo.toml")).arg("--target-dir").arg(&target);
        let st = crate::proc::output(&mut cmd).expect("cargo runs");
        assert!(st.status.success(), "{}", String::from_utf8_lossy(&st.stderr));
        let built = target.join("release").join("milan_aex_test.dll");
        let place = |dir: PathBuf| {
            std::fs::create_dir_all(&dir)?;
            std::fs::copy(&built, dir.join("MilanTest.aex")).map(|_| dir)
        };
        let base = std::env::temp_dir().join("milan-agent").join("aex");
        place(base.join("plugins")).or_else(|_| place(base.join(format!("plugins-{}", std::process::id())))).unwrap()
    })
    .clone()
}

/// A1 acceptance: the test plug-in's file is found and every one of its PiPLs read (in Milan, as data), with names,
/// match names, entry points and flags; the Plug-In Manager rows say "AE-format effect".
#[test]
#[ignore]
fn aex_scan_lists_the_test_plugin() {
    let dir = test_plugin_dir();
    let prefs = AexPrefs { enabled: true, folders: vec![dir.clone()] };
    let only_mine = folders(&prefs).into_iter().filter(|f| f.starts_with(&dir)).collect::<Vec<_>>();
    let cache = scan::scan_all(&[], &only_mine, &ScanCache::default(), false, &|p| read_static(p));
    assert_eq!(cache.binaries.len(), 1);
    let b = &cache.binaries[0];
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    let names: Vec<&str> = b.plugins.iter().map(|d| d.label.as_str()).collect();
    assert_eq!(names.len(), 13, "{names:?}");
    assert_eq!(names[0], "Test Invert");
    let counter = b.plugins.iter().find(|d| d.label == "Test Counter").unwrap();
    let a = counter.aex.as_ref().unwrap();
    assert_eq!((a.match_name.as_str(), a.entry.as_str(), a.out_flags, a.spec), ("MLNT Test Counter", "MilanTestCounter", 1 << 4, (13, 4)));
    assert_eq!((counter.group.as_str(), counter.major), ("Milan Test", 1));
    assert!(b.plugins.iter().all(|d| d.aex.as_ref().is_some_and(|a| a.blocked.is_empty() && a.unsupported.is_empty())));
    assert!(b.plugins.iter().find(|d| d.label == "Test Params").unwrap().aex.as_ref().unwrap().caps.iter().any(|c| c.contains("custom controls")));
    ofx::set_registry(&cache, &ofx::OfxPrefs::default());
    let rows: Vec<ofx::Entry> = ofx::entries().into_iter().filter(|e| e.binary == b.path).collect();
    assert_eq!(rows.len(), 13);
    assert!(rows.iter().all(|e| e.kind == KIND && e.id.starts_with("aex:MLNT ")));
    ofx::set_registry(&ScanCache::default(), &Default::default());
}

/// A1: the PiPLs of the open-source plug-ins in MILAN_AEX_TEST_DIR (plan §3.2) parse; prints what they say. Not run:
/// none of them was available when this was written (`docs/compat/aex.csv`).
#[test]
#[ignore]
fn pipl_of_open_source_plugins_parses() {
    let Some(dir) = std::env::var_os("MILAN_AEX_TEST_DIR").map(PathBuf::from) else {
        panic!("set MILAN_AEX_TEST_DIR to a folder of open-source .aex plug-ins (docs/AE_PREMIERE_PLUGINS.md §3.2)");
    };
    let files = find(&[dir]);
    assert!(!files.is_empty(), "no .aex files found");
    for f in files {
        let b = read_static(&f);
        assert!(b.errors.is_empty(), "{}: {:?}", f.display(), b.errors);
        for d in &b.plugins {
            let a = d.aex.as_ref().unwrap();
            println!("{}: {} / {} / entry {} / flags 0x{:08X} 0x{:08X} / spec {}.{}", f.display(), d.label, a.match_name, a.entry, a.out_flags, a.out_flags2, a.spec.0, a.spec.1);
            assert!(!a.match_name.is_empty() && !a.entry.is_empty());
        }
    }
}

/// A4 acceptance: a project that uses an AE-format effect nobody has keeps its parameters and its saved sequence data
/// through save and load, shows it as missing and bypassed, and an old project's FX (no "blob") load with none.
#[test]
fn a_project_with_a_missing_aex_plugin_keeps_its_parameters() {
    let mut fx = plugins::new_fx(plugins::find("milan.fx.invert").unwrap(), None);
    fx.plugin = "aex:VEND Gone Effect".into();
    fx.params.insert("p3".into(), Keyframes { keys: vec![Keyframe { t: 0.0, v: 0.5, curve: Curve::Linear }] });
    fx.blob.insert("seq".into(), "AQID".into());
    let json = serde_json::to_string(&fx).unwrap();
    let back: Fx = serde_json::from_str(&json).unwrap();
    assert_eq!(back, fx, "parameters and blob survive");
    assert!(ofx::is_bypassed(&fx.plugin));
    assert!(ofx::bypassed_because(&fx.plugin).unwrap().contains("not installed"));
    assert_eq!(ofx::label(&fx.plugin).as_deref(), Some("VEND Gone Effect (missing)"));
    assert!(plugins::eval(&fx, 0.0).is_none(), "nothing runs it");
    // An FX from before A4 has no blob, and an empty blob is not written.
    let mut old: serde_json::Value = serde_json::to_value(&back).unwrap();
    old.as_object_mut().unwrap().remove("blob");
    let old: Fx = serde_json::from_value(old).unwrap();
    assert!(old.blob.is_empty());
    assert!(!serde_json::to_string(&old).unwrap().contains("blob"));
}

// ---- A2-A5 through the plug-in host (`cargo build --release` first), `--ignored --test-threads=1`.

const INVERT: &str = "aex:MLNT Test Invert";
const GAIN: &str = "aex:MLNT Test Gain";
const CRASH: &str = "aex:MLNT Test Crash";
const CRASH_SETUP: &str = "aex:MLNT Test Crash Setup";
const HANG: &str = "aex:MLNT Test Hang";
const PARAMS: &str = "aex:MLNT Test Params";
const MARKER: &str = "aex:MLNT Test Marker";
const COUNTER: &str = "aex:MLNT Test Counter";
const CALLBACKS: &str = "aex:MLNT Test Callbacks";
const PREVIOUS: &str = "aex:MLNT Test Previous Frame";
const SUPERVISOR: &str = "aex:MLNT Test Supervisor";
const LAYER: &str = "aex:MLNT Test Layer";
const ECHO: &str = "aex:MLNT Test Echo";

fn registered() -> ScanCache {
    assert!(ofx::client::host_exe().is_some(), "build milan-plugin-host first: cargo build --release");
    let cache = scan::scan_all(&[], &[test_plugin_dir()], &ScanCache::default(), false, &scan::describe_binary);
    ofx::set_registry(&cache, &Default::default());
    cache
}

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

fn opaque(seed: usize) -> ColorImage {
    let (w, h) = (40usize, 24usize);
    ColorImage { size: [w, h], pixels: (0..w * h).map(|i| Color32::from_rgb(((i * 7 + seed * 31) % 256) as u8, ((i * 3 + seed) % 256) as u8, ((i / w * 11 + seed * 5) % 256) as u8)).collect() }
}

fn new_fx(id: &str) -> Fx {
    plugins::new_fx(plugins::find(id).unwrap_or_else(|| panic!("{id} registered: {:?}", ofx::entries().iter().find(|e| e.id == id))), None)
}

fn eval_at(fx: &Fx, t: f64, full_h: f64) -> FxEval {
    let mut e = plugins::eval(fx, t).expect("evaluates");
    ofx::add_frame_vals(&mut e, &fx.id, ofx::restored(fx), t, full_h, 30.0, 1.0);
    e
}

fn run(fx: &Fx, img: &ColorImage, t: f64) -> ColorImage {
    let mut out = img.clone();
    vfx::apply(&mut out, &eval_at(fx, t, img.size[1] as f64), 1);
    out
}

fn max_diff(a: &ColorImage, b: &ColorImage) -> u8 {
    a.pixels.iter().zip(&b.pixels).flat_map(|(p, q)| (0..4).map(move |i| p.to_array()[i].abs_diff(q.to_array()[i]))).max().unwrap_or(0)
}

/// A2 acceptance: the test plug-in is described through the plug-in host: every parameter type maps into Milan's
/// controls (groups, percent sliders, integer and fixed sliders, angles in degrees, checkboxes, colours, points in
/// percent, popups without their separators, layer parameters, hidden and greyed ones; buttons not shown), the effects
/// are offered in the Video FX chooser, and the suites it asked for that Milan does not serve are noted.
#[test]
#[ignore]
fn aex_test_plugin_describes() {
    let cache = registered();
    let b = &cache.binaries[0];
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert_eq!(b.plugins.len(), 13);
    let d = b.plugins.iter().find(|d| ofx::id_of(d) == PARAMS).unwrap();
    let a = d.aex.as_ref().unwrap();
    assert!(a.described && a.unsupported.is_empty(), "{a:?}");
    let got: Vec<(&str, &str, &str)> = d.params.iter().map(|p| (p.name.as_str(), p.kind.as_str(), p.parent.as_str())).collect();
    assert_eq!(
        got,
        [("p10", "Group", ""), ("p1", "Double", "p10"), ("p2", "Integer", "p10"), ("p3", "Double", ""), ("p4", "Double", ""), ("p5", "Boolean", ""), ("p6", "RGB", ""), ("p7", "Double2D", ""), ("p8", "Choice", ""), ("p12", "Choice", ""), ("p13", "Double", ""), ("p14", "Double", "")]
    );
    let p = |n: &str| d.params.iter().find(|p| p.name == n).unwrap();
    assert_eq!((p("p1").label.as_str(), p("p1").default.as_slice(), p("p1").max), ("Amount (%)", &[50.0][..], 100.0));
    assert_eq!((p("p3").default[0], p("p4").default[0], p("p4").label.as_str()), (1.5, 30.0, "Angle (°)"));
    assert_eq!(p("p6").default, [1.0, 128.0 / 255.0, 0.0]);
    assert_eq!(p("p7").default, [50.0, 25.0]);
    assert_eq!((p("p8").options.clone(), p("p8").default.clone()), (vec!["One".to_string(), "Two".into(), "Three".into()], vec![2.0]));
    assert_eq!(p("p12").options, ["None", "This event"]);
    assert!(p("p13").secret && !p("p14").enabled && p("p5").default == [1.0]);
    assert!(a.refused.iter().any(|r| r == "PF Made Up Suite v2"), "the unserved suite is noted: {:?}", a.refused);
    assert!(a.caps.iter().any(|c| c.contains("custom controls")));
    let info = plugins::find(PARAMS).unwrap();
    let keys: Vec<&str> = info.params.iter().map(|s| s.key).collect();
    assert_eq!(keys, ["p1", "p2", "p3", "p4", "p5", "p6", "p7.x", "p7.y", "p8", "p12", "p13", "p14"]);
    assert_eq!((info.group, info.category), ("Milan Test", "Third Party"));
    assert!(plugins::catalog(plugins::PluginKind::VideoFx).any(|i| i.id == INVERT), "offered in the Video FX chooser");
    assert!(fx_note(PARAMS).unwrap().contains("custom controls"));
    let rows = ofx::layout(PARAMS, "x", &|_, open| open).unwrap();
    assert!(rows.iter().any(|r| matches!(r, ofx::Row::Group { label, .. } if label == "Levels")));
    // It renders (a copy of its input, with every parameter type set).
    let src = picture();
    assert_eq!(max_diff(&run(&new_fx(PARAMS), &src, 0.0), &src), 0, "{:?}", ofx::failure(PARAMS));
    ofx::close_hosts();
}

/// A2 acceptance: an effect that crashes in its global setup costs that effect only: it is listed as failed with why,
/// the others of the same file are described (in a new host) and run.
#[test]
#[ignore]
fn a_plugin_that_crashes_in_global_setup_is_contained() {
    registered();
    let e = ofx::entries().into_iter().find(|e| e.id == CRASH_SETUP).unwrap();
    assert!(matches!(&e.status, Status::Failed(w) if w.contains("crashed")), "{:?}", e.status);
    assert!(plugins::find(CRASH_SETUP).is_none());
    let es = ofx::entries();
    assert_eq!(es.iter().filter(|e| e.kind == KIND && e.status == Status::Ok).count(), 12, "{es:?}");
    let img = picture();
    let out = run(&new_fx(INVERT), &img, 0.0);
    assert!(ofx::failure(INVERT).is_none() && out != img);
    ofx::close_hosts();
}

/// A3 acceptance: the test plug-in's Callbacks effect uses every tier-a callback and suite (utils copy and world,
/// fill matte, colour parameter, iterate, world transform blend, handles, ANSI maths and sprintf, world suite,
/// iterate_generic, abort and progress, a refused suite); its picture equals the Rust reference of the same steps.
#[test]
#[ignore]
fn aex_callbacks_match_a_rust_reference() {
    registered();
    let src = opaque(3);
    let out = run(&new_fx(CALLBACKS), &src, 0.0);
    assert!(ofx::failure(CALLBACKS).is_none(), "{:?}", ofx::failure(CALLBACKS));
    let [w, h] = src.size;
    let reference: Vec<Color32> = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let s = src.pixels[i].to_array();
            let t = if x < w / 2 && y < h / 2 { [10, 20, 30, 255] } else { s };
            let rot = [t[1], t[2], t[0], t[3]];
            let mix = |a: u8, b: u8| ((a as u32 * 32768 + b as u32 * 32768 + 32768) >> 16) as u8;
            Color32::from_rgba_premultiplied(mix(rot[0], s[0]), mix(rot[1], s[1]), mix(rot[2], s[2]), mix(rot[3], s[3]))
        })
        .collect();
    let reference = ColorImage { size: src.size, pixels: reference };
    assert_eq!(max_diff(&out, &reference), 0, "the picture differs from the Rust reference");
    ofx::close_hosts();
}

/// A4 acceptance: the AE-format invert equals the built-in invert within 1 LSB through shared memory, half-transparent
/// rows included.
#[test]
#[ignore]
fn aex_invert_matches_the_builtin_invert() {
    registered();
    let src = picture();
    let mut builtin = src.clone();
    vfx::apply(&mut builtin, &plugins::eval(&plugins::new_fx(plugins::find("milan.fx.invert").unwrap(), None), 0.0).unwrap(), 1);
    let out = run(&new_fx(INVERT), &src, 0.0);
    assert!(ofx::failure(INVERT).is_none(), "{:?}", ofx::failure(INVERT));
    assert!(max_diff(&out, &builtin) <= 1, "{}", max_diff(&out, &builtin));
    ofx::close_hosts();
}

/// A4 acceptance: a keyframed parameter is sent at the frame's time (gain 1 at 0 s to 3 at 2 s: 2 at 1 s).
#[test]
#[ignore]
fn aex_gain_is_keyframed_at_t() {
    registered();
    let mut fx = new_fx(GAIN);
    fx.params.insert("p1".into(), Keyframes { keys: vec![Keyframe { t: 0.0, v: 1.0, curve: Curve::Linear }, Keyframe { t: 2.0, v: 3.0, curve: Curve::Linear }] });
    for (t, want) in [(0.0, 50), (1.0, 100), (2.0, 150)] {
        let out = run(&fx, &ColorImage::new([8, 8], Color32::from_gray(50)), t);
        assert_eq!(out.pixels[0].to_array(), [want, want, want, 255], "t = {t}: {:?}", ofx::failure(GAIN));
    }
    ofx::close_hosts();
}

/// A4 acceptance: the Marker effect paints its world's row 0 red at the left and green at the right and its last row
/// blue at the left: Milan shows red top-left, green top-right, blue bottom-left (rows and channels the right way).
#[test]
#[ignore]
fn row_order_and_channel_order_marker() {
    registered();
    let out = run(&new_fx(MARKER), &ColorImage::new([8, 6], Color32::TRANSPARENT), 0.0);
    assert!(ofx::failure(MARKER).is_none(), "{:?}", ofx::failure(MARKER));
    assert_eq!(out.pixels[0].to_array(), [255, 0, 0, 255]);
    assert_eq!(out.pixels[7].to_array(), [0, 255, 0, 255]);
    assert_eq!(out.pixels[5 * 8].to_array(), [0, 0, 255, 255]);
    assert_eq!(out.pixels[9].to_array(), [0, 0, 0, 0]);
    ofx::close_hosts();
}

/// A4 acceptance: the Counter keeps a count in sequence data that holds a pointer; saving the project flattens it into
/// `Fx.blob`, and after the project is loaded in a new plug-in host the count goes on from there (resetup).
#[test]
#[ignore]
fn sequence_data_survives_save_and_load() {
    use crate::project::{Clip, Project};
    registered();
    let fx = new_fx(COUNTER);
    let img = ColorImage::new([4, 4], Color32::BLACK);
    for k in 1..=3u8 {
        let out = run(&fx, &img, k as f64 / 30.0);
        assert_eq!(out.pixels[0].r(), k, "frame {k}: {:?}", ofx::failure(COUNTER));
    }
    let dir = temp("seq");
    let path = dir.join("Counter.mnle");
    let mut p = Project::new("Counter");
    let mut c = Clip::new("c", 0.0, 1.0);
    c.fx.push(fx.clone());
    p.timeline.video_tracks[0].clips.push(c);
    p.save(&path).unwrap();
    let saved = p.timeline.video_tracks[0].clips[0].fx[0].clone();
    assert!(saved.blob.contains_key("seq"), "flattened on save");
    // The flat bytes are the plug-in's own (magic "FLAT", count 3).
    let flat = ofx::proto::unb64(&saved.blob["seq"]);
    assert_eq!((&flat[..4], flat[4]), (&0x464C4154u32.to_le_bytes()[..], 3));
    // The instance keeps running after the flatten (resetup): the next frame is 4.
    assert_eq!(run(&fx, &img, 0.2).pixels[0].r(), 4);
    let loaded = crate::project::Project::load(&path).unwrap();
    let back = loaded.timeline.video_tracks[0].clips[0].fx[0].clone();
    assert_eq!(back.blob, saved.blob);
    ofx::close_hosts();
    lock(&BLOBS).clear();
    note_blob(&back);
    assert_eq!(run(&back, &img, 0.0).pixels[0].r(), 4, "a new host resets up from the saved count (3), then counts");
    // An instance with nothing saved starts from 0.
    assert_eq!(run(&new_fx(COUNTER), &img, 0.0).pixels[0].r(), 1);
    ofx::close_hosts();
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A4: an effect that crashes or hangs in its render is contained: bypassed with a message, the chain goes on.
#[test]
#[ignore]
fn aex_crash_and_hang_are_contained() {
    registered();
    ofx::client::RENDER_TIMEOUT_MS.store(3000, std::sync::atomic::Ordering::Relaxed);
    let _ = ofx::take_events();
    let src = picture();
    let expect = run(&new_fx(INVERT), &src, 0.0);
    for (bad, word) in [(CRASH, "crashed"), (HANG, "did not answer")] {
        let mut img = src.clone();
        for fx in [new_fx(bad), new_fx(INVERT)] {
            vfx::apply(&mut img, &eval_at(&fx, 0.0, 36.0), 1);
        }
        assert!(max_diff(&img, &expect) <= 1, "{bad} is bypassed, the invert still runs");
        assert!(ofx::failure(bad).unwrap().contains(word));
        assert!(ofx::take_events().iter().any(|m| m.contains("AE-format effect") && m.contains("bypassed")));
        assert!(ofx::bypassed_because(bad).unwrap().contains("kept"));
    }
    ofx::client::RENDER_TIMEOUT_MS.store(30_000, std::sync::atomic::Ordering::Relaxed);
    ofx::clear_failures();
    ofx::close_hosts();
}

/// A5 acceptance: a layer parameter set to None checks out transparent; set to this event, the event's own picture.
#[test]
#[ignore]

/// A5 acceptance: a supervised checkbox, turned off, greys its sibling (UPDATE_PARAMS_UI, PF_UpdateParamUI) and sets
/// its value (USER_CHANGED_PARAM with CHANGED_VALUE), which comes back to Milan as an edit of the project.
#[test]
#[ignore]

/// A5 acceptance: an effect that checks out its input one frame earlier gets that frame from Milan (a nested request
/// over the pipe answered from shared memory), and its output is it.
#[test]
#[ignore]

/// A5 acceptance: ten checkouts in one render (nine of them nested requests to Milan) finish well inside the render
/// time-out, in order, and the effect gets each frame.
#[test]
#[ignore]

/// A4 and A5 acceptance, needs ffmpeg: a render runs AE-format effects as the preview does (invert on the event, gain
/// on the track), and an event effect that checks out the previous frame gets the event's previous source frame
/// through the compositor.
#[test]
#[ignore]

// ---- Open-source plug-ins (docs/AE_PREMIERE_PLUGINS.md §3.2) in MILAN_AEX_TEST_DIR. None was available when this
// was written: these tests are unverified, and so is the ABI against third-party binaries (`docs/compat/aex.csv`).

fn open_source_dir() -> PathBuf {
    std::env::var_os("MILAN_AEX_TEST_DIR").map(PathBuf::from).expect("set MILAN_AEX_TEST_DIR to a folder of open-source .aex plug-ins (docs/AE_PREMIERE_PLUGINS.md §3.2)")
}

/// A2: every open-source plug-in found goes through global and parameter setup; prints its parameters (compare names
/// and counts with its source: ntsc-rs, the `after-effects` crate examples `simplest` and `paramarama`) and the
/// suites it asked for that Milan does not serve.
#[test]
#[ignore]
fn open_source_plugins_describe() {
    let cache = scan::scan_all(&[], &[open_source_dir()], &ScanCache::default(), false, &scan::describe_binary);
    assert!(!cache.binaries.is_empty());
    for b in &cache.binaries {
        assert!(b.errors.is_empty(), "{}: {:?}", b.path.display(), b.errors);
        for d in &b.plugins {
            let a = d.aex.as_ref().unwrap();
            println!("{}: {} ({}): {} parameters {:?}; refused {:?}; {}", b.path.display(), d.label, a.match_name, d.params.len(), d.params.iter().map(|p| &p.label).collect::<Vec<_>>(), a.refused, a.unsupported);
            assert!(a.described && a.unsupported.is_empty(), "{a:?}");
        }
    }
}

/// A4: the test that proves the ABI against a third-party binary: ntsc-rs as an AE-format effect and as an OFX plug-in
/// (MILAN_OFX_TEST_DIR), default settings, the same picture, within 2 in 255.
#[test]
#[ignore]
fn ntsc_rs_aex_matches_ntsc_rs_ofx() {
    let ofx_dir = std::env::var_os("MILAN_OFX_TEST_DIR").map(PathBuf::from).expect("set MILAN_OFX_TEST_DIR to ntsc-rs's OFX bundle folder");
    let cache = scan::scan_all(&[ofx_dir], &[open_source_dir()], &ScanCache::default(), false, &scan::describe_binary);
    ofx::set_registry(&cache, &Default::default());
    let all: Vec<&PluginDesc> = cache.binaries.iter().flat_map(|b| &b.plugins).filter(|d| d.label.to_lowercase().contains("ntsc")).collect();
    let ae = all.iter().find(|d| d.aex.is_some()).expect("ntsc-rs .aex");
    let ofx_d = all.iter().find(|d| d.aex.is_none()).expect("ntsc-rs OFX");
    let src = picture();
    let a = run(&new_fx(&ofx::id_of(ae)), &src, 0.0);
    let o = run(&new_fx(&ofx::id_of(ofx_d)), &src, 0.0);
    assert!(max_diff(&a, &o) <= 2, "{}", max_diff(&a, &o));
    ofx::close_hosts();
}

/// A5: the `after-effects` crate examples `checkout` and `supervisor`, built by their authors' instructions, render.
#[test]
#[ignore]
