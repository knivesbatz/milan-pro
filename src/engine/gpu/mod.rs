//! R21a: the GPU compositor, on its own wgpu device (Vulkan) beside the UI's glow painter. `Gpu::compose` gives the
//! frame `composite::compose` gives, within 2 in 255 per channel: draws (affine, bilinear or nearest), opacity, Source
//! Alpha and the integer blend modes are bit-exact (the passes in `composite.wgsl` use the CPU's integer maths), the
//! float blend modes (any mode over a transparent backdrop, Hard Light to Luminosity, Cut, Multiply (Mask)) round
//! within 2. The CPU compositor stays the reference, and does what the GPU does not yet:
//! - per piece, made by the CPU code and uploaded: decoding, resampling, event FX, masks, nested timelines,
//!   transitions with a kind (`composite::transition_image`), and a whole track with track FX, 3D Track Motion or
//!   2D Shadow / Glow (`composite::layer_image`);
//! - per layer, read back and done on the CPU: Adjustment Tracks / Events, Custom compositors and Hard Mix (a step
//!   whose threshold the GPU's division may cross differently); after the frame: the Video Output FX and the bus's
//!   Fade to Color (`composite::finish`), and motion blur's mean (`Worker::frame`);
//! - per frame: a frame or picture larger than the adapter's textures, or any GPU error;
//! - always: no Vulkan adapter, Preferences › Video › GPU acceleration Off, or a lost device.
//!
//! Renders use the same compositor as the preview (`Worker::frame`), so a render shows what was previewed.
use eframe::egui::{Color32, ColorImage};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use wgpu::util::DeviceExt;
use crate::engine::composite::{self, Affine, Sources};
use crate::engine::plan::{FramePlan, Item, TrackPlan};
use crate::project::Composite;

/// Preferences › Video › GPU acceleration: this value turns it off ("" is automatic, anything else an adapter's name).
pub const OFF: &str = "Off";

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Uint;
/// Textures kept between frames for reuse, at most this many bytes.
const POOL_BYTES: usize = 256 << 20;

struct Tex {
    tex: wgpu::Texture,
    view: wgpu::TextureView,
    w: u32,
    h: u32,
    /// Written from the CPU (`queue.write_texture`): such a write lands before the frame's passes run, so this
    /// texture is reused only after the frame.
    cpu: bool,
}

pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pub name: String,
    layout: wgpu::BindGroupLayout,
    draw: wgpu::RenderPipeline,
    blend: wgpu::RenderPipeline,
    fade: wgpu::RenderPipeline,
    /// Bound where a pass reads no source / no rows.
    none: Tex,
    no_rows: wgpu::Buffer,
    max: u32,
    /// A validation error, a panic inside wgpu or a lost device: the CPU composes from then on.
    broken: Arc<AtomicBool>,
    pool: Mutex<Vec<Tex>>,
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

impl Gpu {
    fn open(adapter: &wgpu::Adapter) -> Result<Gpu, String> {
        let name = adapter.get_info().name;
        let desc = wgpu::DeviceDescriptor { label: Some("milan-compositor"), required_features: wgpu::Features::empty(), required_limits: adapter.limits(), memory_hints: wgpu::MemoryHints::MemoryUsage };
        let (device, queue) = block_on(adapter.request_device(&desc, None)).map_err(|e| format!("{name} could not be opened ({e})"))?;
        let broken = Arc::new(AtomicBool::new(false));
        let b = broken.clone();
        device.on_uncaptured_error(Box::new(move |_| b.store(true, Relaxed)));
        let b = broken.clone();
        device.set_device_lost_callback(move |_, _| b.store(true, Relaxed));
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("composite.wgsl"), source: wgpu::ShaderSource::Wgsl(include_str!("composite.wgsl").into()) });
        let stage = wgpu::ShaderStages::FRAGMENT;
        let tex = |binding| wgpu::BindGroupLayoutEntry { binding, visibility: stage, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Uint, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None };
        let buf = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: stage, ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None }, count: None };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[tex(0), tex(1), buf(2, wgpu::BufferBindingType::Uniform), buf(3, wgpu::BufferBindingType::Storage { read_only: true })],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&layout], push_constant_ranges: &[] });
        let pipe = |fs: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fs),
                layout: Some(&pl),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    targets: &[Some(wgpu::ColorTargetState { format: FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        let (draw, blend, fade) = (pipe("fs_draw"), pipe("fs_blend"), pipe("fs_fade"));
        let none = new_tex(&device, 1, 1, false);
        let no_rows = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 8, usage: wgpu::BufferUsages::STORAGE, mapped_at_creation: false });
        let max = device.limits().max_texture_dimension_2d;
        device.poll(wgpu::Maintain::Wait);
        if broken.load(Relaxed) {
            return Err(format!("{name} could not build the compositor's shaders"));
        }
        Ok(Gpu { device, queue, name, layout, draw, blend, fade, none, no_rows, max, broken, pool: Mutex::new(Vec::new()) })
    }

    pub fn broken(&self) -> bool {
        self.broken.load(Relaxed)
    }

    /// `composite::compose` on the GPU; None when this frame cannot be done here (the caller composes it on the CPU).
    pub fn compose(&self, fp: &FramePlan, src: &mut dyn Sources, w: usize, h: usize, bilinear: bool, k: usize) -> Option<ColorImage> {
        if self.broken() || w == 0 || h == 0 || w.max(h) > self.max as usize {
            return None;
        }
        let run = || {
            let enc = self.device.create_command_encoder(&Default::default());
            let mut f = Frame { g: self, enc, w: w as u32, h: h as u32, spare: Vec::new(), written: Vec::new(), bilinear, k };
            let img = f.frame(fp, src);
            f.done();
            img
        };
        // A panic inside wgpu (a driver gone bad) must not take the editor down: the CPU takes over.
        let img = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).unwrap_or_else(|_| {
            self.broken.store(true, Relaxed);
            None
        });
        let mut img = img.filter(|_| !self.broken())?;
        composite::finish(&mut img, fp, k);
        Some(img)
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

fn new_tex(device: &wgpu::Device, w: u32, h: u32, cpu: bool) -> Tex {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = tex.create_view(&Default::default());
    Tex { tex, view, w, h, cpu }
}

/// One frame being composed: its passes are recorded into `enc`, submitted at a readback.
struct Frame<'a> {
    g: &'a Gpu,
    enc: wgpu::CommandEncoder,
    w: u32,
    h: u32,
    /// Frame-sized targets free for later passes (the GPU runs a frame's passes in order).
    spare: Vec<Tex>,
    /// Textures written from the CPU, back to the pool after the frame.
    written: Vec<Tex>,
    bilinear: bool,
    k: usize,
}

impl Frame<'_> {
    fn frame(&mut self, fp: &FramePlan, src: &mut dyn Sources) -> Option<ColorImage> {
        let mut acc = self.clear(if fp.alpha { Color32::TRANSPARENT } else { Color32::BLACK });
        let mut group: Option<Tex> = None;
        // As `composite::compose`: a compositing parent's children first, then the parent, then the group through
        // Parent Motion with the Parent Composite Mode.
        for l in &fp.layers {
            if l.child {
                let g = match group.take() {
                    Some(g) => g,
                    None => self.clear(Color32::TRANSPARENT),
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
                None => self.clear(Color32::TRANSPARENT),
            };
            if !l.items.is_empty() || !l.adjust.is_empty() {
                g = self.layer_onto(g, src, l, true)?;
            }
            if composite::whole_pixel(motion) != Some((0, 0)) {
                let (w, h) = (self.w as f64, self.h as f64);
                let blank = self.clear(Color32::TRANSPARENT);
                let moved = self.draw(blank, &g, Affine::scale(1.0 / w, 1.0 / h).then(motion).then(Affine::scale(w, h)), 1.0);
                self.give(g);
                g = moved;
            }
            acc = self.blend(acc, g, mode, 1.0, false)?;
        }
        if let Some(g) = group {
            acc = self.blend(acc, g, Composite::SourceAlpha, 1.0, false)?;
        }
        let img = self.read(&acc);
        self.give(acc);
        img
    }

    /// `composite::layer_onto`.
    fn layer_onto(&mut self, mut dst: Tex, src: &mut dyn Sources, l: &TrackPlan, group: bool) -> Option<Tex> {
        let k = self.k;
        for (fx, a) in &l.adjust {
            dst = self.on_cpu(dst, |img| composite::adjust(img, fx, *a, k))?;
        }
        if l.items.is_empty() {
            return Some(dst);
        }
        if !l.fx.is_empty() || l.persp.is_some() || l.shadow.is_some() || l.glow.is_some() || l.mode == Composite::Custom {
            // shortcut: track FX, 3D Track Motion, 2D Shadow / Glow and Custom compositors make the whole track on the
            // CPU (exact, but no faster than the CPU path); R21b moves them to the GPU.
            let layer = composite::layer_image(src, l, [self.w as usize, self.h as usize], self.bilinear, k);
            if l.mode == Composite::Custom {
                return self.on_cpu(dst, |d| composite::blend(d, &layer, l.mode, l.opacity, l.compositor.as_ref(), group, k));
            }
            let t = self.upload(&layer)?;
            return self.blend(dst, t, l.mode, l.opacity, group);
        }
        let draws: usize = l.items.iter().map(|i| composite::events(i).len()).sum();
        if l.fade.is_none() && matches!(l.mode, Composite::SourceAlpha | Composite::ThreeD) && (l.opacity >= 1.0 || draws <= 1) {
            return self.draw_items(dst, src, l, l.opacity);
        }
        let blank = self.clear(Color32::TRANSPARENT);
        let mut layer = self.draw_items(blank, src, l, 1.0)?;
        if let Some((c, t)) = l.fade {
            let p = params([0; 4], [0; 4], [0; 4], [c[0] as u32, c[1] as u32, c[2] as u32, 0], [0.0, t, 0.0, 0.0]);
            let pipe = &self.g.fade;
            layer = self.pass(pipe, layer, None, p, &[]);
        }
        self.blend(dst, layer, l.mode, l.opacity, group)
    }

    /// `composite::draw_items` with Track Motion (tracks without it are made on the CPU).
    fn draw_items(&mut self, mut dst: Tex, src: &mut dyn Sources, l: &TrackPlan, gain: f32) -> Option<Tex> {
        let (w, h) = (self.w as usize, self.h as usize);
        let to_out = composite::layer_to_out(l, w, h);
        for item in &l.items {
            match item {
                Item::Mix { a, b, p, kind: Some(kind) } => {
                    // shortcut: a transition's two sides are drawn and mixed on the CPU; R21b moves transitions.
                    let img = composite::transition_image(src, a.as_ref(), b.as_ref(), *p, kind, to_out, [w, h], self.bilinear, self.k);
                    let t = self.upload(&img)?;
                    dst = self.draw(dst, &t, Affine::scale(1.0 / w as f64, 1.0 / h as f64).then(to_out), gain);
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
                        let Some(img) = composite::event_image(src, e, to_out, self.bilinear, self.k) else { continue };
                        let t = self.upload(&img)?;
                        dst = self.draw(dst, &t, composite::event_to(&img, e, to_out), alpha);
                        self.give(t);
                    }
                }
            }
        }
        Some(dst)
    }

    /// `composite::draw`: `s` through `m` at `alpha` onto `dst`.
    fn draw(&mut self, dst: Tex, s: &Tex, m: Affine, alpha: f32) -> Tex {
        let a8 = (alpha.clamp(0.0, 1.0) * 255.0).round() as u32;
        let (size, out) = ([s.w as usize, s.h as usize], [self.w as usize, self.h as usize]);
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
                (params([x0, y0, x1, y1].map(|v| v as u32), uv, [0, 0, 0, self.bilinear as u32], [a8, 0, 0, 0], [0.0; 4]), rows)
            }
        };
        let pipe = &self.g.draw;
        self.pass(pipe, dst, Some(s), p, &rows)
    }

    /// `composite::blend` of `layer` onto `dst` (Custom and Hard Mix on the CPU).
    fn blend(&mut self, dst: Tex, layer: Tex, mode: Composite, opacity: f32, group: bool) -> Option<Tex> {
        if matches!(mode, Composite::Custom | Composite::HardMix) {
            let (l, k) = (self.read(&layer)?, self.k);
            self.give(layer);
            return self.on_cpu(dst, |d| composite::blend(d, &l, mode, opacity, None, group, k));
        }
        let o8 = (opacity.clamp(0.0, 1.0) * 255.0).round() as u32;
        let p = params([0; 4], [0; 4], [0; 4], [o8, mode as u32, group as u32, 0], [o8 as f32 / 255.0, 0.0, 0.0, 0.0]);
        let pipe = &self.g.blend;
        let out = self.pass(pipe, dst, Some(&layer), p, &[]);
        self.give(layer);
        Some(out)
    }

    /// A frame-sized render target.
    fn target(&mut self) -> Tex {
        self.spare.pop().unwrap_or_else(|| self.g.texture(self.w, self.h, false))
    }

    fn give(&mut self, t: Tex) {
        if t.cpu { self.written.push(t) } else { self.spare.push(t) }
    }

    /// A frame-sized target filled with `c`.
    fn clear(&mut self, c: Color32) -> Tex {
        let t = self.target();
        let [r, g, b, a] = c.to_array().map(f64::from);
        let ops = wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }), store: wgpu::StoreOp::Store };
        let at = wgpu::RenderPassColorAttachment { view: &t.view, resolve_target: None, ops };
        drop(self.enc.begin_render_pass(&wgpu::RenderPassDescriptor { color_attachments: &[Some(at)], ..Default::default() }));
        t
    }

    /// One pass of `pipe` into a new target, reading `dst` and `src`; `dst` goes back to the frame.
    fn pass(&mut self, pipe: &wgpu::RenderPipeline, dst: Tex, src: Option<&Tex>, p: [u32; 20], rows: &[[i32; 2]]) -> Tex {
        let g = self.g;
        let out = self.target();
        let uniform = g.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(&p), usage: wgpu::BufferUsages::UNIFORM });
        let rows = (!rows.is_empty()).then(|| g.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(rows), usage: wgpu::BufferUsages::STORAGE }));
        let bind = g.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &g.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&dst.view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&src.unwrap_or(&g.none).view) },
                wgpu::BindGroupEntry { binding: 2, resource: uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: rows.as_ref().unwrap_or(&g.no_rows).as_entire_binding() },
            ],
        });
        {
            let ops = wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store };
            let at = wgpu::RenderPassColorAttachment { view: &out.view, resolve_target: None, ops };
            let mut rp = self.enc.begin_render_pass(&wgpu::RenderPassDescriptor { color_attachments: &[Some(at)], ..Default::default() });
            rp.set_pipeline(pipe);
            rp.set_bind_group(0, &bind, &[]);
            rp.draw(0..3, 0..1);
        }
        self.give(dst);
        out
    }

    /// `img` as a texture; None when the adapter cannot hold it.
    fn upload(&mut self, img: &ColorImage) -> Option<Tex> {
        let [w, h] = img.size.map(|v| v as u32);
        if w == 0 || h == 0 || w.max(h) > self.g.max {
            return None;
        }
        let t = self.g.texture(w, h, true);
        let layout = wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: None };
        self.g.queue.write_texture(t.tex.as_image_copy(), bytemuck::cast_slice(&img.pixels), layout, t.tex.size());
        Some(t)
    }

    /// Run the recorded passes and read `t` back.
    fn read(&mut self, t: &Tex) -> Option<ColorImage> {
        let g = self.g;
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

    fn done(self) {
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
    adapters: Option<Vec<String>>,
    gpu: Option<Arc<Gpu>>,
    why: String,
}

static STATE: Mutex<State> = Mutex::new(State { choice: None, round: 0, adapters: None, gpu: None, why: String::new() });

/// GPU acceleration as Preferences › Video chooses it: "" the best adapter, `OFF`, or an adapter's name. The adapters
/// are listed and the device opened on a background thread (a missing or old driver only means the CPU composes);
/// until then, and whenever there is no GPU, `current` is None.
pub fn choose(choice: &str) {
    let round = {
        let mut s = lock(&STATE);
        if s.choice.as_deref() == Some(choice) && !s.gpu.as_ref().is_some_and(|g| g.broken()) {
            return;
        }
        (s.choice, s.gpu, s.why) = (Some(choice.to_string()), None, "Looking for graphics adapters...".into());
        s.round += 1;
        s.round
    };
    let choice = choice.to_string();
    let _ = std::thread::Builder::new().name("milan-gpu".into()).spawn(move || {
        let (names, gpu) = std::panic::catch_unwind(|| detect(&choice)).unwrap_or_else(|_| (Vec::new(), Err("The graphics driver failed".into())));
        let mut s = lock(&STATE);
        if s.round == round {
            s.adapters = Some(names);
            (s.gpu, s.why) = match gpu {
                Ok(g) => (Some(Arc::new(g)), String::new()),
                Err(e) => (None, format!("{e}: compositing on the CPU.")),
            };
        }
    });
}

/// (adapter names, the device for `choice`).
fn detect(choice: &str) -> (Vec<String>, Result<Gpu, String>) {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor { backends: wgpu::Backends::VULKAN, ..Default::default() });
    let adapters = instance.enumerate_adapters(wgpu::Backends::VULKAN);
    let mut names: Vec<String> = adapters.iter().map(|a| a.get_info().name).collect();
    names.dedup();
    // Automatic: a discrete GPU, else an integrated one, else a virtual one; never a software rasteriser, which is
    // slower than the CPU compositor.
    use wgpu::DeviceType::*;
    let picked = match choice {
        OFF => return (names, Err("GPU acceleration is off".into())),
        "" => [DiscreteGpu, IntegratedGpu, VirtualGpu].into_iter().find_map(|t| adapters.iter().find(|a| a.get_info().device_type == t)),
        name => adapters.iter().find(|a| a.get_info().name == name),
    };
    let gpu = match picked {
        Some(a) => Gpu::open(a),
        None if choice.is_empty() => Err("No Vulkan graphics adapter was found".into()),
        None => Err(format!("{choice} was not found")),
    };
    (names, gpu)
}

/// The GPU compositor to use now, if any.
pub fn current() -> Option<Arc<Gpu>> {
    #[cfg(test)]
    if std::env::var_os("MILAN_TEST_GPU").is_some() {
        return test_gpu();
    }
    lock(&STATE).gpu.clone().filter(|g| !g.broken())
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
        Some(g) => format!("Compositing on {} (Vulkan).", g.name),
        None => s.why.clone(),
    }
}

/// Tests: the automatic adapter, opened once; None (the GPU tests skip) when there is none.
#[cfg(test)]
pub fn test_gpu() -> Option<Arc<Gpu>> {
    static G: std::sync::OnceLock<Option<Arc<Gpu>>> = std::sync::OnceLock::new();
    G.get_or_init(|| detect("").1.ok().map(Arc::new)).clone()
}

/// Benches: compose on `g` (None: the CPU) whatever the preference says.
#[cfg(test)]
pub fn force(g: Option<Arc<Gpu>>) {
    let mut s = lock(&STATE);
    (s.choice, s.gpu) = (Some("test".into()), g);
}

/// Benches: the CPU, then the GPU when there is one (its name with `_` for spaces, for `tools/bench.ps1`).
#[cfg(test)]
pub fn compositors() -> Vec<(&'static str, Option<Arc<Gpu>>)> {
    let g = test_gpu();
    match &g {
        Some(g) => println!("BENCH adapter name={}", g.name.replace(' ', "_")),
        None => println!("SKIPPED the GPU runs: no Vulkan graphics adapter"),
    }
    std::iter::once(("cpu", None)).chain(g.map(|g| ("gpu", Some(g)))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::composite::tests::{event, layer, media};
    use crate::engine::plan::{self, MediaRef, SrcAt};
    use crate::engine::plugins::{find, preset_eval};
    use crate::project::{Halo, Motion, PanCrop};
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    /// The GPU, or None with a clear message (run with --nocapture to see it): the test then checks nothing.
    fn gpu_or_skip(test: &str) -> Option<Arc<Gpu>> {
        let g = test_gpu();
        if g.is_none() {
            println!("SKIPPED {test}: no Vulkan graphics adapter, so GPU-CPU parity was not checked");
        }
        g
    }

    fn hash(mut x: u32) -> u32 {
        x ^= x >> 16;
        x = x.wrapping_mul(0x7feb_352d);
        x ^= x >> 15;
        x = x.wrapping_mul(0x846c_a68b);
        x ^ (x >> 16)
    }

    struct Rng(u32);
    impl Rng {
        fn f(&mut self) -> f32 {
            self.0 = hash(self.0.wrapping_add(0x9e37_79b9));
            (self.0 >> 8) as f32 / (1 << 24) as f32
        }
        fn pick<T: Copy>(&mut self, v: &[T]) -> T {
            v[(self.f() * v.len() as f32) as usize % v.len()]
        }
    }

    /// Pictures by media id at any size, made once: gradients with noise (so sampling has to be exact); ids starting
    /// with "a" have alpha (clear, half, opaque and random pixels), premultiplied.
    #[derive(Default)]
    struct Noise(HashMap<(Arc<str>, u32, u32), Arc<ColorImage>>);
    impl Sources for Noise {
        fn get(&mut self, s: &SrcAt, w: u32, h: u32) -> Option<Arc<ColorImage>> {
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
    fn diff(a: &ColorImage, b: &ColorImage) -> (u8, usize) {
        assert_eq!(a.size, b.size);
        a.pixels.iter().zip(&b.pixels).flat_map(|(x, y)| x.to_array().into_iter().zip(y.to_array())).fold((0, 0), |(m, n), (x, y)| (m.max(x.abs_diff(y)), n + (x != y) as usize))
    }

    /// `fp` on both, at most 2 in 255 apart.
    fn check(g: &Gpu, fp: &FramePlan, [w, h]: [usize; 2], bilinear: bool, what: &str) -> (u8, usize) {
        let mut src = Noise::default();
        let cpu = composite::compose(fp, &mut src, w, h, bilinear, 3);
        let gpu = g.compose(fp, &mut src, w, h, bilinear, 3).unwrap_or_else(|| panic!("{what}: the GPU did not compose it"));
        let d = diff(&cpu, &gpu);
        assert!(d.0 <= 2, "{what}: the GPU frame differs from the CPU's by up to {} ({} channels)\n{fp:?}", d.0, d.1);
        d
    }

    fn ev(r: &mut Rng, id: String) -> plan::EventPlan {
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
            let fp = random_frame(&mut r);
            let size = r.pick(&[[480, 270], [320, 180], [257, 143]]);
            let (d, _) = check(&g, &fp, size, r.f() < 0.8, &format!("plan {n}"));
            (worst, exact) = (worst.max(d), exact + (d == 0) as usize);
        }
        println!("gpu_matches_cpu_on_random_plans on {}: largest difference {worst}, {exact} of 300 frames bit-exact", g.name);
    }

    /// What the GPU leaves to the CPU gives the CPU's frame too: track FX, 3D Track Motion, 2D Shadow and Glow, a
    /// transition with a kind, Adjustment Tracks and Events, a Custom compositor, Hard Mix, Video Output FX and the bus's
    /// Fade to Color; a frame larger than the adapter's textures is left to the CPU.
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
        l.fx = vec![fx("milan.fx.invert")];
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
        let l = one(&mut r, Composite::HardMix);
        frame("Hard Mix", &mut r, l);
        let mut l = one(&mut r, Composite::SourceAlpha);
        l.parent = Some((Composite::HardMix, Affine::scale(1.0, 1.0)));
        frame("Hard Mix parent", &mut r, l);
        let l = one(&mut r, Composite::Multiply);
        frame("output FX and fade", &mut r, l);
        let last = cases.len() - 1;
        (cases[last].1.out_fx, cases[last].1.fade) = (vec![fx("milan.fx.invert")], Some(([255, 0, 0], 0.4)));
        for (name, fp) in &cases {
            assert_eq!(check(&g, fp, [320, 180], true, name), (0, 0), "{name} is made by the CPU code, exactly");
        }
        let big = g.max as usize + 1;
        assert!(g.compose(&cases[0].1, &mut Noise::default(), big, 2, true, 1).is_none(), "too large for the adapter: the CPU composes it");
    }

    #[test]
    #[ignore]
    fn probe_float() {
        let g = test_gpu().unwrap();
        let src = format!("{}\n{}", include_str!("composite.wgsl"), r#"
fn blend_dbg(d: vec4<u32>, s: vec4<u32>, mode: u32, o: f32) -> vec4<f32> {
    let dp = unit(d);
    let su = unit(s);
    let sp = vec4<f32>(mul(su.r, o), mul(su.g, o), mul(su.b, o), mul(su.a, o));
    let ab = dp.a;
    let a = sp.a;
    let b = clamp(mix_mode(mode, straight(dp, ab), straight(sp, a)), vec3<f32>(0.0), vec3<f32>(1.0));
    let c = mul3(sp.rgb, 1.0 - ab) + mul3(dp.rgb, 1.0 - a) + mul3(b, mul(a, ab));
    return vec4<f32>(b.b, c.b, sp.b, f32(quantize(vec4<f32>(c, a + ab - mul(a, ab))).b));
}
fn h(x0: u32) -> u32 { var x = x0; x ^= x >> 16u; x *= 0x7feb352du; x ^= x >> 15u; x *= 0x846ca68bu; return x ^ (x >> 16u); }
@fragment fn fs_probe(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = vec2<u32>(pos.xy);
    let i = q.y * 256u + q.x;
    let a = f32(h(i) & 0xffffffu) * (1.0 / 16777216.0);
    let b = f32(h(i ^ 0x5bd1e995u) & 0xffffffu) * (1.0 / 16777216.0) + 0.001;
    var r: f32;
    switch p.k.y { case 0u: { r = div(a, b); } case 1u: { r = root(a); } case 2u: { r = mul(a, b) + a; } case 3u: { r = a / b; } default: {
        let dp = unit(vec4<u32>(50u, 88u, 56u, 221u) + p.k.w);
        let su = unit(vec4<u32>(55u, 105u, 83u, 127u) + p.k.w);
        let o = p.f.x;
        let sp = vec4<f32>(mul(su.r, o), mul(su.g, o), mul(su.b, o), mul(su.a, o));
        let ab = dp.a;
        let aa = sp.a;
        let cb = straight(dp, ab);
        let cs = straight(sp, aa);
        let bb = clamp(mix_mode(22u + p.k.w, cb, cs), vec3<f32>(0.0), vec3<f32>(1.0));
        let c = mul3(sp.rgb, 1.0 - ab) + mul3(dp.rgb, 1.0 - aa) + mul3(bb, mul(aa, ab));
        let full = blend_px(vec4<u32>(50u, 88u, 56u, 221u) + p.k.w, vec4<u32>(55u, 105u, 83u, 127u) + p.k.w, 22u + p.k.w, o, false);
        let v1 = blend_px(vec4<u32>(50u, 88u, 56u, 221u), vec4<u32>(55u, 105u, 83u, 127u), 22u, 179.0 / 255.0, false);
        let v2 = blend_px(vec4<u32>(50u, 88u, 56u, 221u) + p.k.w, vec4<u32>(55u, 105u, 83u, 127u) + p.k.w, 22u, o, false);
        let ab2 = dp.a;
        let a2 = sp.a;
        let c2 = mul3(sp.rgb, 1.0 - ab2) + mul3(dp.rgb, 1.0 - a2) + mul3(bb, mul(a2, ab2));
        let al = a2 + ab2 - mul(a2, ab2);
        let v3 = quantize(vec4<f32>(c2, al));
        let dbg = blend_dbg(vec4<u32>(50u, 88u, 56u, 221u) + p.k.w, vec4<u32>(55u, 105u, 83u, 127u) + p.k.w, 22u, o);
        let vals = array<f32, 8>(dbg.x, dbg.y, dbg.z, dbg.w, bb.b, c.b, mul(c.b, 255.0), f32(full.b));
        r = vals[min(i, 7u)];
    } }
    let u = bitcast<u32>(r);
    return vec4<u32>(u & 255u, (u >> 8u) & 255u, (u >> 16u) & 255u, u >> 24u);
}"#);
        let shader = g.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(src.into()) });
        let pl = g.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&g.layout], push_constant_ranges: &[] });
        let pipe = g.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None, layout: Some(&pl),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_probe"), targets: &[Some(wgpu::ColorTargetState { format: FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
            primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview: None, cache: None,
        });
        for op in 0..5u32 {
            let enc = g.device.create_command_encoder(&Default::default());
            let mut f = Frame { g: &g, enc, w: 256, h: 256, spare: Vec::new(), written: Vec::new(), bilinear: true, k: 1 };
            let t = f.clear(Color32::TRANSPARENT);
            let out = f.pass(&pipe, t, None, params([0; 4], [0; 4], [0; 4], [0, op, 0, 0], [179.0 / 255.0, 0.0, 0.0, 0.0]), &[]);
            let img = f.read(&out).unwrap();
            if op == 4 {
                println!("gpu: {:?}", (0..8).map(|i| f32::from_bits(u32::from_le_bytes(img.pixels[i].to_array()))).collect::<Vec<_>>());
            }
            let mut bad = 0;
            for i in 0..65536u32 {
                let a = (hash(i) & 0xffffff) as f32 * (1.0 / 16777216.0);
                let b = (hash(i ^ 0x5bd1e995) & 0xffffff) as f32 * (1.0 / 16777216.0) + 0.001;
                let want = match op { 0 | 3 => a / b, 1 => a.sqrt(), 2 => a * b + a, _ => a.mul_add(b, -(a * b)) };
                let got = f32::from_bits(u32::from_le_bytes(img.pixels[i as usize].to_array()));
                if got != want { bad += 1; if bad < 3 { println!("op {op}: {a} {b}: got {got} want {want}"); } }
            }
            println!("op {op}: {bad} of 65536 differ");
        }
    }

    #[test]
    #[ignore]
    fn probe_modes() {
        {
            let (d, s, o) = ([50f32, 88.0, 56.0, 221.0], [55f32, 105.0, 83.0, 127.0], (0.7f32 * 255.0).round() / 255.0);
            let dp = d.map(|v| v / 255.0);
            let sp = s.map(|v| v / 255.0 * o);
            let (ab, a) = (dp[3], sp[3]);
            let st = |c: [f32; 4], a: f32| [c[0] / a, c[1] / a, c[2] / a];
            let lum = |c: [f32; 3]| 0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2];
            let (cb, cs) = (st(dp, ab), st(sp, a));
            let dl = lum(cs) - lum(cb);
            let c: [f32; 3] = cb.map(|v| v + dl);
            let l = lum(c);
            let (n, x) = (c[0].min(c[1]).min(c[2]), c[0].max(c[1]).max(c[2]));
            println!("cb {cb:?} cs {cs:?} c {c:?} l {l} n {n} x {x}");
            let b = c[2];
            let v = sp[2] * (1.0 - ab) + dp[2] * (1.0 - a) + a * ab * b.clamp(0.0, 1.0);
            println!("v*255 = {:.9} (f64 {:.9})", v * 255.0, v as f64 * 255.0);
            println!("cpu: {:?}", [cb[2], cs[2], lum(cs), lum(cb), b.clamp(0.0, 1.0), v, v * 255.0, sp[2]]);
        }
        let g = test_gpu().unwrap();
        let mut r = Rng(1);
        let mut hist = [0usize; 256];
        for _ in 0..300 {
            let fp = random_frame(&mut r);
            let size = r.pick(&[[480, 270], [320, 180], [257, 143]]);
            let bil = r.f() < 0.8;
            let mut src = Noise::default();
            let cpu = composite::compose(&fp, &mut src, size[0], size[1], bil, 3);
            let gpu = g.compose(&fp, &mut src, size[0], size[1], bil, 3).unwrap();
            let d = diff(&cpu, &gpu);
            hist[d.0 as usize] += 1;
            if d.0 > 2 {
                println!("over 2: {d:?} modes {:?}", fp.layers.iter().map(|l| (l.mode, l.child, l.parent.map(|p| p.0), l.fade.is_some(), fp.alpha)).collect::<Vec<_>>());
            }
        }
        println!("hist {:?}", hist.iter().enumerate().filter(|x| *x.1 > 0).collect::<Vec<_>>());
        for alpha in [false, true] {
            for mode in MODES {
                let mut r = Rng(3);
                let bg = layer(vec![Item::Event(ev(&mut r, "a0".into()))], Composite::SourceAlpha, 1.0, Motion::default());
                let top = layer(vec![Item::Event(ev(&mut r, "a1".into()))], mode, 0.7, Motion::default());
                let fp = FramePlan { alpha, fade: None, samples: Vec::new(), out_fx: Vec::new(), layers: vec![bg, top] };
                let mut src = Noise::default();
                let cpu = composite::compose(&fp, &mut src, 480, 270, true, 3);
                let gpu = g.compose(&fp, &mut src, 480, 270, true, 3).unwrap();
                println!("alpha={alpha} {mode:?}: {:?}", diff(&cpu, &gpu));
                if let Some(i) = (0..cpu.pixels.len()).find(|&i| cpu.pixels[i] != gpu.pixels[i]) {
                    let fp0 = FramePlan { alpha, fade: None, samples: Vec::new(), out_fx: Vec::new(), layers: vec![layer(vec![Item::Event(ev(&mut Rng(3), "a0".into()))], Composite::SourceAlpha, 1.0, Motion::default())] };
                    let d = composite::compose(&fp0, &mut src, 480, 270, true, 3).pixels[i];
                    let s = composite::layer_image(&mut src, &fp.layers[1], [480, 270], true, 3).pixels[i];
                    println!("  px {i}: d {:?} s {:?} cpu {:?} gpu {:?}", d.to_array(), s.to_array(), cpu.pixels[i].to_array(), gpu.pixels[i].to_array());
                }
            }
        }
    }

    /// Off lists the adapters but opens none, so the CPU composes.
    #[test]
    fn off_lists_the_adapters_and_composes_on_the_cpu() {
        if std::env::var_os("MILAN_TEST_GPU").is_some() {
            return; // the whole run is forced onto the GPU
        }
        choose(OFF);
        let t = Instant::now();
        while adapters().is_none() && t.elapsed() < Duration::from_secs(30) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(adapters().is_some(), "the adapters are listed");
        assert!(current().is_none() && status().contains("off"), "{}", status());
    }

    /// R21a: four 1920x1080 layers composed on the CPU and on the GPU from decoded pictures (upload and readback
    /// included): full frames at Level 50 % (the playback benches' project), then picture-in-pictures turned 15°.
    /// `cargo test --release -- --ignored --nocapture bench_compose`
    #[test]
    #[ignore]
    fn bench_compose() {
        let mo = |i: usize, pip: bool| if pip { Motion { w: 0.45, h: 0.45, x: (i % 2) as f32 * 0.5 - 0.25, y: (i / 2) as f32 * 0.5 - 0.25, rot: 15.0, ..Default::default() } } else { Motion::default() };
        let fp = |pip: bool| FramePlan {
            alpha: false,
            fade: None,
            samples: Vec::new(),
            out_fx: Vec::new(),
            layers: (0..4).map(|i| layer(vec![Item::Event(event(&Arc::new(MediaRef { id: format!("m{i}").into(), ..(*media(1920, 1080)).clone() }), PanCrop::default(), 1.0))], Composite::SourceAlpha, 0.5, mo(i, pip))).collect(),
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
        for (kind, pip) in [("full", false), ("pip", true)] {
            let (fp, mut src) = (fp(pip), Noise::default());
            for (name, g) in compositors() {
                let k = composite::stripes();
                let t = ms(&mut || {
                    let img = match &g {
                        Some(g) => g.compose(&fp, &mut src, 1920, 1080, true, k).unwrap(),
                        None => composite::compose(&fp, &mut src, 1920, 1080, true, k),
                    };
                    std::hint::black_box(img);
                });
                println!("BENCH compose kind={kind} compositor={name} layers=4 ms={t:.2}");
            }
        }
    }
}
