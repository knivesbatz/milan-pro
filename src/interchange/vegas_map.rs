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
    // ---- MVFX: the music-video FX that are VEGAS built-ins (docs/research/MUSIC_VIDEO_2000S.md) ----
    FxMap {
        vegas: "com.sonycreativesoftware:glow",
        milan: "milan.fx.glow",
        params: &[same("GlowPercent", "glow_percent"), same("Intensity", "intensity"), same("Suppression", "suppression"), same("Color.r", "color.r"), same("Color.g", "color.g"), same("Color.b", "color.b")],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:filmeffects",
        milan: "milan.fx.film_effects",
        // The particle Type list is the same in both: Dust, Flicker, Hair, Jitter, Scratches.
        params: &[
            same("Grain", "grain"),
            same("Tint", "tint"),
            same("Color.r", "color.r"),
            same("Color.g", "color.g"),
            same("Color.b", "color.b"),
            same("ConvertToGreyscale", "grey"),
            same("EnableEnableParticles", "particles"),
            same("EnableParticlesType", "type"),
            same("EnableParticlesAmount", "amount"),
        ],
        set: &[],
        text: &[],
    },
    // VEGAS spells the third parameter "Chormaticity".
    FxMap { vegas: "com.sonycreativesoftware:filmgrain", milan: "milan.fx.film_grain", params: &[same("Amount", "amount"), same("Granularity", "granularity"), same("Chormaticity", "chroma")], set: &[], text: &[] },
    FxMap {
        vegas: "com.sonycreativesoftware:spherize",
        milan: "milan.fx.spherize",
        params: &[same("Center.x", "x"), p("Center.y", "y", -1.0, 1.0), same("Amount", "amount"), same("ScalingHorizontal", "sh"), same("ScalingVertical", "sv"), same("ScalingProportional", "proportional")],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:pinchpunch",
        milan: "milan.fx.pinch_punch",
        params: &[same("Center.x", "x"), p("Center.y", "y", -1.0, 1.0), same("Amount", "amount"), same("ScalingHorizontal", "sh"), same("ScalingVertical", "sv"), same("ScalingProportional", "proportional")],
        set: &[],
        text: &[],
    },
    // Type: Proportional, Fixed radius, Gaussian in both.
    FxMap { vegas: "com.sonycreativesoftware:radialblur", milan: "milan.fx.radial_blur", params: &[same("Center.x", "x"), p("Center.y", "y", -1.0, 1.0), same("Type", "type"), same("Strength", "strength")], set: &[], text: &[] },
    FxMap { vegas: "com.sonycreativesoftware:linearblur", milan: "milan.fx.linear_blur", params: &[same("Angle", "angle"), same("Amount", "amount")], set: &[], text: &[] },
    FxMap {
        vegas: "com.sonycreativesoftware:tvsimulator",
        milan: "milan.fx.tv_simulator",
        params: &[
            same("DetailZoom", "detail"),
            same("ApertureGrill", "aperture"),
            same("Interlacing", "interlacing"),
            same("LineSync", "line_sync"),
            same("VerticalSync", "vsync"),
            same("ScanPhasing", "phasing"),
            same("Phosphorescence", "phosphor"),
            same("Static", "static"),
        ],
        set: &[],
        text: &[],
    },
    FxMap { vegas: "com.sonycreativesoftware:threshold", milan: "milan.fx.threshold", params: &[same("Threshold", "threshold")], set: &[], text: &[] },
    // Type: Color, Monochrome, Offset color in both.
    FxMap { vegas: "com.sonycreativesoftware:newsprint", milan: "milan.fx.newsprint", params: &[same("DotSize", "dot"), same("Type", "type")], set: &[], text: &[] },
    FxMap {
        vegas: "com.sonycreativesoftware:addnoise",
        milan: "milan.fx.add_noise",
        params: &[same("NoiseLevel", "level"), same("Monochromatic", "mono"), same("GaussianNoise", "gaussian"), same("Animate", "animate")],
        set: &[],
        text: &[],
    },
    FxMap { vegas: "com.sonycreativesoftware:pixelate", milan: "milan.fx.pixelate", params: &[same("HorizontalPixelization", "h"), same("VerticalPixelization", "v")], set: &[], text: &[] },
    // The gradient itself is VEGAS's own control (not an OFX parameter): it imports as Milan's default.
    FxMap { vegas: "com.sonycreativesoftware:gradientmap", milan: "milan.fx.gradient_map", params: &[same("Amount", "amount"), same("Mode", "mode")], set: &[], text: &[] },
    FxMap {
        vegas: "com.sonycreativesoftware:lightrays",
        milan: "milan.fx.light_rays",
        params: &[
            same("LightSource.x", "x"),
            p("LightSource.y", "y", -1.0, 1.0),
            same("TintColor.r", "tint.r"),
            same("TintColor.g", "tint.g"),
            same("TintColor.b", "tint.b"),
            same("group0Sensitivity", "sensitivity"),
            same("group0Strength", "strength"),
            same("group1RadiusX", "radius"),
        ],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:softcontrastvelvetmatter",
        milan: "milan.fx.soft_contrast",
        params: &[same("EffectContrast", "contrast"), same("EffectDiffusion", "diffusion"), same("EffectTint", "tint"), same("EffectCorrectionHue", "hue"), same("VignetteStrength", "vignette")],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:starburstvelvetmatter",
        milan: "milan.fx.starburst",
        params: &[same("Threshold", "threshold"), same("Brilliance", "brilliance"), same("Strength", "strength"), same("Radius", "radius"), same("Points", "points"), same("Orientation", "orientation"), same("EffectOnly", "effect_only")],
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
    // MVFX: VEGAS's Flash with its diffusion and tint (the Fade to Color row below still exports as a Flash).
    FxMap { vegas: "com.sonycreativesoftware:flash", milan: "milan.tr.flash", params: &[same("HorizontalDiffusion", "h_diff"), same("VerticalDiffusion", "v_diff"), same("Tint.r", "tint.r"), same("Tint.g", "tint.g"), same("Tint.b", "tint.b")], set: &[], text: &[] },
    FxMap {
        vegas: "com.sonycreativesoftware:crosseffect",
        milan: "milan.tr.cross_effect",
        // Effect: None, Zoom, Pixelate, Blur; Apply to: Source, Destination, Both, in both.
        params: &[same("EffectType", "effect"), same("ApplyTo", "apply_to"), same("FadeRange", "fade"), same("EffectSettingsMaxZoom", "max_zoom"), same("EffectSettingsMaxX", "max_x"), same("EffectSettingsMaxY", "max_y"), same("EffectSettingsMaxBlur", "max_blur")],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:pagepeel",
        milan: "milan.tr.page_peel",
        params: &[same("PeelAngle", "angle"), same("FoldRadius", "fold"), same("PeelOpacity", "opacity"), same("LightColor.r", "light.r"), same("LightColor.g", "light.g"), same("LightColor.b", "light.b")],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:pageroll",
        milan: "milan.tr.page_roll",
        params: &[same("RollAngle", "angle"), same("FoldRadius", "fold"), same("RollOpacity", "opacity"), same("LightColor.r", "light.r"), same("LightColor.g", "light.g"), same("LightColor.b", "light.b")],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:barndoor",
        milan: "milan.tr.barn_door",
        params: &[same("Orientation", "orientation"), same("Direction", "direction"), same("BorderSize", "border"), same("BorderFeather", "feather"), same("BorderColor.r", "border_color.r"), same("BorderColor.g", "border_color.g"), same("BorderColor.b", "border_color.b")],
        set: &[],
        text: &[],
    },
    FxMap { vegas: "com.sonycreativesoftware:venetianblinds", milan: "milan.tr.venetian_blinds", params: &[same("Angle", "angle"), same("Feather", "feather"), same("NumberOfBlinds", "blinds")], set: &[], text: &[] },
    FxMap {
        vegas: "com.sonycreativesoftware:spiral",
        milan: "milan.tr.spiral",
        params: &[same("Turns", "turns"), same("Zoom", "zoom"), same("Orientation", "orientation"), same("Motion", "motion"), same("Direction", "direction")],
        set: &[],
        text: &[],
    },
    FxMap { vegas: "com.sonycreativesoftware:squeeze", milan: "milan.tr.squeeze", params: &[same("Start", "start"), same("SqueezePreviousImage", "squeeze_previous")], set: &[], text: &[] },
    FxMap {
        vegas: "com.sonycreativesoftware:starwipe",
        milan: "milan.tr.star_wipe",
        params: &[
            same("Arms", "arms"),
            same("Ratio", "ratio"),
            same("Angle", "angle"),
            same("Feather", "feather"),
            same("Border", "border"),
            same("BorderColor.r", "border_color.r"),
            same("BorderColor.g", "border_color.g"),
            same("BorderColor.b", "border_color.b"),
            same("Center.x", "x"),
            p("Center.y", "y", -1.0, 1.0),
            same("Waveform", "waveform"),
        ],
        set: &[],
        text: &[],
    },
    FxMap {
        vegas: "com.sonycreativesoftware:portals",
        milan: "milan.tr.portals",
        params: &[
            same("RandomPatternSeed", "seed"),
            same("Squares", "squares"),
            same("MaxTransparency", "transparency"),
            same("MaxOffset", "offset"),
            same("MaxScale", "scale"),
            same("BorderSize", "border"),
            same("BorderColor.r", "border_color.r"),
            same("BorderColor.g", "border_color.g"),
            same("BorderColor.b", "border_color.b"),
        ],
        set: &[],
        text: &[],
    },
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

/// The row that sends Milan plug-in `id` back to VEGAS (the first row naming it: Sharpen goes back as sharpen).
pub fn to_vegas(milan: &str) -> Option<&'static FxMap> {
    MAP.iter().find(|m| m.milan == milan)
}

/// The Milan audio FX for VEGAS audio plug-in `name` ("VEGAS Track EQ", "Sony Track EQ" or "Track EQ").
pub fn audio_to_milan(name: &str) -> Option<&'static str> {
    let bare = ["VEGAS ", "Sony ", "Sonic Foundry ", "MAGIX "].iter().find_map(|p| name.strip_prefix(p)).unwrap_or(name).trim();
    AUDIO.iter().find(|(v, _)| v.eq_ignore_ascii_case(bare)).map(|x| x.1)
}

/// The VEGAS name that stands for Milan audio FX `milan`.
pub fn audio_to_vegas(milan: &str) -> Option<&'static str> {
    AUDIO.iter().find(|(_, m)| *m == milan).map(|x| x.0)
}

/// The plug-in id an unmapped VEGAS OFX plug-in is kept under: the OFX host's `ofx:<id>@<major>` with the major
/// version VEGAS 15's bundles all have.
pub fn placeholder(id: &str) -> String {
    format!("{}{}@1", crate::engine::ofx::PREFIX, normalize(id))
}

/// A VEGAS id back from a placeholder (`ofx:com.x:y@1` → `com.x:y`); None for anything else.
pub fn from_placeholder(plugin: &str) -> Option<&str> {
    let rest = plugin.strip_prefix(crate::engine::ofx::PREFIX)?;
    Some(rest.rsplit_once('@').map_or(rest, |x| x.0))
}

impl ParamMap {
    /// VEGAS value → Milan value.
    pub fn to_milan(&self, v: f32) -> f32 {
        match self.table {
            [] => v * self.scale + self.offset,
            t => nearest(t, v, |e| e.0).1,
        }
    }

    /// Milan value → VEGAS value (the inverse; a table picks the first VEGAS value that gives this Milan value).
    pub fn to_vegas(&self, v: f32) -> f32 {
        match self.table {
            [] if self.scale != 0.0 => (v - self.offset) / self.scale,
            [] => 0.0,
            t => t.iter().find(|e| e.1 == v.round()).unwrap_or_else(|| nearest(t, v, |e| e.1)).0,
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
        assert!(to_milan("com.sonycreativesoftware:radialpixelate").is_none());
        assert_eq!(to_vegas("milan.tr.fade_to_color").map(|m| m.vegas), Some("com.sonycreativesoftware:flash"));
        assert_eq!(placeholder("{Svfx:com.sonycreativesoftware:radialpixelate}"), "ofx:com.sonycreativesoftware:radialpixelate@1");
        assert_eq!(from_placeholder("ofx:com.sonycreativesoftware:filmgrain@1"), Some("com.sonycreativesoftware:filmgrain"));
        assert_eq!(from_placeholder("milan.fx.invert"), None);
        assert_eq!(audio_to_milan("VEGAS Track EQ"), Some("milan.afx.eq"));
        assert_eq!(audio_to_milan("Sony Wave Hammer"), Some("milan.afx.limiter"));
        assert_eq!(audio_to_milan("Acoustic Mirror"), None);
        assert_eq!(audio_to_vegas("milan.afx.gate"), Some("Track Noise Gate"));
    }

    /// MVFX: VEGAS's music-video built-ins import as Milan's own (settings kept) and export back as themselves.
    #[test]
    fn mvfx_vegas_effects_import_as_built_ins() {
        let rows = [
            ("glow", "milan.fx.glow"),
            ("filmeffects", "milan.fx.film_effects"),
            ("filmgrain", "milan.fx.film_grain"),
            ("spherize", "milan.fx.spherize"),
            ("pinchpunch", "milan.fx.pinch_punch"),
            ("radialblur", "milan.fx.radial_blur"),
            ("linearblur", "milan.fx.linear_blur"),
            ("tvsimulator", "milan.fx.tv_simulator"),
            ("threshold", "milan.fx.threshold"),
            ("newsprint", "milan.fx.newsprint"),
            ("addnoise", "milan.fx.add_noise"),
            ("pixelate", "milan.fx.pixelate"),
            ("gradientmap", "milan.fx.gradient_map"),
            ("lightrays", "milan.fx.light_rays"),
            ("softcontrastvelvetmatter", "milan.fx.soft_contrast"),
            ("starburstvelvetmatter", "milan.fx.starburst"),
            ("flash", "milan.tr.flash"),
            ("crosseffect", "milan.tr.cross_effect"),
            ("pagepeel", "milan.tr.page_peel"),
            ("pageroll", "milan.tr.page_roll"),
            ("barndoor", "milan.tr.barn_door"),
            ("venetianblinds", "milan.tr.venetian_blinds"),
            ("spiral", "milan.tr.spiral"),
            ("squeeze", "milan.tr.squeeze"),
            ("starwipe", "milan.tr.star_wipe"),
            ("portals", "milan.tr.portals"),
        ];
        for (v, m) in rows {
            let id = format!("com.sonycreativesoftware:{v}");
            let row = to_milan(&format!("{{Svfx:{id}}}")).unwrap_or_else(|| panic!("{v} has no row"));
            assert_eq!(row.milan, m);
            assert!(!row.params.is_empty(), "{v} keeps its settings");
            assert_eq!(to_vegas(m).map(|r| r.vegas), Some(id.as_str()), "{m} exports as {v}");
        }
        // VEGAS's centres are y-up; the particle and choice lists line up one to one.
        let y = to_milan("com.sonycreativesoftware:spherize").unwrap().params.iter().find(|p| p.milan == "y").unwrap();
        assert!((y.to_milan(0.25) - 0.75).abs() < 1e-6);
        let ty = to_milan("com.sonycreativesoftware:filmeffects").unwrap().params.iter().find(|p| p.vegas == "EnableParticlesType").unwrap();
        assert_eq!(ty.to_milan(4.0), 4.0);
        let fx = plugins::find("milan.fx.film_effects").unwrap();
        assert!(matches!(fx.params.iter().find(|p| p.key == "type").unwrap().kind, ParamKind::Choice { options, .. } if options[4] == "Scratches"));
    }

    #[test]
    fn scales_and_tables_invert() {
        let blur = &to_milan("com.sonycreativesoftware:gaussianblur").unwrap().params[0];
        assert!((blur.to_milan(0.06) - 0.012).abs() < 1e-6 && (blur.to_vegas(0.012) - 0.06).abs() < 1e-6);
        let push = &to_milan("com.sonycreativesoftware:push").unwrap().params[0];
        // VEGAS Push Left (2) is Milan's Left (0); a value between rows picks the nearest.
        assert_eq!((push.to_milan(2.0), push.to_milan(2.3), push.to_vegas(0.0), push.to_vegas(3.0)), (0.0, 0.0, 2.0, 1.0));
        let mirror = &to_milan("com.sonycreativesoftware:mirror").unwrap().params[0];
        assert_eq!((mirror.to_milan(-90.0), mirror.to_milan(0.0), mirror.to_vegas(0.0)), (1.0, 2.0, 90.0));
        let y = &to_milan("com.sonycreativesoftware:titlesandtext").unwrap().params[2];
        assert!(y.milan == "y" && (y.to_milan(0.2) - 0.8).abs() < 1e-6 && (y.to_vegas(0.8) - 0.2).abs() < 1e-6);
    }
}
