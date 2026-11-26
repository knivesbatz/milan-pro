//! R21c: a composed frame. The CPU compositor gives pixels; the GPU compositor gives the frame as a texture on its
//! device, and the pixels only when someone needs them (`image`: a render, the scopes, a snapshot, the Overlays
//! channel view), read back once. A preview on the UI's own device shows the texture as it is (`gpu`, through
//! `engine::gpu::register`), so playback never copies a frame off the GPU.
use eframe::egui::ColorImage;
use std::ops::Deref;
use std::sync::{Arc, OnceLock};
use crate::engine::gpu;

pub struct Picture {
    size: [usize; 2],
    cpu: OnceLock<Arc<ColorImage>>,
    gpu: Option<gpu::Output>,
}

impl Picture {
    pub fn cpu(img: Arc<ColorImage>) -> Picture {
        Picture { size: img.size, cpu: OnceLock::from(img), gpu: None }
    }

    pub(crate) fn on_gpu(out: gpu::Output) -> Picture {
        Picture { size: out.size(), cpu: OnceLock::new(), gpu: Some(out) }
    }

    /// The pixels (premultiplied gamma-space RGBA), read back from the GPU the first time.
    pub fn image(&self) -> Arc<ColorImage> {
        self.pixels().clone()
    }

    fn pixels(&self) -> &Arc<ColorImage> {
        self.cpu.get_or_init(|| match &self.gpu {
            Some(out) => Arc::new(out.read()),
            None => unreachable!("a picture has pixels or a texture"),
        })
    }

    /// The frame on the GPU, when the GPU composed it.
    pub fn gpu(&self) -> Option<&gpu::Output> {
        self.gpu.as_ref()
    }

    /// Bytes this picture holds (for the composite cache's budget).
    pub fn bytes(&self) -> usize {
        self.size[0] * self.size[1] * 4
    }
}

/// The pixels (`image`): reading them through the picture reads a GPU frame back too.
impl Deref for Picture {
    type Target = ColorImage;

    fn deref(&self) -> &ColorImage {
        self.pixels()
    }
}
