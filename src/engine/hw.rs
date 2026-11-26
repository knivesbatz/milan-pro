//! R22: the hardware acceleration capability table. At startup a background thread asks the bundled FFmpeg what it
//! was built with (`-hwaccels`, `-encoders`) and then tries each hardware decoder path (d3d11va, cuda, qsv) on a short
//! H.264, HEVC and AV1 clip (5 s of 1080p) against the software decoder, and each hardware encoder (h264/hevc/av1 ×
//! nvenc/qsv/amf/mf) on the same generated picture. The result is kept in %LOCALAPPDATA%\MilanPro\hw.json per FFmpeg version and GPU
//! driver, so the tests run once per machine state. Preferences › Video chooses the decode acceleration (Automatic,
//! Off or one hwaccel) and the encode device; `decode_hwaccel` and `encoder_for` answer the decoder and Render As.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Preferences › Video: no hardware decoding / no hardware encoders ("" is Automatic).
pub const OFF: &str = "Off";
/// The decoder paths tried, in the order Automatic prefers them when they measure alike.
pub const HWACCELS: [&str; 3] = ["d3d11va", "cuda", "qsv"];
/// The encoder families tried (the suffix of ffmpeg's encoder name) and their labels.
pub const DEVICES: [(&str, &str); 4] = [("nvenc", "NVIDIA NVENC"), ("qsv", "Intel Quick Sync Video"), ("amf", "AMD AMF"), ("mf", "Windows Media Foundation")];
/// The codecs tested, as ffmpeg names them.
pub const CODECS: [&str; 3] = ["h264", "hevc", "av1"];
/// Frames in each test clip and encode (1920x1080 at 30 fps): long enough that the decoder's start-up (about 150 ms
/// for a hardware one) does not decide the comparison with the software decoder.
const FRAMES: i64 = 150;
/// A test that runs longer than this is killed and counts as failed.
const TIMEOUT: Duration = Duration::from_secs(30);

/// One hardware decode test: `hwaccel` on a `codec` clip.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Decode {
    pub hwaccel: String,
    pub codec: String,
    pub ok: bool,
    /// Milliseconds for `FRAMES` frames (ffmpeg start-up included), and the software decoder's on the same clip.
    pub ms: f64,
    pub software_ms: f64,
    pub error: String,
}

/// One hardware encode test.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Encode {
    pub encoder: String,
    pub ok: bool,
    pub ms: f64,
    pub error: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Table {
    /// The FFmpeg build and the graphics drivers the table was measured with.
    pub ffmpeg: String,
    pub driver: String,
    pub built: String,
    /// `ffmpeg -hwaccels`.
    pub hwaccels: Vec<String>,
    /// The hardware encoders `ffmpeg -encoders` lists (built in; not necessarily working here).
    pub encoders: Vec<String>,
    pub decode: Vec<Decode>,
    pub encode: Vec<Encode>,
}

impl Table {
    /// The decode test of `hwaccel` on `codec`.
    pub fn decode(&self, hwaccel: &str, codec: &str) -> Option<&Decode> {
        self.decode.iter().find(|d| d.hwaccel == hwaccel && d.codec == codec)
    }

    pub fn encode(&self, encoder: &str) -> Option<&Encode> {
        self.encode.iter().find(|e| e.encoder == encoder)
    }

    /// The hwaccels that decoded at least one codec.
    #[cfg(test)]
    pub fn working_hwaccels(&self) -> Vec<String> {
        let mut v: Vec<String> = self.decode.iter().filter(|d| d.ok).map(|d| d.hwaccel.clone()).collect();
        v.dedup();
        v
    }

    /// The encoders that passed.
    #[cfg(test)]
    pub fn passed_encoders(&self) -> Vec<String> {
        self.encode.iter().filter(|e| e.ok).map(|e| e.encoder.clone()).collect()
    }

    /// The best working hwaccel for `codec`: the fastest one that beat the software decoder.
    fn fastest(&self, codec: &str) -> Option<&Decode> {
        self.decode.iter().filter(|d| d.ok && d.codec == codec && d.ms < d.software_ms).min_by(|a, b| a.ms.total_cmp(&b.ms))
    }
}

struct State {
    table: Option<Arc<Table>>,
    /// What is going on, for Preferences › Video while there is no table (or when the probe failed).
    status: String,
    decode_pref: String,
    encode_pref: String,
    round: u64,
}

static STATE: Mutex<State> = Mutex::new(State { table: None, status: String::new(), decode_pref: String::new(), encode_pref: String::new(), round: 0 });

fn lock() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

// ---- Parsers ----

/// The methods `ffmpeg -hwaccels` prints after its heading.
pub fn parse_hwaccels(out: &str) -> Vec<String> {
    out.lines().map(str::trim).skip_while(|l| !l.starts_with("Hardware acceleration methods")).skip(1).filter(|l| !l.is_empty() && !l.contains(' ')).map(String::from).collect()
}

/// The hardware video encoders `ffmpeg -encoders` lists: a `V` flag column and a name `<codec>_<device>` for the
/// codecs and devices tested here.
pub fn parse_encoders(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let flags = it.next()?;
            let name = it.next()?;
            (flags.starts_with('V') && flags.len() == 6 && is_hardware_encoder(name)).then(|| name.to_string())
        })
        .collect()
}

/// `h264_amf`, `hevc_nvenc`...: a tested codec and device.
pub fn is_hardware_encoder(name: &str) -> bool {
    family(name).is_some()
}

/// (codec, device) of a hardware encoder's name.
pub fn family(name: &str) -> Option<(&str, &str)> {
    let (codec, dev) = name.split_once('_')?;
    (CODECS.contains(&codec) && DEVICES.iter().any(|d| d.0 == dev)).then_some((codec, dev))
}

/// The software encoder standing in for a hardware codec family.
pub fn software_for(codec: &str) -> &'static str {
    match codec {
        "hevc" => "libx265",
        "av1" => "libsvtav1",
        _ => "libx264",
    }
}

/// The label of an encoder device suffix.
pub fn device_label(dev: &str) -> &'static str {
    DEVICES.iter().find(|d| d.0 == dev).map_or("hardware", |d| d.1)
}

// ---- The probe ----

/// Run `cmd` (stdout and stderr captured), killed after `TIMEOUT`: (success, ms, the last stderr line).
fn run_timed(cmd: &mut std::process::Command) -> (bool, f64, String) {
    let t = Instant::now();
    let mut child = match crate::proc::spawn(cmd.stdout(Stdio::null()).stderr(Stdio::piped())) {
        Ok(c) => c,
        Err(e) => return (false, 0.0, crate::proc::tool_error(&e, "ffmpeg")),
    };
    let err = child.stderr.take().expect("piped stderr");
    let said = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader};
        BufReader::new(err).lines().map_while(Result::ok).filter(|l| !l.trim().is_empty()).collect::<Vec<String>>()
    });
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if t.elapsed() > TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(_) => break None,
        }
    };
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let lines = said.join().unwrap_or_default();
    // A hwaccel that could not start falls back to software with a warning: that is a failed test, not a pass.
    let fell_back = lines.iter().any(|l| l.contains("Failed setup for format") || l.contains("hwaccel initialisation returned error") || l.contains("Failed to initialise"));
    let last = lines.last().cloned().unwrap_or_default();
    match status {
        Some(s) if s.success() && !fell_back => (true, ms, String::new()),
        Some(_) => (false, ms, if last.is_empty() { "ffmpeg failed".into() } else { last }),
        None => (false, ms, format!("no answer in {} s", TIMEOUT.as_secs())),
    }
}

/// The test clip of `codec` (1920x1080, `FRAMES` frames, software encoded) in `dir`, made once.
fn clip(dir: &Path, codec: &str) -> Option<PathBuf> {
    let path = dir.join(format!("probe-{codec}.mp4"));
    if path.exists() {
        return Some(path);
    }
    std::fs::create_dir_all(dir).ok()?;
    let partial = dir.join(format!("probe-{codec}.partial.mp4"));
    let mut cmd = crate::proc::ffmpeg();
    cmd.args(["-v", "error", "-y", "-f", "lavfi", "-i", &format!("testsrc2=size=1920x1080:rate=30:duration={}", FRAMES as f64 / 30.0), "-pix_fmt", "yuv420p", "-c:v", software_for(codec)]);
    match codec {
        "h264" | "hevc" => cmd.args(["-preset", "ultrafast", "-g", "15"]),
        _ => cmd.args(["-preset", "12"]),
    };
    cmd.arg(&partial);
    let (ok, ..) = run_timed(&mut cmd);
    if !ok || std::fs::rename(&partial, &path).is_err() {
        let _ = std::fs::remove_file(&partial);
        return None;
    }
    Some(path)
}

/// The best of two decodes of `clip`, in ms (the first warms the file cache).
fn decode_ms(clip: &Path, hwaccel: Option<&str>) -> (bool, f64, String) {
    let run = || {
        let mut cmd = crate::proc::ffmpeg();
        cmd.args(["-v", "warning", "-threads", "2"]);
        if let Some(h) = hwaccel {
            cmd.args(["-hwaccel", h]);
        }
        cmd.arg("-i").arg(clip).args(["-an", "-f", "null", "-"]);
        run_timed(&mut cmd)
    };
    let a = run();
    if !a.0 {
        return a;
    }
    let b = run();
    (b.0, a.1.min(b.1), b.2)
}

/// One encode of `FRAMES` generated 1920x1080 frames with `encoder`.
fn encode_test(encoder: &str) -> (bool, f64, String) {
    let mut cmd = crate::proc::ffmpeg();
    cmd.args(["-v", "error", "-f", "lavfi", "-i", &format!("testsrc2=size=1920x1080:rate=30:duration={},format=yuv420p", FRAMES as f64 / 30.0), "-c:v", encoder, "-f", "null", "-"]);
    run_timed(&mut cmd)
}

/// The first line of `ffmpeg -version`.
fn ffmpeg_version() -> String {
    let mut cmd = crate::proc::ffmpeg();
    cmd.arg("-version");
    let out = crate::proc::output(&mut cmd).ok();
    out.and_then(|o| String::from_utf8_lossy(&o.stdout).lines().next().map(|l| l.split(" Copyright").next().unwrap_or(l).trim().to_string())).unwrap_or_else(|| "unknown".into())
}

/// The graphics adapters and their drivers, as Vulkan reports them: a driver update means a new table.
fn driver_key() -> String {
    std::panic::catch_unwind(|| {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor { backends: wgpu::Backends::VULKAN, ..Default::default() });
        let mut v: Vec<String> = instance.enumerate_adapters(wgpu::Backends::VULKAN).iter().map(|a| a.get_info()).map(|i| format!("{} ({} {})", i.name, i.driver, i.driver_info)).collect();
        v.sort();
        v.dedup();
        v.join("; ")
    })
    .unwrap_or_else(|_| "unknown".into())
}

/// Build the table: what FFmpeg lists, then a timed test of every decoder path and encoder, with the clips in `dir`.
/// `progress` is told what is being tested.
pub fn probe(dir: &Path, mut progress: impl FnMut(&str)) -> Table {
    let mut t = Table { ffmpeg: ffmpeg_version(), driver: driver_key(), built: crate::render::utc(std::time::SystemTime::now()), ..Default::default() };
    let list = |flag: &str| {
        let mut cmd = crate::proc::ffmpeg();
        cmd.args(["-hide_banner", flag]);
        crate::proc::output(&mut cmd).map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
    };
    t.hwaccels = parse_hwaccels(&list("-hwaccels"));
    t.encoders = parse_encoders(&list("-encoders"));
    let tried: Vec<&str> = HWACCELS.iter().copied().filter(|h| t.hwaccels.iter().any(|x| x == h)).collect();
    for codec in CODECS {
        progress(&format!("Making the {codec} test clip..."));
        let Some(clip) = clip(dir, codec) else { continue };
        progress(&format!("Decoding {codec} in software..."));
        let (sw_ok, sw_ms, _) = decode_ms(&clip, None);
        if !sw_ok {
            continue;
        }
        for h in &tried {
            progress(&format!("Decoding {codec} with {h}..."));
            let (ok, ms, error) = decode_ms(&clip, Some(h));
            t.decode.push(Decode { hwaccel: h.to_string(), codec: codec.to_string(), ok, ms, software_ms: sw_ms, error });
        }
    }
    for enc in t.encoders.clone() {
        progress(&format!("Encoding with {enc}..."));
        let (ok, ms, error) = encode_test(&enc);
        t.encode.push(Encode { encoder: enc, ok, ms, error });
    }
    t
}

/// `%LOCALAPPDATA%\MilanPro\hw.json`, and the folder of the test clips beside it.
fn cache_path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("MilanPro").join("hw.json"))
}

fn load(path: &Path) -> Option<Table> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Start building the table on a background thread: the cached one when it was measured with this FFmpeg and these
/// drivers, else a new probe (`write`: saved for the next start; the screenshot harness reads but never writes).
/// `again` ignores the cache (Preferences › Video › Test Again). `MILAN_HW_TABLE=<file.json>` (the harness) loads that
/// table as it is, with no probe.
pub fn start(write: bool, again: bool) {
    if let Some(fixture) = std::env::var_os("MILAN_HW_TABLE").map(PathBuf::from) {
        let mut s = lock();
        s.round += 1;
        s.table = Some(Arc::new(load(&fixture).unwrap_or_default()));
        s.status = String::new();
        return;
    }
    let round = {
        let mut s = lock();
        s.round += 1;
        s.status = "Testing hardware acceleration...".into();
        if again {
            s.table = None;
        }
        s.round
    };
    let _ = std::thread::Builder::new().name("milan-hw".into()).spawn(move || {
        let path = cache_path();
        let cached = path.as_deref().filter(|_| !again).and_then(load);
        let (ffmpeg, driver) = (ffmpeg_version(), driver_key());
        let table = match cached {
            Some(t) if t.ffmpeg == ffmpeg && t.driver == driver => t,
            _ => {
                let dir = path.as_ref().and_then(|p| p.parent()).map_or_else(|| std::env::temp_dir().join("milan-hw"), |d| d.join("hw"));
                let t = probe(&dir, |what| {
                    let mut s = lock();
                    if s.round == round {
                        s.status = what.to_string();
                    }
                });
                if write {
                    if let Some(p) = &path {
                        let _ = std::fs::create_dir_all(p.parent().unwrap_or(Path::new(".")));
                        let _ = serde_json::to_string_pretty(&t).map(|s| std::fs::write(p, s));
                    }
                }
                t
            }
        };
        let mut s = lock();
        if s.round == round {
            s.table = Some(Arc::new(table));
            s.status = String::new();
        }
    });
}

/// The table, once built.
pub fn table() -> Option<Arc<Table>> {
    lock().table.clone()
}

/// Tests: a table in place of the probe's.
#[cfg(test)]
pub fn set_table(t: Option<Table>) {
    let mut s = lock();
    s.round += 1;
    s.table = t.map(Arc::new);
    s.status = String::new();
}

/// Preferences › Video: the decode acceleration ("" Automatic, `OFF`, or a hwaccel) and the encode device ("", `OFF`,
/// or a `DEVICES` suffix).
pub fn set_prefs(decode: &str, encode: &str) {
    let mut s = lock();
    if s.decode_pref != decode {
        s.decode_pref = decode.to_string();
    }
    if s.encode_pref != encode {
        s.encode_pref = encode.to_string();
    }
}

/// Tests: hardware encoders are unavailable whatever the table says, until `force_unavailable(false)`.
static UNAVAILABLE: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
pub fn force_unavailable(on: bool) {
    UNAVAILABLE.store(on, Relaxed);
}

/// A source `w` wide of `codec` is heavy: HEVC, AV1, or 2560 px and wider.
pub fn heavy(codec: Option<&str>, w: u32) -> bool {
    w >= 2560 || matches!(codec, Some("hevc" | "av1"))
}

/// The `-hwaccel` for a source of `codec`, `w` wide, during playback: none when Preferences say Off; the chosen hwaccel
/// when the table says it decodes this codec (heavy sources only until the table is built); with Automatic, heavy
/// sources decode on the hwaccel that measured fastest and faster than software (`auto`, as before R22, until the table
/// is built), and light ones in software.
pub fn decode_hwaccel(codec: Option<&str>, w: u32) -> Option<String> {
    let s = lock();
    let pref = s.decode_pref.as_str();
    if pref == OFF {
        return None;
    }
    let heavy = heavy(codec, w);
    match (&s.table, pref) {
        (None, "") => heavy.then(|| "auto".to_string()),
        (None, h) => heavy.then(|| h.to_string()),
        (Some(t), "") => {
            if !heavy {
                return None;
            }
            match codec {
                Some(c) if CODECS.contains(&c) => t.fastest(c).map(|d| d.hwaccel.clone()),
                // A codec the probe did not test (ProRes at 4K): the hwaccel that did best overall, if any did.
                _ => CODECS.iter().filter_map(|c| t.fastest(c)).min_by(|a, b| a.ms.total_cmp(&b.ms)).map(|d| d.hwaccel.clone()),
            }
        }
        (Some(t), h) => match codec {
            Some(c) if CODECS.contains(&c) => t.decode(h, c).filter(|d| d.ok).map(|_| h.to_string()),
            _ => (heavy && t.decode.iter().any(|d| d.ok && d.hwaccel == h)).then(|| h.to_string()),
        },
    }
}

/// The hardware encoder for `codec` ("h264", "hevc", "av1"): the chosen device's when it passed, or with Automatic the
/// first that passed (NVENC, Quick Sync, AMF, Media Foundation); None when the device is Off, nothing passed or the
/// table is not built yet (the software encoder then renders).
pub fn encoder_for(codec: &str) -> Option<String> {
    if UNAVAILABLE.load(Relaxed) {
        return None;
    }
    let s = lock();
    let t = s.table.as_ref()?;
    match s.encode_pref.as_str() {
        OFF => None,
        "" => DEVICES.iter().map(|d| format!("{codec}_{}", d.0)).find(|e| t.encode(e).is_some_and(|x| x.ok)),
        dev => Some(format!("{codec}_{dev}")).filter(|e| t.encode(e).is_some_and(|x| x.ok)),
    }
}

/// `encoder` (a concrete hardware encoder) passed and hardware encoding is not Off.
pub fn encoder_ok(encoder: &str) -> bool {
    if UNAVAILABLE.load(Relaxed) {
        return false;
    }
    let s = lock();
    s.encode_pref != OFF && s.table.as_ref().is_some_and(|t| t.encode(encoder).is_some_and(|e| e.ok))
}

/// Preferences › Video: one line per test, for the page.
pub fn summary() -> Vec<String> {
    let s = lock();
    let Some(t) = &s.table else { return vec![if s.status.is_empty() { "Hardware acceleration has not been tested.".into() } else { s.status.clone() }] };
    let mut v = Vec::new();
    for h in HWACCELS {
        let rows: Vec<&Decode> = t.decode.iter().filter(|d| d.hwaccel == h).collect();
        if rows.is_empty() {
            if !t.hwaccels.iter().any(|x| x == h) {
                v.push(format!("{h}: not in this FFmpeg build"));
            }
            continue;
        }
        let parts: Vec<String> = rows.iter().map(|d| if d.ok { format!("{} {:.0} ms (software {:.0} ms)", codec_label(&d.codec), d.ms, d.software_ms) } else { format!("{} failed", codec_label(&d.codec)) }).collect();
        v.push(format!("{h}: {}", parts.join(", ")));
    }
    for (dev, label) in DEVICES {
        let rows: Vec<&Encode> = t.encode.iter().filter(|e| e.encoder.ends_with(&format!("_{dev}"))).collect();
        if rows.is_empty() {
            continue;
        }
        let parts: Vec<String> = rows.iter().map(|e| format!("{} {}", codec_label(e.encoder.split('_').next().unwrap_or("")), if e.ok { format!("{:.0} ms", e.ms) } else { "failed".into() })).collect();
        v.push(format!("{label}: {}", parts.join(", ")));
    }
    v.push(format!("Tested {} with {}; drivers: {}.", t.built, t.ffmpeg, if t.driver.is_empty() { "none" } else { t.driver.as_str() }));
    v
}

pub fn codec_label(codec: &str) -> &'static str {
    match codec {
        "h264" => "H.264",
        "hevc" => "HEVC",
        "av1" => "AV1",
        _ => "video",
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// `ffmpeg -hide_banner -hwaccels` of the bundled 9.0.2 build on this machine.
    const HWACCELS_OUT: &str = "Hardware acceleration methods:\ncuda\nvaapi\ndxva2\nqsv\nd3d11va\nopencl\nvulkan\nd3d12va\namf\n";

    /// A slice of `ffmpeg -hide_banner -encoders`: the heading, software and audio rows and the hardware video rows.
    const ENCODERS_OUT: &str = "Encoders:\n V..... = Video\n A..... = Audio\n ------\n V....D av1_d3d12va          D3D12VA av1 encoder (codec av1)\n V....D av1_nvenc            NVIDIA NVENC av1 encoder (codec av1)\n V..... av1_qsv              AV1 (Intel Quick Sync Video acceleration) (codec av1)\n V....D av1_amf              AMD AMF AV1 encoder (codec av1)\n V....D av1_mf               AV1 via MediaFoundation (codec av1)\n V....D h264_amf             AMD AMF H.264 Encoder (codec h264)\n V....D h264_mf              H264 via MediaFoundation (codec h264)\n V....D h264_nvenc           NVIDIA NVENC H.264 encoder (codec h264)\n V..... h264_qsv             H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10 (Intel Quick Sync Video acceleration) (codec h264)\n V....D h264_vaapi           H.264/AVC (VAAPI) (codec h264)\n V....D hevc_amf             AMD AMF HEVC encoder (codec hevc)\n V....D hevc_mf              HEVC via MediaFoundation (codec hevc)\n V....D hevc_nvenc           NVIDIA NVENC hevc encoder (codec hevc)\n V..... hevc_qsv             HEVC (Intel Quick Sync Video acceleration) (codec hevc)\n V..... libx264              libx264 H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10 (codec h264)\n V..... mjpeg_qsv            MJPEG (Intel Quick Sync Video acceleration) (codec mjpeg)\n A....D aac_mf               AAC via MediaFoundation (codec aac)\n A....D mp3_mf               MP3 via MediaFoundation (codec mp3)\n";

    #[test]
    fn parses_hwaccels() {
        assert_eq!(parse_hwaccels(HWACCELS_OUT), ["cuda", "vaapi", "dxva2", "qsv", "d3d11va", "opencl", "vulkan", "d3d12va", "amf"]);
        assert!(parse_hwaccels("ffmpeg: command not found").is_empty());
        assert!(parse_hwaccels("Hardware acceleration methods:\n").is_empty());
    }

    #[test]
    fn parses_hardware_encoders() {
        let v = parse_encoders(ENCODERS_OUT);
        assert_eq!(v, ["av1_nvenc", "av1_qsv", "av1_amf", "av1_mf", "h264_amf", "h264_mf", "h264_nvenc", "h264_qsv", "hevc_amf", "hevc_mf", "hevc_nvenc", "hevc_qsv"]);
        // Software, audio, vaapi (no Windows device) and mjpeg rows are not hardware video encoders of a tested family.
        for n in ["libx264", "aac_mf", "h264_vaapi", "mjpeg_qsv", "av1_d3d12va"] {
            assert!(!is_hardware_encoder(n), "{n}");
        }
        assert_eq!(family("hevc_amf"), Some(("hevc", "amf")));
        assert_eq!((software_for("h264"), software_for("hevc"), software_for("av1")), ("libx264", "libx265", "libsvtav1"));
    }

    /// A table as this machine's probe gives it (an AMD iGPU: d3d11va decodes, AMF and MF encode, nothing NVIDIA or Intel).
    pub(crate) fn amd_table() -> Table {
        let dec = |h: &str, c: &str, ok: bool, ms: f64, sw: f64| Decode { hwaccel: h.into(), codec: c.into(), ok, ms, software_ms: sw, error: if ok { String::new() } else { "failed".into() } };
        let enc = |e: &str, ok: bool| Encode { encoder: e.into(), ok, ms: 120.0, error: String::new() };
        Table {
            ffmpeg: "ffmpeg version 9.0.2".into(),
            driver: "AMD Radeon Graphics (radv 25.0)".into(),
            built: "2026-11-26 10:00:00".into(),
            hwaccels: parse_hwaccels(HWACCELS_OUT),
            encoders: parse_encoders(ENCODERS_OUT),
            decode: vec![
                dec("d3d11va", "h264", true, 90.0, 110.0),
                dec("d3d11va", "hevc", true, 80.0, 180.0),
                dec("d3d11va", "av1", true, 200.0, 150.0), // works, but slower than software
                dec("cuda", "h264", false, 30.0, 110.0),
                dec("cuda", "hevc", false, 30.0, 180.0),
                dec("cuda", "av1", false, 30.0, 150.0),
                dec("qsv", "h264", false, 30.0, 110.0),
                dec("qsv", "hevc", false, 30.0, 180.0),
                dec("qsv", "av1", false, 30.0, 150.0),
            ],
            encode: vec![enc("h264_nvenc", false), enc("hevc_nvenc", false), enc("av1_nvenc", false), enc("h264_qsv", false), enc("hevc_qsv", false), enc("av1_qsv", false), enc("h264_amf", true), enc("hevc_amf", true), enc("av1_amf", false), enc("h264_mf", true), enc("hevc_mf", true), enc("av1_mf", false)],
        }
    }

    /// The choices and the table together decide the decoder path and the encoder; other tests touch the same state.
    pub(crate) static CHOICE: Mutex<()> = Mutex::new(());

    #[test]
    fn decode_choice_follows_the_table_and_the_preference() {
        let _g = CHOICE.lock().unwrap_or_else(|e| e.into_inner());
        set_table(None);
        set_prefs("", "");
        // No table yet: as before R22, heavy sources get `-hwaccel auto`.
        assert_eq!(decode_hwaccel(Some("hevc"), 1920), Some("auto".into()));
        assert_eq!(decode_hwaccel(Some("h264"), 1920), None);
        assert_eq!(decode_hwaccel(Some("h264"), 3840), Some("auto".into()));
        set_table(Some(amd_table()));
        // Automatic: heavy sources on the hwaccel that beat software; AV1 did not, so it decodes in software.
        assert_eq!(decode_hwaccel(Some("hevc"), 1920), Some("d3d11va".into()));
        assert_eq!(decode_hwaccel(Some("av1"), 1920), None);
        assert_eq!(decode_hwaccel(Some("h264"), 1920), None, "light sources stay in software");
        assert_eq!(decode_hwaccel(Some("h264"), 3840), Some("d3d11va".into()));
        assert_eq!(decode_hwaccel(Some("prores"), 3840), Some("d3d11va".into()));
        // A chosen hwaccel applies to every codec it passed, light ones too; one that failed for the codec is not used.
        set_prefs("d3d11va", "");
        assert_eq!(decode_hwaccel(Some("h264"), 1280), Some("d3d11va".into()));
        assert_eq!(decode_hwaccel(Some("av1"), 1920), Some("d3d11va".into()));
        set_prefs("cuda", "");
        assert_eq!(decode_hwaccel(Some("hevc"), 1920), None);
        set_prefs(OFF, "");
        assert_eq!(decode_hwaccel(Some("hevc"), 3840), None);
        set_prefs("", "");
        set_table(None);
    }

    #[test]
    fn encoder_choice_follows_the_table_and_the_device() {
        let _g = CHOICE.lock().unwrap_or_else(|e| e.into_inner());
        set_table(None);
        set_prefs("", "");
        assert_eq!(encoder_for("h264"), None, "no table: software");
        set_table(Some(amd_table()));
        assert_eq!(encoder_for("h264"), Some("h264_amf".into()), "the first device that passed");
        assert_eq!(encoder_for("av1"), None, "no AV1 encoder passed");
        set_prefs("", "mf");
        assert_eq!(encoder_for("hevc"), Some("hevc_mf".into()));
        set_prefs("", "nvenc");
        assert_eq!(encoder_for("h264"), None, "the chosen device failed");
        assert!(!encoder_ok("h264_nvenc") && encoder_ok("h264_amf"));
        set_prefs("", OFF);
        assert!(encoder_for("h264").is_none() && !encoder_ok("h264_amf"));
        set_prefs("", "");
        force_unavailable(true);
        assert!(encoder_for("h264").is_none() && !encoder_ok("h264_amf"));
        force_unavailable(false);
        let t = amd_table();
        assert_eq!(t.working_hwaccels(), ["d3d11va"]);
        assert_eq!(t.passed_encoders(), ["h264_amf", "hevc_amf", "h264_mf", "hevc_mf"]);
        assert!(summary().iter().any(|l| l.starts_with("d3d11va: H.264 90 ms (software 110 ms)")), "{:?}", summary());
        assert!(summary().iter().any(|l| l == "AMD AMF: H.264 120 ms, HEVC 120 ms, AV1 failed"), "{:?}", summary());
        set_table(None);
    }

    /// Needs ffmpeg and a few seconds. The probe on this machine, printed for `tools/bench.ps1` (BENCH lines) and
    /// docs/PERFORMANCE.md. `cargo test --release -- --ignored --nocapture hw_capability_table`
    #[test]
    #[ignore]
    fn hw_capability_table() {
        let dir = std::env::temp_dir().join("milan-agent").join("hw-probe");
        let t = probe(&dir, |w| println!("  {w}"));
        // For the screenshot harness (`MILAN_HW_TABLE`).
        std::fs::write(dir.join("hw.json"), serde_json::to_string_pretty(&t).unwrap()).unwrap();
        println!("ffmpeg: {}\ndrivers: {}\nhwaccels: {:?}\nencoders built in: {:?}", t.ffmpeg, t.driver, t.hwaccels, t.encoders);
        for d in &t.decode {
            println!("decode {} {}: {} {:.0} ms (software {:.0} ms) {}", d.hwaccel, d.codec, if d.ok { "ok" } else { "FAILED" }, d.ms, d.software_ms, d.error);
            println!("BENCH hwdecode hwaccel={} codec={} ok={} ms={:.0} software_ms={:.0}", d.hwaccel, d.codec, d.ok, d.ms, d.software_ms);
        }
        for e in &t.encode {
            println!("encode {}: {} {:.0} ms {}", e.encoder, if e.ok { "ok" } else { "FAILED" }, e.ms, e.error);
            println!("BENCH hwencode encoder={} ok={} ms={:.0}", e.encoder, e.ok, e.ms);
        }
        assert!(!t.hwaccels.is_empty(), "ffmpeg listed no hwaccels");
        // The software decodes ran: every tested hwaccel has a software time to compare with.
        assert!(t.decode.iter().all(|d| d.software_ms > 0.0));
    }
}
