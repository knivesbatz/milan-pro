# Milan Pro: interface specification

Status: implementation spec (2026-11-26). Milan Pro draws one look of its own on top of the VEGAS Pro 23 **layout**
described in `VEGAS_UI_SPEC.md`: the docking areas on all four sides, the built-in layouts, the tab strips at the
bottom of each group, the menus, toolbar sets, commands and keys. The window keeps the **native Windows title bar**
and the menu is an **in-window menu row** under it. Sections 2-5 are the **design system**: every surface is painted
from these tokens and recipes (`theme.rs`, `widgets.rs`); a surface that needs something new adds a token or a
recipe here first.

Evidence tags: **[R]** a convention of the period software named in §1; **[B]** taken from Milan Pro's own brand
(`assets/brand/BRAND.md`); **[W]** a Windows XP system value; **[I]** a design decision (tunable, not a
measurement). Nothing here copies a bitmap, a skin or a logo; every surface and icon is painted in code.

---

## 1. Direction

One design language, shared with the brand (Luna-blue film reel, orange play wedge, chrome-and-amber wordmark):
**brushed silver chrome + deep navy LCD + one amber accent**, the idiom of the 2002-2006 media software skins
(players, DVD tools, Pinnacle Studio 9-10, Ulead MediaStudio 8) and of the brand's aluminium footer. [R][B]

| Principle | What it means on screen |
|---|---|
| One light | Light comes from the upper left: every raised surface has a bright top-left lip and a dark bottom-right edge; every recessed one the reverse (dark inner top, light lower lip). |
| Two materials | **Silver** for everything you press, drag or frame (bars, buttons, tabs, panels, knobs). **Navy LCD** for everything that shows a live value or a list you browse (time, meters, read-outs, status panes, media lists, trees). |
| Paper for typing | Fields you type into (text, numbers, combo values) and dialog option lists are light **paper** wells, so dense VEGAS forms stay quick to read. |
| Neutral for pictures | Video, thumbnails, scopes, colour wheels and curves sit on **black / neutral** wells, never navy or silver, so colour judgements are not biased. |
| One blue, one silver, one amber | The selection blue is the brand flange blue; the silver is the brand aluminium; amber marks the single most important lit thing in a region (active dock tab lamp, record, warnings on an LCD). |
| One radius family | 5 px outer frames, 3 px controls and wells, pills fully round. Nothing else is rounded. |
| Text never on texture | The brushed streak texture is faint (at most ±2 % luminance) and only on bars; every text / background pair meets WCAG AA 4.5:1 (§2.8, checked by `theme::tests::contrast_aa`). |

Not taken: any skin's bitmaps, names, mascots, logos or the player layout; translucency, glass, drop shadows wider
than 1 px, anti-aliased icon art. Milan Pro keeps every VEGAS window, menu, shortcut and dialog where it was.

---

## 2. Colour tokens (`theme.rs`)

### 2.1 Silver ramp (`Silver`, `t.silver`)

The cool aluminium of the brand footer, darkened in five steps. A **raised chrome** surface is a vertical
gradient `s1` (top) → `s2` (45 %) → `s3` (bottom); a **pressed** one is the same gradient upside down and one step
darker.

| Token | Hex | Use |
|---|---|---|
| `s0` | `#FFFFFF` | Specular lip: the 1 px line inside the top / left of raised chrome [B] |
| `s1` | `#F3F5F8` | Chrome top, menu popup body, hovered chrome [B footer top] |
| `s2` | `#DDE1E7` | Chrome middle; **panel body** (dock window bodies, dialog pages) [B footer mid] |
| `s3` | `#C4CAD3` | Chrome bottom [B footer bottom] |
| `s4` | `#A6AEBB` | Recess: slider and scroll tracks, inactive tabs, tab strips, ruler band bottom [I] |
| `s5` | `#7A8494` | Shadow edge (bottom-right of raised, top-left of sunken), grooves [B hub dark] |
| `s6` | `#4A5468` | Outline round every control and panel [I] |

### 2.2 Slate (frames and captions)

| Token | Hex | Use |
|---|---|---|
| `slate.hi` / `slate.lo` | `#5B6C8E` / `#34425E` | Caption bar gradient (dialogs, tool and floating windows) [I] |
| `slate.frame` | `#36425C` | Window gutters, splitters' base, the outer frame of dialogs [I] |
| `slate.ridge` | `#7D8DAA` / `#1E2840` | The two grooved ridges either side of a caption's title (light over dark) [I] |
| `slate.text` | `#F3F5F8` | Caption title (bold) [I] |

### 2.3 Navy LCD (`Lcd`, `t.lcd`)

| Token | Hex | Use |
|---|---|---|
| `lcd.deep` | `#00225F` | LCD wells: time displays, meters, read-outs, status panes, tooltips' body is `lcd.tip` [I] |
| `lcd.well` | `#1C3D7D` | List and tree wells (Project Media, Explorer, chooser lists, Plug-In Manager, command palette) [I] |
| `lcd.hover` | `#24488C` | Hovered list row [I] |
| `lcd.rule` | `#3A5A9A` | Tree lines, list grid rules, the LCD dot grid (at 35 %) [I] |
| `lcd.ghost` | `#0E3477` | Unlit segments of the segmented digits [I] |
| `lcd.outline` | `#0B1F4F` | 1 px outline round a well [B stroke navy] |
| `lcd.tip` | `#0A2A6A` | Tooltip body [I] |
| `lcd.text` | `#A8C8FF` | Text on any navy well [I] |
| `lcd.text_hi` | `#E6F0FF` | Emphasised LCD text, tooltip text, list text of the focused item [I] |
| `lcd.text_dim` | `#8DB0EE` | Secondary text, **only on `lcd.deep`** (units, labels inside a read-out) [I] |
| `lcd.digits` | `#DCEBFF` | Lit segments of the big time digits [I] |

### 2.4 Selection and lit states

| Token | Hex | Use |
|---|---|---|
| `sel.top` / `sel.bottom` | `#3A68CE` / `#2450B4` | Selection gradient: list rows, menu highlight, selected tree label, progress fill, ON pills [B flange] |
| `sel.mid` | `#2F5CC1` | The flat selection (egui-drawn highlights, text selection) |
| `sel.lip` | `#7FA2E8` | 1 px light line along the top of a selection bar |
| `sel.text` | `#FFFFFF` | Text on the selection |
| `lit.top` / `lit.bottom` | `#E3EDFF` / `#9DBDF2` | A toggle that is on (toolbar tools, M / S / fx, Sync Cursor, selected tile frame): sky-blue chrome with navy ink [I] |
| `lit.outline` | `#2F5CC1` | Outline of lit chrome and the keyboard-focus ring |

### 2.5 Amber accent

| Token | Hex | Use |
|---|---|---|
| `amber` | `#FFA424` | The active dock tab's lamp, record-armed lamps, LCD warnings (dropped frames, reduced quality) [B wedge] |
| `amber.hi` / `amber.lo` | `#FFDC90` / `#D86000` | Lamp gradient [B] |
| `amber.text` | `#FFB43C` | Amber text on navy (warnings in a read-out) [B] |

Amber is never a fill behind text and never more than one lamp per region.

### 2.6 Ink

| Token | Hex | Use |
|---|---|---|
| `ink` | `#0F1C3A` | Text and glyphs on silver and paper (menus, labels, dialog text, ruler) [I] |
| `ink.label` | `#2E3A55` | Secondary labels on silver (track header slider names, group captions) |
| `ink.button` | `#0C2A78` | Push button and pill labels on silver [B outline navy] |
| `ink.glyph` | `#1A49B8` | Monochrome glyphs we draw on silver: ▾ ▴ ◂ ▸, ×, ?, check marks, radio dots [B flange dark] |
| `ink.disabled` | `#4C566B` | Disabled text on silver and paper (still ≥ 4.5:1: VEGAS-dense dialogs are read while disabled) |
| `paper` | `#FBFCFE` | Fields and dialog option lists |
| `paper.disabled` | `#E8EBF0` | A disabled field |

### 2.7 Domain colours (unchanged meaning)

Timeline lanes move from gunmetal to a **blue-slate** that belongs to the navy family but stays desaturated, so
the pastel event colours keep their identity: `lane_bg #2B3550`, `empty_lane_bg #222A40`, `grid_line #3D4A68`.
Event bodies and headers, the track colour palette, fades, envelopes, markers, regions, loop flags, the playhead,
snap colours and meter LED colours keep their values and meaning (`VEGAS_UI_SPEC.md`). Pan/Crop and Track Motion
keep their light neutral workspace (`workspace_bg #ECECEC`): it shows a picture.

### 2.8 Contrast (WCAG 2.1, normal text ≥ 4.5:1)

Checked against the worst case of each background (the darkest stop of a gradient a label can sit on, and the
streak texture's darkest pixel) by `theme::tests::contrast_aa`:

| Text | On | Ratio |
|---|---|---|
| `ink` | `s3` (chrome bottom) / `s4` (recess) | 10.2 / 7.5 |
| `ink.label` | `s3` | 6.9 |
| `ink.button` | `s3` / `lit.bottom` | 7.9 / 6.8 |
| `ink.disabled` | `s3` / `s2` / `paper.disabled` | ≥ 4.5 each |
| `ink` | `paper` | 16.4 |
| `slate.text` | `slate.hi` (caption top) | 4.8 |
| `lcd.text` | `lcd.well` / `lcd.hover` / `lcd.deep` | 6.2 / 5.2 / 8.9 |
| `lcd.text_dim` | `lcd.deep` | 6.9 |
| `lcd.digits` | `lcd.deep` | 12.5 |
| `lcd.text_hi` | `lcd.tip` | 11.8 |
| `amber.text` | `lcd.well` | 5.9 |
| `sel.text` | `sel.top` / `sel.bottom` | 5.2 / 7.3 |
| `ink` | `lit.top` / `lit.bottom` | 14.3 / 8.8 |
| white event / marker text | their tags | as `VEGAS_UI_SPEC.md` (domain colours) |

---

## 3. Typography

* **Tahoma 8 pt = 11 px** for all UI text (`C:\Windows\Fonts\tahoma.ttf`, bold `tahomabd.ttf`, loaded by
  `fonts::install`; Segoe UI, then egui's fonts, fall in when a file is missing): the VEGAS-dense layout fits only at
  this size. Bold for captions, the default button, track numbers, section tabs and the active dock tab.
* `FontSizes`: ui / menu / dock tab / label / event name / status 11; track number 11 bold; ruler 10; mute/solo 9;
  meters 8; egui `Small` 9.
* **LCD digits** (`widgets::lcd_digits`): time and frame read-outs are painted as seven-segment digits, slanted 8 %,
  with the unlit segments in `lcd.ghost` (as a real LCD shows them). Heights: 22 px in the timeline's time display,
  15 px in the Trimmer and the Video Preview, 11 px in small read-outs (where Tahoma Bold in `lcd.digits` is used
  instead when a value holds letters). Characters other than `0-9 : ; . , - +` fall back to Tahoma Bold.
* Text sits on whole pixels (egui rounds galley origins); 1 px strokes are drawn on pixel centres.

---

## 4. Metrics (`Metrics` in `Theme::new`)

### 4.1 Shape

| Token | px | Use |
|---|---|---|
| `r_frame` | 5 | Outer corners of dialogs, tool windows, dock groups, preset tiles [I] |
| `r_ctl` | 3 | Buttons, tabs (outer corners only), wells, fields, menus, tooltips |
| `r_small` | 2 | Check boxes, toolbar icon buttons under 20 px |
| pill | h / 2 | Toggle pills, section title tabs, slider knobs, scroll thumbs |
| `hair` | 1 | One line of chrome: outline, lip, shadow (`widgets::hair()`, the one place a DPI scale applies) |

**Bevel recipe** (`widgets::chrome`): 1 px `s6` outline (anti-aliased, rounded), inside it 1 px `s0` lip along the
top (and left on tall shapes) and 1 px `s5` shade along the bottom. **Recess recipe** (`widgets::well`): 1 px
outline (`s5` on silver; `lcd.outline` round a navy well), a 2 px inner shadow at the top (black 30 % then 12 %),
a 1 px `s0` lower lip outside the bottom edge on silver.

### 4.2 Spacing and sizes

Spacing grid **2 px** (gaps of 2, 4, 6, 8). Control heights: push button 23 (dialogs) / 21 (dock windows),
toolbar button 22, toggle pill 18, field / combo / spin 21, check box 13, radio 13, section tab 16, slider knob
15 × 9 (horizontal) / 21 × 11 (fader), scrollbar 16, tab strip 21.

### 4.3 Layout bands (unchanged from 0.10)

| | px | Note |
|---|---|---|
| Title bar | 0 (native) | the OS draws it |
| Menu row | 20 | chrome, 1 px `s5` under it |
| Main toolbar | 26, 2 px above, 1 px below, 2 px gap | buttons 22 × 22 on a 23 px pitch |
| Dock toolbars | 26 | chrome bar; the body sits in the group's frame |
| Dock tab strip | 21 | tabs 8 + label + 8 (active + 38 for □ ×) |
| Splitters | 6 | slate with a 3-dot silver grip |
| Outer gutter | 2 | slate |
| Track list | 250 | colour strip 20 |
| Time display | 40 | LCD well inset 3 / 4 px |
| Band | 42 | loop · markers · ruler |
| Tracks | 56 / 56 | slider rows on a 16 px pitch |
| Transport | 28 | chrome bar, LCD read-outs |
| Status bar | 20 | chrome bar, LCD panes |
| Icons | 16, 1 px stroke | |
| Popup rows | 19 | gutter 24 |

Band positions at 1920×1080: menu 0-20, toolbar 23-49, dock 52-586, splitter 586-592, timeline from 592, transport
1027-1055, status 1059-1079 (`chrome::tests::default_bands_match_spec`).

---

## 5. Component recipes (`widgets.rs` unless named)

| Component | Recipe |
|---|---|
| **Chrome fill** `chrome_fill` | Rounded vertical gradient mesh (exact piecewise-linear, any radius) + optional streaks (bars only). |
| **Bar** `metal_bar` / `metal` | Raised chrome without rounding (menu row, toolbars, transport, status, dock toolbars): `s1`→`s2`→`s3`, `s0` top line, `s5` bottom line, streak texture at 25 % (±2 %). |
| **Panel frame** `panel_frame` | `s2` body, 1 px `s6` outline with `r_frame` outer corners, a 1 px `s0` lip inside the top/left. Dock groups, dialog bodies, tool windows. |
| **Section title tab** `section_tab` | A pill 16 px tall set into the top edge of a group: raised chrome, `ink` Tahoma Bold label, and a groove (`s5` over `s0`) running from the pill to the group's right edge. Dialog groups (`dialogs::group`), FX window sections. |
| **LCD well** `lcd_well` | `lcd.deep` (read-outs) or `lcd.well` (lists) with the recess recipe in navy: `lcd.outline` outline, `r_ctl`, inner top shadow, a faint 2 px dot grid in `lcd.rule` at 35 % on read-outs (never under list text). |
| **Push button** `classic_push` | Raised chrome, `r_ctl`, label `ink.button`. Hover: gradient lifted toward `s1` and the outline turns `lit.outline`. Pressed: inverted gradient, label 1 px down-right. Default: a 2 px `lit.outline` ring inside the outline, bold label. Disabled: flat `s2`, outline `s4`, label `ink.disabled`. |
| **Toolbar / icon button** `chrome_button` | As the push button at 22 px with `r_small`; always raised (the VEGAS 5-7 convention). **Active** (a tool, a toggle that is on) = lit chrome (`lit.top`→`lit.bottom`, `lit.outline`). |
| **Toggle pill** `pill` | Pill-shaped chrome with an `ink.button` label; on = `sel.top`→`sel.bottom` with a white label and `sel.lip` (the lit "ON" pill). Chooser category chips, keyframe controller toggles. |
| **Transport button** `transport_button` | Round chrome (`h/2`), the same bevel, the transport glyph centred; Active (loop, play while playing) = lit. |
| **Slider** `slider`, `groove` + `thumb` | Track: a 4 px recessed pill in `s4` with a `s5` top edge and `s0` bottom lip; the part left of the knob filled `sel.mid` (value fill). Knob: a 15 × 9 lozenge of raised chrome with a 1 px dark centre notch; automation tints the notch with the envelope colour. |
| **Fader** `groove_v` + `fader_thumb` | The same, vertical, knob 21 × 11 with a notch across. |
| **Scrollbar** (`timeline/scroll.rs`) | Track `s4` recess; thumb = raised chrome pill with three grip ridges; arrow buttons = chrome with `ink.glyph` triangles. egui scrollbars: `s5` handle on an `s3` rail, rounded. |
| **List / tree in a well** `scroll_pane`, `tree_row`, `list_row` | `lcd.well`; rows 20 px, text `lcd.text`; hover `lcd.hover`; ⊞/⊟ in `lcd.text` outline boxes; tree lines dotted `lcd.rule`; column headers are a chrome bar of raised segments with `ink` labels and an `ink.glyph` sort arrow. |
| **Selection** `selection_bar` | `sel.top`→`sel.bottom`, `sel.lip` top line, `sel.text`; on a full row in lists, the label only in trees; menus use `sel.mid`. |
| **Menu bar** (`menus.rs`) | Bar recipe; item hover = lit pill (`r_ctl`); open item = selection pill with white label. |
| **Menus** (`chrome::menu_style`) | `s1` body, 1 px `s6` outline, `r_ctl`, 3 px margin; hover row `sel.mid` with white text; separator = groove; disabled `ink.disabled`. |
| **Toolbars** | Bar recipe, a 3-dot grip at the left end, separators as grooves (`s5` + `s0`). |
| **Dock tabs** (`dock.rs::classic_tab`) | Strip = `s4` recess. Tabs = chrome with `r_ctl` on the corners away from the content; inactive tabs one step darker (`s2`→`s4`) and 2 px shorter; the active one joins the content, bold `ink` label, and an **amber lamp** (2 px `amber` line, 1 px `amber.hi` over it) along its outer edge. |
| **Dialogs** (`dialogs/mod.rs`) | `slate.frame` 1 px outline with `r_frame` corners; caption = slate gradient 20 px with the bold `slate.text` title between two grooved ridges; caption buttons = 16 × 14 chrome with `ink.glyph` ? and ×. Body = panel frame. Tabs (`tabs`) = the dock-tab recipe standing on the page (no lamp; active label bold). Button row = push buttons, 75 × 23, the default one ringed. |
| **Track headers** (`timeline/tracks.rs`) | Raised chrome card per track (`s1`→`s3`, selected = `lit.top`→`lit.bottom`), 2 px slate rule between tracks, M / S / fx / arm as 18 × 18 chrome buttons (lit when on; the armed lamp amber). |
| **Timeline ruler** (`timeline/ruler.rs`) | Bar recipe for the band; ticks and numbers `ink`; marker / region / loop tags keep their domain colours. Time display = a big LCD well with segmented digits. |
| **Meters** (`master_bus`, `mixer`) | `lcd.deep` LCD well, LED segments (2 px on a 3 px pitch) green / yellow / red, peak line `lcd.text_hi`; labels `ink` on silver beside the well, `lcd.text_dim` inside it. |
| **Tooltips** | `lcd.tip` body, 1 px `lcd.rule` outline, `r_ctl`, `lcd.text_hi` text. |
| **Check box** `check_box` | 13 × 13 `paper` with the recess recipe and `r_small`; check = 2 px `ink.glyph` anti-aliased tick; disabled `paper.disabled` and an `ink.disabled` tick. |
| **Radio** `radio_mark` | `paper` disc, `s5` outline, top inner shadow; on = 5 px `sel` gradient dot with a white glint. |
| **Combo** `combo_frame` | `paper` field with the recess recipe; a chrome arrow button inside the right end (corners `r_ctl` on the right) with an `ink.glyph` ▾; pressed while open. |
| **Spin** `spin_buttons` | Two chrome half-buttons with ▴ ▾ in `ink.glyph` at the right end of a paper field. |
| **Edit** `edit` | `paper` field, recess recipe, `r_ctl`; focus = `lit.outline` ring. |
| **Progress** `progress_blocks` | A recessed `lcd.deep` pill, the done part a `sel` gradient pill with `sel.lip`; the label white (AA on both). The splash's bar is the same recipe. |
| **Thumbnails / preset tiles** `tile`, `thumb_well` | Raised chrome tile, `r_frame`; the picture in a black well with a 1 px `s5` rim; caption `ink` on silver. Hover: lifted gradient; selected: lit chrome with a 2 px `lit.outline` ring. |
| **Splitters** `dotted_splitter` | `slate.frame` base with a 3-dot chrome grip centred along it. |

---

## 6. Layout and workflow

Layout, docking, commands, shortcuts (the VEGAS 23 key map), the toolbar button sets (`Bar::defaults`), the menus, the
Start dialog and the VEGAS 23 windows (Color Grading, Project Notes) follow `VEGAS_UI_SPEC.md`; only the drawing is
Milan Pro's own. Saved layouts and toolbars are keyed by slot (`last`, `0`-`9`, `toolbar`, `timeline_toolbar`);
preferences saved by 0.8.0 and earlier carry their layouts, toolbars and keys over (`settings::migrate_look_keys`).

---

## 7. Icons (`icons.rs`)

Coloured shapes, rendered as 2006 pixel art (unchanged by the design system: they sit on silver chrome, on lit
chrome and on paper, all light, so their dark outline reads everywhere):

* 1 px strokes on pixel centres (`at_s`), fills on pixel edges (`at_f`); widths rounded to whole pixels.
* A **1 px `#202020` outline** round every filled shape (`poly`, `fill`, `dot`), none when the icon is drawn in one
  tint (disabled).
* Colours **quantised** to a 24-entry palette: the VGA colours less the garish ones (`#000000 #404040 #808080 #A0A0A0
  #C0C0C0 #E0E0E0 #FFFFFF #000080 #0000FF #008080 #008000 #800000 #FF0000 #808000 #800080`) plus the Windows 2000/XP
  icon tones (`#3366CC #6699FF #00A0C0 #339933 #99CC66 #CC6666 #FFCC33 #CC9933 #F0E0C0`). [W][I]
* "Paper" (pages, sheets, pointers) is white; ink is black; no drop shadow, no rounded corners.
* Icons drawn on a navy well (list rows) keep their colours; their white paper and dark outline read on `lcd.well`.
* Mute / Solo are bold **M** / **S** letters on the chrome button (the button's lit state shows the toggle); ≡ is the
  "More" glyph (Vegas).
* Every icon of `Icon::ALL` paints, plain and tinted (`icons::tests::every_icon_paints`).

---

## 8. Surfaces

### 8.1 Main window

* Menu row, main toolbar, transport bar and status bar: the bar recipe. The status panes and the transport's time
  read-outs are small LCD wells (`lcd.deep`, `lcd.text`).
* Gutters and splitters: `slate.frame` with the 3-dot grip.
* Dock groups: panel frame round toolbar + body + tab strip; the toolbar is a bar, the body `s2`.

### 8.2 Dock windows

* **Project Media, Explorer**: trees and lists in `lcd.well` with the selection gradient; the thumbnail view's tiles
  are preset tiles (black picture wells). Column headers chrome.
* **Video Preview**: picture on black; toolbar a bar; the status read-outs (project / preview / frame / display) an
  LCD well with `lcd.text` (dropped frames and reduced quality in `amber.text`).
* **Trimmer**: strips in a neutral dark well; In / Out / Length as segmented digits in an LCD well.
* **Master Bus, Mixing Console, Loudness Meters**: meters in `lcd.deep` wells; strips are chrome cards; I/O boxes
  paper; stat read-outs LCD.
* **Video FX, Transitions, Media Generators**: plug-in list in an `lcd.well`, category chips as toggle pills, preset
  tiles with black picture wells, the description strip a paper well.
* **Plug-In Manager**: list in an `lcd.well` with paper check boxes; details strip paper.
* **Color Grading, Video Scopes**: wheels, curves and scopes on neutral black wells (colour-critical); chrome knobs,
  chrome / pill channel buttons.
* **Surround Panner**: the room on a neutral dark well, chrome puck; read-outs LCD.
* **Overlays, Edit Details, Project Notes, Compositors, Properties**: lists in `lcd.well` (details grid rules in
  `lcd.rule`), notes and properties as paper fields.
* **Keyframe controller** (Pan/Crop, Track Motion, FX window): chrome button row, toggles as pills, lanes on `s2` /
  `s3` stripes, time in an LCD well.
* **Command palette**: a dialog-framed popup; search field paper; results in an `lcd.well` with the selection bar.

### 8.3 Dialogs (`dialogs/mod.rs`, one shared frame)

* **Frame** (`classic_caption`, used by `framed` for every modal, window and tool window, and the floating
  windows' in-window caption): the dialog recipe of §5.
* **Tabs** (`tabs`): chrome tabs standing on the page, the selected one joined to it and bold; multi-row as Win32.
* **Page** (`page`): panel frame. **Field boxes** (`field_box`): paper wells. **Groups** (`group`): section title
  tabs.
* **Buttons** (`classic_button` → `classic_push`): push button recipe; **swatch**: a push button with the colour
  in a 1 px `s6` frame.
* **egui controls** (`style`): paper fields with an `s5` outline, chrome button faces, `r_ctl`, no shadows; combo
  popups `s1` with an `s6` outline; the `sel.mid` selection.
* **Preferences, Render As, Project Properties, Media Properties, Start (Welcome), About, Recover, Update /
  Roll Back, FX Packages, Render Queue** all draw through this frame and these controls.

### 8.4 Brand and splash

* `Icon::Logo` paints `milan-pro-256.png` (`widgets::brand_tex`).
* **Splash** (`ui/splash.rs`): `splash-520x320.png` with the progress recipe of §5 in its groove (x 24-496,
  y 300-306) for 1.2 s; never in an off-screen run (`Splash::wanted`), and off with the preference.

### 8.5 Harness

`MILAN_DIALOG` has a value for every dialog and window (`open_from_env`); `MILAN_LOOK` is no longer read (old scripts
may still pass it). Placement: `MILAN_DIALOG` opens once the harness window has its 1920×1080 size, floating windows
and the external preview are drawn inside the main window at their place for the screenshot.

### 8.6 Deliberately left as-is

* The OS title bar above a floating window (two captions: the native one moves and closes the window too).
* Event bodies, envelopes, markers and regions keep their VEGAS domain colours (they are data, not chrome).
* The icon art (§7): already light-surface pixel art; it reads on silver, lit chrome and paper.
* Pan/Crop and Track Motion workspaces stay light neutral (they show a picture).
