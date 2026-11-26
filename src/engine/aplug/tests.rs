//! R34a/R34b/R35 tests. The ones marked `#[ignore]` need the built host (`cargo build --release`) and build the test
//! plug-ins (tools/vst3-test-plugin, tools/clap-test-plugin); run them one at a time:
//! cargo test --release -- --ignored --test-threads=1 aplug::
use super::*;
use crate::engine::ofx::proto::EditorOp;
use crate::engine::ofx::scan;
use crate::project::{Curve, Keyframe, Keyframes};
use std::sync::OnceLock;

fn desc() -> AudioDesc {
    AudioDesc {
        format: "vst3".into(),
        id: "ABCDEF0123456789ABCDEF0123456789".into(),
        name: "Thing".into(),
        vendor: "Acme".into(),
        params: vec![
            AParam { id: 7, name: "Gain".into(), max: 1.0, default: 0.5, automatable: true, ..Default::default() },
            AParam { id: 8, name: "Mode".into(), steps: 2, max: 1.0, options: vec!["A".into(), "B".into(), "C".into()], ..Default::default() },
            AParam { id: 9, name: "On".into(), steps: 1, max: 1.0, default: 1.0, ..Default::default() },
            AParam { id: 10, name: "Secret".into(), max: 1.0, hidden: true, ..Default::default() },
            AParam { id: 11, name: "Bypass".into(), steps: 1, max: 1.0, bypass: true, ..Default::default() },
        ],
        latency: 64,
        ..Default::default()
    }
}

/// VST3 values are normalized, stepped ones step indices; hidden and bypass parameters are not controls; CLAP values
/// are plain.
#[test]
fn parameters_map_both_ways() {
    let d = desc();
    let sp = specs(&d);
    assert_eq!(sp.iter().map(|s| s.1.key).collect::<Vec<_>>(), ["p7", "p8", "p9"]);
    assert!(matches!(sp[0].1.kind, ParamKind::Float { min, max, default } if min == 0.0 && max == 1.0 && default == 0.5));
    assert!(matches!(sp[1].1.kind, ParamKind::Choice { options, default: 0 } if options == ["A", "B", "C"]));
    assert!(matches!(sp[2].1.kind, ParamKind::Bool { default: true }));
    assert_eq!((to_milan(&d.params[1], 0.5), to_plug(&d.params[1], 2.0)), (1.0, 1.0));
    assert_eq!((to_milan(&d.params[2], 1.0), to_plug(&d.params[2], 0.0)), (1.0, 0.0));
    let clap = AParam { id: 1, steps: 3, min: 2.0, max: 5.0, options: vec!["2".into(), "3".into(), "4".into(), "5".into()], ..Default::default() };
    assert_eq!((to_milan(&clap, 4.0), to_plug(&clap, 3.0)), (2.0, 5.0));
    let plain = AParam { id: 2, min: -24.0, max: 24.0, default: 0.0, ..Default::default() };
    assert_eq!((to_milan(&plain, -6.0), to_plug(&plain, 30.0)), (-6.0, 24.0));
    // The registry entry.
    let cache = ScanCache { version: 2, binaries: vec![scan::Binary { path: PathBuf::from(r"C:\x\Thing.vst3"), audio: vec![d.clone()], ..Default::default() }] };
    set_registry(&cache, &OfxPrefs::default());
    let id = id_of(&d);
    assert_eq!(id, "vst3:ABCDEF0123456789ABCDEF0123456789");
    let info = crate::engine::plugins::find(&id).expect("registered");
    assert_eq!((info.name, info.group, info.kind), ("Thing", "Acme", PluginKind::AudioFx));
    assert!(crate::engine::plugins::catalog(PluginKind::AudioFx).any(|i| i.id == id));
    assert_eq!(latency(&id), 64);
    assert_eq!(entries().len(), 1);
    assert!(!is_bypassed(&id) && is_bypassed("clap:nope") && !is_bypassed("milan.afx.eq"));
    assert!(bypassed_because(&id).is_none());
    assert!(bypassed_because("clap:com.example.gone").unwrap().contains("not installed"));
    let mut off = OfxPrefs::default();
    off.disabled.insert(id.clone());
    set_registry(&cache, &off);
    assert!(crate::engine::plugins::find(&id).is_none() && label(&id) == Some("Thing (off)".into()) && bypassed_because(&id).unwrap().contains("turned off"));
    set_registry(&ScanCache::default(), &OfxPrefs::default());
    assert_eq!(label(&id), Some("ABCDEF0123456789ABCDEF0123456789 (missing)".into()));
}

// ---- With the plug-in host and the test plug-ins. Run after `cargo build --release`.

/// The test modules, built once, in a folder of their own under %TEMP%\milan-agent\r34\plugins.
fn test_modules() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        assert!(client::host_exe().is_some(), "build milan-plugin-host first: cargo build --release");
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let build = |tool: &str| {
            let target = repo.join("target").join(tool);
            let mut cmd = std::process::Command::new(&cargo);
            cmd.args(["build", "--release", "--offline", "--quiet", "--manifest-path"]).arg(repo.join("tools").join(tool).join("Cargo.toml")).arg("--target-dir").arg(&target);
            let st = crate::proc::output(&mut cmd).expect("cargo runs");
            assert!(st.status.success(), "{}", String::from_utf8_lossy(&st.stderr));
            target.join("release")
        };
        let vst3 = build("vst3-test-plugin").join("milan_vst3_test.dll");
        let clap = build("clap-test-plugin").join("milan_clap_test.dll");
        let place = |dir: PathBuf| -> std::io::Result<PathBuf> {
            let bin = dir.join("MilanTest.vst3").join("Contents").join("x86_64-win");
            std::fs::create_dir_all(&bin)?;
            std::fs::copy(&vst3, bin.join("MilanTest.vst3"))?;
            let res = dir.join("MilanTest.vst3").join("Contents").join("Resources");
            std::fs::create_dir_all(&res)?;
            std::fs::write(res.join("moduleinfo.json"), MODULEINFO)?;
            std::fs::copy(&clap, dir.join("MilanTestGain.clap"))?;
            Ok(dir)
        };
        let base = std::env::temp_dir().join("milan-agent").join("r34").join("plugins");
        place(base.clone()).or_else(|_| place(base.join(format!("run-{}", std::process::id())))).unwrap()
    })
    .clone()
}

/// What the VST3 SDK's moduleinfotool would write for the test module (its class ids as the host spells them).
const MODULEINFO: &str = r#"{
  "Name": "MilanTest",
  "Version": "1.0.0",
  "Factory Info": {
    "Vendor": "Milan Test",
    "URL": "https://github.com/MeridianNLE",
    "E-Mail": "",
    "Flags": {
      "Unicode": true,
      "Classes Discardable": false,
      "Component Non Discardable": false,
    },
  },
  "Classes": [
    {
      "CID": "614C694D546E7365744761696E000001",
      "Category": "Audio Module Class",
      "Name": "Milan Test Gain",
      "Vendor": "Milan Test",
      "Version": "1.0.0",
      "SDKVersion": "VST 3.7.9",
      "Sub Categories": [
        "Fx",
        "Tools",
      ],
      "Class Flags": 1,
      "Cardinality": 2147483647,
      "Snapshots": [],
    },
  ],
}
"#;

fn registered() -> ScanCache {
    let cache = scan::scan(&[test_modules()], &ScanCache::default(), false, &scan::describe_binary);
    crate::engine::ofx::set_registry(&cache, &Default::default());
    cache
}

/// The registered test effects: (gain, crash, hang, clap gain).
fn ids() -> [String; 4] {
    let find = |name: &str| entries().into_iter().find(|e| e.name == name).map(|e| e.id).unwrap_or_else(|| panic!("{name} is not registered: {:?}", entries()));
    [find("Milan Test Gain"), find("Milan Test Crash"), find("Milan Test Hang"), find("Milan Test CLAP Gain")]
}

fn sine(hz: f64, amp: f32, n: usize) -> Vec<f32> {
    (0..n).map(|k| amp * (std::f64::consts::TAU * hz * k as f64 / crate::audio::RATE as f64).sin() as f32).collect()
}

fn stereo(v: &[f32]) -> Vec<f32> {
    v.iter().flat_map(|&x| [x, x]).collect()
}

fn left(v: &[f32]) -> Vec<f32> {
    v.iter().step_by(2).copied().collect()
}

fn fx_of(id: &str, vals: &[(&str, f32)]) -> Fx {
    let mut f = crate::engine::plugins::new_fx(crate::engine::plugins::find(id).expect("registered"), None);
    for &(k, v) in vals {
        f.params.insert(k.into(), Keyframes { keys: vec![Keyframe { t: 0.0, v, curve: Curve::Linear }] });
    }
    f
}

/// R34a acceptance: the test module is scanned through the host (its three effects, their parameters, the program
/// list, the vendor grouping, moduleinfo.json agreeing) and the CLAP module too.
#[test]
#[ignore]
fn vst3_and_clap_modules_are_scanned() {
    let cache = registered();
    let vst3 = cache.binaries.iter().find(|b| scan::kind_of(&b.path) == "vst3").expect("the VST3 module");
    assert!(vst3.errors.is_empty(), "{:?}", vst3.errors);
    assert_eq!(vst3.audio.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["Milan Test Gain", "Milan Test Crash", "Milan Test Hang"]);
    let gain = &vst3.audio[0];
    assert_eq!((gain.vendor.as_str(), gain.category.as_str(), gain.latency, gain.has_editor, gain.error.as_str()), ("Milan Test", "Fx|Tools", 64, true, ""));
    assert_eq!(gain.params.iter().map(|p| (p.id, p.name.as_str(), p.steps)).collect::<Vec<_>>(), [(0, "Gain", 0), (1, "Wet", 0), (2, "Program", 2)]);
    assert_eq!(gain.params[2].options, ["Unity", "Half", "Double"]);
    assert!(gain.params[2].program);
    let clap = cache.binaries.iter().find(|b| scan::kind_of(&b.path) == "clap").expect("the CLAP module");
    assert!(clap.errors.is_empty(), "{:?}", clap.errors);
    assert_eq!(clap.audio.len(), 1);
    let cg = &clap.audio[0];
    assert_eq!((cg.name.as_str(), cg.vendor.as_str(), cg.latency, cg.error.as_str()), ("Milan Test CLAP Gain", "Milan Test", 32, ""));
    assert_eq!(cg.params.iter().map(|p| (p.id, p.name.as_str(), p.min, p.max, p.default)).collect::<Vec<_>>(), [(0, "Gain", 0.0, 2.0, 1.0), (1, "Mute", 0.0, 1.0, 0.0)]);
    let [g, c, h, cl] = ids();
    assert!(g.starts_with(VST3) && c.starts_with(VST3) && h.starts_with(VST3) && cl.starts_with(CLAP));
    let info = crate::engine::plugins::find(&g).unwrap();
    assert_eq!((info.group, info.kind), ("Milan Test", PluginKind::AudioFx));
    assert!(matches!(info.params[2].kind, ParamKind::Choice { options, .. } if options == ["Unity", "Half", "Double"]));
    assert!(crate::engine::plugins::catalog(PluginKind::AudioFx).filter(|i| is_ext(i.id)).count() >= 4);
    assert_eq!(latency(&g), 64);
    assert_eq!(latency(&cl), 32);
    close_hosts();
}

/// R34a acceptance: the VST3 test gain applies its gain to a sine within 1e-5 (wet 0: no delay); the chain keeps the
/// instance across blocks; a parameter change is heard in the next block.
#[test]
#[ignore]
fn vst3_gain_applies_to_a_sine() {
    registered();
    let [g, ..] = ids();
    // Gain 0.75 (normalized) is 1.5x; Wet 0 means the dry path.
    let fx = fx_of(&g, &[("p0", 0.75), ("p1", 0.0)]);
    let x = sine(440.0, 0.4, 4096);
    let mut buf = stereo(&x);
    let mut chain = crate::engine::afx::Chain::default();
    for (k, b) in buf.chunks_mut(2 * BLOCK).enumerate() {
        chain.run(std::slice::from_ref(&fx), k as f64 * BLOCK as f64 / 48_000.0, b);
    }
    assert!(failure(&g).is_none(), "{:?}", failure(&g));
    let y = left(&buf);
    let err = x.iter().zip(&y).map(|(a, b)| (a * 1.5 - b).abs()).fold(0.0f32, f32::max);
    assert!(err < 1e-5, "gain error {err}");
    // Now 2x, and the right channel follows too.
    let fx2 = fx_of(&g, &[("p0", 1.0), ("p1", 0.0)]);
    let mut b2 = stereo(&x[..BLOCK]);
    chain.run(std::slice::from_ref(&fx2), 1.0, &mut b2);
    assert!(b2.iter().zip(stereo(&x[..BLOCK])).all(|(a, b)| (a - b * 2.0).abs() < 1e-5));
    close_hosts();
}

/// R34a acceptance: the 64-sample-latency plug-in (Wet 1: the delayed path) on one track is aligned with a dry track
/// sample-exactly by plug-in delay compensation, in the mixer; a CLAP effect (32 samples) on a third track too.
#[test]
#[ignore]
fn pdc_aligns_a_latent_plug_in_with_a_dry_track() {
    use crate::engine::mix::tests::{fake_mixer, mix_frames, two_tracks};
    registered();
    let [g, _, _, cl] = ids();
    let (mut p, wet) = two_tracks();
    let n = 4 * BLOCK;
    // Track 1 gets the VST3 delay (gain 1x: p0 0.5, fully wet), track 2 stays dry, track 3 the CLAP delay (gain 1).
    p.timeline.audio_tracks[0].fx = vec![fx_of(&g, &[("p0", 0.5), ("p1", 1.0)])];
    p.timeline.audio_tracks[2].fx = vec![fx_of(&cl, &[("p0", 1.0)])];
    let plan = crate::engine::plan::compile_audio(&p);
    assert_eq!(crate::engine::mix::tests::plan_latency(&plan), 64, "the plan's total latency is the VST3 delay's");
    let mut m = fake_mixer(wet);
    let out = mix_frames(&plan, &mut m, 0, n);
    assert!(failure(&g).is_none() && failure(&cl).is_none(), "{:?} {:?}", failure(&g), failure(&cl));
    // The dry track alone is wet(k); every track plays the same sawtooth, so an aligned sum is 3 * wet(k).
    let want: Vec<f32> = (0..n as i64).map(|k| 3.0 * wet(k)).collect();
    let got = left(&out);
    let first_bad = got.iter().zip(&want).position(|(a, b)| (a - b).abs() > 1e-5);
    assert_eq!(first_bad, None, "misaligned at {first_bad:?}: got {:?}, want {:?}", &got[first_bad.unwrap_or(0)..(first_bad.unwrap_or(0) + 4).min(n)], &want[first_bad.unwrap_or(0)..(first_bad.unwrap_or(0) + 4).min(n)]);
    close_hosts();
}

/// R34b acceptance: a `.vstpreset` written from one instance restores the parameters in another; the state
/// round-trips through the project's text keys; a program change (the program list) sets the plug-in's values and
/// comes back as the plug-in's own edit; a preset of another class is refused.
#[test]
#[ignore]
fn preset_round_trip_restores_parameters() {
    registered();
    let [g, ..] = ids();
    let dir = scan::tests::temp("preset");
    let fx = fx_of(&g, &[("p0", 0.9), ("p1", 0.25)]);
    let file = dir.join("mine.vstpreset");
    save_preset(&fx, &file).unwrap();
    let bytes = std::fs::read(&file).unwrap();
    let pre = preset::decode(&bytes).unwrap();
    assert!(pre.class_id.eq_ignore_ascii_case(&g[VST3.len()..]) && pre.component.len() == 20);
    assert_eq!(pre.component[..8], (0.9f32 as f64).to_le_bytes(), "the values Milan set reached the processor before its state was read");
    let _ = take_edits();
    let other = fx_of(&g, &[]);
    load_preset(&other, &file).unwrap();
    let ed = take_edits().into_iter().find(|e| e.instance == other.id).expect("an edit with the values");
    let v = |k: &str| ed.values.iter().rev().find(|x| x.0 == k).map(|x| x.1);
    assert_eq!((v("p0"), v("p1"), v("p2")), (Some(0.9), Some(0.25), Some(0.0)));
    // Program 3, "Double", sets the gain to 1.0 (normalized) in the controller (reported as the plug-in's own edit) and
    // in the processor (its state).
    let prog = fx_of(&g, &[("p2", 2.0)]);
    let st = fetch_state(&prog).unwrap();
    let bytes = unb64(&st[STATE_KEY]);
    assert_eq!((bytes[..8].to_vec(), bytes[16..20].to_vec()), (1.0f64.to_le_bytes().to_vec(), 2i32.to_le_bytes().to_vec()));
    // (The controller's edit comes with the next reply that carries edits: a poll, as the FX window makes.)
    editor(&prog, EditorOp::Poll).unwrap();
    let edits = take_edits();
    assert!(edits.iter().any(|e| e.instance == prog.id && e.values.contains(&("p0".to_string(), 1.0))), "{edits:?}");
    let (state, cstate) = ed.state.clone().expect("the state travels with it");
    assert_eq!(unb64(&state).len(), 20);
    assert!(cstate.is_empty());
    // The project keeps the state: a third instance made from it starts there.
    let mut third = fx_of(&g, &[]);
    third.text.insert(STATE_KEY.into(), state);
    let st = fetch_state(&third).unwrap();
    assert_eq!(unb64(&st[STATE_KEY])[..8], (0.9f32 as f64).to_le_bytes(), "a parameter without a stored value stays as the state has it");
    let bad = fx_of(&ids()[1], &[]);
    assert!(load_preset(&bad, &file).unwrap_err().contains("another plug-in"));
    close_hosts();
    std::fs::remove_dir_all(&dir).unwrap();
}

/// R32/R34a acceptance: a crashing and a hanging VST3 effect are contained: the host is killed, the effect is
/// bypassed with a message, and the rest of the chain (the gain of the same module, in a new host) keeps running.
#[test]
#[ignore]
fn crash_and_hang_are_contained() {
    registered();
    client::RENDER_TIMEOUT_MS.store(3000, Ordering::Relaxed);
    let _ = take_events();
    let [g, c, h, _] = ids();
    let x = sine(220.0, 0.5, BLOCK);
    for (bad, word) in [(&c, "crashed"), (&h, "did not answer")] {
        let started = std::time::Instant::now();
        let mut buf = stereo(&x);
        let chain_fx = [fx_of(bad, &[]), fx_of(&g, &[("p0", 1.0), ("p1", 0.0)])];
        let mut chain = crate::engine::afx::Chain::default();
        chain.run(&chain_fx, 0.0, &mut buf);
        let why = failure(bad).expect("recorded");
        assert!(why.contains(word), "{why}");
        assert!(take_events().iter().any(|m| m.contains("bypassed")));
        assert!(bypassed_because(bad).unwrap().contains("kept"));
        assert!(buf.iter().zip(stereo(&x)).all(|(a, b)| (a - b * 2.0).abs() < 1e-5), "{bad} is bypassed, the gain still runs (in a new host)");
        assert!(started.elapsed() < std::time::Duration::from_secs(20));
        // Bypassed from now on: no new host, no wait.
        let t = std::time::Instant::now();
        let mut again = stereo(&x);
        chain.run(&chain_fx[..1], 1.0, &mut again);
        assert_eq!(again, stereo(&x));
        assert!(t.elapsed() < std::time::Duration::from_millis(200));
    }
    client::RENDER_TIMEOUT_MS.store(30_000, Ordering::Relaxed);
    clear_failures();
    close_hosts();
}

/// R35 acceptance: the CLAP test plug-in processes audio (gain 1.5 after its 32-sample delay) and keeps its state.
#[test]
#[ignore]
fn clap_plug_in_processes_audio() {
    registered();
    let [_, _, _, cl] = ids();
    let fx = fx_of(&cl, &[("p0", 1.5)]);
    let x = sine(1000.0, 0.3, 2 * BLOCK);
    let mut buf = stereo(&x);
    let mut chain = crate::engine::afx::Chain::default();
    for (k, b) in buf.chunks_mut(2 * BLOCK).enumerate() {
        chain.run(std::slice::from_ref(&fx), k as f64 * BLOCK as f64 / 48_000.0, b);
    }
    assert!(failure(&cl).is_none(), "{:?}", failure(&cl));
    let y = left(&buf);
    assert!(y[..32].iter().all(|v| *v == 0.0), "the first 32 samples are the delay line");
    let err = (32..y.len()).map(|k| (y[k] - 1.5 * x[k - 32]).abs()).fold(0.0f32, f32::max);
    assert!(err < 1e-5, "{err}");
    // Mute is a stepped parameter; the state holds both values.
    let muted = fx_of(&cl, &[("p0", 1.5), ("p1", 1.0)]);
    let mut b2 = stereo(&x[..BLOCK]);
    chain.run(std::slice::from_ref(&muted), 1.0, &mut b2);
    assert!(b2.iter().all(|v| *v == 0.0));
    let st = fetch_state(&muted).unwrap();
    assert_eq!(unb64(&st[STATE_KEY]).len(), 16);
    close_hosts();
}

/// R34b acceptance: the plug-in's editor opens in a host-owned window (off-screen, inactive), reports its size, is
/// polled, and closes with the host.
#[test]
#[ignore]
fn editor_opens_off_screen_and_closes_with_the_host() {
    registered();
    let [g, ..] = ids();
    let fx = fx_of(&g, &[]);
    let st = editor(&fx, EditorOp::Open { x: 100, y: 100, offscreen: true, title: "Milan Test Gain".into() }).unwrap();
    assert!(st.open && st.w == 320 && st.h == 200 && st.hwnd != 0, "{st:?}");
    assert_eq!(editor_state(&fx.id).map(|e| e.open), Some(true));
    // The window exists in the host process and is not the foreground window.
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsWindow};
        assert_ne!(IsWindow(st.hwnd as usize as _), 0);
        assert_ne!(GetForegroundWindow() as usize as u64, st.hwnd);
    }
    let polled = editor(&fx, EditorOp::Poll).unwrap();
    assert!(polled.open);
    // Closing the hosts ends the window with them.
    close_hosts();
    std::thread::sleep(std::time::Duration::from_millis(300));
    #[cfg(windows)]
    unsafe {
        assert_eq!(windows_sys::Win32::UI::WindowsAndMessaging::IsWindow(st.hwnd as usize as _), 0, "the editor window died with its host");
    }
    assert!(editor_state(&fx.id).is_none());
    // Open again, and close it explicitly: the state comes back as an edit.
    let st = editor(&fx, EditorOp::Open { x: 0, y: 0, offscreen: true, title: "x".into() }).unwrap();
    assert!(st.open);
    let _ = take_edits();
    let closed = editor(&fx, EditorOp::Close).unwrap();
    assert!(!closed.open);
    assert!(take_edits().iter().any(|e| e.instance == fx.id && e.state.is_some()));
    close_hosts();
}

/// R33b/R34a acceptance, needs ffmpeg: a render runs VST3 effects as the preview does. A project with the VST3 gain
/// on its audio track renders (PCM WAV) to exactly the mixer's blocks.
#[test]
#[ignore]
fn render_matches_preview_with_a_vst3_fx() {
    use crate::engine::mix::{BLOCK as MB, Mixer};
    use crate::project::{Clip, Project};
    registered();
    let [g, ..] = ids();
    let dir = scan::tests::temp("render-vst3");
    let src = dir.join("tone.wav");
    let ok = crate::proc::ffmpeg().args(["-v", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=1", "-ac", "2", "-c:a", "pcm_f32le"]).arg(&src).status().unwrap().success();
    assert!(ok);
    let m = crate::media::probe_media(&src).unwrap();
    let mut p = Project::new("vst3");
    let mut c = Clip::from_media(&m, 0.0);
    (c.duration, c.out_point) = (1.0, c.in_point + 1.0);
    p.timeline.audio_tracks[0].clips.push(c);
    p.timeline.audio_tracks[0].fx.push(fx_of(&g, &[("p0", 0.75), ("p1", 1.0)]));
    p.media_pool = vec![m];
    let t = crate::render::BUILTIN.iter().find(|t| t.name.contains("WAV") || t.name.contains("Wave")).unwrap_or_else(|| panic!("a WAV template: {:?}", crate::render::BUILTIN.iter().map(|t| t.name.clone()).collect::<Vec<_>>()));
    let out = dir.join("out.wav");
    let job = crate::engine::render::start(&p, t, out.clone(), (0.0, 1.0), Default::default(), eframe::egui::Context::default());
    while job.status() == crate::render::Status::Running {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(job.status(), crate::render::Status::Done);
    // The preview: the same plan through a mixer.
    // The render's default options quickfade the edges (10 ms): the preview plan the same, as playback does with the option on.
    let plan = std::sync::Arc::new(crate::engine::plan::AudioPlan { quickfade: if crate::render::Opts::default().quickfade { crate::engine::plan::QUICKFADE } else { 0.0 }, ..crate::engine::plan::compile_audio(&p) });
    let mut mixer = Mixer::ffmpeg();
    let mut want = vec![0f32; 2 * 48_000];
    for (i, b) in want.chunks_mut(2 * MB).enumerate() {
        mixer.block(&plan, (i * MB) as i64, b);
    }
    let mut cmd = crate::proc::ffmpeg();
    cmd.args(["-v", "error", "-i"]).arg(&out).args(["-f", "f32le", "-ac", "2", "-ar", "48000", "-"]);
    let got = crate::proc::output(&mut cmd).unwrap().stdout;
    let got: &[f32] = bytemuck::cast_slice(&got[..got.len() / 4 * 4]);
    assert!(got.len() >= want.len() - 2 * MB, "{} vs {}", got.len(), want.len());
    let n = got.len().min(want.len());
    let diff = got[..n].iter().zip(&want[..n]).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    let first = got[..n].iter().zip(&want[..n]).position(|(a, b)| (a - b).abs() > 2e-4);
    assert!(diff < 2e-4, "the render differs from the preview by {diff}, first at {first:?} of {n}: got {:?} want {:?}", first.map(|i| &got[i..(i + 6).min(n)]), first.map(|i| &want[i..(i + 6).min(n)]));
    // And something is heard (the whole tone goes through the delayed, compensated path).
    let peak = want.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(peak > 0.05, "peak {peak}");
    assert!(failure(&g).is_none());
    drop(mixer);
    close_hosts();
    std::fs::remove_dir_all(&dir).unwrap();
}

/// R34a: the VST3 process overhead per 1024-frame stereo block against the built-in gain (`docs/PERFORMANCE.md`).
#[test]
#[ignore]
fn bench_vst3_block_overhead() {
    registered();
    let [g, ..] = ids();
    let fx = fx_of(&g, &[("p0", 0.75), ("p1", 0.0)]);
    let builtin = crate::engine::afx::test_fx("milan.afx.compressor", &[("ratio", 1.0), ("output", 3.5)]);
    let x = stereo(&sine(440.0, 0.4, BLOCK));
    let time = |fx: &Fx| {
        let mut chain = crate::engine::afx::Chain::default();
        let mut buf = x.clone();
        for _ in 0..20 {
            chain.run(std::slice::from_ref(fx), 0.0, &mut buf);
        }
        let n = 500;
        let t = std::time::Instant::now();
        for _ in 0..n {
            buf.copy_from_slice(&x);
            chain.run(std::slice::from_ref(fx), 0.0, &mut buf);
        }
        t.elapsed().as_secs_f64() * 1e3 / n as f64
    };
    let (ext, bi) = (time(&fx), time(&builtin));
    assert!(failure(&g).is_none());
    let over = ext - bi;
    println!("BENCH vst3_block ext_ms={ext:.4} builtin_ms={bi:.4} overhead_ms={over:.4} gate_ms=0.2 pass={}", over < 0.2);
    assert!(over < 0.2, "VST3 block overhead {over:.3} ms exceeds 0.2 ms");
    close_hosts();
}

/// R35 research spike: the DMO findings, written to docs/compat/dx-audio.md by hand from this output.
#[test]
#[ignore]
fn dmo_probe_runs_in_the_host() {
    let bin = test_modules().join("MilanTestGain.clap");
    let lines = dmo_probe(&bin).unwrap();
    for l in &lines {
        println!("DMO {l}");
    }
    assert!(lines.iter().any(|l| l.contains("DMO audio effects registered")), "{lines:?}");
}
