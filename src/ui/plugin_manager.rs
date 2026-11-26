//! R31: the Plug-In Manager (View › Window › Plug-In Manager, Ctrl+Alt+1), as in VEGAS: the plug-ins Milan Pro has
//! (built in, the OFX plug-ins found and, R34a/R35, the VST3 and CLAP effects) with their grouping, version and status
//! (OK / Failed with the reason / Disabled), a check box to turn an external plug-in on or off, Rescan, and the folders
//! searched (Add Folder; the VST folders also in Preferences › VST Effects). A1: AE-format effects, behind the
//! experimental switch in Folders (off by default), with the trademark notice next to it.
//! Scanning runs on a thread of its own; the plug-ins are described in plug-in host processes, never in Milan.
use eframe::egui::*;
use std::path::PathBuf;
use std::sync::mpsc;
use crate::engine::{aex, aplug};
use crate::engine::ofx::{self, OfxPrefs, ScanCache, Status, scan};
use crate::engine::plugins::{self, PluginKind};
use super::icons::{Icon, paint_icon};
use super::theme::{self, UiLook};
use super::toolbar::tool_button;
use super::widgets;

const COLS: [(&str, f32); 5] = [("Name", 230.0), ("Type", 96.0), ("Grouping", 160.0), ("Status", 80.0), ("Version", 70.0)];
const CHECK_W: f32 = 22.0;

pub struct PluginManager {
    /// The last scan.
    pub cache: ScanCache,
    job: Option<mpsc::Receiver<ScanCache>>,
    /// plugins.json; None in the screenshot harness, which never reads or writes the user's files.
    cache_file: Option<PathBuf>,
    /// The folders searched (None: the standard ones, OFX_PLUGIN_PATH and the user's).
    only: Option<Vec<PathBuf>>,
    /// A1: the AE-format folders searched while that feature is on (None: the shared ones and the user's).
    only_aex: Option<Vec<PathBuf>>,
    /// The Folders popup is open.
    pub folders_open: bool,
    query: String,
    /// The selected row (plug-in id, or binary path for a binary that failed whole).
    sel: Option<String>,
    /// For the status bar (a scan that failed, a save that failed, a plug-in that crashed).
    pub message: Option<String>,
}

/// One row: (key, name, type, grouping, status, version, details).
struct Row {
    key: String,
    id: String,
    name: String,
    kind: &'static str,
    group: String,
    status: Status,
    version: String,
    file: String,
    /// A1: an AE-format effect's capability line, and the note when an OFX plug-in of the same name is installed.
    caps: String,
}

impl PluginManager {
    /// `cache_file` None: the harness (searches only `only`, keeps nothing).
    pub fn new(cache_file: Option<PathBuf>, only: Option<Vec<PathBuf>>) -> Self {
        let cache = cache_file.as_deref().map(ScanCache::load).unwrap_or_default();
        Self { cache, job: None, cache_file, only, only_aex: None, folders_open: false, query: String::new(), sel: None, message: None }
    }

    /// A1: the harness searches only these AE-format folders (while the feature is on).
    pub fn with_aex(mut self, only: Option<Vec<PathBuf>>) -> Self {
        self.only_aex = only;
        self
    }

    pub fn scanning(&self) -> bool {
        self.job.is_some()
    }

    fn folders(&self, prefs: &OfxPrefs) -> Vec<PathBuf> {
        self.only.clone().unwrap_or_else(|| scan::folders(prefs).into_iter().chain(scan::audio_folders(prefs)).collect())
    }

    /// A1: the AE-format folders: none while the feature is off.
    fn aex_folders(&self, prefs: &OfxPrefs) -> Vec<PathBuf> {
        if !prefs.aex.enabled {
            return Vec::new();
        }
        self.only_aex.clone().unwrap_or_else(|| aex::folders(&prefs.aex))
    }

    /// Scan on a thread of its own (`poll` takes the result). Unchanged binaries keep their entries unless `force`
    /// (Rescan), which also tries failed plug-ins again.
    pub fn rescan(&mut self, ctx: Option<&Context>, prefs: &OfxPrefs, force: bool) {
        if self.job.is_some() {
            return;
        }
        if force {
            ofx::clear_failures();
            ofx::close_hosts();
        }
        let (dirs, aex_dirs, old, (tx, rx), ctx) = (self.folders(prefs), self.aex_folders(prefs), self.cache.clone(), mpsc::channel(), ctx.cloned());
        let spawned = std::thread::Builder::new().name("milan-plugin-scan".into()).spawn(move || {
            let _ = tx.send(scan::scan_all(&dirs, &aex_dirs, &old, force, &scan::describe_binary));
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
        match spawned {
            Ok(_) => self.job = Some(rx),
            Err(e) => self.message = Some(format!("Could not scan the plug-in folders: {e}")),
        }
    }

    /// Scan now, on this thread (the screenshot harness).
    pub fn rescan_now(&mut self, prefs: &OfxPrefs) {
        self.cache = scan::scan_all(&self.folders(prefs), &self.aex_folders(prefs), &self.cache, false, &scan::describe_binary);
    }

    /// A scan that finished becomes the plug-in list (and the cache file). True when it did.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.job else { return false };
        match rx.try_recv() {
            Ok(c) => {
                self.job = None;
                if let Some(f) = self.cache_file.as_deref().filter(|f| c != self.cache || !f.exists()) {
                    if let Err(e) = c.save(f) {
                        self.message = Some(format!("Could not save the plug-in list: {e}"));
                    }
                }
                self.cache = c;
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.job = None;
                self.message = Some("The plug-in scan stopped before it finished; the last list is kept.".into());
                false
            }
        }
    }

    fn rows(&self) -> Vec<Row> {
        let kind = |k: PluginKind| match k {
            PluginKind::VideoFx => "Video FX",
            PluginKind::Transition => "Transition",
            PluginKind::Generator => "Generator",
            PluginKind::AudioFx => "Audio FX",
            PluginKind::Compositor => "Compositor",
        };
        let builtins = plugins::PLUGINS.iter().map(|p| Row {
            key: p.id.into(),
            id: p.id.into(),
            name: p.name.into(),
            kind: kind(p.kind),
            group: format!("Milan\\{}", p.group),
            status: Status::Ok,
            version: env!("CARGO_PKG_VERSION").into(),
            file: "Built in".into(),
            caps: String::new(),
        });
        let entries: Vec<ofx::Entry> = ofx::entries().into_iter().chain(aplug::entries()).collect();
        let ofx_names: std::collections::HashSet<String> = entries.iter().filter(|e| e.kind.starts_with("OFX")).map(|e| e.name.to_lowercase()).collect();
        let found = entries.into_iter().map(|e| {
            let caps = if e.kind == aex::KIND { self.aex_caps(&e, &ofx_names) } else { String::new() };
            Row {
                key: if e.id.is_empty() { e.binary.display().to_string() } else { e.id.clone() },
                id: e.id,
                name: e.name,
                kind: e.kind,
                group: e.group,
                status: e.status,
                version: e.version,
                file: e.binary.display().to_string(),
                caps,
            }
        });
        let q = self.query.trim().to_lowercase();
        let mut v: Vec<Row> = found.chain(builtins).filter(|r| q.is_empty() || [&r.name, &r.group, &r.id, &r.file, &r.kind.to_string(), &status_name(r).to_string()].iter().any(|s| s.to_lowercase().contains(&q))).collect();
        v.sort_by(|a, b| (!external(a.kind), a.kind, a.group.to_lowercase(), a.name.to_lowercase()).cmp(&(!external(b.kind), b.kind, b.group.to_lowercase(), b.name.to_lowercase())));
        v
    }
}

/// An external plug-in's row kind (OFX, VST3, CLAP, AE-format): it has a check box and a file.
fn external(kind: &str) -> bool {
    kind.starts_with("OFX") || kind == "VST3" || kind == "CLAP" || kind == aex::KIND
}

/// The status a row shows: an AE-format effect that loads is "Untested" unless a compatibility test was recorded for it
/// (plan §2.6 (7)).
fn status_name(r: &Row) -> &'static str {
    match &r.status {
        Status::Ok if r.kind == aex::KIND && !aex::tested(r.id.strip_prefix(aex::PREFIX).unwrap_or_default()) => "Untested",
        s => s.name(),
    }
}

impl PluginManager {
    /// An AE-format effect's details: its capability line, what it asked for that Milan does not serve, and a pointer
    /// to the OpenFX version when one of the same name is installed (plan §2.6 (9)).
    fn aex_caps(&self, e: &ofx::Entry, ofx_names: &std::collections::HashSet<String>) -> String {
        let info = self.cache.binaries.iter().filter(|b| b.path == e.binary).flat_map(|b| &b.plugins).find(|d| ofx::id_of(d) == e.id).and_then(|d| d.aex.as_ref());
        let mut parts: Vec<String> = info.map(|a| a.caps.clone()).unwrap_or_default();
        if let Some(a) = info.filter(|a| !a.refused.is_empty()) {
            parts.push(format!("asked for what Milan Pro does not provide: {}", a.refused.join(", ")));
        }
        if ofx_names.contains(&e.name.to_lowercase()) {
            parts.push("an OpenFX version is installed: use that instead".into());
        }
        parts.join("; ")
    }
}

impl PluginManager {
    /// R34a: the VST3 and CLAP effects found and OK, for the toolbar's count.
    fn audio_counts() -> (usize, usize) {
        let a = aplug::entries();
        (a.len(), a.iter().filter(|e| e.status == Status::Ok).count())
    }
}

/// What the window asks of the app this frame.
#[derive(Default)]
pub struct ManagerOut {
    pub rescan: bool,
    /// The OFX preferences changed (a plug-in on or off, a folder added or removed): save them, rebuild the registry.
    pub prefs_changed: bool,
}

/// Toolbar: search, Rescan, the folders, and the counts.
pub fn plugin_manager_toolbar(ui: &mut Ui, pm: &mut PluginManager, prefs: &mut OfxPrefs) -> ManagerOut {
    let t = theme::get();
    let mut out = ManagerOut::default();
    let (icon, _) = ui.allocate_exact_size(vec2(22.0, t.m.button), Sense::hover());
    paint_icon(ui.painter(), Rect::from_center_size(icon.center(), vec2(16.0, 16.0)), Icon::Search, t.look(), None);
    widgets::search_field(ui, &mut pm.query, "Search plug-ins", 160.0);
    ui.add_space(6.0);
    out.rescan = tool_button(ui, Icon::Refresh, "Rescan: read every OFX, VST3 and CLAP plug-in again and try failed ones again", !pm.scanning(), false).clicked();
    let folders = ui.button("Folders");
    if folders.clicked() {
        pm.folders_open = !pm.folders_open;
    }
    if pm.folders_open {
        let area = Area::new(ui.id().with("plugin_folders")).order(Order::Foreground).fixed_pos(folders.rect.left_bottom()).show(ui.ctx(), |ui| {
            Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(560.0);
                folders_menu(ui, pm, prefs, &mut out);
            })
        });
        let outside = ui.input(|i| i.pointer.any_pressed()) && !area.response.contains_pointer() && !folders.contains_pointer();
        if outside || ui.input(|i| i.key_pressed(Key::Escape)) {
            pm.folders_open = false;
        }
    }
    ui.add_space(8.0);
    let found = ofx::entries();
    let text = if pm.scanning() {
        "Scanning plug-in folders...".to_string()
    } else {
        let (ae, other): (Vec<_>, Vec<_>) = found.iter().partition(|e| e.kind == aex::KIND);
        let ok = other.iter().filter(|e| e.status == Status::Ok).count();
        let (a, a_ok) = PluginManager::audio_counts();
        let ae = if prefs.aex.enabled { format!("; {} AE-format found, {} loaded", ae.len(), ae.iter().filter(|e| e.status == Status::Ok).count()) } else { String::new() };
        format!("{} built in; {} OFX found, {ok} OK; {a} VST3/CLAP found, {a_ok} OK{ae}", plugins::PLUGINS.len(), other.len())
    };
    ui.label(RichText::new(text).font(t.font(t.font.ui)).color(t.text_dim));
    out
}

/// Body: the list, then the selected plug-in's details.
pub fn plugin_manager(ui: &mut Ui, pm: &mut PluginManager, prefs: &mut OfxPrefs) -> ManagerOut {
    let t = theme::get();
    let mut out = ManagerOut::default();
    let area = ui.max_rect();
    let strip_h = 26.0;
    let body = Rect::from_min_max(area.min, pos2(area.max.x, area.max.y - strip_h));
    let rows = pm.rows();
    let row_h = if t.look() == UiLook::Vegas16 { widgets::ROW_H } else { 26.0 };
    widgets::scroll_pane(ui, body, "plugin_list", |ui| {
        let cols: Vec<(&str, f32)> = [("", CHECK_W)].into_iter().chain(COLS).collect();
        widgets::header_row(ui, &cols);
        if rows.is_empty() {
            ui.add_space(8.0);
            ui.label(RichText::new(if pm.scanning() { "Scanning plug-in folders..." } else { "No plug-ins match." }).color(t.text_dim));
        }
        for r in &rows {
            let sel = pm.sel.as_ref() == Some(&r.key);
            let resp = widgets::list_row(ui, row_h, sel);
            let ext = external(r.kind) && !r.id.is_empty();
            let on = !ext || !prefs.disabled.contains(&r.id);
            let bx = Rect::from_center_size(pos2(resp.rect.min.x + CHECK_W * 0.5, resp.rect.center().y), vec2(12.0, 12.0));
            let hit = ui.interact(bx.expand(3.0), resp.id.with("on"), if ext { Sense::click() } else { Sense::hover() });
            let p = ui.painter();
            if t.classic() {
                widgets::sunken(p, bx, if ext { t.input_bg } else { t.metal.face }, &t.metal);
            } else {
                p.rect_filled(bx, 0.0, t.input_bg);
                p.rect_stroke(bx, 0.0, Stroke::new(1.0_f32, if ext { t.text_dim } else { t.text_dim.gamma_multiply(0.5) }), StrokeKind::Inside);
            }
            if on {
                let c = bx.center();
                let s = Stroke::new(1.5_f32, if !ext { t.text_dim } else if sel { Color32::WHITE } else { t.text });
                p.line_segment([c + vec2(-3.5, 0.0), c + vec2(-1.0, 2.5)], s);
                p.line_segment([c + vec2(-1.0, 2.5), c + vec2(3.5, -3.0)], s);
            }
            if hit.clicked() {
                if on {
                    prefs.disabled.insert(r.id.clone());
                } else {
                    prefs.disabled.remove(&r.id);
                }
                out.prefs_changed = true;
            }
            hit.on_hover_text(if !ext { "Built-in plug-ins are always on" } else if on { "On: offered in the FX choosers. Click to turn it off" } else { "Off: not offered or run; projects that use it keep it, bypassed. Click to turn it on" });
            let color = if sel { Color32::WHITE } else if !on { t.text_dim } else { t.text };
            let status_color = if matches!(r.status, Status::Failed(_)) && !sel { t.warning_text } else { color };
            let x = resp.rect.min.x + CHECK_W;
            paint_icon(ui.painter(), Rect::from_min_size(pos2(x + 4.0, resp.rect.center().y - 8.0), vec2(16.0, 16.0)), Icon::FxBadge, t.look(), sel.then_some(Color32::WHITE));
            widgets::row_cells(ui, Rect::from_min_max(pos2(x + 18.0, resp.rect.min.y), resp.rect.max), &[("", COLS[0].1 - 18.0)], std::slice::from_ref(&r.name), color);
            let at = |k: usize| x + COLS[..k].iter().map(|c| c.1).sum::<f32>();
            widgets::row_cells(ui, Rect::from_min_max(pos2(at(1), resp.rect.min.y), resp.rect.max), &COLS[1..3], &[r.kind.to_string(), r.group.clone()], color);
            widgets::row_cells(ui, Rect::from_min_max(pos2(at(3), resp.rect.min.y), resp.rect.max), &COLS[3..4], &[status_name(r).to_string()], status_color);
            widgets::row_cells(ui, Rect::from_min_max(pos2(at(4), resp.rect.min.y), resp.rect.max), &COLS[4..], std::slice::from_ref(&r.version), color);
            if resp.clicked() {
                pm.sel = Some(r.key.clone());
            }
            let why = match &r.status {
                Status::Failed(w) => format!("Failed: {w}\n"),
                Status::Blocked(w) => format!("Blocked: {w}\n"),
                _ => String::new(),
            };
            let caps = if r.caps.is_empty() { String::new() } else { format!("{}\n", r.caps) };
            let notice = if r.kind == aex::KIND { format!("\n{}", aex::NOTICE) } else { String::new() };
            resp.on_hover_text(format!("{why}{caps}{}{notice}", r.file));
        }
    });
    // Details: "Name, OFX, status: why, file".
    let strip = Rect::from_min_max(pos2(area.min.x, body.max.y), area.max);
    let p = ui.painter();
    if t.classic() {
        widgets::sunken(p, strip, t.metal.mid, &t.metal);
    } else {
        p.rect_filled(strip, 0.0, t.toolbar_bg);
    }
    if let Some(r) = pm.sel.as_ref().and_then(|k| rows.iter().find(|r| r.key == *k)) {
        let status = match &r.status {
            Status::Failed(w) => format!("Failed: {w}"),
            Status::Blocked(w) => format!("Blocked: {w}"),
            _ => status_name(r).to_string(),
        };
        let caps = if r.caps.is_empty() { String::new() } else { format!(" ({})", r.caps) };
        let text = format!("{}, {}, {status}{caps}, {}", r.name, r.kind, r.file);
        p.with_clip_rect(strip.intersect(ui.clip_rect())).text(pos2(strip.min.x + 6.0, strip.center().y), Align2::LEFT_CENTER, text, t.font(t.font.ui), t.text);
    }
    out
}

/// The Folders popup: the folders searched (the user's can be removed), the Add Folder buttons, and (A1) the
/// experimental AE-format switch with Adobe's position and the trademark notice next to it (plan §2.6 (8)).
fn folders_menu(ui: &mut Ui, pm: &mut PluginManager, prefs: &mut OfxPrefs, out: &mut ManagerOut) {
    for f in pm.folders(prefs).into_iter().chain(pm.aex_folders(prefs)) {
        let mine = prefs.folders.contains(&f) || prefs.vst_folders.contains(&f) || prefs.aex.folders.contains(&f);
        ui.horizontal(|ui| {
            ui.label(f.display().to_string());
            if mine && widgets::small_push(ui, "×").on_hover_text("Stop searching this folder").clicked() {
                prefs.folders.retain(|x| *x != f);
                prefs.vst_folders.retain(|x| *x != f);
                prefs.aex.folders.retain(|x| *x != f);
                out.prefs_changed = true;
            }
        });
    }
    ui.separator();
    if widgets::push_button_enabled(ui, pm.only.is_none(), "Add OFX Folder...").clicked() {
        if let Some(d) = rfd::FileDialog::new().set_title("Add an OFX plug-in folder").pick_folder() {
            if !prefs.folders.contains(&d) {
                prefs.folders.push(d);
                out.prefs_changed = true;
            }
        }
        pm.folders_open = false;
    }
    if widgets::push_button_enabled(ui, pm.only.is_none(), "Add VST3 / CLAP Folder...").on_hover_text("Also in Preferences › VST Effects").clicked() {
        if let Some(d) = rfd::FileDialog::new().set_title("Add a VST3 or CLAP plug-in folder").pick_folder() {
            if !prefs.vst_folders.contains(&d) {
                prefs.vst_folders.push(d);
                out.prefs_changed = true;
            }
        }
        pm.folders_open = false;
    }
    ui.separator();
    let t = theme::get();
    let mut on = prefs.aex.enabled;
    let hint = "Loads effect plug-ins written for the Adobe After Effects effect API (.aex files) that you have installed and licensed yourself. Each runs in a separate process; Milan Pro bypasses no host or licence check.";
    if ui.checkbox(&mut on, aex::SWITCH).on_hover_text(hint).changed() {
        prefs.aex.enabled = on;
        out.prefs_changed = true;
    }
    ui.label(RichText::new(format!("{} {}", aex::NO_SUPPORT, aex::NOTICE)).font(t.font(t.font.ui)).color(t.text_dim));
    if prefs.aex.enabled && widgets::push_button_enabled(ui, pm.only_aex.is_none(), "Add AE-format Folder...").clicked() {
        if let Some(d) = rfd::FileDialog::new().set_title("Add a folder of AE-format effect plug-ins").pick_folder() {
            if !prefs.aex.folders.contains(&d) {
                prefs.aex.folders.push(d);
                out.prefs_changed = true;
            }
        }
        pm.folders_open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window lists the built-ins (and whatever OFX plug-ins are registered), draws, and changes nothing by itself.
    #[test]
    fn lists_and_draws() {
        let mut pm = PluginManager::new(None, Some(Vec::new()));
        assert!(pm.rows().len() >= plugins::PLUGINS.len());
        pm.query = "invert".into();
        assert!(pm.rows().iter().any(|r| r.id == "milan.fx.invert") && pm.rows().iter().all(|r| r.name.to_lowercase().contains("invert") || r.id.contains("invert") || r.file.to_lowercase().contains("invert")));
        let mut prefs = OfxPrefs::default();
        let ctx = Context::default();
        crate::ui::fonts::install(&ctx);
        let _ = ctx.run(RawInput::default(), |ctx| {
            CentralPanel::default().show(ctx, |ui| {
                let a = plugin_manager_toolbar(ui, &mut pm, &mut prefs);
                let b = plugin_manager(ui, &mut pm, &mut prefs);
                assert!(!a.rescan && !a.prefs_changed && !b.prefs_changed);
            });
        });
        assert_eq!(prefs, OfxPrefs::default());
    }
}