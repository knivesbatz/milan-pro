# Milan Pro: interface specification

Status: implementation spec (2026-11-26). Milan Pro draws one look of its own: the chrome of the 2005–2007 Windows
NLEs on top of the VEGAS Pro 23 **layout** described in `VEGAS_UI_SPEC.md`: the docking areas on all four sides, the
built-in layouts, the tab strips at the bottom of each group, the menus, toolbar sets, commands and keys. The window
keeps the **native Windows title bar** (every 2006 application did) and the menu is a classic **in-window menu row**
under it.

Evidence tags: **[R]** a convention taken from the named application (seen in its manual, reviews or screenshots of
the time); **[W]** a Windows 2000 / XP system value (system colour table); **[I]** inferred or a design decision
(tunable, not a measurement). Nothing here copies a bitmap; every icon is drawn in code.

---

## 1. References and what each one gave

| Application (year) | Convention taken |
|---|---|
| Sony Vegas 5 / 6 / 7 (2004–2007) | Medium-grey 3D chrome with raised bevelled toolbars, 20–26 px toolbars, 16 px coloured icons, Tahoma 8 pt, blue-grey track headers with raised M / S / fx buttons, grooved sliders with a thumb that has a centre line, events with a lighter **gradient header bar** holding the name and the ⋯ / fx / crop glyphs, dotted-line splitters, a black time display with light digits, yellow loop-region flags, pastel default event colours. [R] |
| Adobe Premiere Pro 2.0 (2006) | Dense panel chrome, 1 px panel frames, a dark timeline well under a lighter chrome (its "brightness" slider's darker end), monochrome timeline rulers with black ticks. [R] |
| Avid Xpress Pro 5 (2005) | Dark (gunmetal) timeline lanes with light grey chrome around them; flat grey scroll tracks. [R] |
| Pinnacle Studio 10, Ulead MediaStudio Pro 8, Canopus Edius 3 (2005–2006) | Brushed-steel / chrome toolbar and transport skins with specular button tops and electric-blue active states; black LCD-style counters. [R] |
| Final Cut Pro 5 (2005) | The brushed-metal band texture (Mac OS X "metal" windows) as the source of the streak look: fine horizontal streaks, light top, darker bottom. [R] |
| Windows 2000 / XP classic controls (2000–2006) | Everything a 2006 Windows app did not draw itself: 2 px 3D button edges (white / face / shadow / dark), sunken fields and status panes, scrollbars with arrow buttons, the tab control, etched separators, toolbar grips, tooltip yellow, `#D4D0C8` dialogs. [W] |
| Windows XP (Luna) menus | White popup menus with a 3D grey border and the `#316AC5` highlight bar with white text. [W] |

Deliberately **not** taken: the Luna title bar and blue-gradient toolbars (only the OS title bar is native), rounded
corners, soft drop shadows, translucency, anti-aliased icon art.

---

## 2. Colour tokens (`Theme::new`, `theme.rs`)

### 2.1 Brushed metal (`Metal`)

Every bar is a vertical gradient `hi` (top) → `mid` (45 %) → `lo` (bottom) with a 64×64 tiled streak texture over it
(rows of ±18/255 white or black, `widgets::metal`). Painted as two triangles' worth of mesh plus one textured quad;
the texture is made once per context (`TextureWrapMode::Repeat`), so a bar costs no per-frame allocation.

| Token | Hex | Use |
|---|---|---|
| `metal.hi` | `#E4E6E9` | Top of every bar [I] |
| `metal.mid` | `#B9BDC3` | Middle of every bar; flat `toolbar_bg` / `status_bg` fallback [I] |
| `metal.lo` | `#8F949B` | Bottom of every bar [I] |
| `metal.light` | `#FFFFFF` | Outer top-left bevel row, light edge lines [W ButtonHighlight] |
| `metal.face` | `#D4D6DA` | Inner top-left bevel row, button face [W ButtonFace, cooled] |
| `metal.shadow` | `#6E737A` | Inner bottom-right bevel row, etched separator dark line [W ButtonShadow, cooled] |
| `metal.dark` | `#2E3136` | Outer bottom-right bevel row [W ButtonDarkShadow, cooled] |
| `metal.header_tint` | `#C4D0DC` | Multiplier that turns the metal blue-grey on track headers (Vegas 6) [R][I] |
| `metal.specular` | white at 55 % | 1 px line under a raised button's top edge [R Pinnacle/Edius chrome] |
| `metal.pressed` | `#00AEEF` at 35 % | Tint over a pressed-in (active) button [I] |

### 2.2 Chrome and panels

| Token | Hex | Note |
|---|---|---|
| `frame` (gutters, splitter base) | `#5C6168` | gunmetal [I] |
| `panel_bg` (dock window bodies) | `#C9CCD1` | light panel grey [I] |
| `band_bg` (ruler row, empty header column) | `#AEB3BA` | [I] |
| `lane_bg` / `empty_lane_bg` | `#3C4047` / `#2F3339` | dark gunmetal timeline well [R Premiere 2.0 dark, Avid] |
| `tab_strip_bg` | `#9EA3AA` | darker metal behind the tabs (painted as metal tinted `#B4B8BE`) |
| `text` / `text_bright` / `text_dim` / `text_disabled` | `#000000` / `#FFFFFF` / `#5C6168` / `#808080` | black ink on grey [W] |
| `icon` / `icon_disabled` | `#0A0A0A` / `#808080` | glyph ink, greyed glyphs [W GrayText] |
| `input_bg` / `input_placeholder` | `#FFFFFF` / `#808080` | sunken white fields [W Window] |
| `tooltip_bg` / `tooltip_text` / `tooltip_border` | `#FFFFE1` / `#000000` / `#000000` | [W InfoBackground] |
| `accent` | `#0096E0` | electric blue: selection, hyperlinks, FX dots [I] |
| `button_active_bg` / `button_hover_bg` | `#9CD3F0` / `#E4E6E9` | egui widget states (dialogs) |

### 2.3 Menus (XP classic popups)

| Token | Hex |
|---|---|
| `menu_bar_bg` | metal (`#B9BDC3` flat) |
| `menu_text` | `#000000` |
| menu bar hover / open | raised / sunken 1 px edge (`metal.light` / `metal.shadow`), no fill |
| `menu_popup_bg` | `#FFFFFF` [W Luna Menu] |
| `menu_popup_border` | `#808080` outer, white inner top-left, `#2E3136` outer bottom-right (`chrome::classic_popup_border`) [W] |
| `menu_popup_hover` | `#316AC5`, white text [W Luna Highlight] |
| `menu_popup_text_disabled` | `#808080` [W GrayText] |
| shortcut text | black (`row_colors`) |

### 2.4 Timeline

| Token | Hex | Note |
|---|---|---|
| `track_header_bg` | `#9AA6B3` | base; painted as metal × `header_tint` [R Vegas 6] |
| `track_header_sel_bg` | `#B4CFE4` | metal × `#BEDCF2` when selected [I] |
| `track_separator` / `track_separator_2` | `#4A4E55` / `#7A7F86` | 2 px rule under each track |
| `strip_divider` | `#2E3136` | right of the colour strip |
| `time_display_bg` / `time_display_text` | `#14181E` / `#5CE0FF` | black LCD well, cyan digits [R Vegas 5–7, Edius][I colour] |
| `ruler_text` / `ruler_major_tick` / `ruler_minor_tick` | `#000000` / `#000000` / `#3C4047` | [R Premiere 2.0] |
| `grid_line` | `#50555C` | dotted 1/2 |
| `scroll_track` / `scroll_thumb` / `scroll_arrow` | `#A9AEB5` / chrome / `#000000` | [W] |
| `loop_flag` / `loop_flag_shade` | `#FFD800` / `#B09400` | yellow loop flags [R Vegas] |
| `loop_sel_line` / `loop_bar_on` | `#0096E0` | selection line in the loop bar |
| `selection_overlay` | `#00AEEF` at 18 % | time selection over ruler and lanes |
| `marker_flag` / `region_flag` | `#E08A2A` / `#3C9A50` | orange marker tags, green region tags with a line [R Vegas 6] |
| `playhead_line` / `playhead_outline` / `playhead_head` | `#00C8FF` / `#00304A` / `#FFFFFF` | cyan cursor, white 9×12 flag [I] |
| `event_selected_border` | `#00C8FF` 2 px | [I] |
| `event_selected_border_ripple` | `#E03030` | Auto Ripple |
| event header | track colour → white 55 % at the top, 10 % at the bottom, a 60 % rule under it; black Tahoma name | [R Vegas 6] |
| audio waveform | track colour × 0.45 | dark on the pastel event [R Vegas 6] |
| `fade_curve` / `fade_handle` | `#0000C8` / `#0050FF` | 1 px blue fades, blue quarter-arc handles [R Vegas] |
| `envelope_volume` / `envelope_pan` / `envelope_mute` | `#0000C8` / `#C00000` / `#00C8FF` | |
| `meter_bg` / green / yellow / red | `#14181E` / `#00E020` / `#F0E000` / `#FF2020` | LED look [I] |
| `track_colors` | `#6E8EC8` `#C86E6E` `#78B478` `#C8B46E` `#9678C8` `#6EB4B4` `#C8966E` `#8CA0B4` | Vegas 6 pastels [I] |

### 2.5 Status bar, dialogs

Status bar: metal band, a light edge above it, **sunken panes** (`widgets::sunken`, fill `metal.mid`) around the
message pane and each read-out on the right. [W]

Dialogs (`Theme::dialog`): body `#D4D0C8`, fields white with `#808080`
borders, etched group rules `#808080` over white, default button accent `#316AC5`. [W]

---

## 3. Fonts

* **Tahoma 8 pt = 11 px** everywhere (`C:\Windows\Fonts\tahoma.ttf`, bold `tahomabd.ttf`), loaded at runtime by
  `fonts::install` as the `tahoma` / `tahoma-bold` families; Segoe UI, then egui's fonts, fall in when a file is
  missing. [R Vegas 6, every Win32 app of the time]
* `FontSizes`: ui / menu / dock tab / label / event name / transport time / status 11; track number 11 bold; ruler 10;
  mute/solo letters 9 (drawn 10 in Tahoma Bold); time display 20 bold; meters 8; egui `Small` 9.
* Text is positioned on whole pixels (egui rounds galley origins); 1 px strokes are drawn on pixel centres.

---

## 4. Metrics (`Metrics` in `Theme::new`)

| | px | Note |
|---|---|---|
| Title bar | 0 (native) | the OS draws it |
| Menu row | 20 | metal, 1 px shadow line under it |
| Main toolbar | 26, + 2 px raised bevel above, 1 px shadow below, 2 px gap | buttons 22×22 on a 23 px pitch, grip at the left end |
| Dock toolbars | 26 | metal bar with a grip; the body sits in a sunken 2 px frame |
| Dock tab strip | 21 | tabs 8 + label + 8 (active + 38 for □ ×), inactive tabs 2 px shorter |
| Splitters | 6 | metal with a dotted centre line (`dotted_splitter`) [R Vegas 6] |
| Outer gutter | 2 | |
| Track list | 250 | colour strip 20 |
| Time display | 40 | LCD well inset 3 / 4 px |
| Band | 42 | loop 2–8 · markers 8–22 · ruler 18 on the bottom line |
| Tracks | 56 video / 56 audio | slider rows on a 16 px pitch from y = 19 |
| Event header | 16 | 16 under 40 px events, none under 24 px |
| Transport bar | 28, 1 px light edge above | grip, buttons on a 23 px pitch, etched separators, sunken white read-outs |
| Status bar | 20, 2 px gap and 1 px light edge above | sunken panes |
| Scrollbars | 16 | arrow buttons 16, thumb full height |
| Icons | 16, 1 px stroke | |
| Popup rows | 19 | gutter 24 |
| Corner radius | 0 | nothing is rounded |

Band positions at 1920×1080 (client area): menu 0–20, toolbar 23–49, dock 52–586, splitter 586–592, timeline from 592,
transport 1027–1055, status 1059–1079 (`chrome::tests::default_bands_match_spec`).

---

## 5. Controls (`widgets.rs`)

* **`chrome_button`**: metal face + 2 px bevel + specular line. States: Raised (every toolbar button, always visible —
  the Vegas 5–7 / Edius convention, not the flat-until-hover Office one), Hover (brighter), Pressed (sunken, darker),
  Active (sunken + `metal.pressed` tint: toggles that are on, the current editing tool).
* **`bevel` / `edge`**: the Windows 2 px and 1 px 3D edges.
* **`sunken`**: fields, status panes, the time-display well, the length read-outs, dock bodies.
* **`etched_v` / `etched_h`**: separators (shadow + light line).
* **`grip`**: two raised 2 px bars at the left end of a toolbar. [R Office 2000 / Vegas 6]
* **`dotted_splitter`**: metal with 1 px dark/light dot pairs every 3 px.
* **`groove`** + **`thumb`**: sunken 3 px slider groove, 11×15 chrome thumb with a dark centre line (the Vegas fader
  handle; automation controls tint it with their envelope colour).
* **Scrollbars** (`timeline/scroll.rs::paint_bars`): flat `scroll_track`, chrome ◂ ▸ ▴ ▾ buttons with black
  triangles, chrome thumb with two grip lines, chrome + / − / magnifier buttons.
* **Tabs** (`dock.rs::classic_tab`): metal face, light left/top edge, dark right and bottom edges, hanging under the
  content (tabs at the bottom) or standing on it (Preferences › tabs on top); the active tab joins the content, is
  2 px taller and holds □ ×.
* **Menu bar**: labels on the metal row; a raised 1 px edge on hover, a sunken one while the menu is open.
* **Track header buttons**: 18×18 chrome buttons (M, S, fx, compositing, record arm), Active while on.

---

## 6. Layout and workflow

Layout, docking, commands, shortcuts (the VEGAS 23 key map), the toolbar button sets (`Bar::defaults`), the menus, the
Start dialog and the VEGAS 23 windows (Color Grading, Project Notes) follow `VEGAS_UI_SPEC.md`; only the drawing is
Milan Pro's own. Saved layouts and toolbars are keyed by slot (`last`, `0`-`9`, `toolbar`, `timeline_toolbar`);
preferences saved by 0.8.0 and earlier carry their layouts, toolbars and keys over (`settings::migrate_look_keys`).

---

## 7. Icons (`icons.rs`)

Coloured shapes, rendered as 2006 pixel art:

* 1 px strokes on pixel centres (`at_s`), fills on pixel edges (`at_f`); widths rounded to whole pixels.
* A **1 px `#202020` outline** round every filled shape (`poly`, `fill`, `dot`), none when the icon is drawn in one
  tint (disabled).
* Colours **quantised** to a 24-entry palette: the VGA colours less the garish ones (`#000000 #404040 #808080 #A0A0A0
  #C0C0C0 #E0E0E0 #FFFFFF #000080 #0000FF #008080 #008000 #800000 #FF0000 #808000 #800080`) plus the Windows 2000/XP
  icon tones (`#3366CC #6699FF #00A0C0 #339933 #99CC66 #CC6666 #FFCC33 #CC9933 #F0E0C0`). [W][I]
* "Paper" (pages, sheets, pointers) is white; ink is black; no drop shadow, no rounded corners.
* Mute / Solo are bold black **M** / **S** letters on the chrome button (the button's Active state shows the toggle);
  the fader thumb glyph is a chrome block with a centre line; ≡ is the "More" glyph (Vegas).
* Every icon of `Icon::ALL` paints, plain and tinted (`icons::tests::every_icon_paints`).

---

## 8. Dock window contents, dialogs, brand

### 8.1 Dock windows (`widgets.rs` shared painters)

* **List boxes** (`scroll_pane`): a sunken white field (`sunken`, 2 px 3D edge) holding the tree / list / tile grid;
  `pane_split` is a dotted metal splitter. [W]
* **Tree rows** (`tree_row`): ⊞ / ⊟ 9×9 white boxes with a grey edge, dotted lines (`dotted`, a pixel every 2 px) from
  the node to its icon and down the parent's column; the selected label alone carries the `#316AC5` bar with a dotted
  focus rectangle, as the Windows TreeView. No hover fill. [W]
* **List rows** (`list_row`): `#316AC5` full-row highlight, no hover; Edit Details adds `grid_lines` (`#C0C0C0`
  rules under each row and between the columns). [W]
* **Column headers** (`header_row_sorted`): one raised bevelled button per column, a blank one to the row's end, a
  ▲ / ▼ in `metal.shadow` on the sorted column (Explorer by name, Overlays details by its sort). [W]
* **Thumbnails** (`paint_thumb`): 1 px `#202020` frame and a 1 px `#A0A0A0` mat; **tiles** (`tile`) are raised chrome
  buttons (Hover brighter, selected = Active, pressed with the blue tint). [R Vegas 6 presets][I]
* **Meters**: wells sunken (`meter_well`); levels as **LED segments** (`led_bar`: 2 px lit on a 3 px pitch, green to
  −12 dB, yellow to −3 dB, red above); peak-hold line and clip lamps as before. Master Bus, Mixing Console (strips on
  blue-grey `header_metal`, I/O boxes sunken white, the channel list separated by an etched line), Loudness Meters
  (stat read-outs in sunken `meter_bg` boxes). [I]
* **Faders** (`groove_v` + `fader_thumb` 25×13 with the dark bar across; sends and pan on `groove` + `thumb` 9×13).
  [R Vegas 5–7]
* **Video Preview**: the transport row and status block on a metal bar; the read-outs as cyan LCD digits (reduced
  quality / dropped frames in orange-red) in a sunken `time_display_bg` well. [R Vegas 6 status strip][I]
* **Video Scopes**: panel-grey body, each scope a black well in a sunken 3D frame. [I]
* **Keyframe controller** (Pan/Crop, Track Motion, FX window): metal button row with chrome buttons (Sync Cursor =
  Active), a sunken well round the lanes, the time in a black LCD box; black lane ink (`lane_ink`). [I]
* **Plug-In Manager**: sunken check boxes (face-grey when fixed), the details strip a sunken pane; the choosers'
  category chips are chrome buttons (the current one pressed) and their description strip a sunken pane. [W]
* **Tool windows** (Pan/Crop, Track Motion, FX) and **floating windows**: the classic caption (§8.2) on the tool
  windows; floating windows (native OS caption) get a raised 3D frame round their contents.
* egui scroll bars: grey face handle on the white rail (`scroll.foreground_color` off).

### 8.2 Dialogs (`dialogs/mod.rs`, one shared frame)

* **Frame** (`classic_caption`, used by `framed` for every modal, window and tool window): a raised 2 px 3D edge in
  the `dialog_metal` palette (white / `#D4D0C8` / `#808080` / `#404040`), an 18 px caption gradient `#0A246A` →
  `#A6CAF0` with bold white Tahoma 11, 16×14 bevelled ? and × buttons (pressed = sunken, glyph shifted 1 px). The
  title row is 24 px (`title_h`). [W Windows 2000 active caption]
* **Tabs** (`tabs`): raised tabs (white left/top, shadow + dark right), the selected one 2 px taller and joined to the
  page; the **page** (`page`) is a raised panel. [W tab control]
* **Buttons** (`classic_button`, used by `button` / `button_row`, the Welcome dialog): `#D4D0C8` face, 2 px bevel,
  sunken while pressed with the label shifted; the default button has a 1 px black ring outside the bevel; disabled
  labels are grey with a white emboss. The colour **swatch** is bevelled. [W]
* **Field boxes** (`field_box`): sunken white lists / option boxes (Preferences General, Render, Render Queue,
  Customize Keyboard / Toolbar, Organize Layouts, Recorded Files, Update, the Welcome recents).
* **egui controls** (`style`): white fields with a `#808080` edge, `#D4D0C8` button faces with a `#404040` edge, no
  rounding, no shadows; combo popups white with a dark border; the `#316AC5` selection. (Check boxes and radios stay
  egui's flat drawing on those colours: a polish item.)
* **Preferences › General**: "Show splash screen at startup" (`Prefs::show_splash`, on by default).
* **Welcome** (`Welcome::show`): a 540×522 "Milan Pro" dialog: the splash art, a *Start* group with New
  Project (default), Advanced Settings… and Open Project…, the recent projects in a sunken list, "Show this dialog at
  startup", Close (FullHD 29.970p for New).
* **About** (`dialogs::about`): `about-banner-480x120.png` over "Milan Pro version x" (Tahoma bold), licence and a
  *Credits* group, OK.

### 8.3 Brand and splash

* `Icon::Logo` paints `milan-pro-256.png` (the chrome M) (`widgets::brand_tex`, decoded once per context; tinted
  when the icon is).
* **Splash** (`ui/splash.rs`): a 520×320 borderless, non-resizable, always-on-top viewport built with
  `with_active(false)` and no taskbar button, centred on the main window's monitor, showing `splash-520x320.png` with a
  cyan progress bar along its bottom for 1.2 s, then closed. Never in an off-screen run (`MILAN_SCREENSHOT`,
  `MILAN_UPDATE_AUTO`: `Splash::wanted`), and off with the preference.

### 8.4 Harness

`MILAN_DIALOG` has a value for every dialog and window (`open_from_env`); `MILAN_LOOK` is no longer read (old scripts
may still pass it).

### 8.5 Polish

* **Windows 2000 controls everywhere egui draws them** (`widgets.rs` §8.6): check boxes, radio buttons, combo boxes,
  spinners (drag values), sliders, progress bars, text fields and push buttons go through one set of wrappers
  (`widgets::checkbox`, `radio`, `radio_value`, `combo`, `spin`, `slider`, `progress`, `edit`, `push_button`,
  `small_push`; `_enabled` forms for `Ui::add_enabled`): egui lays out and runs the control and the Windows 2000 one
  is painted over it. The dialogs' `classic_button`
  and `sized_button` are the same painter (`classic_push`).
* **Bodies finished**: the Trimmer's strips sit in a sunken well, its transport and read-outs on a metal bar with
  the In / Out / Length as cyan LCD digits in a black well; Color Grading's wheels and curves are on sunken dark wells
  with chrome knobs (puck, curve points) and chrome channel buttons; Project Notes separates notes with etched rules
  and sunken note fields; the Render dialog's lists and details are sunken field boxes and its template tiles raised
  bevelled buttons (pressed, blue-tinted when chosen); the Keyboard Shortcuts and Slideshow lists are sunken white
  boxes (no stripes); the external preview gets a 1 px sunken edge when the picture does not fill its display.
* **Harness placement**: `MILAN_DIALOG` opens once the harness window has its 1920×1080 size (`env_dialog`), so
  dialogs centre as in a live session. eframe cannot shoot an immediate viewport, so an off-screen run draws floating
  windows and the external preview inside the main window at their place (their own frame and caption) for the
  screenshot; a live session keeps the native windows.
* **Tree lines** stop at the last child: each row draws the column above itself from the last connector at its depth
  (the parent's box or the previous sibling), kept per list and pass (`tree_row`), with a stub from the column to the
  node; dots keep one phase (`dotted` on even pixels).
* **Floating windows** carry the Windows 2000 tool-window caption (`classic_caption`, the small bevelled × closes,
  dragging it moves the window) under the OS title bar, which cannot be restyled.
* **Consistency**: no rounded corners, soft shadows, translucent panel fills or dark panels remain (the translucent fills left are overlays: time selection, drop ghosts, the Trimmer's in/out shading); every text
  face is Tahoma through `Theme::font`.
* **Performance**: the metal painters are unchanged; `bench_playback` (release, alone) still holds 30 fps at 1–4
  layers.

Deliberately left as-is:

* The OS title bar above a floating window (two captions: the native one moves and closes the window too).
* The hover bar of a combo list and of egui popup menus stays the dialog's button face rather than the `#316AC5` bar.
* The classic slider has no keyboard stepping (its spinner has); the thumb is the Vegas fader block, not the Windows
  trackbar pointer.
* egui scroll bars inside lists keep egui's grey handle (classic chrome scroll bars are the timeline's only).
* The external preview full screen on a second display stays a plain black picture (the edge shows only when the
  picture is inset).

### 8.6 Control painters (`widgets.rs`)

| Painter | Draws |
|---|---|
| `check_box` | 14×14 sunken 2 px edge, white (face when disabled), a 7×7 pixel check in black [W] |
| `radio_mark` | white disc with a 3D ring (shadow / dark upper left, light / face lower right), a 4 px black dot [W] |
| `combo_frame` | sunken 2 px edge round egui's white field, a raised 20 px arrow button with a black ▾ (pressed while the list is open) [W] |
| `spin_buttons` | two raised 13 px faces with black ▴ ▾ at the right end of a sunken field; they step the value [W] |
| `progress_blocks` | sunken face-grey well, `#316AC5` blocks two thirds as wide as tall, 2 px apart [W] |
| `classic_push` | `#D4D0C8` face, 2 px bevel (sunken while pressed, label shifted), black ring on the default button, greyed label with a white emboss when disabled [W] |
| `slider` | `groove` + 11×15 chrome `thumb`, then the value spinner and the label [R Vegas] |
| `edit` | egui's field with a 5 / 3 px margin and the sunken 2 px edge over it [W] |

The palette comes from `widgets::skin`: the `#D4D0C8` property-sheet one inside a dialog (`dialogs::style` fills the
panel with the dialog page colour), the metal one in the dock windows.
