//! The plug-in registry (decision 0.4): every built-in video FX, transition and generator with its parameters and
//! presets, evaluation of a stored `Fx` at a time, and user presets in %APPDATA%\MilanPro\presets.json.
//! Audio FX register here too from batch 16.
//!
//! Storage: a Float is one key; a Color is four (`key.r`, `key.g`, `key.b`, `key.a`); Choice and Bool are floats
//! (index, 0/1); Text and Font live in `Fx.text`. A missing key means the default.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use crate::project::{Curve, Fx, Keyframe, Keyframes, Transition};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginKind {
    VideoFx,
    Transition,
    Generator,
}

#[derive(Clone, Copy, Debug)]
pub enum ParamKind {
    Float { min: f32, max: f32, default: f32 },
    Color { default: [f32; 4] },
    Choice { options: &'static [&'static str], default: usize },
    Bool { default: bool },
    Text { multiline: bool, default: &'static str },
    Font { default: &'static str },
}

#[derive(Debug)]
pub struct ParamSpec {
    pub key: &'static str,
    pub name: &'static str,
    /// The FX window heading this parameter starts; "" = in the previous one's section.
    pub section: &'static str,
    pub kind: ParamKind,
}

impl ParamSpec {
    const fn sec(self, section: &'static str) -> Self {
        ParamSpec { section, ..self }
    }
}

/// Stored values, in the same keys as `Fx.params` / `Fx.text` (colours as `key.r` .. `key.a`).
#[derive(Debug)]
pub struct Preset {
    pub name: &'static str,
    pub vals: &'static [(&'static str, f32)],
    pub text: &'static [(&'static str, &'static str)],
}

#[derive(Debug)]
pub struct PluginInfo {
    pub id: &'static str,
    /// The VEGAS name without "VEGAS ".
    pub name: &'static str,
    pub kind: PluginKind,
    /// VP16 chooser folder (`Milan\{group}`).
    pub group: &'static str,
    /// VP23 chooser chip.
    pub category: &'static str,
    pub desc: &'static str,
    pub params: &'static [ParamSpec],
    pub presets: &'static [Preset],
}

const fn f(key: &'static str, name: &'static str, min: f32, max: f32, default: f32) -> ParamSpec {
    ParamSpec { key, name, section: "", kind: ParamKind::Float { min, max, default } }
}
const fn color(key: &'static str, name: &'static str, default: [f32; 4]) -> ParamSpec {
    ParamSpec { key, name, section: "", kind: ParamKind::Color { default } }
}
const fn choice(key: &'static str, name: &'static str, options: &'static [&'static str], default: usize) -> ParamSpec {
    ParamSpec { key, name, section: "", kind: ParamKind::Choice { options, default } }
}
const fn flag(key: &'static str, name: &'static str, default: bool) -> ParamSpec {
    ParamSpec { key, name, section: "", kind: ParamKind::Bool { default } }
}
const fn text(key: &'static str, name: &'static str, multiline: bool, default: &'static str) -> ParamSpec {
    ParamSpec { key, name, section: "", kind: ParamKind::Text { multiline, default } }
}
const fn preset(name: &'static str, vals: &'static [(&'static str, f32)]) -> Preset {
    Preset { name, vals, text: &[] }
}
const DEFAULT: Preset = preset("Default", &[]);
const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const DIRECTIONS: &[&str] = &["Left", "Right", "Up", "Down", "Up Left", "Up Right", "Down Left", "Down Right"];

pub const GRADING: &str = "milan.color_grading";

pub static PLUGINS: &[PluginInfo] = &[
    // ---- Video FX ----
    PluginInfo {
        id: "milan.fx.brightness_contrast",
        name: "Brightness and Contrast",
        kind: PluginKind::VideoFx,
        group: "Adjust",
        category: "Color",
        desc: "Adjusts brightness and contrast",
        params: &[f("brightness", "Brightness", -1.0, 1.0, 0.0), f("contrast", "Contrast", -1.0, 1.0, 0.0), f("center", "Contrast center", 0.0, 1.0, 0.5)],
        presets: &[DEFAULT, preset("Brighter", &[("brightness", 0.15)]), preset("Darker", &[("brightness", -0.15)]), preset("More Contrast", &[("contrast", 0.3)]), preset("Less Contrast", &[("contrast", -0.3)])],
    },
    PluginInfo {
        id: "milan.fx.levels",
        name: "Levels",
        kind: PluginKind::VideoFx,
        group: "Adjust",
        category: "Color",
        desc: "Sets input and output levels",
        params: &[
            f("in_start", "Input start", 0.0, 1.0, 0.0),
            f("in_end", "Input end", 0.0, 1.0, 1.0),
            f("gamma", "Gamma", 0.1, 10.0, 1.0),
            f("out_start", "Output start", 0.0, 1.0, 0.0),
            f("out_end", "Output end", 0.0, 1.0, 1.0),
        ],
        presets: &[
            DEFAULT,
            preset("Computer RGB to Studio RGB", &[("out_start", 16.0 / 255.0), ("out_end", 235.0 / 255.0)]),
            preset("Studio RGB to Computer RGB", &[("in_start", 16.0 / 255.0), ("in_end", 235.0 / 255.0)]),
        ],
    },
    PluginInfo {
        id: "milan.fx.invert",
        name: "Invert",
        kind: PluginKind::VideoFx,
        group: "Adjust",
        category: "Color",
        desc: "Inverts the colours",
        params: &[f("amount", "Amount", 0.0, 1.0, 1.0)],
        presets: &[DEFAULT, preset("Half", &[("amount", 0.5)])],
    },
    PluginInfo {
        id: "milan.fx.black_white",
        name: "Black and White",
        kind: PluginKind::VideoFx,
        group: "Adjust",
        category: "Color",
        desc: "Removes colour",
        params: &[f("blend", "Blend amount", 0.0, 1.0, 1.0)],
        presets: &[preset("100% Black and White", &[]), preset("50% Black and White", &[("blend", 0.5)])],
    },
    PluginInfo {
        id: "milan.fx.sepia",
        name: "Sepia",
        kind: PluginKind::VideoFx,
        group: "Adjust",
        category: "Color",
        desc: "Tints the picture brown like an old photograph",
        params: &[f("amount", "Blend amount", 0.0, 1.0, 1.0)],
        presets: &[DEFAULT, preset("Light", &[("amount", 0.5)])],
    },
    PluginInfo {
        id: "milan.fx.hsl",
        name: "HSL Adjust",
        kind: PluginKind::VideoFx,
        group: "Adjust",
        category: "Color",
        desc: "Shifts hue and scales saturation and luminance",
        params: &[f("hue", "Add to hue", -180.0, 180.0, 0.0), f("saturation", "Saturation", 0.0, 4.0, 1.0), f("luminance", "Luminance", 0.0, 4.0, 1.0)],
        presets: &[DEFAULT, preset("Desaturate", &[("saturation", 0.5)]), preset("Saturate", &[("saturation", 1.5)])],
    },
    PluginInfo {
        id: "milan.fx.color_corrector",
        name: "Color Corrector",
        kind: PluginKind::VideoFx,
        group: "Adjust",
        category: "Color",
        desc: "Corrects saturation, gamma, gain and offset",
        params: &[f("saturation", "Saturation", 0.0, 4.0, 1.0), f("gamma", "Gamma", 0.1, 4.0, 1.0), f("gain", "Gain", 0.0, 4.0, 1.0), f("offset", "Offset", -1.0, 1.0, 0.0)],
        presets: &[DEFAULT, preset("Punchy", &[("saturation", 1.3), ("gamma", 0.9)])],
    },
    PluginInfo {
        id: "milan.fx.gaussian_blur",
        name: "Gaussian Blur",
        kind: PluginKind::VideoFx,
        group: "Blur",
        category: "Blur",
        desc: "Softens the image",
        params: &[f("h_range", "Horizontal range", 0.0, 0.1, 0.005), f("v_range", "Vertical range", 0.0, 0.1, 0.005)],
        presets: &[
            DEFAULT,
            preset("Light Blur", &[("h_range", 0.002), ("v_range", 0.002)]),
            preset("Medium Blur", &[("h_range", 0.01), ("v_range", 0.01)]),
            preset("Heavy Blur", &[("h_range", 0.03), ("v_range", 0.03)]),
            preset("Horizontal Only", &[("h_range", 0.01), ("v_range", 0.0)]),
        ],
    },
    PluginInfo {
        id: "milan.fx.sharpen",
        name: "Sharpen",
        kind: PluginKind::VideoFx,
        group: "Blur",
        category: "Utility",
        desc: "Sharpens detail",
        params: &[f("amount", "Amount", 0.0, 5.0, 1.0), f("radius", "Radius", 0.0, 0.02, 0.002), f("threshold", "Threshold", 0.0, 1.0, 0.0)],
        presets: &[DEFAULT, preset("Strong", &[("amount", 2.0)])],
    },
    PluginInfo {
        id: "milan.fx.vignette",
        name: "Vignette",
        kind: PluginKind::VideoFx,
        group: "Stylize",
        category: "Light",
        desc: "Darkens the corners",
        params: &[f("amount", "Amount", 0.0, 1.0, 0.5), f("size", "Size", 0.0, 1.5, 0.7), f("softness", "Softness", 0.0, 1.0, 0.5), color("color", "Color", BLACK)],
        presets: &[DEFAULT, preset("Strong", &[("amount", 1.0)]), preset("White", &[("color.r", 1.0), ("color.g", 1.0), ("color.b", 1.0)])],
    },
    PluginInfo {
        id: "milan.fx.mirror",
        name: "Mirror",
        kind: PluginKind::VideoFx,
        group: "Stylize",
        category: "Creative",
        desc: "Mirrors the frame",
        params: &[choice("direction", "Direction", &["Left onto right", "Right onto left", "Top onto bottom", "Bottom onto top"], 0), f("position", "Position", 0.0, 1.0, 0.5)],
        presets: &[DEFAULT, preset("Vertical", &[("direction", 2.0)])],
    },
    PluginInfo {
        id: "milan.fx.crop",
        name: "Crop",
        kind: PluginKind::VideoFx,
        group: "Utility",
        category: "Utility",
        desc: "Crops the edges, with an optional soft edge",
        params: &[
            f("left", "Left", 0.0, 0.5, 0.0),
            f("top", "Top", 0.0, 0.5, 0.0),
            f("right", "Right", 0.0, 0.5, 0.0),
            f("bottom", "Bottom", 0.0, 0.5, 0.0),
            f("feather", "Feather", 0.0, 0.2, 0.0),
        ],
        presets: &[DEFAULT, preset("Letterbox 2.39:1", &[("top", 0.128), ("bottom", 0.128)])],
    },
    PluginInfo {
        id: "milan.fx.border",
        name: "Border",
        kind: PluginKind::VideoFx,
        group: "Utility",
        category: "Utility",
        desc: "Draws a border around the frame",
        params: &[f("size", "Size", 0.0, 0.5, 0.03), color("color", "Color", WHITE)],
        presets: &[DEFAULT, preset("Black", &[("color.r", 0.0), ("color.g", 0.0), ("color.b", 0.0)])],
    },
    PluginInfo {
        id: "milan.fx.chroma_key",
        name: "Chroma Keyer",
        kind: PluginKind::VideoFx,
        group: "Key",
        category: "Utility",
        desc: "Keys out a colour",
        params: &[
            // VEGAS's Chroma Keyer defaults to blue; the Green Screen preset is one click away.
            color("color", "Color", [0.0, 0.0, 1.0, 1.0]),
            f("low", "Low threshold", 0.0, 1.0, 0.1),
            f("high", "High threshold", 0.0, 1.0, 0.3),
            f("blur", "Blur amount", 0.0, 0.02, 0.0),
            flag("mask_only", "Show mask only", false),
        ],
        presets: &[
            DEFAULT,
            preset("Green Screen", &[("color.r", 0.0), ("color.g", 0.7), ("color.b", 0.25)]),
            preset("Blue Screen", &[("color.r", 0.1), ("color.g", 0.2), ("color.b", 0.8)]),
        ],
    },
    PluginInfo {
        id: GRADING,
        name: "Color Grading",
        kind: PluginKind::VideoFx,
        group: "Color Grading",
        category: "Color",
        desc: "Wheels, curves and LUTs",
        params: &[
            text("input_lut", "Input LUT", false, "").sec("Input LUT"),
            f("offset.r", "Offset R", -1.0, 1.0, 0.0).sec("Color Wheels"),
            f("offset.g", "Offset G", -1.0, 1.0, 0.0),
            f("offset.b", "Offset B", -1.0, 1.0, 0.0),
            f("offset.y", "Offset Y", -1.0, 1.0, 0.0),
            f("lift.r", "Lift R", -1.0, 1.0, 0.0),
            f("lift.g", "Lift G", -1.0, 1.0, 0.0),
            f("lift.b", "Lift B", -1.0, 1.0, 0.0),
            f("lift.y", "Lift Y", -1.0, 1.0, 0.0),
            f("gain.r", "Gain R", 0.0, 4.0, 1.0),
            f("gain.g", "Gain G", 0.0, 4.0, 1.0),
            f("gain.b", "Gain B", 0.0, 4.0, 1.0),
            f("gain.y", "Gain Y", 0.0, 4.0, 1.0),
            f("gamma.r", "Gamma R", 0.1, 4.0, 1.0),
            f("gamma.g", "Gamma G", 0.1, 4.0, 1.0),
            f("gamma.b", "Gamma B", 0.1, 4.0, 1.0),
            f("gamma.y", "Gamma Y", 0.1, 4.0, 1.0),
            // "x:y;x:y" control points, 0..1; empty = straight.
            text("curve.m", "Curve", false, "").sec("Curves"),
            text("curve.r", "Curve R", false, ""),
            text("curve.g", "Curve G", false, ""),
            text("curve.b", "Curve B", false, ""),
            f("saturation", "Saturation", 0.0, 4.0, 1.0).sec("Color"),
            f("temperature", "Temperature", -1.0, 1.0, 0.0),
            f("tint", "Tint", -1.0, 1.0, 0.0),
            text("look_lut", "Look LUT", false, "").sec("Look LUT"),
        ],
        presets: &[DEFAULT],
    },
    // ---- Transitions ----
    PluginInfo {
        id: "milan.tr.dissolve",
        name: "Dissolve",
        kind: PluginKind::Transition,
        group: "Dissolve",
        category: "Utility",
        desc: "Blends the outgoing and incoming clips",
        params: &[flag("additive", "Additive", false)],
        presets: &[preset("Cross Dissolve", &[]), preset("Additive Dissolve", &[("additive", 1.0)])],
    },
    PluginInfo {
        id: "milan.tr.fade_to_color",
        name: "Fade to Color",
        kind: PluginKind::Transition,
        group: "Dissolve",
        category: "Light",
        desc: "Fades out to a colour and in from it",
        params: &[color("color", "Color", BLACK), flag("additive", "Additive", false)],
        presets: &[
            preset("Dip to Black", &[]),
            preset("Dip to White", &[("color.r", 1.0), ("color.g", 1.0), ("color.b", 1.0)]),
            preset("Flash", &[("color.r", 1.0), ("color.g", 1.0), ("color.b", 1.0), ("additive", 1.0)]),
        ],
    },
    PluginInfo {
        id: "milan.tr.linear_wipe",
        name: "Linear Wipe",
        kind: PluginKind::Transition,
        group: "Wipe",
        category: "Utility",
        desc: "Wipes across at an angle",
        params: &[f("angle", "Angle", 0.0, 360.0, 0.0), f("feather", "Feather", 0.0, 0.5, 0.05)],
        presets: &[
            preset("Left to Right", &[]),
            preset("Right to Left", &[("angle", 180.0)]),
            preset("Top Down", &[("angle", 90.0)]),
            preset("Bottom Up", &[("angle", 270.0)]),
            preset("Soft Edge", &[("feather", 0.3)]),
        ],
    },
    PluginInfo {
        id: "milan.tr.push",
        name: "Push",
        kind: PluginKind::Transition,
        group: "Push",
        category: "Utility",
        desc: "Pushes the outgoing clip out of frame",
        params: &[choice("direction", "Direction", DIRECTIONS, 0)],
        presets: &[preset("Push Left", &[]), preset("Push Right", &[("direction", 1.0)]), preset("Push Up", &[("direction", 2.0)]), preset("Push Down", &[("direction", 3.0)])],
    },
    PluginInfo {
        id: "milan.tr.slide",
        name: "Slide",
        kind: PluginKind::Transition,
        group: "Push",
        category: "Utility",
        desc: "Slides the incoming clip in over the outgoing one",
        params: &[choice("direction", "Direction", DIRECTIONS, 0)],
        presets: &[preset("Slide Left", &[]), preset("Slide Right", &[("direction", 1.0)]), preset("Slide Up", &[("direction", 2.0)]), preset("Slide Down", &[("direction", 3.0)])],
    },
    PluginInfo {
        id: "milan.tr.iris",
        name: "Iris",
        kind: PluginKind::Transition,
        group: "Wipe",
        category: "Creative",
        desc: "Opens a shaped iris",
        params: &[choice("shape", "Shape", &["Circle", "Rectangle", "Diamond"], 0), f("feather", "Feather", 0.0, 0.5, 0.05)],
        presets: &[preset("Circle", &[]), preset("Rectangle", &[("shape", 1.0)]), preset("Diamond", &[("shape", 2.0)])],
    },
    PluginInfo {
        id: "milan.tr.clock_wipe",
        name: "Clock Wipe",
        kind: PluginKind::Transition,
        group: "Wipe",
        category: "Creative",
        desc: "Sweeps around the centre like a clock hand",
        params: &[f("feather", "Feather", 0.0, 0.5, 0.02)],
        presets: &[DEFAULT],
    },
    PluginInfo {
        id: "milan.tr.zoom",
        name: "Zoom",
        kind: PluginKind::Transition,
        group: "Zoom",
        category: "Blur",
        desc: "Zooms through the cut",
        params: &[f("amount", "Amount", 0.0, 4.0, 1.0)],
        presets: &[DEFAULT, preset("Subtle", &[("amount", 0.3)])],
    },
    // ---- Generators ----
    PluginInfo {
        id: "milan.gen.solid",
        name: "Solid Color",
        kind: PluginKind::Generator,
        group: "Color",
        category: "Color",
        desc: "Generates a solid colour",
        params: &[color("color", "Color", BLACK)],
        presets: &[
            preset("Black", &[]),
            preset("White", &[("color.r", 1.0), ("color.g", 1.0), ("color.b", 1.0)]),
            preset("Red", &[("color.r", 1.0)]),
            preset("Green", &[("color.g", 1.0)]),
            preset("Blue", &[("color.b", 1.0)]),
        ],
    },
    PluginInfo {
        id: "milan.gen.gradient",
        name: "Color Gradient",
        kind: PluginKind::Generator,
        group: "Color",
        category: "Color",
        desc: "Generates a linear, radial or rectangular gradient",
        params: &[choice("type", "Type", &["Linear", "Radial", "Rectangle"], 0), f("angle", "Angle", 0.0, 360.0, 0.0), text("stops", "Stops", false, "0:#000000ff;1:#ffffffff")],
        presets: &[
            preset("Linear Black to White", &[]),
            preset("Radial", &[("type", 1.0)]),
            Preset { name: "Sunset", vals: &[("angle", 90.0)], text: &[("stops", "0:#ff7a00ff;1:#401060ff")] },
        ],
    },
    PluginInfo {
        id: "milan.gen.checkerboard",
        name: "Checkerboard",
        kind: PluginKind::Generator,
        group: "Pattern",
        category: "Utility",
        desc: "Generates a checkerboard",
        params: &[f("size", "Cell size", 0.005, 0.5, 0.1), color("color1", "Color 1", WHITE), color("color2", "Color 2", BLACK)],
        presets: &[DEFAULT, preset("Small", &[("size", 0.025)])],
    },
    PluginInfo {
        id: "milan.gen.noise",
        name: "Noise Texture",
        kind: PluginKind::Generator,
        group: "Pattern",
        category: "Creative",
        desc: "Generates a noise texture",
        params: &[
            f("scale", "Scale", 0.005, 1.0, 0.1),
            f("octaves", "Octaves", 1.0, 8.0, 4.0),
            f("seed", "Seed", 0.0, 1000.0, 0.0),
            f("speed", "Animation speed", 0.0, 10.0, 0.0),
            color("color1", "Color 1", BLACK),
            color("color2", "Color 2", WHITE),
        ],
        presets: &[DEFAULT, preset("Animated", &[("speed", 1.0)]), preset("Fine", &[("scale", 0.02), ("octaves", 2.0)])],
    },
    PluginInfo {
        id: "milan.gen.text",
        name: "Titles & Text",
        kind: PluginKind::Generator,
        group: "Text",
        category: "Creative",
        desc: "Generates titles",
        params: &[
            text("text", "Text", true, "Sample Text").sec("Text"),
            ParamSpec { key: "font", name: "Font", section: "", kind: ParamKind::Font { default: "Arial" } },
            f("size", "Size", 0.01, 0.5, 0.1),
            color("color", "Text color", WHITE),
            f("x", "Location X", 0.0, 1.0, 0.5).sec("Placement"),
            f("y", "Location Y", 0.0, 1.0, 0.5),
            choice("align", "Alignment", &["Left", "Center", "Right"], 1),
            f("spacing", "Line spacing", 0.5, 3.0, 1.0),
            f("outline", "Outline width", 0.0, 0.05, 0.0).sec("Outline"),
            color("outline_color", "Outline color", BLACK),
            flag("shadow", "Shadow", false).sec("Shadow"),
            f("shadow_x", "Shadow offset X", -0.1, 0.1, 0.005),
            f("shadow_y", "Shadow offset Y", -0.1, 0.1, 0.005),
            f("shadow_blur", "Shadow blur", 0.0, 0.05, 0.005),
            color("shadow_color", "Shadow color", [0.0, 0.0, 0.0, 0.6]),
            color("background", "Background", [0.0, 0.0, 0.0, 0.0]).sec("Background"),
        ],
        presets: &[
            DEFAULT,
            preset("Lower Third", &[("x", 0.06), ("y", 0.82), ("align", 0.0), ("size", 0.06), ("shadow", 1.0)]),
            preset("Title", &[("size", 0.14), ("outline", 0.004)]),
            preset("Subtitle", &[("y", 0.9), ("size", 0.05), ("outline", 0.003)]),
        ],
    },
];

pub fn find(id: &str) -> Option<&'static PluginInfo> {
    PLUGINS.iter().find(|p| p.id == id)
}

pub fn catalog(kind: PluginKind) -> impl Iterator<Item = &'static PluginInfo> {
    PLUGINS.iter().filter(move |p| p.kind == kind)
}

/// A new instance with `preset`'s values as single keys at t = 0.
pub fn new_fx(info: &PluginInfo, preset: Option<&Preset>) -> Fx {
    let key = |v| Keyframes { keys: vec![Keyframe { t: 0.0, v, curve: Curve::Linear }] };
    Fx {
        id: uuid::Uuid::new_v4().to_string(),
        plugin: info.id.into(),
        enabled: true,
        params: preset.map_or(&[][..], |p| p.vals).iter().map(|&(k, v)| (k.to_string(), key(v))).collect(),
        text: preset.map_or(&[][..], |p| p.text).iter().map(|&(k, v)| (k.to_string(), v.to_string())).collect(),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    F(f32),
    C([f32; 4]),
    T(String),
}

/// Every parameter of one plug-in resolved to a plain value at one time.
#[derive(Clone, Debug, PartialEq)]
pub struct FxEval {
    pub id: &'static str,
    pub vals: Vec<(&'static str, Val)>,
}

impl Hash for FxEval {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.id.hash(h);
        for (k, v) in &self.vals {
            k.hash(h);
            match v {
                Val::F(x) => x.to_bits().hash(h),
                Val::C(c) => c.map(f32::to_bits).hash(h),
                Val::T(s) => s.hash(h),
            }
        }
    }
}

impl FxEval {
    fn get(&self, k: &str) -> Option<&Val> {
        self.vals.iter().find(|v| v.0 == k).map(|v| &v.1)
    }

    /// A Float, Choice or Bool value (0 if absent).
    pub fn f(&self, k: &str) -> f32 {
        match self.get(k) {
            Some(Val::F(v)) => *v,
            _ => 0.0,
        }
    }

    /// A colour, straight (not premultiplied) RGBA 0..1.
    pub fn c(&self, k: &str) -> [f32; 4] {
        match self.get(k) {
            Some(Val::C(c)) => *c,
            _ => [0.0; 4],
        }
    }

    pub fn t(&self, k: &str) -> &str {
        match self.get(k) {
            Some(Val::T(s)) => s,
            _ => "",
        }
    }

    pub fn on(&self, k: &str) -> bool {
        self.f(k) >= 0.5
    }

    /// A Choice index.
    pub fn i(&self, k: &str) -> usize {
        self.f(k).round().max(0.0) as usize
    }
}

fn resolve(info: &'static PluginInfo, num: impl Fn(&str) -> Option<f32>, txt: impl Fn(&str) -> Option<String>) -> FxEval {
    let vals = info
        .params
        .iter()
        .map(|p| {
            let v = match p.kind {
                ParamKind::Float { min, max, default } => Val::F(num(p.key).map_or(default, |v| v.clamp(min, max))),
                ParamKind::Choice { options, default } => Val::F(num(p.key).map_or(default as f32, |v| v.round().clamp(0.0, options.len() as f32 - 1.0))),
                ParamKind::Bool { default } => Val::F(num(p.key).map_or(default, |v| v >= 0.5) as u8 as f32),
                ParamKind::Color { default } => Val::C(std::array::from_fn(|i| num(&format!("{}.{}", p.key, ["r", "g", "b", "a"][i])).map_or(default[i], |v| v.clamp(0.0, 1.0)))),
                ParamKind::Text { default, .. } | ParamKind::Font { default } => Val::T(txt(p.key).unwrap_or_else(|| default.to_string())),
            };
            (p.key, v)
        })
        .collect();
    FxEval { id: info.id, vals }
}

/// `fx` at `t` (its own time: event-relative for event FX). None for a plug-in this build does not know: it is bypassed.
pub fn eval(fx: &Fx, t: f64) -> Option<FxEval> {
    let info = find(&fx.plugin)?;
    let num = |k: &str| fx.params.get(k).filter(|kf| !kf.keys.is_empty()).map(|kf| kf.at(t));
    Some(resolve(info, num, |k| fx.text.get(k).cloned()))
}

/// A transition's settings (not keyframed). None for an unknown plug-in (the mix falls back to a dissolve).
pub fn eval_transition(tr: &Transition) -> Option<FxEval> {
    let info = find(&tr.plugin).filter(|i| i.kind == PluginKind::Transition)?;
    Some(resolve(info, |k| tr.params.get(k).copied(), |_| None))
}

/// A built-in preset (None = the defaults) as it evaluates.
pub fn preset_eval(info: &'static PluginInfo, preset: Option<&Preset>) -> FxEval {
    let num = |k: &str| preset?.vals.iter().find(|v| v.0 == k).map(|v| v.1);
    let txt = |k: &str| preset?.text.iter().find(|v| v.0 == k).map(|v| v.1.to_string());
    resolve(info, num, txt)
}

/// A new instance with a user preset's values (single keys at t = 0).
pub fn new_user_fx(info: &PluginInfo, p: &UserPreset) -> Fx {
    let key = |v| Keyframes { keys: vec![Keyframe { t: 0.0, v, curve: Curve::Linear }] };
    Fx { params: p.params.iter().map(|(k, &v)| (k.clone(), key(v))).collect(), text: p.text.clone(), ..new_fx(info, None) }
}

/// `fx`'s values at `t` as a user preset named `name`.
pub fn user_preset_of(fx: &Fx, name: &str, t: f64) -> UserPreset {
    let params = fx.params.iter().filter(|(_, k)| !k.keys.is_empty()).map(|(k, kf)| (k.clone(), kf.at(t))).collect();
    UserPreset { plugin: fx.plugin.clone(), name: name.into(), params, text: fx.text.clone() }
}

/// A preset the user saved from the FX window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserPreset {
    pub plugin: String,
    pub name: String,
    #[serde(default)]
    pub params: BTreeMap<String, f32>,
    #[serde(default)]
    pub text: BTreeMap<String, String>,
}

fn presets_path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("APPDATA")?).join("MilanPro").join("presets.json"))
}

/// Every user preset; none when the file is missing or unreadable.
pub fn load_user_presets() -> Vec<UserPreset> {
    presets_path().and_then(|p| read_presets(&p).ok()).unwrap_or_default()
}

/// Add `p`, replacing a preset of the same plug-in and name.
pub fn save_user_preset(p: UserPreset) -> io::Result<()> {
    write_preset(&presets_path().ok_or_else(|| io::Error::other("APPDATA is not set"))?, p)
}

/// Remove the user preset `name` of `plugin` (the FX window's delete ✕).
pub fn delete_user_preset(plugin: &str, name: &str) -> io::Result<()> {
    rewrite(&presets_path().ok_or_else(|| io::Error::other("APPDATA is not set"))?, |all| all.retain(|q| !(q.plugin == plugin && q.name == name)))
}

/// A missing file is empty; a damaged one is an error, so a save never overwrites presets it could not read.
fn read_presets(path: &Path) -> io::Result<Vec<UserPreset>> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(serde_json::from_str(&s)?),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

fn write_preset(path: &Path, p: UserPreset) -> io::Result<()> {
    rewrite(path, |all| {
        all.retain(|q| !(q.plugin == p.plugin && q.name == p.name));
        all.push(p);
    })
}

fn rewrite(path: &Path, f: impl FnOnce(&mut Vec<UserPreset>)) -> io::Result<()> {
    let mut all = read_presets(path)?;
    f(&mut all);
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&all)?)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_are_unique_and_presets_and_defaults_are_valid() {
        let ids: HashSet<&str> = PLUGINS.iter().map(|p| p.id).collect();
        assert_eq!(ids.len(), PLUGINS.len());
        for p in PLUGINS {
            let keys: HashSet<&str> = p.params.iter().map(|s| s.key).collect();
            assert_eq!(keys.len(), p.params.len(), "{}", p.id);
            assert!(!p.presets.is_empty(), "{}", p.id);
            for s in p.params {
                match s.kind {
                    ParamKind::Float { min, max, default } => assert!(min <= default && default <= max, "{} {}", p.id, s.key),
                    ParamKind::Choice { options, default } => assert!(default < options.len(), "{} {}", p.id, s.key),
                    ParamKind::Color { default } => assert!(default.iter().all(|v| (0.0..=1.0).contains(v))),
                    _ => {}
                }
            }
            for pr in p.presets {
                for k in pr.vals.iter().map(|v| v.0).chain(pr.text.iter().map(|v| v.0)) {
                    let base = k.rsplit_once('.').filter(|(_, c)| ["r", "g", "b", "a"].contains(c)).map(|(b, _)| b);
                    let ok = keys.contains(k) || base.is_some_and(|b| p.params.iter().any(|s| s.key == b && matches!(s.kind, ParamKind::Color { .. })));
                    assert!(ok, "{} preset {}: {k}", p.id, pr.name);
                }
            }
        }
    }

    #[test]
    fn eval_defaults_unknown_and_keys() {
        let info = find("milan.fx.vignette").unwrap();
        let mut fx = new_fx(info, None);
        let e = eval(&fx, 0.0).unwrap();
        assert_eq!(e, preset_eval(info, None));
        assert_eq!((e.f("amount"), e.f("size"), e.c("color")), (0.5, 0.7, BLACK));
        // An empty key list is a missing key, not 0.
        fx.params.insert("amount".into(), Keyframes::default());
        assert_eq!(eval(&fx, 0.0).unwrap().f("amount"), 0.5);
        fx.params.insert("amount".into(), Keyframes { keys: vec![Keyframe { t: 0.0, v: 0.0, curve: Curve::Linear }, Keyframe { t: 2.0, v: 1.0, curve: Curve::Linear }] });
        fx.params.insert("color.r".into(), Keyframes { keys: vec![Keyframe { t: 0.0, v: 7.0, curve: Curve::Linear }] });
        let e = eval(&fx, 1.0).unwrap();
        assert_eq!((e.f("amount"), e.c("color")[0]), (0.5, 1.0)); // keyframed; clamped to the range
        assert!(eval(&Fx { plugin: "ofx:nope".into(), ..fx.clone() }, 0.0).is_none());
        let tr = Transition { plugin: "milan.tr.linear_wipe".into(), params: BTreeMap::from([("angle".to_string(), 90.0)]) };
        assert_eq!(eval_transition(&tr).unwrap().f("angle"), 90.0);
        assert!(eval_transition(&Transition { plugin: "milan.fx.invert".into(), params: BTreeMap::new() }).is_none());
        // Text and presets
        let text = find("milan.gen.text").unwrap();
        let e = eval(&new_fx(text, Some(&text.presets[1])), 0.0).unwrap();
        assert_eq!((e.t("text"), e.i("align"), e.on("shadow")), ("Sample Text", 0, true));
    }

    #[test]
    fn user_preset_round_trips_and_damage_is_kept() {
        let dir = std::env::temp_dir().join(format!("milan-presets-{}", std::process::id()));
        let path = dir.join("presets.json");
        let _ = std::fs::remove_dir_all(&dir);
        let p = UserPreset { plugin: "milan.fx.levels".into(), name: "Mine".into(), params: BTreeMap::from([("gamma".into(), 1.5)]), text: BTreeMap::new() };
        assert_eq!(user_preset_of(&new_user_fx(find("milan.fx.levels").unwrap(), &p), "Mine", 3.0), p);
        write_preset(&path, p.clone()).unwrap();
        write_preset(&path, UserPreset { name: "Other".into(), ..p.clone() }).unwrap();
        write_preset(&path, UserPreset { params: BTreeMap::from([("gamma".into(), 2.0)]), ..p.clone() }).unwrap(); // replaces "Mine"
        let back = read_presets(&path).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back.iter().find(|q| q.name == "Mine").unwrap().params["gamma"], 2.0);
        rewrite(&path, |all| all.retain(|q| q.name != "Other")).unwrap();
        assert_eq!(read_presets(&path).unwrap().len(), 1);
        std::fs::write(&path, "{ damaged").unwrap();
        assert!(write_preset(&path, p).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ damaged");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
