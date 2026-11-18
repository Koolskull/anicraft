//! Pixel paint for one cropped cel (layer L1).
//!
//! A dab is a round stamp: a hard edge, a fringe, or a soft falloff, painted
//! source-over or erased. Pixel centres sit on half-integers, and bytes use
//! round-half-to-even, so a stamp matches VMI Studio's dab. This crate has no
//! document, no window, and no unsafe code.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod blend;
mod cel;
mod clip;
mod dab;
mod mark;
mod smudge;
mod stroke;
mod tips;

pub use blend::{STRETCH_CAP, Smear, blend_smudge};
pub use cel::{Cel, MAX_BYTES, MAX_EDGE};
pub use clip::clip_alpha;
pub use dab::{Antialias, HAIRLINE_DIAMETER, Tip, cover_dab, dab, flow_dab, paint_cover};
pub use mark::{Grain, Stamp, stamp_mark, stroke_aim};
pub use smudge::smudge;
pub use stroke::{Sample, Stroke, dab_opacity, dab_spacing, pen_spacing, sample_stroke, stamp_stroke, stroke_steps};
pub use tips::{Picture, dab_turn, load_pictures, picture_spacing, pictures_loaded, stamp_picture};

#[cfg(test)]
mod tests;
