//! Periodic backup of modified projects to %APPDATA%\MilanPro\autosave (never the user's file), and (R43) the session
//! markers that let the next start find what a crashed session left behind.
//!
//! - Preferences › General sets the interval (off, or 1-60 minutes; VEGAS's default of 2) and how many autosaves are
//!   kept per project: `<name>-autosave.mnle` is the newest, `<name>-autosave.1.mnle` the one before, and so on.
//! - R6b: the parents waiting on the nested-timeline stack are autosaved with the project shown.
//! - `%APPDATA%\MilanPro\sessions\<pid>.json` names this session's projects and their autosave files; it is removed on a
//!   clean exit. At the next start, a marker whose pid is no longer running (`process_running`) is a crashed session:
//!   its autosaves newer than their projects are offered in the Recover Projects dialog (`ui::dialogs::Recover`).
//! - `MILAN_APPDATA=<folder>` moves the autosave and sessions folders (the recovery tests and the screenshot harness,
//!   which never write into the user's AppData otherwise).
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use crate::project::Project;

/// The per-user data folder: `MILAN_APPDATA` when set, else %APPDATA%\MilanPro.
pub fn data_root() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("MILAN_APPDATA").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    Some(PathBuf::from(std::env::var_os("APPDATA")?).join("MilanPro"))
}

pub fn sessions_dir() -> Option<PathBuf> {
    Some(data_root()?.join("sessions"))
}

pub fn autosave_path(name: &str) -> Option<PathBuf> {
    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || " -_.".contains(c) { c } else { '_' }).collect();
    Some(data_root()?.join("autosave").join(format!("{}-autosave.mnle", safe.trim())))
}

/// Saved projects are named after their file plus a hash of its full path (two "Untitled.mnle" in different folders
/// get their own backups); unsaved ones get the pid so instances don't collide.
fn backup_name(project: &Project) -> String {
    backup_name_for(&project.name, project.path.as_deref())
}

fn backup_name_for(name: &str, path: Option<&Path>) -> String {
    use std::hash::{Hash, Hasher};
    match path {
        Some(p) => {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            p.to_string_lossy().to_lowercase().hash(&mut h);
            format!("{name}-{:08x}", h.finish() as u32)
        }
        None => format!("{name}-{}", std::process::id()),
    }
}

/// R42: the autosave file of the project saved at `path` (whether or not there is one).
pub fn autosave_for(path: &Path) -> Option<PathBuf> {
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    autosave_path(&backup_name_for(&name, Some(path)))
}

/// The `k`th older autosave of `newest` (`k` ≥ 1): `Show-1a2b3c4d-autosave.mnle` → `Show-1a2b3c4d-autosave.1.mnle`.
pub fn rotated(newest: &Path, k: u32) -> PathBuf {
    let stem = newest.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = newest.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "mnle".into());
    newest.with_file_name(format!("{stem}.{k}.{ext}"))
}

/// Make room for a new `newest`: the oldest kept copy goes, the others move one place down, and `newest` becomes `.1`.
/// `keep` is the number of files kept in all (at least 1: the newest alone).
pub fn rotate(newest: &Path, keep: u32) {
    let keep = keep.max(1);
    // With keep == 1 the newest is simply written over.
    if keep > 1 {
        let _ = std::fs::remove_file(rotated(newest, keep - 1));
        for k in (1..keep - 1).rev() {
            let _ = std::fs::rename(rotated(newest, k), rotated(newest, k + 1));
        }
        let _ = std::fs::rename(newest, rotated(newest, 1));
    }
    // Copies beyond the kept count (the preference was lowered) go too.
    for k in keep.. {
        if std::fs::remove_file(rotated(newest, k)).is_err() {
            break;
        }
    }
}

/// Every autosave of `newest` on disk, newest first.
pub fn all_copies(newest: &Path) -> Vec<PathBuf> {
    std::iter::once(newest.to_path_buf()).chain((1..).map(|k| rotated(newest, k))).take_while(|p| p.exists()).collect()
}

/// One project of a session, as its marker records it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionProject {
    pub name: String,
    pub path: Option<PathBuf>,
    pub autosave: PathBuf,
}

/// `sessions/<pid>.json`: what a running Milan Pro has open (the project shown last, then the parents waiting on the
/// nested-timeline stack) and where their autosaves go.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionMarker {
    pub pid: u32,
    /// Unix seconds when the session started.
    pub start: u64,
    pub version: String,
    pub projects: Vec<SessionProject>,
}

impl SessionMarker {
    fn new(projects: Vec<SessionProject>) -> Self {
        let start = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        Self { pid: std::process::id(), start, version: env!("CARGO_PKG_VERSION").into(), projects }
    }

    pub fn file(dir: &Path, pid: u32) -> PathBuf {
        dir.join(format!("{pid}.json"))
    }

    pub fn write(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let path = Self::file(dir, self.pid);
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).map_err(std::io::Error::other)?)?;
        std::fs::rename(&tmp, &path)
    }
}

/// Whether a process with `pid` is still running (Windows: it can be opened and has not exited).
pub fn process_running(pid: u32) -> bool {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            // Access denied means it exists (another user's process); any other failure means it is gone.
            return std::io::Error::last_os_error().raw_os_error() == Some(5);
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code) != 0;
        CloseHandle(h);
        ok && code == STILL_ACTIVE as u32
    }
    #[cfg(not(windows))]
    {
        Path::new(&format!("/proc/{pid}")).exists()
    }
}

/// The markers in `dir` of sessions that are not running any more (never this process's own), with their files.
pub fn stale_sessions(dir: &Path, running: impl Fn(u32) -> bool) -> Vec<(PathBuf, SessionMarker)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = e.path();
        if path.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let Ok(m) = std::fs::read_to_string(&path).map_err(|_| ()).and_then(|s| serde_json::from_str::<SessionMarker>(&s).map_err(|_| ())) else {
            // Not a marker (or half-written by a session that died mid-write): nothing to recover from it.
            let _ = std::fs::remove_file(&path);
            continue;
        };
        if m.pid != std::process::id() && !running(m.pid) {
            out.push((path, m));
        }
    }
    out.sort_by_key(|(_, m)| m.start);
    out
}

/// A project a crashed session left an autosave of, newer than the project file (or never saved at all).
#[derive(Clone, Debug, PartialEq)]
pub struct Recoverable {
    pub marker: PathBuf,
    pub name: String,
    pub path: Option<PathBuf>,
    pub autosave: PathBuf,
    pub saved_at: SystemTime,
}

fn modified(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// What the stale markers offer. A marker with nothing to recover (its autosaves gone, or no newer than the projects)
/// is removed here, so it is not looked at again.
pub fn recoverable(stale: Vec<(PathBuf, SessionMarker)>) -> Vec<Recoverable> {
    let mut out = Vec::new();
    for (marker, m) in stale {
        let before = out.len();
        for sp in m.projects {
            let Some(saved_at) = modified(&sp.autosave) else { continue };
            let newer = sp.path.as_deref().and_then(modified).is_none_or(|t| saved_at > t);
            if newer {
                out.push(Recoverable { marker: marker.clone(), name: sp.name, path: sp.path, autosave: sp.autosave, saved_at });
            }
        }
        if out.len() == before {
            let _ = std::fs::remove_file(&marker);
        }
    }
    out
}

impl Recoverable {
    /// The autosave as an unsaved copy of the project: Save asks where, so the original file is left as it was.
    pub fn open_copy(&self) -> anyhow::Result<Project> {
        let mut p = Project::load(&self.autosave)?;
        p.name = self.path.as_deref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| self.name.clone());
        (p.path, p.modified) = (None, true);
        Ok(p)
    }

    /// Discard: the autosave and its older copies.
    pub fn delete_files(&self) {
        for f in all_copies(&self.autosave) {
            let _ = std::fs::remove_file(f);
        }
    }
}

/// The screenshot harness (`MILAN_DIALOG=recover`): two made-up crashed sessions beside the demo media, one of a saved
/// project (`demo`, saved there, then autosaved with one more edit) and one never saved.
pub fn demo_items(demo: &Project) -> Vec<Recoverable> {
    let dir = crate::demo::media_dir().join("recover");
    let _ = std::fs::create_dir_all(&dir);
    let mut p = demo.clone();
    let saved = dir.join("Wedding Highlights.mnle");
    let _ = p.save(&saved);
    let autosave = dir.join("Wedding Highlights-1a2b3c4d-autosave.mnle");
    p.timeline.markers.push(crate::project::Marker { position: 4.0, name: "late edit".into(), color: [255, 160, 0] });
    let _ = std::fs::write(&autosave, serde_json::to_string_pretty(&p).unwrap_or_default());
    let untitled = dir.join("Untitled-4120-autosave.mnle");
    let _ = std::fs::write(&untitled, serde_json::to_string_pretty(demo).unwrap_or_default());
    let now = SystemTime::now();
    vec![
        Recoverable { marker: dir.join("4120.json"), name: "Wedding Highlights".into(), path: Some(saved), autosave, saved_at: now - Duration::from_secs(1500) },
        Recoverable { marker: dir.join("4120.json"), name: "Untitled".into(), path: None, autosave: untitled, saved_at: now - Duration::from_secs(120) },
    ]
}

#[derive(Default)]
pub struct Autosave {
    last: Option<f64>,
    error: Arc<Mutex<Option<String>>>,
    /// The marker last written (None: none yet), so it is written again only when the projects change.
    marker: Option<Vec<SessionProject>>,
    /// Test hook (`MILAN_AUTOSAVE_NOW`): the first tick autosaves at once, modified or not.
    pub force_first: bool,
}

impl Autosave {
    /// Once a frame: `shown` and `parents` (R6b, the projects waiting under a nested timeline) are backed up every
    /// `minutes` (0: never) when modified, `keep` copies each; the session marker follows what is open.
    pub fn tick(&mut self, ctx: &egui::Context, shown: &Project, parents: &[&Project], minutes: u32, keep: u32) {
        let projects: Vec<&Project> = std::iter::once(shown).chain(parents.iter().copied()).collect();
        self.mark(&projects);
        let now = ctx.input(|i| i.time);
        let force = std::mem::take(&mut self.force_first);
        let interval = f64::from(minutes.clamp(0, 60)) * 60.0;
        if interval <= 0.0 && !force {
            self.last = Some(now);
            return;
        }
        let last = *self.last.get_or_insert(now);
        if now - last < interval && !force {
            ctx.request_repaint_after(Duration::from_secs_f64(interval - (now - last)));
            return;
        }
        self.last = Some(now);
        if interval > 0.0 {
            ctx.request_repaint_after(Duration::from_secs_f64(interval));
        }
        for project in projects.into_iter().filter(|p| p.modified || force) {
            let Some(path) = autosave_path(&backup_name(project)).filter(|p| Some(p) != project.path.as_ref()) else { continue };
            let json = match serde_json::to_string_pretty(project) {
                Ok(j) => j,
                Err(e) => {
                    *self.error.lock().unwrap() = Some(format!("Autosave failed: {e}"));
                    continue;
                }
            };
            let error = self.error.clone();
            std::thread::spawn(move || {
                // Write then rotate and rename, so a crash mid-write never destroys the previous good backup.
                let tmp = path.with_extension(format!("{}.tmp", std::process::id())); // two windows never share one
                let res = path
                    .parent()
                    .map_or(Ok(()), std::fs::create_dir_all)
                    .and_then(|_| std::fs::write(&tmp, json))
                    .and_then(|_| {
                        rotate(&path, keep);
                        std::fs::rename(&tmp, &path)
                    });
                match &res {
                    Ok(()) => log::debug!("autosaved {}", path.display()),
                    Err(e) => log::warn!("autosave {}: {e}", path.display()),
                }
                *error.lock().unwrap() = res.err().map(|e| format!("Autosave failed: {e}"));
            });
        }
    }

    /// Write the session marker when the set of open projects changed.
    fn mark(&mut self, projects: &[&Project]) {
        let list: Vec<SessionProject> = projects
            .iter()
            .filter_map(|p| Some(SessionProject { name: p.name.clone(), path: p.path.clone(), autosave: autosave_path(&backup_name(p))? }))
            .collect();
        if self.marker.as_ref() == Some(&list) {
            return;
        }
        if let Some(dir) = sessions_dir() {
            if let Err(e) = SessionMarker::new(list.clone()).write(&dir) {
                log::warn!("session marker: {e}");
            }
        }
        self.marker = Some(list);
    }

    /// A clean exit: nothing to recover from this session.
    pub fn clean_exit(&mut self) {
        if let Some(dir) = sessions_dir() {
            let _ = std::fs::remove_file(SessionMarker::file(&dir, std::process::id()));
        }
        self.marker = None;
    }

    pub fn error(&self) -> Option<String> {
        self.error.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join("milan-agent").join("r42").join(format!("{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn path_is_sanitized_and_in_appdata() {
        if std::env::var_os("MILAN_APPDATA").is_some() {
            return;
        }
        let Some(p) = super::autosave_path("My: Project?") else { return };
        assert!(p.ends_with("MilanPro/autosave/My_ Project_-autosave.mnle"));
    }

    #[test]
    fn same_named_projects_get_their_own_backup() {
        let at = |path: &str| {
            let mut p = crate::project::Project::new("Untitled");
            p.path = Some(path.into());
            super::backup_name(&p)
        };
        assert_ne!(at(r"C:\Jobs\A\Untitled.mnle"), at(r"C:\Jobs\B\Untitled.mnle"));
        assert_eq!(at(r"C:\Jobs\A\Untitled.mnle"), at(r"c:\jobs\a\Untitled.mnle"));
        // R42: the autosave of a file is found from its path alone.
        let mut p = crate::project::Project::new("Show");
        p.path = Some(r"C:\Jobs\A\Show.mnle".into());
        assert_eq!(autosave_for(Path::new(r"C:\Jobs\A\Show.mnle")), autosave_path(&backup_name(&p)));
    }

    /// R43: rotation keeps N copies, newest first.
    #[test]
    fn rotation_keeps_n_copies() {
        let dir = temp("rotate");
        let newest = dir.join("Show-12345678-autosave.mnle");
        assert_eq!(rotated(&newest, 2), dir.join("Show-12345678-autosave.2.mnle"));
        let write = |n: usize, keep: u32| {
            rotate(&newest, keep);
            std::fs::write(&newest, format!("v{n}")).unwrap();
        };
        for n in 1..=6 {
            write(n, 3);
        }
        let copies = all_copies(&newest);
        assert_eq!(copies.len(), 3, "{copies:?}");
        let contents: Vec<String> = copies.iter().map(|p| std::fs::read_to_string(p).unwrap()).collect();
        assert_eq!(contents, ["v6", "v5", "v4"]);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 3, "nothing beyond the kept copies");
        // Lowering the count trims the older copies; keep 1 is the newest alone.
        write(7, 2);
        assert_eq!(all_copies(&newest).len(), 2);
        write(8, 1);
        assert_eq!(all_copies(&newest).len(), 1);
        assert_eq!(std::fs::read_to_string(&newest).unwrap(), "v8");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// R43: a marker whose pid is not running is stale; this process's own and running ones are not.
    #[test]
    fn stale_marker_is_detected() {
        let dir = temp("stale");
        let fake_pid = 0x7FFF_FFF1; // no such process (pids are multiples of 4 on Windows)
        let own = SessionMarker::new(vec![]);
        own.write(&dir).unwrap();
        let mut dead = SessionMarker::new(vec![SessionProject { name: "Show".into(), path: None, autosave: dir.join("Show-autosave.mnle") }]);
        dead.pid = fake_pid;
        dead.write(&dir).unwrap();
        let mut alive = dead.clone();
        alive.pid = 1234;
        alive.write(&dir).unwrap();
        std::fs::write(dir.join("junk.json"), "{").unwrap();
        let running = |pid: u32| pid == 1234;
        let stale = stale_sessions(&dir, running);
        assert_eq!(stale.len(), 1, "{stale:?}");
        assert_eq!((stale[0].1.pid, stale[0].0.clone()), (fake_pid, SessionMarker::file(&dir, fake_pid)));
        assert!(!dir.join("junk.json").exists(), "an unreadable marker is dropped");
        assert!(process_running(std::process::id()) && !process_running(fake_pid), "the real check");
        // Nothing to recover (no autosave file): the stale marker goes.
        assert!(recoverable(stale).is_empty());
        assert!(!SessionMarker::file(&dir, fake_pid).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// R43: a recovered project opens as an unsaved copy and the original file is untouched; Discard removes the files.
    #[test]
    fn recovered_project_opens_unsaved_with_the_original_untouched() {
        let dir = temp("recover");
        let original = dir.join("Show.mnle");
        let mut p = crate::project::Project::new("x");
        p.save(&original).unwrap();
        let bytes = std::fs::read(&original).unwrap();
        let autosave = dir.join("Show-abcdef01-autosave.mnle");
        p.timeline.video_tracks[0].clips.push(crate::project::Clip::new("late edit", 1.0, 2.0));
        std::thread::sleep(Duration::from_millis(30));
        std::fs::write(&autosave, serde_json::to_string(&p).unwrap()).unwrap();
        std::fs::write(rotated(&autosave, 1), "older").unwrap();
        // An autosave older than its project is not offered; one without a project (never saved) is.
        let stale_old = dir.join("Old-autosave.mnle");
        std::fs::write(&stale_old, serde_json::to_string(&p).unwrap()).unwrap();
        let old_time = SystemTime::now() - Duration::from_secs(3600);
        std::fs::File::options().write(true).open(&stale_old).unwrap().set_modified(old_time).unwrap();
        let unsaved = dir.join("Untitled-99-autosave.mnle");
        std::fs::write(&unsaved, serde_json::to_string(&p).unwrap()).unwrap();
        let marker = SessionMarker::file(&dir, 77);
        let items = recoverable(vec![(
            marker.clone(),
            SessionMarker {
                pid: 77,
                start: 0,
                version: "0".into(),
                projects: vec![
                    SessionProject { name: "Show".into(), path: Some(original.clone()), autosave: autosave.clone() },
                    SessionProject { name: "Old".into(), path: Some(original.clone()), autosave: stale_old.clone() },
                    SessionProject { name: "Untitled".into(), path: None, autosave: unsaved.clone() },
                ],
            },
        )]);
        assert_eq!(items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["Show", "Untitled"], "{items:?}");
        let copy = items[0].open_copy().unwrap();
        assert_eq!((copy.name.as_str(), copy.path.is_none(), copy.modified), ("Show", true, true));
        assert_eq!(copy.timeline.video_tracks[0].clips.len(), 1, "the autosave's edit is there");
        assert_eq!(std::fs::read(&original).unwrap(), bytes, "the original file is untouched");
        assert!(autosave.exists(), "Open leaves the autosave for Discard or Keep");
        items[0].delete_files();
        assert!(!autosave.exists() && !rotated(&autosave, 1).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// R43 end to end: the app is started off-screen (the screenshot harness, held open), autosaves the demo project
    /// and is killed; the next start finds the stale marker and shows the Recover Projects dialog (screenshot under
    /// %TEMP%\milan-review\r42). Needs target\release\milan-pro.exe:
    /// cargo build --release && cargo test --release crash_recovery_e2e -- --ignored --test-threads=1
    #[test]
    #[ignore]
    fn crash_recovery_e2e() {
        let exe = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().join("milan-pro.exe");
        assert!(exe.is_file(), "{} is not built", exe.display());
        let root = temp("e2e");
        let review = std::env::temp_dir().join("milan-review").join("r42");
        std::fs::create_dir_all(&review).unwrap();
        let launch = |hold: bool, shot: &Path| {
            let mut c = std::process::Command::new(&exe);
            c.env("MILAN_APPDATA", &root).env("MILAN_DEMO", "1").env("MILAN_LOOK", "classic2006").env("MILAN_SCREENSHOT", shot).env("MILAN_AUTOSAVE_NOW", "1");
            if hold {
                c.env("MILAN_HOLD", "1");
            }
            c.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
            c.spawn().expect("milan-pro.exe starts")
        };
        // 1. A session that will crash: held open until its marker and autosave are on disk.
        let mut first = launch(true, &root.join("hold.png"));
        let marker = SessionMarker::file(&sessions_dir_in(&root), first.id());
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        let autosaved = loop {
            let m = std::fs::read_to_string(&marker).ok().and_then(|s| serde_json::from_str::<SessionMarker>(&s).ok());
            if let Some(m) = m.filter(|m| m.projects.iter().all(|p| p.autosave.is_file())) {
                break m;
            }
            assert!(std::time::Instant::now() < deadline, "no marker or autosave within 60 s");
            assert!(first.try_wait().unwrap().is_none(), "the held app exited on its own");
            std::thread::sleep(Duration::from_millis(200));
        };
        assert_eq!(autosaved.projects[0].name, "Milan Demo");
        // Killed by pid, never by image name: only this test's own instance.
        let killed = std::process::Command::new("taskkill").args(["/F", "/PID", &first.id().to_string()]).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().unwrap();
        assert!(killed.success());
        let _ = first.wait();
        // 2. What the next start sees: one stale session with the demo project to recover.
        let stale = stale_sessions(&sessions_dir_in(&root), process_running);
        assert_eq!(stale.len(), 1, "{stale:?}");
        let items = recoverable(stale);
        assert_eq!(items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["Milan Demo"], "{items:?}");
        assert!(items[0].path.is_none(), "the demo project was never saved");
        // 3. The next start shows the Recover Projects dialog and takes the review screenshot.
        let shot = review.join("recover-dialog-e2e.png");
        let _ = std::fs::remove_file(&shot);
        let mut second = launch(false, &shot);
        let deadline = std::time::Instant::now() + Duration::from_secs(90);
        while second.try_wait().unwrap().is_none() {
            if std::time::Instant::now() > deadline {
                let _ = std::process::Command::new("taskkill").args(["/F", "/PID", &second.id().to_string()]).status();
                panic!("the second run did not finish its screenshot in 90 s");
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        assert!(shot.is_file(), "no screenshot at {}", shot.display());
        assert!(marker.is_file(), "nothing was clicked, so the stale marker stays for later");
        assert!(!SessionMarker::file(&sessions_dir_in(&root), second.id()).exists(), "the clean exit removed its own marker");
        let _ = std::fs::remove_dir_all(&root);
    }

    fn sessions_dir_in(root: &Path) -> PathBuf {
        root.join("sessions")
    }
}
