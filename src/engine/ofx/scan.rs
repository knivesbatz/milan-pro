//! R31: where OFX plug-ins are found and what is known about them. Bundles (`Name.ofx.bundle\Contents\Win64\*.ofx`) in
//! %CommonProgramFiles%\OFX\Plugins, every folder of OFX_PLUGIN_PATH and the user's folders are described by a plug-in
//! host process (never in Milan), and the results kept in %LOCALAPPDATA%\MilanPro\plugins.json by path, size and
//! modification time, so only new or changed binaries are run again.
//! R34a/R35: VST3 modules (`Name.vst3\Contents\x86_64-win\Name.vst3` bundles or single `.vst3` files) and CLAP modules
//! (`.clap` files) in %CommonProgramFiles%\VST3, %LOCALAPPDATA%\Programs\Common\VST3, the CLAP folders alike, and the
//! user's VST folders (Preferences › VST Effects) are scanned the same way into the same cache (`Binary::audio`).
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use super::client::{Conn, describe_timeout};
use super::proto::{AudioDesc, PluginDesc, Reply, Req};

/// Milan Pro's OFX preferences: the folders searched besides the standard ones, and the plug-ins turned off (by id).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OfxPrefs {
    pub folders: Vec<PathBuf>,
    /// R34a: the user's VST3 and CLAP folders (Preferences › VST Effects), searched besides the standard ones.
    pub vst_folders: Vec<PathBuf>,
    /// Turned off by id: `ofx:` plug-ins, and (R34a) `vst3:` and `clap:` effects.
    pub disabled: BTreeSet<String>,
    /// Plug-ins (by id) whose input colour space hint the user dismissed in the FX window.
    pub hints_seen: BTreeSet<String>,
    /// A1: AE-format effects (experimental, off by default) and their folders.
    pub aex: crate::engine::aex::AexPrefs,
}

/// One binary as last described.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Binary {
    pub path: PathBuf,
    pub size: u64,
    /// Modification time, ns since 1970.
    pub mtime: u64,
    #[serde(default)]
    pub plugins: Vec<PluginDesc>,
    /// R34a/R35: the audio effects of a VST3 or CLAP module.
    #[serde(default)]
    pub audio: Vec<AudioDesc>,
    /// Why the binary (or one of its plug-ins) could not be described.
    #[serde(default)]
    pub errors: Vec<(Option<u32>, String)>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScanCache {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub binaries: Vec<Binary>,
}

const VERSION: u32 = 2;

impl ScanCache {
    /// The cache at `path`; empty when missing, damaged or of another version (it is only a cache).
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str::<Self>(&s).ok()).filter(|c| c.version == VERSION).unwrap_or_default()
    }

    /// Written to a temporary file and renamed.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
        let res = std::fs::write(&tmp, serde_json::to_string_pretty(self)?).and_then(|_| std::fs::rename(&tmp, path));
        if res.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        res
    }
}

/// %LOCALAPPDATA%\MilanPro\plugins.json.
pub fn cache_path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("MilanPro").join("plugins.json"))
}

/// The folders searched, in order, each once: the standard OFX folder, OFX_PLUGIN_PATH, then the user's.
pub fn folders(prefs: &OfxPrefs) -> Vec<PathBuf> {
    folders_with(prefs, &|k| std::env::var_os(k).filter(|v| !v.is_empty()))
}

fn folders_with(prefs: &OfxPrefs, env: &dyn Fn(&str) -> Option<std::ffi::OsString>) -> Vec<PathBuf> {
    let common = env("CommonProgramFiles").map_or_else(|| PathBuf::from(r"C:\Program Files\Common Files"), PathBuf::from);
    let mut v = vec![common.join("OFX").join("Plugins")];
    v.extend(env("OFX_PLUGIN_PATH").map(|p| std::env::split_paths(&p).filter(|p| !p.as_os_str().is_empty()).collect::<Vec<_>>()).unwrap_or_default());
    v.extend(prefs.folders.iter().cloned());
    let mut seen = BTreeSet::new();
    v.retain(|p| seen.insert(p.to_string_lossy().trim_end_matches('\\').to_lowercase()));
    v
}

/// R34a: the standard VST3 and CLAP folders, then the user's VST folders, each once.
pub fn audio_folders(prefs: &OfxPrefs) -> Vec<PathBuf> {
    audio_folders_with(prefs, &|k| std::env::var_os(k).filter(|v| !v.is_empty()))
}

fn audio_folders_with(prefs: &OfxPrefs, env: &dyn Fn(&str) -> Option<std::ffi::OsString>) -> Vec<PathBuf> {
    let common = env("CommonProgramFiles").map_or_else(|| PathBuf::from(r"C:\Program Files\Common Files"), PathBuf::from);
    let local = env("LOCALAPPDATA").map(|l| PathBuf::from(l).join("Programs").join("Common"));
    let mut v = vec![common.join("VST3")];
    v.extend(local.as_ref().map(|l| l.join("VST3")));
    v.push(common.join("CLAP"));
    v.extend(local.as_ref().map(|l| l.join("CLAP")));
    v.extend(prefs.vst_folders.iter().cloned());
    let mut seen = BTreeSet::new();
    v.retain(|p| seen.insert(p.to_string_lossy().trim_end_matches('\\').to_lowercase()));
    v
}

/// A binary's extension says what hosts it: "ofx", "vst3", "clap" or (A1) "aex".
pub fn kind_of(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("vst3") => "vst3",
        Some("clap") => "clap",
        Some("aex") => "aex",
        _ => "ofx",
    }
}

/// The 64-bit binaries of every `*.ofx.bundle` under `dirs` (bundles up to 4 folders deep; a bundle is not entered
/// further), every `*.vst3` module (a bundle's `Contents\x86_64-win\*.vst3`, or a single file) and every `*.clap`
/// file, sorted, each once.
pub fn find_binaries(dirs: &[PathBuf]) -> Vec<PathBuf> {
    fn files(dir: &Path, ext: &str) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|b| b.is_file() && b.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext))).collect()).unwrap_or_default();
        v.sort();
        v
    }
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut all: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        all.sort();
        for p in all {
            let name = p.file_name().map_or(String::new(), |n| n.to_string_lossy().to_lowercase());
            if p.is_file() {
                if name.ends_with(".vst3") || name.ends_with(".clap") {
                    out.push(p);
                }
            } else if name.ends_with(".ofx.bundle") {
                out.extend(files(&p.join("Contents").join("Win64"), "ofx"));
            } else if name.ends_with(".vst3") {
                out.extend(files(&p.join("Contents").join("x86_64-win"), "vst3"));
            } else if depth > 1 {
                walk(&p, depth - 1, out);
            }
        }
    }
    let mut out = Vec::new();
    for d in dirs {
        walk(d, 4, &mut out);
    }
    let mut seen = BTreeSet::new();
    out.retain(|p| seen.insert(p.to_string_lossy().to_lowercase()));
    out
}

fn stamp(p: &Path) -> (u64, u64) {
    std::fs::metadata(p).map_or((0, 0), |m| (m.len(), m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos() as u64)))
}

/// Describe the binaries under `dirs`: a binary whose path, size and date match `old` keeps its entry unless `force`;
/// the others go through `describe`.
#[cfg(test)]
pub fn scan(dirs: &[PathBuf], old: &ScanCache, force: bool, describe: &dyn Fn(&Path) -> Binary) -> ScanCache {
    scan_all(dirs, &[], old, force, describe)
}

/// `scan`, with (A1) the AE-format effects under `aex_dirs` (empty while that feature is off).
pub fn scan_all(dirs: &[PathBuf], aex_dirs: &[PathBuf], old: &ScanCache, force: bool, describe: &dyn Fn(&Path) -> Binary) -> ScanCache {
    let binaries = find_binaries(dirs)
        .into_iter()
        .chain(crate::engine::aex::find(aex_dirs))
        .map(|path| {
            let (size, mtime) = stamp(&path);
            match old.binaries.iter().find(|b| !force && b.path == path && b.size == size && b.mtime == mtime) {
                Some(b) => b.clone(),
                None => Binary { path: path.clone(), size, mtime, ..describe(&path) },
            }
        })
        .collect();
    ScanCache { version: VERSION, binaries }
}

/// Describe every plug-in of `binary` in a plug-in host of its own (closed afterwards). A crash or a hang costs the
/// binary (or that plug-in) only, recorded as its error.
pub fn describe_binary(binary: &Path) -> Binary {
    if kind_of(binary) == "aex" {
        return crate::engine::aex::describe_binary(binary);
    }
    let mut out = Binary { path: binary.to_path_buf(), ..Default::default() };
    let (mut conn, n) = match Conn::start(binary) {
        Ok(c) => c,
        Err(e) => {
            out.errors.push((None, e.to_string()));
            return out;
        }
    };
    if kind_of(binary) != "ofx" {
        // R34a/R35: a VST3 or CLAP module: every effect at once (each is set up once, so the scan gets the load time).
        match conn.request(&Req::AScan, Some(super::client::LOAD_TIMEOUT)) {
            Ok(Reply::AScanned(v)) => out.audio = v,
            Ok(Reply::Error(e)) => out.errors.push((None, e)),
            Ok(r) => out.errors.push((None, format!("unexpected reply {r:?}"))),
            Err(e) => out.errors.push((None, format!("scanning: {e}"))),
        }
        if let Some(info) = moduleinfo(binary) {
            // moduleinfo.json names the classes the module says it has: an effect the factory did not list is noted.
            for (cid, name) in info {
                if !out.audio.iter().any(|a| a.id.eq_ignore_ascii_case(&cid)) {
                    out.errors.push((None, format!("moduleinfo.json lists \"{name}\" ({cid}), which the module's factory does not")));
                }
            }
        }
        conn.quit();
        return out;
    }
    for i in 0..n {
        match conn.request(&Req::Describe { index: i }, Some(describe_timeout())) {
            Ok(Reply::Described(d)) => out.plugins.push(*d),
            Ok(Reply::Error(e)) => out.errors.push((Some(i), e)),
            Ok(r) => out.errors.push((Some(i), format!("unexpected reply {r:?}"))),
            Err(e) => {
                out.errors.push((Some(i), format!("describing plug-in {i}: {e}")));
                // A crashed or killed host: the rest are described in a new one.
                match Conn::start(binary) {
                    Ok((c, _)) => conn = c,
                    Err(e) => {
                        out.errors.push((None, e.to_string()));
                        return out;
                    }
                }
            }
        }
    }
    conn.quit();
    out
}

/// R34a: a VST3 bundle's `Contents\Resources\moduleinfo.json` (VST 3.7.5+): the (class id, name) of every "Audio Module
/// Class" it lists, when present and readable. The factory stays the source of truth; this only cross-checks it.
pub fn moduleinfo(binary: &Path) -> Option<Vec<(String, String)>> {
    let contents = binary.parent()?.parent()?;
    if !contents.file_name().is_some_and(|n| n == "Contents") {
        return None;
    }
    let text = std::fs::read_to_string(contents.join("Resources").join("moduleinfo.json")).ok()?;
    // The SDK writes JSON5-ish (comments, trailing commas): strip them before parsing.
    let mut clean = String::new();
    for line in text.lines() {
        let l = match line.find("//") {
            Some(i) if !line[..i].contains('"') => &line[..i],
            _ => line,
        };
        clean.push_str(l.trim_end());
        clean.push('\n');
    }
    let clean = clean.replace(",\n]", "\n]").replace(",\n}", "\n}");
    let mut clean2 = clean.clone();
    // A trailing comma before a closing bracket on a later line, with indentation between.
    loop {
        let before = clean2.len();
        clean2 = clean2.replace(",\n ", ",\n").replace(",\n\n", ",\n");
        if clean2.len() == before {
            break;
        }
    }
    let clean2 = clean2.replace(",\n]", "\n]").replace(",\n}", "\n}");
    let v: serde_json::Value = serde_json::from_str(&clean2).ok()?;
    let classes = v.get("Classes")?.as_array()?;
    Some(
        classes
            .iter()
            .filter(|c| c.get("Category").and_then(|x| x.as_str()) == Some("Audio Module Class"))
            .filter_map(|c| Some((c.get("CID")?.as_str()?.to_string(), c.get("Name").and_then(|x| x.as_str()).unwrap_or_default().to_string())))
            .collect(),
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::RefCell;

    pub fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join("milan-agent").join("ofx").join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn folders_standard_env_and_users() {
        let env = |k: &str| match k {
            "CommonProgramFiles" => Some(r"C:\CF".into()),
            "OFX_PLUGIN_PATH" => Some(r"D:\ofx;E:\more;".into()),
            _ => None,
        };
        let p = OfxPrefs { folders: vec![r"d:\OFX\".into(), r"F:\mine".into()], vst_folders: vec![r"G:\vst".into(), r"c:\cf\VST3".into()], ..Default::default() };
        assert_eq!(folders_with(&p, &env), [r"C:\CF\OFX\Plugins", r"D:\ofx", r"E:\more", r"F:\mine"].map(PathBuf::from));
        // R34a: the VST3 and CLAP folders, the user's after them (one of theirs is a standard one spelt differently).
        let env2 = |k: &str| match k {
            "CommonProgramFiles" => Some(r"C:\CF".into()),
            "LOCALAPPDATA" => Some(r"C:\U\AppData\Local".into()),
            _ => None,
        };
        assert_eq!(audio_folders_with(&p, &env2), [r"C:\CF\VST3", r"C:\U\AppData\Local\Programs\Common\VST3", r"C:\CF\CLAP", r"C:\U\AppData\Local\Programs\Common\CLAP", r"G:\vst"].map(PathBuf::from));
    }

    /// R34a: VST3 bundles, single-file VST3 modules and CLAP files are found beside OFX bundles; moduleinfo.json is read.
    #[test]
    fn finds_vst3_and_clap_modules_and_reads_moduleinfo() {
        let d = temp("audio-scan");
        let bundle = d.join("Thing.vst3");
        let bin = bundle.join("Contents").join("x86_64-win");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(bundle.join("Contents").join("Resources")).unwrap();
        std::fs::write(bin.join("Thing.vst3"), b"dll").unwrap();
        let info = "{\n  \"Name\": \"Thing\",\n  // a comment\n  \"Classes\": [\n    {\n      \"CID\": \"ABCDEF0123456789ABCDEF0123456789\",\n      \"Category\": \"Audio Module Class\",\n      \"Name\": \"Thing\",\n    },\n    {\n      \"CID\": \"00000000000000000000000000000001\",\n      \"Category\": \"Component Controller Class\",\n      \"Name\": \"Thing Controller\",\n    },\n  ],\n}\n";
        std::fs::write(bundle.join("Contents").join("Resources").join("moduleinfo.json"), info).unwrap();
        std::fs::write(d.join("Single.vst3"), b"dll").unwrap();
        std::fs::create_dir_all(d.join("Vendor")).unwrap();
        std::fs::write(d.join("Vendor").join("Effect.clap"), b"dll").unwrap();
        std::fs::write(d.join("readme.txt"), b"x").unwrap();
        let found = find_binaries(&[d.clone()]);
        assert_eq!(found, [d.join("Single.vst3"), bin.join("Thing.vst3"), d.join("Vendor").join("Effect.clap")]);
        assert_eq!(found.iter().map(|p| kind_of(p)).collect::<Vec<_>>(), ["vst3", "vst3", "clap"]);
        assert_eq!(moduleinfo(&bin.join("Thing.vst3")), Some(vec![("ABCDEF0123456789ABCDEF0123456789".to_string(), "Thing".to_string())]));
        assert_eq!(moduleinfo(&d.join("Single.vst3")), None);
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// R31 acceptance: the scan cache is reused while a binary's size and date stand; a new date describes it again;
    /// a forced rescan describes everything; the cache round-trips through its file and a damaged one is empty.
    #[test]
    fn scan_cache_invalidates_on_mtime_change() {
        let d = temp("scan");
        let bin = |root: &Path, name: &str| {
            let w = root.join(format!("{name}.ofx.bundle")).join("Contents").join("Win64");
            std::fs::create_dir_all(&w).unwrap();
            std::fs::create_dir_all(root.join(format!("{name}.ofx.bundle")).join("Contents").join("Resources")).unwrap();
            let f = w.join(format!("{name}.ofx"));
            std::fs::write(&f, b"not really a dll").unwrap();
            f
        };
        let a = bin(&d, "Alpha");
        let b = bin(&d.join("Vendor"), "Beta");
        std::fs::write(d.join("Alpha.ofx.bundle").join("Contents").join("Win64").join("readme.txt"), b"x").unwrap();
        let calls = RefCell::new(Vec::new());
        let describe = |p: &Path| {
            calls.borrow_mut().push(p.to_path_buf());
            Binary { plugins: vec![PluginDesc { id: "x".into(), ..Default::default() }], ..Default::default() }
        };
        let dirs = [d.clone(), d.clone()];
        let first = scan(&dirs, &ScanCache::default(), false, &describe);
        assert_eq!(first.binaries.iter().map(|b| b.path.clone()).collect::<Vec<_>>(), [a.clone(), b.clone()]);
        assert_eq!(calls.borrow().len(), 2);
        let second = scan(&dirs, &first, false, &describe);
        assert_eq!((second.clone(), calls.borrow().len()), (first.clone(), 2), "unchanged binaries come from the cache");
        let later = std::fs::metadata(&a).unwrap().modified().unwrap() + std::time::Duration::from_secs(60);
        std::fs::File::options().write(true).open(&a).unwrap().set_modified(later).unwrap();
        let third = scan(&dirs, &second, false, &describe);
        assert_eq!(calls.borrow().last(), Some(&a), "a new date describes it again");
        assert_eq!(calls.borrow().len(), 3);
        assert_ne!(third.binaries[0].mtime, second.binaries[0].mtime);
        scan(&dirs, &third, true, &describe);
        assert_eq!(calls.borrow().len(), 5, "a forced rescan describes everything");
        let file = d.join("plugins.json");
        third.save(&file).unwrap();
        assert_eq!(ScanCache::load(&file), third);
        std::fs::write(&file, "{ damaged").unwrap();
        assert_eq!(ScanCache::load(&file), ScanCache::default());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
