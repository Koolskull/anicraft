//! Carry the colour under one disc along a move. The disc does not invent a hue.

use crate::cel::Cel;
use crate::dab::{self, Antialias, Tip, rint_byte};
use crate::stroke::{Sample, stroke_steps};

/// Pick up the disc at `(x0, y0)` and lay that colour down toward `(x1, y1)`.
///
/// Strength 0 leaves the cel alone. Strength 1 paints the picked colour at full cover.
/// The colour is the alpha-weighted straight RGB of the ink inside the disc, so a red
/// pixel stays red.
pub fn smudge(cel: &mut Cel, x0: f32, y0: f32, x1: f32, y1: f32, radius: f32, strength: f32) -> bool {
    if !strength.is_finite() || strength <= 0.0 || !radius.is_finite() || radius < 0.5 {
        return false;
    }
    let strength = strength.min(1.0);
    let Some(color) = pickup(cel, x0, y0, radius) else {
        return false;
    };
    let spacing = ((radius as f64) * 0.5).max(0.5) as f32;
    let mut steps = stroke_steps(x0, y0, 1.0, x1, y1, 1.0, spacing);
    let needs_end = steps.last().is_none_or(|step| (step.x - x1).abs() > 0.01 || (step.y - y1).abs() > 0.01);
    if needs_end {
        steps.push(Sample { x: x1, y: y1, pressure: 1.0 });
    }
    let tip = Tip { radius, softness: 0.0, antialias: Antialias::Off, color, opacity: strength, erase: false };
    let mut hit = false;
    for step in steps {
        hit |= cel.stamp(step.x, step.y, tip);
    }
    hit
}

fn pickup(cel: &Cel, cx: f32, cy: f32, radius: f32) -> Option<[u8; 4]> {
    let cx = cx as f64;
    let cy = cy as f64;
    let radius = radius as f64;
    if !cx.is_finite() || !cy.is_finite() || !radius.is_finite() || radius < 0.5 {
        return None;
    }
    let edge = radius.max(0.5);
    let x0 = dab::floor_i32(cx - edge)?;
    let y0 = dab::floor_i32(cy - edge)?;
    let x1 = dab::ceil_i32(cx + edge)?.checked_add(1)?;
    let y1 = dab::ceil_i32(cy + edge)?.checked_add(1)?;
    let mut sum_r = 0.0;
    let mut sum_g = 0.0;
    let mut sum_b = 0.0;
    let mut sum_a = 0.0;
    let mut count = 0.0;
    for y in y0..y1 {
        for x in x0..x1 {
            let dist = ((x as f64 + 0.5) - cx).hypot((y as f64 + 0.5) - cy);
            if dist > edge {
                continue;
            }
            let Some(px) = cel.pixel(x, y) else {
                continue;
            };
            let alpha = px[3] as f64 / 255.0;
            if alpha <= 0.0 {
                continue;
            }
            sum_r += px[0] as f64 * alpha;
            sum_g += px[1] as f64 * alpha;
            sum_b += px[2] as f64 * alpha;
            sum_a += alpha;
            count += 1.0;
        }
    }
    if count <= 0.0 || sum_a <= 0.0 {
        return None;
    }
    Some([rint_byte(sum_r / sum_a), rint_byte(sum_g / sum_a), rint_byte(sum_b / sum_a), rint_byte(sum_a / count * 255.0)])
}
