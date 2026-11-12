mod commands;
mod drop;

use eframe::egui;
use crate::audio::{Player, Waveforms};
use crate::autosave::Autosave;
use crate::decode::{Preview, Thumbs};
use crate::demo::{Launch, Shot};

use crate::history::History;
use crate::media::Importer;
use crate::project::Project;
use crate::render;
use crate::settings::{layout_key, Prefs, SettingsDialog};
use crate::ui::chrome;
use crate::ui::dock::{Dock, Panel};
use crate::ui::theme::{self, LookChoice, Theme, UiLook};
use crate::ui::dialogs::{PrefsDialog, RenderDialog, Welcome};
use crate::ui::keymap::{Command, Keymap};
use crate::ui::{self, ChooserKind, ClipRef, DragState, MenuAction, Playback, Selection, TransportAction, TransportState, WinAction};

pub struct MeridianApp {
    pub project: Project,
    pub history: History,
    /// Selected events and tracks (ids).
    pub sel: Selection,
    /// The focus event (`sel.clips.last()`), looked up again by `sync_sel` after anything that edits or selects.
    pub selected_clip: Option<ClipRef>,
    pub selected_media: Option<usize>,
    pub playing: bool,
    /// Loop Playback (Q): playback repeats the time selection.
    pub looping: bool,
    pub snapping: bool,
    /// Edit › Editing Tool.
    tool: ui::EditTool,
    /// The last edit that changed lengths or moved events, for Post-Edit Ripple (F, Ctrl+F, Ctrl+Shift+F): the
    /// history generation right after it, and its ripple (empty when Auto Ripple already rippled it). Any later
    /// change to the history (any edit, wherever it is made, or Undo, Redo, New, Open) makes it stale.
    last_edit: (u64, Vec<crate::edit::RippleOp>),
    pub drag_state: Option<DragState>,
    /// An envelope point drag (its own undo step on release).
    env_drag: Option<ui::EnvDrag>,
    pub timeline_scroll_y: f32,
    pub preview: Preview,
    pub audio: Player,
    pub waves: Waveforms,
    pub thumbs: Thumbs,
    /// Create Video Proxy jobs; they belong to files, so they carry on through New and Open.
    proxies: crate::proxy::ProxyJobs,
    render: RenderDialog,
    settings: SettingsDialog,
    autosave: Autosave,
    last_frame_time: f64,
    was_playing: bool,
    /// Cursor as set by the playback clock this frame; any other change while playing is a seek.
    clock_pos: f64,
    importer: Importer,
    /// Files dragged to the timeline: probed while they hover, placed when dropped (cancelled by New/Open).
    file_drop: drop::FileDrop,
    /// MILAN_DROP (demo): window point where the demo media pretends to be dragged from Windows, for the screenshot.
    fake_drop: Option<egui::Pos2>,
    /// Last import/open error, shown in the status bar.
    message: Option<String>,
    /// Cursor where playback started; Space and Stop return there, as in VEGAS.
    play_start: f64,
    /// Loop region playback was started with; a change while playing restarts it.
    play_loop: Option<(f64, f64)>,
    /// New/Open/Exit waiting on the "Save changes?" prompt.
    confirm: Option<MenuAction>,
    about: bool,
    look: LookChoice,
    dock: Dock,
    /// The user layout slot last loaded or saved in this look (File-style "Save Layout" saves over it).
    layout_slot: Option<usize>,
    /// Save Layout As: the name and slot being chosen.
    layout_as: Option<(String, usize)>,
    /// `prefs.layouts` keys changed here since the last save; the others are taken from disk (another window's).
    layouts_changed: std::collections::BTreeSet<String>,
    /// Native window decorations are on (VP16) or off (VP23 custom title bar).
    decorated: bool,
    /// MILAN_DEMO: nothing is written to the user's autosave or prefs.
    demo: bool,
    shot: Option<Shot>,
    // Dock window state.
    media_view: ui::MediaView,
    explorer: ui::Explorer,
    choosers: [ui::Chooser; 3],
    trimmer: ui::Trimmer,
    preview_view: ui::PreviewView,
    master_view: ui::MasterView,
    /// A Master Bus fader gesture has its undo step already (see `set_master_gain`).
    master_gesture: bool,
    // Commands, preferences and dialogs (app/commands.rs).
    prefs: Prefs,
    keymap: Keymap,
    prefs_dlg: PrefsDialog,
    /// Event Properties, open on one event.
    event_props: Option<crate::ui::dialogs::EventProps>,
    /// Take › Choose Active... / Delete..., open on one event.
    take_chooser: Option<crate::ui::dialogs::TakeChooser>,
    /// The modeless Event Pan/Crop and Track Motion windows.
    pan_crop: Option<ui::PanCropWin>,
    track_motion: Option<ui::TrackMotionWin>,
    welcome: Option<Welcome>,
    shortcuts_open: bool,
    /// J/L shuttle rate (0 = off; 1 = normal playback).
    shuttle: f64,
    /// Recent project picked from a menu, opened once the save prompt is answered.
    pending_open: Option<std::path::PathBuf>,
    /// File › New opens Project Properties for the new project.
    props_after_new: bool,
    /// Settings for the next new project (Welcome › Create Project).
    new_settings: Option<crate::project::ProjectSettings>,
    /// Dock area last frame: plain keys over it go to the dock window.
    dock_rect: egui::Rect,
    /// Copied or cut events (Edit › Cut/Copy/Paste), from `edit::copy_clips`.
    clipboard: Vec<(bool, usize, crate::project::Clip)>,
    /// Track heights saved by View › Minimize All Tracks, restored by the next toggle.
    minimized: Option<Vec<(String, f32)>>,
    updater: crate::updater::Updater,
    /// Verified installer and its SHA-256, to start when the app exits ("Install and restart", after the save prompt).
    install_on_exit: Option<(std::path::PathBuf, String)>,
    /// `prefs.auto_update` and `skipped_update` as last read or written, to tell this window's changes from another's.
    update_prefs_seen: (bool, Option<String>),
}

impl MeridianApp {
    pub fn new(cc: &eframe::CreationContext<'_>, launch: Launch) -> Self {
        ui::fonts::install(&cc.egui_ctx);
        ui::keymap::install(&cc.egui_ctx);
        // The demo harness never reads or writes the user's preferences.
        let prefs = if launch.demo { Prefs::default() } else { Prefs::load() };
        let look = launch.look.unwrap_or(prefs.look);
        theme::set_palette(&cc.egui_ctx, prefs.palette());
        theme::set(&cc.egui_ctx, look);
        // As the theme has it (a VP16 choice has no White), so `update` sees no look change.
        let look = theme::get().choice;
        let welcome = (look.look == UiLook::Vegas23 && prefs.show_welcome && launch.screenshot.is_none()).then(Welcome::default);
        let mut updater = crate::updater::Updater::from_env();
        if updater.auto || (prefs.auto_update && !launch.demo && launch.screenshot.is_none()) {
            // Off the startup path: the check waits a few seconds on its own thread.
            updater.check(&cc.egui_ctx, false, std::time::Duration::from_secs(if updater.auto { 0 } else { 5 }));
        }
        let (mut project, mut sel, mut message) = (Project::new("Untitled"), Selection::default(), None);
        if launch.demo {
            match crate::demo::project(&theme::get()) {
                Ok((p, cr)) => {
                    sel.clips = p.timeline.tracks(cr.is_video).get(cr.track_idx).and_then(|t| t.clips.get(cr.clip_idx)).map(|c| c.id.clone()).into_iter().collect();
                    project = p;
                }
                Err(e) => message = Some(format!("Demo media failed: {e}")),
            }
        }
        let mut dock = last_dock(&prefs, &theme::get());
        let mut trimmer = ui::Trimmer::new(&cc.egui_ctx);
        let mut explorer = ui::Explorer::default();
        if launch.demo {
            explorer = ui::Explorer::at(crate::demo::media_dir());
            if let Some(m) = project.media_pool.first() {
                trimmer.open(m.path.clone());
                trimmer.set_marks(1.0, 3.8);
            }
        }
        for name in &launch.panels {
            if let Some(&p) = Panel::ALL.iter().find(|p| p.title(look.look).replace(' ', "").eq_ignore_ascii_case(name)) {
                dock.open(p);
            }
        }
        let mut app = Self {
            project,
            history: History::default(),
            sel,
            selected_clip: None,
            selected_media: None,
            playing: false,
            looping: false,
            snapping: true,
            tool: ui::EditTool::Normal,
            last_edit: Default::default(),
            drag_state: None,
            env_drag: None,
            timeline_scroll_y: 0.0,
            preview: Preview::timeline(&cc.egui_ctx),
            audio: Player::new(),
            waves: Waveforms::new(&cc.egui_ctx),
            thumbs: Thumbs::new(&cc.egui_ctx),
            proxies: Default::default(),
            render: RenderDialog::default(),
            settings: SettingsDialog::default(),
            autosave: Autosave::default(),
            last_frame_time: 0.0,
            was_playing: false,
            clock_pos: 0.0,
            importer: Importer::new(),
            file_drop: drop::FileDrop::new(),
            fake_drop: launch.drop_at.filter(|_| launch.demo),
            message,
            play_start: 0.0,
            play_loop: None,
            confirm: None,
            about: false,
            look,
            dock,
            layout_slot: None,
            layout_as: None,
            layouts_changed: Default::default(),
            decorated: look.look == UiLook::Vegas16,
            demo: launch.demo,
            shot: launch.screenshot.map(Shot::new),
            media_view: ui::MediaView::default(),
            explorer,
            choosers: [ChooserKind::Transitions, ChooserKind::VideoFx, ChooserKind::Generators].map(ui::Chooser::new),
            trimmer,
            preview_view: ui::PreviewView::default(),
            master_view: ui::MasterView::default(),
            master_gesture: false,
            keymap: Keymap::default(),
            prefs_dlg: PrefsDialog::default(),
            event_props: None,
            take_chooser: None,
            pan_crop: None,
            track_motion: None,
            welcome,
            shortcuts_open: false,
            shuttle: 0.0,
            pending_open: None,
            props_after_new: false,
            new_settings: None,
            dock_rect: egui::Rect::NOTHING,
            clipboard: Vec::new(),
            minimized: None,
            updater,
            install_on_exit: None,
            update_prefs_seen: (prefs.auto_update, prefs.skipped_update.clone()),
            prefs,
        };
        app.sync_sel();
        app.open_from_env(&cc.egui_ctx);
        app
    }

    /// Drop selected ids that no longer exist (after an undo, a delete) and look the focus event up again.
    fn sync_sel(&mut self) {
        let p = &self.project;
        if !self.sel.clips.is_empty() {
            // One sweep, not a find_clip per id: this runs every frame, and Select All on a long project is big.
            let tl = &p.timeline;
            let all: std::collections::HashSet<&str> = tl.video_tracks.iter().chain(&tl.audio_tracks).flat_map(|t| &t.clips).map(|c| c.id.as_str()).collect();
            self.sel.clips.retain(|id| all.contains(id.as_str()));
        }
        self.sel.tracks.retain(|id| p.find_track(id).is_some());
        // Envelope points: the envelope must still be there (an undo, a removed envelope) and so must the points.
        let env_len = |s: &ui::EnvSel| p.find_track(&s.track).and_then(|(v, i)| p.timeline.tracks(v)[i].envelopes.iter().find(|e| e.kind == s.kind)).map(|e| e.points.keys.len());
        if let Some(n) = self.sel.env.as_ref().map(env_len) {
            let keep = self.sel.env.take().filter(|_| n.is_some()).map(|mut s| {
                s.keys.retain(|k| *k < n.unwrap_or(0));
                s
            });
            self.sel.env = keep.filter(|s| !s.keys.is_empty());
        }
        self.selected_clip = self.sel.clips.last().and_then(|id| p.find_clip(id));
    }

    /// An event or envelope drag holds its own undo step.
    fn dragging(&self) -> bool {
        self.drag_state.is_some() || self.env_drag.is_some()
    }

    /// Files headed for the timeline this frame: dragged from Windows (or just dropped), else the Explorer window's
    /// drag. Their probes start here, so the timeline can show them as soon as they are known.
    fn file_drag(&mut self, ctx: &egui::Context) -> ui::FileDrag {
        let modal = self.modal_open();
        let (hovered, dropped): (Vec<std::path::PathBuf>, Vec<std::path::PathBuf>) = ctx.input(|i| {
            let keep = |p: &Option<std::path::PathBuf>| p.clone().filter(|p| ui::is_media(p));
            (i.raw.hovered_files.iter().filter_map(|f| keep(&f.path)).collect(), i.raw.dropped_files.iter().filter_map(|f| keep(&f.path)).collect())
        });
        let os = !hovered.is_empty() || !dropped.is_empty();
        if !hovered.is_empty() {
            ctx.request_repaint(); // egui hears nothing more from Windows until the drop
        }
        let mut os_pos = os.then(|| drop::os_pointer(ctx)).flatten();
        let os_dropped = !dropped.is_empty() && !modal;
        let mut files = if dropped.is_empty() { hovered } else { dropped };
        if let Some(p) = self.fake_drop {
            let pool = &self.project.media_pool;
            files = [pool.first(), pool.last()].into_iter().flatten().map(|m| m.path.clone()).collect();
            os_pos = Some(p);
        }
        if files.is_empty() {
            if let Some(ui::MediaDrag::File(p)) = egui::DragAndDrop::payload::<ui::MediaDrag>(ctx).as_deref() {
                files = vec![p.clone()];
            }
        }
        self.file_drop.hover(ctx, &files, &self.project.media_pool);
        ui::FileDrag {
            items: self.file_drop.items(&files, &self.project.media_pool),
            os_dropped,
            files,
            // Not over a window or popup above the timeline (egui hit-tests only its own pointer).
            os_pos: os_pos.filter(|p| !modal && ctx.layer_id_at(*p).is_none_or(|l| l.order == egui::Order::Background)),
            still_len: self.prefs.still_len,
            dropped_at: None,
        }
    }

    /// Import `paths` that are not in the project yet (background probe; the app adds them when done).
    fn import_new(&mut self, ctx: &egui::Context, paths: Vec<std::path::PathBuf>) {
        let new: Vec<_> = paths.into_iter().filter(|p| !self.project.media_pool.iter().any(|m| m.path == *p)).collect();
        if !new.is_empty() {
            self.importer.import(ctx, new);
        }
    }
}

impl eframe::App for MeridianApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Playback clock: audio is master while it plays (it wraps loops itself), frame dt otherwise
        if self.playing {
            let now = ctx.input(|i| i.time);
            let end = render::content_end(&self.project);
            let tl = &mut self.project.timeline;
            if let Some(t) = self.audio.position() {
                tl.cursor_position = t;
            } else if self.last_frame_time > 0.0 {
                tl.cursor_position += now - self.last_frame_time;
            }
            let lp = tl.loop_region.filter(|(a, b)| self.looping && b > a);
            if let Some((a, _)) = lp.filter(|&(_, b)| tl.cursor_position >= b) {
                // Loop Playback: jump back to the selection start.
                tl.cursor_position = a;
            } else if lp.is_none() && tl.cursor_position >= end {
                // Playback ends at the last event and stays there.
                tl.cursor_position = end.max(self.play_start);
                self.playing = false;
            }
            self.last_frame_time = now;
            ctx.request_repaint();
        } else {
            self.last_frame_time = 0.0;
        }
        self.clock_pos = self.project.timeline.cursor_position;

        // Keyboard shortcuts, through the keymap (§7); menus and context menus queue theirs for after the UI.
        self.tick_shuttle(ctx);
        self.handle_keys(ctx);
        let mut menu_action = None;

        // Title: "Untitled - Milan Pro" (OS title bar in VP16, drawn in the custom bar in VP23).
        let title = format!("{}{} - Milan Pro", self.project.name, if self.project.modified { " *" } else { "" });
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
        let t = theme::get();
        let v16 = t.look() == UiLook::Vegas16;
        if self.decorated != v16 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(v16));
            self.decorated = v16;
        }

        let mut drag = self.file_drag(ctx);
        let mut transport_action = None;
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.frame)).show(ctx, |ui| {
            let full = ui.max_rect();
            let dock_h = self.dock.height.unwrap_or(t.m.dock_h.min(full.height() * t.m.dock_frac));
            let a = chrome::areas(&t, full, dock_h, self.dock.track_list_w);
            chrome::paint_bands(ui.painter(), &t, &a, full);

            // Menu: own white row (VP16) or inline in the custom title bar (VP23); popups get the menu style.
            self.dock_rect = a.dock;
            ui::menus::publish_state(ctx, self.cmd_state());
            chrome::with_menu_style(ctx, &t, || {
                if v16 { chrome::menu_row(ui, a.menu, ui::draw_menu_bar) } else { chrome::title_bar(ui, a.menu, &title, ui::draw_menu_bar) }
            });

            let mut row = ui.new_child(egui::UiBuilder::new().max_rect(a.toolbar).layout(egui::Layout::left_to_right(egui::Align::Center)));
            let a_tb = ui::draw_main_toolbar(&mut row, &self.project, &self.history);
            menu_action = menu_action.or(a_tb);

            // Dock windows.
            let mut dock_action = None;
            let mut actions: Vec<WinAction> = Vec::new();
            let mut preview_msg = None;
            // The engine makes frames only while the Video Preview window is on screen.
            self.preview.visible = false;
            self.dock.show(ui, a.dock, |panel, bar, body| match panel {
                Panel::ProjectMedia => {
                    ui::draw_media_toolbar(
                        bar,
                        &mut self.project,
                        &mut self.history,
                        &mut self.media_view,
                        &mut self.selected_media,
                        &mut self.sel.clips,
                        &self.importer,
                        &mut actions,
                    );
                    ui::draw_media_browser(
                        body,
                        &mut self.project,
                        &mut self.media_view,
                        &mut self.selected_media,
                        &mut self.sel.clips,
                        &mut self.history,
                        &mut self.thumbs,
                        &mut actions,
                        self.prefs.still_len,
                        self.prefs.auto_crossfade,
                    );
                }
                Panel::Explorer => {
                    ui::draw_explorer_toolbar(bar, &mut self.explorer);
                    ui::draw_explorer(body, &mut self.explorer, &mut actions);
                }
                Panel::Transitions | Panel::VideoFx | Panel::MediaGenerators => {
                    let k = match panel {
                        Panel::Transitions => 0,
                        Panel::VideoFx => 1,
                        _ => 2,
                    };
                    ui::draw_chooser_toolbar(bar, &mut self.choosers[k]);
                    ui::draw_chooser(body, &mut self.choosers[k]);
                }
                Panel::VideoPreview => {
                    self.preview.visible = true;
                    preview_msg = ui::draw_preview_toolbar(bar, &mut self.preview, &mut self.preview_view, &mut dock_action);
                    let mut pb = Playback { playing: &mut self.playing, looping: &mut self.looping, play_start: self.play_start };
                    ui::draw_preview(body, &mut self.project, &mut self.preview, &mut self.preview_view, &mut pb);
                }
                Panel::Trimmer => {
                    ui::draw_trimmer_toolbar(bar, &mut self.trimmer);
                    ui::draw_trimmer(body, &mut self.trimmer, &mut self.project, &mut self.history, &mut self.waves, &mut self.thumbs, self.prefs.still_len, self.prefs.auto_crossfade);
                }
                Panel::Properties => {
                    egui::Frame::NONE.inner_margin(6).show(body, |ui| {
                        ui::draw_properties(ui, &self.project, &self.selected_clip, &self.selected_media);
                    });
                }
                Panel::MasterBus => {
                    ui::master_bus_toolbar(bar);
                    // The faders edit the project's master gain (saved with it, used by the render).
                    self.master_view.gain = self.project.timeline.master.gain;
                    ui::master_bus(body, &mut self.master_view, &self.audio.master);
                    let down = body.input(|i| i.pointer.any_down());
                    set_master_gain(&mut self.project, &mut self.history, &mut self.master_gesture, self.master_view.gain, down);
                }
                p => ui::placeholder(body, p),
            });
            if preview_msg.is_some() {
                self.message = preview_msg;
            }
            for act in actions {
                match act {
                    WinAction::OpenTrimmer(path) => {
                        self.trimmer.open(path);
                        self.dock.open(Panel::Trimmer);
                    }
                    WinAction::Show(p) => self.dock.open(p),
                    WinAction::Import(paths) => self.import_new(ctx, paths),
                    WinAction::CreateProxy(items) => self.proxies.add(&items),
                }
            }
            menu_action = menu_action.or(dock_action);

            // Dock / timeline splitter.
            let split = ui.interact(a.split, ui.id().with("main_split"), egui::Sense::drag());
            if split.hovered() || split.dragged() {
                ctx.set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }
            if split.dragged() {
                self.dock.height = Some(a.dock.height() + split.drag_delta().y);
            }

            let mut tl = ui.new_child(egui::UiBuilder::new().max_rect(a.timeline));
            tl.set_clip_rect(a.timeline);
            let opts = self.edit_opts();
            let dragged = ui::draw_timeline(
                &mut tl,
                &mut self.project,
                &mut self.sel,
                &mut self.drag_state,
                &mut self.env_drag,
                &mut self.timeline_scroll_y,
                &mut self.history,
                opts,
                &mut self.waves,
                &mut self.thumbs,
                self.playing,
                &mut self.dock.track_list_w,
                &mut drag,
            );
            if let Some(op) = dragged {
                // Auto Ripple already rippled it; otherwise Post-Edit Ripple can.
                self.last_edit = (self.history.generation(), if opts.ripple.is_some() { Vec::new() } else { op });
            }
            // The Rate area spans the horizontal scrollbar row and the transport row (§5.9.3).
            let rate = a.rate.with_min_y(a.timeline.max.y - t.m.hscroll_h);
            ui::draw_rate(ui, rate, self.playing, &mut self.project);
            transport_action = ui::draw_transport(
                ui,
                a.transport,
                TransportState {
                    project: &mut self.project,
                    history: &mut self.history,
                    playback: Playback { playing: &mut self.playing, looping: &mut self.looping, play_start: self.play_start },
                    snapping: &mut self.snapping,
                },
            );
            // Last, so hovered-command hints from every bar this frame are shown.
            let msg = self.message.clone().or_else(|| self.audio.status()).or_else(|| self.preview.error.clone()).or_else(|| self.autosave.error());
            if ui::draw_status_bar(ui, a.status, &self.project, msg.as_deref(), self.proxies.status().as_deref()) {
                self.proxies.cancel();
            }
            if !v16 {
                chrome::resize_edges(ui, full);
            }
        });
        self.sync_sel();
        if let Some(TransportAction::ToggleColorGrading) = transport_action {
            self.dock.toggle(Panel::ColorGrading);
        }

        // A new look (Preferences) applies live; each look keeps its own last layout.
        if self.look != t.choice {
            theme::set(ctx, self.look);
            self.look = theme::get().choice;
            self.dock = look_dock(&mut self.prefs, &self.dock, t.look(), &theme::get());
            self.layouts_changed.insert(layout_key(t.look(), "last"));
            self.layout_slot = None;
            self.prefs.look = self.look;
            self.save_prefs();
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.project.modified {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(MenuAction::Exit);
        }
        if let Some(a) = menu_action {
            self.exec(ctx, Command::from(a));
        }
        self.run_bus(ctx);
        match ui::take_tool(ctx) {
            Some(ui::ToolWin::PanCrop(id)) => self.pan_crop = ui::PanCropWin::open(&self.project, &id),
            Some(ui::ToolWin::TrackMotion(id)) => self.track_motion = ui::TrackMotionWin::open(&self.project, &id),
            None => {}
        }
        self.show_dialogs(ctx);
        // Files dropped on the timeline are placed once probed; dropped from Windows anywhere else, only imported
        // (§5.4). Both reuse the probes started while the files hovered.
        if let Some((paths, track, time, mode)) = drag.dropped_at.take() {
            self.file_drop.drop(paths, Some((track, time, mode)));
        } else if drag.os_dropped {
            self.file_drop.drop(drag.files, None);
        }
        // Media added below (drops and imports append to the pool) may get a video proxy automatically.
        let pool_len = self.project.media_pool.len();
        // Not during an event drag: it restores the tracks from its start every frame and would wipe the drop out.
        while !self.dragging() {
            let Some(placed) = self.file_drop.poll(&mut self.project, self.prefs.still_len, self.prefs.auto_crossfade) else { break };
            if let Some(before) = placed.before {
                self.history.push(before);
            }
            if !placed.ids.is_empty() {
                (self.sel.clips, self.sel.list_focus) = (placed.ids, false);
            }
            self.message = if !placed.errors.is_empty() {
                Some(format!("Import failed: {}", placed.errors.join("; ")))
            } else {
                placed.refused.then(|| "Drop refused: the track is locked".to_string())
            };
        }
        while let Some(batch) = self.importer.poll() {
            let before = self.project.clone();
            let (ok, errors): (Vec<_>, Vec<_>) = batch.into_iter().map(|(_, r)| r).partition(Result::is_ok);
            let errors: Vec<String> = errors.into_iter().filter_map(Result::err).collect();
            // Files imported twice (two drops or picks before the first probe finished) are added once.
            if crate::media::merge_new(&mut self.project.media_pool, ok.into_iter().filter_map(Result::ok).collect()) {
                self.history.push(before);
                self.project.mark_modified();
            }
            self.message = (!errors.is_empty()).then(|| format!("Import failed: {}", errors.join("; ")));
        }
        if self.prefs.auto_proxy {
            let added = self.project.media_pool.get(pool_len..).unwrap_or_default();
            self.proxies.add(added.iter().filter(|m| crate::proxy::wants_auto(m)));
        }
        self.proxies.scan(ctx, &self.project.media_pool);
        if let Some(e) = self.proxies.poll() {
            self.message = Some(e);
        }
        if self.proxies.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        self.render.show(ctx, &self.project);
        self.settings.show(ctx, &mut self.project, &mut self.history);
        if !self.demo {
            self.autosave.tick(ctx, &self.project);
        }

        // A clip selection replaces a media selection in Properties.
        if self.selected_clip.is_some() {
            self.selected_media = None;
        }

        // Start/stop video and audio on play state changes, seeks and loop changes. Edits while playing
        // are heard through a live plan swap, not a restart.
        if self.playing && !self.was_playing && self.looping {
            // Loop Playback starts inside the selection.
            let tl = &mut self.project.timeline;
            if let Some((a, _)) = tl.loop_region.filter(|&(a, b)| b > a && (tl.cursor_position < a || tl.cursor_position >= b)) {
                tl.cursor_position = a;
            }
        }
        let cursor = self.project.timeline.cursor_position;
        if self.playing && !self.was_playing {
            self.play_start = cursor;
        }
        // Video and audio play on through loop wraps (both wrap themselves); a seek or a new loop region restarts them.
        let tl = &self.project.timeline;
        let lp = tl.loop_region.filter(|&(a, b)| self.looping && b > a);
        if self.playing != self.was_playing || (self.playing && (cursor != self.clock_pos || lp != self.play_loop)) {
            self.audio.stop();
            if self.playing {
                self.preview.play(lp);
                self.audio.start(ctx, &self.project, cursor, lp);
            } else {
                self.preview.stop();
            }
            self.play_loop = lp;
            self.was_playing = self.playing;
        } else if self.playing {
            if self.audio.stalled() {
                // The device stopped asking for samples (unplugged): the frame clock moves the playhead until it is back.
                self.audio.reopen();
            } else if !self.audio.running() {
                // The device has (re)opened: carry on from the cursor.
                self.audio.start(ctx, &self.project, cursor, lp);
            } else {
                self.audio.set_plan(&self.project);
            }
        }
        // Playback follows the project's master gain also while the Master Bus window is closed (an opened project, Undo).
        self.audio.master.set_gain(self.project.timeline.master.gain);
        // Thumbnail and waveform decodes wait while playing.
        self.thumbs.set_paused(self.playing);
        self.waves.set_paused(self.playing);

        // After the timeline and the play edges, so this frame's cursor/clip edits are what gets requested.
        self.preview.update(ctx, &self.project);
        // MILAN_UPDATE_AUTO: the app being updated takes no screenshot; the relaunched one shots its final message.
        let updating = self.updater.auto && !matches!(self.updater.state, crate::updater::State::Message(_));
        if let Some(shot) = self.shot.as_mut().filter(|_| !updating) {
            let busy = self.preview.busy() || self.trimmer.busy() || self.waves.pending() || self.thumbs.pending();
            shot.tick(ctx, busy);
        }
    }

    /// The window layout comes back at the next launch. Only the layout is written, over what is on disk, so
    /// another window's preferences (recent projects) are kept.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.demo || self.shot.is_some() {
            return;
        }
        let Some(mut p) = Prefs::try_load() else {
            eprintln!("Could not read the preferences; the window layout is not saved.");
            return;
        };
        p.layouts.insert(layout_key(theme::get().look(), "last"), self.dock.layout(""));
        if let Err(e) = p.save() {
            eprintln!("Could not save the window layout: {e}");
        }
    }
}

/// The dock for `t`'s look: the default, then that look's last layout if there is one.
fn last_dock(prefs: &Prefs, t: &Theme) -> Dock {
    let mut d = Dock::new(t);
    if let Some(l) = prefs.layouts.get(&layout_key(t.look(), "last")) {
        d.apply(l, t);
    }
    d
}

/// Switching looks: keep `dock` as the old look's last layout and bring back the new look's.
fn look_dock(prefs: &mut Prefs, dock: &Dock, from: UiLook, to: &Theme) -> Dock {
    prefs.layouts.insert(layout_key(from, "last"), dock.layout(""));
    last_dock(prefs, to)
}

/// Apply the Master Bus faders' `gain` to the project: one undo step per gesture (a drag, a click, a wheel step or a
/// double-click reset), like the track faders, so Undo never reverts the master gain as a side effect of another edit.
fn set_master_gain(project: &mut Project, history: &mut History, gesture: &mut bool, gain: [f32; 2], pointer_down: bool) {
    let changed = gain != project.timeline.master.gain;
    if changed {
        if !*gesture {
            history.push(project.clone());
        }
        project.timeline.master.gain = gain;
        project.mark_modified();
    }
    *gesture = pointer_down && (*gesture || changed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_fader_gesture_is_one_undo_step() {
        let (mut p, mut h, mut g) = (Project::new("t"), History::default(), false);
        // A drag over three frames, then release: one step.
        for v in [0.9, 0.7, 0.5] {
            set_master_gain(&mut p, &mut h, &mut g, [v, v], true);
        }
        set_master_gain(&mut p, &mut h, &mut g, [0.5, 0.5], false);
        assert_eq!(h.lens(), (1, 0));
        assert!(p.modified);
        // A wheel step (no button down) is its own step; an unchanged frame adds none.
        set_master_gain(&mut p, &mut h, &mut g, [0.4, 0.4], false);
        set_master_gain(&mut p, &mut h, &mut g, [0.4, 0.4], false);
        assert_eq!(h.lens(), (2, 0));
        h.undo(&mut p);
        assert_eq!(p.timeline.master.gain, [0.5, 0.5]);
        h.undo(&mut p);
        assert_eq!(p.timeline.master.gain, [1.0, 1.0]);
    }

    #[test]
    fn layouts_save_restore_and_stay_per_look() {
        let (t16, t23) = (Theme::new(LookChoice::VEGAS16), Theme::new(LookChoice::VEGAS23));
        let mut prefs = Prefs::default();
        // SaveLayoutUser(3), a move, LayoutUser(3): the groups come back.
        let mut d = last_dock(&prefs, &t16);
        d.toggle(Panel::MasterBus);
        prefs.layouts.insert(layout_key(UiLook::Vegas16, 3), d.layout("Three"));
        let saved = d.groups.clone();
        d.open(Panel::Properties);
        d.toggle(Panel::VideoPreview);
        assert_ne!(d.groups, saved);
        d.apply(&prefs.layouts[&layout_key(UiLook::Vegas16, 3)], &t16);
        assert_eq!(d.groups, saved);
        // To VP23 and back: each look gets its own last layout.
        let mut d23 = look_dock(&mut prefs, &d, UiLook::Vegas16, &t23);
        assert_eq!(d23.groups, Dock::new(&t23).groups);
        d23.open(Panel::Properties);
        let back = look_dock(&mut prefs, &d23, UiLook::Vegas23, &t16);
        assert_eq!(back.groups, saved);
        assert!(last_dock(&prefs, &t23).is_open(Panel::Properties));
    }
}