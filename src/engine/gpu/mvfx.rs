//! MVFX on the GPU: the recipes of `engine/mvfx` run here step by step, each kernel as the fragment pass `fs_<name>`
//! of its category's WGSL file (with math.wgsl and mvfx_common.wgsl in front), blurs and draws through the
//! compositor's own passes. The CPU kernels are the reference; `gpu_mvfx_*` tests hold the two together.
use std::collections::HashMap;
use std::sync::Arc;
use eframe::egui::{Color32, ColorImage};
use super::{Frame, Tex};
use crate::engine::composite::Sources;
use crate::engine::mvfx::{self, Src, Step};
use crate::engine::plan::EventPlan;
use crate::engine::plugins::FxEval;

/// The WGSL file of each kernel category, with its kernels.
fn files() -> [(&'static str, &'static [(&'static str, mvfx::Kernel)]); 4] {
    [
        (include_str!("mvfx_looks.wgsl"), mvfx::looks::KERNELS),
        (include_str!("mvfx_optics.wgsl"), mvfx::optics::KERNELS),
        (include_str!("mvfx_temporal.wgsl"), mvfx::temporal::KERNELS),
        (include_str!("mvfx_trans.wgsl"), mvfx::trans::KERNELS),
    ]
}

pub(super) struct Pipes(HashMap<&'static str, wgpu::RenderPipeline>);

impl Pipes {
    pub fn new(device: &wgpu::Device, math: &str, render_to: &dyn Fn(&wgpu::ShaderModule, &str, wgpu::TextureFormat) -> wgpu::RenderPipeline) -> Pipes {
        let mut map = HashMap::new();
        for (i, (src, kernels)) in files().into_iter().enumerate() {
            if kernels.is_empty() {
                continue;
            }
            let code = format!("{math}\n{}\n{src}", include_str!("mvfx_common.wgsl"));
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(["mvfx_looks", "mvfx_optics", "mvfx_temporal", "mvfx_trans"][i]), source: wgpu::ShaderSource::Wgsl(code.into()) });
            for (name, _) in kernels {
                map.insert(*name, render_to(&module, &format!("fs_{name}"), super::FORMAT));
            }
        }
        Pipes(map)
    }
}

impl Frame<'_> {
    /// `mvfx::run` on the GPU: the recipe's steps over `inputs` (a new texture; the inputs are left as they were).
    pub(super) fn mv_run(&mut self, steps: &[Step], inputs: &[&Tex]) -> Tex {
        fn pick<'x>(inputs: &[&'x Tex], done: &'x [Tex], s: Src) -> &'x Tex {
            match s {
                Src::In(n) => inputs[n],
                Src::Step(n) => &done[n],
            }
        }
        let mut done: Vec<Tex> = Vec::with_capacity(steps.len());
        for s in steps {
            let out = match s {
                Step::K { entry, m, data, a, b } => {
                    let g = self.g;
                    let pipe = &g.mv.0[entry];
                    // An empty table still binds a buffer.
                    let words = if data.is_empty() { vec![0u32] } else { data.clone() };
                    self.pass(pipe, pick(inputs, &done, *a), Some(pick(inputs, &done, *b)), &m.words(), &words)
                }
                Step::Blur { of, rx, ry } => {
                    let t = pick(inputs, &done, *of);
                    match self.blur(t, *rx, *ry) {
                        Some(b) => b,
                        None => self.copy(t),
                    }
                }
                Step::Draw { of, m, bilinear } => {
                    let t = pick(inputs, &done, *of);
                    let blank = self.clear(t.w, t.h, Color32::TRANSPARENT);
                    self.draw_with(blank, t, *m, 1.0, *bilinear)
                }
            };
            done.push(out);
        }
        let last = done.pop();
        for t in done {
            self.give(t);
        }
        last.unwrap_or_else(|| self.copy(inputs[0]))
    }

    /// An MVFX video FX on `t` (a new texture).
    pub(super) fn mvfx(&mut self, t: &Tex, e: &FxEval) -> Tex {
        match mvfx::recipe(e, t.w as usize, t.h as usize) {
            Some(steps) => self.mv_run(&steps, &[t]),
            None => self.copy(t),
        }
    }

    /// An MVFX transition of `a` into `b` at 0 < p < 1 (a new texture).
    pub(super) fn mv_transition(&mut self, a: &Tex, b: &Tex, e: &FxEval, p: f32) -> Tex {
        match mvfx::recipe_tr(e, a.w as usize, a.h as usize, p) {
            Some(steps) => self.mv_run(&steps, &[a, b]),
            None => self.lerp(a, b, p),
        }
    }

    /// `mvfx::event_fx` on the GPU: event `e`'s FX on `t` (its source at `size`), the ones that read earlier frames
    /// with those frames fetched, uploaded and taken through the FX before them. Err: the device cannot hold them.
    pub(super) fn mv_event(&mut self, t: Tex, e: &EventPlan, src: &mut dyn Sources, size: (u32, u32)) -> Result<Tex, ()> {
        let mut cur = t;
        for (i, f) in e.fx.iter().enumerate() {
            let next = match mvfx::earlier(e, f) {
                Some(offsets) => {
                    let raw: Vec<Option<Arc<ColorImage>>> = std::iter::once(0).chain(offsets).map(|o| mvfx::raw_frame(e, o, src, size)).collect();
                    let mut frames: Vec<Option<Tex>> = Vec::new();
                    for r in &raw[1..] {
                        frames.push(match r {
                            Some(img) => Some(self.earlier_tex(img, e, &e.fx[..i])?),
                            None => None,
                        });
                    }
                    let raws: Vec<Option<&ColorImage>> = raw.iter().map(|r| r.as_deref()).collect();
                    let steps = mvfx::temporal::recipe_with(f, cur.w as usize, cur.h as usize, &raws);
                    let mut inputs: Vec<&Tex> = vec![&cur];
                    inputs.extend(frames.iter().map(|f| f.as_ref().unwrap_or(&cur)));
                    let out = self.mv_run(&steps, &inputs);
                    for f in frames.into_iter().flatten() {
                        self.give(f);
                    }
                    out
                }
                None => self.fx(&cur, f),
            };
            self.give(std::mem::replace(&mut cur, next));
        }
        Ok(cur)
    }

    /// A decoded earlier frame through the flicker filter (when the event has it) and `before`, on the GPU.
    fn earlier_tex(&mut self, img: &ColorImage, e: &EventPlan, before: &[FxEval]) -> Result<Tex, ()> {
        let mut t = self.upload(img).ok_or(())?;
        if e.flicker && t.h >= 2 {
            let f = self.flicker(&t);
            self.give(std::mem::replace(&mut t, f));
        }
        if let Some(out) = self.fx_chain(&t, before) {
            self.give(std::mem::replace(&mut t, out));
        }
        Ok(t)
    }
}


#[cfg(test)]
mod tests {
    /// Every MVFX WGSL file compiles and has an entry point for each of its kernels (the error, when one does not).
    #[test]
    fn mvfx_shaders_compile() {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor { backends: wgpu::Backends::VULKAN, ..Default::default() });
        let Some(adapter) = instance.enumerate_adapters(wgpu::Backends::VULKAN).into_iter().next() else {
            println!("SKIPPED mvfx_shaders_compile: no Vulkan adapter");
            return;
        };
        let (device, _queue) = super::super::block_on(adapter.request_device(&Default::default(), None)).expect("a device");
        for (src, kernels) in super::files() {
            device.push_error_scope(wgpu::ErrorFilter::Validation);
            let code = format!("{}
{}
{src}", include_str!("math.wgsl"), include_str!("mvfx_common.wgsl"));
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(code.into()) });
            if let Some(e) = super::super::block_on(device.pop_error_scope()) {
                panic!("{e}");
            }
            let info = super::super::block_on(module.get_compilation_info());
            for (name, _) in kernels {
                assert!(src.contains(&format!("fn fs_{name}(")), "no fs_{name}");
            }
            assert!(info.messages.iter().all(|m| m.message_type != wgpu::CompilationMessageType::Error), "{:?}", info.messages.iter().map(|m| &m.message).collect::<Vec<_>>());
        }
    }
}
