//! R40a: the view transform that brings HDR (PQ, HLG), BT.2020 and linear-light sources onto Milan Pro's 8-bit
//! Rec.709 timeline. It is an ffmpeg filter chain ahead of the decoder's scaler, so the Video Preview, the Trimmer,
//! thumbnails, proxies and the render all see the same picture (`plan::media_ref` puts it in `MediaRef::pre`).
//!
//! Convention (ITU-R BT.2408): HDR reference white, 203 cd/m², is SDR white. A PQ or HLG source is taken to linear
//! light with that white at 1.0, converted to Rec.709 primaries, and its highlights above 50 % are rolled off with the
//! Möbius curve up to the signal peak (the mastering display's maximum luminance when the file says, else 1000 cd/m²);
//! Project Properties › Color management can clip them instead, or turn the transform off. BT.2020 SDR sources are
//! converted to Rec.709 without a tone map. OpenEXR (linear) gets the sRGB transfer in an 8-bit project (R40b: a float project keeps it in float, `engine/wide`);
//! Radiance HDR is tone-mapped like PQ.
use crate::project::{ColorManagement, Hdr, MediaItem};

/// HDR reference white, cd/m² (BT.2408).
pub const REF_WHITE: f64 = 203.0;
/// The nominal display an HLG signal is decoded for, cd/m².
pub const HLG_PEAK: f64 = 1000.0;
/// The peak assumed for a PQ source without mastering metadata, cd/m².
pub const PQ_DEFAULT_PEAK: f64 = 1000.0;
/// Where the Möbius roll-off starts (linear light, 1.0 = reference white): below it the picture is exact.
const KNEE: f64 = 0.5;

/// A source's HDR handling, for the Project Media badge and Media Properties: "HDR → SDR" when the transform tone-maps
/// it, "BT.2020 → Rec.709" for a wide-gamut SDR source, None for plain SDR (or with the transform off).
pub fn badge(m: &MediaItem, cm: ColorManagement) -> Option<&'static str> {
    if cm == ColorManagement::Off {
        return None;
    }
    match m.hdr() {
        Some(Hdr::Pq | Hdr::Hlg) => Some("HDR → SDR"),
        Some(Hdr::Sdr2020) => Some("BT.2020 → Rec.709"),
        None if linear(m) => Some("linear → sRGB"),
        None => None,
    }
}

/// A linear-light still (OpenEXR, Radiance HDR): the decoder gives float RGB without a transfer curve.
fn linear(m: &MediaItem) -> bool {
    matches!(m.codec.as_deref(), Some("exr" | "hdr")) || m.transfer.as_deref() == Some("linear")
}

/// The signal peak of `m` in units of reference white: the mastering display's maximum luminance (or MaxCLL) when
/// the file carries it, else `PQ_DEFAULT_PEAK`; HLG is always decoded for a 1000 cd/m² display.
pub fn peak(m: &MediaItem) -> f64 {
    let nits = match m.hdr() {
        Some(Hdr::Hlg) => HLG_PEAK,
        _ => m
            .mastering
            .and_then(|md| [md.max_luminance, md.max_cll as f64].into_iter().find(|&v| v > 0.0))
            .unwrap_or(PQ_DEFAULT_PEAK),
    };
    (nits / REF_WHITE).clamp(1.2, 10_000.0 / REF_WHITE)
}

/// The tone-map step: Möbius (exact below `KNEE`, rolled off up to `peak`) or a clip at reference white.
fn tonemap(peak: f64, cm: ColorManagement) -> String {
    match cm {
        ColorManagement::SdrClip => "tonemap=clip:desat=0".to_string(),
        _ => format!("tonemap=mobius:param={KNEE}:desat=0:peak={peak:.3}"),
    }
}

/// The filters (each ending in a comma; "" for an SDR source or the transform off) ahead of the decoder's
/// `scale=...,format=rgba`. They end in `format=gbrp`, so zimg does the colour conversion and dithering itself and
/// the scaler only resizes and repacks.
pub fn view_transform(m: &MediaItem, cm: ColorManagement) -> String {
    if cm == ColorManagement::Off {
        return String::new();
    }
    let tail = "zscale=t=bt709:r=pc,format=gbrp,";
    match m.hdr() {
        // PQ is absolute: 203 cd/m² -> 1.0 linear.
        Some(Hdr::Pq) => format!("zscale=t=linear:npl={REF_WHITE},format=gbrpf32le,zscale=p=bt709,{},{tail}", tonemap(peak(m), cm)),
        // HLG is decoded for a 1000 cd/m² display (its OOTF depends on the display), then exposed so that 203 cd/m²
        // (75 % HLG) is 1.0 linear.
        Some(Hdr::Hlg) => {
            let ev = (HLG_PEAK / REF_WHITE).log2();
            format!("zscale=t=linear:npl={HLG_PEAK},format=gbrpf32le,zscale=p=bt709,exposure=exposure={ev:.3},{},{tail}", tonemap(peak(m), cm))
        }
        Some(Hdr::Sdr2020) => "zscale=p=bt709:t=bt709:m=bt709:r=pc,format=gbrp,".to_string(),
        None if m.codec.as_deref() == Some("hdr") => format!("format=gbrpf32le,{},zscale=tin=linear:pin=bt709:t=iec61966-2-1:p=bt709:r=pc,format=gbrp,", tonemap(peak(m), cm)),
        // R38: linear EXR shown with a simple sRGB transfer (values above 1.0 clip) in an 8-bit project; R40b's float pipeline keeps it linear.
        None if linear(m) => "format=gbrpf32le,zscale=tin=linear:pin=bt709:t=iec61966-2-1:p=bt709:r=pc,format=gbrp,".to_string(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Mastering, MediaType};

    fn item(transfer: Option<&str>, primaries: Option<&str>) -> MediaItem {
        MediaItem { media_type: MediaType::Video, transfer: transfer.map(String::from), primaries: primaries.map(String::from), ..Default::default() }
    }

    /// R40a acceptance: the decode filter per transfer. SDR sources (tagged 709 or untagged) get nothing, so their
    /// 8-bit output is what it was before R40a.
    #[test]
    fn view_transform_per_transfer() {
        let cm = ColorManagement::Sdr;
        assert_eq!(view_transform(&item(None, None), cm), "");
        assert_eq!(view_transform(&item(Some("bt709"), Some("bt709")), cm), "");
        assert_eq!(view_transform(&item(Some("smpte170m"), Some("smpte170m")), cm), "");
        let pq = item(Some("smpte2084"), Some("bt2020"));
        assert_eq!(view_transform(&pq, cm), "zscale=t=linear:npl=203,format=gbrpf32le,zscale=p=bt709,tonemap=mobius:param=0.5:desat=0:peak=4.926,zscale=t=bt709:r=pc,format=gbrp,");
        // Mastering metadata sets the peak; MaxCLL when the display luminance is missing.
        let md = MediaItem { mastering: Some(Mastering { max_luminance: 4000.0, ..Default::default() }), ..pq.clone() };
        assert!(view_transform(&md, cm).contains(":peak=19.704,"));
        let md = MediaItem { mastering: Some(Mastering { max_cll: 600, ..Default::default() }), ..pq.clone() };
        assert!(view_transform(&md, cm).contains(":peak=2.956,"));
        assert_eq!(view_transform(&pq, ColorManagement::SdrClip), "zscale=t=linear:npl=203,format=gbrpf32le,zscale=p=bt709,tonemap=clip:desat=0,zscale=t=bt709:r=pc,format=gbrp,");
        assert_eq!(view_transform(&pq, ColorManagement::Off), "");
        let hlg = item(Some("arib-std-b67"), Some("bt2020"));
        assert_eq!(view_transform(&hlg, cm), "zscale=t=linear:npl=1000,format=gbrpf32le,zscale=p=bt709,exposure=exposure=2.300,tonemap=mobius:param=0.5:desat=0:peak=4.926,zscale=t=bt709:r=pc,format=gbrp,");
        assert_eq!(view_transform(&item(Some("bt2020-10"), Some("bt2020")), cm), "zscale=p=bt709:t=bt709:m=bt709:r=pc,format=gbrp,");
        assert_eq!(view_transform(&item(Some("bt709"), Some("bt2020")), cm), "zscale=p=bt709:t=bt709:m=bt709:r=pc,format=gbrp,");
        // Linear stills: EXR gets the sRGB transfer, Radiance HDR a tone map before it.
        let exr = MediaItem { codec: Some("exr".into()), ..item(Some("linear"), None) };
        assert_eq!(view_transform(&exr, cm), "format=gbrpf32le,zscale=tin=linear:pin=bt709:t=iec61966-2-1:p=bt709:r=pc,format=gbrp,");
        let hdr = MediaItem { codec: Some("hdr".into()), ..item(None, None) };
        assert!(view_transform(&hdr, cm).starts_with("format=gbrpf32le,tonemap=mobius:param=0.5:desat=0:peak=4.926,zscale=tin=linear"));
        assert_eq!((badge(&pq, cm), badge(&hlg, cm), badge(&item(None, None), cm), badge(&pq, ColorManagement::Off)), (Some("HDR → SDR"), Some("HDR → SDR"), None, None));
        assert_eq!((badge(&item(Some("bt2020-10"), Some("bt2020")), cm), badge(&exr, cm)), (Some("BT.2020 → Rec.709"), Some("linear → sRGB")));
        assert_eq!((pq.hdr(), hlg.hdr(), exr.hdr()), (Some(Hdr::Pq), Some(Hdr::Hlg), None));
        assert_eq!((pq.color_space_name().as_str(), hlg.color_space_name().as_str(), exr.color_space_name().as_str()), ("Rec.2020 ST.2084 (PQ, HDR10)", "Rec.2020 HLG", "Linear (scene-referred)"));
    }
}
