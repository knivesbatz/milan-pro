//! Auto-updater: asks GitHub Releases for the latest release, downloads its MilanPro-<ver>-Setup.exe over HTTPS
//! with a SHA-256 check, and runs it silently (the installer relaunches the app). Network work runs on
//! background threads; the app polls `Updater` each frame and draws it with `ui::dialogs::update_window`.
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
use serde::Deserialize;
use sha2::{Digest, Sha256};

const API: &str = "https://api.github.com/repos/knivesbatz/milan-pro/releases/latest";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

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
    /// URL of the `<name>.sha256` asset, the fallback checksum.
    pub sha_url: Option<String>,
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
fn newer(v: (u64, u64, u64), current: &str) -> Option<bool> {
    let core = current.split(['-', '+']).next().unwrap_or(current);
    let pre = current.split('+').next().is_some_and(|c| c.contains('-'));
    let cur = parse_version(core)?;
    Some(v > cur || (v == cur && pre))
}

/// The latest release from the API's JSON if it is newer than `current`: Ok(None) when it is not (or is a
/// draft, pre-release or non-X.Y.Z tag); Err when the JSON is malformed or the installer asset is missing.
/// A pre-release `current` (0.2.0-rc.1) counts as older than its X.Y.Z.
pub fn parse_release(json: &str, current: &str) -> Result<Option<Release>, String> {
    let r: ApiRelease = serde_json::from_str(json).map_err(|e| format!("GitHub sent an unexpected reply ({e})."))?;
    let Some(v) = parse_version(&r.tag_name) else { return Ok(None) };
    if r.draft || r.prerelease || newer(v, current) != Some(true) {
        return Ok(None);
    }
    let version = format!("{}.{}.{}", v.0, v.1, v.2);
    let name = format!("MilanPro-{version}-Setup.exe");
    let Some(a) = r.assets.iter().find(|a| a.name == name) else {
        return Err(format!("Milan Pro {version} is out, but its release has no {name}."));
    };
    let sha_name = format!("{name}.sha256");
    Ok(Some(Release {
        notes: r.body.unwrap_or_default().replace('\r', ""),
        url: a.browser_download_url.clone(),
        size: a.size,
        digest: a.digest.as_deref().and_then(parse_digest),
        sha_url: r.assets.iter().find(|a| a.name == sha_name).map(|a| a.browser_download_url.clone()),
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

fn check(api: &str, local: bool) -> Result<Option<Release>, String> {
    let json = get(&agent(local), api, local, describe)?.body_mut().read_to_string().map_err(describe)?;
    parse_release(&json, CURRENT)
}

pub fn update_dir() -> PathBuf {
    std::env::temp_dir().join("MilanPro-update")
}

/// Delete `.part` files and `MilanPro-<ver>-Setup.exe` installers no newer than `current`; newer installers
/// stay (a "Later" download is reused) and so does anything else. A `.part` another Milan Pro window is still
/// writing survives: `create_part` opens it without delete sharing, so its removal fails. One written in the
/// last minute survives too, as `download` closes it (dropping that lock) just before renaming it.
pub fn clean_downloads(dir: &Path, current: &str) {
    let recent = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().is_ok_and(|age| age < Duration::from_secs(60)));
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let installer = name.strip_prefix("MilanPro-").and_then(|n| n.strip_suffix("-Setup.exe")).and_then(parse_version);
        if (name.ends_with(".part") && !recent(&e.path())) || installer.is_some_and(|v| newer(v, current) == Some(false)) {
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

/// Download and verify `rel`'s installer into `dir`; the file only gets its real name once its hash matches.
/// Returns the installer and its verified hash.
fn download(rel: &Release, dir: &Path, local: bool, cancel: &AtomicBool, progress: impl Fn(u64)) -> Result<(PathBuf, String), String> {
    let agent = agent(local);
    let expected = match (&rel.digest, &rel.sha_url) {
        (Some(h), _) => h.clone(),
        (None, Some(url)) => {
            let text = get(&agent, url, local, download_failed)?.body_mut().read_to_string().map_err(download_failed)?;
            parse_sha_file(&text).ok_or("The update's .sha256 file is not valid.")?
        }
        (None, None) => return Err("The update has no SHA-256 checksum, so it was not downloaded.".into()),
    };
    std::fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    clean_downloads(dir, CURRENT);
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
            Ok((exe, expected))
        }
        r => {
            let _ = std::fs::remove_file(&part);
            Err(r.err().unwrap_or_else(|| "The download's SHA-256 does not match the release; it was deleted.".into()))
        }
    }
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
    // A plain spawn, never proc::spawn: its Job Object is KILL_ON_JOB_CLOSE and would kill Setup as Milan exits.
    cmd.spawn().map(drop)
}

enum Msg {
    Checked(Result<Option<Release>, String>),
    Progress(u64),
    Downloaded(Result<(PathBuf, String), String>),
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
}

impl Updater {
    pub fn from_env() -> Self {
        // Only a local test server may replace the API, and only then does AUTO apply, so a stray or planted
        // variable can neither point the app at another host nor make it install without a click.
        let api = std::env::var("MILAN_UPDATE_API").ok().filter(|v| url_allowed(v, true) && !url_allowed(v, false));
        let auto = api.is_some() && std::env::var("MILAN_UPDATE_AUTO").is_ok_and(|v| v == "1");
        Self { state: State::Idle, manual: false, rx: None, cancel: Arc::default(), local: api.is_some(), api: api.unwrap_or(API.into()), auto }
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

    /// Check in the background after `delay`. Does nothing while a download is under way or ready.
    pub fn check(&mut self, ctx: &egui::Context, manual: bool, delay: Duration) {
        if matches!(self.state, State::Downloading(..) | State::Ready(..)) {
            return;
        }
        (self.manual, self.state) = (manual || self.auto, if manual { State::Checking } else { State::Idle });
        let (api, local) = (self.api.clone(), self.local);
        self.spawn(ctx, move |tx| {
            std::thread::sleep(delay);
            let _ = tx.send(Msg::Checked(check(&api, local)));
        });
    }

    pub fn download(&mut self, ctx: &egui::Context) {
        let State::Available(rel) = &self.state else { return };
        let rel = rel.clone();
        self.state = State::Downloading(rel.clone(), 0);
        self.cancel = Arc::default();
        let (cancel, local) = (self.cancel.clone(), self.local);
        let repaint = ctx.clone();
        self.spawn(ctx, move |tx| {
            let r = download(&rel, &update_dir(), local, &cancel, |n| {
                let _ = tx.send(Msg::Progress(n));
                repaint.request_repaint();
            });
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

    #[test]
    fn versions_compare_numerically_and_reject_prereleases() {
        assert!(parse_version("v0.10.0") > parse_version("0.9.0"));
        assert!(parse_version("v1.0.0") > parse_version("v0.99.99"));
        assert_eq!(parse_version("v0.2.0"), Some((0, 2, 0)));
        for bad in ["v0.3.0-beta.1", "0.3.0+build", "v0.3", "0.3.0.1", "v+1.0.0", "", "latest"] {
            assert_eq!(parse_version(bad), None, "{bad}");
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
        let (exe, sha) = download(&rel, &dir, false, &AtomicBool::new(false), |_| {}).unwrap();
        println!("latest {} -> {} ({sha})", rel.version, exe.display());
        assert_eq!(open_hashed(&exe).unwrap().1, sha);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn release_json(tag: &str, prerelease: bool, assets: &str) -> String {
        format!(r#"{{"tag_name":"{tag}","draft":false,"prerelease":{prerelease},"body":"Notes\r\nline 2","assets":[{assets}]}}"#)
    }
    const SETUP: &str = r#"{"name":"MilanPro-0.10.0-Setup.exe","browser_download_url":"https://github.com/a/b/x.exe","size":5,"digest":"sha256:ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef0123456789"}"#;
    const SHA: &str = r#"{"name":"MilanPro-0.10.0-Setup.exe.sha256","browser_download_url":"https://github.com/a/b/x.sha256","size":1}"#;
    const OTHER: &str = r#"{"name":"MilanPro-0.9.0-Setup.exe","browser_download_url":"https://github.com/a/b/old.exe","size":5}"#;

    #[test]
    fn release_selection() {
        let r = parse_release(&release_json("v0.10.0", false, &format!("{OTHER},{SETUP},{SHA}")), "0.9.0").unwrap().unwrap();
        assert_eq!((r.version.as_str(), r.url.as_str(), r.notes.as_str()), ("0.10.0", "https://github.com/a/b/x.exe", "Notes\nline 2"));
        assert_eq!(r.digest.as_deref(), Some("abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"));
        assert_eq!(r.sha_url.as_deref(), Some("https://github.com/a/b/x.sha256"));
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
    }

    #[test]
    fn clean_downloads_removes_old_installers_and_parts() {
        let dir = std::env::temp_dir().join(format!("milan-clean-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let removed = ["MilanPro-0.2.9-Setup.exe", "MilanPro-0.3.0-Setup.exe", "MilanPro-0.4.0-Setup.exe.12-34.part", "x.part"];
        let kept = ["MilanPro-0.3.1-Setup.exe", "MilanPro-1.0.0-Setup.exe", "MilanPro-0.1-Setup.exe", "notes.txt", "MilanPro-0.2.0-Setup.exe.sha256"];
        for n in removed.iter().chain(&kept) {
            std::fs::write(dir.join(n), b"x").unwrap();
        }
        // Left-over parts are old; a part just closed by a finished download (about to be renamed) is not.
        for n in &removed[2..] {
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
