//! R40b: Project Properties › Video's pixel format and colour rows, as VEGAS Pro 15-23 has them: Pixel format, HDR
//! Mode, Compositing gamma, ACES version, ACES color space and View transform. An 8-bit project has them all at their
//! defaults and saves none of them (`PixelSettings::is_8bit`), so its file is as before R40b.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Pixel format. VEGAS's "8-bit" (Milan Pro's pipeline until R40b), and its two 32-bit floating point formats.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PixelFormat {
    #[default]
    Bit8,
    /// 32-bit floating point (video levels): the 8-bit project's levels and look at float precision.
    FloatVideo,
    /// 32-bit floating point (full range): linear compositing by default, ACES view transforms and HDR.
    FloatFull,
}

impl PixelFormat {
    pub const ALL: [PixelFormat; 3] = [PixelFormat::Bit8, PixelFormat::FloatVideo, PixelFormat::FloatFull];

    pub fn name(self) -> &'static str {
        match self {
            PixelFormat::Bit8 => "8-bit",
            PixelFormat::FloatVideo => "32-bit floating point (video levels)",
            PixelFormat::FloatFull => "32-bit floating point (full range)",
        }
    }
}

/// Compositing gamma: what blending, resampling, dissolves and motion blur work on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Gamma {
    /// 2.222: the pixels as video codes them, as in an 8-bit project.
    #[default]
    Video,
    /// 1.000: linear light.
    Linear,
}

impl Gamma {
    pub const ALL: [Gamma; 2] = [Gamma::Video, Gamma::Linear];

    pub fn name(self) -> &'static str {
        match self {
            Gamma::Video => "2.222 (Video)",
            Gamma::Linear => "1.000 (Linear)",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AcesVersion {
    #[default]
    V10,
    V11,
}

impl AcesVersion {
    pub const ALL: [AcesVersion; 2] = [AcesVersion::V10, AcesVersion::V11];

    pub fn name(self) -> &'static str {
        match self {
            AcesVersion::V10 => "1.0",
            AcesVersion::V11 => "1.1",
        }
    }
}

/// ACES color space: the working space while an ACES view transform is on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AcesSpace {
    /// ACES2065-1: AP0 primaries, linear.
    #[default]
    Aces2065,
    /// ACEScg: AP1 primaries, linear.
    AcesCg,
    /// ACEScc: AP1, logarithmic.
    AcesCc,
    /// ACEScct: AP1, logarithmic with a linear toe.
    AcesCct,
}

impl AcesSpace {
    pub const ALL: [AcesSpace; 4] = [AcesSpace::Aces2065, AcesSpace::AcesCg, AcesSpace::AcesCc, AcesSpace::AcesCct];

    pub fn name(self) -> &'static str {
        match self {
            AcesSpace::Aces2065 => "Default (ACES2065-1)",
            AcesSpace::AcesCg => "ACEScg",
            AcesSpace::AcesCc => "ACEScc",
            AcesSpace::AcesCct => "ACEScct",
        }
    }
}

/// View transform (32-bit floating point full range only): how the working picture reaches the display and the
/// render. Off is VEGAS's own: no ACES, the picture as composited.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewTransform {
    #[default]
    Off,
    Srgb,
    Rec709,
    /// HDR10: Rec.2020 primaries, SMPTE ST 2084 (PQ), a 1000 cd/m² display.
    Pq1000,
    /// Rec.2020 HLG for a 1000 cd/m² display.
    Hlg1000,
    /// A .cube LUT from the working picture to Rec.709 (`PixelSettings::view_lut`), for an OpenColorIO transform
    /// baked into one.
    Lut,
}

impl ViewTransform {
    pub const ALL: [ViewTransform; 6] = [ViewTransform::Off, ViewTransform::Srgb, ViewTransform::Rec709, ViewTransform::Pq1000, ViewTransform::Hlg1000, ViewTransform::Lut];

    pub fn name(self) -> &'static str {
        match self {
            ViewTransform::Off => "Off",
            ViewTransform::Srgb => "sRGB (ACES)",
            ViewTransform::Rec709 => "Rec.709 (ACES)",
            ViewTransform::Pq1000 => "Rec.2020 ST 2084 1000 nits (ACES)",
            ViewTransform::Hlg1000 => "Rec.2020 HLG 1000 nits (ACES)",
            ViewTransform::Lut => "Custom LUT (.cube)",
        }
    }

    pub fn hdr(self) -> HdrMode {
        match self {
            ViewTransform::Pq1000 => HdrMode::Hdr10,
            ViewTransform::Hlg1000 => HdrMode::Hlg,
            _ => HdrMode::Off,
        }
    }
}

/// HDR Mode (VEGAS 18 and later): a shortcut that sets the pixel format, the compositing gamma and the view transform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HdrMode {
    Off,
    Hdr10,
    Hlg,
}

impl HdrMode {
    pub const ALL: [HdrMode; 3] = [HdrMode::Off, HdrMode::Hdr10, HdrMode::Hlg];

    pub fn name(self) -> &'static str {
        match self {
            HdrMode::Off => "Off",
            HdrMode::Hdr10 => "HDR10",
            HdrMode::Hlg => "HLG",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PixelSettings {
    #[serde(deserialize_with = "lenient")]
    pub format: PixelFormat,
    #[serde(deserialize_with = "lenient")]
    pub gamma: Gamma,
    #[serde(deserialize_with = "lenient")]
    pub aces_version: AcesVersion,
    #[serde(deserialize_with = "lenient")]
    pub aces_space: AcesSpace,
    #[serde(deserialize_with = "lenient")]
    pub view: ViewTransform,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view_lut: Option<PathBuf>,
}

/// A value this build does not know (a later version's) reads as the default.
fn lenient<'de, D: serde::Deserializer<'de>, T: Deserialize<'de> + Default>(d: D) -> Result<T, D::Error> {
    Ok(T::deserialize(serde_json::Value::deserialize(d)?).unwrap_or_default())
}

impl PixelSettings {
    /// Everything at VEGAS's defaults: the 8-bit pipeline, nothing saved.
    pub fn is_8bit(&self) -> bool {
        *self == PixelSettings::default()
    }

    pub fn float(&self) -> bool {
        self.format != PixelFormat::Bit8
    }

    /// The view transform in effect: only full range has one.
    pub fn view(&self) -> ViewTransform {
        if self.format == PixelFormat::FloatFull { self.view } else { ViewTransform::Off }
    }

    /// An ACES view transform (or a LUT) is on: the working space is the ACES color space.
    pub fn aces(&self) -> bool {
        self.view() != ViewTransform::Off
    }

    pub fn hdr(&self) -> HdrMode {
        self.view().hdr()
    }

    /// Pixel format `f`, with the compositing gamma VEGAS switches to with it (full range: 1.000, else 2.222).
    pub fn set_format(&mut self, f: PixelFormat) {
        if f != self.format {
            self.format = f;
            self.gamma = if f == PixelFormat::FloatFull { Gamma::Linear } else { Gamma::Video };
        }
    }

    /// HDR Mode `m`: HDR10 and HLG need 32-bit floating point (full range) with linear compositing and their view
    /// transform; Off leaves the format and goes back to the SDR view (Rec.709) when an HDR one was on.
    pub fn set_hdr(&mut self, m: HdrMode) {
        match m {
            HdrMode::Off if self.view.hdr() != HdrMode::Off => self.view = ViewTransform::Rec709,
            HdrMode::Off => {}
            _ => {
                self.set_format(PixelFormat::FloatFull);
                self.gamma = Gamma::Linear;
                self.view = if m == HdrMode::Hdr10 { ViewTransform::Pq1000 } else { ViewTransform::Hlg1000 };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vegas_switches_and_old_files() {
        let mut s = PixelSettings::default();
        assert!(s.is_8bit() && !s.float() && s.view() == ViewTransform::Off);
        s.set_format(PixelFormat::FloatFull);
        assert_eq!(s.gamma, Gamma::Linear);
        s.set_format(PixelFormat::FloatVideo);
        assert_eq!(s.gamma, Gamma::Video);
        // A view transform counts in full range only.
        s.view = ViewTransform::Pq1000;
        assert_eq!((s.view(), s.hdr()), (ViewTransform::Off, HdrMode::Off));
        s.set_hdr(HdrMode::Hlg);
        assert_eq!((s.format, s.gamma, s.view(), s.hdr()), (PixelFormat::FloatFull, Gamma::Linear, ViewTransform::Hlg1000, HdrMode::Hlg));
        s.set_hdr(HdrMode::Off);
        assert_eq!((s.view(), s.format), (ViewTransform::Rec709, PixelFormat::FloatFull));
        // Unknown values (a later version's) read as the defaults; a missing block is 8-bit.
        let v: PixelSettings = serde_json::from_str(r#"{"format":"Float64","view":"Pq4000","aces_space":"AcesCct"}"#).unwrap();
        assert_eq!((v.format, v.view, v.aces_space), (PixelFormat::Bit8, ViewTransform::Off, AcesSpace::AcesCct));
        assert!(serde_json::from_str::<PixelSettings>("{}").unwrap().is_8bit());
    }
}
