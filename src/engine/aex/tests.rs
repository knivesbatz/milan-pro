use super::*;
use crate::engine::ofx::{self, ScanCache, Status, scan};
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
    let cache = scan::scan_all(&[], &[d.clone()], &ScanCache::default(), false, &scan::describe_binary);
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
