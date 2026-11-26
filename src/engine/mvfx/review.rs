//! Review material: contact sheets of the MVFX looks, optics, temporal FX, transitions (at p = 0.25, 0.5 and 0.75)
//! and FX Packages, each rendered through the real render path (a PNG sequence from `engine::render`, titles by
//! Milan's own Titles & Text) on FFmpeg-generated footage, into %TEMP%\milan-review\mvfx\. Needs ffmpeg:
//! `cargo test --release -- --ignored mvfx_contact_sheets` (`MILAN_TEST_GPU=1` renders on the GPU).
use eframe::egui::{Color32, ColorImage};
use std::path::{Path, PathBuf};
use crate::engine::plugins::{PluginKind, find, new_fx};
use crate::project::{Clip, MediaItem, Project, Track, TrackKind};
use crate::render::{BUILTIN, Template};

const W: u32 = 480;
const H: u32 = 270;
const FPS: f64 = 24.0;

fn footage(dir: &Path, name: &str, src: &str) -> MediaItem {
    let path = dir.join(name);
    if !path.exists() {
        let ok = crate::proc::ffmpeg().args(["-v", "error", "-y", "-f", "lavfi", "-i", src]).args(["-c:v", "libx264", "-qp", "0", "-pix_fmt", "yuv444p"]).arg(&path).status().map(|s| s.success()).unwrap_or(false);
        assert!(ok, "ffmpeg made {name}");
    }
    crate::media::probe_media(&path).expect("probed")
}

fn clip(m: &MediaItem, at: f64, len: f64, from: f64) -> Clip {
    let mut c = Clip::from_media(m, at);
    (c.in_point, c.duration, c.out_point) = (from, len, from + len);
    c
}

fn project() -> Project {
    let mut p = Project::new("mvfx review");
    (p.settings.width, p.settings.height, p.settings.frame_rate) = (W, H, FPS);
    p.timeline.video_tracks = vec![Track::new("Titles", TrackKind::Video), Track::new("Video", TrackKind::Video)];
    p
}

/// A caption on the title track over `at..at + len`.
fn caption(p: &mut Project, at: f64, len: f64, text: &str) {
    let info = find("milan.gen.text").expect("built in");
    let mut fx = new_fx(info, info.presets.iter().find(|pr| pr.name == "Subtitle"));
    fx.text.insert("text".into(), text.into());
    let key = |v| crate::project::Keyframes { keys: vec![crate::project::Keyframe { t: 0.0, v, curve: crate::project::Curve::Linear }] };
    fx.params.insert("size".into(), key(0.075));
    fx.params.insert("shadow".into(), key(1.0));
    crate::edit::insert_generated(p, Some(0), at, fx, text, len).expect("a caption");
}

/// The project rendered as a PNG sequence over `0..end`, its frames by number.
fn render(p: &Project, dir: &Path, stem: &str, end: f64) -> Vec<PathBuf> {
    let t: &Template = BUILTIN.iter().find(|t| t.name == "PNG sequence").expect("a PNG sequence template");
    let out = dir.join(format!("{stem}_000000.png"));
    let job = crate::engine::render::start(p, t, out, (0.0, end), Default::default(), eframe::egui::Context::default());
    while job.status() == crate::render::Status::Running {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(job.status(), crate::render::Status::Done, "{stem} rendered");
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).unwrap().filter_map(Result::ok).map(|e| e.path()).filter(|f| f.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&format!("{stem}_")))).collect();
    v.sort();
    v
}

fn load(path: &Path) -> ColorImage {
    let d = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
    let mut r = d.read_info().unwrap();
    let mut buf = vec![0; r.output_buffer_size().expect("a PNG size")];
    let info = r.next_frame(&mut buf).unwrap();
    let (w, h) = (info.width as usize, info.height as usize);
    let ch = info.line_size / w;
    ColorImage { size: [w, h], pixels: (0..w * h).map(|i| Color32::from_rgb(buf[i * ch], buf[i * ch + 1], buf[i * ch + 2])).collect() }
}

/// Tiles in rows of `cols`, 6 px apart on dark grey.
fn sheet(tiles: &[ColorImage], cols: usize) -> ColorImage {
    let [tw, th] = tiles[0].size;
    let rows = tiles.len().div_ceil(cols);
    let (w, h) = (6 + cols * (tw + 6), 6 + rows * (th + 6));
    let mut out = ColorImage::new([w, h], Color32::from_gray(40));
    for (i, t) in tiles.iter().enumerate() {
        let (ox, oy) = (6 + (i % cols) * (tw + 6), 6 + (i / cols) * (th + 6));
        for y in 0..th {
            out.pixels[(oy + y) * w + ox..(oy + y) * w + ox + tw].copy_from_slice(&t.pixels[y * tw..(y + 1) * tw]);
        }
    }
    out
}

fn frame_at(frames: &[PathBuf], t: f64) -> ColorImage {
    load(&frames[((t * FPS).round() as usize).min(frames.len() - 1)])
}

/// Each FX (plug-in, preset) on its own `seg`-second event of `src`; the frames at `picks` seconds into each.
fn fx_sheet(dir: &Path, stem: &str, src: &MediaItem, items: &[(&str, &str)], seg: f64, picks: &[f64], cols: usize) -> PathBuf {
    let mut p = project();
    p.media_pool.push(src.clone());
    for (i, (id, preset)) in items.iter().enumerate() {
        let info = find(id).unwrap_or_else(|| panic!("{id}"));
        let at = i as f64 * seg;
        let mut c = clip(src, at, seg, at % 8.0);
        c.fx = vec![new_fx(info, info.presets.iter().find(|p| p.name == *preset))];
        p.timeline.video_tracks[1].clips.push(c);
        caption(&mut p, at, seg, &if *preset == "Default" { info.name.to_string() } else { format!("{} / {preset}", info.name) });
    }
    let frames = render(&p, dir, stem, items.len() as f64 * seg);
    let tiles: Vec<ColorImage> = (0..items.len()).flat_map(|i| picks.iter().map(move |t| i as f64 * seg + t)).map(|t| frame_at(&frames, t)).collect();
    let out = dir.join(format!("{stem}.png"));
    crate::demo::save_png(&out, &sheet(&tiles, cols)).unwrap();
    out
}

/// The plug-ins of `set` with their most telling preset (the second, else Default).
fn pick(set: &'static [crate::engine::plugins::PluginInfo], kind: PluginKind) -> Vec<(&'static str, &'static str)> {
    set.iter().filter(|i| i.kind == kind).map(|i| (i.id, i.presets.get(1).unwrap_or(&i.presets[0]).name)).collect()
}

#[test]
#[ignore]
fn mvfx_contact_sheets() {
    let dir = std::env::temp_dir().join("milan-review").join("mvfx");
    let work = std::env::temp_dir().join("milan-agent").join("mvfx").join("sheets");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let a = footage(&work, "a.mp4", "testsrc2=s=480x270:r=24:d=12");
    let b = footage(&work, "b.mp4", "mandelbrot=s=480x270:r=24,trim=duration=12");
    let mut made = Vec::new();
    // Looks: every look at its era preset.
    let looks = pick(super::looks::PLUGINS, PluginKind::VideoFx);
    made.push(fx_sheet(&work, "looks", &a, &looks, 1.0, &[0.5], 5));
    // Optics and motion: each lens and blur at two presets, the shake at three moments.
    let optics: Vec<(&str, &str)> = super::optics::PLUGINS.iter().flat_map(|i| i.presets.iter().take(3).skip(1).map(move |p| (i.id, p.name))).collect();
    made.push(fx_sheet(&work, "optics", &a, &optics, 1.0, &[0.5], 4));
    // Temporal: four consecutive moments of each (a quarter second apart).
    let temporal: Vec<(&str, &str)> = super::temporal::PLUGINS.iter().flat_map(|i| i.presets.iter().skip(1).take(2).map(move |p| (i.id, p.name))).chain([("milan.fx.camera_shake", "Beat Hit 120 BPM"), ("milan.fx.glitch", "Every Frame")]).collect();
    made.push(fx_sheet(&work, "temporal", &a, &temporal, 2.0, &[0.25, 0.5, 0.75, 1.0], 4));
    // Transitions at p = 0.25, 0.5 and 0.75: A for 1.5 s, B from 0.5 s in, a 1 s overlap.
    {
        let mut p = project();
        p.media_pool = vec![a.clone(), b.clone()];
        let trs: Vec<&crate::engine::plugins::PluginInfo> = super::trans::PLUGINS.iter().collect();
        for (i, info) in trs.iter().enumerate() {
            let at = i as f64 * 2.0;
            let mut cb = clip(&b, at + 0.5, 1.5, (at % 8.0) + 0.5);
            cb.transition = Some(crate::edit::transition_of(&new_fx(info, None)));
            p.timeline.video_tracks[1].clips.push(clip(&a, at, 1.5, at % 8.0));
            p.timeline.video_tracks[1].clips.push(cb);
            caption(&mut p, at, 2.0, info.name);
        }
        let frames = render(&p, &work, "transitions", trs.len() as f64 * 2.0);
        let tiles: Vec<ColorImage> = (0..trs.len()).flat_map(|i| [0.75, 1.0, 1.25].map(|t| frame_at(&frames, i as f64 * 2.0 + t))).collect();
        let out = work.join("transitions.png");
        crate::demo::save_png(&out, &sheet(&tiles, 3)).unwrap();
        made.push(out);
    }
    // FX Packages: each built-in package on its own event.
    {
        let mut p = project();
        p.media_pool.push(a.clone());
        let pk = super::packages::builtins();
        for (i, k) in pk.iter().enumerate() {
            let at = i as f64;
            let mut c = clip(&a, at, 1.0, at % 8.0);
            c.fx = super::packages::instantiate(k);
            p.timeline.video_tracks[1].clips.push(c);
            caption(&mut p, at, 1.0, &k.name);
        }
        let frames = render(&p, &work, "packages", pk.len() as f64);
        let tiles: Vec<ColorImage> = (0..pk.len()).map(|i| frame_at(&frames, i as f64 + 0.5)).collect();
        let out = work.join("packages.png");
        crate::demo::save_png(&out, &sheet(&tiles, 5)).unwrap();
        made.push(out);
    }
    for f in &made {
        let to = dir.join(f.file_name().unwrap());
        std::fs::copy(f, &to).unwrap();
        println!("contact sheet: {}", to.display());
    }
    let _ = std::fs::remove_dir_all(&work);
}
