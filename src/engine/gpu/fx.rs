//! R21b: the video FX, transitions and generators on the GPU: `fx.wgsl`'s passes driven with the values the CPU code
//! derives from a plug-in's parameters (`engine/vfx.rs`, `transition.rs`, `generate.rs`), so each gives the CPU's
//! picture. A plug-in without a `GpuPort` (`engine/plugins.rs`: Cookie Cutter, Bezier Masking, the text generators,
//! OFX plug-ins) is run by the CPU code on its picture, which is then uploaded (`Frame::event_tex`).
use eframe::egui::Color32;
use super::{Frame, Tex};
use crate::engine::composite::Affine;
use crate::engine::generate;
use crate::engine::lut::Cube;
use crate::engine::plugins::{FxEval, GpuPort, find};
use crate::engine::vfx::{self, ColorOp, GradeStep};

/// The passes of fx.wgsl.
pub(super) struct Pipes {
    pub color_op: wgpu::RenderPipeline,
    pub cube: wgpu::RenderPipeline,
    pub lerp: wgpu::RenderPipeline,
    pub resample: wgpu::RenderPipeline,
    pub flicker: wgpu::RenderPipeline,
    pub sharpen: wgpu::RenderPipeline,
    pub vignette: wgpu::RenderPipeline,
    pub mirror: wgpu::RenderPipeline,
    pub crop: wgpu::RenderPipeline,
    pub border: wgpu::RenderPipeline,
    pub chroma_mask: wgpu::RenderPipeline,
    pub chroma_apply: wgpu::RenderPipeline,
    pub transition: wgpu::RenderPipeline,
    pub generate: wgpu::RenderPipeline,
    pub opaque: wgpu::RenderPipeline,
    /// R21c: the frame into its output texture (`Intermediate::Rgba8`, `Intermediate::Rgba16Float`).
    pub present8: wgpu::RenderPipeline,
    pub present16: wgpu::RenderPipeline,
    /// `cs_box`, with its own layout: the passes' bindings plus the output storage texture.
    pub box_blur: wgpu::ComputePipeline,
    pub box_layout: wgpu::BindGroupLayout,
}

impl Pipes {
    pub fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, math: &str, render_to: &dyn Fn(&wgpu::ShaderModule, &str, wgpu::TextureFormat) -> wgpu::RenderPipeline) -> Pipes {
        let render = |module: &wgpu::ShaderModule, fs: &str| render_to(module, fs, super::FORMAT);
        let src = format!("{math}\n{}", include_str!("fx.wgsl"));
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("fx.wgsl"), source: wgpu::ShaderSource::Wgsl(src.into()) });
        let stage = wgpu::ShaderStages::COMPUTE;
        let tex = |binding| wgpu::BindGroupLayoutEntry { binding, visibility: stage, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Uint, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None };
        let buf = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: stage, ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None }, count: None };
        let out = wgpu::BindGroupLayoutEntry { binding: 4, visibility: stage, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: super::FORMAT, view_dimension: wgpu::TextureViewDimension::D2 }, count: None };
        let box_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("box"),
            entries: &[tex(0), tex(1), buf(2, wgpu::BufferBindingType::Uniform), buf(3, wgpu::BufferBindingType::Storage { read_only: true }), out],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&box_layout], push_constant_ranges: &[] });
        let box_blur = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some("cs_box"), layout: Some(&pl), module: &module, entry_point: Some("cs_box"), compilation_options: Default::default(), cache: None });
        let _ = layout;
        Pipes {
            color_op: render(&module, "fs_color_op"),
            cube: render(&module, "fs_cube"),
            lerp: render(&module, "fs_lerp"),
            resample: render(&module, "fs_resample"),
            flicker: render(&module, "fs_flicker"),
            sharpen: render(&module, "fs_sharpen"),
            vignette: render(&module, "fs_vignette"),
            mirror: render(&module, "fs_mirror"),
            crop: render(&module, "fs_crop"),
            border: render(&module, "fs_border"),
            chroma_mask: render(&module, "fs_chroma_mask"),
            chroma_apply: render(&module, "fs_chroma_apply"),
            transition: render(&module, "fs_transition"),
            generate: render(&module, "fs_generate"),
            opaque: render(&module, "fs_opaque"),
            present8: render_to(&module, "fs_present", wgpu::TextureFormat::Rgba8Unorm),
            present16: render_to(&module, "fs_present_linear", wgpu::TextureFormat::Rgba16Float),
            box_blur,
            box_layout,
        }
    }
}

/// fx.wgsl's uniform block `F`.
#[derive(Clone, Copy, Default)]
pub(crate) struct U {
    pub i: [i32; 4],
    pub j: [i32; 4],
    /// z[0] stays 0 (math.wgsl's `rd`); z[1..] a premultiplied colour.
    pub z: [u32; 4],
    pub f: [[f32; 4]; 8],
}

impl U {
    pub fn words(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.i.iter().chain(&self.j).map(|&x| x as u32).collect();
        v.extend(self.z);
        v.extend(self.f.iter().flatten().map(|f| f.to_bits()));
        v
    }

    /// A premultiplied colour: z.yzw and j.w.
    fn colour(mut self, c: Color32) -> Self {
        let [r, g, b, a] = c.to_array();
        self.z = [0, r as u32, g as u32, b as u32];
        self.j[3] = a as i32;
        self
    }
}

/// The GPU runs this plug-in (`PluginInfo::gpu`).
pub fn ported(e: &FxEval) -> bool {
    port(e).is_some()
}

fn port(e: &FxEval) -> Option<GpuPort> {
    find(e.id).and_then(|p| p.gpu)
}

/// `color_op`'s LUT as the pass reads it.
fn lut_words(op: &ColorOp) -> Vec<u32> {
    op.lut.iter().flatten().map(|&v| v as u32).collect()
}

/// A .cube's (shaper and main) tables as f32 bits: (words, shaper offset, main offset).
fn cube_words(c: &Cube) -> (Vec<u32>, usize, usize) {
    let mut v = Vec::new();
    if let Some(s) = &c.shaper {
        v.extend(s.data.iter().flatten().map(|f| f.to_bits()));
    }
    let main = v.len();
    v.extend(c.data.iter().flatten().map(|f| f.to_bits()));
    (v, 0, main)
}

fn span(c: &Cube) -> [f32; 4] {
    [(c.max[0] - c.min[0]).max(1e-6), (c.max[1] - c.min[1]).max(1e-6), (c.max[2] - c.min[2]).max(1e-6), 0.0]
}

impl Frame<'_> {
    /// Plug-in `e` on `t` (a picture of any size), a new texture; `t` is left as it was (the caller gives it back).
    pub(super) fn fx(&mut self, t: &Tex, e: &FxEval) -> Tex {
        let h = t.h as usize;
        let hf = h as f32;
        let pipes = &self.g.fx;
        match port(e).expect("a ported plug-in (`fx::ported`)") {
            GpuPort::ColorOp => {
                let op = vfx::color_op_for(e).expect("a colour plug-in");
                self.color_op(t, &op)
            }
            GpuPort::Grade => {
                let steps = vfx::Grade::new(e).steps();
                let mut cur: Option<Tex> = None;
                for step in &steps {
                    let input = cur.as_ref().unwrap_or(t);
                    let next = match step {
                        GradeStep::Op(op) => self.color_op(input, op),
                        GradeStep::Cube(c) => self.cube(input, c),
                    };
                    if let Some(old) = cur.replace(next) {
                        self.give(old);
                    }
                }
                cur.unwrap_or_else(|| self.copy(t))
            }
            GpuPort::Blur => {
                let (fx, fy) = vfx::blur_radii(e, h);
                self.soft_blur(t, fx, fy)
            }
            GpuPort::Sharpen => {
                let (amount, r, threshold) = vfx::sharpen_params(e, h);
                if r == 0 || amount <= 0.0 {
                    return self.copy(t);
                }
                let bl = self.blur(t, r, r).expect("a radius");
                let u = U { f: [[amount, threshold * 255.0, 0.0, 0.0], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4]], ..Default::default() };
                let out = self.pass(&pipes.sharpen, t, Some(&bl), &u.words(), &[]);
                self.give(bl);
                out
            }
            GpuPort::Vignette => {
                let (amount, size, soft) = (e.f("amount"), e.f("size"), e.f("softness"));
                let (cx, cy) = (t.w as f32 / 2.0, t.h as f32 / 2.0);
                let u = U { f: [[amount, size - soft / 2.0, size + soft / 2.0, cx], [cy, 0.0, 0.0, 0.0], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4]], ..Default::default() }.colour(vfx::premul(e.c("color")));
                self.pass(&pipes.vignette, t, None, &u.words(), &[])
            }
            GpuPort::Mirror => {
                let dir = e.i("direction");
                let n = if dir < 2 { t.w } else { t.h } as f32;
                let axis = (e.f("position").clamp(0.0, 1.0) * n).round() as i32;
                let u = U { i: [dir as i32, axis, 0, 0], ..Default::default() };
                self.pass(&pipes.mirror, t, None, &u.words(), &[])
            }
            GpuPort::Crop => {
                let wf = t.w as f32;
                let u = U { f: [[e.f("left") * wf, e.f("top") * hf, (1.0 - e.f("right")) * wf, (1.0 - e.f("bottom")) * hf], [e.f("feather") * hf, 0.0, 0.0, 0.0], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4]], ..Default::default() };
                self.pass(&pipes.crop, t, None, &u.words(), &[])
            }
            GpuPort::Border => {
                let u = U { f: [[e.f("size") * hf, 0.0, 0.0, 0.0], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4]], ..Default::default() }.colour(vfx::premul(e.c("color")));
                self.pass(&pipes.border, t, None, &u.words(), &[])
            }
            GpuPort::ChromaKey => {
                let key = e.c("color");
                let (kb, kr) = vfx::cbcr([key[0], key[1], key[2]]);
                let u = U { f: [[kb, kr, e.f("low"), e.f("high")], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4]], ..Default::default() };
                let mask = self.pass(&pipes.chroma_mask, t, None, &u.words(), &[]);
                let r = vfx::px_of(e.f("blur"), h);
                let mask = match self.blur(&mask, r, r) {
                    Some(b) => {
                        self.give(mask);
                        b
                    }
                    None => mask,
                };
                let d = (0..3).max_by(|&a, &b| key[a].total_cmp(&key[b])).unwrap_or(1);
                let u = U { i: [e.on("mask_only") as i32, d as i32, 0, 0], f: [[e.f("spill"), 0.0, 0.0, 0.0], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4]], ..Default::default() };
                let out = self.pass(&pipes.chroma_apply, t, Some(&mask), &u.words(), &[]);
                self.give(mask);
                out
            }
            GpuPort::Mvfx => self.mvfx(t, e),
            port => unreachable!("{port:?} is not a video FX"),
        }
    }

    /// `e`'s chain of FX on `t`, in order; None when it is empty.
    pub(super) fn fx_chain(&mut self, t: &Tex, fxs: &[FxEval]) -> Option<Tex> {
        let mut cur: Option<Tex> = None;
        for f in fxs {
            let next = self.fx(cur.as_ref().unwrap_or(t), f);
            if let Some(old) = cur.replace(next) {
                self.give(old);
            }
        }
        cur
    }

    /// `t` again (a lerp by 0): for a step that changes nothing, as the CPU's copy.
    pub(super) fn copy(&mut self, t: &Tex) -> Tex {
        let pipes = &self.g.fx;
        self.pass(&pipes.lerp, t, Some(t), &U::default().words(), &[])
    }

    /// `vfx::lerp_px` of `a` to `b` by `t`.
    pub(super) fn lerp(&mut self, a: &Tex, b: &Tex, t: f32) -> Tex {
        let pipes = &self.g.fx;
        let u = U { f: [[t, 0.0, 0.0, 0.0], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4]], ..Default::default() };
        self.pass(&pipes.lerp, a, Some(b), &u.words(), &[])
    }

    /// `composite::blend_frames`: `a` becomes (1 - w) a + w b.
    pub(super) fn resample(&mut self, a: &Tex, b: &Tex, w: f32) -> Tex {
        let pipes = &self.g.fx;
        let u = U { i: [(w.clamp(0.0, 1.0) * 256.0).round() as i32, 0, 0, 0], ..Default::default() };
        self.pass(&pipes.resample, a, Some(b), &u.words(), &[])
    }

    /// `composite::reduce_flicker`.
    pub(super) fn flicker(&mut self, t: &Tex) -> Tex {
        let pipes = &self.g.fx;
        self.pass(&pipes.flicker, t, None, &U::default().words(), &[])
    }

    /// `composite::finish`'s opaque step.
    pub(super) fn opaque(&mut self, t: &Tex) -> Tex {
        let pipes = &self.g.fx;
        self.pass(&pipes.opaque, t, None, &U::default().words(), &[])
    }

    fn color_op(&mut self, t: &Tex, op: &ColorOp) -> Tex {
        if op.is_identity() {
            return self.copy(t);
        }
        let mut u = U { i: [op.m.is_some() as i32, 0, 0, 0], ..Default::default() };
        if let Some(m) = op.m {
            for (i, row) in m.iter().enumerate() {
                u.f[i] = [row[0], row[1], row[2], 0.0];
            }
        }
        u.f[3][0] = op.mix;
        let pipes = &self.g.fx;
        self.pass(&pipes.color_op, t, None, &u.words(), &lut_words(op))
    }

    fn cube(&mut self, t: &Tex, c: &Cube) -> Tex {
        let (words, shaper_at, main_at) = cube_words(c);
        let mut u = U { i: [c.shaper.is_some() as i32, c.shaper.as_ref().map_or(0, |s| s.size as i32), c.size as i32, c.three_d as i32], j: [shaper_at as i32, main_at as i32, 0, 0], ..Default::default() };
        if let Some(s) = &c.shaper {
            u.f[0] = [s.min[0], s.min[1], s.min[2], 0.0];
            u.f[1] = span(s);
        }
        u.f[2] = [c.min[0], c.min[1], c.min[2], 0.0];
        u.f[3] = span(c);
        let pipes = &self.g.fx;
        self.pass(&pipes.cube, t, None, &u.words(), &words)
    }

    /// `vfx::box3_rows` along one axis: three sliding-window passes at radius `r`.
    fn box3(&mut self, t: &Tex, r: usize, along_x: bool) -> Tex {
        let u = U { i: [r as i32, (!along_x) as i32, 0, 0], ..Default::default() };
        let a = self.box_pass(t, &u);
        let b = self.box_pass(&a, &u);
        self.give(a);
        let c = self.box_pass(&b, &u);
        self.give(b);
        c
    }

    /// `vfx::blur`: three box passes per axis; None when both radii are 0 (nothing to do).
    pub(super) fn blur(&mut self, t: &Tex, rx: usize, ry: usize) -> Option<Tex> {
        let mut cur: Option<Tex> = None;
        for (r, along_x) in [(rx, true), (ry, false)] {
            if r == 0 {
                continue;
            }
            let next = self.box3(cur.as_ref().unwrap_or(t), r, along_x);
            if let Some(old) = cur.replace(next) {
                self.give(old);
            }
        }
        cur
    }

    /// `vfx::soft_blur`: one under a pixel blurs 1 px and mixes back to the source by that share.
    fn soft_blur(&mut self, t: &Tex, fx: f32, fy: f32) -> Tex {
        let mut cur: Option<Tex> = None;
        for (f, along_x) in [(fx, true), (fy, false)] {
            if f <= 0.0 {
                continue;
            }
            let r = (f.round() as usize).max(1);
            let input = cur.as_ref().unwrap_or(t);
            let blurred = self.box3(input, r, along_x);
            let next = if f < 1.0 {
                let mixed = self.lerp(input, &blurred, f);
                self.give(blurred);
                mixed
            } else {
                blurred
            };
            if let Some(old) = cur.replace(next) {
                self.give(old);
            }
        }
        cur.unwrap_or_else(|| self.copy(t))
    }

    /// `transition::mix` of `a` into `b` at `p` (both the frame's size, 0 < p < 1), consuming both.
    pub(super) fn transition(&mut self, a: Tex, b: Tex, e: &FxEval, p: f32) -> Tex {
        let (wf, hf) = (a.w as f32, a.h as f32);
        let (cx, cy) = (wf / 2.0, hf / 2.0);
        let feather = e.f("feather");
        let offset = |d: (f32, f32), t: f32| ((d.0 * t * wf).round() as i32, (d.1 * t * hf).round() as i32);
        let band = [p, feather, p * (1.0 + feather) - feather, 0.0];
        let mut u = U { f: [band, [cx, cy, 0.0, 0.0], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4]], ..Default::default() };
        let pipes = &self.g.fx;
        let out = match port(e).expect("a ported transition") {
            GpuPort::Dissolve if e.on("additive") => {
                u.i[0] = 1;
                u.f[0] = [p, (2.0 - 2.0 * p).min(1.0), (2.0 * p).min(1.0), 0.0];
                self.pass(&pipes.transition, &a, Some(&b), &u.words(), &[])
            }
            GpuPort::Dissolve => self.pass(&pipes.transition, &a, Some(&b), &u.words(), &[]),
            GpuPort::FadeToColor => {
                // Out to the colour over the first half, in from it over the second.
                let (side, t) = if p < 0.5 { (&a, p * 2.0) } else { (&b, 2.0 - 2.0 * p) };
                u.i[0] = if e.on("additive") { 3 } else { 2 };
                u.f[0][3] = t;
                u = u.colour(vfx::premul(e.c("color")));
                self.pass(&pipes.transition, side, None, &u.words(), &[])
            }
            GpuPort::LinearWipe => {
                let (s, c) = e.f("angle").to_radians().sin_cos();
                let ext = (c.abs() * wf + s.abs() * hf).max(1.0);
                u.i[0] = 4;
                u.f[2] = [c, s, ext, 0.0];
                self.pass(&pipes.transition, &a, Some(&b), &u.words(), &[])
            }
            GpuPort::Iris => {
                u.i = [5, e.i("shape") as i32, 0, 0];
                u.f[2] = [(cx * cx + cy * cy).sqrt(), 0.0, 0.0, 0.0];
                self.pass(&pipes.transition, &a, Some(&b), &u.words(), &[])
            }
            GpuPort::ClockWipe => {
                u.i[0] = 6;
                self.pass(&pipes.transition, &a, Some(&b), &u.words(), &[])
            }
            GpuPort::Push => {
                let d = direction(e.i("direction"));
                let (oa, ob) = (offset(d, p), offset(d, p - 1.0));
                u.i[0] = 7;
                u.j = [oa.0, oa.1, ob.0, ob.1];
                self.pass(&pipes.transition, &a, Some(&b), &u.words(), &[])
            }
            GpuPort::Slide => {
                let ob = offset(direction(e.i("direction")), p - 1.0);
                u.i[0] = 7;
                u.j = [0, 0, ob.0, ob.1];
                self.pass(&pipes.transition, &a, Some(&b), &u.words(), &[])
            }
            GpuPort::Zoom => {
                let amt = e.f("amount") as f64;
                let za = self.zoomed(&a, 1.0 + p as f64 * amt);
                let zb = self.zoomed(&b, 1.0 + (1.0 - p as f64) * amt);
                let out = self.pass(&pipes.transition, &za, Some(&zb), &u.words(), &[]);
                self.give(za);
                self.give(zb);
                out
            }
            GpuPort::Mvfx => self.mv_transition(&a, &b, e, p),
            port => unreachable!("{port:?} is not a transition"),
        };
        self.give(a);
        self.give(b);
        out
    }

    /// `transition::zoomed`: `t` scaled by `s` about the frame centre.
    fn zoomed(&mut self, t: &Tex, s: f64) -> Tex {
        let (cx, cy) = (t.w as f64 / 2.0, t.h as f64 / 2.0);
        let blank = self.clear(t.w, t.h, Color32::TRANSPARENT);
        self.draw_with(blank, t, Affine::translate(-cx, -cy).then(Affine::scale(s, s)).then(Affine::translate(cx, cy)), 1.0, true)
    }

    /// `generate::render` for a ported generator, `w` x `h` at its time `t`.
    pub(super) fn generate(&mut self, e: &FxEval, w: u32, h: u32, t: f64) -> Tex {
        let (w, h) = (w.max(1), h.max(1));
        let hf = h as f32;
        let blank = self.clear(w, h, Color32::TRANSPARENT);
        let mut u = U::default();
        let mut data: Vec<u32> = Vec::new();
        match port(e).expect("a ported generator") {
            GpuPort::Solid => u = u.colour(vfx::premul(e.c("color"))),
            GpuPort::Gradient => {
                u.i = [1, e.i("type") as i32, 0, 0];
                data = generate::gradient_lut(e).iter().map(|c| u32::from_le_bytes(c.to_array())).collect();
                // Pixel centres: the first and last pixels sit exactly on the end stops.
                let (cx, cy) = ((w - 1) as f32 / 2.0, (h - 1) as f32 / 2.0);
                let (sn, cs) = e.f("angle").to_radians().sin_cos();
                let proj = |x: f32, y: f32| x * cs + y * sn;
                let ends = [(0.0, 0.0), (2.0 * cx, 0.0), (0.0, 2.0 * cy), (2.0 * cx, 2.0 * cy)].map(|(x, y)| proj(x, y));
                let (lo, hi) = (ends.iter().copied().fold(f32::INFINITY, f32::min), ends.iter().copied().fold(f32::NEG_INFINITY, f32::max));
                u.f[0] = [cs, sn, lo, (hi - lo).max(1e-6)];
                u.f[1] = [cx, cy, cx.hypot(cy).max(1e-6), cx.max(1e-6)];
                u.f[2] = [cy.max(1e-6), 0.0, 0.0, 0.0];
            }
            GpuPort::Checkerboard => {
                let cell = (e.f("size") * hf).round().max(1.0) as i32;
                let (a, b) = (vfx::premul(e.c("color1")), vfx::premul(e.c("color2")));
                u = u.colour(a);
                u.i = [2, cell, b.a() as i32, 0];
                u.j = [b.r() as i32, b.g() as i32, b.b() as i32, a.a() as i32];
            }
            GpuPort::Noise => {
                let cell = (e.f("scale") * hf).max(1.0);
                let oct = e.f("octaves").round().clamp(1.0, 8.0) as i32;
                let seed = e.f("seed").round() as u32;
                let z = (t * e.f("speed") as f64) as f32;
                u.i = [3, oct, seed as i32, 0];
                u.f[0] = [cell, z, 0.0, 0.0];
                u.f[1] = e.c("color1");
                u.f[2] = e.c("color2");
            }
            port => unreachable!("{port:?} is not a generator"),
        }
        let pipes = &self.g.fx;
        let out = self.pass(&pipes.generate, &blank, None, &u.words(), &data);
        self.give(blank);
        out
    }
}

/// `transition::direction`: the direction of travel for push and slide (`plugins::DIRECTIONS`).
fn direction(i: usize) -> (f32, f32) {
    [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0), (-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)][i.min(7)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::tests::{Rng, check_within, ev, gpu_or_skip};
    use crate::engine::composite::tests::{layer, media};
    use crate::engine::plan::{FramePlan, Item, MediaRef, SrcAt, TrackPlan};
    use crate::engine::plugins::{ParamKind, PluginInfo, PluginKind, catalog, eval, new_fx};
    use crate::project::{Composite, Curve, Keyframe, Keyframes, Motion};
    use std::sync::Arc;

    /// Parameter set `k` (0..3) of `info`: its defaults, then every parameter a quarter and four fifths of the way
    /// through its range (choices: the second and last option, flags on), colours and texts of the set's own.
    fn param_set(info: &'static PluginInfo, k: usize, lut: &str) -> FxEval {
        let mut fx = new_fx(info, None);
        let key = |v: f32| Keyframes { keys: vec![Keyframe { t: 0.0, v, curve: Curve::Linear }] };
        if k > 0 {
            let at = if k == 1 { 0.25 } else { 0.8 };
            for p in info.params {
                match p.kind {
                    ParamKind::Float { min, max, .. } => {
                        fx.params.insert(p.key.into(), key(min + (max - min) * at));
                    }
                    ParamKind::Choice { options, .. } => {
                        fx.params.insert(p.key.into(), key(if k == 1 { 1.0 } else { options.len() as f32 - 1.0 }));
                    }
                    ParamKind::Bool { .. } => {
                        fx.params.insert(p.key.into(), key(1.0));
                    }
                    ParamKind::Color { .. } => {
                        let c = if k == 1 { [0.2, 0.7, 0.3, 1.0] } else { [0.9, 0.1, 0.5, 0.5] };
                        for (i, ch) in ["r", "g", "b", "a"].iter().enumerate() {
                            fx.params.insert(format!("{}.{ch}", p.key), key(c[i]));
                        }
                    }
                    ParamKind::Text { .. } | ParamKind::Font { .. } => {
                        let v = match p.key {
                            "stops" => if k == 1 { "0:#ff7a00ff;0.5:#00ff00ff;1:#401060ff" } else { "0.2:#000000ff;0.8:#ffffff80" },
                            "input_lut" | "look_lut" => if k == 1 { lut } else { "" },
                            k2 if k2.starts_with("curve") => if k == 1 { "0:0;0.3:0.5;1:1" } else { "0:0.1;0.5:0.4;0.7:0.9;1:0.8" },
                            _ => "",
                        };
                        fx.text.insert(p.key.into(), v.into());
                    }
                }
            }
        }
        eval(&fx, 0.0).expect("a known plug-in")
    }

    /// A .cube with a 1D shaper and a 3D table that bends every channel, for the Color Grading sets.
    fn test_lut() -> String {
        let dir = std::env::temp_dir().join("milan-agent").join("r21");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("parity.cube");
        let mut s = String::from("TITLE \"parity\"\nLUT_1D_SIZE 3\nLUT_3D_SIZE 5\n0 0 0\n0.6 0.4 0.5\n1 1 1\n");
        for b in 0..5 {
            for g in 0..5 {
                for r in 0..5 {
                    let f = |v: usize, s: f32| ((v as f32 / 4.0).powf(s)).min(1.0);
                    s += &format!("{:.5} {:.5} {:.5}\n", f(r, 0.8) * 0.9 + 0.05 * f(g, 1.0), f(g, 1.3), (f(b, 1.0) + 0.1 * f(r, 2.0)).min(1.0));
                }
            }
        }
        std::fs::write(&path, s).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn frame(layers: Vec<TrackPlan>) -> FramePlan {
        FramePlan { alpha: false, fade: None, samples: Vec::new(), out_fx: Vec::new(), layers }
    }

    /// R21b acceptance: every video FX with a port, at three parameter sets, as an event FX on a picture-in-picture
    /// with alpha over a background (the decoded picture's own size), within 2 in 255 of the CPU (a blur within 3).
    /// Invert and Gaussian Blur also as track FX, Adjustment Event FX and Video Output FX (the other three places
    /// an FX runs).
    #[test]
    fn gpu_video_fx_match_cpu() {
        let Some(g) = gpu_or_skip("gpu_video_fx_match_cpu") else { return };
        let lut = test_lut();
        let mut r = Rng(11);
        let pip = Motion { w: 0.7, h: 0.7, x: 0.05, rot: 5.0, ..Default::default() };
        let mut n = 0;
        for info in catalog(PluginKind::VideoFx).filter(|i| i.gpu.is_some()) {
            let within = if matches!(info.gpu, Some(GpuPort::Blur)) { 3 } else { crate::engine::mvfx::tolerance(info.id) };
            for k in 0..3 {
                let e = param_set(info, k, &lut);
                let mut top = ev(&mut r, "a1".into());
                top.fx = vec![e.clone()];
                let bg = layer(vec![Item::Event(ev(&mut r, "m0".into()))], Composite::SourceAlpha, 1.0, Motion::default());
                let fp = frame(vec![bg, layer(vec![Item::Event(top)], Composite::SourceAlpha, 0.9, pip)]);
                let (d, _) = check_within(&g, &fp, [320, 180], true, within, &format!("{} set {k} as an event FX", info.id));
                n += 1;
                println!("{} set {k}: largest difference {d}", info.id);
            }
        }
        for id in ["milan.fx.invert", "milan.fx.gaussian_blur"] {
            let info = find(id).unwrap();
            let within = if id.contains("blur") { 3 } else { 2 };
            let e = param_set(info, 2, &lut);
            let bg = |r: &mut Rng| layer(vec![Item::Event(ev(r, "m0".into()))], Composite::SourceAlpha, 1.0, Motion::default());
            let mut l = layer(vec![Item::Event(ev(&mut r, "a1".into()))], Composite::Screen, 0.8, pip);
            l.fx = vec![e.clone()];
            check_within(&g, &frame(vec![bg(&mut r), l]), [320, 180], true, within, &format!("{id} as a track FX"));
            let mut l = layer(vec![Item::Event(ev(&mut r, "a1".into()))], Composite::SourceAlpha, 1.0, pip);
            l.adjust = vec![(vec![e.clone()], 0.6)];
            check_within(&g, &frame(vec![bg(&mut r), l]), [320, 180], true, within, &format!("{id} as an adjustment"));
            let mut fp = frame(vec![bg(&mut r), layer(vec![Item::Event(ev(&mut r, "a1".into()))], Composite::SourceAlpha, 1.0, pip)]);
            (fp.out_fx, fp.fade) = (vec![e], Some(([0, 128, 255], 0.3)));
            check_within(&g, &fp, [320, 180], true, within, &format!("{id} as an output FX"));
        }
        println!("gpu_video_fx_match_cpu on {}: {n} plug-in parameter sets", g.name);
    }

    /// R21b acceptance: every transition with a port, at three parameter sets and progresses, between two events
    /// (one with alpha) inside a picture-in-picture, within 2 in 255 of the CPU.
    #[test]
    fn gpu_transitions_match_cpu() {
        let Some(g) = gpu_or_skip("gpu_transitions_match_cpu") else { return };
        let mut r = Rng(12);
        let pip = Motion { w: 0.8, h: 0.8, y: 0.05, ..Default::default() };
        for info in catalog(PluginKind::Transition).filter(|i| i.gpu.is_some()) {
            for (k, p) in [(0, 0.3), (1, 0.5), (2, 0.75)] {
                let e = param_set(info, k, "");
                let mix = Item::Mix { a: Some(ev(&mut r, "m2".into())), b: Some(ev(&mut r, "a3".into())), p, kind: Some(e) };
                let bg = layer(vec![Item::Event(ev(&mut r, "m0".into()))], Composite::SourceAlpha, 1.0, Motion::default());
                let fp = frame(vec![bg, layer(vec![mix], Composite::SourceAlpha, 1.0, pip)]);
                let (d, _) = check_within(&g, &fp, [320, 180], true, crate::engine::mvfx::tolerance(info.id), &format!("{} set {k} at {p}", info.id));
                println!("{} set {k} at {p}: largest difference {d}", info.id);
            }
        }
    }

    /// R21b acceptance: the generators with a port (solid, gradient, checkerboard, noise), at three parameter sets,
    /// as an event's source, within 2 in 255 of the CPU's `generate::render`; the animated noise at a later time too.
    #[test]
    fn gpu_generators_match_cpu() {
        let Some(g) = gpu_or_skip("gpu_generators_match_cpu") else { return };
        let mut r = Rng(13);
        for info in catalog(PluginKind::Generator).filter(|i| i.gpu.is_some()) {
            for k in 0..3 {
                let e = param_set(info, k, "");
                for t in [0.0, 1.7] {
                    let mut top = ev(&mut r, "m1".into());
                    top.src = SrcAt { m: Arc::new(MediaRef { id: "gen".into(), ..(*media(640, 360)).clone() }), sf: None, generator: Some((e.clone(), t)), nested: None };
                    let bg = layer(vec![Item::Event(ev(&mut r, "m0".into()))], Composite::SourceAlpha, 1.0, Motion::default());
                    let fp = frame(vec![bg, layer(vec![Item::Event(top)], Composite::SourceAlpha, 1.0, Motion { w: 0.9, h: 0.9, ..Default::default() })]);
                    let (d, _) = check_within(&g, &fp, [320, 180], true, 2, &format!("{} set {k} at {t} s", info.id));
                    println!("{} set {k} at {t} s: largest difference {d}", info.id);
                }
            }
        }
    }
}
