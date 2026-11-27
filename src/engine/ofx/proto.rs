//! The messages between Milan Pro and `milan-plugin-host.exe` (shared by both binaries): length-prefixed JSON over the
//! named pipe, one reply per request. Pictures do not travel here: they are in shared memory (`Render::shm`).
#![allow(dead_code)]
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::path::PathBuf;

/// The largest message either side accepts (a describe of a big bundle is well under it).
pub const MAX_MSG: u32 = 64 << 20;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Req {
    /// Load the binary (once per host) and run each plug-in's load action.
    Load { binary: PathBuf },
    /// Describe plug-in `index` of the loaded binary (describe, then describeInContext: Filter, else General).
    Describe { index: u32 },
    Render(Render),
    /// A push button of the instance was clicked (its name), or "" to only create the instance and read its values.
    /// No picture: `Render::shm` is unused.
    Button(Render, String),
    /// Testing aid: the host panics (the R32 crash test without a plug-in).
    Panic,
    /// R34a/R35: the loaded binary is a VST3 or CLAP module (by its extension): list its audio effects.
    AScan,
    /// Make (or reset) an audio plug-in instance without processing: its latency and tail, its values.
    AOpen(AInstance),
    /// Process `frames` interleaved stereo 32-bit float frames: the input at the start of the shared memory, the output
    /// right after it (`frames * 2 * 4` bytes each).
    AProcess(AProcess),
    /// The instance's state (component and controller), for the project and `.vstpreset` files.
    AGetState(AInstance),
    /// Load a state into the instance (a `.vstpreset`, a program): the values afterwards come back.
    ASetState(AInstance, AState),
    /// R34b: the plug-in's own editor window, owned by the host.
    AEditor(AInstance, EditorOp),
    /// Forget an instance (its FX was removed).
    AClose(String),
    /// R35 research spike: enumerate the DirectX Media Object audio effects and try one; the findings are in
    /// `Outcome::messages`. Read-only; nothing is kept.
    DmoProbe,
    /// Can this process make a Vulkan device of its own (as Spektrafilm does)? The answer is in `Outcome::messages`.
    GpuCheck,
    /// A4: an AE-format instance's sequence data, flattened (`Outcome::blob`; None when the host has no such instance).
    AexFlatten { index: u32, instance: String },
    /// A5: the answer to `Reply::NeedFrame` during a render: true when Milan wrote the frame after the output in shared
    /// memory, false for none (the plug-in gets a transparent one).
    Frame(bool),
    Quit,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Render {
    pub index: u32,
    /// The instance this frame belongs to (Milan's FX id): it keeps its own parameter state in the host.
    pub instance: String,
    /// Plug-in time (frames).
    pub time: f64,
    pub w: u32,
    pub h: u32,
    pub depth: Depth,
    /// Pictures before the output: 0 for a generator, 1 for a filter (Source), 2 for a transition (SourceFrom,
    /// SourceTo), one after the other from the start of the shared memory.
    pub inputs: u32,
    /// Milan already holds this instance's values (a saved, copied or edited FX): they replace whatever the plug-in
    /// sets up in its create instance action. False for a new FX: the plug-in's own starting values (saved defaults,
    /// a random seed) stand and are all reported back (`Outcome::initial`).
    pub restored: bool,
    /// Picture pixels per project pixel (Draft and Preview quality render smaller).
    pub scale: f64,
    pub par: f64,
    pub fps: f64,
    /// The shared memory (`Local\...` name) and its size; the inputs start at 0, the output at `out`.
    pub shm: String,
    pub shm_size: u64,
    pub out: u64,
    pub params: Vec<(String, PVal)>,
    /// A4: an AE-format instance's saved data (base64 by key: "seq" its flattened sequence data), sent when the host
    /// does not have the instance yet.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub blob: BTreeMap<String, String>,
}

/// The pixel depth of the pictures in shared memory (RGBA, premultiplied).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Depth {
    #[default]
    Byte,
    Short,
    Half,
    Float,
}

impl Depth {
    /// Bytes per pixel.
    pub fn bpp(self) -> usize {
        match self {
            Depth::Byte => 4,
            Depth::Short | Depth::Half => 8,
            Depth::Float => 16,
        }
    }

    /// The OFX name ("OfxBitDepthByte").
    pub fn ofx(self) -> &'static str {
        match self {
            Depth::Byte => "OfxBitDepthByte",
            Depth::Short => "OfxBitDepthShort",
            Depth::Half => "OfxBitDepthHalf",
            Depth::Float => "OfxBitDepthFloat",
        }
    }

    /// The depth Milan uses with a plug-in that takes `depths`: float, else 8-bit (Milan's own), else 16-bit.
    pub fn pick(depths: &[String]) -> Option<Depth> {
        [Depth::Float, Depth::Byte, Depth::Short, Depth::Half].into_iter().find(|d| depths.iter().any(|x| x == d.ofx()))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PVal {
    N(Vec<f64>),
    S(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Reply {
    /// The binary's plug-in count.
    Loaded(u32),
    Described(Box<PluginDesc>),
    /// A frame was rendered, or a button's action ran.
    Rendered(Outcome),
    Error(String),
    /// R34a/R35: the audio effects of the loaded module.
    AScanned(Vec<AudioDesc>),
    /// A5: in the middle of a render, an AE-format effect checked out layer parameter `layer` at `offset` frames from
    /// the frame being rendered: Milan writes that frame after the output in shared memory and answers `Req::Frame`;
    /// the render's own reply follows.
    NeedFrame { layer: i32, offset: f64 },
    /// An audio instance did something (opened, processed, loaded a state, its editor changed).
    AOut(AOut),
    AStateIs(AState),
}

// ---------------------------------------------------------------- audio plug-ins (R34a, R34b, R35)

/// One audio effect of a VST3 or CLAP module, as scanned.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AudioDesc {
    /// "vst3" or "clap".
    pub format: String,
    /// VST3: the class id, 32 hex digits (as `.vstpreset` and `moduleinfo.json` spell it); CLAP: the plug-in id.
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub vendor: String,
    #[serde(default)]
    pub version: String,
    /// VST3 sub categories ("Fx|Dynamics"); CLAP features ("audio-effect|compressor").
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub sdk: String,
    #[serde(default)]
    pub params: Vec<AParam>,
    #[serde(default)]
    pub has_editor: bool,
    /// Its latency (samples at 48 kHz) and tail when first set up.
    #[serde(default)]
    pub latency: u32,
    #[serde(default)]
    pub tail: u32,
    /// Why it cannot run (not stereo, no audio buses, failed to set up); "" when it can.
    #[serde(default)]
    pub error: String,
}

/// One parameter of an audio effect. VST3 values are normalized 0..=1 (as the API has them); CLAP values are plain.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AParam {
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub units: String,
    /// 0: continuous; n: n + 1 discrete steps (VST3 stepCount; a stepped CLAP range).
    #[serde(default)]
    pub steps: i32,
    #[serde(default)]
    pub min: f64,
    #[serde(default)]
    pub max: f64,
    #[serde(default)]
    pub default: f64,
    #[serde(default)]
    pub automatable: bool,
    #[serde(default)]
    pub hidden: bool,
    /// The plug-in's own bypass switch (Milan has its own; this one is not shown).
    #[serde(default)]
    pub bypass: bool,
    #[serde(default)]
    pub readonly: bool,
    /// The names of the steps (a list parameter, a program change parameter's programs), when the plug-in gives them.
    #[serde(default)]
    pub options: Vec<String>,
    /// VST3 kIsProgramChange: selects a program of the plug-in's program list.
    #[serde(default)]
    pub program: bool,
}

/// An instance of an audio effect in the host: what is needed to make it (or make it again after the host died).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AInstance {
    /// The effect (`AudioDesc::id`).
    pub id: String,
    /// Milan's FX id.
    pub instance: String,
    pub rate: f64,
    /// The largest block that will be processed.
    pub block: u32,
    /// A saved state to load when the instance is made (the project's).
    #[serde(default)]
    pub state: Option<AState>,
    /// Parameter values (by id) to set: all of them when the instance is made, the changed ones afterwards.
    #[serde(default)]
    pub params: Vec<(u32, f64)>,
    /// Start afresh (silent delay lines): the chain was idle.
    #[serde(default)]
    pub reset: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AProcess {
    pub inst: AInstance,
    pub frames: u32,
    /// Timeline time of the first frame (seconds) and whether the transport runs.
    pub time: f64,
    pub playing: bool,
    pub shm: String,
    pub shm_size: u64,
}

/// A plug-in's state: its processor's (component) and its controller's bytes, base64.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AState {
    pub component: String,
    #[serde(default)]
    pub controller: String,
}

/// What an audio instance reports with a reply.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AOut {
    pub latency: u32,
    #[serde(default)]
    pub tail: u32,
    /// Parameters the plug-in changed itself (its editor, a program change), by id.
    #[serde(default)]
    pub edits: Vec<(u32, f64)>,
    /// Begin (true) and end (false) of a control gesture in the editor, by parameter id: Touch automation.
    #[serde(default)]
    pub gestures: Vec<(u32, bool)>,
    /// Every parameter's value, after a state was loaded or the instance was made.
    #[serde(default)]
    pub values: Option<Vec<(u32, f64)>>,
    #[serde(default)]
    pub editor: Option<EditorState>,
    #[serde(default)]
    pub messages: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EditorOp {
    /// Open the editor in a top-level window at (x, y); `offscreen`: never shown on a monitor nor activated (tests).
    Open { x: i32, y: i32, offscreen: bool, title: String },
    Close,
    /// Nothing: the reply says how it is (and brings the edits made in it).
    Poll,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorState {
    pub open: bool,
    pub w: u32,
    pub h: u32,
    pub hwnd: u64,
}

pub fn b64(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub fn unb64(s: &str) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s.trim()).unwrap_or_default()
}

/// What the plug-in did besides the picture.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    /// Values the plug-in set itself (paramSetValue), as stored in the host afterwards.
    #[serde(default)]
    pub edits: Vec<(String, PVal)>,
    /// `edits` are a new instance's starting values (every parameter): they fill in what Milan does not hold yet.
    #[serde(default)]
    pub initial: bool,
    /// Every parameter's hidden / enabled / label / choice options, when they changed since the last reply.
    #[serde(default)]
    pub state: Option<Vec<ParamState>>,
    /// The plug-in's messages during this request, as it worded them ("Error: [id] text").
    #[serde(default)]
    pub messages: Vec<String>,
    /// A4: an AE-format instance's flattened data (`Req::AexFlatten`), by key.
    #[serde(default)]
    pub blob: Option<BTreeMap<String, String>>,
}

/// What a plug-in can change about a parameter at run time.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ParamState {
    pub name: String,
    pub secret: bool,
    pub enabled: bool,
    pub label: String,
    #[serde(default)]
    pub options: Vec<String>,
}

/// One plug-in as its describe actions set it up.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PluginDesc {
    pub index: u32,
    pub id: String,
    pub major: u32,
    pub minor: u32,
    pub label: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub contexts: Vec<String>,
    #[serde(default)]
    pub depths: Vec<String>,
    /// The context Milan uses: Filter, else General, Transition or Generator ("" when it supports none of them).
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub params: Vec<ParamDesc>,
    /// A1-A5: an AE-format effect (`aex:` id): what its PiPL and its setup in the host said. None for OFX.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aex: Option<AexInfo>,
}

/// What is known about an AE-format effect (`docs/AE_PREMIERE_PLUGINS.md` §4.1, §4.7).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AexInfo {
    /// The stable id (`AE_Effect_Match_Name`).
    pub match_name: String,
    /// The exported entry point the PiPL names.
    pub entry: String,
    /// Which effect PiPL of the file (0-based, in resource order).
    pub pipl: u32,
    /// The API version it was built for, and its own packed version.
    pub spec: (u16, u16),
    pub version: u32,
    /// Its flags: from the PiPL, then as its global setup set them.
    pub out_flags: u32,
    pub out_flags2: u32,
    /// Why Milan does not load it ("" when it may): Adobe's own effects.
    pub blocked: String,
    /// Why it cannot run (audio, no entry point, its setup failed...): "" when it can.
    pub unsupported: String,
    /// Went through global and parameter setup in a plug-in host (A2); false: known from its PiPL only.
    pub described: bool,
    /// The capability line: what it uses that Milan gives only in part.
    pub caps: Vec<String>,
    /// Suites it asked for that Milan does not serve ("name v1"), and utility callbacks it called that Milan does not
    /// serve: the single most useful compatibility datum (plan §4.7).
    pub refused: Vec<String>,
    /// Its own message from setup, verbatim.
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParamDesc {
    pub name: String,
    pub label: String,
    /// The OFX type without its prefix: "Double", "Integer", "Boolean", "Choice", "RGB", "RGBA", "Double2D", "Double3D",
    /// "Integer2D", "Integer3D", "String", "Custom", "Group", "Page", "PushButton".
    pub kind: String,
    #[serde(default)]
    pub default: Vec<f64>,
    #[serde(default)]
    pub text: String,
    /// The display range (kOfxParamPropDisplayMin/Max, else Min/Max): the slider's.
    #[serde(default)]
    pub min: f64,
    #[serde(default)]
    pub max: f64,
    /// The hard range (kOfxParamPropMin/Max): values are kept within it, never within the display range.
    #[serde(default = "lowest")]
    pub vmin: f64,
    #[serde(default = "highest")]
    pub vmax: f64,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub parent: String,
    #[serde(default)]
    pub secret: bool,
    #[serde(default)]
    pub multiline: bool,
    #[serde(default)]
    pub hint: String,
    /// kOfxParamPropEnabled as described (greyed when false).
    #[serde(default = "yes")]
    pub enabled: bool,
    /// A group that starts open.
    #[serde(default = "yes")]
    pub open: bool,
    /// A page's children, in its order.
    #[serde(default)]
    pub children: Vec<String>,
}

fn yes() -> bool {
    true
}

fn lowest() -> f64 {
    -f64::MAX
}

fn highest() -> f64 {
    f64::MAX
}

impl Default for ParamDesc {
    fn default() -> Self {
        serde_json::from_str(r#"{"name":"","label":"","kind":""}"#).expect("the defaults")
    }
}

pub fn write_msg<T: Serialize>(w: &mut impl Write, m: &T) -> io::Result<()> {
    let b = serde_json::to_vec(m)?;
    let mut out = (b.len() as u32).to_le_bytes().to_vec();
    out.extend(b);
    w.write_all(&out)?;
    w.flush()
}

pub fn read_msg<T: for<'de> Deserialize<'de>>(r: &mut impl Read) -> io::Result<T> {
    let mut n = [0u8; 4];
    r.read_exact(&mut n)?;
    let n = u32::from_le_bytes(n);
    if n > MAX_MSG {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "message too large"));
    }
    let mut b = vec![0; n as usize];
    r.read_exact(&mut b)?;
    Ok(serde_json::from_slice(&b)?)
}
