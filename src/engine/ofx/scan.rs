//! R31: where OFX plug-ins are found and what is known about them. Bundles (`Name.ofx.bundle\Contents\Win64\*.ofx`) in
//! %CommonProgramFiles%\OFX\Plugins, every folder of OFX_PLUGIN_PATH and the user's folders are described by a plug-in
//! host process (never in Milan), and the results kept in %LOCALAPPDATA%\MilanPro\plugins.json by path, size and
//! modification time, so only new or changed binaries are run again.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use super::client::{Conn, DESCRIBE_TIMEOUT};
use super::proto::{PluginDesc, Reply, Req};

/// Milan Pro's OFX preferences: the folders searched besides the standard ones, and the plug-ins turned off (by id).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OfxPrefs {
    pub folders: Vec<PathBuf>,
    pub disabled: BTreeSet<String>,
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

const VERSION: u32 = 1;

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

/// The 64-bit binaries of every `*.ofx.bundle` under `dirs` (bundles up to 4 folders deep; a bundle is not entered
/// further), sorted, each once.
pub fn find_binaries(dirs: &[PathBuf]) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut all: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        all.sort();
        for p in all {
            let bundle = p.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().ends_with(".ofx.bundle"));
            if bundle {
                let Ok(rd) = std::fs::read_dir(p.join("Contents").join("Win64")) else { continue };
                let mut bins: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|b| b.is_file() && b.extension().is_some_and(|x| x.eq_ignore_ascii_case("ofx"))).collect();
                bins.sort();
                out.extend(bins);
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
pub fn scan(dirs: &[PathBuf], old: &ScanCache, force: bool, describe: &dyn Fn(&Path) -> Binary) -> ScanCache {
    let binaries = find_binaries(dirs)
        .into_iter()
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
    let mut out = Binary { path: binary.to_path_buf(), ..Default::default() };
    let (mut conn, n) = match Conn::start(binary) {
        Ok(c) => c,
        Err(e) => {
            out.errors.push((None, e.to_string()));
            return out;
        }
    };
    for i in 0..n {
        match conn.request(&Req::Describe { index: i }, DESCRIBE_TIMEOUT) {
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
        let p = OfxPrefs { folders: vec![r"d:\OFX\".into(), r"F:\mine".into()], ..Default::default() };
        assert_eq!(folders_with(&p, &env), [r"C:\CF\OFX\Plugins", r"D:\ofx", r"E:\more", r"F:\mine"].map(PathBuf::from));
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
