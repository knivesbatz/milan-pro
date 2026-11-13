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
    /// 32-bit float RGBA, else 8-bit.
    pub float: bool,
    /// Picture pixels per project pixel (Draft and Preview quality render smaller).
    pub scale: f64,
    pub par: f64,
    pub fps: f64,
    /// The shared memory (`Local\...` name) and its size; the source starts at 0, the output at `out`.
    pub shm: String,
    pub shm_size: u64,
    pub out: u64,
    pub params: Vec<(String, PVal)>,
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
    Rendered,
    Error(String),
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
    /// The context Milan uses ("" when it supports none of them).
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub params: Vec<ParamDesc>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
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
