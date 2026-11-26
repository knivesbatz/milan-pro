//! R25/R26: the table between VEGAS Pro's OFX plug-ins (`PlugIn.UniqueID`, e.g. `com.sonycreativesoftware:gaussianblur`)
//! and Milan Pro's built-ins, with each parameter's name, scale and offset, so an imported FX keeps its settings and
//! an exported one lands on the VEGAS plug-in it came from. The VEGAS parameter names, choice indexes and value
//! ranges come from the plug-ins' own preset and resource XML (`Vfx1.ofx.bundle\Contents\Presets\PresetPackage.xml`,
//! `Resources\Vfx1.xml`), read in place from the VEGAS Pro 15 install; nothing of VEGAS's is copied here but names.
//!
//! A VEGAS plug-in with no row becomes a disabled `ofx:<id>@1` placeholder (`placeholder`): the OFX host loads it
//! when the plug-in is installed, and until then the project keeps its settings and the import report names it.

/// One parameter: a VEGAS OFX parameter name (a 2D or colour parameter's component as `Name.x` / `Name.r`) and the
/// Milan key it fills. `milan = vegas * scale + offset`, unless `table` is given: then the nearest VEGAS value in the
/// table picks its Milan value (choice indexes, angles that pick a direction).
#[derive(Debug)]
pub struct ParamMap {
    pub vegas: &'static str,
    pub milan: &'static str,
    pub scale: f32,
    pub offset: f32,
    pub table: &'static [(f32, f32)],
}

/// One plug-in. `set` holds Milan values the VEGAS plug-in implies without a parameter (Flash is a white additive
/// Fade to Color); `text` maps string parameters into `Fx.text`.
#[derive(Debug)]
pub struct FxMap {
    /// Without the `{Svfx:...}` wrapper VEGAS puts round OFX ids.
    pub vegas: &'static str,
    pub milan: &'static str,
    pub params: &'static [ParamMap],
    pub set: &'static [(&'static str, f32)],
    pub text: &'static [(&'static str, &'static str)],
}

const fn p(vegas: &'static str, milan: &'static str, scale: f32, offset: f32) -> ParamMap {
    ParamMap { vegas, milan, scale, offset, table: &[] }
}
const fn same(name: &'static str, milan: &'static str) -> ParamMap {
    p(name, milan, 1.0, 0.0)
}
const fn pick(vegas: &'static str, milan: &'static str, table: &'static [(f32, f32)]) -> ParamMap {
    ParamMap { vegas, milan, scale: 1.0, offset: 0.0, table }
}
/// VEGAS 0 Up, 1 Down, 2 Left, 3 Right → Milan `DIRECTIONS` Left 0, Right 1, Up 2, Down 3.
const PUSH_DIR: &[(f32, f32)] = &[(0.0, 2.0), (1.0, 3.0), (2.0, 0.0), (3.0, 1.0)];
/// Slide's angle (the direction the picture moves) → the same list.
const SLIDE_ANGLE: &[(f32, f32)] = &[(0.0, 1.0), (90.0, 2.0), (180.0, 0.0), (270.0, 3.0), (45.0, 5.0), (135.0, 4.0), (225.0, 6.0), (315.0, 7.0), (360.0, 1.0)];
/// Iris / Cookie Cutter shapes: Circle 0, Oval 1, Oval side 2, Rectangle 3, Diamond 4, Square 5, Triangle 6..13.
const IRIS_SHAPE: &[(f32, f32)] = &[(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (3.0, 1.0), (4.0, 2.0), (5.0, 1.0)];
const COOKIE_SHAPE: &[(f32, f32)] = &[(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (3.0, 1.0), (4.0, 2.0), (5.0, 1.0), (6.0, 3.0)];
/// Mirror's angle: 0 Reflect Top (top onto bottom), 180 Bottom, 90 Left (left onto right), -90 Right.
const MIRROR_ANGLE: &[(f32, f32)] = &[(0.0, 2.0), (180.0, 3.0), (-180.0, 3.0), (90.0, 0.0), (-90.0, 1.0), (270.0, 1.0)];
/// Test patterns: Color bars NTSC 0 / PAL 1, Gradient 2, Pluge 3, Ramp 4, SMPTE NTSC 5 / PAL 6, porches 7, 8.
const TEST_PATTERN: &[(f32, f32)] = &[(0.0, 1.0), (1.0, 1.0), (2.0, 2.0), (3.0, 0.0), (4.0, 2.0), (5.0, 0.0), (6.0, 0.0), (7.0, 0.0), (8.0, 0.0)];
/// Color Gradient: Linear 0, Elliptical 1, Rectangular 2 → Linear, Radial, Rectangle.
const GRADIENT_TYPE: &[(f32, f32)] = &[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)];

/// The whole table (section 4.2 of REMAINING.md plus the `docs/compat/ofx-vegas15.csv` ids that have a built-in).
pub static MAP: &[FxMap] = &[
    // ---- Video FX ----
    FxMap {
        vegas: "com.sonycreativesoftware:gaussianblur",
        milan: "milan.fx.gaussian_blur",
        // VEGAS's Light Blur is 0.01 and Medium 0.06 of the frame; Milan's are 0.002 and 0.01.
        params: &[p("HorizontalRange", "h_range", 0.2, 0.0), p("VerticalRange", "v_range", 0.2, 0.0)],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:brightnessandcontrast",
        milan: "milan.fx.brightness_contrast",
        params: &[same("Brightness", "brightness"), same("Contrast", "contrast"), same("ContrastCenter", "center")],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:colorcorrector",
        // The Low / Mid / High wheels have no Milan counterpart; the four sliders do.
        milan: "milan.fx.color_corrector",
        params: &[same("Saturation", "saturation"), same("Gamma", "gamma"), same("Gain", "gain"), same("Offset", "offset")],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:levels",
        milan: "milan.fx.levels",
        params: &[same("InputStart", "in_start"), same("InputEnd", "in_end"), same("Gamma", "gamma"), same("OutputStart", "out_start"), same("OutputEnd", "out_end")],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:hsladjust",
        milan: "milan.fx.hsl",
        // VEGAS's hue is in turns (0.5 = half way round); Milan's in degrees.
        params: &[p("AddToHue", "hue", 360.0, 0.0), same("Saturation", "saturation"), same("Luminance", "luminance")],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:chromakeyer",
        milan: "milan.fx.chroma_key",
        params: &[
            same("color.r", "color.r"),
            same("color.g", "color.g"),
            same("color.b", "color.b"),
            same("LowThreshold", "low"),
            same("HighThreshold", "high"),
            // VEGAS blurs over 0..1; Milan's slider stops at 0.02 of the frame.
            p("BlurAmount", "blur", 0.02, 0.0),
            same("ShowMaskOnly", "mask_only"),
        ],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:sharpen",
        milan: "milan.fx.sharpen",
        // VEGAS's Medium is 0.5 of 1; Milan's default amount is 1 of 5.
        params: &[p("Amount", "amount", 2.0, 0.0)],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:unsharpmask",
        milan: "milan.fx.sharpen",
        params: &[same("Amount", "amount"), p("Radius", "radius", 0.02, 0.0), same("Threshold", "threshold")],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:sepia",
        milan: "milan.fx.sepia",
        params: &[same("BlendingStrength", "amount")],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:invert",
        milan: "milan.fx.invert",
        params: &[same("BlendAmount", "amount")],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:blackandwhite",
        milan: "milan.fx.black_white",
        params: &[same("BlendAmount", "blend")],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:mirror",
        milan: "milan.fx.mirror",
        // The mirror line's position along the chosen direction: VEGAS's Center is y-up, Milan's position is the
        // fraction across; the two match for a line through the centre.
        params: &[pick("Angle", "direction", MIRROR_ANGLE), same("Center.x", "position")],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:border",
        milan: "milan.fx.border",
        params: &[same("Size", "size"), same("color.r", "color.r"), same("color.g", "color.g"), same("color.b", "color.b"), same("color.a", "color.a")],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:vignette",
        milan: "milan.fx.vignette",
        params: &[same("Outer", "size"), same("Feather", "softness"), same("Color.r", "color.r"), same("Color.g", "color.g"), same("Color.b", "color.b")],
        set: &[],
        text: &[],
    },
    FxMap {
        // XScale / YScale / Location become left / top / right / bottom in `vegas::crop_params`.
        vegas: "com.sonycreativesoftware:crop",
        milan: "milan.fx.crop",
        params: &[],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:cookiecutter",
        milan: "milan.fx.cookie_cutter",
        params: &[
            pick("Shape", "shape", COOKIE_SHAPE),
            same("Size", "size"),
            same("Center.x", "x"),
            p("Center.y", "y", -1.0, 1.0),
            same("Feather", "feather"),
            same("Border", "border"),
            same("Color.r", "border_color.r"),
            same("Color.g", "border_color.g"),
            same("Color.b", "border_color.b"),
            // Method 1 is Cut away section.
            same("Method", "invert"),
        ],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:lutfilter",
        milan: "milan.color_grading",
        params: &[],
        set: &[],
        text: &[("LUTFilename", "look_lut")],
    },
    // ---- Generators ----
    FxMap {
        vegas: "com.sonycreativesoftware:solidcolor",
        milan: "milan.gen.solid",
        params: &[same("Color.r", "color.r"), same("Color.g", "color.g"), same("Color.b", "color.b"), same("Color.a", "color.a")],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:colorgradient",
        milan: "milan.gen.gradient",
        params: &[pick("GradientType", "type", GRADIENT_TYPE), same("AspectRatioAngle", "angle")],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:checkerboard",
        milan: "milan.gen.checkerboard",
        params: &[
            same("TileDimensionsWidth", "size"),
            same("Color1.r", "color1.r"),
            same("Color1.g", "color1.g"),
            same("Color1.b", "color1.b"),
            same("Color1.a", "color1.a"),
            same("Color2.r", "color2.r"),
            same("Color2.g", "color2.g"),
            same("Color2.b", "color2.b"),
            same("Color2.a", "color2.a"),
        ],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:noisetexture",
        milan: "milan.gen.noise",
        params: &[
            same("NumberOfLayers", "octaves"),
            same("Color1.r", "color1.r"),
            same("Color1.g", "color1.g"),
            same("Color1.b", "color1.b"),
            same("Color1.a", "color1.a"),
            same("Color2.r", "color2.r"),
            same("Color2.g", "color2.g"),
            same("Color2.b", "color2.b"),
            same("Color2.a", "color2.a"),
        ],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:testpattern",
        milan: "milan.gen.test_pattern",
        params: &[pick("TestPatternType", "type", TEST_PATTERN)],
        set: &[],
        text: &[],
    },
    FxMap {
        // The RTF Text parameter is turned into plain text and a font in `vegas::titles_text`.
        vegas: "com.sonycreativesoftware:titlesandtext",
        milan: "milan.gen.text",
        params: &[
            // VEGAS's Scale 1 draws its 96 pt default at about a tenth of the frame height.
            p("Scale", "size", 0.1, 0.0),
            same("Location.x", "x"),
            p("Location.y", "y", -1.0, 1.0),
            same("TextColor.r", "color.r"),
            same("TextColor.g", "color.g"),
            same("TextColor.b", "color.b"),
            same("TextColor.a", "color.a"),
            same("Background.r", "background.r"),
            same("Background.g", "background.g"),
            same("Background.b", "background.b"),
            same("Background.a", "background.a"),
            p("OutlineWidth", "outline", 0.002, 0.0),
            same("OutlineColor.r", "outline_color.r"),
            same("OutlineColor.g", "outline_color.g"),
            same("OutlineColor.b", "outline_color.b"),
            same("OutlineColor.a", "outline_color.a"),
            same("ShadowEnable", "shadow"),
            p("ShadowOffsetX", "shadow_x", 0.025, 0.0),
            p("ShadowOffsetY", "shadow_y", 0.025, 0.0),
            p("ShadowBlur", "shadow_blur", 0.01, 0.0),
            same("ShadowColor.r", "shadow_color.r"),
            same("ShadowColor.g", "shadow_color.g"),
            same("ShadowColor.b", "shadow_color.b"),
            same("ShadowColor.a", "shadow_color.a"),
            same("LineSpacing", "spacing"),
        ],
        set: &[],
        text: &[],
    },
    // ---- Transitions ----
    FxMap { vegas: "com.sonycreativesoftware:dissolve", milan: "milan.tr.dissolve", params: &[], set: &[], text: &[] },
    FxMap {
        vegas: "com.sonycreativesoftware:linearwipe",
        milan: "milan.tr.linear_wipe",
        params: &[same("Angle", "angle"), p("Feather", "feather", 0.5, 0.0)],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:clockwipe",
        milan: "milan.tr.clock_wipe",
        // A soft edge of so many degrees, as a fraction of the sweep.
        params: &[p("FeatherAngle", "feather", 0.5 / 360.0, 0.0)],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:iris",
        milan: "milan.tr.iris",
        params: &[pick("Shape", "shape", IRIS_SHAPE), p("Feather", "feather", 0.5, 0.0)],
        set: &[],
        text: &[],
    },
    FxMap { vegas: "com.sonycreativesoftware:push", milan: "milan.tr.push", params: &[pick("Direction", "direction", PUSH_DIR)], set: &[], text: &[] },
    FxMap { vegas: "com.sonycreativesoftware:slide", milan: "milan.tr.slide", params: &[pick("Angle", "direction", SLIDE_ANGLE)], set: &[], text: &[] },
    FxMap { vegas: "com.sonycreativesoftware:zoom", milan: "milan.tr.zoom", params: &[], set: &[], text: &[] },
    FxMap {
        vegas: "com.sonycreativesoftware:flash",
        milan: "milan.tr.fade_to_color",
        params: &[same("Tint.r", "color.r"), same("Tint.g", "color.g"), same("Tint.b", "color.b")],
        // VEGAS's Flash is a bright additive dip: Milan's Flash preset.
        set: &[("additive", 1.0), ("color.r", 1.0), ("color.g", 1.0), ("color.b", 1.0)],
        text: &[],
    },
];

/// VEGAS audio FX, by plug-in name (the DirectX plug-ins have no OFX id): the Milan audio FX that stands in.
pub static AUDIO: &[(&str, &str)] = &[
    ("Track EQ", "milan.afx.eq"),
    ("Track Compressor", "milan.afx.compressor"),
    ("Track Noise Gate", "milan.afx.gate"),
    ("Noise Gate", "milan.afx.gate"),
    ("Graphic EQ", "milan.afx.eq"),
    ("Paragraphic EQ", "milan.afx.eq"),
    ("Parametric EQ", "milan.afx.eq"),
    ("Reverb", "milan.afx.reverb"),
    ("Simple Delay", "milan.afx.delay"),
    ("Multi-Tap Delay", "milan.afx.delay"),
    ("Wave Hammer", "milan.afx.limiter"),
    ("Wave Hammer Surround", "milan.afx.limiter"),
    ("ExpressFX Dynamics", "milan.afx.compressor"),
    ("ExpressFX Equalization", "milan.afx.eq"),
    ("ExpressFX Delay", "milan.afx.delay"),
    ("ExpressFX Reverb", "milan.afx.reverb"),
    ("ExpressFX Noise Gate", "milan.afx.gate"),
];

/// A VEGAS plug-in id as the table spells it: `PlugIn.UniqueID` wraps OFX ids as `{Svfx:com.vendor:name}`.
pub fn normalize(id: &str) -> &str {
    let id = id.trim();
    let inner = id.strip_prefix('{').and_then(|s| s.strip_suffix('}')).unwrap_or(id);
    inner.strip_prefix("Svfx:").or_else(|| inner.strip_prefix("svfx:")).unwrap_or(inner)
}

/// The row for VEGAS plug-in `id` (wrapped or not).
pub fn to_milan(id: &str) -> Option<&'static FxMap> {
    let id = normalize(id);
    MAP.iter().find(|m| m.vegas.eq_ignore_ascii_case(id))
}

/// The Milan audio FX for VEGAS audio plug-in `name` ("VEGAS Track EQ", "Sony Track EQ" or "Track EQ").
pub fn audio_to_milan(name: &str) -> Option<&'static str> {
    let bare = ["VEGAS ", "Sony ", "Sonic Foundry ", "MAGIX "].iter().find_map(|p| name.strip_prefix(p)).unwrap_or(name).trim();
    AUDIO.iter().find(|(v, _)| v.eq_ignore_ascii_case(bare)).map(|x| x.1)
}

/// The plug-in id an unmapped VEGAS OFX plug-in is kept under: the OFX host's `ofx:<id>@<major>` with the major
/// version VEGAS 15's bundles all have.
pub fn placeholder(id: &str) -> String {
    format!("{}{}@1", crate::engine::ofx::PREFIX, normalize(id))
}

impl ParamMap {
    /// VEGAS value → Milan value.
    pub fn to_milan(&self, v: f32) -> f32 {
        match self.table {
            [] => v * self.scale + self.offset,
            t => nearest(t, v, |e| e.0).1,
        }
    }
}

fn nearest(t: &'static [(f32, f32)], v: f32, key: impl Fn(&(f32, f32)) -> f32) -> &'static (f32, f32) {
    t.iter().min_by(|a, b| (key(a) - v).abs().total_cmp(&(key(b) - v).abs())).expect("a table has rows")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::plugins::{self, ParamKind};

    /// Every row names a Milan built-in, and every Milan key it fills is a parameter of that built-in (component
    /// suffixes included); VEGAS names are spelled once.
    #[test]
    fn every_row_lands_on_a_built_in_parameter() {
        for m in MAP {
            let info = plugins::find(m.milan).unwrap_or_else(|| panic!("{} is not a built-in", m.milan));
            let has = |key: &str| {
                let (base, suffix) = key.rsplit_once('.').map_or((key, ""), |(b, s)| (b, s));
                info.params.iter().any(|p| match p.kind {
                    ParamKind::Color { .. } => p.key == base && ["r", "g", "b", "a"].contains(&suffix),
                    ParamKind::Text { .. } | ParamKind::Font { .. } => p.key == key,
                    _ => p.key == key,
                })
            };
            for pm in m.params {
                assert!(has(pm.milan), "{}: {} → {} is not a parameter", m.vegas, pm.vegas, pm.milan);
                assert!(!pm.table.is_empty() || pm.scale != 0.0, "{}: {} has a zero scale", m.vegas, pm.vegas);
            }
            for (k, _) in m.set {
                assert!(has(k), "{}: set {k} is not a parameter", m.vegas);
            }
            for (_, k) in m.text {
                assert!(info.params.iter().any(|p| p.key == *k && matches!(p.kind, ParamKind::Text { .. })), "{}: text {k}", m.vegas);
            }
            assert!(m.vegas.starts_with("com.sonycreativesoftware:"), "{}", m.vegas);
        }
        for (_, id) in AUDIO {
            assert!(plugins::find(id).is_some(), "{id}");
        }
    }

    #[test]
    fn ids_are_found_wrapped_or_bare_and_round_trip() {
        let m = to_milan("{Svfx:com.sonycreativesoftware:gaussianblur}").unwrap();
        assert_eq!(m.milan, "milan.fx.gaussian_blur");
        assert_eq!(to_milan("com.sonycreativesoftware:GaussianBlur").map(|m| m.milan), Some("milan.fx.gaussian_blur"));
        assert!(to_milan("com.sonycreativesoftware:filmgrain").is_none());
        assert_eq!(placeholder("{Svfx:com.sonycreativesoftware:filmgrain}"), "ofx:com.sonycreativesoftware:filmgrain@1");
        assert_eq!(audio_to_milan("VEGAS Track EQ"), Some("milan.afx.eq"));
        assert_eq!(audio_to_milan("Sony Wave Hammer"), Some("milan.afx.limiter"));
        assert_eq!(audio_to_milan("Acoustic Mirror"), None);
    }

    #[test]
    fn scales_and_tables_convert() {
        let blur = &to_milan("com.sonycreativesoftware:gaussianblur").unwrap().params[0];
        assert!((blur.to_milan(0.06) - 0.012).abs() < 1e-6);
        let push = &to_milan("com.sonycreativesoftware:push").unwrap().params[0];
        // VEGAS Push Left (2) is Milan's Left (0); a value between rows picks the nearest.
        assert_eq!((push.to_milan(2.0), push.to_milan(2.3)), (0.0, 0.0));
        let mirror = &to_milan("com.sonycreativesoftware:mirror").unwrap().params[0];
        assert_eq!((mirror.to_milan(-90.0), mirror.to_milan(0.0)), (1.0, 2.0));
        let y = &to_milan("com.sonycreativesoftware:titlesandtext").unwrap().params[2];
        assert!(y.milan == "y" && (y.to_milan(0.2) - 0.8).abs() < 1e-6);
    }
}
