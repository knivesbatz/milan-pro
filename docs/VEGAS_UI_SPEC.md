# Milan Pro: VEGAS look specification ("VEGAS 16" and "VEGAS 23")

Status: implementation spec, v1 (2026-11-11). Target: `eframe 0.31` / egui, Windows first.

This document tells an engineer how to make Milan Pro look and behave like MAGIX/Sony VEGAS Pro 16 (2018) and VEGAS Pro 23 (2025). Both looks are selectable at runtime. Everything here can be built without further research. Proprietary bitmaps, icons and logos must **not** be copied. Section 8 describes every icon as original vector shapes to draw with `egui::Painter`.

**Evidence tags used throughout**

| Tag | Meaning |
|---|---|
| **[L]** | Measured locally from a real VEGAS Pro 15.0 install (real, unscaled pixels) or read from its exe resources. VP16 kept the VP15 interface. Highest confidence for the 16 look. |
| **[M]** | Measured from a 1:1 screenshot: the official VP16/VP17 Steam store screenshots (JPEG, ±2 per channel), the official VP23 help images, or 1:1 screenshots of Vegas Pro 2026 (same UI as 23). |
| **[C]** | Confirmed by documentation, a manual, release notes or a press release (Section 10). |
| **[I]** | Inferred or a design decision. Tunable; do not treat as fact. |

All pixel values assume 100% Windows scaling (`pixels_per_point = 1.0`). Multiply by the egui `pixels_per_point` for other DPI.

---

## 1. Overview of the two looks

### 1.1 "VEGAS 16" look (VP15 to VP18 chrome, 2017–2020)
- Grey-on-grey dark theme with **raised toolbars lighter than the content**:
  - toolbars #464646;
  - content #2D2D2D;
  - ruler and tab bands #373737;
  - gutters #232323.

  Thin 1–2 px bevels sit on toolbar edges. [L]
- **Native Windows chrome.** The white Win32 title bar and white native menu bar (Segoe UI 9 pt, black text) are not themed. [L][M]
- **Coloured, semi-skeuomorphic 16×16 icons** on a 24 px pitch:
  - yellow folder, blue floppy, red mic, red/white magnet;
  - pink "M" and gold "S" round badges on track headers. [L][M]
- Active toggle buttons get a **cyan-blue #12B1FF** filled square. [M]
- The "More" glyph is a **hamburger ≡** on track colour strips, event headers and preview transports. [L][M][C]
- **Events show a coloured header bar** (≈22 px, track colour) with name and buttons, thumbnails or waveform below. Unselected events are filled with the **track colour**. Fades are drawn as **pure-blue curves**. [M][C]
- Track colours are **muted and desaturated**: slate blue, maroon, teal, steel-teal, purple. [L][M]
- One docking area (top by default), with floating windows. Dock tabs sit **at the bottom** of each dock group, with □ (maximise) and × on the active tab. [L][C]
- Space = Play/**Stop**: stop returns the cursor to where playback started. [C]

### 1.2 "VEGAS 23" look (2025 "Modernized UI")
- **Flat near-black theme**:
  - #141414 frame, tab strip and splitters;
  - #282828 for toolbars, panels and track rows;
  - #1E1E1E empty timeline area.

  No bevels. [M]
- **Custom-drawn 32 px title bar with the menu inside it**: logo, File…Help, centred title, caption buttons. [M]
- **Flat monochrome line icons** (#F0F0F0) with few accents (red record dot, orange marker flag, green region flags). Mute and Solo are plain bold letters **M** and **S**. [M]
- Active toggles get a **#3C8CDC** rounded square. [M]
- The "More" glyph is **⋯ (three dots)**. [M][C]
- Events have **no header bar by default**. A translucent dark name label sits top-left, and three dark chips sit bottom-right: crop, fx, ⋯. Audio event background = track colour × 0.5, with the waveform in full track colour. [M]
- Track colours are **saturated**: #3C8CDC, #64C8DC, #50C878, #DCB43C, #DC8C3C, #DC6450, #C878DC, #783CF0. [M]
- Playhead is a **coral #EE5544** line with a white pentagon head. [M]
- **Docking areas on all four sides** of the timeline. Text-only dock tabs at the bottom; the active tab is bold white with a 1 px line. [C][M]
- Space = Play/**Pause** by default [C: VP23 release notes build 278, "The spacebar performs a Play/Pause action by default instead of Play/Stop", https://www.moviestudiozen.com/free-tutorials/vegas-pro/vegas-pro-23-update-history]. **Conflict:** the VP23 online help (videopreview.htm, transportcontrols.htm, shortcutkeys.htm) still says Space defaults to Play/Stop. That text is carried over from older versions; follow the release notes. Since VP21 Update 1 the toggle is a **Spacebar Switch** button in the Video Preview transport (reached via ⋯ › Edit Visible Button Set). VP22 removed the old Preferences › General checkbox [C: forum threads 146966, 147123].
- Theme variants: Dark (default), Medium, Light, White. Dark and White are measured; Medium and Light are inferred.

### 1.3 Key differences (implementation checklist of switches)

| Aspect | VEGAS 16 | VEGAS 23 |
|---|---|---|
| Title bar and menu | Native OS title bar + native white menu bar (2 rows, 31 + 20 px) | Custom 32 px dark title bar with the menu inline |
| Main toolbar | 34 px (31 + bevels), #464646, 12 colour icons | 35 px, #282828, 11 mono icons (last is "VEGAS Hub Login" with text) |
| Base greys | #232323 / #2D2D2D / #373737 / #464646 | #141414 / #1E1E1E / #282828 / #505050 |
| Splitters | 7 px #232323 | 5 px #141414 |
| Dock tabs | 17 px strip; boxed tabs; active tab #464646 with □ × | 29 px strip #141414; text-only; active bold white + 1 px line |
| Toggle highlight | #12B1FF square, ≈21×17 | #3C8CDC rounded square, ≈22×18, radius 2 |
| More glyph | ≡ | ⋯ |
| Track header buttons | Mute and Solo round coloured badges (≈15 px) | Composite, fx, **M**, **S** as text/line glyphs |
| Default track height | video 64 px, audio 68 px (incl. 2 px separator) | 52 px |
| Event header bar | On (22 px, track colour) | Off; name label overlay + bottom-right chips |
| Event body | Track colour fill (thumbnails over it) | Thumbnails; audio = track × 0.5; generated media = track colour |
| Fade curves | Blue #0000F8; white when selected | White 2 px; crossfade-in curve blue |
| Selected event outline | ≈2 px #CEE500 | ≈2 px #C8C240 (red in Auto Ripple) |
| Playhead | 1 px inverse/near-black line, white flag head | 1 px #EE5544 line with #141414 outline; white pentagon head |
| Meters | Solid #00FF57 | Green → yellow → red gradient |
| Transport/edit bar | 27 buttons | 32 buttons (+ Smart Split, Color Grading, 3 nested-timeline buttons) |
| Spacebar | Play/Stop (returns to start) | Play/Pause |
| Docking | Top/bottom area + floating | Top, bottom, left, right + floating |
| File › Render | "Render As…" | "Render…" |

---

## 2. Colour tokens

Implement one `Theme` struct whose fields are the token names below (as `egui::Color32`). Provide constructors `Theme::vegas16_dark()`, `vegas23_dark()`, `vegas23_white()`, `vegas23_medium()`, `vegas23_light()`. Map them onto `egui::Visuals` (Section 2.6). Use the same token names in both looks so drawing code stays look-agnostic.

### 2.1 VEGAS 16, Dark (default; the only VP16 scheme with measured values)

| Token | Hex | Colours what | Src |
|---|---|---|---|
| `os_title_bg` | system (≈#FFFFFF) | Native title bar. Not painted by us; keep OS decorations on. | [L] |
| `menu_bar_bg` | #FFFFFF | Menu bar row | [L] |
| `menu_bar_bottom` | #F1F1F1, then 1 px #7D7D7D | Lines under the menu bar | [L] |
| `menu_text` | #000000 | Menu bar labels | [L] |
| `menu_popup_bg` | #F2F2F2 | Popup menu body (Win10 native look) | [I] |
| `menu_popup_hover` | #91C9F7 | Hovered popup item fill (Win10 native) | [I] |
| `menu_popup_text_disabled` | #6D6D6D | Disabled popup item | [I] |
| `frame` | #232323 | Client background, gutters (2–3 px), all splitters, empty area below the last track | [L] |
| `toolbar_bg` | #464646 | Main toolbar, docked-window toolbars, transport/edit bar, time display box, active dock tab | [L] |
| `bevel_hi_1` / `bevel_hi_2` | #5C5C5C / #606060 | 2 px top highlight of the main toolbar | [L] |
| `dock_bevel_1` / `dock_bevel_2` | #575757 / #616161 | 2 px top highlight of each docked window | [L] |
| `line_dark` | #333333 | 1 px under toolbars and under the ruler | [L] |
| `dock_divider` | #393939 | 1 px between a docked window's toolbar and its content | [L] |
| `panel_bg` | #2D2D2D | List/tree content, track lanes, unselected track header body, preview surround | [L] |
| `band_bg` | #373737 | Loop + marker bar, ruler, inactive tabs, status bar | [L] |
| `tab_strip_bg` | #373737 | Strip behind tabs | [L] |
| `tab_active_bg` | #464646 | Active tab; merges with the window | [L] |
| `tab_separator` | #2D2D2D | 1 px between tabs | [L] |
| `text` | #DCDCDC | Primary label text, tab labels, time display | [L] |
| `text_secondary` | #C2C6C5 | Tree items, secondary values | [L] |
| `text_label` | #959594 | "Level:", "Vol:", "Pan:", "Rate:" labels | [L] |
| `text_bright` | #FFFFFF | Event header names, glyphs on the track colour strip | [M] |
| `text_disabled` / `icon_disabled` | #6A6A6A | Disabled toolbar glyphs | [L] |
| `icon` | #F0F0F0 (white glyph parts) | Monochrome parts of icons | [L] |
| `input_bg` | #383838 | Search fields | [L] |
| `input_placeholder` | #4F5559 | Placeholder text | [L] |
| `button_active_bg` | #12B1FF | Background of active toggles (Play while playing, current edit tool, Snapping, Auto Crossfade, Lock Envelopes, Auto-Preview). The VP16 manual lists "Selected button highlight color" as **new in VP16** (Preferences › Display › Button Highlight Color). Before it (VP15 build 177, measured locally) the active fill was near-black #101010 with #2B2B2B/#151515 rounded edges. | [M] colour; [C] pref: VP16 manual "What's new" |
| `button_hover_bg` | #555555 | Hover fill of toolbar buttons | [I] |
| `track_header_sel_bg` | #738C9A | Whole header body of the selected track (blue-grey) | [L] |
| `track_header_sel_top` / `_bottom` | #434D53 / #55656E | 1 px edges of a selected header | [L] |
| `track_header_top` | #272727 | 1 px top edge of an unselected header | [L] |
| `track_separator` | #292929 + #272727 | 2 px separator under each track (header and lane) | [L] |
| `strip_divider` | #242425 | 1 px between colour strip and header body | [L] |
| `time_display_bg` | #464646 | Time display box | [L] |
| `time_display_text` | #DCDCDC | Big timecode | [L] |
| `ruler_text` | #B4B4B4 | Ruler labels. Local small-glyph reading is #989898 because of antialiasing; use #B4B4B4 as ink colour. | [M] |
| `ruler_major_tick` | #9B9B9B | 1 px, 11 px tall | [L] |
| `ruler_minor_tick` | #626262 (+1 px #414141 shadow) | 1 px, 3 px tall | [L] |
| `grid_line` | #525252 | Vertical grid at major ticks through the lanes, dashed 2 on / 2 off | [L] |
| `scroll_track` | #2E2E2E | Timeline scrollbar trough | [L] |
| `scroll_thumb` | #606060 | Scrollbar thumb and arrow buttons | [L] |
| `scroll_arrow` | #DCDCDC | Arrow glyphs | [L] |
| `native_scroll_bg` | #F0F0F0 | Light native-style scrollbars inside the Explorer and plug-in tree (VP16 quirk) | [L] |
| `loop_sel_line` | #7BB5FE (edges #567192 / #5C7CA4) | 3 px time-selection line in the loop bar | [L] |
| `loop_flag` | #FEF50C | Yellow selection/loop triangles | [M] |
| `selection_overlay` | #7BB5FE @ 22% alpha | Time selection tint over lanes | [I] |
| `length_tag_bg` / `length_tag_text` | #FFFFFF / #000000 | "+5:00" selection-length tag, crossfade length box | [L] |
| `marker_flag` | #DB8E57 | Marker tags, white number | [I] (older help image) |
| `region_flag` | #549E65 (alt #60A065, hi #73AF81, edge #5E7463) | Region tags | [L] |
| `region_text` | #D2F3E3 | Number on region tags | [L] |
| `region_line` | #4E8F5D | 1 px vertical line through lanes at region start | [L] |
| `cursor_line` | inverse of background (≈#060606 over #2D2D2D; ≈#D3D3D3 when inverted) | Edit cursor (see §5.9.6) | [L][M] |
| `cursor_shadow` | #272727 | 1 px to the right of the cursor | [L] |
| `cursor_head` | #FFFFFF | Flag on the ruler | [L] |
| `event_selected_border` | #CEE500 | ≈2 px outline of selected events | [M] |
| `fade_curve` | #0000F8 | Fade-in/out and crossfade curves (unselected) | [M][L] |
| `fade_curve_selected` | #FFFFFF | Same curves on a selected event | [M] |
| `fade_handle` | #0202F3 / #1212AD | Fade handle arcs at the event top corners | [L] |
| `snap_line` | per target type, see the snap table below (event edge #4FC2EE, cursor #FFE864, marker #E4AA81, grid #F4FFEE) | Snap indicator line | [L] |
| `meter_bg` | #232323 | Meter wells | [L] |
| `meter_green` | #00FF57 | Meter fill | [M] |
| `meter_yellow` / `meter_red` | #FFFF00 / #FF2020 | Fill above −6 dB / clip | [I] |
| `meter_label` | #999A98 | dB labels | [L] |
| `fader_track` | #6A6A6A | 1 px slider groove | [L] |
| `fader_thumb` | #FFFFFF with #232323 bars | Slider thumbs | [L] |
| `mute_badge` | #E07080 (#D18D95–#E198A0) | Round Mute button | [L][M] |
| `solo_badge` | #C8B060 (#AF9F5C–#BEAD63) | Round Solo button | [L][M] |
| `warning_text` | #EF8F7C | Preview "Display:" value when fps is below target | [L] |
| `preview_bg` | #2D2D2D | Around the video frame; frame itself #000000 | [L] |
| `preset_tile_selected` | #FFFFFF (label grey) | Selected preset tile in the FX choosers | [L] |
| `status_bg` | #373737 (top edges #2D2D2D / #424242) | Status bar | [L] |
| `envelope_volume` / `envelope_composite` | #0F009B | Volume and composite-level envelopes (full table below) | [L] |
| `envelope_pan` | #BC0400 | Pan envelope | [L] |
| `envelope_mute` | #32F0FF | Mute envelope | [L] |
| `envelope_point` | #FFFFFF | Envelope point squares | [I] |
| `icon_color_strength` | 0.75 | Saturation of the coloured icons (Preferences › Display "Icon color strength", default 75 %). Blend each coloured icon pixel 25 % toward its own luminance grey. | [L] |

**VP16 default track colour palette.** Assigned to new tracks in order, then repeating.

| Slot | Pref name | Hex | Src |
|---|---|---|---|
| 1 | Color A | #5973A8 slate blue | [L] |
| 2 | Color B | #7C3441 maroon | [L] |
| 3 | Color C | #599CA8 steel-teal | [L] |
| 4 | Color D | #5D4181 purple | [L] |
| 5 | Color E | #33897C teal-green | [L] |
| 6 | Color F | #7C7634 olive | [L] |
| 7 | Color G | #415E81 dark slate blue | [L] |
| 8 | Color H | #59A869 green | [L] |

All eight were read from the swatches of Options › Preferences › Display › Track Colors ("Track: Color A…H") in the local VP15 install (2026-11-11 re-check). They match the hues seen in the VP16 Steam screenshot. This replaces the earlier [I] slots 6–8 and fixes the order of slots 3–5. The user may have edited them, but A and B match a fresh install.

**VP16 default envelope colours** (Preferences › Display › Envelope Colors, 27 types) [L]:

| Envelope type | Hex |
|---|---|
| Volume | #0F009B |
| Pan | #BC0400 |
| Composite Level | #0F009B |
| Fade to Color | #FF453F |
| Mute | #32F0FF |
| Event Velocity | #6AD37F |
| Transition Progress | #BF7FD3 |
| Motion Blur Length | #BF7FD3 |
| Video Supersampling | #B8693C |
| Assignable FX Vol 1–4 | #6AD37F / #69EE63 / #8BFFC5 / #7CC4A1 |
| Aux Vol 1–4 | #BF7FD3 / #7C4379 / #9A2BB5 / #CF57D8 |
| FX Parameter 1–10 | #B8693C / #D8BB57 / #F2E1AE / #EB9C28 / #602B18 / #9A1600 / #F5E5D1 / #610004 / #A55317 / #703A1F |

**VP16 default snap-indicator colours** (Preferences › Display › Snap Colors, 12 types) [L]. The snap line takes the colour of the *target* type:

| Snap type | Hex |
|---|---|
| Cursor | #FFE864 |
| Envelope Point | #C85AFA |
| Loop | #4040A0 |
| Drag Origin | #FF9B50 |
| Marker | #E4AA81 |
| Region | #7FB68C |
| CD Index | #CE81E4 |
| CD Track | #E48181 |
| Command Marker | #5989FF |
| Event | #4FC2EE |
| Event Fade | #FC6AFF |
| Grid | #F4FFEE |

VP23 keeps the same three pickers (track, envelope, snap) plus **Button Highlight Color** [C: VP23 help prefdisplay]. Its defaults were not measured; use the VP16 envelope and snap tables unless a VP23 value is listed in §2.2.

Derived event colours in VP16:
- Event body = track colour (Preferences "Reverse event background colour" off).
- Event bottom edge 1 px = track × 0.73 (#7C3441 → #5B2E36) [L].
- Waveform = lerp(track, #FFFFFF, 0.55) (≈#B1A0C4 on #5E4183) [M].

### 2.2 VEGAS 23, Dark (default)

| Token | Hex | Colours what | Src |
|---|---|---|---|
| `title_bg` | #141414 | Custom title bar with inline menu (32 px) | [M] |
| `menu_text` | #F0F0F0 | Menu labels, title text | [M] |
| `menu_popup_bg` | #282828 | Popup body | [M] |
| `menu_popup_border` | #141414 | 1 px popup border | [I] |
| `menu_popup_hover` | #3C8CDC | Hovered item fill; text stays white | [M] |
| `menu_popup_text_disabled` | #787878 | Disabled item | [I] |
| `frame` | #141414 | Outer border (5 px), splitters (5 px), separators | [M] |
| `toolbar_bg` | #282828 | Main toolbar, transport/edit bar, docked-window toolbars | [M] |
| `line_dark` | #141414 | 1 px lines above and below toolbars | [M] |
| `panel_bg` | #282828 | Docked window content, track header body, track lanes | [M] |
| `band_bg` | #282828 | Ruler/marker band (same as panel) | [M] |
| `empty_lane_bg` | #1E1E1E | Timeline area below the last track | [M] |
| `tab_strip_bg` | #141414 | Dock tab strip | [M] |
| `tab_text` | #787878 | Inactive tab label | [M] |
| `tab_text_active` | #F0F0F0, bold | Active tab label | [M] |
| `tab_active_line` | #F0F0F0 | 1 px line on the edge of the active tab that faces the content | [M] |
| `text` | #F0F0F0 | Primary text | [M] |
| `text_dim` | #787878 | Dim labels | [M] |
| `icon` | #F0F0F0 (toolbar #CCCCCC–#EEEEEE) | Line icons | [M] |
| `icon_disabled` | #4B4B4B | Disabled icons | [M] |
| `button_active_bg` | #3C8CDC | Active toggle background (preference "Button Highlight Color") | [M] |
| `button_hover_bg` | #3A3A3A | Hover fill | [I] |
| `button_group_sep` | #505050 | 1 px separators between button groups | [M] |
| `track_header_sel_bg` | #505050 | Selected track header body | [M] |
| `track_separator` | #141414 | 1 px between headers, 2 px between lanes | [M] |
| `time_display_bg` | #282828 | Time display box | [M] |
| `time_display_text` | #F0F0F0, semibold | Big timecode | [M] |
| `ruler_text` | #B0B0B0 | Ruler labels | [M] |
| `ruler_tick` | #787878 | Ticks | [I] |
| `grid_line` | #505050, dotted 1 on / 2 off | Major-tick grid lines through lanes | [M] |
| `scroll_track` | #2D2D2D | Scrollbar trough | [M] |
| `scroll_thumb` | #464646 | Scrollbar thumb | [M] |
| `accent` | #3C8CDC | Selection, list selection, active chip, welcome tabs | [M] |
| `selection_overlay` | #3C8CDC @ 40% alpha | Time selection band over lanes and ruler | [M] |
| `loop_bar_off` | #3E3E3E | Loop region bar when Loop Playback is off | [M] |
| `loop_bar_on` | #3C8CDC | Loop region bar when on. The help image shows legacy navy #000080. | [I] |
| `loop_flag` | #F9F400 (shade #9D9800) | Loop/selection end triangles | [M] |
| `marker_flag` | #F38E28 | Marker tags | [M] |
| `region_flag` | #26CB5C | Region tags | [M] |
| `playhead_line` | #EE5544 | 1 px playhead line | [M] |
| `playhead_outline` | #141414 | 1 px on each side of the line | [M] |
| `playhead_head` | #F0F0F0 (outline #141414) | Pentagon head in the ruler | [M] |
| `event_selected_border` | #C8C240 | Selected event outline | [M] |
| `event_selected_border_ripple` | #DC3C3C | Selected outline while Auto Ripple is on | [C] colour [I] |
| `event_label_bg` | #000000 @ 68% alpha (1 px #141414 border) | Event name label ("Active Take Information") | [M] |
| `event_chip_bg` | #000000 @ 60% alpha | Event button chips | [M] |
| `fade_curve` | #FFFFFF (2 px) | Fade curves | [M] |
| `fade_curve_in_xfade` | #3C8CDC | Fade-in half of a crossfade | [M] |
| `fade_region_tint` | #000000 @ 35% with 4 px checker | Faded region overlay | [M] |
| `fade_handle` | #64C8DC | Cyan corner triangles and the gain/opacity pill | [M] |
| `snap_line` | #5A9FC3 + 4 px glow @ 35% | Snap indicator | [M] |
| `envelope_volume` / `envelope_composite` | #493FAB | Volume / composite-level envelope | [M] |
| `envelope_pan` | #DC6450 | Pan envelope | [I] |
| `meter_bg` | #141414 | Meter wells | [M] |
| `meter_green` / `meter_yellow` / `meter_red` | #3CC864 / #DCB43C / #DC3C3C | Gradient stops at −∞…−12 dB, −12…−3 dB, −3…0 dB | [I] |
| `fader_thumb` | #F0F0F0 square with #282828 "II" | Slider thumbs | [M] |
| `warning_text` | #E0643C | Preview "Preview"/"Display" values | [M] |
| `status_bg` | #282828 (5 px #141414 gap above) | Status bar | [M] |
| `logo_bg` / `logo_border` | #001751 / #258EE4 | 20×20 app mark in the title bar. Draw an original "M" monogram for Milan Pro; never the VEGAS "V". | [M] |

**VP23 default track colour palette.** Assigned in order 1→8, then repeating. [M] (official VP23 help screenshot)

| Slot | Hex | Name |
|---|---|---|
| 1 | #3C8CDC | blue |
| 2 | #64C8DC | cyan |
| 3 | #50C878 | green |
| 4 | #DCB43C | yellow |
| 5 | #DC8C3C | orange |
| 6 | #DC6450 | coral |
| 7 | #C878DC | lilac |
| 8 | #783CF0 | violet |

Derived event colours in VP23:
- Audio event background = track × 0.5 (#DC8C3C → #6E461E) [M].
- Audio waveform = track colour [M].
- Silence = 1 px line in track colour [M].
- Locked event = track × 0.35 plus a padlock glyph before the name [M].
- Generated-media event body = track colour [M]. Titles show a transparency checkerboard of #9A9A9A/#C8C8C8 8 px squares [I].

### 2.3 VEGAS 23, White ("lightest" interface type)

Measured from a 1:1 1920×1080 Vegas Pro 2026 White screenshot [M]. Tokens not listed are the same as Dark.

| Token | Hex |
|---|---|
| `title_bg`, `frame`, `tab_strip_bg`, `line_dark`, `track_separator` | #B4B4B4 |
| `toolbar_bg`, `panel_bg`, `band_bg`, `time_display_bg`, `status_bg` | #C8C8C8 |
| `empty_lane_bg` and track lanes | #BEBEBE |
| `scroll_track` | #F2F2F2 |
| `scroll_thumb` | #A0A0A0 [I] |
| `text`, `menu_text`, `time_display_text`, `tab_text_active`, `icon` | #1E1E1E |
| `text_dim`, `tab_text` | #6E6E6E |
| `tab_active_line` | #1E1E1E, 2 px |
| `track_header_sel_bg` | #E6E6E6 [I] |
| `menu_popup_bg` | #DCDCDC [I] |
| `icon_disabled` | #A0A0A0 [I] |
| `warning_text` | #78321E |
| `accent`, `button_active_bg`, `menu_popup_hover` | #3C8CDC (text on accent stays #FFFFFF) |

### 2.4 Medium and Light variants (both looks): all [I]

VEGAS offers Dark, Medium, Light and White radio buttons (Options › Preferences › Display › Interface Type), and a change takes effect after a restart [C]. Medium and Light were never measured. Generate them from Dark with these offsets so the code path exists, and flag them as approximations in the UI.

| Variant | VP16 panel / toolbar / band / frame | VP23 panel / frame / empty | Text |
|---|---|---|---|
| Medium | #454545 / #5E5E5E / #4F4F4F / #3A3A3A | #404040 / #2C2C2C / #363636 | white |
| Light | #9A9A9A / #B4B4B4 / #A6A6A6 / #808080 | #8C8C8C / #787878 / #828282 | black; menus stay dark-grey in VP16 (user report) |
| White (VP16) | #F0F0F0 / #FFFFFF / #E6E6E6 / #C8C8C8 | (use §2.3) | black |

User reports say VP16 Medium "is actually the shade of the Light swatch but with white text". Reproduce that quirk only if you want bug-for-bug fidelity. [C]

### 2.5 Colour rules that apply to both looks
- Track colours are per track and user-editable (8 swatches in Preferences › Display, RGBA picker) [C]. The strip, unselected event and waveform derive from it as above.
- Changing the theme requires a restart in VEGAS [C]. Milan Pro may switch live; that is acceptable.
- Text in selected or active blue elements is #FFFFFF in all themes.

### 2.6 Mapping onto `egui::Visuals`
- `visuals.dark_mode = theme != White/Light`.
- `panel_fill = panel_bg`. `window_fill = panel_bg` (floating windows). `extreme_bg_color = input_bg` (16) or `scroll_track` (23).
- `widgets.noninteractive.bg_fill = panel_bg`. `widgets.inactive.bg_fill = toolbar_bg`. `widgets.hovered.bg_fill = button_hover_bg`. `widgets.active.bg_fill = button_active_bg`.
- `selection.bg_fill = accent` (#3C8CDC; #12B1FF for 16). `selection.stroke = white`.
- `widgets.*.corner_radius`: VP16 = 0; VP23 = 2 for buttons and 0 for panels.
- `window_shadow = NONE`. `popup_shadow`: small (VP23) or none (VP16).
- `override_text_color = Some(text)`.
- `spacing.button_padding = (4, 3)`. `spacing.item_spacing = (4, 2)`. `interact_size.y = 20`. `menu_margin = 2`. `scroll.bar_width = 16`. `scroll.floating = false`.

---

## 3. Typography

| Item | Spec |
|---|---|
| UI family | **Segoe UI** for both looks [L][M] (GDI/ClearType rendering; VP23 "updated fonts" face unpublished, visually still Segoe-like). At runtime load `C:\Windows\Fonts\segoeui.ttf` (regular), `seguisb.ttf` (semibold) and `segoeuib.ttf` (bold) if present. Never redistribute them. Fallback when missing: bundle **Selawik** (Microsoft's OFL, Segoe-metric-compatible) or keep egui's default font. Insert at the front of `FontFamily::Proportional`. Register `FontFamily::Name("semibold")` and `Name("bold")`. |
| Monospace | Not used for timecode; VEGAS uses proportional Segoe digits. |
| egui size unit | Points = px at 100% DPI. Segoe UI 9 pt = **12 px**. |

| Element | VEGAS 16 | VEGAS 23 |
|---|---|---|
| Menu bar labels | 12 px regular, black, native; menu row 20 px; items start x=15 (relative to the client) with ≈10 px gaps | 12 px regular, #F0F0F0; labels ≈12 px apart, inline in the title bar starting at x≈32 |
| Popup menu items | 12 px; row 22 px; left gutter 28 px for check/radio; shortcut right-aligned with ≥24 px gap; ▸ for submenus | 12 px; row 24 px; gutter 28 px; hover fill accent |
| Window title | OS | 12 px regular, centred on the full window width |
| Toolbar tooltips | 12 px, native tooltip (light yellow-white box, black text) [I] | 12 px, dark box #282828 with 1 px #141414 border [I] |
| Dock tab labels | 12 px regular #DCDCDC | 12 px; inactive regular #787878; active **bold** #F0F0F0 |
| Panel and track labels ("Level:", "Vol:", "Pan:") | 12 px (cap 9 px) #959594; values #C4C7C2 | 12 px #F0F0F0 |
| Track number in colour strip | 14 px bold white [M, approx] | 12 px regular white |
| Track header M / S | glyphs inside 15 px round badges, 9 px bold | 16 px **bold** letters (cap 12 px), no box |
| Event name | 12 px white in the header bar, truncated with "…" | 12 px #F0F0F0 in the label box |
| Ruler labels | 10 px (digit height 7 px) [L] | 10 px (digit height 7 px) |
| Big time display | 24 px regular (digit height 16–17 px), #DCDCDC | 22 px **semibold** (digit height 15–16 px), #F0F0F0 |
| Transport-bar cursor time / length boxes | 11 px #AFAFAF | 11–12 px #F0F0F0 |
| Status bar | 12 px #DCDCDC | 12 px #F0F0F0 |
| Video Preview status lines | 12 px; labels left column, values right column | 12 px |
| Meter dB labels | 9 px (digit height 7 px) #999A98 | 9 px #B0B0B0 |
| Locale | Decimal separator follows locale. English default is "100.0 %", "0.0 dB", "Rate: 0.00". Timecode "00:00:05;00" uses ";" for drop-frame and ":" for non-drop. Use ":" and "." by default. | same |

Rendering: egui does greyscale antialiasing. Snap text positions to whole pixels (`painter.text` at `.round()` positions) to approximate the crispness of GDI.

---

## 4. Layout

### 4.1 VEGAS 16 default layout at 1920×1080 (maximised, 100% DPI)

```
y     ┌────────────────────────────────────────────────────────────────────────────────────────────┐
0     │ [OS title bar, native, 31 px]  "Untitled - Milan Pro"                           _  □  ×       │
31    │ File  Edit  View  Insert  Tools  Options  Help        (native menu bar, 20 px, #FFFFFF)       │
51    │─1 px #7D7D7D─ 2 px gap #232323                                                              │
54    │ ▔bevel 2▔ [main toolbar 31 px #464646: 12 icons, 24 px pitch, first icon x=9]  ▁1px #333333▁ │
88    │ 2 px gap #232323                                                                            │
90    ├─────────────────────────────────────────┬7┬─────────────────────────────────────┬7┬─────────┤
      │▔▔ dock window toolbar 28 px #464646 ▔▔ │ │▔▔ preview toolbar 28 px ▔▔          │ │ master  │
      │ tree 170 │ thumbnails (#2D2D2D)         │ │      video frame (black)            │ │ toolbar │
      │          │                              │ │      surround #2D2D2D               │ │ meters  │
      │          │                              │ │   ▶ ❚❚ ■ ≡  (centred transport)     │ │         │
      │          │                              │ │ Project:…  Frame:…  (status, 2 rows)│ │         │
668   │[Project Media □×][Explorer][Transitions][Video FX][Media Generators] │ [Video Preview □×][Trimmer] │[Master Bus]
      │   tab strip 17 px (#373737; active tab #464646)                                             │
686   ╞════════════════════ main splitter 7 px #232323 (+1 px edges) ═══════════════════════════════╡
695   │▔2▔ Time display 251×46 #464646 │4│▔ loop bar 5 + marker bar 22 (#373737) ▔│ ▣ marker tool 17×17 │
      │   ⋮            00:00:00;00      │ │  ruler 16 px (#373737) labels 10 px    │                     │
741   ├────────────────────────────────┤ ├──────────────────────────────────────────┤──┤
      │ ▉≡ │ header body (#2D2D2D)  (M)(S)│ │ lane #2D2D2D (events)                    │▲ │
      │ ▉1 │ Level: 100.0 %  ───────[▮▮]  │ │                                          │  │ v-scroll 17
      │ ▉▭ │                              │ │                                          │  │
      │ …tracks 64 / 68 px…             │ │ empty area #232323                        │+−│
1009  │ Rate: 0.00  ──────[▮▮]──        │ │ ◀ ▓▓▓▓ h-scroll 16 ▓▓▓▓ ▶          + − 🔍   │
1027  │   ▲ (normal-rate mark)          │ │ [transport/edit toolbar 30 px #464646] …  ⚑ 00:00:00;00 ◣ ⊡ │
1061  │ Complete: 00:00:00                               Record Time (2 channels): 37:37:45 (17 px #373737) │
1080  └────────────────────────────────────────────────────────────────────────────────────────────┘
```

**Horizontal measurements (x, 1920 wide)** [L scaled; M]

| Element | x range | Width |
|---|---|---|
| Client left gutter | 0–2 | 3 px #232323 |
| Dock column A (Project Media group) | 3–942 | ≈940 px (49%) |
| Splitter | 943–949 | 7 px |
| Dock column B (Video Preview + Trimmer) | 950–1784 | ≈835 px (43.5%) |
| Splitter | 1785–1791 | 7 px |
| Master Bus | 1792–1916 | 125–128 px fixed default width |
| Right gutter | | 3 px |
| Track list (time display, headers, Rate) | 3–253 | 251 px |
| Gap to timeline | 254–257 | 4 px #232323 |
| Timeline time-zero | x=258 | |
| Vertical scrollbar column (incl. marker-tool button on top, +/− at the bottom) | 1900–1916 | 17 px |

**Dock height.** Content area from y=90 to y=685 (≈56% of the client height). The timeline takes the rest. [M]

**Alternative stock layouts** (View › Window Layouts) [L]: decoded layout files, saved at 1920×1080. Implement them as presets.

| Layout | Dock height | Columns |
|---|---|---|
| VEGAS Pro 14 Default (Alt+D, V) | 449 | group[Project Media, Explorer, Transitions, Video FX, Media Generators] 545 · Trimmer 610 · Video Preview 613 · Master Bus 125. Splitters 7. |
| Audio Mixing (Alt+D, A) | | group 1157 · Video Preview 511 |
| Color Correction (Alt+D, C) | 438 | group 451 · Video Scopes 539 · Video Preview 539 · Master Bus 127 |
| Default Expanded Edit Mode | 747 | Video Preview 1752 · Master Bus 156 |

### 4.2 VEGAS 23 default layout at 1920×1080 (maximised) [M]

```
y     ┌────────────────────────────────────────────────────────────────────────────────────────────┐
0     │▣ File Edit View Insert Tools Options Help      Untitled - Milan Pro          ─  ▢  ✕      │ 32 px #141414
32    │ [main toolbar 35 px #282828: ▯ 📂 💾 ⇱ ⚙ ✂ ⧉ 📋 ↶▾ ↷▾  (H) Hub Login]                     │
67    │─1 px #141414─                                                                              │
68    ├5┬────────────────────────────────────────────┬5┬─────────────────────────────────┬5┬───────┬5┤
      │ │ docked window (#282828)                    │ │ Video Preview toolbar           │ │Master │ │
      │ │  (Color Grading / Explorer / …)            │ │   video frame (black)           │ │ Bus   │ │
      │ │                                            │ │   ⟲ ▶ ❚❚ ■ ⋯                    │ │ 120px │ │
      │ │                                            │ │ Project: …  Preview: …          │ │       │ │
577   │ │ Explorer  Project Media  Media Generator  Transitions  Video FX  Color Grading  VEGAS Hub  H…◂▸ │ Video Preview  Trimmer │ Master Bus │
      │ │   tab strip 29 px #141414, text-only, active = bold white + 1 px line on top                 │
606   ├5┼──────────────────────────────┬5┬───────────────────────────────────────────────────────┬──┤
      │ │ ⋮        00:00:00;00  (45 px) │ │ marker bar / loop bar / ruler (45 px, #282828)        │⚑ │
651   │ ├──────────────────────────────┤ ├───────────────────────────────────────────────────────┤  │
      │ │•••│ ▣ fx  M  S  (header)      │ │ lanes #282828, 52 px each                              │▲ │
      │ │ 1 │ Level: 100.0 %  ─────[II] │ │                                                        │  │
      │ │⌄⌄ │                           │ │ empty #1E1E1E                                          │+−│
1004  │ │ (blank)                       │ │ ◂ ▓▓▓ h-scroll 16 ▓▓▓ ▸                         + − 🔍 │
1021  │ │ Rate: 0.00  ────[II]──        │ │ ● ⟲ ▶| ▶ ❚❚ ■ |◀ ▶| ◀❚ ❚▶ │ ↖▾ ⌇ ⬚ 🔍 │ ✕ … │ ⚑ ⚑⚑ │ 🧲 ✕ … │ ●●● │ ⧉ ⧉ ⧉ │   ⚲ 00:00:00;00 ⊡ ⊡ │ 32 px
1053  │ 5 px gap #141414                                                                           │
1058  │ Complete: 00:00:19                                     Record Time (2 channels): 1:04:21:55 │ 20 px #282828
1080  └────────────────────────────────────────────────────────────────────────────────────────────┘
```

**Horizontal measurements**

| Element | x range | Width / note |
|---|---|---|
| Outer frame | | 5 px #141414 on left, right and bottom |
| Dock column A (left group) | 0–1034 | ≈1035 px (54%) |
| Splitter | | 5 px |
| Dock column B (Video Preview, Trimmer) | 1040–1789 | ≈750 px (39%) |
| Splitter | | 5 px |
| Master Bus | 1795–1914 | ≈120 px (≈7%) |
| Track list | 5–254 | 250 px |
| Splitter | | 5 px |
| Timeline time-zero | x=260 | |
| Vertical scrollbar column | | 17 px |

**Dock height.** Content plus tab strip, y 68–605 (≈50% of the client height).

**Left-group tab order (VP23)** [C]:
1. Explorer
2. Project Media
3. Media Generators (the tab reads "Media Generator")
4. Transitions
5. Video FX
6. Color Grading
7. VEGAS Hub
8. Hub Explorer

Milan Pro: drop the Hub tabs (no online service) or replace them with a "Notes" tab [I]. Active tab on first launch: Project Media [I] (the official screenshot shows Color Grading).

**Stock layouts (VP23)**, View › Window Layouts:
- Default (Alt+D, D)
- Audio Mixing (Alt+D, A)
- Color Correction (Alt+D, C)
- Source Window Layout (Alt+D, S)
- Timeline Top Layout (Alt+D, T)
- "all tabs at bottom" layout (build 302, name unknown [I])
- slots 0–9

The VP16 option "Display timeline at bottom of main window" was **removed** in 23 [C].

### 4.3 Docking model

**Both looks**
- Dock tabs at the **bottom** of each group by default. The preference "Position tabs at top of docked windows" defaults to off [C].
- Drag a tab or title to move it. Dropping on a group's centre makes it a tab. Dropping on an edge splits. Dropping outside floats it.
- **Ctrl while dragging** inverts "Allow floating windows to dock" [C].
- Closing a window only hides it; reopen it from View › Window [C].
- Layouts persist as JSON in `%APPDATA%\Milan Pro\layouts\*.json` [I]. VEGAS uses `.VegasWindowLayout` [C].

**Preferences › Display defaults that shape docking** [L: VP15 local; same options in the VP16 manual p. 548–549]:

| Checkbox | VP16 default |
|---|---|
| Automatically hide docking area | off |
| Display timeline at bottom of main window | **on** (removed in VP23) |
| Position tabs at top of docked windows | off |
| Allow floating windows to dock (hold Ctrl to override) | **off** in the local install (it may have been user-changed). Off means a floating window docks only while Ctrl is held. |

**VP16**
- One docking area above the timeline (or below it via the VP14 layout), plus floating windows [C].
- F11 minimises or restores the dock area.
- Ctrl+F11 maximises the timeline.
- Shift+F11 collapses the track list.

**VP23**
- Four docking areas around the timeline: top, bottom, left, right [C].
- An empty area collapses automatically and remembers its last size [C].
- A **grey outline** shows the drop target while dragging [C].

  | Shortcut | Action |
  |---|---|
  | F11 / Alt+` | All docks |
  | Alt+F11 | Top dock |
  | Ctrl+Alt+F11 | Bottom dock |
  | Ctrl+Shift+F11 | Right dock |
  | Ctrl+F11 | Collapse/expand track headers |
  | Ctrl+Alt+Shift+F11 | Maximise timeline |

  [C]

**egui implementation** (no new dependency). Build the frame from:
- `TopBottomPanel::top("title")`
- `TopBottomPanel::top("toolbar")`
- `TopBottomPanel::bottom("status")`
- `TopBottomPanel::top("dock_top").resizable(true)`. VP23 adds `TopBottomPanel::bottom("dock_bottom")`, `SidePanel::left/right("dock_l/r")` (hidden when empty).
- `CentralPanel` for the timeline.

Inside a dock area, split columns with nested resizable `SidePanel`s and render each group as a manual tab bar plus content. Paint the splitter gap yourself: set panel `Frame` margins to 0 and fill the gap with `frame`. Free-form drag-and-drop docking can come later (or use `egui_dock` then). Get the static default layouts pixel-right first.

### 4.4 Dock tab style

| Property | VEGAS 16 | VEGAS 23 |
|---|---|---|
| Strip height | 17 px (16 + edges) | 29 px |
| Strip background | #373737 | #141414 |
| Tab shape | Rectangle; label padding 8 px left / 10 px right; 1 px #2D2D2D separator | No box; ≈20 px padding each side (40–60 px between labels) |
| Inactive | bg #373737, text #DCDCDC regular | text #787878 regular |
| Active | bg #464646 (merges with the window above), text #DCDCDC, followed by **□** (maximise in dock, 9×9 outline square) and **×** (close, 8×8 cross), each in a 13 px cell | text #F0F0F0 **bold** plus a **1 px #F0F0F0 line** across the label width on the content-facing edge (top edge when tabs are at the bottom). No close button visible. |
| Hover | text brightens to #FFFFFF [I] | text #C8C8C8 [I] |
| Overflow | Last visible label fades over 16 px; ◂ ▸ buttons, 13×16 each, at the right end | Truncated label ("H…") plus ◂ ▸ arrows at the right end |
| Single-window group | Strip still shown, background #202020 | Strip shown |
| Interaction | Click activates. Double-click toggles maximise within the dock [I]. Drag undocks. Middle-click does nothing. | same |

### 4.5 Splitters
- VP16: 7 px #232323. The main dock/timeline splitter is 7 px plus 1 px edges.
- VP23: 5 px #141414.
- Cursor: `ResizeHorizontal`/`ResizeVertical`. No grip glyph is drawn.
- Double-clicking the track-list/timeline splitter toggles between full width and colour strips only [C].

### 4.6 Window
- Default restored window: 1680×840 at (80,80) [L: VP14 default layout restored rect].
- Minimum size: 800×600 [I].
- **VP23 only:** `ViewportBuilder::with_decorations(false)`. Use `ViewportCommand::StartDrag` on title-bar drag, double-click to maximise, and draw the three caption buttons. Each is 46×32 with a hover fill (#2D2D2D; close hover #C42B1C). Glyphs are 10 px: ─, ▢ (or ⧉ when maximised), ✕.
- Switching looks at runtime sends `ViewportCommand::Decorations(bool)`.

---

## 5. Components

Coordinates are relative to each component's own rect unless stated. "Pitch" is the distance between button origins.

### 5.1 Menu bar
- 7 menus: **File, Edit, View, Insert, Tools, Options, Help** [C][L]. Menu tree in §6.
- **VP16**: a separate 20 px white row under the OS title bar. Labels are 12 px black with mnemonic underlines shown when Alt is held.
  - Hovered label: fill #E5F3FF with a 1 px #CCE8FF border.
  - Open label: #CCE8FF [I, Win10 native].
  - Popups: native-light (`menu_popup_bg`).
- **VP23**: inside the 32 px title bar, starting right of the 20×20 logo (x 5–24) at x≈32.
  - Hovered or open label: #3C3C3C fill [I].
  - Popups: dark, hover fill #3C8CDC.
- Popup anatomy for both:
  - 2 px inner margin.
  - Items 22 px (16) or 24 px (23) tall.
  - Separators: 1 px line with 4 px margins, colour `text_dim` @ 40%.
  - Check mark ✓ or radio ● in a 28 px left gutter.
  - Shortcut right-aligned, `text_dim` colour in VP23 and black in VP16.
  - Submenu ▸ at the right.
  - Disabled items greyed.
- Keyboard:
  - Alt or F10 focuses the menu bar.
  - Alt+letter opens a menu.
  - Arrows navigate; Enter activates; Esc closes.
- egui: use `egui::menu::bar` with a custom `Frame`. Implement shortcuts as `ui.add(egui::Button::new(label).shortcut_text(sc))`. Check items use `ui.checkbox`-style rows or a "✓" prefix in the gutter.

### 5.2 Main toolbar
- Height:
  - VP16: 31 px body plus a 2 px bevel above and a 1 px #333333 line below.
  - VP23: 35 px with 1 px #141414 lines.
- Buttons:
  - 22×22 hit area with a 16×16 glyph centred.
  - 24 px pitch (VP16) or ≈26 px pitch (VP23).
  - First button starts at x=6.
  - Split buttons (Undo, Redo) are icon + ▾: a 6 px-wide arrow region, 39 px total [L].
- States:
  - hover = `button_hover_bg` rounded square;
  - pressed = `button_active_bg`;
  - disabled glyph = `icon_disabled`.
- Tooltips show the name plus the shortcut, e.g. "Save (Ctrl+S)". The status bar shows the command's description while hovering [C].
- Default buttons, left to right:

| # | VEGAS 16 | VEGAS 23 | Action |
|---|---|---|---|
| 1 | New Empty Project | New Empty Project | Ctrl+Shift+N |
| 2 | Open | Open | Ctrl+O |
| 3 | Save | Save | Ctrl+S |
| 4 | Render As | Render | File › Render |
| 5 | Properties | Properties | Alt+Enter |
| 6 | Cut | Cut | Ctrl+X (disabled with no selection) |
| 7 | Copy | Copy | Ctrl+C |
| 8 | Paste | Paste | Ctrl+V |
| 9 | Undo + ▾ history | Undo + ▾ | Ctrl+Z. ▾ lists undo history; clicking an entry undoes back to it. |
| 10 | Redo + ▾ history | Redo + ▾ | Ctrl+Shift+Z |
| 11 | Interactive Tutorials | (Hub Login: omit in Milan Pro) | VP16: opens Help |
| 12 | What's This? Help | (none) | Shift+F1: cursor becomes "?" until the next click |

- No group separators in the default set [M][L].
- Customisable via Options › Customize Toolbar: Add, Remove, Move up/down, Separator, Reset [C]. Built in R15.

### 5.3 Docked windows: common chrome
- **VP16**:
  - 2 px top bevel;
  - **28 px toolbar** (#464646);
  - 1 px #393939 divider;
  - content #2D2D2D.

  Toolbar buttons are 22×22 on a 24 px pitch.
- **VP23**: toolbar ≈30 px #282828, no bevel, content #282828.
- Right-click on the tab strip: Undock, Close [I].

### 5.4 Project Media
- **Toolbar, left to right** [L][M]. Icon + ▾ buttons share one hover box.
  1. Remove All Unused Media
  2. Import Media… (icon + text "Import Media…" + ▾)
  3. Capture Video (disabled)
  4. Get Media from the Web
  5. Remove Selected Media
  6. Media Properties
  7. Media FX
  8. ▶ Play / ■ Stop (preview)
  9. Auto Preview (toggle, default on, active fill)
  10. Views ▾ (List / Details / Thumbnails)
  11. Search (magnifier)
- **Left tree pane** ≈170 px wide (resizable; 5 px splitter):
  - All Media (selected)
  - Media Bins / Media By Type ▸ (Audio, Video, Stills)
  - Tagged Media
  - Custom Bins
  - Smart Bins
  - Storyboard Bins ▸ Main Timeline (VP16 and later)

  Node glyph: small yellow folder (16) or outline folder (23). Expander: ⊞/⊟ (16) or ▸/▾ (23). Selected node: VP16 white box with dark text and a 1 px dark border; VP23 accent fill.
- **Right pane, thumbnail view (default):**
  - tiles ≈100×65 px thumbnail (16:9 letterboxed) with a 1 px white border (16);
  - filename centred below, 12 px, up to 2 lines;
  - grid gap 28 px horizontal, 30 px vertical;
  - audio files show a white page with a blue circular "♪" mark.
- **Details view:** columns Name, Use Count, Type, Length, Frame Rate, Frame Size, Path…; header row 20 px.
- **Interaction:**
  - Double-click opens the media in the Trimmer.
  - Drag to the timeline adds it.
  - Right-click: §6.9.
  - Del removes.
  - Ctrl/Shift multi-select.
  - Drop files from the OS to import.

### 5.5 Explorer
- **Toolbar:** ← Back, → Forward, ↑ Up, then the address combo (path ▾), Refresh, Delete, Add to Favorites, Start Preview ▶, Stop ■, Auto Preview (toggle), Views ▾.
- **Below the toolbar:** a folder tree on the left (≈180 px) and a file list on the right. VP16 used light native scrollbars here; reproduce that only for bug-fidelity, otherwise dark.
- **VP23 "Unified Explorer":** ← → ↑ · "Explorer" · ★ Favorites · "Media" ▾ · refresh · search field (accent outline when focused) · view-mode ▾. Thumbnail grid with names centred below. Files already used in the project carry a small accent dot at the thumbnail's top-right corner [C].
- **Interaction:** single-click with Auto Preview plays audio or shows the frame in the preview. Double-click opens in the Trimmer. Drag to the timeline adds.

### 5.6 Transitions / Video FX / Media Generators (plug-in choosers)

**VP16** [L]:
- **Header row (28 px):** magnifier glyph, then a "Search plug-ins" field (x 25–160, `input_bg`, placeholder `input_placeholder`), then the bold label "**Preset:**" (x≈175).
- **Left tree (≈166 px wide):**
  - Top node "All" (folder), then one item per plug-in, each with a small blue "fx" badge (12×10) before the name.
  - Selection is an accent fill.
  - A 5 px splitter follows.
- **Right pane, preset tiles:**
  - Each tile is 116×96: a 100×60 thumbnail plus a label below (12 px, centred, 2 lines).
  - **Selected tile has a white #FFFFFF background with grey text.**
  - Transitions show an animated A→B preview of the preset on hover. Video FX show the effect applied to a sample image. Generators show the generated image. Thumbnails are generated procedurally; never ship VEGAS images.
- **Bottom description strip (26 px, #464646):** "Name: description, OFX, 32-bit floating point, GPU Accelerated, Grouping: VEGAS\\Blurs, Version 1.0".

**VP23** [C][M]:
- **Category chip bar under the tab strip:** "All Plug-ins" (with magnifier), ★ Favorites, AI/ML, Creative, Color, Utility, Blur, Light, 360°, Third Party, Filter Packages, then a list/grid toggle at the far right.
  - Chips are 24 px tall with ≈12 px padding and 1 px #141414 dividers.
  - The active chip has an accent fill and white text.
- **Plug-in list rows:** 26 px each — "fx" glyph, then the name. Selected row has an accent fill; a ☆/★ favourite toggle appears at the right on hover or when set.
- **Preset grid:** ≈120×75 thumbnails with the caption below, 12 px gaps.
- **Description line** at the bottom.

**Interaction (both):**
- Drag a preset tile onto an event, a track header, the preview or the Video Output FX to apply it.
- Drag a transition onto an event edge or overlap.
- Double-click a generator preset to insert it at the cursor on the selected video track (default length 5 s [C]).

### 5.7 Trimmer
- **Toolbar:** media path combo "file.mp4 [C:\…\]" ▾ · Show Video in Preview (toggle) · Clear History · Remove from History · More.
- **Body:** video monitor area (black, letterboxed). Below it, an audio waveform strip (when the media has audio) and a **filmstrip/timeline strip** with the selected in/out range shaded #7A90C8 @ 50% and yellow in/out triangles (#FEF50C) above.
- **Transport** (centred row, 22 px) [L]:
  - VP16: ▶ ❚❚ ■ · Show in Preview (active) · ≡.
  - Full set behind the More menu: Loop, Play From Start, Play, Pause, Stop, Go to Start, Go to End, Previous Frame, Next Frame, Enable Timeline Overwrite, Add to Timeline from Cursor, Add to Timeline up to Cursor, Fit to Fill, Create Subclip, Set In, Set Out, Insert Marker, Insert Region, Save Markers/Regions.
- **Bottom row:** In ◤ "00:00:00;29" · Out ◥ "00:00:03;23" · Length ⊡ "00:00:02;24".
- **Keys:** I/O set in/out. A adds from the cursor. Shift+A adds up to the cursor. Tab cycles A/V/both streams. Ctrl+P shows in the preview [C].

### 5.8 Video Preview
- **Toolbar, left to right** [L][M][C]:
  1. Project Video Properties: gear.
  2. Preview on External Monitor: two stacked monitor outlines.
  3. Video Output FX: italic "fx".
  4. Split-Screen View: square with a diagonal fill, plus ▾ (Select Right Half, Select Left Half, Select All; FX Bypassed / Clipboard modes).
  5. **Preview Quality**, a text drop-down "Preview (Auto) ▾":
     - top level: **Draft, Preview, Good, Best**;
     - each with a submenu **Auto, Full, Half, Quarter**;
     - label format "Quality (Size)";
     - factory default **Preview (Auto)** [I]; VP16 screenshots show "Best (Full)" and local VP15 "Good (Full)".
     - Also in View › Preview Quality.
  6. **VP23 only:** Zoom "100 % ▾" (Fit, 25–400%).
  7. Overlays: # grid glyph plus ▾. Options: Grid, Safe Areas, Closed Captioning CC1–CC4, Isolate Red / Green / Blue / Alpha, Alpha as Grayscale.
  8. Copy Snapshot to Clipboard.
  9. Save Snapshot to File.
  10. **VP23 only:** HDR and 360 toggles (greyed when not applicable).
- **Video area:** surround `preview_bg`. The frame is letterboxed and aspect-correct, black #000000. Zoom (VP23): Ctrl+wheel; middle-drag pans [C].
- **Transport, centred under the frame (22 px row):**
  - VP16: ▶ ❚❚ ■ ≡.
  - VP23: ⟲ Loop · ▶ · ❚❚ · ■ · ⋯. VP23 also has a "spacebar Play/Pause" toggle reachable from ⋯.
  - Play shows the active fill while playing.
  - Full set behind More: Record, Loop, Play From Start, Play, Pause, Stop, Go to Start, Go to End, Previous Frame, Next Frame.
- **Status block (2 rows × 2 columns, 12 px):**

```
Project:  1920x1080x32; 29.970p          Frame:    0
Preview:  480x270x32; 29.970p            Display:  748x421x32; 29.970
```

  The "Display" value uses `warning_text` when real-time fps < project fps (16). In VP23 the Preview and Display values are always `warning_text`. VP23 also adds a small glyph before each label [M].
- **Right-click menu:**
  - Simulate Device Aspect Ratio
  - Scale Video to Fit Preview Window
  - Adjust Size and Quality for Optimal Playback
  - Display at Project Size
  - ─
  - Show Status Bar ✓
  - Show Transport Bar ✓
  - Make spacebar and F12 Play/Pause instead of Play/Stop

### 5.9 Timeline

#### 5.9.1 Time display (top of the track list)
- Size: 251×46 (16) or 250×45 (23). Background `time_display_bg`.
- Gripper at the left: a 2×3 dot grid (2×2 px dots, 2 px gaps) at x 2–7, centred vertically. It drags the time display out as a floating window.
- Timecode **right-aligned with 8 px padding**; font per §3. Shows the cursor position, or the playback position during playback.
- Default format: Time & Frames `hh:mm:ss;ff` (";" for drop-frame) [C].
- **Right-click menu:**
  - Time at Cursor ●
  - MIDI Timecode In
  - MIDI Timecode Out
  - MIDI Clock Out
  - ─
  - Time Format ▸ (ruler formats)
  - ─
  - Text Color ▸ (Default / Custom…)
  - Background Color ▸ (Default / Custom…)

#### 5.9.2 Marker bar, loop bar and ruler (band right of the time display; same height)

VP16 stack [L], 48 px total:

| Rows (px) | Element |
|---|---|
| 0–1 | bevel |
| 2–6 | **loop bar** (5 px) |
| 8–21 | **marker bar** (14 px flags) |
| 22–28 | free |
| 29–30 | separator #2E2E2E / #323232 |
| 31–46 | **ruler** (16 px) |
| 47 | 1 px #333333 |

VP23 stack [M], 45 px total: marker bar 14 px · loop bar 8 px · ruler 23 px.

**Loop bar**
- Shows the time selection / loop region.
- Each end has a **yellow right-triangle flag** (6×6, `loop_flag`) pointing inward.
- Between the flags:
  - VP16: a 3 px line in `loop_sel_line`;
  - VP23: a full-height bar in `loop_bar_off`, or `loop_bar_on` while Loop Playback (Q) is on.
- Selection length tag: a white box with black text "+5:00" (12 px, ≈32×12) at the right end while dragging and while a selection exists (16) [L].
- Interaction:
  - Drag in the empty loop bar to create a selection.
  - Drag a flag to resize.
  - Drag the middle to move.
  - Double-click to select the whole project.
  - Right-click: §6.10.

**Marker bar**
- **Marker:**
  - a 13×14 tag in `marker_flag` with the white marker number (1, 2…, 10 px bold) centred;
  - optional label text to the right (12 px, `text`);
  - a 1 px line of the same colour drawn down through all lanes.
- **Region:**
  - a start tag and an end tag in `region_flag`, both with the same number;
  - the start tag carries the label text to its right;
  - VP16 draws a 1 px `region_line` from the start tag.
- Keyboard 0–9 jumps to marker N [C].
- Drag a tag to move it. Double-click a tag to rename it.
- Alt-drag a region tag moves the region without changing its length [C].
- Right-click: §6.10.

**Ruler**
- Background `band_bg`.
- Major ticks: 1 px, 11 px tall, rising from the ruler bottom.
- Labels: 10 px, `ruler_text`, starting 4 px right of the major tick and baseline 3 px above the bottom, e.g. "00:00:10;00".
- Minor ticks: 1 px, 3 px tall, at the bottom.
- Spacing: choose the major interval from {1 frame, 2f, 5f, 10f, 15f, 1 s, 2 s, 5 s, 10 s, 15 s, 30 s, 1 min, 2, 5, 10, 15, 30 min, 1 h} so that major ticks are ≥ 90 px apart; minor ticks land 6–8 px apart [L: 6.6–7 px observed].
- Grid lines: at every major tick, draw a vertical `grid_line` through the lanes. Dashed 2 on / 2 off (16) or dotted 1 on / 2 off (23).
- Interaction:
  - Click sets the cursor.
  - Drag scrubs (16 always; VP23 drag does not scrub audio [C]).
  - Alt+click scrubs [C].
  - Wheel zooms.
  - Right-click: Ruler Format list plus Set Time at Cursor (and VP23 Set Project Tempo).

**Marker tool / corner button** above the vertical scrollbar: a 17×17 flag glyph on `scroll_thumb` (16) or `toolbar_bg` (23).

#### 5.9.3 Track list (headers)

**Column structure, left to right** (track list width 251 px in VP16, 250 px in VP23):

| Part | VEGAS 16 | VEGAS 23 |
|---|---|---|
| Outer margin | 3 px `frame` | 5 px `frame` |
| Colour strip | 24 px, solid track colour, full track height | 24 px, solid track colour |
| Divider | 1 px #242425 | 1 px #141414 |
| Header body | rest of the width, #2D2D2D (selected #738C9A) | #282828 (selected #505050) |
| Audio meter (audio tracks) | 21 px at the right edge of the body, `meter_bg`, labels 12/24/36/48 | 18 px, dB labels 9/18/27/36/45/54 |

**Colour strip content (both looks)**
- **More button:** a 14×14 hit area at (5, 4).
  - VP16 glyph: ≡, three 10×2 white bars 2 px apart.
  - VP23 glyph: ⋯, three 2×2 dots 2 px apart.
  - Opens the track menu of hidden buttons plus "Edit Visible Button Set…".
- **Track number:** white, at (4, 22) in VP16 or vertically centred in VP23.
- **Type badge** to the number's lower right, 8×8:
  - video: filmstrip (outline rectangle with two notches) or a 2×2 grid square in VP23;
  - audio: speaker (16) or ♪ (23);
  - adjustment track: "fx".
- **VP23 only:** when the track has OFX keyframe lanes, a "⌄⌄" double chevron sits at the bottom of the strip to expand them; it turns "⌃⌃" when expanded.

**Video track header** (default visible set)
- **Row 1** (top +4 to +18), right-aligned with a 6 px right padding, on a 24 px pitch:
  - VP16 (right to left): **Solo** (gold 15 px circle with dark "S!") · **Mute** (pink 15 px circle with white "M" and a diagonal slash) [L][M].
  - VP23, left to right: **Compositing Mode** (blue square with a small arrow and a white square) · **Track FX** "fx" (with a 4 px accent dot at its lower right when FX are present) · **M** · **S** [M].
  - Active state:
    - Mute in VP16: the badge is drawn fully saturated with a white ring.
    - Mute in VP23: the letter is drawn in `accent` on a 16×16 #3C8CDC @ 30% box [I].
    - Muted tracks dim their lane events to 50% opacity [C].
- **Row 2** (+28 to +40):
  - label "Level:" at x=6 of the body, value "100.0 %" at x=48;
  - a slider from x=100 to the body's right edge −8: 1 px groove `fader_track` (16) or #505050 (23);
  - thumb at the value position: 17×12 (16) or 12×12 (23) white rectangle with two dark 1×6 vertical bars ("▮▮" / "II").
  - Range 0–100%. Double-click resets to 100%. Ctrl-drag is fine control (×0.1). Wheel over it adjusts. Tooltip shows the value.
- **Rows 3+ (VP23, beta):** one 20 px sub-row per track OFX effect when expanded: name, then a "⌄⌄" expand toggle on the right. These become keyframe lanes on the timeline side.

**Audio track header**
- Row 1: Mute · Solo (same styles). In VP23 an automation/fx glyph with a blue dot and ▾ comes before them.
- Row 2: "Vol:" · "0.0 dB" · horizontal fader.
  - The fader maps −inf…+12 dB, with 0 dB at **70% of travel** [L].
  - Double-click resets to 0 dB.
- Row 3: "Pan:" · "Center" · a slider whose **thumb is a white downward triangle**, 20×12 (16) or 10×8 (23), centred at 0. The value shows "Center", "L 35%" or "R 35%" [I].
- Meter at the right edge (see the column table). In VP16 the peak readout ("-4.3") sits above the meter and clicking it resets it [M].

**Minimised track** (` key)
- 22 px tall, one row: ≡ · fader · Mute · Solo · meter [M].
- Track number hidden.

**Track heights**
- New tracks: VP16 video 64 / audio 68 px [L]; VP23 52 px [M]. Both include the separator.
- Minimum 22 px; maximum ≈ timeline height.
- Ctrl+Shift+↑/↓ steps all tracks.
- ` toggles minimise.
- Ctrl+` restores the default height.
- Drag a track's bottom edge (±3 px hit zone) to resize it [C].

**Selection**
- Click a header to select it. Ctrl+click toggles. Shift+click selects a range.
- Selected header body gets `track_header_sel_bg`. VP16 adds 1 px edges #434D53 / #55656E [L].
- The lane is **not** recoloured.
- Drag a header vertically to reorder tracks (a 2 px accent insertion line shows the target).
- Double-click the header's top-left (above Level) to rename. The scribble strip is shown only when enabled [C].

**Hidden buttons and Edit Visible Button Set** [L][C]
- Video, in order: Bypass Motion Blur · Track Motion · Track FX · Activate Automation Controls · Automation Settings · Mute · Solo · Compositing Mode · Make Compositing Parent · Make Compositing Child · ─ · Minimize/Maximize · Maximize.
- Audio, in order: Arm for Record · Invert Track Phase · Activate Automation Controls · Automation Settings · Track FX · ─ · Output Bus · Record Input · Mute · Solo · Min/Max · Maximize.
- "Edit Visible Button Set…" opens a checklist dialog with Show All / Hide All / OK / Cancel. The choice applies to all tracks of that type.
- Buttons that do not fit the header width are hidden automatically and appear in the More menu instead [C].

**Track groups (VP23)**
- A full-width 22 px header row: ▾ disclosure, group name, group M/S on the right.
- Children are indented 10 px [C][M].

**Empty area below the headers:** `empty_lane_bg` (23) or `frame` (16). Right-click: §6.8.

**Rate slider** (bottom of the track list, the same height as h-scroll + transport):
- Label "Rate: 0.00" at x=6 in **both** looks (re-measured live in VP15 on 2026-11-11: at rest it reads "Rate: 0.00" with the thumb centred; the earlier "1.00" for VP16 was wrong) [L].
- Slider from x=90 to x=240 with a square thumb.
- A small white ▲ under the 1.0 position.
- Range −4…+4 [I].
- Drag scrubs with a spring return to 0 on release (VP23 shows 0.00 at rest).
- Double-click the label to type a value [C].

#### 5.9.4 Track lanes and events

**Lanes**
- Fill: `panel_bg` (16) or #282828 (23).
- Separator below each lane: 2 px `track_separator`.
- Area below the last track: `frame` (16) or `empty_lane_bg` (23).
- Lanes are vertically aligned with their headers. Both scroll together via a shared `ScrollArea` offset.

**Event geometry (both looks)**
- Rect = [start_x, end_x] × [lane_top + 1, lane_bottom − 2], no corner radius.
- A 1 px darker line along the bottom edge (track × 0.73).

**VP16 event**
1. Body fill = track colour.
2. **Header bar** 22 px (event height < 40 → header shrinks to 16 px; < 24 → no header [I]):
   - fill = track colour;
   - name text at x+4, white 12 px, truncated with "…";
   - right-aligned white glyph buttons on an 18 px pitch, 16×16, in this order from left to right:
     1. Generated Media (generated events only)
     2. Event Pan/Crop (video only)
     3. Event FX ("fx", with a small badge when FX are applied)
     4. ≡ More
3. **Thumbnails** fill the area below the header:
   - Each thumbnail is height × (aspect) wide, placed from the event start.
   - Gaps between thumbnails show the track colour.
   - Default mode is head/tail plus fill, i.e. as many frames as fit, not stretched [M].
   - Stills show a single image at the start [M].
4. **Audio:** waveform centred in the body below the header in `lerp(track, white, 0.55)`. Stereo shows two channels stacked with a 1 px divider in track × 0.6.
5. **Fades:**
   - The fade-in curve runs from the bottom-left corner up to the top at the fade length.
   - The fade-out curve runs from the top at the fade start down to the bottom-right.
   - 1 px `fade_curve` (white when selected).
   - Crossfades draw both curves as an "X".
   - The area outside the curve (above the fade-in) is darkened with black @ 35% [I].
6. **Handles** (hover):
   - fade handles are small blue quarter-arcs (`fade_handle`) at the top corners;
   - trim zones are ±4 px at the left and right edges (cursor `ResizeHorizontal`, "]" or "[" glyph cursor [I]).
7. **Selected:** 2 px `event_selected_border` around the whole event, including the header.

**VP23 event**
1. Body:
   - video thumbnails across the full event height;
   - generated media = track colour with content preview;
   - audio = track × 0.5 with the waveform in track colour, one lane per channel separated by 1 px #141414.
2. **Name label** (View › Active Take Information, Ctrl+Shift+I, default on):
   - top-left box, height 20 px;
   - padding 4 px;
   - `event_label_bg` with a 1 px #141414 border;
   - text #F0F0F0 12 px, clipped to the event width.
3. **Event buttons** (View › Event Buttons, Ctrl+Shift+C, default on):
   - bottom-right, 20×20 chips on a 22 px pitch with a 2 px inset;
   - `event_chip_bg`, white glyphs, in order: [Generated Media] · Pan/Crop (video) · **fx** (accent dot when FX present) · **⋯**.
4. **Event headers mode** (View › Event Headers, Ctrl+Alt+M, default **off**):
   - full-width 20 px strip in track colour;
   - name on the left, then event length timecode right-aligned, then the Pan/Crop, fx and ⋯ icons.
5. **Fades:**
   - region outside the curve gets `fade_region_tint` (dark checker);
   - curve is a 2 px white line; in crossfades the fade-in curve is accent blue.
   - A boxed length tooltip (white box, dark text) shows while dragging.
   - Event Fade Lengths (Ctrl+Shift+T) labels the length.
6. **Handles:**
   - **Fade handles:** cyan 7×7 right triangles in the top-left and top-right corners.
   - **Opacity/gain handle:** a cyan 12×5 pill centred on the top edge. Dragging it down lowers opacity or gain and shows a "48.1%" tooltip box.
   - **Trim grips:** 6×14 "II" grips at mid-height on the left and right edges, shown on hover.
   - **Snap offset:** a white 5 px triangle at the bottom-left.
7. **Selected:** 2 px `event_selected_border`.
8. **Time-selection overlay:** events inside the selection render 15% whiter [M].

**Fade curve shapes** (both): Linear, Fast, Slow, Smooth (S-curve, default), Sharp, Hold. Right-click a fade to choose one.

**Event interaction (both looks)** [C]
- Click selects. Ctrl+click toggles. Shift+click selects a range on the same track.
- Drag moves the event, and the event can change tracks.
- Ctrl+drag (start) duplicates.
- Shift+drag disables snapping.
- Drag an edge to trim.
- Ctrl+drag an edge to time-stretch.
- Alt+drag inside: slip.
- Alt+drag an edge: slip-trim.
- Ctrl+Alt+drag in the middle: slide.
- Ctrl+Alt+drag an edge: trim the adjacent event.
- Drag a top corner to set the fade length. Drag one event onto another to create an automatic crossfade (when Automatic Crossfades is on).
- Right-drag then release: popup Move Here / Copy Here / Shuffle Events / Cancel.
- Double-click: select the event's time range (16). In VP23 it opens the event in the Trimmer when Preferences say so [I].
- **Empty-lane drag:**
  - VP16: left-drag makes a time selection.
  - VP23 (22+): left-drag makes a **rubber-band** event selection (dashed accent rectangle, 1 px) and right-drag makes a time selection [C].

#### 5.9.5 Envelopes
- **Line:** 1 px, or 2 px when hovered. Colour per type (§2).
- **Points:** 5×5 squares filled with the envelope colour, 1 px white outline. Selected point: filled white.
- **Hover tooltip:** a white box with black 11 px text, e.g. "-3.2 dB" or "Composite Level 75.0 %".
- **Interaction:**
  - Double-click the line to add a point.
  - Drag a point; Ctrl+drag is fine.
  - Right-click: §6.7.
  - With Lock Envelopes to Events on, points move with events.
- **Lane mapping:** volume 0 dB sits at ≈70% of the lane height from the bottom (top = +6 dB). Pan centre is the middle. Composite level 100% is the top.

#### 5.9.6 Cursor / playhead
**VP16** [L][M]
- **Line:** 1 px through the ruler and all lanes, drawn as the **inverse** of the underlying pixel. In egui, approximate with #060606 @ 86% over dark backgrounds and #F0F0F0 over backgrounds with luminance < 0.15 inside events. Alternatively draw a 1 px black line plus a 1 px `cursor_shadow` to its right.
- **Head:** a white flag in the ruler, 9 px wide × 12 px tall: a rectangle 9×8 plus a downward triangle tip of 4 px. Two 1×4 dark vertical ticks inside. It sits with its tip on the ruler bottom.
- **Blink:** the edit cursor blinks at ≈530 ms (visible/inverted) while the timeline has focus and playback is stopped [I].
- **During playback:** a separate playback cursor line moves. The edit cursor stays where playback started; Stop returns to it.

**VP23** [M]
- **Line:** 1 px `playhead_line` #EE5544 with 1 px `playhead_outline` on both sides (3 px total).
- **Head:** a #F0F0F0 pentagon with a 1 px #141414 outline. It is 9 px wide and 15 px tall (a 9×10 rectangle plus a 5 px pointed bottom), placed in the ruler. While dragging, it shows a small "II" glyph inside.
- **No blink.**
- Dragging the head moves the cursor freely without audio scrub [C].

**Both**
- Click on the ruler or on an empty lane moves the cursor. VP23 has an option to not move the cursor when clicking events [C].
- **Auto-scroll:** during playback, when the cursor reaches the right edge, the view pages forward (default "page" scroll) [I].

#### 5.9.7 Snapping indicators
- While dragging an event, edge, marker or cursor, a snap target within 8 px [I] pulls the item and draws a vertical `snap_line`:
  - **only across the target track** when snapping to an event edge;
  - **across the full timeline height** when snapping to the cursor, markers, regions, the selection or the grid.
- VP23 draws the line with a soft 4 px glow (same colour, 35% alpha). A dashed line means the snap is quantised to a frame [C].
- Snap targets come from Options: Enable Snapping (F8), Snap to Grid (Ctrl+F8), Snap to Markers (Shift+F8), Snap to All Events (Ctrl+Shift+F8). Quantize to Frames (Alt+F8) is always applied.

#### 5.9.8 Time selection
- A translucent `selection_overlay` band spans the ruler and all lanes between the selection start and end.
- The loop-bar flags mark its ends.
- Shift+click on the ruler extends the selection.
- I and O set the in and out points.
- The length boxes on the transport bar show start, end and length (§5.10).
- Backspace restores the previous selection (up to 5).

#### 5.9.9 Scrollbars and zoom
- **Horizontal scrollbar** under the lanes (16 px):
  - ◂ ▸ arrow buttons 16×16 and a thumb in `scroll_thumb`.
  - Dragging the **thumb's ends** zooms (2 px handles). Double-clicking the thumb zooms to the whole project [C].
  - At its right end: **+** and **−** zoom-in/out buttons (16×16) and a **magnifier** (Zoom Tool, toggle).
- **Vertical scrollbar** (17 px) on the right of the lanes, with **+ / −** track-height buttons at the bottom.
- **Keys and wheel:**
  - ↑/↓ zoom by small steps; Ctrl+↑/↓ by large steps or to the selection.
  - Wheel zooms around the mouse.
  - Ctrl+wheel scrolls vertically; Shift+wheel scrolls horizontally.
  - Middle-drag auto-scrolls.

### 5.10 Transport and editing toolbar (bottom of the timeline)
- **Row:** 30 px (16) or 32 px (23), `toolbar_bg`, from the timeline's left edge (x=258/260) to the right edge. The track-list column to its left holds the Rate slider.
- **Buttons:** 22×22 hit area, 16×16 glyph, **24 px pitch** (16) or ≈26 px (23).
- **Separators:**
  - VP16: 2 px dark vertical lines (#232323 + #555555 [I]), 16 px tall.
  - VP23: 1 px #505050.
- **Active-toggle fill:**
  - VP16: a 21×17 `button_active_bg` rectangle, square corners.
  - VP23: a 22×18 rounded rectangle, radius 2.

**Default buttons**, VP16 x centres measured at 1920 [M]:

| # | VP16 x | Button | VP16 | VP23 | Behaviour |
|---|---|---|---|---|---|
| 1 | 271 | Record | ✓ | ✓ | Ctrl+R; records to armed tracks |
| 2 | 295 | Loop Playback | ✓ | ✓ | Toggle, Q |
| 3 | 319 | Play From Start | ✓ | ✓ | Shift+Space |
| 4 | 343 | Play | ✓ | ✓ | Active fill while playing |
| 5 | 367 | Pause | ✓ | ✓ | Enter |
| 6 | 391 | Stop | ✓ | ✓ | Esc |
| 7 | 415 | Go to Start | ✓ | ✓ | Ctrl+Home |
| 8 | 439 | Go to End | ✓ | ✓ | Ctrl+End |
| 9 | 463 | Previous Frame | ✓ | ✓ | Alt+← |
| 10 | 487 | Next Frame | ✓ | ✓ | Alt+→ |
| ┆ | | separator | | | |
| 11 | 517 (+▾ 538) | Normal Edit Tool ▾ | ✓ | ✓ | Radio with 12–14. ▾ lists Normal, Shuffle, Slip, Slide, Time Stretch/Compress, Split Trim. Ctrl+D. |
| 12 | 558 | Envelope Edit Tool | ✓ | ✓ | |
| 13 | 582 | Selection Edit Tool | ✓ | ✓ | Right-click while dragging cycles Free → Vertical → Horizontal |
| 14 | 606 | Zoom Edit Tool | ✓ | ✓ | |
| ┆ | | separator | | | |
| 15 | 637 | Delete | ✓ | ✓ | Disabled with nothing selected |
| 16 | 660 | Trim | ✓ | ✓ | Ctrl+T |
| 17 | 685 | Trim Start | ✓ | ✓ | Alt+[ |
| 18 | 710 | Trim End | ✓ | ✓ | Alt+] |
| 19 | 734 | Split | ✓ | ✓ | S |
| 20 | — | Smart Split | ✗ | ✓ | Alt+S |
| 21 | 758 | Lock | ✓ | ✓ | Toggles lock on selected events |
| ┆ | | separator | | | |
| 22 | 789 | Insert Marker | ✓ | ✓ | M |
| 23 | 813 | Insert Region | ✓ | ✓ | R |
| ┆ | | separator | | | |
| 24 | 845 | Enable Snapping | ✓ (on) | ✓ (on) | F8 |
| 25 | 870 | Automatic Crossfades | ✓ (on) | ✓ (on) | Ctrl+Shift+X |
| 26 | 894 (+▾ 913) | Auto Ripple ▾ | ✓ (off) | ✓ (off) | Ctrl+L. ▾ lists: Affected Tracks / Affected Tracks, Bus Tracks, Markers, and Regions / All Tracks, Markers, and Regions. |
| 27 | 933 | Lock Envelopes to Events | ✓ (on) | ✓ (on) | |
| 28 | 957 | Ignore Event Grouping | ✓ (off) | ✓ (off) | Ctrl+Shift+U |
| ┆ | | separator | | | |
| 29 | — | Color Grading | ✗ | ✓ | Alt+G; toggles the panel |
| ┆ | | separator | | | |
| 30 | — | Open Parent Timeline | ✗ | ✓ | Alt+P (greyed when N/A) |
| 31 | — | Open Nested Timeline | ✗ | ✓ | Alt+N |
| 32 | — | Create Nested Timeline | ✗ | ✓ | Alt+C |

**Right end of the same row (both):**
- three **length boxes**, each a 14×14 icon followed by 11 px timecode, 8 px apart:
  1. **Cursor / Selection Start**: pin icon. Shows the cursor position when there is no selection.
  2. **Selection End**: icon is a small box with a yellow corner triangle.
  3. **Selection Length**: icon is a box with a yellow double arrow.
- Values are blank when there is no selection (16). Double-click to edit. "+10" and "-10" relative entries are allowed [C].
- In VP16 they sit at x≈1650–1900.

**Customisation:** Options › Customize Timeline Toolbar (R15).

### 5.11 Status bar
- **Height:** 17 px (16) or 20 px (23). Background `status_bg`. A 5 px #141414 gap sits above it in VP23.
- **Left:** progress or help text at x=6. It shows "Complete: 00:00:00" after an operation, the hovered command's description while hovering menus or toolbar buttons, and progress "Building peaks… 45%" with a 120×10 progress bar [I].
- **Right:** "Record Time (2 channels): 37:37:45", the free recording time on the record drive, right-aligned with a 6 px pad.
- **Toggle:** View › Status Bar.

### 5.12 Master Bus (right dock)
- **Width:** 125 px default.
- **Toolbar** (#464646 / #282828), 4 icons:
  1. Master Bus Properties: gear.
  2. Downmix Output: two arrows pointing inward. Yellow in 16, white in 23.
  3. Dim Output: speaker with a ▼. Blue ▼ in 16.
  4. Mixing Console: three vertical slider bars.
- **Strip header:**
  - row 1: □ "Master" (or "Surround Master");
  - row 2: fx · automation (circle with "R" in 16; envelope knob in 23) · Mute · Solo, in the look's style.
- **Body:**
  - "Front" label (surround only).
  - Peak-hold readouts per channel, e.g. "-20.2 -20.6", at the top. Click resets them.
  - **Two vertical meters**, 10 px wide each, in `meter_bg` wells, with a centred dB scale in **3 dB steps** (-3 … -57), 9 px labels, and 2 px ticks.
  - **Faders:** VP16 has white "88" double-square thumbs on the left and right of the meters, travelling the full height. VP23 has one stereo-linked thumb with a double "II".
  - Bottom: 🔒 link-channels glyphs at both ends, gain values "0.0 0.0", and a horizontal scrollbar (surround channel groups) in VP16.
- **Meter fill:**
  - VP16: solid `meter_green`, turning `meter_yellow` above −6 dB and `meter_red` at 0 dB [I].
  - VP23: a vertical gradient (green → yellow → red, stops per §2.2).
  - Peak hold: a 1 px line that falls back after 2 s [I].
  - Clip: the top 4 px turn red and latch until clicked.
- **Right-click:** Show Preview Fader, then the meter range list (−12 … −138 to 0 dB), Show Labels, Hold Peaks, Hold Valleys [L].

### 5.13 Color Grading panel (VP23 only; a normal dockable window)
- **Header:** "Event FX:" ▾ · filename.
- **Left tree:** LUTs, Color Wheels, RL Color Wheels, Utilities, Curves.
- **Main area:** 4 wheels: Lift, Gamma, Gain, Offset (or Shadows, Midtones, Highlights, Offset).
  - Each wheel is a hue ring (8 px wide) around a dark disc, with a white 6 px puck and a ↺ reset button.
  - R / G / B / Y numeric fields sit below each wheel.
  - An asterisk marks a changed parameter [C].
- Toggled by the transport button (#29) or Alt+G.

### 5.14 Welcome screen (VP23; modal)
- 900×600 [I].
- **Left vertical tab rail** (100 px), icon over label: New Project (active, accent fill), Open Project, Getting Started.
- **New Project page:**
  - Aspect tiles: Widescreen 16:9 (selected, accent border), Fullscreen 4:3, Portrait 9:16, Square 1:1, Scope 2.4:1.
  - Resolution ▾ "FullHD (1920 x 1080)".
  - Framerate ▾ "30p (29.970)", plus "Drop Frame (1/1.001)" ▾.
  - Buttons: [Create Project] (accent) [Advanced Settings…].
- **Footer:** ☑ Show Welcome Screen on Startup.
- **Reopen:** File › Welcome Screen.
- **VP16 has no equivalent.** Its Help › Welcome is a simple dialog; skip it.

### 5.15 Dialogs and floating tool windows

The two looks differ most here. Earlier versions of this spec left dialogs out.

#### 5.15.1 VP16: modal dialogs are native, light, OS-themed [L]
Project Properties, Preferences, the save prompt and similar modal dialogs in VP15/16 are **plain Win32 property sheets**. They are **not** dark-themed: OS light theme, Segoe UI 9 pt (12 px), black text, native tab controls, combo boxes, checkboxes and buttons. Measured from the local VP15 install via PrintWindow at 100 % logical pixels; Windows 11 colours, so a Win10 host shows #F0F0F0 everywhere.
- Colours (Win11 host): dialog body #F0F0F0, tab page #F9F9F9, edit fields #FFFFFF with a #BCBCBC bottom border, title bar #F3F3F3. On Win10, use #F0F0F0 for the body and #FFFFFF for the tab page.
- Buttons: 75×23 with 6 px gaps (OK, Cancel, Apply) at the bottom right. Apply starts disabled.
- In egui, draw these as light-themed `Window`s with a local light `Visuals` override, `corner_radius` 0 (Win10) or 4 (Win11), plus a "?" help caption button and ×.

**Project Properties** (File › Properties, Alt+Enter): window ≈427×696 including a 31 px title.
- Tabs: **Video, Audio, Ruler, Summary, Audio CD** [L]. VP23 adds **Advanced** (Master Display, 360 Output) [C: VP23 help project_properties].
- Video tab, top to bottom:
  1. Template ▾ "HD 1080-60i (1920x1080, 29.970 fps)", then Save-template (floppy), Delete-template (red ×) and Match Media Settings buttons.
  2. Width / Height edit boxes, with Field order ▾, Pixel aspect ratio ▾ and Output rotation ▾ to their right.
  3. Frame rate ▾.
  4. Stereoscopic 3D mode ▾, with greyed Swap Left/Right, a Crosstalk cancellation slider and "Include cancellation…".
  5. A separator line.
  6. A two-column label / combo grid: Pixel format, Compositing gamma, ACES version, ACES color space, View transform, Full-resolution rendering quality, Motion blur type, Deinterlace method, Resample mode.
  7. ☑ Adjust source media to better match project or render settings.
  8. Prerendered files folder: an edit box plus Browse…, then "Free storage space in selected folder: 82.6 Gigabytes".
  9. ☐ Start all new projects with these settings.
- VP23 adds **HDR Mode** ▾ and **Look modification transform** ▾ to the Video tab, and Resample mode offers Frame blend / Optical Flow / Disable resampling (default Disable) [C].

**Preferences** (Options › Preferences): window ≈433×670.
- **Two rows of tabs** [L]:
  - front row: General, Video, Preview Device, Audio, Audio Device, MIDI, VST Effects;
  - back row: Editing, Display, CD Settings, Sync, External Control & Automation.

  12 tabs in VP16 (the manual lists the same 12). VP23 adds **File I/O** and **Deprecated Features** [C], and Options › Export/Import Preferences (*.vegprefs).
- General tab: a scrolling checklist "General preferences:" (≈24 visible rows, 16 px each), then ☑ Recently used project list [spin], Temporary files folder plus Browse…, the free-space line and **Default All** [L].
- **Display tab** [L]:
  1. A "Track Colors" group: "Track:" ▾ (Color A…H) plus a 60×23 colour-swatch button.
  2. An "Envelope Colors" group: "Envelope Type:" ▾ plus a swatch.
  3. A "Snap Colors" group: "Snap Type:" ▾ plus a swatch.
  4. **Icon color strength** slider, default 75 %.
  5. (VP16+: **Button Highlight Color** swatch.)
  6. **Interface Type** radios: Dark (default), Medium, Light, White.
  7. The four docking checkboxes (§4.3).
  8. Default All.

  Group headings are text followed by a 2 px etched line to the right edge.

**Render As (VP16)** [C: VP16 manual pp. 468–470; visual style I]: a light native dialog containing:
- Output File: Folder ▾ + Browse…, Name + Browse…;
- a **Search render templates** box, a **Filters** ▾ (Show favorites only, Match project settings) and a Formats tree with templates beneath each format. Each template has a grey ☆ that turns gold when it is a favourite. A template description box sits below;
- **Customize Template…**;
- **Render Options** checkboxes: Render loop region only, Stretch video to fill output frame size (do not letterbox), Use project output rotation setting, Save project markers in media file, Enable multichannel mapping;
- Estimated file size, then Render / Cancel.

  Disabled while the timeline is empty.

#### 5.15.2 VP16: dark FX and keyframe windows [L]
The Event/Track/Output FX window, Pan/Crop and Track Motion are dark modeless tool windows with a **native OS title bar**.

**Video Output FX / Video Event FX window**: default ≈542×447 including the title.
- Body #464646.
- **Plug-in chain bar** at y+7, 442×21:
  - fill is a 1 px checker of #515151/#5D5D5D (a fine hatch);
  - bold 12 px #DCDCDC label "Video Output FX" (or "Video Event FX: <clip>") at x+6;
  - each plug-in in the chain appears as a button inside the bar.
- Right of the bar, three 24×21 **parameter-column toggles** (1, 2, 3 columns; glyphs are 1/2/3 stacks of 2 px bars). The active one is filled #101010 in b177, `button_active_bg` in VP16.
- Second row, right-aligned: **fx+** (Plug-in Chain…) and **fx×** (Remove Selected Plug-in), 24×21, greyed when not applicable.
- The OFX parameter area below uses the dark grid style of the next item.

**Track Motion window and Pan/Crop window (VP16)**, measured on Track Motion, ≈683×334 default:
1. Preset row, 26 px: "Preset:" label, then a **white native combo box** (18 px tall), a save floppy (blue) and a delete ✕ (red).
2. Toolbar row, 28 px #464646.
   - Track Motion: "Source Alpha ▾", then toggle buttons (rotate-around-centre, snap magnet, lock aspect, …).
   - Pan/Crop: Show Properties, Normal, Zoom, Enable Snapping, Lock Aspect Ratio, Size About Center, Move Freely, and the mask tools when Mask is checked.
3. Left **property grid**, ≈198 px wide + a 15 px **light native scrollbar** (#F0F0F0):
   - background #2D2D2D;
   - category header rows 14 px #464646 with a ⊟ box and bold label (Position, Orientation/Rotation, Keyframe Interpolation, Source, Workspace);
   - property rows 17 px: label at x+24 in #DCDCDC, value at x+120;
   - a 5 px splitter follows.
4. **Workspace**: #ECECEC (light grey) with a dotted grid and the frame rectangle drawn as a 1 px #505050 outline. Selection box: a dashed rectangle with 8 handles, a centre "F"/axes glyph and a dashed rotation circle. Zoom and pan buttons sit bottom-right.
5. **Keyframe controller** (bottom), ≈125 px:
   - a left row-label column (≈140 px) listing "Position" (selected: fill #828282) plus checkboxes "2D Shadow" and "2D Glow" (or "Mask" in Pan/Crop);
   - on the right, a 15 px ruler #373737 and 12 px lanes alternating #383838/#2B2B2B;
   - keyframes are small grey diamonds and the cursor is a white flag;
   - an #ECECEC/#C8C8C8 checker shows beyond the event end;
   - a scrollbar with + / −;
   - a bottom button row: Sync Cursor (lock), First / Previous / Next / Last keyframe, Create keyframe ◆+, Delete ◆×; cursor time box at the right.

#### 5.15.3 VP23: dark everywhere [M: official VP23 help images pan-crop-dialog.png and render-dlg.png]
Modal and tool windows use the dark palette. Floating windows keep a **native Windows title bar** (white in the help image), unlike the custom main-window title bar.

**Pan/Crop (inside "Video Event FX")**, 900×630 help image, 1:1:
- Chain bar 30 px #282828: a FX-chain glyph, then "**Event Pan/Crop:** <clip name>", with three column toggles at the right (active #3C8CDC/#4E88C1).
- Chain row 26 px: plug-ins appear as **blue pills** (#198CFE) joined by thin lines with ○ end nodes ("○[Pan/Crop]○"), with fx+ and fx× at the far right.
- Preset row 21 px: label "Preset:", a #141414 field, ▾, a save glyph and a red ✕.
- Vertical tool strip 40 px wide on the left: icons stacked on a 24 px pitch, active tools filled #3C8CDC.
- Property grid 200 px #282828: category headers 15 px #505050 with ⊟; rows 17 px.
- 5 px #141414 splitter.
- Workspace #ECECEC with a dotted grid.
- Keyframe controller: rows "Position" (#505050, selected) and "Mask" (#3C3C3C), ruler #333333, lanes #282828; bottom buttons with blue/white keyframe glyphs; time "00:00:00:00" at the right.

**Render dialog (VP23, "Render…")**, redesigned:
- Header line "Render: <project>".
- Left tree: Search field (red ✕ to clear), ★ Favorites, Recent, **Usecase** (YouTube, Social Media, Web & Streaming, Video VFX & Master, Image Compositing, DVD & Blu-ray, Audio Mix & Master, Podcast & Audio Streaming), then folders Video, Image Sequence, Audio.
- Template grid: 4 columns of icon + bold title + 2-line grey description.
- Template details box: name, codec name on the right, help "?", Notes / Video / Audio lines.
- **Customize…** button.
- Folder ▾ / Name fields with browse and project-location icons; Free disk space and Estimated size.
- Bottom row: "Render loop region only" toggle + gear ▾ (render options), **Render…** (accent) and Cancel.
- Top-right: Filter ▾ (Match project video/audio settings, Show all formats) and View ▾ (List/Details/Thumbnails, Sort by) [C: VP23 help render.htm].

---

## 6. Menu trees

**Notation**
- `&` marks the mnemonic letter. Show it underlined when Alt is held.
- `→` gives the shortcut, right-aligned.
- `(chk)` is a check item; `(radio)` is a radio item.
- `[16]` = only in the VEGAS 16 look. `[23]` = only in the VEGAS 23 look. Unmarked = both.
- `[pos I]` = the item exists, but its position in the v23 menu is inferred.

The VP16 order is exact: read from the VP15 exe resources, which VP16 shares. Items that make no sense for Milan Pro (Sony wireless, XDCAM disc, Share Online, HitFilm, VEGAS Pro Connect, debug items) are marked **(omit)**. Keep their slots out of the build entirely.

### 6.1 File
```
&New...                                   → Ctrl+N
New Empty Pro&ject                        → Ctrl+Shift+N
&Open...                                  → Ctrl+O
&Close                                    → Ctrl+F4
───
&Save                                     → Ctrl+S
Save &As...                               → [16] none  [23] Ctrl+Shift+S
Save Version                              → Ctrl+Alt+S        [23] [pos I]
───
&Render As...            [16]   /   &Render...   [23]
Real-Ti&me Render...
───
&Import ▸
    &Media...
    Media from &Project...
    &AAF...                                                   [16]
    P&remiere/After Effects (*.prproj)...
    &Final Cut Pro 7/DaVinci Resolve (*.xml)...
    Final Cut Pro &X (*.fcpxml)...
    &EDL Text File (*.txt)...
    &Broadcast Wave Format...
    &Closed Captioning...
E&xport ▸
    &VEGAS Project Archive (*.veg)...      (label: "Milan Pro Project Archive")
    Premiere/&After Effects (*.prproj)...
    &Final Cut Pro 7/DaVinci Resolve (*.xml)...
    Final Cut Pro &X (*.fcpxml)...
    &EDL Text File (*.txt)...
Capture &Video...                                             [16]
&Extract Audio from CD...
───
Welcome Screen                                                [23] [pos I]
&Properties...                            → Alt+Enter
───
&1 <recent project path>  … (count from Preferences › General, default 4)
───
E&xit                                     → Alt+F4
```

### 6.2 Edit
```
&Undo                                     → Ctrl+Z   (alt Alt+Backspace)
&Redo                                     → Ctrl+Shift+Z   (alt Ctrl+Y)
───
Cu&t                                      → Ctrl+X
&Copy                                     → Ctrl+C
&Paste                                    → Ctrl+V
P&aste Repeat...                          → Ctrl+B
Pa&ste Insert                             → Ctrl+Shift+V
Paste E&vent Attributes
Selectively Paste Event Attributes...
───
&Delete                                   → Delete
Tri&m                                     → Ctrl+T
Trim &Start                               → Alt+[
Trim &End                                 → Alt+]
Spl&it                                    → S
Smart Split                               → Alt+S            [23]
───
Na&vigate ▸
    &Go to Time...                        → Ctrl+G
    Go to Project &Start                  → Ctrl+Home
    Go to Project &End                    → Ctrl+End
    Go to &Time Selection/View Start      → Home
    Go to Time Selection/&View End        → End
    M&ove Left by Grid Marks              → Page Up
    Move Right by Grid &Marks             → Page Down
    Move Left by &Pixel                   → Left
    Move Right by P&ixel                  → Right
    Move Left by &Frame                   → Alt+Left
    Move Right by F&rame                  → Alt+Right
    ───
    Move Left by M&arker/Region           → Ctrl+Left
    Move Rig&ht by Marker/Region          → Ctrl+Right
    Time Sele&ct Right by Marker/Region   → Ctrl+Shift+Right
    ───
    Move &Left by Event Edit Points       → Ctrl+Alt+Left
    Move Right by Event E&dit Points      → Ctrl+Alt+Right
    Move Left &by Selected Event          → Ctrl+[
    Move Right b&y Selected Event         → Ctrl+]
    ───
    Move Cursor to Play&head              → Alt+Down
    Center View aro&und Cursor            → \
P&ost-Edit Ripple ▸
    Affected &Tracks                      → F
    Affected Tracks, &Bus Tracks, Markers, and Regions → Ctrl+F
    Affected Tracks, &Markers, and Regions
    &All Tracks, Markers, and Regions     → Ctrl+Shift+F
Se&lect ▸
    Select &All                           → Ctrl+A
    Select All After Cursor                                   [23] [pos I]
    Select Event Sta&rt                   → [
    Select Event &End                     → ]
    Enter Expanded Edit &Mode             → Num 5
    Exit E&dit Mode                       → Num 5 / Esc   (v23 label "Exit Expanded Edit Mode")
    ───
    &Loop Playback (chk)                  → Q
    ───
    &Zoom Selection                       → Ctrl+Up
    Set Selection to &View
    Set Selection to &Project
    &Select Loop Region                   → Shift+Q
    ───
    Set Selection S&tart                  → I
    Set Selection E&nd                    → O
    Set Selection to Prev&ious Selected Event → Ctrl+Shift+[
    Set Selection to Ne&xt Selected Event → Ctrl+Shift+]
    Rest&ore Previous Time Selection      → Backspace
&Editing Tool ▸  (radio)
    &Normal Edit Tool                     → Ctrl+D
    S&huffle Tool
    Sl&ip Tool
    Sli&de Tool
    Time Stretch/&Compress Tool
    Split &Trim Tool
    ───
    &Envelope
    &Selection
    &Zoom
    ───
    Ne&xt Tool                            → D
    &Previous Tool                        → Shift+D
───
S&witches ▸  (chk, on selected events)
    &Mute
    &Lock
    L&oop
    ───
    &Invert Phase
    &Normalize
    Auto Normalize                                            [23]
    ───
    Maintain &Aspect Ratio
    Reduce Interlace &Flicker
    ───
    Use P&roject Resample Mode (radio)
    Force Re&sample (radio)
    &Disable Resample (radio)
    Optical Flow (radio)                                      [23] [label I]
Ta&ke ▸
    &Rename Active
    &Choose Active...
    ───
    &Next Take                            → T
    &Previous Take                        → Shift+T
    ───
    Delete &Active
    &Delete...
&Group ▸
    Create &New                           → G
    ───
    &Remove From                          → U
    &Clear                                → Ctrl+U
    &Select All                           → Shift+G
    ───
    Cut &All
    Cop&y All
    &Delete All
&Stream ▸   (radio list built at runtime: Stream &1, Stream &2…)
C&hannels ▸ (radio)
    &Both
    &Left Only
    &Right Only
    &Combine
    &Swap
───
U&ndo All
Clear Edit Histor&y...
```

### 6.3 View
```
&Toolbar (chk)
Status &Bar (chk)
───
Window La&youts ▸
    &Organize Layouts...
    &Save Layout
    Save Layout &As...
    R&eload Selected Layout
    ───
    &Default Layout                       → Alt+D, D
    VEGAS Pro 14 Default Layout           → Alt+D, V          [16]
    Audio Mixing                          → Alt+D, A
    Color Correction                      → Alt+D, C
    Source Window Layout                  → Alt+D, S          [23]
    Timeline Top Layout                   → Alt+D, T          [23]
    ───
    &0 <EMPTY> … &9 <EMPTY>               → Alt+D, 0…9   (save: Ctrl+Alt+D, 0…9)
───
&Focus to Timeline                        → Alt+0
Window ▸  (chk = window visible)
    E&xplorer                             → Alt+1
    T&rimmer                              → Alt+2
    &Master Bus                           → Alt+3
    &Video Preview                        → Alt+4
    Pro&ject Media                        → Alt+5
    Edit &Details                         → [16] Alt+6   [23] Ctrl+Alt+4
    Project Notes                         → Alt+6             [23]
    &Transitions                          → Alt+7
    V&ideo FX                             → Alt+8
    Media &Generators                     → Alt+9
    Compositors
    &Plug-In Manager                      → Ctrl+Alt+1
    Video &Scopes                         → Ctrl+Alt+2
    S&urround Panner                      → Ctrl+Alt+3
    Mixing Console                        → [16] Ctrl+Alt+6   [23] Ctrl+Alt+5
    Device Exp&lorer                      → [16] Ctrl+Alt+7   [23] Ctrl+Alt+6
    Loudness Meters                       → [16] Ctrl+Alt+8   [23] Ctrl+Alt+7
    Dashboard                             → Ctrl+Alt+8        [23]
    Color Grading                         → Alt+G             [23]
───
Mixer Previe&w Fader (chk)
───
Audio Bus Tra&cks (chk)                   → B
Vide&o Bus Track (chk)                    → Ctrl+Shift+B
───
Eve&nt Media Markers ▸
    &Show Markers (chk)                   → Ctrl+Shift+K
    Show Marker &Labels (chk)
Active Take &Information (chk, on)        → Ctrl+Shift+I
Audio Event &Waveforms (chk, on)          → Ctrl+Shift+W
Event &Buttons (chk, on)                  → Ctrl+Shift+C
Event Fade &Lengths (chk, on)             → Ctrl+Shift+T
Video &Event Edge Edit Frames (chk, on)   → Ctrl+Shift+O
Event Headers (chk; [16] on, [23] off)    → Ctrl+Alt+M   [pos I]
Event Length (chk)                                            [23]
Show OFX Effect Envelopes (Beta) (chk, off)                   [23]
───
&Audio Envelopes ▸ (chk)
    Track &Mute
    Track &Volume
    Track &Pan
Video &Envelopes ▸ (chk)
    Track &Mute
    Track &Composite Level
    Track &Fade to Color
    Track &Keyframes
    ───
    Event &Velocity
    Transition &Progress
    ───
    Motion &Blur Amount
    Video &Supersampling
S&how Envelopes (chk)                     → Ctrl+Shift+E
───
Preview &Quality ▸ (radio)
    &Draft
    &Preview
    &Good
    &Best
───
Minimi&ze All Tracks                      → `
Rebuild Audio Pea&ks                      → Shift+F5
```

### 6.4 Insert
```
A&udio Envelopes ▸ (chk = exists)
    &Mute
    &Volume                               → V
    &Pan                                  → P
V&ideo Envelopes ▸
    Track &Mute
    Track &Composite Level
    Track &Fade to Color
    ───
    Event &Velocity
    Transition &Progress
───
&Audio Track                              → Ctrl+Q
&Video Track                              → Ctrl+Shift+Q
Adjustment Track                          → Ctrl+Shift+Alt+Q  [23]
───
Audio &Bus
Audio In&put Bus
Audio Assignable &FX...
───
&Empty Event
Adjustment Event                                              [23]
&Generated Media...
&Slideshow...                                                 [23]  (Slideshow Creator is v17+)
Insert Subtitles from Files...                                [23]
Insert Subtitles from Regions...                              [23]
───
Audio Tra&nsition...
───
&Time...
───
&Marker                                   → M
&Region                                   → R
───
Audio CD Trac&k Region                    → N
Audio CD Track Inde&x                     → Shift+N
───
&Command...                               → C
```

### 6.5 Tools
```
&Open in Audio Editor                     → Ctrl+E   (label uses the configured editor name)
Open &Copy in Audio Editor
Edit Source Pro&ject
&Audio ▸
    &Open in Audio Editor
    Open &Copy in Audio Editor
    Edit Source Pro&ject
    ───
    Audio &Event FX...
    Apply Non-Real-Time Event &FX...
    ───
    &Track FX...
    ───
    &Remove Assignable FX...
&Video ▸
    &Media FX...
    ───
    Video Event &Pan/Crop...
    Video Event &FX...
    ───
    Video Track F&X...
    &Track Motion...
    ───
    Vi&deo Output FX...
    Motion Tracking                       → Alt+M             [23]
───
&Render to New Track...                   → Ctrl+M
Preview on E&xternal Monitor (chk)        → Alt+Shift+4
&Selectively Prerender Video...           → Shift+M
Clean &Up Prerendered Video...
Build Dynamic RAM Previe&w                → Shift+B
Clear Dynamic RAM Preview                                     [23]
───
&Lay Out Audio CD from Events
&Burn Disc ▸
    &Track-at-Once Audio CD...
    &Disc-at-Once Audio CD...
    &DVD...
    &Blu-ray Disc...
Generate Loudness Log
Advanced Save...                                              [23]
Speech to Text...                                             [23]
Text to Speech...                         → Ctrl+Shift+P      [23]
───
Scrip&ting ▸
    Ru&n Script...
    Res&can Script Menu Folder
    ───
    <scripts from the Script Menu folder>
&Multicamera ▸
    Synchronize Audio to A&lign Events
    Synchronize Media &Timecode to Align Events
    Synchronize &Date/Time Stamp to Align Events
    &Create Multicamera Track
    E&xpand to Multiple Tracks...
    ───
    &Enable Multicamera Editing (chk)     → Ctrl+Shift+D
    Edit Multicamera &Audio with Video (chk)
    Show Full Frame in External &Monitor (chk)
───
&Clean Project Media
Rebuild Offline &Nested Audio
```

### 6.6 Options
```
&Quantize to Frames (chk, on)             → Alt+F8
───
&Enable Snapping (chk, on)                → F8
&Snap to Grid (chk)                       → Ctrl+F8
S&nap to Markers (chk, on)                → Shift+F8
Snap to All Even&ts (chk)                 → Ctrl+Shift+F8
───
Gr&id Spacing ▸ (radio)
    &Ruler Marks
    ───
    Mi&nutes
    &Seconds
    &Milliseconds
    &Frames
    ───
    Me&asures
    &Quarter Notes
    Q&uarter Note Triplets
    &8th Notes
    8th Note &Triplets
    &16th Notes
    16t&h Note Triplets
    &32nd Notes
    32nd Note Tr&iplets
Ruler &Format ▸ (radio)
    &Samples
    &Time
    Se&conds
    Time && Fra&mes   (default)
    Absol&ute Frames
    Measures && &Beats
    Feet and Frames &16mm (40 fpf)
    Feet and Frames &35mm (16 fpf)
    SMPTE Fi&lm Sync IVTC (23.976 fps, Video)
    SMPTE &Film Sync (24 fps, Film)
    SMPTE &EBU (25 fps, Video)
    SMPTE &Non-Drop (29.97 fps, Video)
    SMPTE &Drop (29.97 fps, Video)
    SM&PTE 30 (30 fps, Audio)
    &Audio CD Time
    ───
    Set T&ime at Cursor
    Set Project Tempo                                         [23]
───
&Automatic Crossfades (chk, on)           → Ctrl+Shift+X
Q&uickfade Audio Edits (chk, on)
Auto &Ripple (chk)                        → Ctrl+L
&Ripple ▸
    Auto &Ripple (chk)
    ───
    Affected &Tracks (radio)
    Affected Tracks, &Bus Tracks, Markers, and Regions (radio)
    Affected Tracks, &Markers, and Regions (radio)
    &All Tracks, Markers, and Regions (radio)
Lock En&velopes to Events (chk, on)
I&gnore Event Grouping (chk)              → Ctrl+Shift+U
Slip All Ta&kes (chk)
───
&Loop Playback (chk)                      → Q
Metr&onome (chk)
&Loudness Meters ▸
    &Reset Clip
    Reset Meterin&g Engine
    ───
    EB&U R 128 Mode (radio)
    &ATSC A 85 Mode (radio)
    ───
    True Peak &Blocking Filter (chk)
    &True Peak Range ▸
        -12 / -24 / -42 / -60 / -78 / -90 / -120 / -138 to 0 dB (radio)
    Loudness S&cale ▸
        &EBU +9 / E&BU +18 / ─ / &Absolute (-23 LUFS) (radio)
    ───
    Show True P&eak Meter (chk)
    &Show Labels (chk)
    Hold Pea&ks (chk)
    Hold &Valleys (chk)
───
&Bypass All Audio FX (chk)
───
&Mute All Audio (chk)
Mute All Vi&deo (chk)
───
Time&code ▸
    &Generate MIDI Timecode               → F7
    Generate &MIDI Clock                  → Shift+F7
    &Trigger from MIDI Timecode           → Ctrl+F7
External Control (chk)
───
Customize &Toolbar...
Customize Timeline Toolbar...
Customize &Keyboard...
&Preferences...
Export Preferences...                                         [23]
Import Preferences...                                         [23]
```

### 6.7 Help
```
&Contents and Index                       → F1
What's &This?                             → Shift+F1
&Keyboard Shortcuts
&Show Me How...
Interactive Tutorial&s...                                     [16]
&Welcome...                                                   [16]
───
Check for News...       (Milan Pro: "Check for Updates...")
Download User Manual                                          [23] [label I]
───
About Milan Pro...
```

### 6.8 Context menus: tracks

**Video track header**
- &Rename
- ───
- Insert &Video Track
- D&uplicate Track
- &Delete Track
- ───
- Insert/Remove &Envelope ▸ (Mute, Composite Level, Fade to Color)
- ───
- &Switches ▸ (Mute, Solo)
- Fade &Colors ▸ (Top…, Bottom…)
- E&xpand Track Layers (chk)
- ───
- Set De&fault Track Properties…
- ───
- Track &Group ▸
  - Rename Track Group
  - ─
  - Group Selected Tracks
  - Ungroup Selected Tracks
  - Collapse
  - Expand
  - ─
  - Mute Track Group
  - Solo Track Group
- Trac&k Display Color ▸ (the 8 palette swatches with names, plus Custom…)
- **v23 adds:** Select All After Cursor [pos I].

**Audio track header**
- Hide
- ───
- &Rename
- ───
- &Insert Audio Track
- D&uplicate Track
- &Delete Track
- ───
- Cu&t, &Copy, &Paste, &Delete
- ───
- &Arm for Record
- I&nput ▸
- &Output ▸
- Au&tomation ▸
  - Show Automation Controls
  - ─
  - Off
  - Read
  - Write (Touch)
  - Write (Latch)
- &Pan Type ▸
  - Add Channels (0 dB Center)
  - Balance (0 dB Center)
  - Balance (-3 dB Center)
  - Balance (-6 dB Center)
  - Constant Power
  - Film
- ───
- Insert/Remove &Envelope ▸ (Mute, Volume, Pan)
- F&X Automation Envelopes…
- ───
- &Switches ▸ (Mute, Solo, Invert Phase)
- ───
- Set De&fault Track Properties…
- ───
- Track &Group ▸
- Trac&k Display Color ▸
- ───
- Use &Vertical Meters (chk)
- Show Output &Meters (chk)
- ───
- Channel Width ▸ (Narrow / Default / Wide)

**Empty track-list area**
- &Open…
- &Insert Audio Track
- Insert &Video Track
- ───
- &Preferences…

**Empty area of a video lane**
- &Paste
- ───
- Delete, Trim, Trim Start, Trim End, Split
- ───
- Insert &Video Track
- D&uplicate Track
- &Delete Track
- ───
- Insert/Remove Envelope ▸
- ───
- Fade Colors ▸
- **v23:** Insert Slideshow

**Empty area of an audio lane**
- Paste
- ───
- Delete, Trim, Trim Start, Trim End, Split
- ───
- Insert &Audio Track
- Duplicate Track
- Delete Track
- ───
- Insert/Remove Envelope ▸
- FX Automation Envelopes
- ───
- Use Vertical Meters
- Show Output Meters

**Inside a time selection on a lane**
- &Insert Empty Event
- &Zoom Selection

### 6.9 Context menus: events and media

**Video event**
- Open in Tri&mmer
- Open Parent Media in Tri&mmer
- Select &in Project Media List
- Edit Source Pro&ject
- ───
- Media &FX…
- ───
- Video Event Pan/C&rop…
- Video &Event FX…
- Red E&ye Reduction…
- ───
- Cu&t
- &Copy
- Paste E&vent Attributes
- Selectively Paste Event Attributes…
- ───
- &Delete
- Tri&m
- Trim &Start
- Trim &End
- Spl&it
- ───
- Create Su&bclip
- Rever&se
- ───
- Select Events to E&nd
- Insert/Remove &Envelope ▸
  - Velocity
  - Freeze Frame at Cursor
  - Transition Progress
- ───
- Switc&hes ▸
- T&ake ▸
- &Group ▸
- &Stream ▸
- &Synchronize ▸ (By Moving, By Slipping)
- Create Sync &Link with Selected Events
- Remove Sync Link from Selected Events
- ───
- &Properties…
- **v23 adds** [pos I]:
  - Motion Tracking (Alt+M)
  - Add Missing Stream for Selected Event
  - Open Nested Timeline (nested events only)
  - Beat Detection
  - Switches ▸ Auto Normalize / Optical Flow

**Audio event:** the same structure, with these differences:
- The top block is: Open in Trimmer, Open Parent Media in Trimmer, Open in Audio Editor, Open Copy in Audio Editor, Edit Source Project, Select in Project Media List.
- Then Audio Event FX… and Apply Non-Real-Time Event FX…
- "Reverse" has mnemonic &R.
- Adds Channe&ls ▸ (Both, Left Only, Right Only, Combine, Swap).
- No Pan/Crop or Red Eye.

**Event More (≡ / ⋯) button**
- Video FX…
- Pan/Crop…
- Generated Media… (generated only)
- Freeze Frame at Cursor
- Playback Rate…
- ───
- Hide/Show Trim Handles (23)
- Detect Scenes and Split (23)
- ───
- Edit Visible Button Set…

[I, partially C]

**Right-drag of an event, on release**
- &Move Here
- &Copy Here
- &Shuffle Events
- ───
- Cance&l

**Right-drag from Explorer or Project Media onto the timeline**
- A&dd Across Time
- &Add Across Tracks
- Add as &Takes
- ───
- &Video Only ▸
- A&udio Only ▸
- ───
- Ope&n Project
- ───
- &Cancel

**Crossfade or overlap**
- Transition ▸ (installed transitions)
- Fade Type ▸ (Linear, Fast, Slow, Smooth, Sharp)

**Fade handle**
- Fade In ▸
  - Fade in from 6.0 dB
  - Fade in from 0 dB
  - Fade in from -Inf dB
  - ─
  - Linear, Smooth, Fast, Slow, Sharp
  - Reset ASR
- Fade Out ▸ (mirror)

**Envelope line**
- Add &Point
- ───
- &Linear Fade
- &Fast Fade
- &Slow Fade
- S&mooth Fade
- S&harp Fade
- H&old
- ───
- Fl&ip All Points
- Thi&n All Points
- &Reset All
- ───
- &Properties

**Envelope point**
- Set to Ma&ximum
- (2 preset values above default)
- Set to D&efault
- (4 preset values below)
- Set to M&inimum
- ───
- Set &To…
- ───
- &Delete
- ───
- curve items as on the envelope line
- ───
- Properties

**Project Media item**
- Open in &Trimmer
- Open Parent Media in &Trimmer
- Add as CD Trac&k
- Add at Project &Frame Rate
- Se&lect Timeline Events
- Match Pr&oject Video Settings
- Edit Source Pro&ject
- Explore &Containing Folder
- Create Video Pro&xy
- ───
- Remove from Current &Bin
- &Remove from Project
- Remove from Project and &Delete File(s)
- ───
- &Search Media Bins…
- ───
- Re&name
- R&eplace…
- Refres&h
- ───
- Media &FX…
- Rotate 90° Co&unterclockwise
- Rotate 90° Clock&wise
- ───
- Lay Out Tracks Using Media &Timecode
- Lay Out Tracks Using Media &Date/Time Stamp
- ───
- V&iew ▸ (column checklist; details view only)
- &Auto-Size Columns (details view only)
- ───
- File Format Pr&operties…
- &Properties…
- **v23 adds:**
  - Tempo Detection
  - Detect Scenes and Add to Timeline
  - Detect Scenes and Add to Timeline from Cursor
  - Detect Scenes and Create Subclips

**Project Media list background**
- &List (radio)
- &Details (radio)
- &Thumbnail (radio)
- ───
- &Media Tags (chk)

**Bin tree**
- Create &New Bin
- &Save as Bin
- Save as S&mart Bin
- ───
- &Delete
- &Rename
- &Edit Search
- ───
- &Search Media Bins…

### 6.10 Context menus: marker bar, ruler and time display

**Empty marker bar**
- &Loop Playback (chk)
- ───
- Set Selection to &View
- Set Selection to &Project
- Set Selection to &All
- &Select Loop Region
- ───
- &Markers/Regions ▸
  - Insert Marker
  - Insert Region
  - ─
  - Delete All
  - Delete All in Selection
- ───
- &Quantize to Frames (chk)
- ───
- &Enable Snapping (chk)
- &Snap to Grid (chk)
- S&nap to Markers (chk)
- Snap to All Even&ts (chk)
- ───
- Gr&id Spacing ▸
- ───
- Selectively Pre&render Video…
- &Clean Up Prerendered Video…

**Marker tag**
- &Go To
- &Rename
- ───
- &Delete

**Region start tag**
- &Go To
- Go to &End
- &Select Region
- &Rename
- ───
- &Create Subclip
- ───
- &Delete

**Region end tag**
- &Go To
- Go to S&tart
- &Select Region
- ───
- &Create Subclip
- ───
- &Delete

**Loop bar**
- Insert &Region
- ───
- &Loop Playback
- ───
- &Zoom Selection
- Set Selection to &View
- Set Selection to &Project
- &Select Loop Region
- ───
- Render to New &Track…
- Selectively Prerender V&ideo…

**Time ruler**
- The Ruler Format radio list
- ───
- Set T&ime at Cursor
- Set Project Tempo [23]

**Time display:** see §5.9.1.

---

## 7. Keyboard shortcuts (defaults)

The v23 column is authoritative; the v16 column lists only differences. Implement a `Keymap` with two default tables selected by look. Keys are rebindable per look in Options › Customize Keyboard (R15).

| Area | Command | v23 | v16 if different |
|---|---|---|---|
| File | New / New skipping Properties | Ctrl+N / Ctrl+Shift+N | |
| | Open / Close / Save | Ctrl+O / Ctrl+F4 / Ctrl+S | |
| | Save As | Ctrl+Shift+S | none |
| | Save Version | Ctrl+Alt+S | n/a |
| | Properties | Alt+Enter | |
| | Exit | Alt+F4 | |
| Edit | Undo | Ctrl+Z, Alt+Backspace | |
| | Redo | Ctrl+Shift+Z, Ctrl+Y | |
| | Cut / Copy / Paste | Ctrl+X / Ctrl+C / Ctrl+V (alts Shift+Del, Ctrl+Ins, Shift+Ins) | |
| | Paste Insert / Paste Repeat | Ctrl+Shift+V / Ctrl+B | |
| | Delete | Del | |
| | Trim / Trim Start / Trim End | Ctrl+T / Alt+[ / Alt+] | |
| | Split / Smart Split | S / Alt+S | S / n/a |
| | Rename event | F2 | n/a |
| | Select all / none | Ctrl+A / Ctrl+Shift+A | |
| | Normal tool / next / previous | Ctrl+D / D / Shift+D | |
| | Group / Remove from group / Clear / Select group | G / U / Ctrl+U / Shift+G | |
| | Ignore grouping | Ctrl+Shift+U | |
| | Next / previous take | T / Shift+T | |
| | Post-edit ripple: affected / +bus, markers / all | F / Ctrl+F / Ctrl+Shift+F | |
| | Auto Ripple | Ctrl+L | |
| | Automatic crossfades | Ctrl+Shift+X | |
| | Nested: create / open / parent | Alt+C / Alt+N / Alt+P | n/a |
| | Render to New Track | Ctrl+M | |
| | Open in audio editor | Ctrl+E | |
| | Multicamera editing | Ctrl+Shift+D | |
| Playback | Play / Stop | Space = **Play/Pause** (release notes; the help's shortcut table still says Play/Stop) | Space = **Play/Stop** (stop returns to start). VP16 toggle: Preferences › General "Make spacebar and F12 Play/Pause instead of Play/Stop", also in the Video Preview right-click menu. |
| | Play from start | Shift+Space, Shift+F12 | |
| | Play from any window | Ctrl+Space, F12 | |
| | Play/Pause | Enter, Ctrl+F12 | |
| | Stop | Esc | |
| | Loop | Q, Ctrl+Shift+L | |
| | Record / arm | Ctrl+R / Ctrl+Alt+R | |
| | Scrub J/K/L | J reverse (repeat = faster), K pause, L forward; hold K + J/L = shuttle | |
| | Preview at cursor | Num 0 | |
| | Dim master | Ctrl+Shift+F12 | |
| | Prerender / RAM preview | Shift+M / Shift+B | |
| | External monitor | Alt+Shift+4 | |
| Cursor | Project start / end | Ctrl+Home (or W) / Ctrl+End | |
| | Selection or view start / end | Home / End | |
| | By frame | Alt+← / Alt+→ | |
| | By pixel | ← / → | |
| | By grid | PgUp / PgDn | |
| | By marker | Ctrl+← / Ctrl+→ | |
| | By event edit points | Ctrl+Alt+← / → | |
| | To selected event edge | Ctrl+[ / Ctrl+] | |
| | Go To | Ctrl+G | |
| | Marker N | 0–9 | |
| | Centre view | \ | |
| | Cursor to playhead | Alt+↓ | |
| | Jog | F3 / F9 | |
| Selection | In / Out | I / O | |
| | Select loop region | Shift+Q | |
| | Restore selection | Backspace | |
| | Extend | add Shift to any cursor move | |
| | Time-select prev/next selected event | Ctrl+Shift+[ / ] | |
| Event nudge | Move selected events 1 frame / 1 pixel | Num1/Num3 / Num4/Num6 | |
| | Up/down a track | Num8 / Num2 | |
| | Expanded edit (trim) mode | Num5 (Esc exits) | |
| | Pitch ± semitone / cent / octave | = − / Ctrl+= Ctrl+− / Shift+= Shift+− | |
| Markers | Marker / Region / Command | M / R / C | |
| | CD track / CD index | N / Shift+N | |
| Tracks | Insert audio / video | Ctrl+Q / Ctrl+Shift+Q | |
| | Insert adjustment track | Ctrl+Shift+Alt+Q | n/a |
| | Mute / exclusive mute | Z / Shift+Z | |
| | Solo / exclusive solo | X / Shift+X | |
| | Group mute / solo | Ctrl+Alt+Z / Ctrl+Alt+X | |
| | Level or volume ± (track list focused) | Ctrl+← / → | |
| | Pan or fade ± | Ctrl+Shift+← / → | |
| | Focus previous/next track | Alt+Shift+↑ / ↓ | |
| | Track list ↔ timeline focus | Tab | |
| | Volume / pan envelope (insert, show, hide) | V / P (remove: Shift+V / Shift+P) | |
| | Cycle FX envelopes | E / Shift+E | |
| | Show envelopes | Ctrl+Shift+E | |
| Snapping | Enable / grid / markers / all events / quantize | F8 / Ctrl+F8 / Shift+F8 / Ctrl+Shift+F8 / Alt+F8 | |
| | Override while dragging | hold Shift | |
| Zoom | Horizontal small / large or to selection | ↑↓ / Ctrl+↑↓ | |
| | One frame per thumbnail | Alt+↑ | |
| | Waveform vertical zoom | Shift+↑ / ↓ | |
| | All tracks taller / shorter | Ctrl+Shift+↑ / ↓ | |
| | Minimise/restore track / default heights | ` / Ctrl+` | |
| View | Audio bus tracks / video bus track | B / Ctrl+Shift+B | |
| | Media markers / take info / waveforms / event buttons / fade lengths / edge frames | Ctrl+Shift+K / I / W / C / T / O | |
| | Event headers | Ctrl+Alt+M | n/a (use View menu) |
| | Next / previous window | F6 or Ctrl+Tab / Shift+F6 or Ctrl+Shift+Tab | |
| Docking | All docks | F11, Alt+` | F11 (minimise/restore dock area) |
| | Top / bottom / right dock | Alt+F11 / Ctrl+Alt+F11 / Ctrl+Shift+F11 | n/a |
| | Track headers collapse | Ctrl+F11 | Shift+F11 (minimise track list) |
| | Maximise timeline | Ctrl+Alt+Shift+F11 | Ctrl+F11 |
| Layouts | Default / Audio / Color / Source / Timeline Top / user N | Alt+D then D / A / C / S / T / 0–9 | VP14 layout = Alt+D, V |
| | Save user layout N | Ctrl+Alt+D then 0–9 | |
| Windows | Timeline | Alt+0 | |
| | Explorer, Trimmer, Master Bus, Video Preview, Project Media | Alt+1 … Alt+5 | |
| | Alt+6 | Project Notes | Edit Details |
| | Transitions, Video FX, Media Generators | Alt+7, Alt+8, Alt+9 | |
| | Plug-In Manager, Video Scopes, Surround Panner | Ctrl+Alt+1, 2, 3 | |
| | Ctrl+Alt+4 | Edit Details | unassigned |
| | Ctrl+Alt+5 | Mixing Console | XDCAM Explorer (omit) |
| | Ctrl+Alt+6 | Device Explorer | Mixing Console |
| | Ctrl+Alt+7 | Loudness Meters | Device Explorer |
| | Ctrl+Alt+8 | Dashboard | Loudness Meters |
| | Color Grading / Motion Tracking / Text to Speech | Alt+G / Alt+M / Ctrl+Shift+P | n/a |
| Misc | Help / What's This / context menu | F1 / Shift+F1 / Shift+F10 | |
| | Rebuild peaks | Shift+F5 | |
| | MIDI timecode / clock / trigger | F7 / Shift+F7 / Ctrl+F7 | |
| | Fine slider adjust | Ctrl+drag | |
| Mouse wheel | Zoom (time) | wheel | |
| | Scroll vertical / horizontal | Ctrl+wheel / Shift+wheel | |
| | Move cursor in small steps (trim 1 px in edge mode; scrub rate during playback) | Ctrl+Shift+wheel | |
| | Same, by frames | Ctrl+Alt+Shift+wheel | |
| | Auto-scroll | middle-drag | |
| | Fader under the mouse | wheel over the fader (Ctrl = fine) | |

**Trimmer-window keys:** A add from cursor · Shift+A add up to cursor · Tab cycle streams · Ctrl+P show in preview · S save markers/regions.

**Mixing Console keys:** F2 rename · D/N/W strip width · F/I/H/M/S toggle regions.

Implementation note: the Keymap must resolve chorded keys (Alt+D, then X) with a 1.5 s chord timeout [I]. Number keys 0–9 jump to markers only when the timeline has focus.

---

## 8. Icon descriptions (draw originally as vector shapes; never copy bitmaps)

**Drawing rules**
- Draw every icon in a 16×16 box with `egui::Painter`: `line_segment`, `rect_stroke`, `circle_stroke`, `add(Shape::convex_polygon)`.
- Strokes are 1.25 px (VP23) or 1.5 px (VP16), with round caps.
- Colours:
  - **VP23**: monochrome `icon` with only the accents named below.
  - **VP16**: the coloured variants named below. The rest are white #F0F0F0 with 1 px #1E1E1E shading on the lower-right edges for a slightly raised look.

The shape and meaning are what must match. Exact proportions are ours.

### 8.1 Main toolbar

| Command | Shape | VP16 colour | VP23 |
|---|---|---|---|
| New | Page outline 10×13 with a 4 px folded top-right corner (diagonal + small triangle) | white page, grey fold | outline |
| Open | Folder: a tab on the top-left, body 13×9, front flap skewed open | tan #D8B45A body, darker #A88A3C flap | solid mono |
| Save | Floppy: 12×12 square with a cut top-right corner, a shutter rect at the top, a label rect at the bottom | blue #3C78C8 body, white label | solid with a hollow label |
| Render / Render As | Page or square with an arrow leaving through the top-right corner (diagonal + arrowhead) | white page, blue arrow #2E8BE6 | square outline, arrow breaking out |
| Properties | Gear: circle r=4 with 8 rectangular teeth, hollow centre r=1.5 | white | outline |
| Cut | Scissors: two loops (r=2.5) at the bottom, blades crossing to the top | red-pink loops #E05A6E, grey blades | outline |
| Copy | Two overlapping page outlines offset (3,3) | white | outline |
| Paste | Clipboard: board 11×13 with a clip tab at the top, a small page overlapping at the lower right | tan board, white page | outline |
| Undo | Counter-clockwise arc (≈270°) with an arrowhead at the left end; ▾ 5×3 to its right | white | outline |
| Redo | Mirror of Undo | white | outline |
| Interactive Tutorials (16) | Pointing hand with a yellow circle "?" badge at the lower right | white hand, yellow #F0C800 badge | — |
| What's This (16) | Arrow pointer with a yellow circle "?" badge | white arrow, yellow badge | — |

### 8.2 Transport and edit toolbar

| Command | Shape | VP16 colour | VP23 accent |
|---|---|---|---|
| Record | VP16: microphone (capsule 5×8 plus a U-shaped holder and stand). VP23: filled circle r=5. | mic head pink-red #E0505A | circle #FF7B5F |
| Loop Playback | Two curved arrows forming a rounded rectangle loop | white | — |
| Play From Start | Vertical bar 2×10 + right-pointing triangle | white | — |
| Play | Right-pointing triangle 9×10 | white | — |
| Pause | Two bars 3×10, 3 px gap | white | — |
| Stop | Square 9×9 | white | — |
| Go to Start | Bar + left triangle | white | — |
| Go to End | Right triangle + bar | white | — |
| Previous Frame | Small left triangle + two thin bars ("◀❚") | white | — |
| Next Frame | Two thin bars + small right triangle ("❚▶") | white | — |
| Normal Edit Tool | Arrow cursor (classic pointer) with a small "[ ]" bracket at the lower right; ▾ beside it | white | — |
| Envelope Edit Tool | Arrow cursor + a polyline with two small square nodes | white, blue line | — |
| Selection Edit Tool | Arrow cursor + dashed rectangle | white | — |
| Zoom Edit Tool | Magnifier: circle r=4 + 45° handle; small box inside (16) | white | — |
| Delete | Diagonal cross ✕ 9×9 | red #E04040 | white (disabled grey) |
| Trim | Event bar with two outward brackets | white | — |
| Trim Start | Left half red-hatched block + bar | red hatch #D04040 | white hatch |
| Trim End | Bar + right half red-hatched block | red hatch | white hatch |
| Split | Rounded bar broken in two by a vertical gap with a small cut line | blue halves #3C78C8 | outline halves |
| Smart Split (23) | Split glyph + a small sparkle (4-point star) at the top-right | — | white |
| Lock | Padlock: shackle arc + body 8×6 | white/grey | outline |
| Insert Marker | Flag on a pole: pole 1×12, flag triangle or rectangle 6×5 | yellow-olive #D8C040 | orange #F38E28 |
| Insert Region | Two flags facing each other, joined by a baseline | green #54A065 | orange + green #26CB5C |
| Enable Snapping | Horseshoe magnet (U, poles down) with tip bands | red body #D03030, white tips | white |
| Automatic Crossfades | Two diagonals crossing (an "X") inside a box | white with blue lines | white |
| Auto Ripple | Three stepped right-arrows offset diagonally; ▾ beside | white | — |
| Lock Envelopes to Events | Envelope polyline over an event bar + a tiny padlock | white | — |
| Ignore Event Grouping | Two overlapping boxes + a small ✕ | white | — |
| Color Grading (23) | Three dots in a triangle | — | red #DC5050, green #50C878, blue #3C8CDC |
| Open Parent / Open Nested / Create Nested (23) | Stacked rectangles with an arrow up / arrow down / "+" | — | white |
| Length-box cursor | Map-pin / flag-pin outline 8×12 | white | white |
| Length-box selection end | Small box with a yellow triangle in its lower-left corner | yellow #F0D000 | same |
| Length-box length | Box with a yellow ↔ arrow inside | yellow | same |

### 8.3 Track header, event and panel glyphs

| Glyph | Shape |
|---|---|
| More (16) | ≡: three horizontal bars 10×2 with 2 px gaps |
| More (23) | ⋯: three 2×2 squares or dots with 2 px gaps |
| Mute (16) | 15 px circle in `mute_badge`, white bold "M" with a 1 px diagonal slash |
| Solo (16) | 15 px circle in `solo_badge`, dark bold "S" with a small "!" |
| Mute / Solo (23) | Bold letters "M" / "S", 16 px; active = accent box behind |
| Track FX | Italic "fx" (draw two strokes: an "f" with a crossbar, an "x"); VP23 adds a 4 px accent dot at the lower right when FX are present |
| Compositing Mode (23) | 10×10 accent-blue square with a white 4×4 square and a tiny arrow at the lower right |
| Pan/Crop (event) | Crop corners: two interlocking L-brackets |
| Generated media (event) | Rectangle with a small gear or circle inside |
| Track type: video | Filmstrip: rectangle 8×6 with notches on the top and bottom edges (16); 2×2 grid square (23) |
| Track type: audio | Speaker: small square + triangle (16); ♪ (23) |
| Expand keyframes (23) | Two stacked chevrons ⌄⌄ (7 px wide, 3 px apart) |
| Fader thumb | Rectangle 17×12 (16) or 12×12 (23), white, with two dark 1×6 vertical bars |
| Pan thumb | Downward white triangle |
| Master Bus: Downmix | Two arrows pointing inward to a centre line |
| Master Bus: Dim | Speaker + small ▼ (blue in 16) |
| Master Bus: Mixing Console | Three vertical lines with small rectangular knobs at different heights |
| Preview: External monitor | Two monitor outlines stacked or offset |
| Preview: Split screen | Square with its left half filled diagonally |
| Preview: Overlays | "#" grid of 2 + 2 lines inside a square |
| Preview: Snapshot copy / save | Two pages / floppy (as in §8.1) |
| Project Media: Auto Preview | Arrow cursor + small green ▶ (green #3CC850) |
| Dock tab maximise / close (16) | 9×9 outline square / 8×8 ✕ |
| Title-bar logo (23) | Original Milan Pro monogram: a 20×20 rounded square `logo_bg` with a 1 px `logo_border` and a white "M". Do not reproduce the VEGAS "V". |

---

## 9. Prioritised implementation checklist

Each step gives a large "this looks like VEGAS" payoff for its effort. Do them in order.

1. **Theme tokens and look switch.**
   - Add `enum UiLook { Vegas16, Vegas23 }` and `Theme` with every token from §2.
   - Replace the constants at the top of `src/ui/mod.rs` with theme lookups.
   - Persist the look in settings.
   - Add a View or Options menu item "Interface: VEGAS 16 / VEGAS 23" and an Interface Type radio set in Preferences.
2. **Frame layout** (§4): title/menu/toolbar/dock/timeline/transport/status bands at exact heights, splitter widths and colours, track list 251/250 px, a dock area of ≈56% (16) or ≈50% (23) with three columns, Master Bus 125 px.
3. **Fonts** (§3): Segoe UI loaded at runtime, 12 px UI; the big time display at 24 px regular (16) or 22 px semibold (23).
4. **Track headers** (§5.9.3): colour strip with ≡/⋯ and number; M/S style per look; Level/Vol/Pan rows with the "II" thumb; selected-header colour; per-look track palette.
5. **Events** (§5.9.4): VP16 header bar with name and buttons, track-colour body, blue fades, #CEE500 outline. VP23 name label, chips, audio track × 0.5, white fades, cyan handles.
6. **Ruler, marker bar, loop bar, cursor** (§5.9.2, §5.9.6): exact tick sizes, label placement, yellow loop flags, marker/region tags, playhead style per look, grid lines through the lanes.
7. **Transport/edit toolbar** (§5.10) with the full default button set, separators, active-toggle fills, and length boxes on the right; Rate slider in the track-list column.
8. **Dock tab strips** (§4.4) at the bottom of each group, with per-look tab styling; static default tab sets.
9. **Main toolbar** (§5.2) and **status bar** (§5.11).
10. **Menu tree** (§6) with mnemonics and right-aligned shortcuts; **Keymap** (§7), including the per-look spacebar behaviour.
11. **Video Preview** toolbar, quality menu, centred transport and status block (§5.8); **Master Bus** meters and faders (§5.12).
12. **Context menus** (§6.8–6.10).
13. **Interaction details:** snapping indicators, Ctrl/Alt drag modifiers, rubber-band selection (23), track-height drag, envelopes.
14. **Plug-in chooser windows** (§5.6), Explorer, Trimmer, Project Media tree.
15. **VP23 extras:** custom title bar with caption buttons, four-side docking, Color Grading panel, Welcome screen, theme variants (White, then Medium and Light).
16. Customize Toolbar and Keyboard dialogs; saved window layouts.
17. **Dialogs** (§5.15): Project Properties and Preferences as light native-style sheets in the 16 look and dark in the 23 look; FX chain / Pan/Crop / Track Motion window chrome.

**Acceptance check:** render Milan Pro at 1920×1080 at 100% scaling and place it side by side with the reference screenshots (§10.2). Band heights and x-splits must be within ±2 px. Sampled flat colours must match the token hex exactly.

---

## 10. Sources

### 10.1 Documents and pages
- VEGAS Pro 23 online help (MAGIX), e.g. https://help.magix-hub.com/video/vegas/23/en/content/topics/1-before/whatsnew.htm. Topics used under `/2-window/`:
  - mainwindow, maintoolbar, tracklist, windowdockingarea, window_layouts, transportcontrols, trackwindow, ruler, welcomescreen, window_videofx, customizing_vegas.
  - `/5-preview/` videopreview and loopplayback.
  - `/7-edit/` events_intro, adjusting_eventslength, enablesnapping, selectingevents, markerbar, videotrack, smartsplittingevents, scenedetection, nested timelines, adjustmenttracks, quantize_frames.
  - `/9-audio/` audiotrack, autonormalize, autoducking.
  - `/12-preferences/` prefdisplay, prefdepracted, preferences_overview.
  - `/13-appendix/shortcutkeys.htm`.
- Vegas Pro 2026 help (Boris FX; UI-identical to 23): https://cdn.borisfx.com/borisfx/Documentation/vegas/2026/en/content/topics/2-window/mainwindow.htm (plus transportcontrols, tracklist, shortcutkeys).
- Official release history, VP15–2026: https://forum.borisfx.com/t/24284
- VP23 Pan/Crop and Render help: https://help.magix-hub.com/video/vegas/23/en/content/topics/7-edit/pan-crop-dialog.htm, …/10-export/render.htm (images under …/content/images/localized/)
- Spacebar default change (VP23 build 278 release notes): https://www.moviestudiozen.com/free-tutorials/vegas-pro/vegas-pro-23-update-history; option moved out of Preferences in VP22: https://www.vegascreativesoftware.info/us/forum/make-spacebar-and-f12-play-pause-instead-of-play-stop-missing-in-vp22--146966
- Forum threads:
  - https://forum.borisfx.com/t/21702 (themes)
  - https://forum.borisfx.com/t/23140 (1:1 2026 screenshot)
  - https://forum.borisfx.com/t/24626 (Window Layouts menu screenshot)
  - https://forum.borisfx.com/t/24542 (event label)
- MAGIX VEGAS Pro 23 press release: https://www.magix.com/fileadmin/user_upload/Presse/2025/press-release-vegas-pro-23-int.pdf
- https://digitalproduction.com/2025/09/10/vegas-pro-23-boosts-engine-streamlines-colour-grading-modernises-interface/
- Movie Studio Zen update histories:
  - https://www.moviestudiozen.com/free-tutorials/vegas-pro/vegas-pro-23-update-history
  - https://www.moviestudiozen.com/free-tutorials/vegas-pro/vegas-pro-15-update-history
- VEGAS Pro 16 manual PDF (shortcuts pp. 556–569): https://files.bbystatic.com/AO37XI7GR2ciSfTb053kTQ==/c5b4a4a0-c7fe-4f33-94fd-fa7f379ae7c1.pdf
- VEGAS Pro 17 manual: http://download2.version-2.hk/vegascreativesoftware/download/VEGAS-Pro-17/manual_vegaspro17_en.pdf
- VEGAS Pro 14 manual: https://cdn.cloudflare.steamstatic.com/steam/apps/528200/manuals/Vegas140_Manual_EN.pdf
- VEGAS Pro 18 keyboard shortcuts: https://cdn.cloudflare.steamstatic.com/steam/apps/1325400/manuals/VEGAS_Pro_18_EN_Keyboard_shortcuts.pdf
- VP15 reviews:
  - https://www.redsharknews.com/post-vfx/item/4856-the-all-new-vegas-pro-15-a-nle-reborn
  - https://vegas-magazine.com/vegas-pro-15/
  - https://bedroomproducersblog.com/2017/11/05/vegas-pro-15-review/
- VP16 = VP15 interface: https://www.vegascreativesoftware.info/us/forum/release-vegas-pro-16-powerful-video-production-audio-editing--112586
- Official Steam screenshots:
  - VP16: https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/883140/ss_67fcb871f164d2947a95baf116390a24cf7ffdaa.1920x1080.jpg
  - VP17: https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/1110410/ss_ecbd817803247fe4c2b8f8f778f53d7b39148196.1920x1080.jpg
- Local install, VEGAS Pro 15.0 build 177:
  - `vegas150.exe` menu and string resources (52 menus decoded);
  - `Standard Layouts\*.VegasWindowLayout` (decoded dock geometry);
  - live native-pixel captures.

### 10.2 Reference screenshots (measurement only; proprietary; never ship or commit)
Root: `C:\Users\Maya\AppData\Local\Temp\claude\57ed9b841c207c7c\233b5401-eddd-4cfa-8776-54b50e17df4a\scratchpad\vegas-research\`

| Look | File (under the root) | Shows |
|---|---|---|
| 16 | `v15_native_default.png` | Real VP15 default layout, native pixels |
| 16 | `v15_native_markers.png` | Main measurement image: tracks, event, selection, region, cursor |
| 16 | `v15_native_tall_timeline.png`, `v15_native_tracks.png` | Track headers, selected header |
| 16 | `v15_native_projectmedia2.png`, `v15_native_explorer.png`, `v15_native_transitions.png`, `v15_native_mediagen.png`, `v15_native_trimmer.png`, `v15_native_masterbus.png` | Docked windows |
| 16 | `zoom_trackheaders.png`, `zoom_transport.png`, `zoom_toolbar_menu.png`, `zoom_tabs.png`, `zoom_preview.png`, `zoom_plugin_panel.png` | Nearest-neighbour enlargements |
| 16 | `steam_vp16.jpg`, `steam_vp17.jpg` | Official 1920×1080 VP16 and VP17 screenshots |
| 16 | `vegas150exe_res.txt`, `vegas150exe_res.txt.strings.txt` | Decoded menus and strings |
| 23 | `ui23\v23\img\vegasui.png` | Official VP23 main window, dark |
| 23 | `ui23\v23\img\track-list.png`, `transportedittoolbar.png`, `maintoolbar.png`, `window_videofx.png`, `win-docking.png`, `videopreview.png`, `masterbus_window.png`, `eventhandles.png`, `events.png`, `eventselected.png`, `regionexample.png`, `markerexample.png`, `loopplayback_on.png`, `loopplayback_off.png`, `snap_print.png`, `dragplayhead.png`, `lengthboxes.png`, `trimmer.png`, `welcomescreen_400x284.png`, `cg_window.png`, `projectmediawindow.png`, `explorer.png` | Official VP23 help images, per component |
| 23 | `ui23\forum\dark2026.png` | 1:1 1920×1200 Vegas Pro 2026 (≈23), dark: main measurement image |
| 23 | `ui23\forum\light2026.png` | 1:1 1920×1080 Vegas Pro 2026, White theme, default layout |
| 23 | `ui23\forum\t24626_2d7138b9.png`, `ui23\forum\t24626_502362a1.png` | View and Window Layouts menus (2026) |
| 16 | `n_start.png` | VP15 main window, live, 2026-11-11 (Rate: 0.00, b177 #101010 active toggles) |
| 16 | `n_projprops.png`, `n_prefs.png`, `n_prefs_display.png` (+ `_z`) | Native light Project Properties / Preferences / Display tab (PrintWindow, 100 % logical px) |
| 16 | `n_outfx.png` (+ `_z`), `n_trackmotion.png` | Video Output FX chain window (empty) and Track Motion window |
| 23 | `img23b\pan-crop-dialog.png`, `img23b\render-dlg.png`, `img23b\render-options.png`, `img23b\trackmotion.png` | Official VP23 help images: Pan/Crop, the new Render dialog, Track Motion |

---

**Not verified (tunable):**
- Exact VP23 Medium/Light colours and all VP16 non-Dark schemes (§2.4).
- v23 positions of new menu items.
- The VP23 loop-bar "on" colour.
- Meter gradient stops.
- VP23 default envelope and snap colours. Only volume/composite was measured; the rest fall back to the VP16 table.
- The VP16 Render As dialog's visual style. It is disabled on an empty project so it was not captured; assumed light native like the other VP16 dialogs.
- VP16 keyframe-controller button names and glyphs (§5.15.2) were read from a small capture; treat them as [I].
- Default "Recently used project list" count: the local VP15 shows 6, §6.1 says 4. One of them may be user-changed.
- Whether "Allow floating windows to dock" defaults to off. The local install shows off, but it could have been changed.

**Risk:**
- Pixel values for 23 come partly from the 2026 build.
- The VP16 Steam image is a composed marketing shot.
- The local captures are VP15 build 177. VP16 shares that UI, but the active-toggle colour became a preference ("Button Highlight Color") in VP16 [C: VP16 manual "What's new"]. b177 draws #101010; this spec uses #12B1FF from the VP16 Steam shot.
- The local VP15 colours (track, envelope, snap) are whatever the install's preferences hold. They match the factory values wherever they could be cross-checked.
