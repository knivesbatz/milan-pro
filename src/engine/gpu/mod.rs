//! R21a: the GPU compositor, a `Compositor` on wgpu (Vulkan). `GpuCompositor::compose` gives the frame
//! `composite::compose` gives, within 2 in 255 per channel (bit-exact on every plan tried so far): draws (affine,
//! bilinear or nearest), opacity, Source Alpha and the integer blend modes use the CPU's own integer maths (the passes in
//! `composite.wgsl` work on `Rgba8Uint` textures), and the float blend modes (any mode over a transparent backdrop, Hard
//! Light to Luminosity, Cut, Multiply (Mask)) replay the CPU's f32 operations in order through `math.wgsl`, with
//! division and square root corrected to the rounded result. R21b: the video FX, transitions and generators with a
//! `GpuPort` (`engine/plugins.rs`) run here too (`fx.rs`, `fx.wgsl`), as event FX, track FX, Adjustment FX and Video
//! Output FX, with resampling, the flicker filter and the bus's Fade to Color. The CPU compositor stays the reference,
//! and does what the GPU does not:
//! - per piece, made by the CPU code and uploaded: decoding; an event with a mask, a nested timeline, or an FX without
//!   a port (Cookie Cutter, Bezier Masking, OFX); a whole track with such a track FX, 3D Track Motion or 2D Shadow /
//!   Glow (`composite::layer_image`); a transition without a port, or over such events (`composite::transition_image`);
//! - per layer, read back and done on the CPU: an Adjustment FX without a port and Custom compositors; after the
//!   frame: a Video Output FX without a port (`composite::finish`), and motion blur's mean (`Worker::frame`);
//! - per frame: a frame or picture larger than the device's textures, or any GPU error;
//! - always: no Vulkan adapter, Preferences › Video › GPU acceleration Off, or a lost device.
//!
//! The device: with the `wgpu` feature the UI paints through wgpu on the adapter Preferences › Video chose
//! (`wgpu_options`), and the compositor shares that device (`attach`), so a frame can stay on the GPU for the
//! preview (R21c). A preference naming another adapter, or the `glow` painter, opens the adapter on its own device
//! (`choose`), and every frame is read back. Renders use the same compositor as the preview (`Worker::frame`), so a
//! render shows what was previewed.
use eframe::egui::{Color32, ColorImage, TextureId};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering::Relaxed};
use std::sync::{Arc, Mutex, MutexGuard, Weak, mpsc};
use wgpu::util::DeviceExt;
use crate::engine::composite::{self, Affine, Compositor, Sources};
use crate::engine::picture::Picture;
use crate::engine::plan::{EventPlan, FramePlan, Item, SrcAt, TrackPlan};
use crate::engine::plugins::FxEval;
use crate::project::Composite;

pub mod fx;
pub mod mvfx;

/// Preferences › Video › GPU acceleration: this value turns it off ("" is automatic, anything else an adapter's name).
pub const OFF: &str = "Off";

/// The passes' textures: integers, so the CPU's 8-bit maths replays exactly.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Uint;
/// Textures kept between frames for reuse, at most this many bytes.
const POOL_BYTES: usize = 256 << 20;


/// R21c: the format a composed frame is kept in on the GPU (`GpuCompositor::set_intermediate`). The passes compose in
/// 8-bit either way today; `Rgba16Float` holds the result in linear light with 16-bit float channels, the format
/// R40b's float pipeline composes in. The preview samples both; a readback converts both to the 8-bit picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Intermediate {
    #[default]
    Rgba8,
    Rgba16Float,
}

impl Intermediate {
    fn format(self) -> wgpu::TextureFormat {
        match self {
            Intermediate::Rgba8 => wgpu::TextureFormat::Rgba8Unorm,
            Intermediate::Rgba16Float => wgpu::TextureFormat::Rgba16Float,
        }
    }
}

/// R21c: a composed frame on the GPU: the compositor's output in its `Intermediate` format, with the view the UI
/// samples (sRGB-decoding for 8-bit, linear for float, as egui expects). Back to the compositor's pool when dropped.
pub struct Output {
    g: Weak<GpuCompositor>,
    tex: Option<OutTex>,
}

struct OutTex {
    tex: wgpu::Texture,
    /// The render target (raw bytes).
    target: wgpu::TextureView,
    /// What the UI samples (feature `wgpu`).
    #[cfg_attr(not(feature = "wgpu"), allow(dead_code))]
    sample: wgpu::TextureView,
    format: Intermediate,
    w: u32,
    h: u32,
}

impl Output {
    pub fn size(&self) -> [usize; 2] {
        let t = self.tex.as_ref().expect("an output until dropped");
        [t.w as usize, t.h as usize]
    }

    #[cfg_attr(not(test), allow(dead_code))] // R40b composes in float: the preview asks then
    pub fn format(&self) -> Intermediate {
        self.tex.as_ref().expect("an output until dropped").format
    }

    /// The pixels, read back (premultiplied gamma-space RGBA; a float frame converted).
    pub(crate) fn read(&self) -> ColorImage {
        let t = self.tex.as_ref().expect("an output until dropped");
        match self.g.upgrade() {
            Some(g) => g.read_output(t),
            None => ColorImage::new([t.w as usize, t.h as usize], Color32::BLACK),
        }
    }

    /// Registered with the UI's painter as an egui texture (`register`); None when the UI does not paint through
    /// this device (or paints through OpenGL, feature `glow`).
    pub fn register(&self) -> Option<TextureId> {
        #[cfg(feature = "wgpu")]
        {
            let t = self.tex.as_ref()?;
            let g = self.g.upgrade().filter(|g| g.shared)?;
            let ui = lock(&STATE).ui.clone()?;
            let mut r = ui.renderer.write();
            Some(r.register_native_texture(&g.device, &t.sample, wgpu::FilterMode::Linear))
        }
        #[cfg(not(feature = "wgpu"))]
        None
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        if let (Some(g), Some(t)) = (self.g.upgrade(), self.tex.take()) {
            g.give_output(t);
        }
    }
}

/// `Output::register`'s texture freed again, once the frame it showed is gone.
pub fn unregister(id: TextureId) {
    #[cfg(feature = "wgpu")]
    if let Some(ui) = lock(&STATE).ui.clone() {
        ui.renderer.write().free_texture(&id);
    }
    #[cfg(not(feature = "wgpu"))]
    let _ = id;
}

/// The UI's painter (feature `wgpu`): where `Output::register` lands.
#[cfg(feature = "wgpu")]
#[derive(Clone)]
struct Ui {
    renderer: Arc<eframe::egui::mutex::RwLock<eframe::egui_wgpu::Renderer>>,
}

#[cfg(not(feature = "wgpu"))]
#[derive(Clone)]
struct Ui;

pub(super) struct Tex {
    tex: wgpu::Texture,
    view: wgpu::TextureView,
    pub(super) w: u32,
    pub(super) h: u32,
    /// Written from the CPU (`queue.write_texture`): such a write lands before the frame's passes run, so this
    /// texture is reused only after the frame.
    cpu: bool,
}

pub struct GpuCompositor {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pub name: String,
    /// The UI paints with this device too (`attach`): a frame can stay on it for the preview.
    pub shared: bool,
    me: Weak<GpuCompositor>,
    /// `Intermediate` as a u8.
    intermediate: AtomicU8,
    /// R21c: frames and pieces read back into `ColorImage`s so far (a zero-copy preview adds none).
    readbacks: AtomicU64,
    layout: wgpu::BindGroupLayout,
    draw: wgpu::RenderPipeline,
    blend: wgpu::RenderPipeline,
    fade: wgpu::RenderPipeline,
    /// R21b: the FX, transitions and generators.
    fx: fx::Pipes,
    /// MVFX: the music-video FX and transitions' kernels.
    mv: mvfx::Pipes,
    /// Bound where a pass reads no source / no rows.
    none: Tex,
    no_rows: wgpu::Buffer,
    max: u32,
    /// A validation error, a panic inside wgpu or a lost device: the CPU composes from then on.
    broken: Arc<AtomicBool>,
    pool: Mutex<Vec<Tex>>,
    /// Output textures (`Output`) kept for reuse.
    outputs: Mutex<Vec<OutTex>>,
    /// Held while a frame records one step (a pass, a clear, an upload) and while it creates that step's buffers and
    /// bind group. Two threads composing at once (a render beside the preview) and recording freely lose draws now
    /// and then on wgpu 24's Vulkan backend (the parity tests run in parallel reproduce it: a layer comes out
    /// transparent); one step at a time, never. The steps are short, and decoding happens between them, unlocked.
    record: Mutex<()>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// wgpu's futures are ready at once on native backends.
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

/// An IEEE half to f32 (a readback of an `Rgba16Float` frame).
fn half(v: u16) -> f32 {
    let (s, e, m) = ((v >> 15) as u32, ((v >> 10) & 31) as u32, (v & 1023) as u32);
    let bits = match e {
        0 if m == 0 => s << 31,
        0 => {
            // Subnormal: normalise.
            let mut e = 127 - 15 + 1;
            let mut m = m;
            while m & 1024 == 0 {
                m <<= 1;
                e -= 1;
            }
            (s << 31) | (e << 23) | ((m & 1023) << 13)
        }
        31 => (s << 31) | (255 << 23) | (m << 13),
        _ => (s << 31) | ((e + 127 - 15) << 23) | (m << 13),
    };
    f32::from_bits(bits)
}

/// The limits the compositor asks of a device: the adapter's own (frames up to its largest texture).
fn device_descriptor(adapter: &wgpu::Adapter) -> wgpu::DeviceDescriptor<'static> {
    wgpu::DeviceDescriptor { label: Some("milan"), required_features: wgpu::Features::empty(), required_limits: adapter.limits(), memory_hints: wgpu::MemoryHints::MemoryUsage }
}

impl GpuCompositor {
    /// A device of its own on `adapter`.
    fn open(adapter: &wgpu::Adapter) -> Result<Arc<GpuCompositor>, String> {
        let name = adapter.get_info().name;
        let (device, queue) = block_on(adapter.request_device(&device_descriptor(adapter), None)).map_err(|e| format!("{name} could not be opened ({e})"))?;
        Self::on(name, device, queue, false)
    }

    /// The compositor on `device` (`shared`: the UI's).
    fn on(name: String, device: wgpu::Device, queue: wgpu::Queue, shared: bool) -> Result<Arc<GpuCompositor>, String> {
        let broken = Arc::new(AtomicBool::new(false));
        let b = broken.clone();
        // The UI's own errors (there are none) would land here too: they are still reported, and the compositor steps
        // aside either way.
        device.on_uncaptured_error(Box::new(move |e| {
            log::warn!("wgpu: {e}");
            b.store(true, Relaxed);
        }));
        if !shared {
            let b = broken.clone();
            device.set_device_lost_callback(move |_, _| b.store(true, Relaxed));
        }
        let math = include_str!("math.wgsl");
        let composite_src = format!("{math}\n{}", include_str!("composite.wgsl"));
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("composite.wgsl"), source: wgpu::ShaderSource::Wgsl(composite_src.into()) });
        let stage = wgpu::ShaderStages::FRAGMENT;
        let tex = |binding| wgpu::BindGroupLayoutEntry { binding, visibility: stage, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Uint, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None };
        let buf = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: stage, ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None }, count: None };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[tex(0), tex(1), buf(2, wgpu::BufferBindingType::Uniform), buf(3, wgpu::BufferBindingType::Storage { read_only: true })],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&layout], push_constant_ranges: &[] });
        let pipe_to = |module: &wgpu::ShaderModule, fs: &str, format: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fs),
                layout: Some(&pl),
                vertex: wgpu::VertexState { module, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(fs),
                    targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        let pipe = |module: &wgpu::ShaderModule, fs: &str| pipe_to(module, fs, FORMAT);
        let (draw, blend, fade) = (pipe(&shader, "fs_draw"), pipe(&shader, "fs_blend"), pipe(&shader, "fs_fade"));
        let fx = fx::Pipes::new(&device, &layout, math, &pipe_to);
        let mv = mvfx::Pipes::new(&device, math, &pipe_to);
        let none = new_tex(&device, 1, 1, false);
        // Cleared once, so no frame's submission has to zero it.
        {
            let mut enc = device.create_command_encoder(&Default::default());
            let ops = wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store };
            let at = wgpu::RenderPassColorAttachment { view: &none.view, resolve_target: None, ops };
            drop(enc.begin_render_pass(&wgpu::RenderPassDescriptor { color_attachments: &[Some(at)], ..Default::default() }));
            queue.submit([enc.finish()]);
        }
        let no_rows = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 8, usage: wgpu::BufferUsages::STORAGE, mapped_at_creation: false });
        let max = device.limits().max_texture_dimension_2d;
        device.poll(wgpu::Maintain::Wait);
        if broken.load(Relaxed) {
            return Err(format!("{name} could not build the compositor's shaders"));
        }
        Ok(Arc::new_cyclic(|me| GpuCompositor {
            device,
            queue,
            name,
            shared,
            me: me.clone(),
            intermediate: AtomicU8::new(0),
            readbacks: AtomicU64::new(0),
            layout,
            draw,
            blend,
            fade,
            fx,
            mv,
            none,
            no_rows,
            max,
            broken,
            pool: Mutex::new(Vec::new()),
            outputs: Mutex::new(Vec::new()),
            record: Mutex::new(()),
        }))
    }

    /// R21c: the format composed frames are kept in (`Intermediate`), for the frames composed from now on.
    #[cfg_attr(not(test), allow(dead_code))] // R40b's Project Properties pixel format sets it
    pub fn set_intermediate(&self, f: Intermediate) {
        self.intermediate.store(f as u8, Relaxed);
    }

    /// R21c: how many frames (or pieces a CPU step needed) this compositor has read back from the GPU.
    pub fn readbacks(&self) -> u64 {
        self.readbacks.load(Relaxed)
    }

    pub fn intermediate(&self) -> Intermediate {
        match self.intermediate.load(Relaxed) {
            1 => Intermediate::Rgba16Float,
            _ => Intermediate::Rgba8,
        }
    }

    /// A `w` x `h` output texture in `format`, from its pool or new.
    fn output(&self, w: u32, h: u32, format: Intermediate) -> OutTex {
        let mut pool = lock(&self.outputs);
        if let Some(i) = pool.iter().position(|t| (t.w, t.h, t.format) == (w, h, format)) {
            return pool.remove(i);
        }
        let f = format.format();
        let srgb = format == Intermediate::Rgba8;
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("milan-frame"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: f,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: if srgb { &[wgpu::TextureFormat::Rgba8UnormSrgb] } else { &[] },
        });
        let target = tex.create_view(&Default::default());
        let sample = tex.create_view(&wgpu::TextureViewDescriptor { format: srgb.then_some(wgpu::TextureFormat::Rgba8UnormSrgb), ..Default::default() });
        OutTex { tex, target, sample, format, w, h }
    }

    /// An output texture back to its pool (at most `POOL_BYTES` / 4 of them).
    fn give_output(&self, t: OutTex) {
        let mut pool = lock(&self.outputs);
        pool.push(t);
        let bytes = |t: &OutTex| t.w as usize * t.h as usize * if t.format == Intermediate::Rgba8 { 4 } else { 8 };
        let mut total: usize = pool.iter().map(bytes).sum();
        while total > POOL_BYTES / 4 {
            total -= bytes(&pool.remove(0));
        }
    }

    /// `Output::read`: the frame's pixels, a float frame converted to 8-bit gamma space.
    fn read_output(&self, t: &OutTex) -> ColorImage {
        self.readbacks.fetch_add(1, Relaxed);
        let bpp = if t.format == Intermediate::Rgba8 { 4 } else { 8 };
        let row = (t.w * bpp).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: row as u64 * t.h as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = self.device.create_command_encoder(&Default::default());
        let layout = wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None };
        enc.copy_texture_to_buffer(t.tex.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &buf, layout }, t.tex.size());
        self.queue.submit([enc.finish()]);
        let (tx, rx) = mpsc::channel();
        buf.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device.poll(wgpu::Maintain::Wait);
        let (w, h) = (t.w as usize, t.h as usize);
        if rx.recv().ok().and_then(Result::ok).is_none() || self.broken() {
            return ColorImage::new([w, h], Color32::BLACK);
        }
        let data = buf.slice(..).get_mapped_range();
        let mut pixels = Vec::with_capacity(w * h);
        for y in 0..h {
            let line = &data[y * row as usize..];
            if t.format == Intermediate::Rgba8 {
                pixels.extend_from_slice(bytemuck::cast_slice::<u8, Color32>(&line[..w * 4]));
            } else {
                let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                for px in bytemuck::cast_slice::<u8, [u16; 4]>(&line[..w * 8]) {
                    let [r, g, b, a] = px.map(|v| f32::from(half(v)));
                    pixels.push(Color32::from_rgba_premultiplied(to8(crate::engine::vfx::to_srgb(r)), to8(crate::engine::vfx::to_srgb(g)), to8(crate::engine::vfx::to_srgb(b)), to8(a)));
                }
            }
        }
        ColorImage { size: [w, h], pixels }
    }

    pub fn broken(&self) -> bool {
        self.broken.load(Relaxed)
    }

    /// A `w` x `h` texture from the pool, or a new one.
    fn texture(&self, w: u32, h: u32, cpu: bool) -> Tex {
        let mut pool = lock(&self.pool);
        match pool.iter().position(|t| (t.w, t.h) == (w, h)) {
            Some(i) => Tex { cpu, ..pool.remove(i) },
            None => new_tex(&self.device, w, h, cpu),
        }
    }

    /// Textures back to the pool, the oldest dropped past `POOL_BYTES`.
    fn give_back(&self, ts: impl IntoIterator<Item = Tex>) {
        let mut pool = lock(&self.pool);
        pool.extend(ts);
        let mut bytes: usize = pool.iter().map(|t| t.w as usize * t.h as usize * 4).sum();
        while bytes > POOL_BYTES {
            let t = pool.remove(0);
            bytes -= t.w as usize * t.h as usize * 4;
        }
    }
}

impl Compositor for GpuCompositor {
    fn name(&self) -> String {
        format!("{} (Vulkan)", self.name)
    }

    /// `composite::compose` on the GPU, the frame left there (`Picture::gpu`); None when this frame cannot be done
    /// here (the caller composes it on the CPU).
    fn compose(&self, fp: &FramePlan, src: &mut dyn Sources, w: usize, h: usize, bilinear: bool, k: usize) -> Option<Picture> {
        if self.broken() || w == 0 || h == 0 || w.max(h) > self.max as usize {
            return None;
        }
        let run = || {
            let enc = self.device.create_command_encoder(&Default::default());
            let mut f = Frame { g: self, enc, w: w as u32, h: h as u32, spare: Vec::new(), written: Vec::new(), bilinear, k };
            let out = f.frame(fp, src);
            f.done();
            out
        };
        // A panic inside wgpu (a driver gone bad) must not take the editor down: the CPU takes over.
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).unwrap_or_else(|_| {
            self.broken.store(true, Relaxed);
            None
        });
        let out = out.filter(|_| !self.broken())?;
        Some(Picture::on_gpu(Output { g: self.me.clone(), tex: Some(out) }))
    }
}

fn new_tex(device: &wgpu::Device, w: u32, h: u32, cpu: bool) -> Tex {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = tex.create_view(&Default::default());
    Tex { tex, view, w, h, cpu }
}

/// One frame being composed: its passes are recorded into `enc`, submitted at a readback.
pub(super) struct Frame<'a> {
    pub(super) g: &'a GpuCompositor,
    enc: wgpu::CommandEncoder,
    w: u32,
    h: u32,
    /// Targets free for later passes (the GPU runs a frame's passes in order), of any size.
    spare: Vec<Tex>,
    /// Textures written from the CPU, back to the pool after the frame.
    written: Vec<Tex>,
    bilinear: bool,
    k: usize,
}

impl Frame<'_> {
    fn frame(&mut self, fp: &FramePlan, src: &mut dyn Sources) -> Option<OutTex> {
        let (w, h) = (self.w, self.h);
        let mut acc = self.clear(w, h, if fp.alpha { Color32::TRANSPARENT } else { Color32::BLACK });
        let mut group: Option<Tex> = None;
        // As `composite::compose`: a compositing parent's children first, then the parent, then the group through
        // Parent Motion with the Parent Composite Mode.
        for l in &fp.layers {
            if l.child {
                let g = match group.take() {
                    Some(g) => g,
                    None => self.clear(w, h, Color32::TRANSPARENT),
                };
                group = Some(self.layer_onto(g, src, l, true)?);
                continue;
            }
            let Some((mode, motion)) = l.parent else {
                if let Some(g) = group.take() {
                    acc = self.blend(acc, g, Composite::SourceAlpha, 1.0, false)?;
                }
                acc = self.layer_onto(acc, src, l, false)?;
                continue;
            };
            let mut g = match group.take() {
                Some(g) => g,
                None => self.clear(w, h, Color32::TRANSPARENT),
            };
            if !l.items.is_empty() || !l.adjust.is_empty() {
                g = self.layer_onto(g, src, l, true)?;
            }
            if composite::whole_pixel(motion) != Some((0, 0)) {
                let (wf, hf) = (w as f64, h as f64);
                let blank = self.clear(w, h, Color32::TRANSPARENT);
                let moved = self.draw(blank, &g, Affine::scale(1.0 / wf, 1.0 / hf).then(motion).then(Affine::scale(wf, hf)), 1.0);
                self.give(g);
                g = moved;
            }
            acc = self.blend(acc, g, mode, 1.0, false)?;
        }
        if let Some(g) = group {
            acc = self.blend(acc, g, Composite::SourceAlpha, 1.0, false)?;
        }
        // `composite::finish`: the Video Output FX (here when every one has a port), then the bus's Fade to Color.
        if fp.out_fx.iter().all(fx::ported) {
            if let Some(out) = self.fx_chain(&acc, &fp.out_fx) {
                self.give(acc);
                acc = self.opaque(&out);
                self.give(out);
            }
            if let Some((c, t)) = fp.fade {
                acc = self.fade(acc, c, t);
            }
            let out = self.present(&acc);
            self.give(acc);
            return Some(out);
        }
        let mut img = self.read(&acc)?;
        self.give(acc);
        composite::finish(&mut img, fp, self.k);
        let t = self.upload(&img)?;
        let out = self.present(&t);
        self.give(t);
        Some(out)
    }

    /// The frame `acc` into an output texture (`Intermediate`), submitted: the frame is complete on the GPU.
    fn present(&mut self, acc: &Tex) -> OutTex {
        let g = self.g;
        let format = g.intermediate();
        let out = g.output(acc.w, acc.h, format);
        {
            let _step = lock(&g.record);
            let pipe = match format {
                Intermediate::Rgba8 => &g.fx.present8,
                Intermediate::Rgba16Float => &g.fx.present16,
            };
            let bind = self.bind(&g.layout, acc, None, &fx::U::default().words(), &[], None);
            let ops = wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store };
            let at = wgpu::RenderPassColorAttachment { view: &out.target, resolve_target: None, ops };
            let mut rp = self.enc.begin_render_pass(&wgpu::RenderPassDescriptor { color_attachments: &[Some(at)], ..Default::default() });
            rp.set_pipeline(pipe);
            rp.set_bind_group(0, &bind, &[]);
            rp.draw(0..3, 0..1);
        }
        let enc = std::mem::replace(&mut self.enc, g.device.create_command_encoder(&Default::default()));
        g.queue.submit([enc.finish()]);
        out
    }

    /// `composite::fade_to` on `t`.
    fn fade(&mut self, t: Tex, c: [u8; 3], how_far: f32) -> Tex {
        let p = params([0; 4], [0; 4], [0; 4], [c[0] as u32, c[1] as u32, c[2] as u32, 0], [0.0, how_far, 0.0, 0.0]);
        let pipe = &self.g.fade;
        let out = self.pass(pipe, &t, None, &p, &[]);
        self.give(t);
        out
    }

    /// `composite::layer_onto`.
    fn layer_onto(&mut self, mut dst: Tex, src: &mut dyn Sources, l: &TrackPlan, group: bool) -> Option<Tex> {
        let k = self.k;
        // R18b: an Adjustment Track's or Adjustment Event's FX change what is below first (`composite::adjust`).
        for (fx, a) in &l.adjust {
            if fx.iter().all(fx::ported) {
                if let Some(out) = self.fx_chain(&dst, fx) {
                    let mixed = if *a >= 1.0 { out } else {
                        let m = self.lerp(&dst, &out, *a);
                        self.give(out);
                        m
                    };
                    self.give(dst);
                    dst = mixed;
                }
            } else {
                dst = self.on_cpu(dst, |img| composite::adjust(img, fx, *a, k))?;
            }
        }
        if l.items.is_empty() {
            return Some(dst);
        }
        let (w, h) = (self.w, self.h);
        let fx_here = l.fx.iter().all(fx::ported);
        if !fx_here || l.persp.is_some() || l.shadow.is_some() || l.glow.is_some() || l.mode == Composite::Custom {
            // shortcut: a track FX without a port, 3D Track Motion, 2D Shadow / Glow and Custom compositors make the
            // whole track on the CPU (exact, but no faster than the CPU path).
            let layer = composite::layer_image(src, l, [w as usize, h as usize], self.bilinear, k);
            if l.mode == Composite::Custom {
                return self.on_cpu(dst, |d| composite::blend(d, &layer, l.mode, l.opacity, l.compositor.as_ref(), group, k));
            }
            let t = self.upload(&layer)?;
            return self.blend(dst, t, l.mode, l.opacity, group);
        }
        let draws: usize = l.items.iter().map(|i| composite::events(i).len()).sum();
        if l.fx.is_empty() && l.fade.is_none() && matches!(l.mode, Composite::SourceAlpha | Composite::ThreeD) && (l.opacity >= 1.0 || draws <= 1) {
            return self.draw_items(dst, src, l, l.opacity, true);
        }
        // `composite::layer_image`: the events, the track FX on the track's own frame then Track Motion, Fade to Color.
        let blank = self.clear(w, h, Color32::TRANSPARENT);
        let mut layer = self.draw_items(blank, src, l, 1.0, l.fx.is_empty())?;
        if let Some(out) = self.fx_chain(&layer, &l.fx) {
            self.give(layer);
            let blank = self.clear(w, h, Color32::TRANSPARENT);
            layer = self.draw(blank, &out, Affine::scale(1.0 / w as f64, 1.0 / h as f64).then(composite::layer_to_out(l, w as usize, h as usize)), 1.0);
            self.give(out);
        }
        if let Some((c, t)) = l.fade {
            layer = self.fade(layer, c, t);
        }
        self.blend(dst, layer, l.mode, l.opacity, group)
    }

    /// Every step of `e`'s picture has a port here: no nested timeline (composed by the CPU code), no masks
    /// (rasterised there), no FX without a port.
    fn event_here(e: &EventPlan) -> bool {
        e.src.nested.is_none() && e.masks.is_empty() && e.fx.iter().all(fx::ported)
    }

    /// `composite::event_image` on the GPU: the source (a ported generator made here, else fetched and uploaded),
    /// resampling, the flicker filter, the event FX. Err: the device cannot hold it (the frame goes to the CPU);
    /// Ok(None): nothing to draw.
    fn event_tex(&mut self, src: &mut dyn Sources, e: &EventPlan, to_out: Affine) -> Result<Option<Tex>, ()> {
        if !Self::event_here(e) {
            let Some(img) = composite::event_image(src, e, to_out, self.bilinear, self.k) else { return Ok(None) };
            return self.upload(&img).map(Some).ok_or(());
        }
        let (dw, dh) = composite::decode_size(e, to_out);
        let mut t = match &e.src.generator {
            Some((g, time)) if fx::ported(g) => self.generate(g, dw, dh, *time),
            _ => {
                let Some(img) = src.get(&e.src, dw, dh) else { return Ok(None) };
                self.upload(&img).ok_or(())?
            }
        };
        if let Some((n, w)) = e.next {
            if let Some(b) = src.get(&SrcAt { m: e.src.m.clone(), sf: Some(n), generator: None, nested: None }, dw, dh) {
                if b.size == [t.w as usize, t.h as usize] {
                    let bt = self.upload(&b).ok_or(())?;
                    let mixed = self.resample(&t, &bt, w);
                    self.give(t);
                    self.give(bt);
                    t = mixed;
                }
            }
        }
        if e.flicker && t.h >= 2 {
            let f = self.flicker(&t);
            self.give(t);
            t = f;
        }
        if crate::engine::mvfx::needs_sources(&e.fx) {
            t = self.mv_event(t, e, src, (dw, dh))?;
        } else if let Some(out) = self.fx_chain(&t, &e.fx) {
            self.give(t);
            t = out;
        }
        Ok(Some(t))
    }

    /// `composite::transition_image`: both sides as whole frames without Track Motion, mixed; None: the CPU composes.
    fn transition_tex(&mut self, src: &mut dyn Sources, a: Option<&EventPlan>, b: Option<&EventPlan>, p: f32, kind: &FxEval, to_out: Affine) -> Option<Tex> {
        let (w, h) = (self.w, self.h);
        if !fx::ported(kind) || !a.into_iter().chain(b).all(Self::event_here) {
            let img = composite::transition_image(src, a, b, p, kind, to_out, [w as usize, h as usize], self.bilinear, self.k);
            return self.upload(&img);
        }
        let frame = Affine::scale(w as f64, h as f64);
        let mut side = |f: &mut Self, e: Option<&EventPlan>| -> Option<Tex> {
            let mut t = f.clear(w, h, Color32::TRANSPARENT);
            if let Some(e) = e {
                if let Some(img) = f.event_tex(src, e, to_out).ok()? {
                    t = f.draw(t, &img, composite::event_to_size([img.w as usize, img.h as usize], e, frame), e.alpha);
                    f.give(img);
                }
            }
            Some(t)
        };
        if p <= 0.0 {
            return side(self, a);
        }
        if p >= 1.0 {
            return side(self, b);
        }
        let ta = side(self, a)?;
        let tb = side(self, b)?;
        Some(self.transition(ta, tb, kind, p))
    }

    /// `composite::draw_items`: each item's picture drawn at `gain`, through Track Motion when `with_motion`.
    fn draw_items(&mut self, mut dst: Tex, src: &mut dyn Sources, l: &TrackPlan, gain: f32, with_motion: bool) -> Option<Tex> {
        let (w, h) = (self.w as usize, self.h as usize);
        let to_out = composite::layer_to_out(l, w, h);
        let place_out = if with_motion { to_out } else { Affine::scale(w as f64, h as f64) };
        for item in &l.items {
            match item {
                Item::Mix { a, b, p, kind: Some(kind) } => {
                    let t = self.transition_tex(src, a.as_ref(), b.as_ref(), *p, kind, to_out)?;
                    let m = if with_motion { Affine::scale(1.0 / w as f64, 1.0 / h as f64).then(to_out) } else { Affine::translate(0.0, 0.0) };
                    dst = self.draw(dst, &t, m, gain);
                    self.give(t);
                }
                _ => {
                    // An unknown transition draws as a dissolve: A, then B at alpha p (or A alone at 1 - p, out to nothing).
                    let (p, out) = if let Item::Mix { p, b, .. } = item { (*p, b.is_none()) } else { (1.0, false) };
                    let evs = composite::events(item);
                    let last = evs.len().saturating_sub(1);
                    for (i, e) in evs.into_iter().enumerate() {
                        let alpha = e.alpha * gain * if out { 1.0 - p } else if i == last { p } else { 1.0 };
                        if alpha <= 0.0 {
                            continue;
                        }
                        let Some(t) = self.event_tex(src, e, to_out).ok()? else { continue };
                        dst = self.draw(dst, &t, composite::event_to_size([t.w as usize, t.h as usize], e, place_out), alpha);
                        self.give(t);
                    }
                }
            }
        }
        Some(dst)
    }

    /// `composite::draw`: `s` through `m` at `alpha` onto `dst`, at the frame's quality.
    fn draw(&mut self, dst: Tex, s: &Tex, m: Affine, alpha: f32) -> Tex {
        self.draw_with(dst, s, m, alpha, self.bilinear)
    }

    /// `composite::draw`, bilinear or nearest as asked.
    pub(super) fn draw_with(&mut self, dst: Tex, s: &Tex, m: Affine, alpha: f32, bilinear: bool) -> Tex {
        let a8 = (alpha.clamp(0.0, 1.0) * 255.0).round() as u32;
        let (size, out) = ([s.w as usize, s.h as usize], [dst.w as usize, dst.h as usize]);
        let Some(inv) = m.inverse() else { return dst };
        if a8 == 0 || s.w == 0 || s.h == 0 {
            return dst;
        }
        let Some([x0, x1, y0, y1]) = composite::bounds(m, size, out) else { return dst };
        let i32_of = |v: i64| v.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        let (p, rows) = match composite::whole_pixel(m) {
            Some((ox, oy)) => {
                let Some([x0, x1, y0, y1]) = composite::spans((ox, oy), size, out) else { return dst };
                let rect = [x0, y0, x1, y1].map(|v| v as u32);
                (params(rect, [0; 4], [i32_of(ox) as u32, i32_of(oy) as u32, 1, 0], [a8, 0, 0, 0], [0.0; 4]), Vec::new())
            }
            None => {
                let (du, dv) = ((inv.a * composite::ONE).round() as i64, (inv.c * composite::ONE).round() as i64);
                let uv = [i32_of(du), i32_of(dv), i32_of((s.w as i64) << 16), i32_of((s.h as i64) << 16)].map(|v| v as u32);
                // Each row's start exactly as the CPU computes it (f64), so both step through the same source pixels.
                let rows: Vec<[i32; 2]> = (y0..y1)
                    .map(|y| {
                        let (u0, v0) = inv.apply(x0 as f64 + 0.5, y as f64 + 0.5);
                        [i32_of((u0 * composite::ONE).floor() as i64), i32_of((v0 * composite::ONE).floor() as i64)]
                    })
                    .collect();
                (params([x0, y0, x1, y1].map(|v| v as u32), uv, [0, 0, 0, bilinear as u32], [a8, 0, 0, 0], [0.0; 4]), rows)
            }
        };
        let pipe = &self.g.draw;
        let out = self.pass(pipe, &dst, Some(s), &p, bytemuck::cast_slice(&rows));
        self.give(dst);
        out
    }

    /// `composite::blend` of `layer` onto `dst` (Custom on the CPU).
    fn blend(&mut self, dst: Tex, layer: Tex, mode: Composite, opacity: f32, group: bool) -> Option<Tex> {
        if mode == Composite::Custom {
            let (l, k) = (self.read(&layer)?, self.k);
            self.give(layer);
            return self.on_cpu(dst, |d| composite::blend(d, &l, mode, opacity, None, group, k));
        }
        let o8 = (opacity.clamp(0.0, 1.0) * 255.0).round() as u32;
        let p = params([0; 4], [0; 4], [0; 4], [o8, mode as u32, group as u32, 0], [o8 as f32 / 255.0, 0.0, 0.0, 0.0]);
        let pipe = &self.g.blend;
        let out = self.pass(pipe, &dst, Some(&layer), &p, &[]);
        self.give(dst);
        self.give(layer);
        Some(out)
    }

    /// A `w` x `h` render target.
    fn target(&mut self, w: u32, h: u32) -> Tex {
        match self.spare.iter().position(|t| (t.w, t.h) == (w, h)) {
            Some(i) => self.spare.swap_remove(i),
            None => self.g.texture(w, h, false),
        }
    }

    pub(super) fn give(&mut self, t: Tex) {
        if t.cpu { self.written.push(t) } else { self.spare.push(t) }
    }

    /// A `w` x `h` target filled with `c`.
    pub(super) fn clear(&mut self, w: u32, h: u32, c: Color32) -> Tex {
        let _step = lock(&self.g.record);
        let t = self.target(w, h);
        let [r, g, b, a] = c.to_array().map(f64::from);
        let ops = wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }), store: wgpu::StoreOp::Store };
        let at = wgpu::RenderPassColorAttachment { view: &t.view, resolve_target: None, ops };
        drop(self.enc.begin_render_pass(&wgpu::RenderPassDescriptor { color_attachments: &[Some(at)], ..Default::default() }));
        t
    }

    fn bind(&self, layout: &wgpu::BindGroupLayout, dst: &Tex, src: Option<&Tex>, uniform: &[u32], data: &[u32], out: Option<&Tex>) -> wgpu::BindGroup {
        let g = self.g;
        let uniform = g.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(uniform), usage: wgpu::BufferUsages::UNIFORM });
        let data = (!data.is_empty()).then(|| g.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(data), usage: wgpu::BufferUsages::STORAGE }));
        let mut entries = vec![
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&dst.view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&src.unwrap_or(&g.none).view) },
            wgpu::BindGroupEntry { binding: 2, resource: uniform.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: data.as_ref().unwrap_or(&g.no_rows).as_entire_binding() },
        ];
        if let Some(out) = out {
            entries.push(wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&out.view) });
        }
        g.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout, entries: &entries })
    }

    /// One pass of `pipe` into a new target of `dst`'s size, reading `dst` and `src` with `uniform` and `data`
    /// (a storage buffer). `dst` is left as it was.
    pub(super) fn pass(&mut self, pipe: &wgpu::RenderPipeline, dst: &Tex, src: Option<&Tex>, uniform: &[u32], data: &[u32]) -> Tex {
        let _step = lock(&self.g.record);
        let out = self.target(dst.w, dst.h);
        let bind = self.bind(&self.g.layout, dst, src, uniform, data, None);
        {
            let ops = wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store };
            let at = wgpu::RenderPassColorAttachment { view: &out.view, resolve_target: None, ops };
            let mut rp = self.enc.begin_render_pass(&wgpu::RenderPassDescriptor { color_attachments: &[Some(at)], ..Default::default() });
            rp.set_pipeline(pipe);
            rp.set_bind_group(0, &bind, &[]);
            rp.draw(0..3, 0..1);
        }
        out
    }

    /// `cs_box` (fx.wgsl) over `t`: one invocation per line along the axis `u` names.
    pub(super) fn box_pass(&mut self, t: &Tex, u: &fx::U) -> Tex {
        let _step = lock(&self.g.record);
        let out = self.target(t.w, t.h);
        let along_x = u.i[1] == 0;
        let lines = if along_x { t.h } else { t.w };
        let bind = self.bind(&self.g.fx.box_layout, t, None, &u.words(), &[], Some(&out));
        {
            let mut cp = self.enc.begin_compute_pass(&Default::default());
            cp.set_pipeline(&self.g.fx.box_blur);
            cp.set_bind_group(0, &bind, &[]);
            cp.dispatch_workgroups(lines.div_ceil(64), 1, 1);
        }
        out
    }

    /// `img` as a texture; None when the device cannot hold it.
    fn upload(&mut self, img: &ColorImage) -> Option<Tex> {
        let [w, h] = img.size.map(|v| v as u32);
        if w == 0 || h == 0 || w.max(h) > self.g.max {
            return None;
        }
        // `write_texture` stages the picture once more (wgpu's own belt, its memory reused): a mapped buffer of the
        // frame's own was twice as slow (an allocation and a mapping per picture).
        let _step = lock(&self.g.record);
        let t = self.g.texture(w, h, true);
        let layout = wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: None };
        self.g.queue.write_texture(t.tex.as_image_copy(), bytemuck::cast_slice(&img.pixels), layout, t.tex.size());
        Some(t)
    }

    /// Run the recorded passes and read `t` back.
    pub(super) fn read(&mut self, t: &Tex) -> Option<ColorImage> {
        let g = self.g;
        g.readbacks.fetch_add(1, Relaxed);
        let row = (t.w * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buf = g.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: row as u64 * t.h as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let layout = wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None };
        self.enc.copy_texture_to_buffer(t.tex.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &buf, layout }, t.tex.size());
        let enc = std::mem::replace(&mut self.enc, g.device.create_command_encoder(&Default::default()));
        g.queue.submit([enc.finish()]);
        let (tx, rx) = mpsc::channel();
        buf.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        g.device.poll(wgpu::Maintain::Wait);
        rx.recv().ok()?.ok()?;
        if g.broken() {
            return None;
        }
        let data = buf.slice(..).get_mapped_range();
        let (w, row) = (t.w as usize * 4, row as usize);
        let mut pixels = Vec::with_capacity(t.w as usize * t.h as usize);
        for y in 0..t.h as usize {
            pixels.extend_from_slice(bytemuck::cast_slice::<u8, Color32>(&data[y * row..y * row + w]));
        }
        Some(ColorImage { size: [t.w as usize, t.h as usize], pixels })
    }

    /// `f` on the CPU on `t`'s picture, uploaded again.
    fn on_cpu(&mut self, t: Tex, f: impl FnOnce(&mut ColorImage)) -> Option<Tex> {
        let mut img = self.read(&t)?;
        self.give(t);
        f(&mut img);
        self.upload(&img)
    }

    pub(super) fn done(self) {
        self.g.give_back(self.spare.into_iter().chain(self.written));
    }
}

/// The passes' uniform block (`P` in composite.wgsl).
fn params(rect: [u32; 4], uv: [u32; 4], off: [u32; 4], k: [u32; 4], f: [f32; 4]) -> [u32; 20] {
    let mut p = [0; 20];
    for (i, v) in rect.into_iter().chain(uv).chain(off).chain(k).chain(f.map(f32::to_bits)).enumerate() {
        p[i] = v;
    }
    p
}

/// What Preferences › Video chose and found.
struct State {
    choice: Option<String>,
    /// Counts choices, so a slow detection of an older one does not land.
    round: u64,
    /// The Vulkan adapters' names (None while they are being listed).
    adapters: Option<Vec<String>>,
    /// The compositor on the UI's own device (`attach`), when the UI paints through wgpu on Vulkan.
    shared: Option<Arc<GpuCompositor>>,
    /// The UI's painter, for `Output::register` (feature `wgpu`).
    #[cfg_attr(not(feature = "wgpu"), allow(dead_code))]
    ui: Option<Ui>,
    gpu: Option<Arc<GpuCompositor>>,
    why: String,
}

static STATE: Mutex<State> = Mutex::new(State { choice: None, round: 0, adapters: None, shared: None, ui: None, gpu: None, why: String::new() });

/// Automatic: a discrete GPU, else an integrated one, else a virtual one; never a software rasteriser, which is
/// slower than the CPU compositor.
fn automatic<'a>(adapters: impl Iterator<Item = &'a wgpu::Adapter> + Clone) -> Option<&'a wgpu::Adapter> {
    use wgpu::DeviceType::*;
    [DiscreteGpu, IntegratedGpu, VirtualGpu].into_iter().find_map(|t| adapters.clone().find(|a| a.get_info().device_type == t))
}

#[cfg_attr(not(feature = "wgpu"), allow(dead_code))]
fn vulkan(a: &&wgpu::Adapter) -> bool {
    a.get_info().backend == wgpu::Backend::Vulkan
}

fn names<'a>(adapters: impl Iterator<Item = &'a wgpu::Adapter>) -> Vec<String> {
    let mut names: Vec<String> = adapters.map(|a| a.get_info().name).collect();
    names.dedup();
    names
}

/// eframe's wgpu painter set up for Milan Pro: Vulkan (OpenGL only where there is none), on the adapter Preferences ›
/// Video chose (`choice`, as `choose` reads it) when it exists, else the automatic one; the compositor's limits.
#[cfg(feature = "wgpu")]
pub fn wgpu_options(choice: &str) -> eframe::egui_wgpu::WgpuConfiguration {
    use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};
    let choice = choice.to_string();
    let select = move |adapters: &[wgpu::Adapter], surface: Option<&wgpu::Surface<'_>>| -> Result<wgpu::Adapter, String> {
        let fits = |a: &&wgpu::Adapter| surface.is_none_or(|s| !s.get_capabilities(a).formats.is_empty());
        let usable = adapters.iter().filter(fits);
        let named = usable.clone().filter(vulkan).find(|a| a.get_info().name == choice);
        named
            .or_else(|| automatic(usable.clone().filter(vulkan)))
            .or_else(|| automatic(usable.clone()))
            .or_else(|| usable.clone().next())
            .cloned()
            .ok_or_else(|| "No graphics adapter can show the window".to_string())
    };
    WgpuConfiguration {
        wgpu_setup: WgpuSetup::CreateNew(WgpuSetupCreateNew {
            instance_descriptor: wgpu::InstanceDescriptor { backends: wgpu::Backends::VULKAN | wgpu::Backends::GL, ..Default::default() },
            native_adapter_selector: Some(Arc::new(select)),
            device_descriptor: Arc::new(|adapter| {
                if adapter.get_info().backend == wgpu::Backend::Vulkan {
                    device_descriptor(adapter)
                } else {
                    wgpu::DeviceDescriptor { label: Some("milan"), required_features: wgpu::Features::empty(), required_limits: wgpu::Limits { max_texture_dimension_2d: 8192, ..wgpu::Limits::downlevel_webgl2_defaults() }, memory_hints: Default::default() }
                }
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// The UI paints through wgpu: its adapters are the list Preferences › Video offers, and when its device is a Vulkan
/// one the compositor runs on it (a frame can then stay on the GPU for the preview, R21c). Before `choose`.
#[cfg(feature = "wgpu")]
pub fn attach(rs: &eframe::egui_wgpu::RenderState) {
    let info = rs.adapter.get_info();
    let mut s = lock(&STATE);
    s.adapters = Some(names(rs.available_adapters.iter().filter(vulkan)));
    if info.backend == wgpu::Backend::Vulkan {
        match GpuCompositor::on(info.name.clone(), rs.device.clone(), rs.queue.clone(), true) {
            Ok(g) => {
                s.shared = Some(g);
                s.ui = Some(Ui { renderer: rs.renderer.clone() });
            }
            Err(e) => s.why = format!("{e}: compositing on the CPU."),
        }
    } else {
        s.why = format!("The window is drawn through {:?} on {}, which has no Vulkan: compositing on the CPU.", info.backend, info.name);
    }
}

/// GPU acceleration as Preferences › Video chooses it: "" the best adapter, `OFF`, or an adapter's name. The UI's own
/// device serves when it is that adapter (`attach`); another adapter is opened on its own device on a background
/// thread (a missing or old driver only means the CPU composes). Until then, and whenever there is no GPU, `current`
/// is None.
pub fn choose(choice: &str) {
    let round = {
        let mut s = lock(&STATE);
        if s.choice.as_deref() == Some(choice) && !s.gpu.as_ref().is_some_and(|g| g.broken()) {
            return;
        }
        s.choice = Some(choice.to_string());
        s.round += 1;
        if choice == OFF {
            (s.gpu, s.why) = (None, "GPU acceleration is off: compositing on the CPU.".into());
            return;
        }
        if let Some(g) = s.shared.clone().filter(|g| !g.broken() && (choice.is_empty() || g.name == choice)) {
            (s.gpu, s.why) = (Some(g), String::new());
            return;
        }
        if s.shared.is_none() && s.adapters.is_some() && choice.is_empty() && s.why.contains("no Vulkan") {
            s.gpu = None; // the window's adapter has no Vulkan: nothing else to look for
            return;
        }
        (s.gpu, s.why) = (None, "Looking for graphics adapters...".into());
        s.round
    };
    let choice = choice.to_string();
    let _ = std::thread::Builder::new().name("milan-gpu".into()).spawn(move || {
        let (names, gpu) = std::panic::catch_unwind(|| detect(&choice)).unwrap_or_else(|_| (Vec::new(), Err("The graphics driver failed".into())));
        let mut s = lock(&STATE);
        if s.round == round {
            if s.adapters.is_none() {
                s.adapters = Some(names);
            }
            (s.gpu, s.why) = match gpu {
                Ok(g) => (Some(g), String::new()),
                Err(e) => (None, format!("{e}: compositing on the CPU.")),
            };
        }
    });
}

/// (the Vulkan adapters' names, a device of its own for `choice`).
fn detect(choice: &str) -> (Vec<String>, Result<Arc<GpuCompositor>, String>) {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor { backends: wgpu::Backends::VULKAN, ..Default::default() });
    let adapters = instance.enumerate_adapters(wgpu::Backends::VULKAN);
    let names = names(adapters.iter());
    let picked = match choice {
        OFF => return (names, Err("GPU acceleration is off".into())),
        "" => automatic(adapters.iter()),
        name => adapters.iter().find(|a| a.get_info().name == name),
    };
    let gpu = match picked {
        Some(a) => GpuCompositor::open(a),
        None if choice.is_empty() => Err("No Vulkan graphics adapter was found".into()),
        None => Err(format!("{choice} was not found")),
    };
    (names, gpu)
}

/// The GPU compositor to use now, if any.
pub fn current() -> Option<Arc<GpuCompositor>> {
    #[cfg(test)]
    {
        if let Some(forced) = lock(&FORCED).clone() {
            return forced;
        }
        if std::env::var_os("MILAN_TEST_GPU").is_some() {
            return test_gpu();
        }
    }
    lock(&STATE).gpu.clone().filter(|g| !g.broken())
}

/// The compositor frames go through now: the GPU when Preferences › Video chose one and it works, else the CPU.
pub fn compositor() -> Arc<dyn Compositor> {
    match current() {
        Some(g) => g,
        None => Arc::new(composite::CpuCompositor),
    }
}

/// R21c: frames the current GPU compositor has read back so far (None: the CPU composes).
pub fn readback_count() -> Option<u64> {
    current().map(|g| g.readbacks())
}

/// The adapters found (None while they are being listed).
pub fn adapters() -> Option<Vec<String>> {
    lock(&STATE).adapters.clone()
}

/// What compositing runs on, for Preferences › Video.
pub fn status() -> String {
    let s = lock(&STATE);
    match &s.gpu {
        Some(g) if g.broken() => format!("{} stopped working: compositing on the CPU until Milan Pro restarts.", g.name),
        Some(g) if g.shared => format!("Compositing on {}, the display's own device.", g.name()),
        Some(g) => format!("Compositing on {}, read back for the display.", g.name()),
        None => s.why.clone(),
    }
}

/// Tests: the automatic adapter on a device of its own, opened once; None (the GPU tests skip) when there is none.
#[cfg(test)]
pub fn test_gpu() -> Option<Arc<GpuCompositor>> {
    static G: std::sync::OnceLock<Option<Arc<GpuCompositor>>> = std::sync::OnceLock::new();
    G.get_or_init(|| detect("").1.ok()).clone()
}

/// Tests: compose on `g` (None: the CPU) whatever the preference or `MILAN_TEST_GPU` says, until `unforce`.
#[cfg(test)]
static FORCED: Mutex<Option<Option<Arc<GpuCompositor>>>> = Mutex::new(None);

#[cfg(test)]
pub fn force_for_tests(g: Option<Arc<GpuCompositor>>) {
    *lock(&FORCED) = Some(g);
}

#[cfg(test)]
pub fn unforce() {
    *lock(&FORCED) = None;
}

/// Benches: the CPU, then the GPU when there is one (its name with `_` for spaces, for `tools/bench.ps1`).
#[cfg(test)]
pub fn compositors() -> Vec<(&'static str, Arc<dyn Compositor>)> {
    let g = test_gpu();
    match &g {
        Some(g) => println!("BENCH adapter adapter={}", g.name.replace(' ', "_")),
        None => println!("SKIPPED the GPU runs: no Vulkan graphics adapter"),
    }
    let cpu: Arc<dyn Compositor> = Arc::new(composite::CpuCompositor);
    std::iter::once(("cpu", cpu)).chain(g.map(|g| ("gpu", g as Arc<dyn Compositor>))).collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::engine::composite::tests::{event, layer, media};
    use crate::engine::plan::{self, MediaRef, SrcAt};
    use crate::engine::plugins::{find, preset_eval};
    use crate::project::{Halo, Motion, PanCrop};
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    /// The GPU, or None with a clear message (run with --nocapture to see it): the test then checks nothing.
    pub(crate) fn gpu_or_skip(test: &str) -> Option<Arc<GpuCompositor>> {
        let g = test_gpu();
        if g.is_none() {
            println!("SKIPPED {test}: no Vulkan graphics adapter, so GPU-CPU parity was not checked");
        }
        g
    }

    pub(crate) fn hash(mut x: u32) -> u32 {
        x ^= x >> 16;
        x = x.wrapping_mul(0x7feb_352d);
        x ^= x >> 15;
        x = x.wrapping_mul(0x846c_a68b);
        x ^ (x >> 16)
    }

    pub(crate) struct Rng(pub u32);
    impl Rng {
        pub fn f(&mut self) -> f32 {
            self.0 = hash(self.0.wrapping_add(0x9e37_79b9));
            (self.0 >> 8) as f32 / (1 << 24) as f32
        }
        pub fn pick<T: Copy>(&mut self, v: &[T]) -> T {
            v[(self.f() * v.len() as f32) as usize % v.len()]
        }
    }

    /// Pictures by media id at any size, made once: gradients with noise (so sampling has to be exact); ids starting
    /// with "a" have alpha (clear, half, opaque and random pixels), premultiplied.
    #[derive(Default)]
    pub(crate) struct Noise(HashMap<(Arc<str>, u32, u32), Arc<ColorImage>>);
    impl Sources for Noise {
        fn get(&mut self, s: &SrcAt, w: u32, h: u32) -> Option<Arc<ColorImage>> {
            // Generated media render as the worker renders them (`Live::fetch`).
            if let Some((e, t)) = &s.generator {
                return Some(Arc::new(crate::engine::generate::render(e, w, h, *t)));
            }
            let id = s.m.id.clone();
            let img = self.0.entry((id.clone(), w, h)).or_insert_with(|| {
                let seed = hash(id.bytes().fold(7, |a, b| a.wrapping_mul(31) ^ b as u32));
                let pixels = (0..w * h)
                    .map(|i| {
                        let (x, y, n) = (i % w, i / w, hash(seed ^ i));
                        let a = if id.starts_with('a') { [0, 128, 255, n >> 24][(n >> 4 & 3) as usize] } else { 255 };
                        let c = |v: u32| composite::mul255(v % 256, a) as u8;
                        Color32::from_rgba_premultiplied(c(x * 255 / w + (n & 31)), c(y * 255 / h + (n >> 8 & 31)), c(n >> 16), a as u8)
                    })
                    .collect();
                Arc::new(ColorImage { size: [w as usize, h as usize], pixels })
            });
            Some(img.clone())
        }
    }

    /// (largest per-channel difference, channels that differ).
    pub(crate) fn diff(a: &ColorImage, b: &ColorImage) -> (u8, usize) {
        assert_eq!(a.size, b.size);
        a.pixels.iter().zip(&b.pixels).flat_map(|(x, y)| x.to_array().into_iter().zip(y.to_array())).fold((0, 0), |(m, n), (x, y)| (m.max(x.abs_diff(y)), n + (x != y) as usize))
    }

    /// `fp` on both, at most `within` in 255 apart.
    pub(crate) fn check_within(g: &GpuCompositor, fp: &FramePlan, [w, h]: [usize; 2], bilinear: bool, within: u8, what: &str) -> (u8, usize) {
        let mut src = Noise::default();
        let cpu = composite::compose(fp, &mut src, w, h, bilinear, 3);
        let gpu = g.compose(fp, &mut src, w, h, bilinear, 3).unwrap_or_else(|| panic!("{what}: the GPU did not compose it"));
        let d = diff(&cpu, &gpu);
        if d.0 > within {
            // The pair, for a look: %TEMP%\milan-agent\r21\fail\<what>-{cpu,gpu}.png.
            let dir = std::env::temp_dir().join("milan-agent").join("r21").join("fail");
            std::fs::create_dir_all(&dir).unwrap();
            let name = what.replace(|c: char| !c.is_ascii_alphanumeric(), "_");
            crate::demo::save_png(&dir.join(format!("{name}-cpu.png")), &cpu).unwrap();
            crate::demo::save_png(&dir.join(format!("{name}-gpu.png")), &gpu).unwrap();
        }
        assert!(d.0 <= within, "{what}: the GPU frame differs from the CPU's by up to {} ({} channels)\n{fp:?}", d.0, d.1);
        d
    }

    /// `fp` on both, at most 2 in 255 apart.
    fn check(g: &GpuCompositor, fp: &FramePlan, size: [usize; 2], bilinear: bool, what: &str) -> (u8, usize) {
        check_within(g, fp, size, bilinear, 2, what)
    }

    pub(crate) fn ev(r: &mut Rng, id: String) -> plan::EventPlan {
        let (mw, mh) = r.pick(&[(1920, 1080), (640, 480), (1280, 720), (480, 270)]);
        let m = Arc::new(MediaRef { id: id.into(), ..(*media(mw, mh)).clone() });
        let pc = if r.f() < 0.5 {
            PanCrop::default()
        } else {
            let flip = if r.f() < 0.2 { -1.0 } else { 1.0 };
            let any = 360.0 * r.f();
            PanCrop { cx: 0.3 + 0.4 * r.f(), cy: 0.3 + 0.4 * r.f(), w: flip * (0.4 + r.f()), h: 0.4 + r.f(), rot: r.pick(&[0.0, 90.0, any]) }
        };
        event(&m, pc, if r.f() < 0.7 { 1.0 } else { r.f() })
    }

    const MODES: [Composite; 22] = {
        use Composite::*;
        [SourceAlpha, Add, Subtract, Multiply, Screen, Overlay, Darken, Lighten, Difference, ThreeD, Cut, HardLight, Dodge, Burn, DifferenceSquared, SoftLight, Exclusion, HardMix, Hue, Saturation, Color, Luminosity]
    };

    /// 1-4 random layers; sometimes a compositing group with Parent Motion, a transparent background.
    fn random_frame(r: &mut Rng) -> FramePlan {
        let n = 1 + (r.f() * 4.0) as usize;
        let mut layers: Vec<TrackPlan> = (0..n)
            .map(|i| {
                let id = |r: &mut Rng, j: usize| format!("{}{i}{j}", if r.f() < 0.4 { "a" } else { "m" });
                let (a, b) = (id(r, 0), id(r, 1));
                let item = if r.f() < 0.2 {
                    let a = ev(r, a);
                    let b = (r.f() < 0.7).then(|| ev(r, b));
                    Item::Mix { a: Some(a), b, p: r.f(), kind: None }
                } else {
                    Item::Event(ev(r, a))
                };
                let any = 360.0 * r.f();
                let mo = if r.f() < 0.4 { Motion::default() } else { Motion { x: r.f() - 0.5, y: r.f() - 0.5, w: 0.2 + r.f(), h: 0.2 + r.f(), rot: r.pick(&[0.0, any]), ..Default::default() } };
                let mut l = layer(vec![item], r.pick(&MODES), if r.f() < 0.5 { 1.0 } else { r.f() }, mo);
                if r.f() < 0.15 {
                    l.fade = Some(([(r.f() * 255.0) as u8, 40, 200], r.f()));
                }
                l
            })
            .collect();
        if n >= 2 && r.f() < 0.3 {
            layers[..n - 1].iter_mut().for_each(|l| l.child = true);
            let pm = if r.f() < 0.5 { Affine::scale(1.0, 1.0) } else { plan::motion(Motion { w: 0.6, h: 0.7, x: 0.1, rot: 20.0, ..Default::default() }, (1920.0, 1080.0)) };
            layers[n - 1].parent = Some((r.pick(&MODES), pm));
        }
        FramePlan { alpha: r.f() < 0.2, fade: None, samples: Vec::new(), out_fx: Vec::new(), layers }
    }

    /// (plan, frame size, bilinear) number `n` of the random sequence the parity tests walk.
    fn plan_n(r: &mut Rng) -> (FramePlan, [usize; 2], bool) {
        let fp = random_frame(r);
        let size = r.pick(&[[480, 270], [320, 180], [257, 143]]);
        (fp, size, r.f() < 0.8)
    }

    /// R21a acceptance: 300 random plans of 1-4 layers in every blend mode (all but Custom), random opacity, Track Motion
    /// (scale, rotation, offset), Pan/Crop (flips, rotation), event alpha, sources with alpha, bilinear and nearest,
    /// dissolves, Fade to Color, compositing groups with Parent Motion, transparent backgrounds: the GPU's frame is
    /// within 2 in 255 of the CPU's.
    #[test]
    fn gpu_matches_cpu_on_random_plans() {
        let Some(g) = gpu_or_skip("gpu_matches_cpu_on_random_plans") else { return };
        let mut r = Rng(1);
        let (mut worst, mut exact) = (0, 0);
        for n in 0..300 {
            let (fp, size, bilinear) = plan_n(&mut r);
            let (d, _) = check(&g, &fp, size, bilinear, &format!("plan {n}"));
            (worst, exact) = (worst.max(d), exact + (d == 0) as usize);
        }
        println!("gpu_matches_cpu_on_random_plans on {}: largest difference {worst}, {exact} of 300 frames bit-exact", g.name);
    }

    /// The same over 3000 plans, with a histogram of the largest difference per frame (the 300 above are its start).
    /// `cargo test --release -- --ignored --nocapture gpu_parity_histogram`
    #[test]
    #[ignore]
    fn gpu_parity_histogram() {
        let Some(g) = gpu_or_skip("gpu_parity_histogram") else { return };
        let mut r = Rng(1);
        let mut hist = [0usize; 256];
        for n in 0..3000 {
            let (fp, size, bilinear) = plan_n(&mut r);
            let (d, _) = check(&g, &fp, size, bilinear, &format!("plan {n}"));
            hist[d as usize] += 1;
        }
        println!("gpu_parity_histogram on {}: {:?}", g.name, hist.iter().enumerate().filter(|x| *x.1 > 0).collect::<Vec<_>>());
    }

    /// What the GPU leaves to the CPU gives the CPU's frame too: track FX, 3D Track Motion, 2D Shadow and Glow, a
    /// transition with a kind, Adjustment Tracks and Events, a Custom compositor, Video Output FX and the bus's Fade
    /// to Color; a frame larger than the device's textures is left to the CPU.
    #[test]
    fn gpu_matches_cpu_with_its_cpu_pieces() {
        let Some(g) = gpu_or_skip("gpu_matches_cpu_with_its_cpu_pieces") else { return };
        let mut r = Rng(7);
        let fx = |id: &str| preset_eval(find(id).unwrap(), None);
        let pip = Motion { w: 0.6, h: 0.6, rot: 10.0, ..Default::default() };
        let one = |r: &mut Rng, mode: Composite| layer(vec![Item::Event(ev(r, "a1".into()))], mode, 0.8, pip);
        let mut cases: Vec<(&str, FramePlan)> = Vec::new();
        let mut frame = |name, r: &mut Rng, top: TrackPlan| {
            let bg = layer(vec![Item::Event(ev(r, "m0".into()))], Composite::SourceAlpha, 1.0, Motion::default());
            cases.push((name, FramePlan { alpha: false, fade: None, samples: Vec::new(), out_fx: Vec::new(), layers: vec![bg, top] }));
        };
        let mut l = one(&mut r, Composite::Screen);
        l.fx = vec![fx("milan.fx.cookie_cutter")];
        frame("track FX", &mut r, l);
        let mut l = one(&mut r, Composite::ThreeD);
        l.persp = plan::motion3d(Motion { ry: 40.0, z: 0.2, ..Default::default() }, (1920.0, 1080.0));
        frame("3D Track Motion", &mut r, l);
        let mut l = one(&mut r, Composite::SourceAlpha);
        (l.shadow, l.glow) = (Some(Halo::default()), Some(Halo::glow()));
        frame("2D Shadow and Glow", &mut r, l);
        let mix = Item::Mix { a: Some(ev(&mut r, "m2".into())), b: Some(ev(&mut r, "a3".into())), p: 0.4, kind: Some(fx("milan.tr.push")) };
        frame("push transition", &mut r, layer(vec![mix], Composite::SourceAlpha, 0.9, pip));
        let mut l = one(&mut r, Composite::Overlay);
        l.adjust = vec![(vec![fx("milan.fx.gaussian_blur")], 1.0), (vec![fx("milan.fx.invert")], 0.5)];
        frame("adjustment track with an event", &mut r, l);
        let mut l = layer(Vec::new(), Composite::SourceAlpha, 1.0, Motion::default());
        l.adjust = vec![(vec![fx("milan.fx.invert")], 0.3)];
        frame("adjustment track", &mut r, l);
        let mut l = one(&mut r, Composite::Custom);
        l.compositor = Some(fx("milan.comp.displacement"));
        frame("Custom compositor", &mut r, l);
        let l = one(&mut r, Composite::Multiply);
        frame("output FX and fade", &mut r, l);
        let last = cases.len() - 1;
        (cases[last].1.out_fx, cases[last].1.fade) = (vec![fx("milan.fx.invert")], Some(([255, 0, 0], 0.4)));
        for (name, fp) in &cases {
            assert_eq!(check(&g, fp, [320, 180], true, name), (0, 0), "{name} is made by the CPU code, exactly");
        }
        let big = g.max as usize + 1;
        assert!(g.compose(&cases[0].1, &mut Noise::default(), big, 2, true, 1).is_none(), "too large for the device: the CPU composes it");
    }

    /// The float emulation `composite.wgsl` builds the float blend modes on: its division and square root give the
    /// correctly rounded f32 result (the GPU's own are approximate), and a product added to a sum is not fused.
    #[test]
    fn gpu_float_ops_round_as_the_cpu() {
        let Some(g) = gpu_or_skip("gpu_float_ops_round_as_the_cpu") else { return };
        let src = format!("{}\n{}\n{}", include_str!("math.wgsl"), include_str!("composite.wgsl"), r#"
fn h(x0: u32) -> u32 { var x = x0; x ^= x >> 16u; x *= 0x7feb352du; x ^= x >> 15u; x *= 0x846ca68bu; return x ^ (x >> 16u); }
@fragment fn fs_probe(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = vec2<u32>(pos.xy);
    let i = q.y * 256u + q.x;
    let a = f32(h(i) & 0xffffffu) * (1.0 / 16777216.0);
    let b = f32(h(i ^ 0x5bd1e995u) & 0xffffffu) * (1.0 / 16777216.0) + 0.001;
    var r: f32;
    switch p.k.y { case 0u: { r = div(a, b); } case 1u: { r = root(a); } case 2u: { r = add(mul(a, b), a); } default: { r = a / b; } }
    let u = bitcast<u32>(r);
    return vec4<u32>(u & 255u, (u >> 8u) & 255u, (u >> 16u) & 255u, u >> 24u);
}"#);
        let shader = g.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(src.into()) });
        let pl = g.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&g.layout], push_constant_ranges: &[] });
        let pipe = g.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_probe"), targets: &[Some(wgpu::ColorTargetState { format: FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let mut native_div_differs = 0;
        for op in 0..4u32 {
            let enc = g.device.create_command_encoder(&Default::default());
            let mut f = Frame { g: &g, enc, w: 256, h: 256, spare: Vec::new(), written: Vec::new(), bilinear: true, k: 1 };
            let t = f.clear(256, 256, Color32::TRANSPARENT);
            let out = f.pass(&pipe, &t, None, &params([0; 4], [0; 4], [0; 4], [0, op, 0, 0], [0.0; 4]), &[]);
            let img = f.read(&out).unwrap();
            f.done();
            let mut bad = 0;
            for i in 0..65536u32 {
                let a = (hash(i) & 0xffffff) as f32 * (1.0 / 16777216.0);
                let b = (hash(i ^ 0x5bd1e995) & 0xffffff) as f32 * (1.0 / 16777216.0) + 0.001;
                let want = match op { 0 | 3 => a / b, 1 => a.sqrt(), _ => a * b + a };
                bad += (f32::from_bits(u32::from_le_bytes(img.pixels[i as usize].to_array())) != want) as usize;
            }
            match op {
                3 => native_div_differs = bad,
                _ => assert_eq!(bad, 0, "op {op} ({}): {bad} of 65536 results are not the CPU's", ["div", "sqrt", "mul then add"][op as usize]),
            }
        }
        println!("gpu_float_ops_round_as_the_cpu on {}: the GPU's own division differs from the CPU's in {native_div_differs} of 65536 cases; the emulation in none", g.name);
    }

    /// The tests that set the process-wide choice take turns.
    static CHOICE: Mutex<()> = Mutex::new(());

    /// R21c acceptance: a preview frame the GPU composed stays there (no readback: the counter does not move), and
    /// its pixels, asked for as a render asks (`Picture::image`), are read back once and are the CPU's; the worker
    /// hands the preview that picture, and a render's frames come through the same worker.
    #[test]
    fn preview_frames_stay_on_the_gpu_and_renders_read_back() {
        use crate::engine::worker::Worker;
        use crate::project::{Clip, MediaItem, MediaType, Project};
        if gpu_or_skip("preview_frames_stay_on_the_gpu_and_renders_read_back").is_none() {
            return;
        }
        // Its own device, so its counter is this test's alone.
        let Some(g) = detect("").1.ok() else { return };
        let _turn = lock(&CHOICE);
        force_for_tests(Some(g.clone()));
        let mut p = Project::new("gpu");
        (p.settings.width, p.settings.height, p.settings.frame_rate) = (320, 180, 30.0);
        let info = find("milan.gen.gradient").unwrap();
        let m = MediaItem { id: "g".into(), name: "g".into(), media_type: MediaType::Image, duration: 60.0, generator: Some(crate::engine::plugins::new_fx(info, info.presets.get(2))), ..Default::default() };
        let mut c = Clip::from_media(&m, 0.0);
        (c.duration, c.out_point) = (10.0, 10.0);
        p.timeline.video_tracks[0].clips.push(c);
        p.media_pool.push(m);
        let mut wk = Worker::new();
        wk.set_plan(Arc::new(plan::compile(&p)));
        let before = g.readbacks();
        let (pic, err) = wk.frame(30, 320, 180, crate::decode::Quality::Preview, None);
        assert!(err.is_none(), "{err:?}");
        assert!(pic.gpu().is_some(), "the frame is on the GPU");
        assert_eq!(pic.gpu().unwrap().size(), [320, 180]);
        assert_eq!(g.readbacks(), before, "a preview frame is not read back");
        let img = pic.image();
        assert_eq!(g.readbacks(), before + 1, "a render reads the frame back");
        let _ = pic.image();
        assert_eq!(g.readbacks(), before + 1, "once");
        force_for_tests(None);
        let mut cpu = Worker::new();
        cpu.set_plan(Arc::new(plan::compile(&p)));
        let (want, _) = cpu.frame(30, 320, 180, crate::decode::Quality::Preview, None);
        assert!(want.gpu().is_none() && img.pixels == want.pixels, "the GPU frame read back is the CPU frame");
        // The frame through `Picture`'s pixels (the scopes, a snapshot) is the same readback.
        assert_eq!(pic.size, [320, 180]);
        unforce();
    }

    /// R21c: the `Rgba16Float` intermediate (R40b's): a frame held in linear float reads back within 1 in 255 of the
    /// 8-bit frame (sRGB in and out of 16-bit float), and the preview sees a float texture.
    #[test]
    fn float_intermediate_reads_back_within_one() {
        if gpu_or_skip("float_intermediate_reads_back_within_one").is_none() {
            return;
        }
        // Its own device: the shared test compositor stays 8-bit for the other tests.
        let Some(g) = detect("").1.ok() else { return };
        g.set_intermediate(Intermediate::Rgba16Float);
        assert_eq!(g.intermediate(), Intermediate::Rgba16Float);
        let mut r = Rng(21);
        let (fp, size, bilinear) = plan_n(&mut r);
        let mut src = Noise::default();
        let cpu = composite::compose(&fp, &mut src, size[0], size[1], bilinear, 3);
        let pic = g.compose(&fp, &mut src, size[0], size[1], bilinear, 3).expect("composed");
        assert_eq!(pic.gpu().unwrap().format(), Intermediate::Rgba16Float);
        let d = diff(&cpu, &pic.image());
        assert!(d.0 <= 1, "a float frame reads back within 1: {d:?}");
        g.set_intermediate(Intermediate::Rgba8);
        let pic = g.compose(&fp, &mut src, size[0], size[1], bilinear, 3).expect("composed");
        assert_eq!(diff(&cpu, &pic.image()), (0, 0), "and 8-bit is exact again");
    }

    /// Off lists the adapters but opens none, so the CPU composes.
    #[test]
    fn off_lists_the_adapters_and_composes_on_the_cpu() {
        if std::env::var_os("MILAN_TEST_GPU").is_some() {
            return; // the whole run is forced onto the GPU
        }
        let _turn = lock(&CHOICE);
        choose("probe"); // an adapter that does not exist: the list is made on the detection thread
        let t = Instant::now();
        while adapters().is_none() && t.elapsed() < Duration::from_secs(30) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(adapters().is_some(), "the adapters are listed");
        choose(OFF);
        assert!(current().is_none() && status().contains("off"), "{}", status());
        assert_eq!(compositor().name(), "CPU");
    }

    /// R21a: four 1920x1080 layers composed on the CPU and on the GPU from decoded pictures (upload and readback
    /// included): full frames at Level 50 % (the playback benches' project), then picture-in-pictures turned 15°;
    /// R21c: the same at 3840x2160 (a 4K project at Best (Full)).
    /// `cargo test --release -- --ignored --nocapture bench_compose`
    #[test]
    #[ignore]
    fn bench_compose() {
        let mo = |i: usize, pip: bool| if pip { Motion { w: 0.45, h: 0.45, x: (i % 2) as f32 * 0.5 - 0.25, y: (i / 2) as f32 * 0.5 - 0.25, rot: 15.0, ..Default::default() } } else { Motion::default() };
        let fp = |pip: bool, (mw, mh): (u32, u32)| FramePlan {
            alpha: false,
            fade: None,
            samples: Vec::new(),
            out_fx: Vec::new(),
            layers: (0..4).map(|i| layer(vec![Item::Event(event(&Arc::new(MediaRef { id: format!("m{i}").into(), ..(*media(mw, mh)).clone() }), PanCrop::default(), 1.0))], Composite::SourceAlpha, 0.5, mo(i, pip))).collect(),
        };
        let ms = |f: &mut dyn FnMut()| {
            f(); // warm: pictures made, textures pooled
            let mut t: Vec<f64> = (0..30)
                .map(|_| {
                    let at = Instant::now();
                    f();
                    at.elapsed().as_secs_f64() * 1000.0
                })
                .collect();
            t.sort_by(f64::total_cmp);
            t[15]
        };
        for (kind, pip, size) in [("full", false, (1920, 1080)), ("pip", true, (1920, 1080)), ("4k", false, (3840, 2160))] {
            let (fp, mut src) = (fp(pip, size), Noise::default());
            for (name, c) in compositors() {
                let k = composite::stripes();
                // As the preview gets it (R21c: a GPU frame stays on the GPU), and with its pixels read back (a render).
                let t = ms(&mut || {
                    let pic = c.compose(&fp, &mut src, size.0 as usize, size.1 as usize, true, k).expect("composed");
                    std::hint::black_box(pic);
                });
                let tr = ms(&mut || {
                    let pic = c.compose(&fp, &mut src, size.0 as usize, size.1 as usize, true, k).expect("composed");
                    std::hint::black_box(pic.image());
                });
                println!("BENCH compose kind={kind} compositor={name} layers=4 ms={t:.2} ms_readback={tr:.2}");
            }
        }
    }
}
