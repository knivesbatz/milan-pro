//! Timeline engine: one plan of the project for preview, playback and render (docs/FEATURE_PLAN.md section 1).
//! Batch 1 lands only the pure keyframe maths; the plan, compositor and mixer follow from batch 2.

// shortcut: nothing outside the tests calls keyframes yet; drop this allow when batch 2's plan compiler uses it.
#[allow(dead_code)]
pub mod keyframes;
