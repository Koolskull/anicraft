//! Blend colours already in a buffer. A smudge never introduces a new hue.
//!
//! Softness 0 is a hard disc and a short pull along the stroke. Softness 1
//! fades at the rim and pulls from further back, so the same colours mix.

use crate::dab::{ceil_i32, floor_i32, rint_byte};
use crate::stroke::{Sample, stroke_steps};

/// How far a smudge mixes. Strength 0 leaves the buffer alone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Smear {
    /// Brush radius in pixels. Below 0.5 the smudge does nothing.
    pub radius: f32,
    /// 0 is a hard edge. 1 fades from the centre and pulls further.
    pub softness: f32,
    /// 0 leaves pixels alone. 1 replaces covered pixels with the colour behind them.
    pub strength: f32,
}

/// Mix used when the Raster tool's Stretch slider is at 1.
///
/// Strength 1 replaces every dab, so the colour rides the whole stroke. That is
/// past a useful maximum: the slider tops out here and the paint fades along the drag.
pub const STRETCH_CAP: f32 = 0.32;

/// Blend along the segment. One sample (`x0 == x1` and `y0 == y1`) mixes the disc in place.
#[allow(clippy::too_many_arguments)]
pub fn blend_smudge(pixels: &mut [u8], width: u32, height: u32, x0: f32, y0: f32, x1: f32, y1: f32, smear: Smear) -> bool {
    if !smear.strength.is_finite() || smear.strength <= 0.0 {
        return false;
    }
    let radius = smear.radius as f64;
    if !radius.is_finite() || radius < 0.5 || [x0, y0, x1, y1].iter().any(|v| !v.is_finite()) {
        return false;
    }
    let softness = if smear.softness.is_finite() { (smear.softness as f64).clamp(0.0, 1.0) } else { 0.0 };
    let strength = (smear.strength as f64).clamp(0.0, 1.0);
    let spacing = (radius * 0.35).max(1.0);
    let mut path = vec![Sample { x: x0, y: y0, pressure: 1.0 }];
    path.extend(stroke_steps(x0, y0, 1.0, x1, y1, 1.0, spacing as f32));
    let at_end = path.last().is_some_and(|step| (step.x - x1).abs() <= 0.01 && (step.y - y1).abs() <= 0.01);
    if !at_end {
        path.push(Sample { x: x1, y: y1, pressure: 1.0 });
    }
    let brush = Brush { radius, softness, strength };
    let mut hit = false;
    if path.len() < 2 {
        if let Some(only) = path.first().copied() {
            hit |= smear_at(pixels, width, height, only, only, brush);
        }
        return hit;
    }
    for pair in path.windows(2) {
        let (Some(from), Some(to)) = (pair.first().copied(), pair.get(1).copied()) else { continue };
        hit |= smear_at(pixels, width, height, from, to, brush);
    }
    hit
}

#[derive(Clone, Copy)]
struct Brush {
    radius: f64,
    softness: f64,
    strength: f64,
}

fn smear_at(pixels: &mut [u8], width: u32, height: u32, from: Sample, to: Sample, brush: Brush) -> bool {
    let Brush { radius, softness, strength } = brush;
    let (cx, cy) = (to.x as f64, to.y as f64);
    let edge = radius.max(0.5);
    let (dx, dy) = (cx - from.x as f64, cy - from.y as f64);
    let len = dx.hypot(dy);
    let still = len <= 1e-3;
    // Softness 0 picks up the previous dab, same centre, so the colour travels whole.
    // A shifted disc would include a crescent of empty pixels and the stroke would fade.
    // Softness 1 reaches a step further back and mixes what the brush has already crossed.
    let back = if still { 0.0 } else { len * (1.0 + softness) };
    let (ux, uy) = if still { (0.0, 0.0) } else { (dx / len, dy / len) };
    let (sx, sy) = (cx - ux * back, cy - uy * back);
    let pad = back + edge + 1.0;
    let Some(x0) = floor_i32(cx - pad) else { return false };
    let Some(y0) = floor_i32(cy - pad) else { return false };
    let Some(x1) = ceil_i32(cx + pad).and_then(|v| v.checked_add(1)) else { return false };
    let Some(y1) = ceil_i32(cy + pad).and_then(|v| v.checked_add(1)) else { return false };
    let Some(width_i) = i32::try_from(width).ok() else { return false };
    let Some(height_i) = i32::try_from(height).ok() else { return false };
    let x0 = x0.max(0);
    let y0 = y0.max(0);
    let x1 = x1.min(width_i);
    let y1 = y1.min(height_i);
    if x1 <= x0 || y1 <= y0 {
        return false;
    }
    let ww = (x1 - x0) as usize;
    let hh = (y1 - y0) as usize;
    let Some(bytes) = ww.checked_mul(hh).and_then(|n| n.checked_mul(4)) else { return false };
    let mut copy = vec![0u8; bytes];
    for row in 0..hh {
        let src = ((y0 as usize + row) * width as usize + x0 as usize) * 4;
        let dst = row * ww * 4;
        if let (Some(from_px), Some(to_px)) = (pixels.get(src..src + ww * 4), copy.get_mut(dst..dst + ww * 4)) {
            to_px.copy_from_slice(from_px);
        }
    }
    let src = disc_average(&copy, ww, hh, x0, y0, sx, sy, edge, softness);
    let mix_scale = if still { 0.35 + 0.65 * softness } else { 1.0 };
    let Some(bx0) = floor_i32(cx - edge) else { return false };
    let Some(by0) = floor_i32(cy - edge) else { return false };
    let Some(bx1) = ceil_i32(cx + edge).and_then(|v| v.checked_add(1)) else { return false };
    let Some(by1) = ceil_i32(cy + edge).and_then(|v| v.checked_add(1)) else { return false };
    let mut hit = false;
    for y in by0.max(y0)..by1.min(y1) {
        for x in bx0.max(x0)..bx1.min(x1) {
            let dist = ((x as f64 + 0.5) - cx).hypot((y as f64 + 0.5) - cy);
            let cover = brush_cover(dist, edge, softness);
            if cover <= 0.0 {
                continue;
            }
            let mix = cover * strength * mix_scale;
            let o = ((y as usize) * (width as usize) + (x as usize)) * 4;
            let Some(px) = pixels.get_mut(o..o + 4) else { continue };
            let next = mix_premul(px, src, mix);
            if next.as_slice() != px {
                px.copy_from_slice(&next);
                hit = true;
            }
        }
    }
    hit
}

fn brush_cover(dist: f64, edge: f64, softness: f64) -> f64 {
    if softness <= 0.0 {
        if dist <= edge { 1.0 } else { 0.0 }
    } else {
        let inner = edge * (1.0 - softness);
        ((edge - dist) / (edge - inner).max(0.001)).clamp(0.0, 1.0)
    }
}

#[allow(clippy::too_many_arguments)]
fn disc_average(copy: &[u8], ww: usize, hh: usize, origin_x: i32, origin_y: i32, cx: f64, cy: f64, edge: f64, softness: f64) -> [f64; 4] {
    let mut red = 0.0;
    let mut green = 0.0;
    let mut blue = 0.0;
    let mut alpha = 0.0;
    let mut weight = 0.0;
    for y in 0..hh {
        for x in 0..ww {
            let px = origin_x as f64 + x as f64 + 0.5;
            let py = origin_y as f64 + y as f64 + 0.5;
            let cover = brush_cover((px - cx).hypot(py - cy), edge, softness);
            if cover <= 0.0 {
                continue;
            }
            let Some(sample) = copy.get((y * ww + x) * 4..(y * ww + x) * 4 + 4) else { continue };
            let a = sample[3] as f64 / 255.0 * cover;
            red += sample[0] as f64 / 255.0 * a;
            green += sample[1] as f64 / 255.0 * a;
            blue += sample[2] as f64 / 255.0 * a;
            alpha += a;
            weight += cover;
        }
    }
    if alpha <= 0.0 || weight <= 0.0 {
        [0.0; 4]
    } else {
        [red / alpha, green / alpha, blue / alpha, alpha / weight]
    }
}

fn premul(c: [f64; 4]) -> [f64; 4] {
    [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]]
}

fn unpremul(c: [f64; 4]) -> [f64; 4] {
    if c[3] <= 1e-6 { [0.0; 4] } else { [c[0] / c[3], c[1] / c[3], c[2] / c[3], c[3]] }
}

fn mix_premul(dst: &[u8], src: [f64; 4], t: f64) -> [u8; 4] {
    let t = t.clamp(0.0, 1.0);
    let d = [
        dst.first().copied().unwrap_or(0) as f64 / 255.0,
        dst.get(1).copied().unwrap_or(0) as f64 / 255.0,
        dst.get(2).copied().unwrap_or(0) as f64 / 255.0,
        dst.get(3).copied().unwrap_or(0) as f64 / 255.0,
    ];
    let mixed = {
        let dp = premul(d);
        let sp = premul(src);
        let mut out = [0.0; 4];
        for i in 0..4 {
            out[i] = dp[i] + (sp[i] - dp[i]) * t;
        }
        unpremul(out)
    };
    [rint_byte(mixed[0] * 255.0), rint_byte(mixed[1] * 255.0), rint_byte(mixed[2] * 255.0), rint_byte(mixed[3] * 255.0)]
}
