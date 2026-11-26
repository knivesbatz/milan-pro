//! Auto-updater: asks GitHub Releases for the latest release, downloads its MilanPro-<ver>-Setup.exe over HTTPS
//! with a SHA-256 check, and runs it silently (the installer relaunches the app). Network work runs on
//! background threads; the app polls `Updater` each frame and draws it with `ui::dialogs::update_window`.
//!
//! R46:
//! - Channels (`Channel`): Stable reads `/releases/latest`; Preview reads `/releases` and accepts pre-releases and
//!   pre-release tags (`v0.8.0-beta.1`).
//! - Authenticity: with `MINISIGN_PUBLIC_KEY` compiled in, the release's `<Setup>.sha256.minisig` must verify over the
//!   `.sha256` file (minisign, Ed25519) or the update is refused; the hash then comes from that file alone. With the
//!   key empty (no key generated yet: SECURITY.md), the SHA-256 check alone stands, and the dialog says so.
//! - Rollback: with "keep the previous version" on, the current version's installer is fetched from its own release
//!   (verified the same way) into `%LOCALAPPDATA%\MilanPro\updates\previous\` before an update; Help › Roll Back runs
//!   it. Setup's relaunch passes `--after-update`; `start_marker` counts the starts until a first frame clears it, and
//!   a version that failed to start twice is offered a roll back.
//! - Guards: `install_guard` (no install while recording; a render is R12's guard).
//!
//! Test hooks: `MILAN_UPDATE_API=http://127.0.0.1:<port>/...` replaces the API URL (any other value is ignored);
//! with it, `MILAN_UPDATE_AUTO=1` checks at once, downloads and installs without clicks, and closes the app on
//! any other outcome (with MILAN_SCREENSHOT, after a screenshot of its message).
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;
use eframe::egui;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const REPO: &str = "https://api.github.com/repos/knivesbatz/milan-pro";
const API: &str = "https://api.github.com/repos/knivesbatz/milan-pro/releases/latest";
const API_LIST: &str = "https://api.github.com/repos/knivesbatz/milan-pro/releases?per_page=30";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// The minisign public key the release checksums are signed with (the `RW...` line of `minisign.pub`). Empty until the
/// maintainer generates the key pair (SECURITY.md): then updates are checked by SHA-256 alone.
pub const MINISIGN_PUBLIC_KEY: &str = "";

/// The signing key this build checks updates with, when one is compiled in.
pub fn signing_key() -> Option<&'static str> {
    Some(MINISIGN_PUBLIC_KEY.trim()).filter(|k| !k.is_empty())
}

/// What the update dialog says about verification.
pub fn verification_note() -> &'static str {
    if signing_key().is_some() {
        "Verified with SHA-256 and the release's minisign signature."
    } else {
        "Verified with SHA-256 only: this build has no signing key compiled in (see SECURITY.md)."
    }
}

/// Preferences › Update › Update channel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Channel {
    #[default]
    Stable,
    Preview,
}

/// A release newer than this build, with its installer.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub name: String,
    pub url: String,
    pub size: u64,
    /// Lowercase hex SHA-256 from the API's asset `digest`, if GitHub gave one; once downloaded, the verified hash.
    pub digest: Option<String>,
    /// URL of the `<name>.sha256` asset, the fallback checksum (with a signing key: the only accepted one).
    pub sha_url: Option<String>,
    /// R46: URL of the `<name>.sha256.minisig` asset.
    pub sig_url: Option<String>,
    pub prerelease: bool,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    digest: Option<String>,
}

/// "v1.2.3" or "1.2.3"; pre-release/build suffixes and anything else give None.
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim();
    let mut parts = s.strip_prefix('v').unwrap_or(s).split('.');
    let mut num = || parts.next().filter(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))?.parse().ok();
    let v = (num()?, num()?, num()?);
    parts.next().is_none().then_some(v)
}

/// A pre-release identifier: numbers compare as numbers and sort before words (semver).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum PreId {
    Num(u64),
    Str(String),
}

/// R46: a version with an optional pre-release ("0.8.0-beta.1"); a release is newer than its own pre-releases.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub core: (u64, u64, u64),
    pre: Option<Vec<PreId>>,
}

impl Version {
    /// "v0.8.0", "0.8.0-beta.1"; build metadata ("+git1") is dropped. None for anything else.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let s = s.strip_prefix('v').unwrap_or(s);
        let s = s.split('+').next().unwrap_or(s);
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (s, None),
        };
        let core = parse_version(core)?;
        let pre = match pre {
            Some(p) if !p.is_empty() => Some(
                p.split('.')
                    .map(|id| {
                        if !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) {
                            id.parse().ok().map(PreId::Num)
                        } else if !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                            Some(PreId::Str(id.to_string()))
                        } else {
                            None
                        }
                    })
                    .collect::<Option<Vec<_>>>()?,
            ),
            Some(_) => return None,
            None => None,
        };
        Some(Self { core, pre })
    }

    pub fn is_prerelease(&self) -> bool {
        self.pre.is_some()
    }
}

impl Ord for Version {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.core.cmp(&o.core).then_with(|| match (&self.pre, &o.pre) {
            (None, None) => std::cmp::Ordering::Equal,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(_), None) => std::cmp::Ordering::Less,
            (Some(a), Some(b)) => a.cmp(b),
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

/// 64 hex digits, lowercased.
fn hex64(s: &str) -> Option<String> {
    (s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())).then(|| s.to_ascii_lowercase())
}

/// The API's asset digest, "sha256:<hex>".
pub fn parse_digest(s: &str) -> Option<String> {
    hex64(s.strip_prefix("sha256:")?)
}

/// A `.sha256` file: "<hex>  <file name>" (sha256sum format) or just the hex.
pub fn parse_sha_file(s: &str) -> Option<String> {
    hex64(s.split_whitespace().next()?.trim_start_matches('\u{feff}'))
}

/// R46: the hash an update must have. With a signing `key`, `sig` (the `.minisig` text) must verify over the exact
/// bytes of the `.sha256` file first; without one, the file is taken as it is.
pub fn verified_hash(key: Option<&str>, sha_text: &str, sig: Option<&str>) -> Result<String, String> {
    if let Some(key) = key {
        let pk = minisign_verify::PublicKey::from_base64(key).map_err(|e| format!("This build's update signing key is not valid ({e}); the update was refused."))?;
        let sig = sig.ok_or("The release has no minisign signature for its checksum, so the update was refused.")?;
        let sig = minisign_verify::Signature::decode(sig).map_err(|e| format!("The release's minisign signature is not valid ({e}); the update was refused."))?;
        pk.verify(sha_text.as_bytes(), &sig, false).map_err(|_| "The release's checksum does not match its minisign signature, so the update was refused.".to_string())?;
    }
    parse_sha_file(sha_text).ok_or_else(|| "The update's .sha256 file is not valid.".into())
}

/// API and download hosts: github.com, api.github.com and *.githubusercontent.com over HTTPS; 127.0.0.1 too
/// when `local` (test override).
pub fn url_allowed(url: &str, local: bool) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else { return false };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default().to_ascii_lowercase();
    if authority.contains('@') {
        return false;
    }
    let host = authority.rsplit_once(':').map_or(authority.as_str(), |(h, _)| h);
    match scheme.to_ascii_lowercase().as_str() {
        "https" if host == "github.com" || host == "api.github.com" || host.ends_with(".githubusercontent.com") => true,
        "https" | "http" => local && host == "127.0.0.1",
        _ => false,
    }
}

/// Whether release `v` is newer than build `current` (None if `current` is not X.Y.Z[-pre][+build]).
/// A pre-release `current` (0.2.0-rc.1) counts as older than its X.Y.Z.
fn newer(v: &Version, current: &str) -> Option<bool> {
    Some(*v > Version::parse(current)?)
}

/// The newest release in `json` (one release, or the `/releases` list) that is newer than `current` and allowed on
/// `channel`: Stable takes releases with plain X.Y.Z tags; Preview also pre-releases and pre-release tags. Ok(None)
/// when there is none; Err when the JSON is malformed or the chosen release lacks its installer.
pub fn parse_releases(json: &str, current: &str, channel: Channel) -> Result<Option<Release>, String> {
    let value: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("GitHub sent an unexpected reply ({e})."))?;
    let list: Vec<ApiRelease> = match value {
        serde_json::Value::Array(_) => serde_json::from_value(value),
        v => serde_json::from_value(v).map(|r| vec![r]),
    }
    .map_err(|e| format!("GitHub sent an unexpected reply ({e})."))?;
    let mut best: Option<(Version, ApiRelease)> = None;
    for r in list {
        let Some(v) = Version::parse(&r.tag_name) else { continue };
        if r.draft || ((r.prerelease || v.is_prerelease()) && channel != Channel::Preview) || newer(&v, current) != Some(true) {
            continue;
        }
        if best.as_ref().is_none_or(|(b, _)| v > *b) {
            best = Some((v, r));
        }
    }
    let Some((v, r)) = best else { return Ok(None) };
    let version = r.tag_name.trim().trim_start_matches('v').split('+').next().unwrap_or_default().to_string();
    let name = format!("MilanPro-{version}-Setup.exe");
    let Some(a) = r.assets.iter().find(|a| a.name == name) else {
        return Err(format!("Milan Pro {version} is out, but its release has no {name}."));
    };
    let sha_name = format!("{name}.sha256");
    let sig_name = format!("{sha_name}.minisig");
    let asset_url = |n: &str| r.assets.iter().find(|a| a.name == n).map(|a| a.browser_download_url.clone());
    Ok(Some(Release {
        notes: r.body.unwrap_or_default().replace('\r', ""),
        url: a.browser_download_url.clone(),
        size: a.size,
        digest: a.digest.as_deref().and_then(parse_digest),
        sha_url: asset_url(&sha_name),
        sig_url: asset_url(&sig_name),
        prerelease: r.prerelease || v.is_prerelease(),
        version,
        name,
    }))
}

/// Copy `src` to `dst`, hashing on the way; returns the lowercase hex SHA-256. `progress` gets the bytes so far.
pub fn copy_hashed(mut src: impl Read, mut dst: impl Write, cancel: &AtomicBool, progress: impl Fn(u64)) -> Result<String, String> {
    let (mut hasher, mut buf, mut done) = (Sha256::new(), vec![0u8; 64 * 1024], 0u64);
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("Download cancelled.".into());
        }
        let n = src.read(&mut buf).map_err(|e| format!("Download failed: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        dst.write_all(&buf[..n]).map_err(|e| format!("Could not write the update: {e}"))?;
        done += n as u64;
        progress(done);
    }
    dst.flush().map_err(|e| format!("Could not write the update: {e}"))?;
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn agent(local: bool) -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(!local)
        .user_agent(format!("MilanPro-Updater/{CURRENT}"))
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_per_read(Some(Duration::from_secs(60)))
        .save_redirect_history(true)
        .build()
        .into()
}

fn describe(e: ureq::Error) -> String {
    match e {
        ureq::Error::StatusCode(404) => "No Milan Pro release has been published yet.".into(),
        ureq::Error::StatusCode(403 | 429) => "GitHub's rate limit was reached. Try again later.".into(),
        e => format!("Could not reach GitHub: {e}"),
    }
}

/// Errors from fetching the installer or its checksum (`describe`'s wording is about the API).
fn download_failed(e: ureq::Error) -> String {
    format!("Download failed: {e}. Try again later.")
}

/// GET `url`, refusing it, or any redirect hop, outside the allow-list.
fn get(agent: &ureq::Agent, url: &str, local: bool, err: fn(ureq::Error) -> String) -> Result<ureq::http::Response<ureq::Body>, String> {
    use ureq::ResponseExt;
    if !url_allowed(url, local) {
        return Err(format!("Refusing to download from {url}."));
    }
    let resp = agent.get(url).call().map_err(err)?;
    let hops = resp.get_redirect_history().unwrap_or_default().iter().chain([resp.get_uri()]);
    if let Some(bad) = hops.map(|u| u.to_string()).find(|u| !url_allowed(u, local)) {
        return Err(format!("Refusing to download from {bad}."));
    }
    Ok(resp)
}

fn check(api: &str, local: bool, channel: Channel) -> Result<Option<Release>, String> {
    let json = get(&agent(local), api, local, describe)?.body_mut().read_to_string().map_err(describe)?;
    let r = parse_releases(&json, CURRENT, channel);
    match &r {
        Ok(Some(rel)) => log::info!("update check ({channel:?}): {} available", rel.version),
        Ok(None) => log::info!("update check ({channel:?}): up to date"),
        Err(e) => log::warn!("update check ({channel:?}): {e}"),
    }
    r
}

pub fn update_dir() -> PathBuf {
    std::env::temp_dir().join("MilanPro-update")
}

/// R46: `%LOCALAPPDATA%\MilanPro\updates`: `previous\` keeps the installer Help › Roll Back runs, `starting.json`
/// counts the starts after an update.
pub fn updates_dir() -> Option<PathBuf> {
    Some(crate::logger::local_root()?.join("updates"))
}

pub fn previous_dir() -> Option<PathBuf> {
    Some(updates_dir()?.join("previous"))
}

/// Delete `.part` files and `MilanPro-<ver>-Setup.exe` installers no newer than `current`; newer installers
/// stay (a "Later" download is reused) and so does anything else. A `.part` another Milan Pro window is still
/// writing survives: `create_part` opens it without delete sharing, so its removal fails. One written in the
/// last minute survives too, as `download` closes it (dropping that lock) just before renaming it.
pub fn clean_downloads(dir: &Path, current: &str) {
    let recent = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().is_ok_and(|age| age < Duration::from_secs(60)));
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let installer = name.strip_prefix("MilanPro-").and_then(|n| n.strip_suffix("-Setup.exe")).and_then(Version::parse);
        if (name.ends_with(".part") && !recent(&e.path())) || installer.is_some_and(|v| newer(&v, current) == Some(false)) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// A new download file, locked against deletion (see `clean_downloads`) until it is closed.
fn create_part(path: &Path) -> std::io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(windows)]
    std::os::windows::fs::OpenOptionsExt::share_mode(&mut o, 1); // FILE_SHARE_READ
    o.open(path)
}

/// Open `path` read-only so nobody can change or delete it while the handle lives, and hash it.
fn open_hashed(path: &Path) -> std::io::Result<(std::fs::File, String)> {
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    #[cfg(windows)]
    std::os::windows::fs::OpenOptionsExt::share_mode(&mut o, 1); // FILE_SHARE_READ
    let f = o.open(path)?;
    let h = copy_hashed(&f, std::io::sink(), &AtomicBool::new(false), |_| {}).map_err(std::io::Error::other)?;
    Ok((f, h))
}

/// The hash `rel`'s installer must have. With a signing `key`: the signed `.sha256` file, nothing else (the API's
/// digest is not authenticated). Without: the API's digest, else the `.sha256` file.
fn expected_hash(rel: &Release, agent: &ureq::Agent, local: bool, key: Option<&str>) -> Result<String, String> {
    let fetch = |url: &str| get(agent, url, local, download_failed)?.body_mut().read_to_string().map_err(download_failed);
    if key.is_some() {
        let Some(sha_url) = &rel.sha_url else { return Err("The release has no .sha256 checksum file, so the update was refused.".into()) };
        let Some(sig_url) = &rel.sig_url else { return Err("The release has no minisign signature for its checksum, so the update was refused.".into()) };
        let (sha, sig) = (fetch(sha_url)?, fetch(sig_url)?);
        return verified_hash(key, &sha, Some(&sig));
    }
    match (&rel.digest, &rel.sha_url) {
        (Some(h), _) => Ok(h.clone()),
        (None, Some(url)) => verified_hash(None, &fetch(url)?, None),
        (None, None) => Err("The update has no SHA-256 checksum, so it was not downloaded.".into()),
    }
}

/// Download and verify `rel`'s installer into `dir`; the file only gets its real name once its hash matches.
/// Returns the installer and its verified hash.
fn download(rel: &Release, dir: &Path, local: bool, cancel: &AtomicBool, progress: impl Fn(u64), key: Option<&str>) -> Result<(PathBuf, String), String> {
    let agent = agent(local);
    let expected = expected_hash(rel, &agent, local, key)?;
    std::fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    let exe = dir.join(&rel.name);
    // Verified earlier, then "Later" or a cancelled save prompt: no need to download it again.
    if open_hashed(&exe).is_ok_and(|(_, h)| h == expected) {
        return Ok((exe, expected));
    }
    // A name of its own, so a cancelled earlier attempt or another window never truncates or deletes this file.
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    let part = dir.join(format!("{}.{}-{nonce}.part", rel.name, std::process::id()));
    let mut resp = get(&agent, &rel.url, local, download_failed)?;
    let file = create_part(&part).map_err(|e| format!("Could not create {}: {e}", part.display()))?;
    let got = copy_hashed(resp.body_mut().as_reader(), std::io::BufWriter::new(file), cancel, progress);
    match got {
        Ok(h) if h == expected => {
            std::fs::rename(&part, &exe).map_err(|e| {
                let _ = std::fs::remove_file(&part);
                format!("Could not save the update: {e}")
            })?;
            log::info!("downloaded and verified {}", exe.display());
            Ok((exe, expected))
        }
        r => {
            let _ = std::fs::remove_file(&part);
            Err(r.err().unwrap_or_else(|| "The download's SHA-256 does not match the release; it was deleted.".into()))
        }
    }
}

/// R46: an installer kept for Help › Roll Back, with the hash it was verified against (`<name>.sha256` beside it).
#[derive(Clone, Debug, PartialEq)]
pub struct Previous {
    pub exe: PathBuf,
    pub version: String,
    pub sha256: String,
}

/// The newest kept installer in `dir` that has its `.sha256` sidecar.
pub fn previous_installer(dir: &Path) -> Option<Previous> {
    let mut found: Vec<(Version, Previous)> = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(v) = name.strip_prefix("MilanPro-").and_then(|n| n.strip_suffix("-Setup.exe")) else { continue };
        let Some(version) = Version::parse(v) else { continue };
        let side = e.path().with_file_name(format!("{name}.sha256"));
        let Some(sha256) = std::fs::read_to_string(side).ok().and_then(|s| parse_sha_file(&s)) else { continue };
        found.push((version, Previous { exe: e.path(), version: v.to_string(), sha256 }));
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().next().map(|(_, p)| p)
}

/// The version Help › Roll Back would install: the kept installer, when it is not this build's own.
pub fn rollback_target(dir: &Path) -> Option<Previous> {
    previous_installer(dir).filter(|p| p.version != CURRENT)
}

/// Before an update: this build's own installer into `prev` (from its release, verified like an update), so Help ›
/// Roll Back can put it back. Older kept installers go. Already there: nothing to fetch.
fn keep_current(prev: &Path, api: &str, local: bool, key: Option<&str>, cancel: &AtomicBool) -> Result<PathBuf, String> {
    if let Some(p) = previous_installer(prev).filter(|p| p.version == CURRENT) {
        return Ok(p.exe);
    }
    let agent = agent(local);
    let json = get(&agent, api, local, download_failed)?.body_mut().read_to_string().map_err(download_failed)?;
    let rel = parse_releases(&json, "0.0.0", Channel::Preview)?.filter(|r| r.version == CURRENT).ok_or(format!("the release of Milan Pro {CURRENT} was not found"))?;
    std::fs::create_dir_all(prev).map_err(|e| e.to_string())?;
    let (exe, sha) = download(&rel, prev, local, cancel, |_| {}, key)?;
    std::fs::write(prev.join(format!("{}.sha256", rel.name)), format!("{sha}  {}\n", rel.name)).map_err(|e| e.to_string())?;
    for e in std::fs::read_dir(prev).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with("MilanPro-") && !name.starts_with(&rel.name) {
            let _ = std::fs::remove_file(e.path());
        }
    }
    log::info!("kept {} for roll back", exe.display());
    Ok(exe)
}

/// The URL of this build's own release (the tags endpoint; the test override stays the test server).
fn own_release_api(api: &str, local: bool) -> String {
    if local { api.to_string() } else { format!("{REPO}/releases/tags/v{CURRENT}") }
}

/// R46: why an update may not be installed right now (a render is R12's guard, which waits for the queue instead).
pub fn install_guard(rendering: bool, recording: bool) -> Option<&'static str> {
    if recording {
        Some("The update is not installed while a recording is running. Stop the recording first.")
    } else if rendering {
        Some("The update is not installed while a render is running.")
    } else {
        None
    }
}

/// `updates\starting.json`: the version being started and how many starts it took so far.
#[derive(Serialize, Deserialize)]
struct StartMarker {
    version: String,
    attempts: u32,
}

/// At start. `after_update`: started by Setup's relaunch (`--after-update`), the first start of a new version. The
/// marker counts the starts; a successful first frame removes it (`start_ok`). True when this version has failed to
/// start twice (this is the third try): the app then offers a roll back.
pub fn start_marker(dir: &Path, version: &str, after_update: bool) -> bool {
    let file = dir.join("starting.json");
    let read = || std::fs::read_to_string(&file).ok().and_then(|s| serde_json::from_str::<StartMarker>(&s).ok());
    let mut m = match (after_update, read()) {
        (true, _) => StartMarker { version: version.into(), attempts: 0 },
        (false, Some(m)) if m.version == version => m,
        (false, _) => {
            let _ = std::fs::remove_file(&file);
            return false;
        }
    };
    m.attempts += 1;
    if std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&file, serde_json::to_string(&m).unwrap_or_default())).is_err() {
        return false;
    }
    m.attempts >= 3
}

/// The first frame was drawn: this version starts.
pub fn start_ok(dir: &Path) {
    let _ = std::fs::remove_file(dir.join("starting.json"));
}

/// Start the installer detached if it still hashes to `sha256`: silent, then it relaunches Milan Pro
/// (/RELAUNCH=<this exe>, see installer/milan-pro.iss). `hidden` (MILAN_UPDATE_AUTO) also hides Setup's
/// progress window, so the end-to-end test puts no window on the user's screen.
pub fn launch_installer(exe: &Path, sha256: &str, hidden: bool) -> std::io::Result<()> {
    // The file sat in a user-writable folder since it was verified: check it again, and hold it locked
    // against changes until Setup has started.
    let (_lock, h) = open_hashed(exe)?;
    if h != sha256 {
        return Err(std::io::Error::other("the installer changed after it was downloaded"));
    }
    let mut relaunch = std::ffi::OsString::from("/RELAUNCH=");
    relaunch.push(std::env::current_exe()?);
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS"]).arg(relaunch);
    if hidden {
        cmd.arg("/VERYSILENT");
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    log::info!("starting the installer {}", exe.display());
    // A plain spawn, never proc::spawn: its Job Object is KILL_ON_JOB_CLOSE and would kill Setup as Milan exits.
    cmd.spawn().map(drop)
}

enum Msg {
    Checked(Result<Option<Release>, String>),
    Progress(u64),
    Downloaded(Result<(PathBuf, String), String>),
    /// R46: what happened to the roll-back copy of the current version.
    Note(String),
}

pub enum State {
    Idle,
    Checking,
    Available(Release),
    Downloading(Release, u64),
    Ready(Release, PathBuf),
    /// Result of a manual check or a failed download.
    Message(String),
}

pub struct Updater {
    pub state: State,
    /// Started from Help › Check for Updates: report "up to date" and errors, and ignore the skipped version.
    manual: bool,
    rx: Option<mpsc::Receiver<Msg>>,
    cancel: Arc<AtomicBool>,
    api: String,
    local: bool,
    /// MILAN_UPDATE_AUTO=1 with a local MILAN_UPDATE_API (end-to-end test).
    pub auto: bool,
    /// R46: Preferences › Update › Update channel, as last checked with.
    pub channel: Channel,
    /// R46: where to keep the current version's installer before updating (None: the preference is off).
    pub keep_previous: Option<PathBuf>,
    /// R46: shown with the Ready state: the roll-back copy could not be kept.
    pub note: Option<String>,
}

impl Updater {
    pub fn from_env() -> Self {
        // Only a local test server may replace the API, and only then does AUTO apply, so a stray or planted
        // variable can neither point the app at another host nor make it install without a click.
        let api = std::env::var("MILAN_UPDATE_API").ok().filter(|v| url_allowed(v, true) && !url_allowed(v, false));
        let auto = api.is_some() && std::env::var("MILAN_UPDATE_AUTO").is_ok_and(|v| v == "1");
        Self {
            state: State::Idle,
            manual: false,
            rx: None,
            cancel: Arc::default(),
            local: api.is_some(),
            api: api.unwrap_or_default(),
            auto,
            channel: Channel::Stable,
            keep_previous: None,
            note: None,
        }
    }

    /// The API URL for `channel` (the test override serves both).
    fn api_for(&self, channel: Channel) -> String {
        if self.local {
            self.api.clone()
        } else {
            match channel {
                Channel::Stable => API.into(),
                Channel::Preview => API_LIST.into(),
            }
        }
    }

    fn spawn(&mut self, ctx: &egui::Context, work: impl FnOnce(&mpsc::Sender<Msg>) + Send + 'static) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            work(&tx);
            ctx.request_repaint();
        });
    }

    /// Check `channel` in the background after `delay`. Does nothing while a download is under way or ready.
    pub fn check(&mut self, ctx: &egui::Context, manual: bool, delay: Duration, channel: Channel) {
        if matches!(self.state, State::Downloading(..) | State::Ready(..)) {
            return;
        }
        (self.manual, self.state, self.channel) = (manual || self.auto, if manual { State::Checking } else { State::Idle }, channel);
        let (api, local) = (self.api_for(channel), self.local);
        self.spawn(ctx, move |tx| {
            std::thread::sleep(delay);
            let _ = tx.send(Msg::Checked(check(&api, local, channel)));
        });
    }

    pub fn download(&mut self, ctx: &egui::Context) {
        let State::Available(rel) = &self.state else { return };
        let rel = rel.clone();
        self.state = State::Downloading(rel.clone(), 0);
        self.note = None;
        self.cancel = Arc::default();
        let (cancel, local, keep, api) = (self.cancel.clone(), self.local, self.keep_previous.clone(), own_release_api(&self.api, self.local));
        let repaint = ctx.clone();
        self.spawn(ctx, move |tx| {
            let dir = update_dir();
            clean_downloads(&dir, CURRENT);
            let r = download(&rel, &dir, local, &cancel, |n| {
                let _ = tx.send(Msg::Progress(n));
                repaint.request_repaint();
            }, signing_key());
            if r.is_ok() {
                if let Some(prev) = keep {
                    if let Err(e) = keep_current(&prev, &api, local, signing_key(), &cancel) {
                        log::warn!("roll-back copy of {CURRENT}: {e}");
                        let _ = tx.send(Msg::Note(format!("The current version's installer could not be kept for Roll Back: {e}")));
                    }
                }
            }
            let _ = tx.send(Msg::Downloaded(r));
        });
    }

    /// Close the dialog, stopping a check or download.
    pub fn dismiss(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        (self.rx, self.state) = (None, State::Idle);
    }

    /// Take results from the background thread; an automatic check skips `skipped`.
    pub fn poll(&mut self, ctx: &egui::Context, skipped: Option<&str>) {
        while let Some(msg) = self.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.state = match (msg, std::mem::replace(&mut self.state, State::Idle)) {
                (Msg::Checked(Ok(Some(r))), _) if self.manual || skipped != Some(r.version.as_str()) => State::Available(r),
                (Msg::Checked(Ok(None)), _) if self.manual => State::Message(format!("Milan Pro {CURRENT} is up to date.")),
                (Msg::Checked(Err(e)), _) if self.manual => State::Message(e),
                (Msg::Checked(_), _) => State::Idle,
                (Msg::Progress(n), State::Downloading(r, _)) => State::Downloading(r, n),
                (Msg::Downloaded(Ok((p, h))), State::Downloading(r, _)) => State::Ready(Release { digest: Some(h), ..r }, p),
                (Msg::Downloaded(Err(e)), State::Downloading(..)) => State::Message(e),
                (Msg::Note(n), s) => {
                    self.note = Some(n);
                    s
                }
                (_, s) => s,
            };
        }
        if self.auto && matches!(self.state, State::Available(_)) {
            self.download(ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The latest stable release from the API's JSON if it is newer than `current` (the Stable channel).
    fn parse_release(json: &str, current: &str) -> Result<Option<Release>, String> {
        parse_releases(json, current, Channel::Stable)
    }

    #[test]
    fn versions_compare_numerically_and_reject_prereleases() {
        assert!(parse_version("v0.10.0") > parse_version("0.9.0"));
        assert!(parse_version("v1.0.0") > parse_version("v0.99.99"));
        assert_eq!(parse_version("v0.2.0"), Some((0, 2, 0)));
        for bad in ["v0.3.0-beta.1", "0.3.0+build", "v0.3", "0.3.0.1", "v+1.0.0", "", "latest"] {
            assert_eq!(parse_version(bad), None, "{bad}");
        }
    }

    /// R46: pre-release versions order as semver says, below their release.
    #[test]
    fn prerelease_versions_order() {
        let v = |s: &str| Version::parse(s).unwrap_or_else(|| panic!("{s}"));
        assert!(v("0.8.0-beta.1") < v("0.8.0"));
        assert!(v("0.8.0-beta.1") < v("0.8.0-beta.2"));
        assert!(v("0.8.0-beta.2") < v("0.8.0-rc.1"));
        assert!(v("0.8.0-alpha") < v("0.8.0-alpha.1"));
        assert!(v("0.7.0") < v("0.8.0-beta.1"));
        assert_eq!(v("v0.8.0+git1"), v("0.8.0"));
        assert!(v("0.8.0-beta.1").is_prerelease() && !v("0.8.0").is_prerelease());
        for bad in ["0.8", "0.8.0-", "0.8.0-beta_1", "x"] {
            assert!(Version::parse(bad).is_none(), "{bad}");
        }
    }

    /// Release check against real GitHub: what an installed 0.2.0 sees, downloaded and verified with the app's code.
    /// Run after publishing: cargo test --release live_github_release -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_github_release() {
        let json = get(&agent(false), API, false, describe).unwrap().body_mut().read_to_string().unwrap();
        let rel = parse_release(&json, "0.2.0").unwrap().expect("a release newer than 0.2.0");
        assert!(rel.digest.is_some() && rel.sha_url.is_some(), "{rel:?}");
        let dir = std::env::temp_dir().join(format!("milan-live-update-{}", std::process::id()));
        let (exe, sha) = download(&rel, &dir, false, &AtomicBool::new(false), |_| {}, None).unwrap();
        println!("latest {} -> {} ({sha})", rel.version, exe.display());
        assert_eq!(open_hashed(&exe).unwrap().1, sha);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn release_json(tag: &str, prerelease: bool, assets: &str) -> String {
        format!(r#"{{"tag_name":"{tag}","draft":false,"prerelease":{prerelease},"body":"Notes\r\nline 2","assets":[{assets}]}}"#)
    }
    const SETUP: &str = r#"{"name":"MilanPro-0.10.0-Setup.exe","browser_download_url":"https://github.com/a/b/x.exe","size":5,"digest":"sha256:ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef0123456789"}"#;
    const SHA: &str = r#"{"name":"MilanPro-0.10.0-Setup.exe.sha256","browser_download_url":"https://github.com/a/b/x.sha256","size":1}"#;
    const SIG: &str = r#"{"name":"MilanPro-0.10.0-Setup.exe.sha256.minisig","browser_download_url":"https://github.com/a/b/x.minisig","size":1}"#;
    const OTHER: &str = r#"{"name":"MilanPro-0.9.0-Setup.exe","browser_download_url":"https://github.com/a/b/old.exe","size":5}"#;
    const BETA: &str = r#"{"name":"MilanPro-0.11.0-beta.1-Setup.exe","browser_download_url":"https://github.com/a/b/beta.exe","size":5}"#;

    #[test]
    fn release_selection() {
        let r = parse_release(&release_json("v0.10.0", false, &format!("{OTHER},{SETUP},{SHA},{SIG}")), "0.9.0").unwrap().unwrap();
        assert_eq!((r.version.as_str(), r.url.as_str(), r.notes.as_str()), ("0.10.0", "https://github.com/a/b/x.exe", "Notes\nline 2"));
        assert_eq!(r.digest.as_deref(), Some("abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"));
        assert_eq!(r.sha_url.as_deref(), Some("https://github.com/a/b/x.sha256"));
        assert_eq!(r.sig_url.as_deref(), Some("https://github.com/a/b/x.minisig"));
        assert!(!r.prerelease);
        // Same or older version, a pre-release, or a pre-release tag: no update.
        assert_eq!(parse_release(&release_json("v0.10.0", false, SETUP), "0.10.0"), Ok(None));
        assert_eq!(parse_release(&release_json("v0.10.0", true, SETUP), "0.9.0"), Ok(None));
        assert_eq!(parse_release(&release_json("v0.10.0-rc1", false, SETUP), "0.9.0"), Ok(None));
        // A pre-release build is older than its own release; build metadata is not a pre-release.
        assert!(parse_release(&release_json("v0.10.0", false, SETUP), "0.10.0-rc.1").unwrap().is_some());
        assert!(parse_release(&release_json("v0.10.0", false, SETUP), "0.9.1-beta").unwrap().is_some());
        assert_eq!(parse_release(&release_json("v0.10.0", false, SETUP), "0.10.0+git1"), Ok(None));
        assert_eq!(parse_release(&release_json("v0.10.0", false, SETUP), "0.11.0-rc.1"), Ok(None));
        // Newer but without its installer, or malformed: an error.
        assert!(parse_release(&release_json("v0.10.0", false, OTHER), "0.9.0").is_err());
        assert!(parse_release("{\"message\":\"Not Found\"}", "0.9.0").is_err());
        assert!(parse_release("<html>", "0.9.0").is_err());
    }

    /// R46: the release list per channel: Stable takes the newest plain release, Preview the newest of all.
    #[test]
    fn release_list_per_channel() {
        let list = format!(
            "[{},{},{},{}]",
            release_json("v0.11.0-beta.1", true, BETA),
            release_json("v0.10.0", false, &format!("{SETUP},{SHA}")),
            release_json("v0.9.0", false, OTHER),
            r#"{"tag_name":"v0.12.0","draft":true,"prerelease":false,"assets":[]}"#
        );
        let stable = parse_releases(&list, "0.9.0", Channel::Stable).unwrap().unwrap();
        assert_eq!((stable.version.as_str(), stable.prerelease), ("0.10.0", false));
        let preview = parse_releases(&list, "0.9.0", Channel::Preview).unwrap().unwrap();
        assert_eq!((preview.version.as_str(), preview.name.as_str(), preview.prerelease), ("0.11.0-beta.1", "MilanPro-0.11.0-beta.1-Setup.exe", true));
        // On the beta already, Preview waits for something newer; Stable still offers nothing below it.
        assert_eq!(parse_releases(&list, "0.11.0-beta.1", Channel::Preview), Ok(None));
        assert_eq!(parse_releases(&list, "0.11.0-beta.1", Channel::Stable), Ok(None));
        // The list's pre-release without its installer: an error on Preview, nothing on Stable.
        let broken = format!("[{}]", release_json("v0.11.0-beta.1", true, OTHER));
        assert!(parse_releases(&broken, "0.10.0", Channel::Preview).is_err());
        assert_eq!(parse_releases(&broken, "0.10.0", Channel::Stable), Ok(None));
        assert_eq!(parse_releases("[]", "0.9.0", Channel::Preview), Ok(None));
    }

    #[test]
    fn digests() {
        let h = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(parse_digest(&format!("sha256:{h}")).as_deref(), Some(h));
        assert_eq!(parse_digest(&format!("sha512:{h}")), None);
        assert_eq!(parse_digest("sha256:abc"), None);
        assert_eq!(parse_sha_file(&format!("{}  MilanPro-0.2.0-Setup.exe\n", h.to_uppercase())).as_deref(), Some(h));
        assert_eq!(parse_sha_file(&format!("\u{feff}{h}")).as_deref(), Some(h));
        assert_eq!(parse_sha_file("not a hash  file.exe"), None);
        assert_eq!(parse_sha_file(""), None);
    }

    // minisign's own test vector: this key signed the four bytes "test" (pre-hashed mode).
    const TEST_KEY: &str = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    const TEST_SIG: &str = "untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1633700835\tfile:test\tprehashed\nwLMDjy9FLAuxZ3q4NlEvkgtyhrr0gtTu6KC4KBJdITbbOeAi1zBIYo0v4iTgt8jJpIidRJnp94ABQkJAgAooBQ==";

    /// R46: with a signing key, a checksum whose signature is missing, wrong or for other bytes is refused; the
    /// verification itself passes on minisign's test vector; without a key the checksum stands alone.
    #[test]
    fn bad_signature_is_rejected_when_a_key_is_configured() {
        let h = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let sha = format!("{h}  MilanPro-0.10.0-Setup.exe\n");
        assert_eq!(verified_hash(None, &sha, None).as_deref(), Ok(h));
        assert!(verified_hash(Some(TEST_KEY), &sha, None).unwrap_err().contains("no minisign signature"));
        assert!(verified_hash(Some(TEST_KEY), &sha, Some("garbage")).unwrap_err().contains("not valid"));
        // A well-formed signature over other bytes: refused.
        assert!(verified_hash(Some(TEST_KEY), &sha, Some(TEST_SIG)).unwrap_err().contains("does not match its minisign signature"));
        // The vector itself verifies (the "file" is not a checksum, so only the parse fails after it).
        assert_eq!(verified_hash(Some(TEST_KEY), "test", Some(TEST_SIG)).unwrap_err(), "The update's .sha256 file is not valid.");
        assert!(verified_hash(Some(TEST_KEY), "tesT", Some(TEST_SIG)).unwrap_err().contains("does not match"));
        assert!(verified_hash(Some("not a key"), &sha, Some(TEST_SIG)).unwrap_err().contains("signing key is not valid"));
        // The release's assets must include the signature when a key is configured: refused before any download.
        let rel = Release { version: "0.10.0".into(), notes: String::new(), name: "MilanPro-0.10.0-Setup.exe".into(), url: "https://github.com/a/b/x.exe".into(), size: 1, digest: Some(h.into()), sha_url: Some("https://github.com/a/b/x.sha256".into()), sig_url: None, prerelease: false };
        let dir = std::env::temp_dir().join(format!("milan-sig-test-{}", std::process::id()));
        let err = download(&rel, &dir, false, &AtomicBool::new(false), |_| {}, Some(TEST_KEY)).unwrap_err();
        assert!(err.contains("no minisign signature"), "{err}");
        assert!(!dir.exists(), "nothing was downloaded or created");
        assert!(verification_note().contains("SHA-256"));
    }

    /// R46: no install while recording (a render is R12's guard, which waits instead).
    #[test]
    fn install_guard_blocks_a_running_job() {
        assert!(install_guard(false, true).unwrap().contains("recording"));
        assert!(install_guard(true, false).unwrap().contains("render"));
        assert!(install_guard(true, true).unwrap().contains("recording"));
        assert_eq!(install_guard(false, false), None);
    }

    /// R46: the start marker Setup's relaunch leaves offers a roll back on the third try of the same version.
    #[test]
    fn rollback_marker_counts_failed_starts() {
        let dir = std::env::temp_dir().join("milan-agent").join("r42").join(format!("start-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // A plain start with no marker: nothing.
        assert!(!start_marker(&dir, "0.8.0", false));
        assert!(!dir.join("starting.json").exists());
        // Setup relaunches 0.8.0: first try; it fails (no start_ok) twice more.
        assert!(!start_marker(&dir, "0.8.0", true));
        assert!(!start_marker(&dir, "0.8.0", false));
        assert!(start_marker(&dir, "0.8.0", false), "the third try offers a roll back");
        // A successful first frame clears it.
        start_ok(&dir);
        assert!(!start_marker(&dir, "0.8.0", false));
        // A marker of another version (the user rolled back) is dropped.
        assert!(!start_marker(&dir, "0.8.0", true));
        assert!(!start_marker(&dir, "0.7.0", false));
        assert!(!dir.join("starting.json").exists());
        // The kept installer: found by its sidecar; this build's own is not a roll-back target.
        let prev = dir.join("previous");
        std::fs::create_dir_all(&prev).unwrap();
        std::fs::write(prev.join("MilanPro-0.6.0-Setup.exe"), b"x").unwrap();
        assert_eq!(previous_installer(&prev), None, "no sidecar, no roll back");
        std::fs::write(prev.join("MilanPro-0.6.0-Setup.exe.sha256"), format!("{}  MilanPro-0.6.0-Setup.exe\n", "a".repeat(64))).unwrap();
        let p = rollback_target(&prev).unwrap();
        assert_eq!((p.version.as_str(), p.sha256.as_str()), ("0.6.0", "a".repeat(64).as_str()));
        std::fs::write(prev.join(format!("MilanPro-{CURRENT}-Setup.exe")), b"x").unwrap();
        std::fs::write(prev.join(format!("MilanPro-{CURRENT}-Setup.exe.sha256")), format!("{}  x\n", "b".repeat(64))).unwrap();
        assert_eq!(previous_installer(&prev).unwrap().version, CURRENT);
        assert_eq!(rollback_target(&prev), None, "this build's own installer is not a roll-back target");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hash_verification() {
        let (cancel, mut out) = (AtomicBool::new(false), Vec::new());
        let h = copy_hashed(&b"abc"[..], &mut out, &cancel, |_| {}).unwrap();
        assert_eq!(h, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(out, b"abc");
        assert_ne!(copy_hashed(&b"abd"[..], Vec::new(), &cancel, |_| {}).unwrap(), h);
        cancel.store(true, Ordering::Relaxed);
        assert!(copy_hashed(&b"abc"[..], Vec::new(), &cancel, |_| {}).is_err());
    }

    #[test]
    fn host_allow_list() {
        for ok in [
            "https://github.com/knivesbatz/milan-pro/releases/download/v0.2.0/MilanPro-0.2.0-Setup.exe",
            "https://objects.githubusercontent.com/x?y=1",
            "https://release-assets.githubusercontent.com/x",
            "HTTPS://GitHub.com/x",
            API,
            API_LIST,
        ] {
            assert!(url_allowed(ok, false), "{ok}");
        }
        for bad in [
            "http://github.com/x",
            "https://github.com.evil.com/x",
            "https://evilgithubusercontent.com/x",
            "https://github.com@evil.com/x",
            "https://evil.com/github.com",
            "https://evil.com/?h=.githubusercontent.com",
            "ftp://github.com/x",
            "http://127.0.0.1:8080/x",
            "github.com/x",
        ] {
            assert!(!url_allowed(bad, false), "{bad}");
        }
        assert!(url_allowed("http://127.0.0.1:8080/x", true));
        assert!(!url_allowed("http://localhost:8080/x", true));
        assert!(!url_allowed("http://github.com/x", true));
        assert!(!url_allowed("http://api.github.com/x", false));
        assert!(own_release_api("http://127.0.0.1:1/x", true) == "http://127.0.0.1:1/x");
        assert!(own_release_api("", false).ends_with(&format!("/releases/tags/v{CURRENT}")));
    }

    #[test]
    fn clean_downloads_removes_old_installers_and_parts() {
        let dir = std::env::temp_dir().join(format!("milan-clean-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let removed = ["MilanPro-0.2.9-Setup.exe", "MilanPro-0.3.0-Setup.exe", "MilanPro-0.4.0-Setup.exe.12-34.part", "x.part", "MilanPro-0.3.0-rc.1-Setup.exe"];
        let kept = ["MilanPro-0.3.1-Setup.exe", "MilanPro-1.0.0-Setup.exe", "MilanPro-0.1-Setup.exe", "notes.txt", "MilanPro-0.2.0-Setup.exe.sha256", "MilanPro-0.4.0-beta.1-Setup.exe"];
        for n in removed.iter().chain(&kept) {
            std::fs::write(dir.join(n), b"x").unwrap();
        }
        // Left-over parts are old; a part just closed by a finished download (about to be renamed) is not.
        for n in &removed[2..4] {
            let f = std::fs::File::options().write(true).open(dir.join(n)).unwrap();
            f.set_modified(std::time::SystemTime::now() - Duration::from_secs(120)).unwrap();
        }
        let finished = dir.join("MilanPro-0.4.0-Setup.exe.90-12.part");
        std::fs::write(&finished, b"x").unwrap();
        // A download another window is still writing.
        let busy = dir.join("MilanPro-0.4.0-Setup.exe.56-78.part");
        let writing = create_part(&busy).unwrap();
        clean_downloads(&dir, "0.3.0");
        for n in removed {
            assert!(!dir.join(n).exists(), "{n} kept");
        }
        for n in kept {
            assert!(dir.join(n).exists(), "{n} removed");
        }
        #[cfg(windows)]
        assert!(busy.exists(), "an open .part was removed");
        assert!(finished.exists(), "a just-finished .part was removed");
        // A pre-release build keeps its own release's installer.
        clean_downloads(&dir, "0.3.1-rc.1");
        assert!(dir.join("MilanPro-0.3.1-Setup.exe").exists());
        drop(writing);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn installer_launch_stays_out_of_the_job_object() {
        // proc::spawn would put Setup in the KILL_ON_JOB_CLOSE job, which kills it as Milan exits.
        let src = include_str!("updater.rs");
        let body = src.split("pub fn launch_installer").nth(1).and_then(|s| s.split("\n}\n").next()).unwrap();
        assert!(body.contains("cmd.spawn()"), "{body}");
        assert!(!body.contains("proc::spawn(") && !body.contains("crate::proc"), "{body}");
    }

    #[cfg(windows)]
    #[test]
    fn installer_is_hashed_again_before_it_runs() {
        // whoami.exe stands in for Setup: it rejects the Setup switches and exits without a window.
        let dir = std::env::temp_dir().join(format!("milan-update-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("Setup.exe");
        std::fs::copy(r"C:\Windows\System32\whoami.exe", &exe).unwrap();
        let (_, h) = open_hashed(&exe).unwrap();
        assert!(launch_installer(&exe, "0".repeat(64).as_str(), false).is_err());
        assert!(launch_installer(&exe, &h, false).is_ok());
        std::thread::sleep(Duration::from_millis(500));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
