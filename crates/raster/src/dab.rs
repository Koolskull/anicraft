//! One round stamp into an RGBA buffer.
//!
//! The coverage rules are VMI Studio's: a bool antialias of "on" is a 0.5 px
//! fringe, not 1. Softness feathers the inside, and a fringe of 0 turns that
//! feather into a hard 0.5 threshold. Erase punches alpha and leaves the colour.

/// How the rim of a dab is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Antialias {
    /// Hard disc.
    Off,
    /// Half-pixel fringe. This is the old checked antialias button.
    On,
    /// Fringe width in pixels. A non-positive value is a hard disc.
    Px(f32),
}

impl Antialias {
    /// Fringe in pixels. `On` stays 0.5 so it does not become a 1 px rim.
    pub fn fringe(self) -> f32 {
        match self {
            Self::Off => 0.0,
            Self::On => 0.5,
            Self::Px(value) => {
                if value.is_finite() && value > 0.0 {
                    value
                } else {
                    0.0
                }
            }
        }
    }
}

/// Finest diameter that still stores a pixel. Thinner than this rounds to alpha 0.
pub const HAIRLINE_DIAMETER: f32 = 0.05;
/// Radius of [`HAIRLINE_DIAMETER`]. Below this the stamp does nothing.
const HAIRLINE_RADIUS: f64 = 0.025;

/// One disc. Coordinates are passed separately so a cel can place it in document space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tip {
    /// Radius in pixels. Below [`HAIRLINE_RADIUS`] the stamp does nothing.
    /// Under half a pixel the stamp is one texel, not a disc.
    pub radius: f32,
    /// 0 is a flat disc. 1 fades from the centre. Values outside 0..=1 are clamped on the soft path.
    pub softness: f32,
    pub antialias: Antialias,
    /// Straight RGBA.
    pub color: [u8; 4],
    /// 0 skips the stamp. Above 1 is treated as 1.
    pub opacity: f32,
    /// Punch alpha and leave RGB.
    pub erase: bool,
}

/// The strongest coverage of `tip` at `(x, y)`, as one byte per pixel. Overlaps keep the max.
///
/// This is the shape of a dab, including `tip.opacity`. The stroke's colour alpha is applied
/// later, once, by [`paint_cover`].
pub fn cover_dab(mask: &mut [u8], width: u32, height: u32, x: f32, y: f32, tip: Tip) -> bool {
    if !x.is_finite() || !y.is_finite() {
        return false;
    }
    cover_f64(mask, width, height, x as f64, y as f64, tip, Coat::Max)
}

/// Deposit `flow` of `tip` into the coverage mask. `1` is [`cover_dab`]: the stronger stamp wins.
/// Below that, each dab source-overs its share, so a stroke can build up. The colour's alpha is
/// still applied once, by [`paint_cover`].
pub fn flow_dab(mask: &mut [u8], width: u32, height: u32, x: f32, y: f32, tip: Tip, flow: f32) -> bool {
    if !x.is_finite() || !y.is_finite() {
        return false;
    }
    if !flow.is_finite() || flow >= 1.0 {
        return cover_dab(mask, width, height, x, y, tip);
    }
    if flow <= 0.0 {
        return false;
    }
    cover_f64(mask, width, height, x as f64, y as f64, tip, Coat::Flow(flow as f64))
}

#[derive(Clone, Copy)]
enum Coat {
    Max,
    Flow(f64),
}

/// Blend `cover` onto `pixels` in place. A byte of 255 is a full dab. `color`'s alpha is the
/// opacity of the whole stroke, so a second dab at the same cover does not paint a darker pixel.
pub fn paint_cover(pixels: &mut [u8], cover: &[u8], color: [u8; 4], erase: bool) -> bool {
    let n = cover.len();
    if n == 0 || pixels.len() != n.saturating_mul(4) {
        return false;
    }
    let stroke_a = color[3] as f64 / 255.0;
    if !(stroke_a.is_finite() && stroke_a > 0.0) {
        return false;
    }
    let mut hit = false;
    for (i, byte) in cover.iter().copied().enumerate() {
        if byte == 0 {
            continue;
        }
        let o = i.saturating_mul(4);
        let Some(px) = pixels.get_mut(o..o + 4) else { continue };
        let amount = (byte as f64 / 255.0) * stroke_a;
        if erase {
            let next = rint_byte(px[3] as f64 * (1.0 - amount));
            if next != px[3] {
                px[3] = next;
                hit = true;
            }
            continue;
        }
        let src_a = amount.clamp(0.0, 1.0);
        let dst_a = px[3] as f64 / 255.0;
        let out_a = src_a + dst_a * (1.0 - src_a);
        let mut next = [0u8; 4];
        if out_a > 0.0 {
            for channel in 0..3 {
                let mixed = (color[channel] as f64 * src_a + px[channel] as f64 * dst_a * (1.0 - src_a)) / out_a.max(1e-6);
                next[channel] = rint_byte(mixed);
            }
        }
        next[3] = rint_byte(out_a * 255.0);
        if next != [px[0], px[1], px[2], px[3]] {
            px.copy_from_slice(&next);
            hit = true;
        }
    }
    hit
}

/// Stamp `tip` at `(x, y)` in buffer pixels. Returns whether any coverage landed inside the buffer.
///
/// `pixels` is tightly packed RGBA, length `width * height * 4`. A bad length does nothing.
pub fn dab(pixels: &mut [u8], width: u32, height: u32, x: f32, y: f32, tip: Tip) -> bool {
    if !x.is_finite() || !y.is_finite() {
        return false;
    }
    dab_f64(pixels, width, height, x as f64, y as f64, tip)
}

fn cover_f64(mask: &mut [u8], width: u32, height: u32, cx: f64, cy: f64, tip: Tip, coat: Coat) -> bool {
    if !cx.is_finite() || !cy.is_finite() {
        return false;
    }
    let opacity = tip.opacity as f64;
    if !opacity.is_finite() || opacity <= 0.0 {
        return false;
    }
    let opacity = opacity.clamp(0.0, 1.0);
    let radius = tip.radius as f64;
    if !radius.is_finite() || radius < HAIRLINE_RADIUS {
        return false;
    }
    let softness = tip.softness as f64;
    let fringe = tip.antialias.fringe() as f64;
    if !softness.is_finite() || !fringe.is_finite() {
        return false;
    }
    let Some(need) = (width as usize).checked_mul(height as usize) else { return false };
    if mask.len() != need || need == 0 {
        return false;
    }
    let Some(width_i) = i32::try_from(width).ok() else { return false };
    let Some(height_i) = i32::try_from(height).ok() else { return false };
    if radius < 0.5 {
        let (Some(x), Some(y)) = (floor_i32(cx), floor_i32(cy)) else { return false };
        if x < 0 || y < 0 || x >= width_i || y >= height_i {
            return false;
        }
        let cover = (radius / 0.5) * (radius / 0.5) * opacity;
        return lay(mask, width, x, y, cover, coat);
    }
    let Some(reach) = reach(radius, softness, fringe) else { return false };
    let Some(mut x0) = floor_i32(cx - reach) else { return false };
    let Some(mut y0) = floor_i32(cy - reach) else { return false };
    let Some(mut x1) = ceil_i32(cx + reach).and_then(|v| v.checked_add(1)) else { return false };
    let Some(mut y1) = ceil_i32(cy + reach).and_then(|v| v.checked_add(1)) else { return false };
    x0 = x0.max(0);
    y0 = y0.max(0);
    x1 = x1.min(width_i);
    y1 = y1.min(height_i);
    if x1 <= x0 || y1 <= y0 {
        return false;
    }
    let edge = radius.max(0.5);
    let mut hit = false;
    for y in y0..y1 {
        for x in x0..x1 {
            let dist = ((x as f64 + 0.5) - cx).hypot((y as f64 + 0.5) - cy);
            let cover = coverage(dist, edge, softness, fringe) * opacity;
            if cover <= 0.0 {
                continue;
            }
            hit |= lay(mask, width, x, y, cover, coat);
        }
    }
    hit
}

fn lay(mask: &mut [u8], width: u32, x: i32, y: i32, cover: f64, coat: Coat) -> bool {
    match coat {
        Coat::Max => raise(mask, width, x, y, cover),
        Coat::Flow(flow) => {
            let flow = if flow.is_finite() { flow.clamp(0.0, 1.0) } else { 1.0 };
            deposit(mask, width, x, y, cover * flow)
        }
    }
}

fn raise(mask: &mut [u8], width: u32, x: i32, y: i32, cover: f64) -> bool {
    let Some(i) = (y as usize).checked_mul(width as usize).and_then(|row| row.checked_add(x as usize)) else { return false };
    let Some(slot) = mask.get_mut(i) else { return false };
    let byte = rint_byte(cover * 255.0);
    if byte > *slot {
        *slot = byte;
        true
    } else {
        false
    }
}

/// Source-over `src` (0 to 1) onto one coverage byte.
fn deposit(mask: &mut [u8], width: u32, x: i32, y: i32, src: f64) -> bool {
    if !(src.is_finite() && src > 0.0) {
        return false;
    }
    let src = src.clamp(0.0, 1.0);
    let Some(i) = (y as usize).checked_mul(width as usize).and_then(|row| row.checked_add(x as usize)) else { return false };
    let Some(slot) = mask.get_mut(i) else { return false };
    let dst = *slot as f64 / 255.0;
    let byte = rint_byte((src + dst * (1.0 - src)) * 255.0);
    if byte != *slot {
        *slot = byte;
        true
    } else {
        false
    }
}

pub(crate) fn dab_f64(pixels: &mut [u8], width: u32, height: u32, cx: f64, cy: f64, tip: Tip) -> bool {
    if !cx.is_finite() || !cy.is_finite() {
        return false;
    }
    let opacity = tip.opacity as f64;
    if !opacity.is_finite() || opacity <= 0.0 {
        return false;
    }
    let opacity = opacity.clamp(0.0, 1.0);
    let radius = tip.radius as f64;
    if !radius.is_finite() || radius < HAIRLINE_RADIUS {
        return false;
    }
    let softness = tip.softness as f64;
    let fringe = tip.antialias.fringe() as f64;
    if !softness.is_finite() || !fringe.is_finite() {
        return false;
    }
    let Some(width_i) = i32::try_from(width).ok() else {
        return false;
    };
    let Some(height_i) = i32::try_from(height).ok() else {
        return false;
    };
    let Some(need) = (width as usize).checked_mul(height as usize).and_then(|n| n.checked_mul(4)) else {
        return false;
    };
    if pixels.len() != need || need == 0 {
        return false;
    }
    // Under half a pixel: one texel, coverage (radius / 0.5)². The disc path's
    // `edge = radius.max(0.5)` would paint a full pixel instead.
    if radius < 0.5 {
        return hairline(pixels, width, height, cx, cy, radius, tip);
    }
    let Some(reach) = reach(radius, softness, fringe) else {
        return false;
    };
    let Some(mut x0) = floor_i32(cx - reach) else {
        return false;
    };
    let Some(mut y0) = floor_i32(cy - reach) else {
        return false;
    };
    let Some(mut x1) = ceil_i32(cx + reach).and_then(|v| v.checked_add(1)) else {
        return false;
    };
    let Some(mut y1) = ceil_i32(cy + reach).and_then(|v| v.checked_add(1)) else {
        return false;
    };
    x0 = x0.max(0);
    y0 = y0.max(0);
    x1 = x1.min(width_i);
    y1 = y1.min(height_i);
    if x1 <= x0 || y1 <= y0 {
        return false;
    }
    let edge = (tip.radius as f64).max(0.5);
    let mut hit = false;
    for y in y0..y1 {
        for x in x0..x1 {
            let dist = ((x as f64 + 0.5) - cx).hypot((y as f64 + 0.5) - cy);
            let cover = coverage(dist, edge, softness, fringe) * opacity;
            if cover <= 0.0 {
                continue;
            }
            let o = ((y as usize) * (width as usize) + (x as usize)) * 4;
            let Some(px) = pixels.get_mut(o..o + 4) else {
                continue;
            };
            write_pixel(px, tip.color, cover, tip.erase);
            hit = true;
        }
    }
    hit
}

pub(crate) fn reach(radius: f64, softness: f64, fringe: f64) -> Option<f64> {
    if !radius.is_finite() || radius < HAIRLINE_RADIUS || !softness.is_finite() || !fringe.is_finite() {
        return None;
    }
    // The containing texel. Callers that want a real disc still take the >= 0.5 path.
    if radius < 0.5 {
        return Some(0.5);
    }
    let pad = if fringe > 0.0 || softness > 0.0 { fringe.max(1.0) } else { 0.0 };
    Some(radius + pad)
}

/// One pixel, the one that contains `(cx, cy)`. No softness and no fringe.
fn hairline(pixels: &mut [u8], width: u32, height: u32, cx: f64, cy: f64, radius: f64, tip: Tip) -> bool {
    let (Some(x), Some(y)) = (floor_i32(cx), floor_i32(cy)) else { return false };
    let Some(width_i) = i32::try_from(width).ok() else { return false };
    let Some(height_i) = i32::try_from(height).ok() else { return false };
    if x < 0 || y < 0 || x >= width_i || y >= height_i {
        return false;
    }
    let opacity = (tip.opacity as f64).clamp(0.0, 1.0);
    let cover = (radius / 0.5) * (radius / 0.5) * opacity;
    if cover <= 0.0 {
        return false;
    }
    let o = ((y as usize) * (width as usize) + (x as usize)) * 4;
    let Some(px) = pixels.get_mut(o..o + 4) else { return false };
    write_pixel(px, tip.color, cover, tip.erase);
    true
}

/// The disc's coverage at `dist` from the centre. A mark multiplies this. It does not replace it.
pub(crate) fn disc_cover(dist: f64, edge: f64, softness: f64, fringe: f64) -> f64 {
    coverage(dist, edge, softness, fringe)
}

/// Write one coverage sample the way [`flow_dab`] does. `flow` of 1 keeps the stronger byte.
pub(crate) fn blend_cover(mask: &mut [u8], width: u32, x: i32, y: i32, cover: f64, flow: f32) -> bool {
    if !(cover.is_finite() && cover > 0.0) {
        return false;
    }
    if !flow.is_finite() || flow >= 1.0 {
        lay(mask, width, x, y, cover, Coat::Max)
    } else if flow <= 0.0 {
        false
    } else {
        lay(mask, width, x, y, cover, Coat::Flow(f64::from(flow)))
    }
}

fn coverage(dist: f64, edge: f64, softness: f64, fringe: f64) -> f64 {
    if softness <= 0.0 && fringe <= 0.0 {
        if dist <= edge { 1.0 } else { 0.0 }
    } else if softness <= 0.0 {
        (edge + fringe - dist).clamp(0.0, 1.0)
    } else {
        let inner = edge * (1.0 - softness.clamp(0.0, 1.0));
        let mut cover = ((edge - dist) / (edge - inner).max(0.001)).clamp(0.0, 1.0);
        if fringe <= 0.0 {
            cover = if cover >= 0.5 { 1.0 } else { 0.0 };
        }
        cover
    }
}

fn write_pixel(px: &mut [u8], color: [u8; 4], cover: f64, erase: bool) {
    let Some(px) = px.get_mut(..4) else {
        return;
    };
    if erase {
        px[3] = rint_byte(px[3] as f64 * (1.0 - cover));
        return;
    }
    let src_a = cover * (color[3] as f64 / 255.0);
    let dst_a = px[3] as f64 / 255.0;
    let out_a = src_a + dst_a * (1.0 - src_a);
    if out_a > 0.0 {
        for i in 0..3 {
            let mixed = (color[i] as f64 * src_a + px[i] as f64 * dst_a * (1.0 - src_a)) / out_a.max(1e-6);
            px[i] = rint_byte(mixed);
        }
    }
    px[3] = rint_byte(out_a * 255.0);
}

pub(crate) fn floor_i32(value: f64) -> Option<i32> {
    if !value.is_finite() {
        return None;
    }
    let floored = value.floor();
    if floored < i32::MIN as f64 || floored > i32::MAX as f64 {
        return None;
    }
    Some(floored as i32)
}

pub(crate) fn ceil_i32(value: f64) -> Option<i32> {
    if !value.is_finite() {
        return None;
    }
    let ceiled = value.ceil();
    if ceiled < i32::MIN as f64 || ceiled > i32::MAX as f64 {
        return None;
    }
    Some(ceiled as i32)
}

/// `numpy.rint`: half goes to the even integer. `f64::round` goes away from zero, which
/// disagrees on *.5 and would miss the frozen dab.
pub(crate) fn rint_byte(value: f64) -> u8 {
    let x = if value.is_finite() { value.clamp(0.0, 255.0) } else { 0.0 };
    let down = x.floor();
    let frac = x - down;
    let even = (down as i64) & 1 == 0;
    let rounded = if frac > 0.5 || (frac == 0.5 && !even) { down + 1.0 } else { down };
    rounded as u8
}
