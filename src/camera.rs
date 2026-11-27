//! R41: professional camera formats. A camera card's folder structure (AVCHD, XDCAM EX/HD, XAVC, P2, Canon XF,
//! GoPro, plain DCIM) dropped or picked as a folder is scanned for its clips, and spanned recordings (an AVCHD `.MTS`
//! series whose timestamps run on, GoPro chapter files) are joined into one media item through ffmpeg's concat
//! demuxer: a `.ffconcat` list beside the first file, which every reader (ffprobe, the video decoders, the mixer,
//! proxies) opens like any other file. RAW camera formats (BRAW, R3D, ARRIRAW, CinemaDNG) import as offline items
//! that say why (`RAW`).
use std::path::{Path, PathBuf};

/// RAW camera formats Milan Pro cannot decode: (extension, name, reason).
pub static RAW: &[(&str, &str, &str)] = &[
    ("braw", "Blackmagic RAW", "needs the Blackmagic RAW SDK, whose licence forbids redistribution; FFmpeg has no decoder"),
    ("r3d", "RED R3D", "needs the RED R3D SDK (proprietary); FFmpeg has no decoder"),
    ("ari", "ARRIRAW", "needs the ARRI Image SDK (proprietary); FFmpeg has no decoder"),
    ("arx", "ARRIRAW", "needs the ARRI Image SDK (proprietary); FFmpeg has no decoder"),
    ("crm", "Canon Cinema RAW Light", "needs Canon's Cinema RAW Development SDK; FFmpeg has no decoder"),
    ("nev", "Nikon N-RAW", "needs Nikon's SDK; FFmpeg has no decoder"),
];

/// The reason a RAW camera file is offline, by its extension (lower case).
pub fn raw_reason(ext: &str) -> Option<String> {
    RAW.iter().find(|r| r.0 == ext).map(|(_, name, why)| format!("unsupported format {name}: {why}"))
}

/// Folder layouts recognised: (name, the glob-like path of the clips below the card root, as components; `*` is any
/// folder). The first component may also be the folder the user picked itself.
pub static LAYOUTS: &[(&str, &[&str])] = &[
    ("AVCHD", &["PRIVATE", "AVCHD", "BDMV", "STREAM"]),
    ("AVCHD", &["AVCHD", "BDMV", "STREAM"]),
    ("AVCHD", &["BDMV", "STREAM"]),
    ("XDCAM EX", &["BPAV", "CLPR", "*"]),
    ("XDCAM HD / XAVC", &["XDROOT", "Clip"]),
    ("XDCAM HD / XAVC", &["Clip"]),
    ("P2", &["CONTENTS", "VIDEO"]),
    ("P2 (audio essence)", &["CONTENTS", "AUDIO"]),
    ("Canon XF", &["CONTENTS", "CLIPS001", "*"]),
    ("GoPro / DCIM", &["DCIM", "*"]),
];

/// A clip found on a card: its files in play order (one, or the spanned parts) and the layout it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub files: Vec<PathBuf>,
    pub layout: &'static str,
}

fn ext_of(p: &Path) -> String {
    p.extension().unwrap_or_default().to_string_lossy().to_lowercase()
}

/// Media files directly in `dir` (not folders), sorted by name, upper-case names included.
fn media_in(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && crate::media::classify_extension(&ext_of(p)) != crate::project::MediaType::Unknown)
        .collect();
    v.sort_by_key(|p| p.file_name().unwrap_or_default().to_string_lossy().to_uppercase());
    v
}

/// The folders `pattern` names below `root` (case-insensitive; `*` is every subfolder), also when `root` is already
/// inside the pattern (the user picked the STREAM folder itself).
fn expand(root: &Path, pattern: &[&str]) -> Vec<PathBuf> {
    fn step(dir: &Path, rest: &[&str], out: &mut Vec<PathBuf>) {
        let Some((first, rest)) = rest.split_first() else {
            out.push(dir.to_path_buf());
            return;
        };
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_dir() {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            if *first == "*" || name.eq_ignore_ascii_case(first) {
                step(&p, rest, out);
            }
        }
    }
    let mut out = Vec::new();
    // The root may be any folder of the pattern: try it as the card root, then as each later component.
    // Picked inside the layout: the root and its parents must spell out the whole head of the pattern (a `*` alone
    // says nothing, so any folder is not a GoPro folder because `DCIM/*` ends in `*`).
    let inside = |k: usize| {
        let mut dir = Some(root);
        pattern[..k].iter().rev().all(|c| {
            let name = dir.and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned());
            dir = dir.and_then(Path::parent);
            name.is_some_and(|n| *c == "*" || n.eq_ignore_ascii_case(c))
        }) && pattern[..k].iter().any(|c| *c != "*")
    };
    for k in 0..=pattern.len() {
        if k == 0 || inside(k) {
            step(root, &pattern[k..], &mut out);
        }
    }
    out.sort();
    out.dedup();
    out
}

/// ffprobe's start time and duration of a transport stream part (for span detection).
fn ts_bounds(p: &Path) -> Option<(f64, f64)> {
    let mut cmd = crate::proc::ffprobe();
    cmd.args(["-v", "error", "-show_entries", "format=start_time,duration", "-of", "csv=p=0"]).arg(p);
    let out = crate::proc::output(&mut cmd).ok().filter(|o| o.status.success())?;
    let line = String::from_utf8_lossy(&out.stdout);
    let mut it = line.trim().split(',');
    Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
}

/// Spanned groups among `files` (one folder, sorted): transport-stream parts whose timestamps run on from the previous
/// part (within `tol` seconds; AVCHD splits at 2 GB without resetting the clock, a new recording restarts near 1.4 s),
/// and GoPro chapters (`GX01xxxx.MP4`, `GX02xxxx.MP4`: the same number, chapters 01, 02...). `bounds` reads a part's
/// (start, duration), so tests can fake it. Each file's (start, duration) comes back with the groups for the list.
pub fn spans(files: &[PathBuf], bounds: &dyn Fn(&Path) -> Option<(f64, f64)>, tol: f64) -> Vec<Vec<(PathBuf, f64)>> {
    let mut groups: Vec<Vec<(PathBuf, f64)>> = Vec::new();
    let mut prev_end: Option<f64> = None;
    let mut gopro: Vec<(String, u32, usize)> = Vec::new(); // (number, chapter, group index)
    for f in files {
        let ext = ext_of(f);
        let name = f.file_stem().unwrap_or_default().to_string_lossy().to_uppercase();
        if matches!(ext.as_str(), "mts" | "m2ts" | "ts" | "m2t") {
            let (start, dur) = bounds(f).unwrap_or((0.0, 0.0));
            let joins = prev_end.is_some_and(|e| dur > 0.0 && (start - e).abs() <= tol) && groups.last().is_some_and(|g| matches!(ext_of(&g[0].0).as_str(), "mts" | "m2ts" | "ts" | "m2t"));
            if joins {
                groups.last_mut().expect("a group").push((f.clone(), dur));
            } else {
                groups.push(vec![(f.clone(), dur)]);
            }
            prev_end = (dur > 0.0).then_some(start + dur);
            continue;
        }
        prev_end = None;
        // GoPro: GX01 0001 (HEVC), GH01 0001 (H.264), GP01 0001 (older); chapter then number.
        let chapter = (name.len() == 8 && ext == "mp4" && (name.starts_with("GX") || name.starts_with("GH") || name.starts_with("GP")) && name[2..].chars().all(|c| c.is_ascii_digit()))
            .then(|| (name[4..].to_string(), name[2..4].parse::<u32>().unwrap_or(0)));
        match chapter {
            Some((number, ch)) if gopro.iter().any(|g| g.0 == number) => {
                let g = gopro.iter().find(|g| g.0 == number).expect("found").2;
                let dur = bounds(f).map_or(0.0, |b| b.1);
                groups[g].push((f.clone(), dur));
                let _ = ch;
            }
            Some((number, ch)) => {
                let dur = bounds(f).map_or(0.0, |b| b.1);
                groups.push(vec![(f.clone(), dur)]);
                gopro.push((number, ch, groups.len() - 1));
            }
            None => groups.push(vec![(f.clone(), 0.0)]),
        }
    }
    groups
}

/// A file name ffmpeg's concat demuxer accepts in its default safe mode: letters, digits, `.`, `_`, `-`, not
/// starting with a `.`.
pub fn safe_name(p: &Path) -> bool {
    let n = p.file_name().unwrap_or_default().to_string_lossy();
    !n.is_empty() && !n.starts_with('.') && n.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// The `.ffconcat` list text for `parts` (file names relative to the list's folder, with their durations so the
/// total length is known at open).
pub fn concat_list(parts: &[(PathBuf, f64)]) -> String {
    let mut s = String::from("ffconcat version 1.0\n");
    for (p, dur) in parts {
        s += &format!("file {}\n", p.file_name().unwrap_or_default().to_string_lossy());
        if *dur > 0.0 {
            s += &format!("duration {dur:.6}\n");
        }
    }
    s
}

/// The list file's path for a span starting with `first`: `<first stem>-span.ffconcat` beside it.
pub fn list_path(first: &Path) -> PathBuf {
    first.with_file_name(format!("{}-span.ffconcat", first.file_stem().unwrap_or_default().to_string_lossy()))
}

/// Write (or refresh) the list for a span and give its path; None when the folder cannot be written (a read-only
/// card): the parts then import one by one.
pub fn write_span(parts: &[(PathBuf, f64)]) -> Option<PathBuf> {
    let list = list_path(&parts[0].0);
    let text = concat_list(parts);
    if std::fs::read_to_string(&list).ok().as_deref() != Some(&text) {
        std::fs::write(&list, &text).ok()?;
    }
    Some(list)
}

/// The files a `.ffconcat` list joins (absolute, beside the list), for `MediaItem::spanned`.
pub fn list_members(list: &Path) -> Vec<PathBuf> {
    let dir = list.parent().unwrap_or(Path::new("."));
    std::fs::read_to_string(list)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.strip_prefix("file "))
        .map(|f| dir.join(f.trim().trim_matches('\'')))
        .collect()
}

/// The clips of camera folder `root` (a card root, or any folder of a known layout; else the media files in the
/// folder itself and one level down), spanned parts grouped. `bounds` reads a transport stream part's (start,
/// duration): `ts_bounds` (ffprobe) in the app, a fake in tests.
pub fn scan_with(root: &Path, bounds: &dyn Fn(&Path) -> Option<(f64, f64)>) -> Vec<Found> {
    let mut out = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    for (layout, pattern) in LAYOUTS {
        for dir in expand(root, pattern) {
            if seen.contains(&dir) {
                continue;
            }
            seen.push(dir.clone());
            for g in spans(&media_in(&dir), bounds, 0.1) {
                out.push(Found { files: g.into_iter().map(|(p, _)| p).collect(), layout });
            }
        }
    }
    if out.is_empty() {
        // Not a known card: the folder's own media, and one level down.
        let mut dirs = vec![root.to_path_buf()];
        dirs.extend(std::fs::read_dir(root).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir() && !p.file_name().unwrap_or_default().to_string_lossy().starts_with('.')));
        for dir in dirs {
            for g in spans(&media_in(&dir), bounds, 0.1) {
                out.push(Found { files: g.into_iter().map(|(p, _)| p).collect(), layout: "folder" });
            }
        }
    }
    out
}

/// The paths to import for camera folder `root`: one `.ffconcat` per joined span (written beside its first part),
/// else the files themselves. `notes` gets what could not be joined.
pub fn import_paths(root: &Path, notes: &mut Vec<String>) -> Vec<PathBuf> {
    import_paths_with(root, &ts_bounds, notes)
}

pub fn import_paths_with(root: &Path, bounds: &dyn Fn(&Path) -> Option<(f64, f64)>, notes: &mut Vec<String>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for f in scan_with(root, bounds) {
        if f.files.len() < 2 {
            paths.extend(f.files);
            continue;
        }
        // The concat demuxer's safe mode (which every reader opens the list with) takes plain names only; camera
        // cards name their files that way.
        if let Some(bad) = f.files.iter().find(|p| !safe_name(p)) {
            notes.push(format!("{}: not joined with the parts after it (only letters, digits, '.', '_' and '-' can be joined)", bad.display()));
            paths.extend(f.files);
            continue;
        }
        let parts: Vec<(PathBuf, f64)> = f.files.iter().map(|p| (p.clone(), bounds(p).map_or(0.0, |b| b.1))).collect();
        match write_span(&parts) {
            Some(list) => paths.push(list),
            None => {
                notes.push(format!("{}: {} spanned parts could not be joined (the folder is read-only); imported one by one", f.files[0].display(), f.files.len()));
                paths.extend(f.files);
            }
        }
    }
    paths
}

/// Camera folder layouts and their support, for docs/FORMAT_SUPPORT.md (written by `formats_e2e::format_corpus_e2e`).
#[cfg_attr(not(test), allow(dead_code))]
pub static SUPPORT: &[(&str, &str)] = &[
    ("AVCHD (PRIVATE/AVCHD/BDMV/STREAM/*.MTS)", "imported as clips; parts whose timestamps run on (a recording split at 2 GB) join into one item through a .ffconcat list beside the first part; timecode from the H.264 SEI when present"),
    ("XDCAM EX (BPAV/CLPR/*/*.MP4)", "each clip imports; spans are not joined (the .SMI/XML links are not read)"),
    ("XDCAM HD / XDCAM HD422 (Clip/*.MXF, MPEG-2 4:2:2 in MXF OP1a)", "decodes; MXF timecode, reel (material package) and company/product tags read"),
    ("XAVC / XAVC Intra / XAVC-S (MXF OP1a or MP4, H.264/HEVC)", "decodes; MXF or QuickTime timecode read"),
    ("P2 (CONTENTS/VIDEO/*.MXF, AVC-Intra or DVCPRO HD, MXF OP-Atom)", "the picture and each audio essence file import as separate items (Milan Pro has no multi-file item); drop them together and group them"),
    ("Canon XF (CONTENTS/CLIPS001/*/*.MXF, MPEG-2 4:2:2)", "decodes; spans are not joined"),
    ("GoPro (DCIM/100GOPRO/GX01xxxx.MP4 ...)", "chapters of one recording join into one item; the GPMF data stream is skipped (-dn)"),
    ("Sony / Panasonic / Canon MP4 and MOV", "decode through FFmpeg; make/model and timecode tags read"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    /// AVCHD parts whose clock runs on join; a part that restarts its clock starts a new clip; GoPro chapters join by
    /// number; other files stand alone.
    #[test]
    fn spans_by_timestamps_and_gopro_chapters() {
        let files = [p(r"C:\c\00000.MTS"), p(r"C:\c\00001.MTS"), p(r"C:\c\00002.MTS"), p(r"C:\c\00003.MTS"), p(r"C:\c\GX010001.MP4"), p(r"C:\c\GX020001.MP4"), p(r"C:\c\GX010002.MP4"), p(r"C:\c\clip.mov")];
        let bounds = |f: &Path| -> Option<(f64, f64)> {
            match f.file_name()?.to_str()? {
                "00000.MTS" => Some((1.4, 2.0)),
                "00001.MTS" => Some((3.4, 3.0)),
                "00002.MTS" => Some((6.4, 1.0)),
                "00003.MTS" => Some((1.4, 2.5)),
                "GX010001.MP4" => Some((0.0, 4.0)),
                "GX020001.MP4" => Some((0.0, 1.5)),
                _ => Some((0.0, 1.0)),
            }
        };
        let g = spans(&files, &bounds, 0.1);
        let names: Vec<Vec<String>> = g.iter().map(|g| g.iter().map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned()).collect()).collect();
        assert_eq!(names, [vec!["00000.MTS", "00001.MTS", "00002.MTS"], vec!["00003.MTS"], vec!["GX010001.MP4", "GX020001.MP4"], vec!["GX010002.MP4"], vec!["clip.mov"]]);
        assert_eq!(concat_list(&g[0]), "ffconcat version 1.0\nfile 00000.MTS\nduration 2.000000\nfile 00001.MTS\nduration 3.000000\nfile 00002.MTS\nduration 1.000000\n");
        assert_eq!(list_path(&g[0][0].0), p(r"C:\c\00000-span.ffconcat"));
        assert_eq!(raw_reason("braw").unwrap(), "unsupported format Blackmagic RAW: needs the Blackmagic RAW SDK, whose licence forbids redistribution; FFmpeg has no decoder");
        assert!(raw_reason("r3d").is_some() && raw_reason("ari").is_some() && raw_reason("mp4").is_none());
        assert!(safe_name(&p(r"C:\c d\00001.MTS")) && safe_name(&p("GX010001.MP4")) && !safe_name(&p("my clip.mts")) && !safe_name(&p(".hidden.mts")));
    }

    /// A made-up card: the AVCHD STREAM folder found from the card root and from the folder itself, the clips of a
    /// plain folder, and a read-only (made-up) write failure falling back to the parts.
    #[test]
    fn card_layouts_are_found() {
        let dir = std::env::temp_dir().join(format!("milan-cam-{}", std::process::id()));
        let stream = dir.join("PRIVATE").join("AVCHD").join("BDMV").join("STREAM");
        let dcim = dir.join("DCIM").join("100GOPRO");
        std::fs::create_dir_all(&stream).unwrap();
        std::fs::create_dir_all(&dcim).unwrap();
        for n in ["00000.MTS", "00001.MTS", "00002.MTS"] {
            std::fs::write(stream.join(n), b"x").unwrap();
        }
        std::fs::write(stream.join("notes.txt"), b"x").unwrap();
        for n in ["GX010001.MP4", "GX020001.MP4", "GX010002.MP4"] {
            std::fs::write(dcim.join(n), b"x").unwrap();
        }
        let bounds = |f: &Path| -> Option<(f64, f64)> {
            match f.file_name()?.to_str()? {
                "00000.MTS" => Some((1.4, 2.0)),
                "00001.MTS" => Some((3.4, 3.0)),
                "00002.MTS" => Some((1.4, 1.0)),
                _ => Some((0.0, 1.0)),
            }
        };
        let found = scan_with(&dir, &bounds);
        let sizes: Vec<(usize, &str)> = found.iter().map(|f| (f.files.len(), f.layout)).collect();
        assert_eq!(sizes, [(2, "AVCHD"), (1, "AVCHD"), (2, "GoPro / DCIM"), (1, "GoPro / DCIM")]);
        // The STREAM folder itself as the root finds the same clips.
        assert_eq!(scan_with(&stream, &bounds).len(), 2);
        let mut notes = Vec::new();
        let paths = import_paths_with(&dir, &bounds, &mut notes);
        assert_eq!(paths.len(), 4);
        assert!(paths[0].ends_with("00000-span.ffconcat") && paths[2].ends_with("GX010001-span.ffconcat") && notes.is_empty());
        assert_eq!(std::fs::read_to_string(&paths[0]).unwrap(), "ffconcat version 1.0\nfile 00000.MTS\nduration 2.000000\nfile 00001.MTS\nduration 3.000000\n");
        assert_eq!(list_members(&paths[0]), [stream.join("00000.MTS"), stream.join("00001.MTS")]);
        // A plain folder: its media files, none joined.
        let plain = dir.join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        std::fs::write(plain.join("b.mp4"), b"x").unwrap();
        std::fs::write(plain.join("a.wav"), b"x").unwrap();
        let f = scan_with(&plain, &bounds);
        assert_eq!(f.iter().map(|f| f.files[0].file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>(), ["a.wav", "b.mp4"]);
        assert!(f.iter().all(|f| f.layout == "folder"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
