//! R40b: the float pipeline, Project Properties' "32-bit floating point" pixel formats. Pictures are `WImage`s:
//! premultiplied RGBA in f32, in the project's working space (`Working`): Rec.709 coded as video codes it (compositing
//! gamma 2.222), Rec.709 linear light (1.000), or with an ACES view transform the ACES color space (ACES2065-1, ACEScg,
//! ACEScc or ACEScct), HDR sources kept as scene light with BT.2408's reference white (203 cd/m²) at 1.0.
//!
//! - In: each source is decoded at 16 bits (`rgba64le`) or, a float one (OpenEXR, float TIFF), in 32-bit float
//!   (`gbrapf32le`), and brought into the working space by its `Input` (`plan::media_ref` makes one per source). Its
//!   frames travel through the decoders and caches as `ColorImage`s 2 or 4 times as wide (`Depth::factor`), so that
//!   machinery is the 8-bit one; `Input::convert` reads them.
//! - Compose: `compose.rs` (the CPU, the reference) and `engine/gpu/wide.rs` (`Rgba16Float` textures).
//! - Out: the view transform (`Out`) for the render (`to_out16`, 16 bits to the encoder) and, tone-mapped to SDR when
//!   it is HDR, for the Video Preview, scopes and snapshots (`to_preview`).
//!
//! Video FX run on the working values, as VEGAS hands them to its FX. Those with a float kernel (the colour FX, Color
//! Grading's colour steps, Gaussian Blur, masks, dissolves, OFX plug-ins at their float depth) keep full precision; the
//! others run on an 8-bit copy (`via8`) and keep the part under one 8-bit step where they left a pixel as it was, as
//! VEGAS runs an 8-bit-only plug-in in a float project.
use eframe::egui::{Color32, ColorImage};
use std::path::PathBuf;
use std::sync::OnceLock;
use crate::project::pixel::{AcesSpace, Gamma, PixelFormat, ViewTransform};
use crate::project::{ColorManagement, Hdr, MediaItem, ProjectSettings};

pub mod compose;
#[cfg(test)]
mod tests;

pub type M3 = [[f32; 3]; 3];

/// A picture in the working space: premultiplied RGBA, f32.
#[derive(Clone, Debug, PartialEq)]
pub struct WImage {
    pub size: [usize; 2],
    pub px: Vec<[f32; 4]>,
}

impl WImage {
    pub fn new(size: [usize; 2], c: [f32; 4]) -> WImage {
        WImage { size, px: vec![c; size[0] * size[1]] }
    }
}

/// How the working values code light.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enc {
    /// As video codes it: linear light to the power 1 / 2.222 (VEGAS's compositing gamma 2.222).
    Video,
    Linear,
    /// ACEScc and ACEScct.
    Cc,
    Cct,
}

/// RGB primaries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prim {
    Rec709,
    Rec2020,
    /// ACES AP0 (ACES2065-1) and AP1 (ACEScg, ACEScc, ACEScct), white D60.
    Ap0,
    Ap1,
}

/// What the picture is shown and rendered as: the view transform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Out {
    /// Rec.709 as video codes it: no view transform.
    Video,
    /// The ACES output transforms for SDR displays: sRGB, Rec.709 (BT.1886).
    AcesSrgb,
    AcesRec709,
    /// Rec.2020 PQ (HDR10) and HLG for a 1000 cd/m² display.
    Pq,
    Hlg,
    /// A .cube LUT from the working values to Rec.709 (`Working::lut`).
    Lut,
}

impl Out {
    pub fn hdr(self) -> bool {
        matches!(self, Out::Pq | Out::Hlg)
    }
}

/// The working space of a float project.
#[derive(Clone, Debug, PartialEq)]
pub struct Working {
    pub enc: Enc,
    pub prim: Prim,
    pub out: Out,
    /// HDR (PQ, HLG) and wide-gamut sources come in as scene light in their own primaries (an ACES view transform or a
    /// LUT is on); otherwise R40a's view transform brings them to SDR Rec.709 at decode, as in an 8-bit project.
    pub hdr_in: bool,
    pub lut: Option<PathBuf>,
    /// R40a's Color management (SDR working spaces).
    pub cm: ColorManagement,
}

/// HDR reference white (BT.2408), the working spaces' 1.0, cd/m².
pub const REF_WHITE: f32 = 203.0;
/// The peak of the HDR view transforms, cd/m².
pub const HDR_PEAK: f32 = 1000.0;

impl Working {
    /// An 8-bit project's pictures as a float one sees them (R40b: an 8-bit project into an HDR template).
    pub fn sdr(cm: ColorManagement) -> Working {
        Working { enc: Enc::Video, prim: Prim::Rec709, out: Out::Video, hdr_in: false, lut: None, cm }
    }

    /// The working space of `s`; None for an 8-bit project.
    pub fn of(s: &ProjectSettings) -> Option<Working> {
        let p = &s.pixel;
        if p.format == PixelFormat::Bit8 {
            return None;
        }
        let gamma = if p.gamma == Gamma::Linear { Enc::Linear } else { Enc::Video };
        let (enc, prim) = match (p.aces(), p.aces_space) {
            (false, _) => (gamma, Prim::Rec709),
            (true, AcesSpace::Aces2065) => (Enc::Linear, Prim::Ap0),
            (true, AcesSpace::AcesCg) => (Enc::Linear, Prim::Ap1),
            (true, AcesSpace::AcesCc) => (Enc::Cc, Prim::Ap1),
            (true, AcesSpace::AcesCct) => (Enc::Cct, Prim::Ap1),
        };
        let out = match p.view() {
            ViewTransform::Off => Out::Video,
            ViewTransform::Srgb => Out::AcesSrgb,
            ViewTransform::Rec709 => Out::AcesRec709,
            ViewTransform::Pq1000 => Out::Pq,
            ViewTransform::Hlg1000 => Out::Hlg,
            ViewTransform::Lut => Out::Lut,
        };
        let lut = if out == Out::Lut { p.view_lut.clone() } else { None };
        Some(Working { enc, prim, out, hdr_in: p.aces(), lut, cm: s.color_management })
    }

    /// Working values -> linear light.
    pub fn decode(&self, v: f32) -> f32 {
        match self.enc {
            Enc::Video => video_to_lin(v),
            Enc::Linear => v,
            Enc::Cc => cc_to_lin(v),
            Enc::Cct => cct_to_lin(v),
        }
    }

    /// Linear light -> working values.
    pub fn encode(&self, v: f32) -> f32 {
        match self.enc {
            Enc::Video => lin_to_video(v),
            Enc::Linear => v,
            Enc::Cc => lin_to_cc(v),
            Enc::Cct => lin_to_cct(v),
        }
    }

    /// The darkest and brightest values a blend keeps: video-coded pictures stay within 0..1 as 8-bit ones do; linear
    /// light keeps its highlights; the log encodings go below 0.
    pub fn range(&self) -> (f32, f32) {
        match self.enc {
            Enc::Video => (0.0, 1.0),
            // Half floats' largest (what the GPU keeps).
            Enc::Linear => (0.0, 65504.0),
            Enc::Cc | Enc::Cct => (-65504.0, 65504.0),
        }
    }

    /// How an SDR Rec.709 picture (8-bit generated media, a colour, an 8-bit source) comes in.
    pub fn sdr_input(&self, depth: Depth) -> Input {
        let tf = match self.out {
            // An ACES SDR view: the inverse of the view, so SDR media shows as it is coded (VEGAS's default input
            // transform for video is the view's own).
            Out::AcesSrgb | Out::AcesRec709 if self.hdr_in => Tf::InvView(self.out),
            _ if self.enc == Enc::Video && self.prim == Prim::Rec709 => Tf::Code,
            _ => Tf::Video,
        };
        let m = if tf == Tf::Code { None } else { conv(Prim::Rec709, self.prim) };
        Input { depth, tf, m, enc: self.enc }
    }

    /// A Rec.709 colour as 8-bit video codes it, in working values.
    pub fn color8(&self, c: [u8; 3]) -> [f32; 3] {
        self.sdr_input(Depth::B8).pixel(c.map(|v| v as f32 / 255.0))
    }

    /// How source `m` decodes and comes in (`plan::media_ref`): its `Input`, the filters ahead of the scaler, and the
    /// scaler's matrix. `pre8` and `matrix8` are what an 8-bit project decodes it with.
    pub fn source(&self, m: &MediaItem, pre8: &str, matrix8: Option<&'static str>) -> (Input, String, Option<&'static str>) {
        let float = matches!(m.codec.as_deref(), Some("exr" | "hdr")) || m.transfer.as_deref() == Some("linear") || m.bit_depth.is_some_and(|b| b > 16);
        let deep = m.bit_depth.is_none_or(|b| b > 8);
        let linear = matches!(m.codec.as_deref(), Some("exr" | "hdr")) || m.transfer.as_deref() == Some("linear");
        let to = |p| conv(p, self.prim);
        let input = |depth, tf, m| Input { depth, tf, m, enc: self.enc };
        if self.hdr_in {
            // Scene light: no filters, the source's own matrix and transfer.
            return match (m.hdr(), linear) {
                (_, true) => (input(Depth::F32, Tf::Linear, to(Prim::Rec709)), String::new(), None),
                (Some(Hdr::Pq), _) => (input(Depth::W16, Tf::Pq, to(Prim::Rec2020)), String::new(), Some("bt2020")),
                (Some(Hdr::Hlg), _) => (input(Depth::W16, Tf::Hlg, to(Prim::Rec2020)), String::new(), Some("bt2020")),
                (Some(Hdr::Sdr2020), _) => (input(Depth::W16, Tf::Video, to(Prim::Rec2020)), String::new(), Some("bt2020")),
                (None, _) => {
                    let depth = if float { Depth::F32 } else if deep { Depth::W16 } else { Depth::B8 };
                    (Input { depth, ..self.sdr_input(depth) }, pre8.to_string(), matrix8)
                }
            };
        }
        // SDR working spaces: R40a's transform brings HDR to SDR, at 16 bits; a linear still comes in as it is.
        if linear && m.codec.as_deref() != Some("hdr") {
            return (input(Depth::F32, Tf::Linear, None), String::new(), None);
        }
        let pre = pre8.replace("format=gbrp,", "format=gbrp16le,");
        let depth = if float && !linear { Depth::F32 } else if deep || !pre.is_empty() { Depth::W16 } else { Depth::B8 };
        (Input { depth, ..self.sdr_input(depth) }, pre, matrix8)
    }

    /// Working values (straight) -> what `out` writes: display-coded 0..1 (PQ and HLG codes for HDR).
    pub fn view_px(&self, c: [f32; 3], out: Out) -> [f32; 3] {
        match out {
            Out::Video => {
                if self.enc == Enc::Video && self.prim == Prim::Rec709 {
                    return c.map(|v| v.clamp(0.0, 1.0));
                }
                self.linear709(c).map(|v| lin_to_video(v).clamp(0.0, 1.0))
            }
            Out::AcesSrgb => aces_fit(self.linear709(c)).map(srgb_oetf),
            Out::AcesRec709 => aces_fit(self.linear709(c)).map(|v| v.max(0.0).powf(1.0 / 2.4)),
            Out::Pq => {
                let l = roll_off(mul(conv_m(self.prim, Prim::Rec2020), self.decode3(c)), HDR_PEAK / REF_WHITE);
                l.map(|v| pq_oetf(v * REF_WHITE))
            }
            Out::Hlg => {
                let l = roll_off(mul(conv_m(self.prim, Prim::Rec2020), self.decode3(c)), HDR_PEAK / REF_WHITE);
                hlg_from_display(l.map(|v| v * REF_WHITE / HDR_PEAK))
            }
            Out::Lut => match self.lut.as_deref().map(crate::engine::lut::load) {
                Some(Ok(cube)) => cube.apply(c).map(|v| v.clamp(0.0, 1.0)),
                // No LUT (or one that cannot load: the preview says so): the picture as video codes it.
                _ => self.view_px(c, Out::Video),
            },
        }
    }

    /// Working values (straight) -> SDR Rec.709 display values for the Video Preview: the view itself, or for an HDR
    /// view a tone map of the scene light to SDR (R40a's: exact below half of reference white, Möbius up to the peak).
    pub fn preview_px(&self, c: [f32; 3]) -> [f32; 3] {
        if !self.out.hdr() {
            return self.view_px(c, self.out);
        }
        let l = self.linear709(c).map(|v| v.max(0.0));
        let m = l[0].max(l[1]).max(l[2]);
        let t = mobius(m, 0.5, HDR_PEAK / REF_WHITE);
        let k = if m > 1e-9 { t / m } else { 1.0 };
        l.map(|v| lin_to_video((v * k).min(1.0)))
    }

    fn decode3(&self, c: [f32; 3]) -> [f32; 3] {
        c.map(|v| self.decode(v))
    }

    /// The working primaries -> Rec.709 (on linear light).
    pub fn matrix709(&self) -> M3 {
        conv_m(self.prim, Prim::Rec709)
    }

    /// Working values -> linear Rec.709.
    pub fn linear709(&self, c: [f32; 3]) -> [f32; 3] {
        mul(conv_m(self.prim, Prim::Rec709), self.decode3(c))
    }

    /// The picture for the Video Preview, scopes and snapshots: 8-bit, premultiplied as the 8-bit pipeline keeps it.
    pub fn to_preview(&self, img: &WImage, k: usize) -> ColorImage {
        let mut out = ColorImage::new(img.size, Color32::TRANSPARENT);
        let w = img.size[0];
        crate::engine::composite::par_rows(&mut out.pixels, w, k, |y, row| {
            for (o, p) in row.iter_mut().zip(&img.px[y * w..(y + 1) * w]) {
                let a = p[3].clamp(0.0, 1.0);
                if a <= 0.0 {
                    continue;
                }
                let c = self.preview_px([p[0] / a, p[1] / a, p[2] / a]);
                let a8 = (a * 255.0).round() as u8;
                let q = |v: f32| (v.clamp(0.0, 1.0) * a8 as f32).round() as u8;
                *o = Color32::from_rgba_premultiplied(q(c[0]), q(c[1]), q(c[2]), a8);
            }
        });
        out
    }

    /// The picture as `out` writes it (None: as the preview shows it, an HDR view tone-mapped to SDR), 16 bits per
    /// channel, straight alpha (ffmpeg's `rgba64le`).
    pub fn to_out16(&self, img: &WImage, out: Option<Out>, k: usize) -> Vec<[u16; 4]> {
        let w = img.size[0];
        let mut px = vec![[0u16; 4]; img.px.len()];
        par_chunks(&mut px, w, k, |y, row| {
            for (o, p) in row.iter_mut().zip(&img.px[y * w..(y + 1) * w]) {
                let a = p[3].clamp(0.0, 1.0);
                let s = if a > 0.0 { [p[0] / a, p[1] / a, p[2] / a] } else { [self.encode(0.0); 3] };
                let c = match out {
                    Some(o) => self.view_px(s, o),
                    None => self.preview_px(s),
                };
                let q = |v: f32| (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
                *o = [q(c[0]), q(c[1]), q(c[2]), q(a)];
            }
        });
        px
    }

    /// The picture as linear Rec.709 light, straight alpha, planar G, B, R, A (`gbrapf32le`): what an OpenEXR render
    /// writes (scene-linear, highlights kept).
    pub fn to_linear_planes(&self, img: &WImage, alpha: bool) -> Vec<f32> {
        let n = img.px.len();
        let mut planes = vec![0.0f32; n * if alpha { 4 } else { 3 }];
        for (i, p) in img.px.iter().enumerate() {
            let a = p[3].clamp(0.0, 1.0);
            let c = if a > 0.0 { self.linear709([p[0] / a, p[1] / a, p[2] / a]) } else { [0.0; 3] };
            (planes[i], planes[n + i], planes[2 * n + i]) = (c[1], c[2], c[0]);
            if alpha {
                planes[3 * n + i] = a;
            }
        }
        planes
    }

    /// An 8-bit SDR picture (premultiplied, video-coded Rec.709) in the working space.
    pub fn from_sdr8(&self, img: &ColorImage) -> WImage {
        self.sdr_input(Depth::B8).convert(img, img.size[0], img.size[1], true)
    }
}

/// How a decoded picture is held: 8 bits (`rgba`), 16 (`rgba64le`) or 32-bit float (`gbrapf32le`, interleaved by
/// the reader).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Depth {
    B8,
    W16,
    F32,
}

impl Depth {
    /// `Color32`s per pixel in the `ColorImage` that carries a frame of this depth.
    pub fn factor(self) -> usize {
        match self {
            Depth::B8 => 1,
            Depth::W16 => 2,
            Depth::F32 => 4,
        }
    }

    /// The decoders' scaling filter to `w` x `h` with `scaler` (`Quality::scaler`) and the source's matrix: the
    /// 8-bit one as it was; 16 bits with exact rounding; float through zimg, which keeps values above 1.0 (swscale
    /// would clip them).
    pub fn scale(self, w: u32, h: u32, scaler: &str, matrix: Option<&str>) -> String {
        let matrix = matrix.unwrap_or("auto");
        match self {
            Depth::B8 => format!("scale={w}:{h}:flags={scaler}:in_color_matrix={matrix}:out_range=full,format=rgba"),
            Depth::W16 => format!("scale={w}:{h}:flags={scaler}+accurate_rnd+full_chroma_int:in_color_matrix={matrix}:out_range=full,format=rgba64le"),
            Depth::F32 => {
                let f = match scaler {
                    "lanczos" => "lanczos",
                    "bicubic" => "bicubic",
                    _ => "bilinear",
                };
                format!("zscale=w={w}:h={h}:f={f},format=gbrapf32le")
            }
        }
    }
}

/// A source's transfer: code values -> linear light (relative to reference white).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tf {
    /// Already working values (an SDR source in a video-coded Rec.709 working space).
    Code,
    /// Video: to the power 2.222.
    Video,
    Linear,
    /// SMPTE ST 2084, 203 cd/m² = 1.0.
    Pq,
    /// ARIB STD-B67 for a 1000 cd/m² display, 203 cd/m² = 1.0.
    Hlg,
    /// The inverse of an ACES SDR view (an SDR source shows as it is coded).
    InvView(Out),
}

/// How one source comes into the working space.
#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    pub depth: Depth,
    pub tf: Tf,
    /// The source's primaries to the working ones (None: the same).
    pub m: Option<M3>,
    pub enc: Enc,
}

impl Input {
    /// Straight code values 0..1 (more for float sources) -> straight working values.
    pub fn pixel(&self, c: [f32; 3]) -> [f32; 3] {
        let lin = match self.tf {
            Tf::Code => return c,
            Tf::Video => c.map(video_to_lin),
            Tf::Linear => c,
            Tf::Pq => c.map(|v| pq_eotf(v) / REF_WHITE),
            Tf::Hlg => hlg_to_display(c).map(|v| v * HDR_PEAK / REF_WHITE),
            Tf::InvView(out) => inv_view(c, out),
        };
        let lin = self.m.map_or(lin, |m| mul(m, lin));
        match self.enc {
            Enc::Video => lin.map(lin_to_video),
            Enc::Linear => lin,
            Enc::Cc => lin.map(lin_to_cc),
            Enc::Cct => lin.map(lin_to_cct),
        }
    }

    /// A decoded frame `w` x `h` (`img` holds it at this input's depth; an 8-bit one premultiplied, a wider one
    /// straight) in the working space, premultiplied. `premultiplied`: an 8-bit picture that is (the decoders and
    /// generators premultiply theirs).
    pub fn convert(&self, img: &ColorImage, w: usize, h: usize, premultiplied: bool) -> WImage {
        let n = w * h;
        let mut px = vec![[0.0f32; 4]; n];
        let bytes: &[u8] = bytemuck::cast_slice(&img.pixels);
        let simple = self.tf == Tf::Code;
        // A transfer that works per channel on integer codes is a table (256 or 65536 entries), then the matrix and
        // the working encoding: no `powf` per sample for 8 and 16-bit sources.
        let table = lin_table(self.tf, self.depth);
        let finish = |lin: [f32; 3]| -> [f32; 3] {
            let lin = self.m.map_or(lin, |m| mul(m, lin));
            match self.enc {
                Enc::Linear => lin,
                Enc::Video => lin.map(lin_to_video),
                Enc::Cc => lin.map(lin_to_cc),
                Enc::Cct => lin.map(lin_to_cct),
            }
        };
        let k = crate::engine::composite::stripes();
        par_chunks(&mut px, w.max(1), k, |y, row| {
            for (x, o) in row.iter_mut().enumerate() {
                let i = y * w + x;
                let (c, a) = match self.depth {
                    Depth::B8 => {
                        let p = &bytes[i * 4..i * 4 + 4];
                        let a = p[3] as f32 / 255.0;
                        if let Some(t) = table.filter(|_| p[3] == 255 || !premultiplied) {
                            let c = finish([t[p[0] as usize], t[p[1] as usize], t[p[2] as usize]]);
                            *o = [c[0] * a, c[1] * a, c[2] * a, a];
                            continue;
                        }
                        let un = |v: u8| if premultiplied && p[3] < 255 { if p[3] == 0 { 0.0 } else { v as f32 / p[3] as f32 } } else { v as f32 / 255.0 };
                        ([un(p[0]), un(p[1]), un(p[2])], a)
                    }
                    Depth::W16 => {
                        let q = |j: usize| u16::from_le_bytes([bytes[i * 8 + j * 2], bytes[i * 8 + j * 2 + 1]]);
                        let a = q(3) as f32 / 65535.0;
                        if let Some(t) = table {
                            let c = finish([t[q(0) as usize], t[q(1) as usize], t[q(2) as usize]]);
                            *o = [c[0] * a, c[1] * a, c[2] * a, a];
                            continue;
                        }
                        ([q(0), q(1), q(2)].map(|v| v as f32 / 65535.0), a)
                    }
                    Depth::F32 => {
                        let f = |j: usize| f32::from_le_bytes(bytes[i * 16 + j * 4..i * 16 + j * 4 + 4].try_into().expect("4 bytes"));
                        ([f(0), f(1), f(2)], f(3).clamp(0.0, 1.0))
                    }
                };
                let c = if simple { c } else { self.pixel(c) };
                *o = [c[0] * a, c[1] * a, c[2] * a, a];
            }
        });
        WImage { size: [w, h], px }
    }
}

/// The linear light of every integer code of a per-channel transfer at `depth` (None: not one, or float).
fn lin_table(tf: Tf, depth: Depth) -> Option<&'static [f32]> {
    static V8: OnceLock<Vec<f32>> = OnceLock::new();
    static V16: OnceLock<Vec<f32>> = OnceLock::new();
    static L8: OnceLock<Vec<f32>> = OnceLock::new();
    static L16: OnceLock<Vec<f32>> = OnceLock::new();
    static P16: OnceLock<Vec<f32>> = OnceLock::new();
    let make = |n: usize, f: fn(f32) -> f32| (0..n).map(|i| f(i as f32 / (n - 1) as f32)).collect::<Vec<f32>>();
    Some(match (tf, depth) {
        (Tf::Video, Depth::B8) => V8.get_or_init(|| make(256, video_to_lin)),
        (Tf::Video, Depth::W16) => V16.get_or_init(|| make(65536, video_to_lin)),
        (Tf::Linear, Depth::B8) => L8.get_or_init(|| make(256, |v| v)),
        (Tf::Linear, Depth::W16) => L16.get_or_init(|| make(65536, |v| v)),
        (Tf::Pq, Depth::W16) => P16.get_or_init(|| make(65536, |v| pq_eotf(v) / REF_WHITE)),
        _ => return None,
    })
}
/// Run `f(y, row)` over the rows (`w` wide) of `px`, on up to `k` threads.
pub(crate) fn par_chunks<T: Send>(px: &mut [T], w: usize, k: usize, f: impl Fn(usize, &mut [T]) + Sync) {
    let rows = px.len() / w.max(1);
    if k <= 1 || rows < 2 * k || px.len() < 16_384 {
        px.chunks_mut(w.max(1)).enumerate().for_each(|(y, r)| f(y, r));
        return;
    }
    let (per, low) = (rows.div_ceil(k), crate::proc::is_below_normal());
    let f = &f;
    std::thread::scope(|s| {
        for (i, chunk) in px.chunks_mut(per * w).enumerate() {
            s.spawn(move || {
                if low {
                    crate::proc::below_normal_thread();
                }
                chunk.chunks_mut(w).enumerate().for_each(|(j, r)| f(i * per + j, r))
            });
        }
    });
}

// ---- the 8-bit bridge ----

/// The working values (premultiplied) as 8-bit, clamped: what an FX without a float kernel works on.
pub fn to8(img: &WImage) -> ColorImage {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let pixels = img.px.iter().map(|p| {
        let a = q(p[3]);
        Color32::from_rgba_premultiplied(q(p[0]).min(a), q(p[1]).min(a), q(p[2]).min(a), a)
    });
    ColorImage { size: img.size, pixels: pixels.collect() }
}

/// An 8-bit picture of working values (`to8`'s inverse, no transform).
pub fn from8(img: &ColorImage) -> WImage {
    WImage { size: img.size, px: img.pixels.iter().map(|p| p.to_array().map(|v| v as f32 / 255.0)).collect() }
}

/// `f` (an 8-bit FX) on `img`: on `to8`'s copy; where it left a pixel as it was, the pixel keeps its float value, and
/// elsewhere it keeps the part under half an 8-bit step, so a gradient through the FX does not band.
pub fn via8(img: &mut WImage, f: impl FnOnce(&mut ColorImage)) {
    let before = to8(img);
    let mut after = before.clone();
    f(&mut after);
    if after.size != img.size {
        *img = from8(&after);
        return;
    }
    let half = 0.5 / 255.0;
    for ((p, b), a) in img.px.iter_mut().zip(&before.pixels).zip(&after.pixels) {
        if a == b {
            continue;
        }
        let q = a.to_array();
        if q[3] == 0 {
            *p = [0.0; 4];
            continue;
        }
        let keep = q[3] == b.a();
        let r = |i: usize| if keep { (p[i] - b.to_array()[i] as f32 / 255.0).clamp(-half, half) } else { 0.0 };
        *p = std::array::from_fn(|i| (q[i] as f32 / 255.0 + r(i)).max(0.0));
    }
}

/// `f` (an 8-bit transition) from `a` to `b`, the same way.
pub fn via8_2(a: &WImage, b: &WImage, p: f32, f: impl FnOnce(&ColorImage, &ColorImage) -> ColorImage) -> WImage {
    let (qa, qb) = (to8(a), to8(b));
    let out = f(&qa, &qb);
    if out.size != a.size {
        return from8(&out);
    }
    let half = 0.5 / 255.0;
    let mut px = Vec::with_capacity(out.pixels.len());
    for (i, o) in out.pixels.iter().enumerate() {
        let q = o.to_array();
        // A pixel the transition took from one side as it was keeps that side's float value.
        if *o == qa.pixels[i] && (p < 0.5 || *o != qb.pixels[i]) {
            px.push(a.px[i]);
            continue;
        }
        if *o == qb.pixels[i] {
            px.push(b.px[i]);
            continue;
        }
        if q[3] == 0 {
            px.push([0.0; 4]);
            continue;
        }
        let r = |c: usize| {
            let (ra, rb) = (a.px[i][c] - qa.pixels[i].to_array()[c] as f32 / 255.0, b.px[i][c] - qb.pixels[i].to_array()[c] as f32 / 255.0);
            (ra + (rb - ra) * p).clamp(-half, half)
        };
        px.push(std::array::from_fn(|c| (q[c] as f32 / 255.0 + r(c)).max(0.0)));
    }
    WImage { size: a.size, px }
}

// ---- transfers ----

const VIDEO_GAMMA: f32 = 2.222;

pub fn video_to_lin(v: f32) -> f32 {
    if v >= 0.0 { v.powf(VIDEO_GAMMA) } else { -(-v).powf(VIDEO_GAMMA) }
}

pub fn lin_to_video(v: f32) -> f32 {
    if v >= 0.0 { v.powf(1.0 / VIDEO_GAMMA) } else { -(-v).powf(1.0 / VIDEO_GAMMA) }
}

pub fn srgb_oetf(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

fn srgb_eotf(v: f32) -> f32 {
    if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

/// ACEScc (S-2014-003).
pub fn lin_to_cc(v: f32) -> f32 {
    let l = if v <= 0.0 { 2f32.powi(-16) } else if v < 2f32.powi(-15) { 2f32.powi(-16) + v * 0.5 } else { v };
    (l.log2() + 9.72) / 17.52
}

pub fn cc_to_lin(v: f32) -> f32 {
    if v < (9.72 - 15.0) / 17.52 {
        (2f32.powf(v * 17.52 - 9.72) - 2f32.powi(-16)) * 2.0
    } else {
        2f32.powf(v * 17.52 - 9.72).min(65504.0)
    }
}

/// ACEScct (S-2016-001).
const CCT_X: f32 = 0.007_812_5;
const CCT_Y: f32 = 0.155_251_14;
const CCT_A: f32 = 10.540_237;
const CCT_B: f32 = 0.072_905_53;

pub fn lin_to_cct(v: f32) -> f32 {
    if v <= CCT_X { CCT_A * v + CCT_B } else { (v.log2() + 9.72) / 17.52 }
}

pub fn cct_to_lin(v: f32) -> f32 {
    if v <= CCT_Y { (v - CCT_B) / CCT_A } else { 2f32.powf(v * 17.52 - 9.72).min(65504.0) }
}

const PQ_M1: f32 = 0.159_301_76;
const PQ_M2: f32 = 78.843_75;
const PQ_C1: f32 = 0.835_937_5;
const PQ_C2: f32 = 18.851_562;
const PQ_C3: f32 = 18.687_5;

/// PQ code -> cd/m².
pub fn pq_eotf(v: f32) -> f32 {
    let p = v.clamp(0.0, 1.0).powf(1.0 / PQ_M2);
    ((p - PQ_C1).max(0.0) / (PQ_C2 - PQ_C3 * p)).powf(1.0 / PQ_M1) * 10_000.0
}

/// cd/m² -> PQ code.
pub fn pq_oetf(nits: f32) -> f32 {
    let y = (nits / 10_000.0).clamp(0.0, 1.0).powf(PQ_M1);
    ((PQ_C1 + PQ_C2 * y) / (1.0 + PQ_C3 * y)).powf(PQ_M2)
}

const HLG_A: f32 = 0.178_832_77;
const HLG_B: f32 = 0.284_668_92;
const HLG_C: f32 = 0.559_910_7;
/// BT.2100 luminance weights (Rec.2020).
const Y2020: [f32; 3] = [0.2627, 0.6780, 0.0593];

fn hlg_inv_oetf(e: f32) -> f32 {
    let e = e.clamp(0.0, 1.0);
    if e <= 0.5 { e * e / 3.0 } else { (((e - HLG_C) / HLG_A).exp() + HLG_B) / 12.0 }
}

fn hlg_oetf(l: f32) -> f32 {
    let l = l.clamp(0.0, 1.0);
    if l <= 1.0 / 12.0 { (3.0 * l).sqrt() } else { HLG_A * (12.0 * l - HLG_B).ln() + HLG_C }
}

/// HLG codes -> display light relative to the display's peak (1000 cd/m²: the OOTF's gamma 1.2).
pub fn hlg_to_display(c: [f32; 3]) -> [f32; 3] {
    let s = c.map(hlg_inv_oetf);
    let ys = Y2020[0] * s[0] + Y2020[1] * s[1] + Y2020[2] * s[2];
    let g = if ys > 0.0 { ys.powf(0.2) } else { 0.0 };
    s.map(|v| v * g)
}

/// Display light relative to the peak -> HLG codes (the inverse OOTF, then the OETF).
pub fn hlg_from_display(d: [f32; 3]) -> [f32; 3] {
    let d = d.map(|v| v.clamp(0.0, 1.0));
    let yd = Y2020[0] * d[0] + Y2020[1] * d[1] + Y2020[2] * d[2];
    if yd <= 0.0 {
        return [0.0; 3];
    }
    let ys = yd.powf(1.0 / 1.2);
    d.map(|v| hlg_oetf(v / ys.powf(0.2)))
}

/// The Möbius roll-off (as FFmpeg's tonemap and libplacebo have it): `x` as it is up to `knee`, then bent so that
/// `peak` lands on 1.
pub fn mobius(x: f32, knee: f32, peak: f32) -> f32 {
    if x <= knee {
        return x;
    }
    if peak <= 1.0 {
        return x.min(1.0);
    }
    let j = knee;
    let a = -j * j * (peak - 1.0) / (j * j - 2.0 * j + peak);
    let b = (j * j - 2.0 * j * peak + peak) / (peak - 1.0).max(1e-6);
    (b * b + 2.0 * b * j + j * j) / (b - a) * (x + a) / (x + b)
}

/// HDR output: linear light (1.0 = reference white) with highlights rolled off to `peak` (exact below three quarters
/// of it), the brightest channel setting the curve so hues hold.
fn roll_off(l: [f32; 3], peak: f32) -> [f32; 3] {
    let l = l.map(|v| v.max(0.0));
    let m = l[0].max(l[1]).max(l[2]);
    if m <= 0.75 * peak {
        return l;
    }
    let t = mobius(m / peak, 0.75, 10_000.0 / REF_WHITE / peak) * peak;
    l.map(|v| v * t / m)
}

// ---- the ACES SDR output transforms (Stephen Hill's fit of the RRT and the sRGB ODT, MIT licensed) ----

const ACES_IN: M3 = [[0.597_19, 0.354_58, 0.048_23], [0.076_00, 0.908_34, 0.015_66], [0.028_40, 0.133_83, 0.837_77]];
const ACES_OUT: M3 = [[1.604_75, -0.531_08, -0.073_67], [-0.102_08, 1.108_13, -0.006_05], [-0.003_27, -0.072_76, 1.076_02]];

fn rrt_odt(v: f32) -> f32 {
    (v * (v + 0.024_578_6) - 0.000_090_537) / (v * (0.983_729 * v + 0.432_951) + 0.238_081)
}

fn rrt_odt_inv(y: f32) -> f32 {
    let y = y.clamp(0.0, 0.999);
    let (a, b, c) = (0.983_729 * y - 1.0, 0.432_951 * y - 0.024_578_6, 0.238_081 * y + 0.000_090_537);
    (-b - (b * b - 4.0 * a * c).max(0.0).sqrt()) / (2.0 * a)
}

/// Linear Rec.709 scene light -> linear display light 0..1 through the fitted ACES curve.
fn aces_fit(l: [f32; 3]) -> [f32; 3] {
    mul(ACES_OUT, mul(ACES_IN, l).map(rrt_odt)).map(|v| v.clamp(0.0, 1.0))
}

/// Display codes of an ACES SDR view -> linear Rec.709 scene light (`aces_fit`'s inverse).
fn inv_view(c: [f32; 3], out: Out) -> [f32; 3] {
    let d = match out {
        Out::AcesSrgb => c.map(srgb_eotf),
        _ => c.map(|v| v.max(0.0).powf(2.4)),
    };
    mul(inv3(ACES_IN), mul(inv3(ACES_OUT), d).map(rrt_odt_inv))
}

// ---- primaries ----

/// Linear RGB of `p` -> linear Rec.709 (and back by `inv3`). AP0 and AP1 through Bradford D60 -> D65.
fn to709(p: Prim) -> M3 {
    match p {
        Prim::Rec709 => IDENTITY,
        Prim::Rec2020 => [[1.660_491, -0.587_641_1, -0.072_849_9], [-0.124_550_5, 1.132_899_9, -0.008_349_4], [-0.018_150_8, -0.100_578_9, 1.118_729_7]],
        Prim::Ap0 => [[2.521_686, -1.134_131, -0.387_555], [-0.276_479, 1.372_719, -0.096_240], [-0.015_378, -0.152_975, 1.168_353]],
        Prim::Ap1 => [[1.705_051, -0.621_792_1, -0.083_259], [-0.130_256_4, 1.140_804_7, -0.010_548_3], [-0.024_003_4, -0.128_969, 1.152_972_4]],
    }
}

const IDENTITY: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// `from` -> `to` primaries (None: the same).
pub fn conv(from: Prim, to: Prim) -> Option<M3> {
    (from != to).then(|| conv_m(from, to))
}

fn conv_m(from: Prim, to: Prim) -> M3 {
    if from == to {
        return IDENTITY;
    }
    mm(inv3(to709(to)), to709(from))
}

pub fn mul(m: M3, c: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| m[i][0] * c[0] + m[i][1] * c[1] + m[i][2] * c[2])
}

fn mm(a: M3, b: M3) -> M3 {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] as f64 * b[k][j] as f64).sum::<f64>() as f32))
}

fn inv3(m: M3) -> M3 {
    let m = m.map(|r| r.map(|v| v as f64));
    let [[a, b, c], [d, e, f], [g, h, i]] = m;
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    let r = [[e * i - f * h, c * h - b * i, b * f - c * e], [f * g - d * i, a * i - c * g, c * d - a * f], [d * h - e * g, b * g - a * h, a * e - b * d]];
    r.map(|row| row.map(|v| (v / det) as f32))
}
