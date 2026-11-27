//! R40b: the float compositor on the GPU: `wide::compose::compose` with `Rgba16Float` textures (`wide.wgsl`). Each
//! event's picture (its source, resampling, flicker filter, event FX and masks), a track with track FX, 3D Track
//! Motion, 2D Shadow / Glow or a Custom compositor, and a transition with a kind are made by the CPU's float code and
//! uploaded as `Rgba32Float`; the draws (Track Motion, Pan/Crop, Parent Motion), every blend mode, opacity, Fade to
//! Color and the compositing groups run here. A frame with Adjustment FX, Video Output FX or a LUT view transform is
//! left to the CPU (`compose_wide` gives None). The frame stays on the GPU twice: in float (`WideOut`, read back by
//! a render) and through the view transform in 8 bits (`Output`, what the Video Preview shows without a copy).
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Weak, mpsc};
use wgpu::util::DeviceExt;
use super::{GpuCompositor, Intermediate, Output, half, lock};
use crate::engine::composite::{self, Affine, Sources};
use crate::engine::picture::Picture;
use crate::engine::plan::{FramePlan, Item, TrackPlan};
use crate::engine::wide::{self, Enc, Out, Prim, WImage, Working, compose};
use crate::project::Composite;

/// What composed frames are kept in.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// What the CPU's pictures are uploaded as.
const UPLOAD: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;

pub struct Pipes {
    layout: wgpu::BindGroupLayout,
    draw: wgpu::RenderPipeline,
    blend: wgpu::RenderPipeline,
    fade: wgpu::RenderPipeline,
    view: wgpu::RenderPipeline,
}

impl Pipes {
    pub fn new(device: &wgpu::Device) -> Pipes {
        let src = include_str!("wide.wgsl");
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("wide.wgsl"), source: wgpu::ShaderSource::Wgsl(src.into()) });
        let stage = wgpu::ShaderStages::FRAGMENT;
        let tex = |binding| wgpu::BindGroupLayoutEntry { binding, visibility: stage, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None };
        let buf = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: stage, ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None }, count: None };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("wide"),
            entries: &[tex(0), tex(1), buf(2, wgpu::BufferBindingType::Uniform), buf(3, wgpu::BufferBindingType::Storage { read_only: true })],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&layout], push_constant_ranges: &[] });
        let pipe = |fs: &str, format: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fs),
                layout: Some(&pl),
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
                fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some(fs), targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        Pipes { draw: pipe("fs_draw", FORMAT), blend: pipe("fs_blend", FORMAT), fade: pipe("fs_fade", FORMAT), view: pipe("fs_view", wgpu::TextureFormat::Rgba8Unorm), layout }
    }
}

pub(super) struct WTex {
    tex: wgpu::Texture,
    view: wgpu::TextureView,
    w: u32,
    h: u32,
    format: wgpu::TextureFormat,
    /// Written from the CPU: reused only after the frame.
    cpu: bool,
}

fn new_tex(device: &wgpu::Device, w: u32, h: u32, format: wgpu::TextureFormat) -> WTex {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("milan-wide"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = tex.create_view(&Default::default());
    WTex { tex, view, w, h, format, cpu: false }
}

/// What a pass binds where it reads no source (wgpu clears a texture before its first use).
pub(super) fn none(device: &wgpu::Device) -> WTex {
    new_tex(device, 1, 1, FORMAT)
}

fn bytes(t: &WTex) -> usize {
    t.w as usize * t.h as usize * if t.format == UPLOAD { 16 } else { 8 }
}

impl GpuCompositor {
    fn wtex(&self, w: u32, h: u32, format: wgpu::TextureFormat) -> WTex {
        let mut pool = lock(&self.wpool);
        match pool.iter().position(|t| (t.w, t.h, t.format) == (w, h, format)) {
            Some(i) => pool.remove(i),
            None => new_tex(&self.device, w, h, format),
        }
    }

    fn give_wide(&self, ts: impl IntoIterator<Item = WTex>) {
        let mut pool = lock(&self.wpool);
        pool.extend(ts.into_iter().map(|t| WTex { cpu: false, ..t }));
        let mut total: usize = pool.iter().map(bytes).sum();
        while total > super::POOL_BYTES {
            total -= bytes(&pool.remove(0));
        }
    }

    /// `wide::compose::compose` on the GPU, the frame left there; None when this frame is the CPU's.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn compose_wide_gpu(&self, fp: &FramePlan, src: &mut dyn Sources, w: usize, h: usize, bilinear: bool, k: usize, wk: &Arc<Working>) -> Option<Picture> {
        if self.broken() || !self.wide_ok || w == 0 || h == 0 || w.max(h) > self.max as usize || wk.out == Out::Lut || !fp.out_fx.is_empty() || fp.layers.iter().any(|l| !l.adjust.is_empty()) {
            return None;
        }
        let run = || {
            let enc = self.device.create_command_encoder(&Default::default());
            let mut f = WFrame { g: self, enc, w: w as u32, h: h as u32, spare: Vec::new(), written: Vec::new(), bilinear, k, wk };
            let out = f.frame(fp, src);
            f.done();
            out
        };
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).unwrap_or_else(|_| {
            self.broken.store(true, Relaxed);
            None
        });
        let (float, view) = out.filter(|_| !self.broken())?;
        let me = self.me.clone();
        Some(Picture::wide_on_gpu(WideOut { g: me.clone(), tex: Some(float) }, Output { g: me, tex: Some(view) }, wk.clone()))
    }

    /// `WideOut::read`.
    fn read_wide(&self, t: &WTex) -> WImage {
        self.readbacks.fetch_add(1, Relaxed);
        let (w, h) = (t.w as usize, t.h as usize);
        let row = (t.w * 8).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
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
        if rx.recv().ok().and_then(Result::ok).is_none() || self.broken() {
            return WImage::new([w, h], [0.0, 0.0, 0.0, 1.0]);
        }
        let data = buf.slice(..).get_mapped_range();
        let mut px = Vec::with_capacity(w * h);
        for y in 0..h {
            let line = &data[y * row as usize..][..w * 8];
            px.extend(line.chunks_exact(8).map(|c| std::array::from_fn(|i| half(u16::from_le_bytes([c[i * 2], c[i * 2 + 1]])))));
        }
        WImage { size: [w, h], px }
    }
}

/// R40b: a float frame on the GPU (`Rgba16Float`), back to the compositor's pool when dropped.
pub struct WideOut {
    g: Weak<GpuCompositor>,
    tex: Option<WTex>,
}

impl WideOut {
    /// The float frame, read back.
    pub fn read(&self) -> WImage {
        let t = self.tex.as_ref().expect("an output until dropped");
        match self.g.upgrade() {
            Some(g) => g.read_wide(t),
            None => WImage::new([t.w as usize, t.h as usize], [0.0, 0.0, 0.0, 1.0]),
        }
    }
}

impl Drop for WideOut {
    fn drop(&mut self) {
        if let (Some(g), Some(t)) = (self.g.upgrade(), self.tex.take()) {
            g.give_wide([t]);
        }
    }
}

/// The uniform block (`P` in wide.wgsl).
#[derive(Default, Clone, Copy)]
struct U {
    rect: [i32; 4],
    uv: [i32; 4],
    off: [i32; 4],
    k: [u32; 4],
    f: [f32; 4],
    g: [f32; 4],
    m: [[f32; 4]; 3],
}

impl U {
    fn words(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::with_capacity(36);
        v.extend(self.rect.iter().chain(&self.uv).chain(&self.off).map(|x| *x as u32));
        v.extend(self.k);
        v.extend(self.f.iter().chain(&self.g).chain(self.m.iter().flatten()).map(|x| x.to_bits()));
        v
    }
}

struct WFrame<'a> {
    g: &'a GpuCompositor,
    enc: wgpu::CommandEncoder,
    w: u32,
    h: u32,
    spare: Vec<WTex>,
    written: Vec<WTex>,
    bilinear: bool,
    k: usize,
    wk: &'a Working,
}

impl WFrame<'_> {
    fn frame(&mut self, fp: &FramePlan, src: &mut dyn Sources) -> Option<(WTex, super::OutTex)> {
        let (w, h) = (self.w, self.h);
        let black = self.wk.encode(0.0) as f64;
        let mut acc = self.clear(w, h, if fp.alpha { [0.0; 4] } else { [black, black, black, 1.0] });
        let mut group: Option<WTex> = None;
        for l in &fp.layers {
            if l.child {
                let g = match group.take() {
                    Some(g) => g,
                    None => self.clear(w, h, [0.0; 4]),
                };
                group = Some(self.layer_onto(g, src, l, true)?);
                continue;
            }
            let Some((mode, motion)) = l.parent else {
                if let Some(g) = group.take() {
                    acc = self.blend(acc, g, Composite::SourceAlpha, 1.0, false);
                }
                acc = self.layer_onto(acc, src, l, false)?;
                continue;
            };
            let mut g = match group.take() {
                Some(g) => g,
                None => self.clear(w, h, [0.0; 4]),
            };
            if !l.items.is_empty() {
                g = self.layer_onto(g, src, l, true)?;
            }
            if composite::whole_pixel(motion) != Some((0, 0)) {
                let (wf, hf) = (w as f64, h as f64);
                let blank = self.clear(w, h, [0.0; 4]);
                let moved = self.draw(blank, &g, Affine::scale(1.0 / wf, 1.0 / hf).then(motion).then(Affine::scale(wf, hf)), 1.0);
                self.give(g);
                g = moved;
            }
            acc = self.blend(acc, g, mode, 1.0, false);
        }
        if let Some(g) = group {
            acc = self.blend(acc, g, Composite::SourceAlpha, 1.0, false);
        }
        if let Some((c, t)) = fp.fade {
            acc = self.fade(acc, self.wk.color8(c), t);
        }
        let view = self.view(&acc);
        let enc = std::mem::replace(&mut self.enc, self.g.device.create_command_encoder(&Default::default()));
        self.g.queue.submit([enc.finish()]);
        Some((acc, view))
    }

    /// The frame through the view transform into an 8-bit output texture (the preview's).
    fn view(&mut self, acc: &WTex) -> super::OutTex {
        let g = self.g;
        let out = g.output(acc.w, acc.h, Intermediate::Rgba8);
        let wk = self.wk;
        let m = wk.matrix709();
        let enc = match wk.enc {
            Enc::Video => 0,
            Enc::Linear => 1,
            Enc::Cc => 2,
            Enc::Cct => 3,
        };
        let view = match wk.out {
            Out::Video => 0,
            Out::AcesSrgb => 1,
            Out::AcesRec709 => 2,
            _ => 3,
        };
        let plain = (wk.enc == Enc::Video && wk.prim == Prim::Rec709) as u32;
        let u = U { k: [enc, view, plain, 0], g: [wide::HDR_PEAK / wide::REF_WHITE, 0.0, 0.0, 0.0], m: m.map(|r| [r[0], r[1], r[2], 0.0]), ..Default::default() };
        let _step = lock(&g.record);
        let bind = self.bind(acc, None, &u, &[]);
        let ops = wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store };
        let at = wgpu::RenderPassColorAttachment { view: &out.target, resolve_target: None, ops };
        let mut rp = self.enc.begin_render_pass(&wgpu::RenderPassDescriptor { color_attachments: &[Some(at)], ..Default::default() });
        rp.set_pipeline(&g.wide.view);
        rp.set_bind_group(0, &bind, &[]);
        rp.draw(0..3, 0..1);
        drop(rp);
        out
    }

    fn layer_onto(&mut self, dst: WTex, src: &mut dyn Sources, l: &TrackPlan, group: bool) -> Option<WTex> {
        if l.items.is_empty() {
            return Some(dst);
        }
        let (w, h) = (self.w, self.h);
        if !l.fx.is_empty() || l.persp.is_some() || l.shadow.is_some() || l.glow.is_some() || l.mode == Composite::Custom {
            // The whole track on the CPU (exact), blended here; a Custom compositor blends there too.
            let layer = compose::layer_image(src, l, [w as usize, h as usize], self.bilinear, self.k, self.wk);
            if l.mode == Composite::Custom {
                let mut d = self.read(&dst)?;
                self.give(dst);
                compose::blend(&mut d, &layer, l.mode, l.opacity, l.compositor.as_ref(), group, self.k, self.wk);
                return self.upload(&d);
            }
            let t = self.upload(&layer)?;
            return Some(self.blend(dst, t, l.mode, l.opacity, group));
        }
        let draws: usize = l.items.iter().map(|i| composite::events(i).len()).sum();
        if l.fade.is_none() && matches!(l.mode, Composite::SourceAlpha | Composite::ThreeD) && (l.opacity >= 1.0 || draws <= 1) {
            return self.draw_items(dst, src, l, l.opacity);
        }
        let blank = self.clear(w, h, [0.0; 4]);
        let mut layer = self.draw_items(blank, src, l, 1.0)?;
        if let Some((c, t)) = l.fade {
            layer = self.fade(layer, self.wk.color8(c), t);
        }
        Some(self.blend(dst, layer, l.mode, l.opacity, group))
    }

    fn draw_items(&mut self, mut dst: WTex, src: &mut dyn Sources, l: &TrackPlan, gain: f32) -> Option<WTex> {
        let (w, h) = (self.w as usize, self.h as usize);
        let to_out = composite::layer_to_out(l, w, h);
        for item in &l.items {
            match item {
                Item::Mix { a, b, p, kind: Some(kind) } => {
                    let img = compose::transition_image(src, a.as_ref(), b.as_ref(), *p, kind, to_out, [w, h], self.bilinear, self.k, self.wk);
                    let t = self.upload(&img)?;
                    dst = self.draw(dst, &t, Affine::scale(1.0 / w as f64, 1.0 / h as f64).then(to_out), gain);
                    self.give(t);
                }
                _ => {
                    let (p, out) = if let Item::Mix { p, b, .. } = item { (*p, b.is_none()) } else { (1.0, false) };
                    let evs = composite::events(item);
                    let last = evs.len().saturating_sub(1);
                    for (i, e) in evs.into_iter().enumerate() {
                        let alpha = e.alpha * gain * if out { 1.0 - p } else if i == last { p } else { 1.0 };
                        if alpha <= 0.0 {
                            continue;
                        }
                        let Some(img) = compose::event_image(src, e, to_out, self.bilinear, self.k, self.wk) else { continue };
                        let t = self.upload(&img)?;
                        dst = self.draw(dst, &t, composite::event_to_size(img.size, e, to_out), alpha);
                        self.give(t);
                    }
                }
            }
        }
        Some(dst)
    }

    /// `compose::draw`: `s` through `m` at `alpha` onto `dst`.
    fn draw(&mut self, dst: WTex, s: &WTex, m: Affine, alpha: f32) -> WTex {
        let a = alpha.clamp(0.0, 1.0);
        let (size, out) = ([s.w as usize, s.h as usize], [dst.w as usize, dst.h as usize]);
        let Some(inv) = m.inverse() else { return dst };
        if a <= 0.0 || s.w == 0 || s.h == 0 {
            return dst;
        }
        let Some([x0, x1, y0, y1]) = composite::bounds(m, size, out) else { return dst };
        let i32_of = |v: i64| v.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        let (u, rows) = match composite::whole_pixel(m) {
            Some((ox, oy)) => {
                let Some([x0, x1, y0, y1]) = composite::spans((ox, oy), size, out) else { return dst };
                (U { rect: [x0, y0, x1, y1].map(|v| v as i32), off: [i32_of(ox), i32_of(oy), 1, 0], f: [a, 0.0, 0.0, 0.0], ..Default::default() }, Vec::new())
            }
            None => {
                let (du, dv) = ((inv.a * composite::ONE).round() as i64, (inv.c * composite::ONE).round() as i64);
                let rows: Vec<[i32; 2]> = (y0..y1)
                    .map(|y| {
                        let (u0, v0) = inv.apply(x0 as f64 + 0.5, y as f64 + 0.5);
                        [i32_of((u0 * composite::ONE).floor() as i64), i32_of((v0 * composite::ONE).floor() as i64)]
                    })
                    .collect();
                let uv = [i32_of(du), i32_of(dv), i32_of((s.w as i64) << 16), i32_of((s.h as i64) << 16)];
                (U { rect: [x0, y0, x1, y1].map(|v| v as i32), uv, off: [0, 0, 0, self.bilinear as i32], f: [a, 0.0, 0.0, 0.0], ..Default::default() }, rows)
            }
        };
        let words: Vec<u32> = rows.iter().flatten().map(|v| *v as u32).collect();
        let out = self.pass(Pass::Draw, &dst, Some(s), &u, &words);
        self.give(dst);
        out
    }

    fn blend(&mut self, dst: WTex, layer: WTex, mode: Composite, opacity: f32, group: bool) -> WTex {
        let (lo, hi) = self.wk.range();
        let u = U { k: [mode as u32, group as u32, 0, 0], f: [opacity.clamp(0.0, 1.0), lo, hi, 0.0], ..Default::default() };
        let out = self.pass(Pass::Blend, &dst, Some(&layer), &u, &[]);
        self.give(dst);
        self.give(layer);
        out
    }

    fn fade(&mut self, t: WTex, c: [f32; 3], how_far: f32) -> WTex {
        let u = U { f: [c[0], c[1], c[2], 0.0], g: [how_far, 0.0, 0.0, 0.0], ..Default::default() };
        let out = self.pass(Pass::Fade, &t, None, &u, &[]);
        self.give(t);
        out
    }

    fn target(&mut self, w: u32, h: u32) -> WTex {
        match self.spare.iter().position(|t| (t.w, t.h) == (w, h)) {
            Some(i) => self.spare.swap_remove(i),
            None => self.g.wtex(w, h, FORMAT),
        }
    }

    fn give(&mut self, t: WTex) {
        if t.cpu || t.format == UPLOAD { self.written.push(t) } else { self.spare.push(t) }
    }

    fn clear(&mut self, w: u32, h: u32, [r, g, b, a]: [f64; 4]) -> WTex {
        let _step = lock(&self.g.record);
        let t = self.target(w, h);
        let ops = wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }), store: wgpu::StoreOp::Store };
        let at = wgpu::RenderPassColorAttachment { view: &t.view, resolve_target: None, ops };
        drop(self.enc.begin_render_pass(&wgpu::RenderPassDescriptor { color_attachments: &[Some(at)], ..Default::default() }));
        t
    }

    fn bind(&self, dst: &WTex, src: Option<&WTex>, u: &U, data: &[u32]) -> wgpu::BindGroup {
        let g = self.g;
        let uniform = g.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(&u.words()), usage: wgpu::BufferUsages::UNIFORM });
        let data = (!data.is_empty()).then(|| g.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(data), usage: wgpu::BufferUsages::STORAGE }));
        g.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &g.wide.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&dst.view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&src.unwrap_or(&g.wnone).view) },
                wgpu::BindGroupEntry { binding: 2, resource: uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: data.as_ref().unwrap_or(&g.no_rows).as_entire_binding() },
            ],
        })
    }

    fn pass(&mut self, which: Pass, dst: &WTex, src: Option<&WTex>, u: &U, data: &[u32]) -> WTex {
        let g = self.g;
        let _step = lock(&g.record);
        let out = self.target(dst.w, dst.h);
        let bind = self.bind(dst, src, u, data);
        let pipe = match which {
            Pass::Draw => &g.wide.draw,
            Pass::Blend => &g.wide.blend,
            Pass::Fade => &g.wide.fade,
        };
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

    /// `img` as an `Rgba32Float` texture; None when the device cannot hold it.
    fn upload(&mut self, img: &WImage) -> Option<WTex> {
        let [w, h] = img.size.map(|v| v as u32);
        if w == 0 || h == 0 || w.max(h) > self.g.max {
            return None;
        }
        let _step = lock(&self.g.record);
        let t = WTex { cpu: true, ..self.g.wtex(w, h, UPLOAD) };
        let layout = wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(16 * w), rows_per_image: None };
        self.g.queue.write_texture(t.tex.as_image_copy(), bytemuck::cast_slice(&img.px), layout, t.tex.size());
        Some(t)
    }

    /// Run the recorded passes and read `t` back (a Custom compositor's backdrop).
    fn read(&mut self, t: &WTex) -> Option<WImage> {
        let enc = std::mem::replace(&mut self.enc, self.g.device.create_command_encoder(&Default::default()));
        self.g.queue.submit([enc.finish()]);
        let img = self.g.read_wide(t);
        (!self.g.broken()).then_some(img)
    }

    fn done(self) {
        self.g.give_wide(self.spare.into_iter().chain(self.written));
    }
}

#[derive(Clone, Copy)]
enum Pass {
    Draw,
    Blend,
    Fade,
}


#[cfg(test)]
mod tests {
    /// The float passes' shader parses and validates (naga, as wgpu compiles it), with or without a graphics adapter:
    /// a shader error would leave every frame, 8-bit ones too, to the CPU.
    #[test]
    fn wide_shader_validates() {
        use wgpu::naga::valid::{Capabilities, ValidationFlags, Validator};
        let src = include_str!("wide.wgsl");
        let module = wgpu::naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&src)));
        Validator::new(ValidationFlags::all(), Capabilities::empty()).validate(&module).unwrap_or_else(|e| panic!("{e:?}"));
        for entry in ["vs", "fs_draw", "fs_blend", "fs_fade", "fs_view"] {
            assert!(module.entry_points.iter().any(|e| e.name == entry), "{entry}");
        }
    }
}