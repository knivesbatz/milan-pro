//! The messages between Milan Pro and `milan-plugin-host.exe` (shared by both binaries): length-prefixed JSON over the
//! named pipe, one reply per request. Pictures do not travel here: they are in shared memory (`Render::shm`).
#![allow(dead_code)]
use serde::{Deserialize, Serialize};
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
    #[serde(default)]
    pub min: f64,
    #[serde(default)]
    pub max: f64,
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
