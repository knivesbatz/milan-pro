//! R37, R38, R40a, R41: end-to-end format tests. They need ffmpeg and ffprobe and are ignored by default; every file
//! they read is generated here from lavfi sources (nothing recorded, nothing downloaded) into `caps::corpus::corpus_dir`
//! (`MILAN_TESTMEDIA`, else `target/testmedia`).
//!
//! `cargo test --release -- --ignored --test-threads=1 format_corpus_e2e image_sequence_e2e animated_gif_e2e
//! hdr_view_transform_e2e camera_formats_e2e`
use std::path::{Path, PathBuf};
use std::sync::Arc;
use eframe::egui::ColorImage;
use crate::decode::Quality;
use crate::engine::caps::{self, corpus::*};
use crate::engine::vdecode::{DecoderPool, Pull};
use crate::project::{Clip, ColorManagement, Hdr, MediaItem, MediaType, Project, ProjectSettings};

/// A folder of the corpus, emptied first.
fn fresh(name: &str) -> PathBuf {
    let d = corpus_dir().join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Run ffmpeg with `args`, panicking with its message when it fails.
fn ff(args: &[String]) {
    let mut cmd = crate::proc::ffmpeg();
    cmd.args(args);
    let out = crate::proc::output(&mut cmd).unwrap();
    assert!(out.status.success(), "ffmpeg {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr));
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

/// Frame `sf` (on a `fps` grid) of `m` through the engine's decoder at `w` x `h`, as the timeline reads it in a project
/// with settings `ps`.
fn decode(m: &MediaItem, ps: &ProjectSettings, sf: i64, w: u32, h: u32) -> Result<Arc<ColorImage>, String> {
    let r = crate::engine::plan::source_ref(m, ps).ok_or("no picture source")?;
    if r.still {
        return crate::decode::still(&r.path, w, h, Quality::Best.scaler(), &r.pre).map(Arc::new);
    }
    let mut pool = DecoderPool::default();
    pool.set_rate(ps.frame_rate, &crate::render::fps_str(ps.frame_rate));
    match pool.frame(&r, sf, w, h, Quality::Best, None) {
        Pull::Exact(img) | Pull::Held(Some(img)) => Ok(img),
        Pull::Held(None) => Err(format!("no frame {sf}")),
        Pull::Err(e) => Err(e),
    }
}

fn px(img: &ColorImage, x: usize, y: usize) -> [u8; 3] {
    let c = img.pixels[y * img.size[0] + x];
    [c.r(), c.g(), c.b()]
}

/// One frame of `path` as ffmpeg decodes it through `vf` (ending in an RGB format) at `w` x `h`: the reference.
fn ffmpeg_frame(path: &Path, vf: &str, w: u32, h: u32) -> Vec<u8> {
    let mut cmd = crate::proc::ffmpeg();
    cmd.args(["-v", "error", "-i"]).arg(path).args(["-frames:v", "1", "-vf", &format!("{vf},scale={w}:{h}:flags=bicubic,format=rgb24"), "-f", "rawvideo", "-"]);
    let out = crate::proc::output(&mut cmd).unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    out.stdout
}

/// The `smptehdbars` check of a decoded frame: the 40 % and 75 % grey bars are neutral (any matrix gives them alike),
/// and the five colour bars after the first two have their 75 % channels high and the others low. Ok, or what failed.
fn bars_ok(img: &ColorImage) -> Result<(), String> {
    let (w, h) = (img.size[0] as f64, img.size[1] as f64);
    let y = (h * 7.0 / 24.0) as usize;
    let bar = |i: f64| (w / 8.0 + (i + 0.5) * (w * 3.0 / 28.0)) as usize;
    let near = |c: [u8; 3], want: [i32; 3], tol: i32| c.iter().zip(want).all(|(&a, b)| (a as i32 - b).abs() <= tol);
    let checks: [(&str, usize, [i32; 3], i32); 8] = [
        ("40% grey", (w / 16.0) as usize, [104, 104, 104], 10),
        ("75% grey", bar(0.0), [191, 191, 191], 10),
        ("yellow", bar(1.0), [191, 191, 0], 40),
        ("cyan", bar(2.0), [0, 191, 191], 40),
        ("green", bar(3.0), [0, 191, 0], 40),
        ("magenta", bar(4.0), [191, 0, 191], 40),
        ("red", bar(5.0), [191, 0, 0], 40),
        ("blue", bar(6.0), [0, 0, 191], 40),
    ];
    for (name, x, want, tol) in checks {
        let c = px(img, x, y);
        if !near(c, want, tol) {
            return Err(format!("{name} bar at {x},{y} is {c:?}, want {want:?} ±{tol}"));
        }
    }
    Ok(())
}

/// The project a file is tested in: 30 fps, 48 kHz, the file's own size.
fn project_for(m: &MediaItem) -> Project {
    let mut p = Project::new("formats");
    (p.settings.width, p.settings.height, p.settings.frame_rate) = (m.width.unwrap_or(320).max(2) & !1, m.height.unwrap_or(240).max(2) & !1, 30.0);
    p.media_pool = vec![m.clone()];
    p
}

/// RMS of the first second of `m` through the mixer (an audio event at 0 on the first audio track), both channels.
fn audio_rms(m: &MediaItem) -> f32 {
    let mut p = project_for(m);
    let mut c = Clip::from_media(m, 0.0);
    (c.duration, c.out_point) = (1.0, c.in_point + 1.0);
    p.timeline.audio_tracks[0].clips = vec![c];
    p.timeline.audio_tracks[0].volume = 1.0;
    let plan = crate::engine::plan::compile_audio(&p);
    let mut mixer = crate::engine::mix::Mixer::ffmpeg();
    mixer.render = Some(Arc::new(std::sync::atomic::AtomicBool::new(false)));
    let n = crate::engine::mix::frame(1.0) as usize;
    mixer.warm(&plan, 0, n as i64);
    let mut out = vec![0f32; n * 2];
    for (i, b) in out.chunks_mut(2 * 1024).enumerate() {
        mixer.block(&plan, (i * 1024) as i64, b);
    }
    (out.iter().map(|v| v * v).sum::<f32>() / out.len() as f32).sqrt()
}

/// R37: the legal corpus. Every row is generated, imported (probe: codecs as made), decoded at frame 0 and the middle
/// frame through the engine (the SMPTE bars where they belong) and, with sound, 1 s is mixed (RMS of the 1 kHz sine).
/// Then docs/FORMAT_SUPPORT.md's decode table, capability table and camera section are rewritten.
#[test]
#[ignore]
fn format_corpus_e2e() {
    let dir = fresh("corpus");
    let caps = caps::probe();
    caps::set_table(Some(caps.clone()));
    let mut rows = Vec::new();
    let mut failed = Vec::new();
    for r in CORPUS {
        let path = dir.join(r.file);
        ff(&corpus_args(r, &path));
        let mut verdict = Vec::new();
        let m = match crate::media::probe_media(&path) {
            Ok(m) => m,
            Err(e) => {
                failed.push(format!("{}: probe {e}", r.file));
                rows.push(format!("| {} | {} | no: {e} | {} | {} |", r.file, r.note, encode_of(r, &caps), r.licence));
                continue;
            }
        };
        if let Some(why) = &m.unsupported {
            verdict.push(format!("offline: {why}"));
        }
        if let Some(v) = r.video {
            if m.codec.as_deref() != Some(v) || m.media_type != MediaType::Video {
                verdict.push(format!("video probed as {:?} {:?}", m.media_type, m.codec));
            }
            let ps = project_for(&m).settings;
            let (w, h) = (320u32, ((320.0 * m.height.unwrap_or(180) as f64 / m.width.unwrap_or(320) as f64).round() as u32) & !1);
            for sf in [0, 30] {
                match decode(&m, &ps, sf, w, h) {
                    Ok(img) => {
                        if let Err(e) = bars_ok(&img) {
                            verdict.push(format!("frame {sf}: {e}"));
                        }
                    }
                    Err(e) => verdict.push(format!("frame {sf}: {e}")),
                }
            }
        }
        if let Some(a) = r.audio {
            let stream = if r.video.is_some() { m.audio_streams > 0 } else { m.media_type == MediaType::Audio && m.codec.as_deref() == Some(a) };
            if !stream || m.sample_rate.is_none() {
                verdict.push(format!("audio probed as {:?} {:?} ({} streams)", m.media_type, m.codec, m.audio_streams));
            } else {
                // The sine is 1/8 full scale (RMS 0.088); a downmix may raise it, a mono file plays at unity to both sides.
                let rms = audio_rms(&m);
                if !(0.04..=0.5).contains(&rms) {
                    verdict.push(format!("1 s of sound has RMS {rms:.3}"));
                }
            }
        }
        let ok = verdict.is_empty();
        if !ok {
            failed.push(format!("{}: {}", r.file, verdict.join("; ")));
        }
        let what = [r.video, r.audio].into_iter().flatten().collect::<Vec<_>>().join(" + ");
        let size = if r.size.is_empty() { "48 kHz".to_string() } else { format!("{} @ {}", r.size, r.rate) };
        let result = if ok { "yes".to_string() } else { format!("no: {}", verdict.join("; ")) };
        rows.push(format!("| {} | {what}, {size} | {result} | {} | {} | {} |", r.file, encode_of(r, &caps), r.note, r.licence));
    }
    let table = format!(
        "Generated by `format_corpus_e2e` ({}), {} UTC: each file made from lavfi (`smptehdbars`, a 1 kHz `sine`), imported, frame 0 and the middle frame decoded through the engine and checked against the bars, 1 s of sound mixed and its RMS checked.\n\n| File | Streams | Decodes in Milan Pro | Render as same | Notes | Licence |\n|---|---|---|---|---|---|\n{}\n",
        caps.ffmpeg,
        chrono_now(),
        rows.join("\n")
    );
    let mut camera = String::from("| Camera format | Milan Pro |\n|---|---|\n");
    for (k, v) in crate::camera::SUPPORT {
        camera += &format!("| {k} | {v} |\n");
    }
    for (ext, name, why) in crate::camera::RAW {
        camera += &format!("| {name} (.{ext}) | not supported: {why}; imports as an offline item that says so |\n");
    }
    camera += "| CinemaDNG (folders of .dng frames) | not supported: FFmpeg reads some DNG stills through its TIFF decoder but without the camera's colour matrix, white balance or the compressions most cameras use; DNG stills import as pictures with that caveat, and DNG files are not offered as a sequence |\n";
    let doc = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs").join("FORMAT_SUPPORT.md");
    let text = std::fs::read_to_string(&doc).unwrap_or_else(|_| "# Format support\n".into());
    let text = replace_block(&text, "decode-table", &table);
    let text = replace_block(&text, "capability-table", &capability_section(&caps));
    let text = replace_block(&text, "camera-table", &camera);
    std::fs::write(&doc, text).unwrap();
    caps::set_table(None);
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// The encode column of a corpus row: the render templates that write its codec, or why none does.
fn encode_of(r: &Row, caps: &caps::Caps) -> String {
    let codec = r.video.or(r.audio).unwrap_or("");
    let m = MediaItem { media_type: if r.video.is_some() { MediaType::Video } else { MediaType::Audio }, codec: Some(codec.into()), sample_rate: Some(48000), width: r.video.map(|_| 640), ..Default::default() };
    caps::verdict(&m, Some(caps)).into_iter().find(|(k, _)| k == "Render as same").map_or("-".into(), |(_, v)| v)
}

fn chrono_now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(mo <= 2);
    format!("{y:04}-{mo:02}-{d:02} {:02}:{:02}:{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// The 64x64 frame of a numbered still: a grey level per number, so a frame says which file it came from.
fn level(n: u32) -> u8 {
    (16 + n * 4 % 220) as u8
}

/// R38 acceptance: a 48-frame PNG sequence opened at a 24 fps project rate is one 2.0 s item; its 10th frame is file
/// 0010 (the first frame is file 0001); a gap in the numbering stops it; rendered to a
/// PNG sequence, every frame comes out as its file, pixel for pixel. Stills FFmpeg can write (PNG with alpha, JPEG,
/// 16-bit TIFF, BMP, WebP, TGA, DPX, JPEG 2000, one-frame GIF, OpenEXR, Radiance HDR) import as pictures and decode;
/// PSD has no FFmpeg encoder, so it is covered by the capability table only.
#[test]
#[ignore]
fn image_sequence_e2e() {
    let dir = fresh("seq");
    // Files 0001..0048, then a gap, then 0050..0052.
    for n in (1..=48).chain(50..=52) {
        let l = level(n);
        ff(&s(&["-v", "error", "-y", "-f", "lavfi", "-i", &format!("color=c=0x{l:02x}{l:02x}{l:02x}:s=64x64:d=1"), "-frames:v", "1", "-pix_fmt", "rgb24", &dir.join(format!("shot_{n:04}.png")).to_string_lossy()]));
    }
    let picked = dir.join("shot_0007.png");
    let seqs = crate::media::sequences_in(std::slice::from_ref(&picked));
    assert_eq!(seqs.len(), 1);
    assert_eq!((seqs[0].1.1, seqs[0].1.2), (1, 48), "the run from 0001 to the gap");
    let m = crate::media::probe_media_with(&picked, crate::media::ImportOpts { sequence: Some(24.0) }).unwrap();
    assert_eq!((m.media_type, m.path.clone(), m.duration, m.frame_rate, m.width), (MediaType::Video, dir.join("shot_0001.png"), 2.0, Some(24.0), Some(64)));
    let seq = m.sequence.clone().unwrap();
    assert_eq!((seq.start, seq.count), (1, 48));
    // After the gap: a run of its own.
    let after = crate::media::probe_media_with(&dir.join("shot_0051.png"), crate::media::ImportOpts { sequence: Some(24.0) }).unwrap();
    assert_eq!((after.sequence.as_ref().map(|s| (s.start, s.count)), after.duration), (Some((50, 3)), 3.0 / 24.0));
    // The picked file alone is still a still.
    let still = crate::media::probe_media(&picked).unwrap();
    assert_eq!((still.media_type, still.sequence.is_none()), (MediaType::Image, true));

    let mut p = project_for(&m);
    p.settings.frame_rate = 24.0;
    // The 10th frame (index 9) is file 0010.
    for sf in [0i64, 9, 10, 47] {
        let img = decode(&m, &p.settings, sf, 64, 64).unwrap();
        let file = ffmpeg_frame(&dir.join(format!("shot_{:04}.png", sf + 1)), "null", 64, 64);
        assert_eq!(px(&img, 32, 32)[0], file[(32 * 64 + 32) * 3], "frame {sf} is file {:04}", sf + 1);
    }
    // Rendered as a PNG sequence at the sequence's size and rate: frame k is file k+1 exactly.
    p.timeline.video_tracks[0].clips = vec![Clip::from_media(&m, 0.0)];
    p.timeline.audio_tracks.iter_mut().for_each(|t| t.clips.clear());
    let t = crate::render::BUILTIN.iter().find(|t| t.name == "PNG sequence").unwrap();
    let out_dir = dir.join("render");
    std::fs::create_dir_all(&out_dir).unwrap();
    let out = out_dir.join("out_000000.png");
    let job = crate::engine::render::start(&p, t, out, (0.0, 2.0), Default::default(), eframe::egui::Context::default());
    while job.status() == crate::render::Status::Running {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(job.status(), crate::render::Status::Done);
    let mut files: Vec<PathBuf> = std::fs::read_dir(&out_dir).unwrap().flatten().map(|e| e.path()).collect();
    files.sort();
    assert_eq!(files.len(), 48, "{files:?}");
    for (k, f) in files.iter().enumerate() {
        let got = ffmpeg_frame(f, "null", 64, 64);
        let want = ffmpeg_frame(&dir.join(format!("shot_{:04}.png", k + 1)), "null", 64, 64);
        assert!(got == want, "rendered frame {k} differs from shot_{:04}.png", k + 1);
    }

    // Stills: each format FFmpeg writes, imported as a picture and decoded.
    let stills = dir.join("stills");
    std::fs::create_dir_all(&stills).unwrap();
    for (name, extra) in [
        ("a.png", &["-pix_fmt", "rgba"][..]),
        ("a.jpg", &["-q:v", "2"][..]),
        ("a16.tif", &["-pix_fmt", "rgb48le"][..]),
        ("a.bmp", &[][..]),
        ("a.webp", &["-lossless", "1"][..]),
        ("a.tga", &[][..]),
        ("a.dpx", &[][..]),
        ("a.jp2", &[][..]),
        ("a.exr", &["-pix_fmt", "gbrpf32le"][..]),
        ("a.hdr", &[][..]),
        ("one.gif", &[][..]),
    ] {
        let f = stills.join(name);
        let src = if name == "a.png" { "smptehdbars=s=320x180:d=1,format=rgba,colorchannelmixer=aa=0.5" } else { "smptehdbars=s=320x180:d=1" };
        let mut a = s(&["-v", "error", "-y", "-f", "lavfi", "-i", src, "-frames:v", "1"]);
        a.extend(s(extra));
        a.push(f.to_string_lossy().into_owned());
        ff(&a);
        let m = crate::media::probe_media(&f).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!((m.media_type, m.unsupported.as_deref()), (MediaType::Image, None), "{name}");
        let img = decode(&m, &project_for(&m).settings, 0, 320, 180).unwrap_or_else(|e| panic!("{name}: {e}"));
        if name == "a.png" {
            assert!(m.has_alpha && img.pixels[90 * 320 + 160].a() < 200, "{name}: alpha kept");
        } else if name == "one.gif" {
            // 256 colours, dithered: the bars only roughly.
            assert!(img.pixels.iter().any(|c| c.r() > 150 && c.b() < 80), "{name}: a yellow or red bar");
        } else if !matches!(name, "a.exr" | "a.hdr") {
            bars_ok(&img).unwrap_or_else(|e| panic!("{name}: {e}"));
        } else {
            // Linear light through the view transform: the 75 % bar (0.53 linear after the source's transfer) reads
            // back near its SDR value.
            let g = px(&img, (320.0 / 8.0 + 0.5 * 320.0 * 3.0 / 28.0) as usize, 52);
            assert!(g.iter().all(|&c| c > 120), "{name}: {g:?}");
        }
    }
    // SVG: offline with the reason when the build has no librsvg decoder.
    let svg = stills.join("a.svg");
    std::fs::write(&svg, r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="red"/></svg>"#).unwrap();
    let m = crate::media::probe_media(&svg).unwrap();
    if caps::get().can_decode("svg") {
        assert!(!m.is_offline());
    } else {
        assert!(m.unsupported.as_deref().is_some_and(|w| w.contains("librsvg")), "{:?}", m.unsupported);
    }
}

/// R38 acceptance: a 10-frame GIF is a video of 10 frames that all differ; one frame of it is a picture.
#[test]
#[ignore]
fn animated_gif_e2e() {
    let dir = fresh("gif");
    let gif = dir.join("ten.gif");
    // testsrc draws a frame counter and moves its gradient, so every frame differs (also after GIF's 256 colours).
    ff(&s(&["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=s=64x64:r=10:d=1", "-frames:v", "10", &gif.to_string_lossy()]));
    let webp = dir.join("ten.webp");
    ff(&s(&["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=s=64x64:r=10:d=1", "-frames:v", "10", "-c:v", "libwebp_anim", "-lossless", "1", "-loop", "0", &webp.to_string_lossy()]));
    for f in [&gif, &webp] {
        let m = crate::media::probe_media(f).unwrap();
        assert_eq!(m.media_type, MediaType::Video, "{}", f.display());
        assert!((m.duration - 1.0).abs() < 0.05, "{}: {} s", f.display(), m.duration);
        let mut ps = project_for(&m).settings;
        ps.frame_rate = 10.0;
        let frames: Vec<Vec<eframe::egui::Color32>> = (0..10).map(|k| decode(&m, &ps, k, 64, 64).unwrap().pixels.clone()).collect();
        let distinct = frames.windows(2).filter(|w| w[0] != w[1]).count() + 1;
        assert_eq!(distinct, 10, "{}: frames that differ from the one before", f.display());
    }
}

/// An SDR `smptehdbars` clip as PQ or HLG (BT.2020, 10-bit HEVC): SDR white placed at 203 cd/m² as BT.2408 has it,
/// the inverse of the view transform's convention; `None` keeps it SDR Rec.709.
fn hdr_clip(dir: &Path, name: &str, hdr: Option<Hdr>) -> PathBuf {
    let path = dir.join(name);
    let src = "smptehdbars=s=640x360:r=25:d=1,setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv";
    let (vf, tags, x265) = match hdr {
        Some(Hdr::Pq) => (
            "zscale=t=linear:npl=203,format=gbrpf32le,zscale=p=bt2020,zscale=t=smpte2084:m=bt2020nc:r=tv:npl=203,format=yuv420p10le",
            ["bt2020", "smpte2084", "bt2020nc"],
            "log-level=error:hdr10=1:master-display=G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,50):max-cll=1000,400",
        ),
        Some(Hdr::Hlg) => (
            "zscale=t=linear,format=gbrpf32le,zscale=p=bt2020,exposure=exposure=-2.300,zscale=t=arib-std-b67:m=bt2020nc:r=tv:npl=1000,format=yuv420p10le",
            ["bt2020", "arib-std-b67", "bt2020nc"],
            "log-level=error",
        ),
        _ => ("format=yuv420p10le", ["bt709", "bt709", "bt709"], "log-level=error"),
    };
    ff(&s(&[
        "-v", "error", "-y", "-f", "lavfi", "-i", src, "-vf", vf, "-c:v", "libx265", "-preset", "ultrafast", "-crf", "4", "-x265-params", x265,
        "-color_primaries", tags[0], "-color_trc", tags[1], "-colorspace", tags[2], &path.to_string_lossy(),
    ]));
    path
}

/// R40a acceptance: PQ and HLG clips are detected (transfer, primaries, bit depth, the PQ clip's mastering display and
/// content light level), shown through the view transform within tolerance of FFmpeg's own tone map of the same file
/// and within ±5 % of the SDR original on the mid greys; an SDR clip decodes bit-identically with the transform on or
/// off, and its decode filter has no transform.
#[test]
#[ignore]
fn hdr_view_transform_e2e() {
    let dir = fresh("hdr");
    let sdr = crate::media::probe_media(&hdr_clip(&dir, "sdr.mp4", None)).unwrap();
    assert_eq!((sdr.hdr(), sdr.bit_depth), (None, Some(10)));
    let ps = |cm| ProjectSettings { width: 640, height: 360, frame_rate: 25.0, color_management: cm, ..Default::default() };
    assert_eq!(crate::engine::plan::source_ref(&sdr, &ps(ColorManagement::Sdr)).unwrap().pre, "");
    let (on, off) = (decode(&sdr, &ps(ColorManagement::Sdr), 0, 320, 180).unwrap(), decode(&sdr, &ps(ColorManagement::Off), 0, 320, 180).unwrap());
    assert!(on.pixels == off.pixels, "an SDR source is untouched by the view transform");
    let grey = |img: &ColorImage, x: usize| px(img, x, 52)[1] as f64;
    // The 40 % bar and the 75 % bar.
    let (g40, g75) = ((320.0 / 16.0) as usize, (320.0 / 8.0 + 0.5 * 320.0 * 3.0 / 28.0) as usize);
    for (name, kind, transfer) in [("pq.mp4", Hdr::Pq, "smpte2084"), ("hlg.mp4", Hdr::Hlg, "arib-std-b67")] {
        let m = crate::media::probe_media(&hdr_clip(&dir, name, Some(kind))).unwrap();
        assert_eq!((m.hdr(), m.transfer.as_deref(), m.primaries.as_deref(), m.bit_depth), (Some(kind), Some(transfer), Some("bt2020"), Some(10)), "{name}");
        if kind == Hdr::Pq {
            let md = m.mastering.expect("HDR10 metadata");
            assert_eq!((md.max_luminance, md.max_cll, md.max_fall), (1000.0, 1000, 400), "{md:?}");
            assert!((md.min_luminance - 0.005).abs() < 1e-6, "{md:?}");
        }
        let img = decode(&m, &ps(ColorManagement::Sdr), 0, 320, 180).unwrap();
        // FFmpeg's own tone map of the file, through the same filters in one command.
        let pre = crate::engine::color::view_transform(&m, ColorManagement::Sdr);
        let reference = ffmpeg_frame(&dir.join(name), pre.trim_end_matches(','), 320, 180);
        let diff = img.pixels.iter().zip(reference.chunks(3)).map(|(c, r)| (c.r() as i32 - r[0] as i32).abs() + (c.g() as i32 - r[1] as i32).abs() + (c.b() as i32 - r[2] as i32).abs()).sum::<i32>() as f64 / (3.0 * img.pixels.len() as f64);
        assert!(diff < 2.0, "{name}: mean difference {diff:.2} from FFmpeg's tone map");
        for x in [g40, g75] {
            let (got, want) = (grey(&img, x), grey(&on, x));
            assert!((got - want).abs() <= want * 0.05, "{name}: grey at {x} is {got}, SDR {want}");
        }
        // Off: decoded as if Rec.709 SDR, the washed-out picture R40a ends.
        let washed = decode(&m, &ps(ColorManagement::Off), 0, 320, 180).unwrap();
        assert!((grey(&washed, g75) - grey(&on, g75)).abs() > 20.0, "{name}: off differs: {} vs {}", grey(&washed, g75), grey(&on, g75));
        // Proxies are made through the transform.
        assert!(crate::proxy::args(&m, Path::new("p.mp4")).join(" ").contains(&pre), "{name}: proxy");
    }
}

/// R41 acceptance on FFmpeg-made stand-ins (no camera originals are used): a spanned AVCHD pair (two transport streams
/// whose clock runs on) in a card's folder structure imports as one item of the summed length that plays the second part;
/// XAVC-style MP4 (make, model, QuickTime timecode), XDCAM HD422 and DNxHD in MXF OP1a (timecode), a P2-style card
/// (AVC-Intra 100 in MXF, DNxHD and PCM in MXF OP-Atom) decode; RAW files import offline with the reason.
#[test]
#[ignore]
fn camera_formats_e2e() {
    let root = fresh("card");
    let stream = root.join("PRIVATE").join("AVCHD").join("BDMV").join("STREAM");
    std::fs::create_dir_all(&stream).unwrap();
    // 4 s at 25 fps whose frame n has luma 16 + 2n, cut into two parts at 2 s without resetting the clock.
    let whole = root.join("whole.m2ts");
    ff(&s(&["-v", "error", "-y", "-f", "lavfi", "-i", "nullsrc=s=320x180:r=25:d=4,geq=lum='16+2*N':cb=128:cr=128,format=yuv420p", "-f", "lavfi", "-i", "sine=f=1000:d=4", "-c:v", "libx264", "-preset", "ultrafast", "-qp", "0", "-g", "25", "-c:a", "ac3", "-f", "mpegts", &whole.to_string_lossy()]));
    ff(&s(&["-v", "error", "-y", "-i", &whole.to_string_lossy(), "-c", "copy", "-f", "segment", "-segment_time", "2", "-segment_format", "mpegts", "-segment_start_number", "0", &stream.join("%05d.MTS").to_string_lossy()]));
    std::fs::remove_file(&whole).unwrap();
    let mut notes = Vec::new();
    let paths = crate::camera::import_paths(&root, &mut notes);
    assert_eq!(paths.len(), 1, "{paths:?} {notes:?}");
    let m = crate::media::probe_media(&paths[0]).unwrap();
    assert_eq!((m.media_type, m.spanned.len(), m.name.as_str()), (MediaType::Video, 2, "00000.MTS (+1 spanned)"));
    assert!((m.duration - 4.0).abs() < 0.1, "{} s", m.duration);
    let ps = ProjectSettings { width: 320, height: 180, frame_rate: 25.0, ..Default::default() };
    // Every frame through the engine (each one a fresh seek) equals FFmpeg reading the joined list straight through on
    // the same grid, across the join; the second part's frames (luma 16 + 2n, n from 50) are there.
    let mut cmd = crate::proc::ffmpeg();
    cmd.args(["-v", "error", "-i"]).arg(&paths[0]).args(["-map", "0:v:0", "-vf", "fps=25:round=up:start_time=0,scale=320:180,format=rgba", "-frames:v", "100", "-f", "rawvideo", "-"]);
    let all = crate::proc::output(&mut cmd).unwrap().stdout;
    let want: Vec<u8> = all.chunks(320 * 180 * 4).map(|f| f[(90 * 320 + 160) * 4]).collect();
    assert_eq!(want.len(), 100);
    assert!(want[99] as f64 > 2.0 * 80.0 * 255.0 / 219.0, "the second part plays: {want:?}");
    for sf in (0..100).step_by(3).chain([49, 50, 51]) {
        let got = px(&decode(&m, &ps, sf as i64, 320, 180).unwrap(), 160, 90)[0];
        assert!(got.abs_diff(want[sf]) <= 1, "frame {sf}: {got}, straight through {}: {want:?}", want[sf]);
    }
    assert!(audio_rms(&m) > 0.04, "the joined clip has sound");

    // XAVC-style MP4 (H.264 4:2:2 10-bit, Sony tags, a timecode track).
    let cam = fresh("cam");
    let xavc = cam.join("C0001.MP4");
    ff(&s(&["-v", "error", "-y", "-f", "lavfi", "-i", "smptehdbars=s=1920x1080:r=25:d=1", "-f", "lavfi", "-i", "sine=f=1000:d=1", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv422p10le", "-c:a", "pcm_s24be", "-metadata", "make=Sony", "-metadata", "model=ILME-FX6", "-timecode", "01:02:03:04", "-f", "mov", &xavc.to_string_lossy()]));
    let m = crate::media::probe_media(&xavc).unwrap();
    assert_eq!(m.camera.as_deref(), Some("Sony ILME-FX6"));
    assert!(m.timecode.is_some_and(|t| (t - (3723.0 + 4.0 / 25.0)).abs() < 1e-6), "{:?}", m.timecode);
    bars_ok(&decode(&m, &project_for(&m).settings, 0, 320, 180).unwrap()).unwrap();
    // XDCAM HD422 and DNxHD in MXF OP1a; FFmpeg's own MXF identification is not a camera.
    for (name, args) in [
        ("XDCAM.MXF", &["-c:v", "mpeg2video", "-pix_fmt", "yuv422p", "-b:v", "50M", "-minrate", "50M", "-maxrate", "50M", "-bufsize", "17825792", "-g", "12", "-c:a", "pcm_s16le"][..]),
        ("DNXHD.MXF", &["-c:v", "dnxhd", "-pix_fmt", "yuv422p", "-b:v", "120M", "-c:a", "pcm_s16le"][..]),
        ("AVCI.MXF", &["-c:v", "libx264", "-avcintra-class", "100", "-pix_fmt", "yuv422p10le", "-c:a", "pcm_s24le"][..]),
    ] {
        let f = cam.join(name);
        let mut a = s(&["-v", "error", "-y", "-f", "lavfi", "-i", "smptehdbars=s=1920x1080:r=25:d=1", "-f", "lavfi", "-i", "sine=f=1000:r=48000:d=1"]);
        a.extend(s(args));
        a.extend(s(&["-timecode", "10:00:00:00", "-f", "mxf", &f.to_string_lossy()]));
        ff(&a);
        let m = crate::media::probe_media(&f).unwrap();
        assert_eq!((m.timecode, m.camera.as_deref(), m.unsupported.as_deref()), (Some(36000.0), None, None), "{name}");
        bars_ok(&decode(&m, &project_for(&m).settings, 0, 320, 180).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
    // A P2-style card: picture and sound as separate MXF OP-Atom files.
    let p2 = fresh("p2");
    let (video, audio) = (p2.join("CONTENTS").join("VIDEO"), p2.join("CONTENTS").join("AUDIO"));
    std::fs::create_dir_all(&video).unwrap();
    std::fs::create_dir_all(&audio).unwrap();
    ff(&s(&["-v", "error", "-y", "-f", "lavfi", "-i", "smptehdbars=s=1920x1080:r=25:d=1", "-c:v", "dnxhd", "-pix_fmt", "yuv422p", "-b:v", "120M", "-timecode", "10:00:00:00", "-f", "mxf_opatom", &video.join("0001AB.MXF").to_string_lossy()]));
    ff(&s(&["-v", "error", "-y", "-f", "lavfi", "-i", "sine=f=1000:r=48000:d=1", "-c:a", "pcm_s24le", "-f", "mxf_opatom", "-mxf_audio_edit_rate", "25", &audio.join("0001AB00.MXF").to_string_lossy()]));
    let found = crate::camera::import_paths(&p2, &mut notes);
    assert_eq!(found.len(), 2, "{found:?}");
    let items: Vec<MediaItem> = found.iter().map(|f| crate::media::probe_media(f).unwrap()).collect();
    assert_eq!(items.iter().map(|m| m.media_type).collect::<Vec<_>>(), [MediaType::Video, MediaType::Audio]);
    bars_ok(&decode(&items[0], &project_for(&items[0]).settings, 0, 320, 180).unwrap()).unwrap();
    assert!(audio_rms(&items[1]) > 0.04);
    // RAW camera files: offline with the reason.
    let braw = cam.join("A001_C001.braw");
    std::fs::write(&braw, b"not a real clip").unwrap();
    let m = crate::media::probe_media(&braw).unwrap();
    assert!(m.is_offline() && m.unsupported.as_deref().is_some_and(|w| w.contains("Blackmagic RAW SDK")));
}
