//! R21c: a composed frame. The CPU compositor gives pixels; the GPU compositor gives the frame as a texture on its
//! device, and the pixels only when someone needs them (`image`: a render, the scopes, a snapshot, the Overlays
//! channel view), read back once. A preview on the UI's own device shows the texture as it is (`gpu`, through
//! `engine::gpu::register`), so playback never copies a frame off the GPU.
//!
//! R40b: a frame of a 32-bit floating point project is also held in float (`wide`), on the CPU or the GPU; its
//! 8-bit pixels (`image`) are its view transform for the display (tone-mapped to SDR for an HDR view).
use eframe::egui::ColorImage;
use std::ops::Deref;
use std::sync::{Arc, OnceLock};
use crate::engine::gpu;
use crate::engine::wide::{WImage, Working};

pub struct Picture {
    size: [usize; 2],
    cpu: OnceLock<Arc<ColorImage>>,
    gpu: Option<gpu::Output>,
    wide: Option<Wide>,
}

/// R40b: the float frame and its working space.
struct Wide {
    wk: Arc<Working>,
    img: OnceLock<Arc<WImage>>,
    /// The float frame on the GPU (`gpu::WideOut`), read back when `wide` is asked for.
    gpu: Option<gpu::WideOut>,
}

impl Picture {
    pub fn cpu(img: Arc<ColorImage>) -> Picture {
        Picture { size: img.size, cpu: OnceLock::from(img), gpu: None, wide: None }
    }

    pub(crate) fn on_gpu(out: gpu::Output) -> Picture {
        Picture { size: out.size(), cpu: OnceLock::new(), gpu: Some(out), wide: None }
    }

    /// R40b: a float frame composed on the CPU.
    pub fn wide(img: Arc<WImage>, wk: Arc<Working>) -> Picture {
        Picture { size: img.size, cpu: OnceLock::new(), gpu: None, wide: Some(Wide { wk, img: OnceLock::from(img), gpu: None }) }
    }

    /// R40b: a float frame on the GPU: `float` holds it, `view` its display picture (what the preview shows).
    pub(crate) fn wide_on_gpu(float: gpu::WideOut, view: gpu::Output, wk: Arc<Working>) -> Picture {
        Picture { size: view.size(), cpu: OnceLock::new(), gpu: Some(view), wide: Some(Wide { wk, img: OnceLock::new(), gpu: Some(float) }) }
    }

    /// The pixels (premultiplied gamma-space RGBA), read back from the GPU the first time; a float frame through
    /// its view transform.
    pub fn image(&self) -> Arc<ColorImage> {
        self.pixels().clone()
    }

    fn pixels(&self) -> &Arc<ColorImage> {
        self.cpu.get_or_init(|| match (&self.gpu, &self.wide) {
            (Some(out), _) => Arc::new(out.read()),
            (None, Some(w)) => Arc::new(w.wk.to_preview(&self.float(w), crate::engine::composite::stripes())),
            (None, None) => unreachable!("a picture has pixels or a texture"),
        })
    }

    fn float(&self, w: &Wide) -> Arc<WImage> {
        w.img
            .get_or_init(|| match &w.gpu {
                Some(t) => Arc::new(t.read()),
                None => unreachable!("a float picture has its pixels or a texture"),
            })
            .clone()
    }

    /// R40b: a float frame (its pixels are not read back to say so).
    pub fn is_wide(&self) -> bool {
        self.wide.is_some()
    }

    /// R40b: the float frame and its working space (None for an 8-bit frame).
    pub fn wide_image(&self) -> Option<(Arc<WImage>, Arc<Working>)> {
        let w = self.wide.as_ref()?;
        Some((self.float(w), w.wk.clone()))
    }

    /// The frame on the GPU, when the GPU composed it (a float frame's display picture).
    pub fn gpu(&self) -> Option<&gpu::Output> {
        self.gpu.as_ref()
    }

    /// Bytes this picture holds (for the composite cache's budget).
    pub fn bytes(&self) -> usize {
        self.size[0] * self.size[1] * if self.wide.is_some() { 20 } else { 4 }
    }
}

/// The pixels (`image`): reading them through the picture reads a GPU frame back too.
impl Deref for Picture {
    type Target = ColorImage;

    fn deref(&self) -> &ColorImage {
        self.pixels()
    }
}
