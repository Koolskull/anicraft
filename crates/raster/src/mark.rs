//! Spray, paper grain, and a pencil laid on its side.
//!
//! A round dab still goes through [`flow_dab`]. These three amounts are extra,
//! and each one at 0 leaves that part out. A later brush should change the
//! numbers here, or pass a pen's azimuth as the aim, and should not edit the
//! disc in `dab`.
//!
//! * **Spray** drops tiny dots inside the brush. Their size and opacity vary.
//!   A soft brush keeps more of them near the middle.
//! * **Grain** is tooth on the page. The same pixel always has the same tooth,
//!   and the soft side of a mark loses more of it.
//! * **Lay** turns the disc into a pencil on its side: a sharp edge, then a
//!   fade in the direction of the stroke. A corner swings that edge.

use crate::dab::{Antialias, Tip, blend_cover, disc_cover, flow_dab};
use crate::stroke::Sample;
use crate::tips::{Picture, stamp_picture};

/// How far a brush leaves the round dab. All zero is that dab.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grain {
    /// 0 is one solid dab. 1 is only speckles.
    pub speckle: f32,
    /// 0 is smooth. 1 is paper tooth.
    pub grain: f32,
    /// 0 is round. 1 is a pencil lying on its side.
    pub wedge: f32,
}

/// One stamp: the brush's grain, how much this dab deposits, and where it points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stamp {
    pub flow: f32,
    pub grain: Grain,
    /// Stroke direction. A zero vector leaves the dab round.
    pub aim_x: f32,
    pub aim_y: f32,
    /// A supplied spray picture. [`Picture::None`] keeps the disc and the grain.
    pub picture: Picture,
    /// Radians the picture is turned. Ignored when [`Self::picture`] is none.
    pub turn: f32,
}

impl Stamp {
    /// A disc or grain stamp, with no picture.
    pub const fn plain(flow: f32, grain: Grain, aim_x: f32, aim_y: f32) -> Self {
        Self { flow, grain, aim_x, aim_y, picture: Picture::None, turn: 0.0 }
    }
}

impl Grain {
    pub const ROUND: Self = Self { speckle: 0.0, grain: 0.0, wedge: 0.0 };

    /// True when this brush is the plain disc.
    pub fn plain(self) -> bool {
        unit(self.speckle) == 0.0 && unit(self.grain) == 0.0 && unit(self.wedge) == 0.0
    }
}

/// Direction of dab `index`, toward the next sample, or on from the previous one.
pub fn stroke_aim(points: &[Sample], index: usize) -> (f32, f32) {
    let Some(here) = points.get(index) else { return (0.0, 0.0) };
    if let Some(next_i) = index.checked_add(1)
        && let Some(next) = points.get(next_i)
    {
        return (next.x - here.x, next.y - here.y);
    }
    if index > 0
        && let Some(prev) = points.get(index - 1)
    {
        return (here.x - prev.x, here.y - prev.y);
    }
    (0.0, 0.0)
}

/// Stamp `tip` into the coverage mask. A plain [`Grain`] is [`flow_dab`].
pub fn stamp_mark(mask: &mut [u8], width: u32, height: u32, x: f32, y: f32, tip: Tip, stamp: Stamp) -> bool {
    if !x.is_finite() || !y.is_finite() {
        return false;
    }
    // Same length rule as `flow_dab`. A short mask must not take a partial disc.
    let Some(need) = (width as usize).checked_mul(height as usize) else { return false };
    if mask.len() != need || need == 0 {
        return false;
    }
    if stamp.picture != Picture::None {
        let diameter = tip.radius * 2.0;
        // Flow stays 1: the stronger sample wins, and the fill alpha is applied once
        // for the stroke. A lower flow would darken where the turned pictures overlap.
        return stamp_picture(mask, width, height, x, y, diameter, stamp.turn, 1.0, tip.opacity, stamp.picture);
    }
    if stamp.grain.plain() {
        return flow_dab(mask, width, height, x, y, tip, stamp.flow);
    }
    let radius = tip.radius as f64;
    if !radius.is_finite() || radius < 0.5 {
        return flow_dab(mask, width, height, x, y, tip, stamp.flow);
    }
    let speckle = unit(stamp.grain.speckle);
    let mist = (1.0 - speckle) * (1.0 - speckle);
    let mut hit = false;
    if mist > 0.001 {
        hit |= shade_disc(mask, width, height, f64::from(x), f64::from(y), tip, stamp, mist);
    }
    if speckle > 0.0 {
        hit |= spray(mask, width, height, f64::from(x), f64::from(y), tip, stamp, speckle);
    }
    hit
}

fn shade_disc(mask: &mut [u8], width: u32, height: u32, cx: f64, cy: f64, tip: Tip, stamp: Stamp, mist: f64) -> bool {
    let opacity = (tip.opacity as f64).clamp(0.0, 1.0);
    if opacity <= 0.0 || mist <= 0.0 {
        return false;
    }
    let radius = tip.radius as f64;
    let softness = tip.softness as f64;
    let fringe = tip.antialias.fringe() as f64;
    if !softness.is_finite() || !fringe.is_finite() {
        return false;
    }
    let Some(bounds) = disc_bounds(width, height, cx, cy, radius, softness, fringe) else { return false };
    let (x0, y0, x1, y1) = bounds;
    let edge = radius.max(0.5);
    let grain = unit(stamp.grain.grain);
    let wedge = unit(stamp.grain.wedge);
    let mut hit = false;
    for y in y0..y1 {
        for x in x0..x1 {
            let dx = (f64::from(x) + 0.5) - cx;
            let dy = (f64::from(y) + 0.5) - cy;
            let mut cover = disc_cover(dx.hypot(dy), edge, softness, fringe) * opacity * mist;
            if cover <= 0.0 {
                continue;
            }
            let along = lay_factor(dx, dy, edge, f64::from(stamp.aim_x), f64::from(stamp.aim_y), wedge);
            cover *= along * tooth_keep(paper(x, y), grain, along);
            if cover <= 0.0 {
                continue;
            }
            hit |= blend_cover(mask, width, x, y, cover, stamp.flow);
        }
    }
    hit
}

fn spray(mask: &mut [u8], width: u32, height: u32, cx: f64, cy: f64, tip: Tip, stamp: Stamp, speckle: f64) -> bool {
    let radius = tip.radius as f64;
    let softness = tip.softness as f64;
    let fringe = tip.antialias.fringe() as f64;
    if !softness.is_finite() || !fringe.is_finite() || radius < 0.5 {
        return false;
    }
    let edge = radius.max(0.5);
    let count = (speckle * radius * 1.35).clamp(5.0, 48.0) as u32;
    let budget = count.saturating_mul(5).clamp(8, 240);
    let seed = dab_seed(cx, cy);
    let grain = unit(stamp.grain.grain);
    let wedge = unit(stamp.grain.wedge);
    let mut placed = 0u32;
    let mut hit = false;
    for attempt in 1..=budget {
        if placed >= count {
            break;
        }
        let ang = hash(seed, attempt, 1) * std::f64::consts::TAU;
        let reach = radius * hash(seed, attempt, 2).sqrt();
        let dx = ang.cos() * reach;
        let dy = ang.sin() * reach;
        let env = disc_cover(reach, edge, softness, fringe);
        if hash(seed, attempt, 3) > env {
            continue;
        }
        let along = lay_factor(dx, dy, edge, f64::from(stamp.aim_x), f64::from(stamp.aim_y), wedge);
        if along <= 0.02 {
            continue;
        }
        let weight = 0.22 + 0.78 * hash(seed, attempt, 4);
        let size_roll = hash(seed, attempt, 5);
        let dot_r = if size_roll < 0.55 { 0.18 + 0.32 * size_roll } else { 0.55 + 1.15 * (size_roll - 0.55) };
        let px = cx + dx;
        let py = cy + dy;
        let (Some(gx), Some(gy)) = (crate::dab::floor_i32(px), crate::dab::floor_i32(py)) else { continue };
        let keep = tooth_keep(paper(gx, gy), grain, along);
        let opacity = (tip.opacity as f64) * weight * along * keep;
        if !(opacity.is_finite() && opacity > 0.0) {
            continue;
        }
        let dot = Tip {
            radius: dot_r as f32,
            softness: 0.0,
            antialias: Antialias::On,
            color: tip.color,
            opacity: opacity as f32,
            erase: false,
        };
        if flow_dab(mask, width, height, px as f32, py as f32, dot, stamp.flow) {
            placed += 1;
            hit = true;
        }
    }
    hit
}

fn disc_bounds(width: u32, height: u32, cx: f64, cy: f64, radius: f64, softness: f64, fringe: f64) -> Option<(i32, i32, i32, i32)> {
    let Some(reach) = crate::dab::reach(radius, softness, fringe) else { return None };
    let Some(width_i) = i32::try_from(width).ok() else { return None };
    let Some(height_i) = i32::try_from(height).ok() else { return None };
    let Some(need) = (width as usize).checked_mul(height as usize) else { return None };
    if need == 0 {
        return None;
    }
    let Some(mut x0) = crate::dab::floor_i32(cx - reach) else { return None };
    let Some(mut y0) = crate::dab::floor_i32(cy - reach) else { return None };
    let Some(mut x1) = crate::dab::ceil_i32(cx + reach).and_then(|v| v.checked_add(1)) else { return None };
    let Some(mut y1) = crate::dab::ceil_i32(cy + reach).and_then(|v| v.checked_add(1)) else { return None };
    x0 = x0.max(0);
    y0 = y0.max(0);
    x1 = x1.min(width_i);
    y1 = y1.min(height_i);
    if x1 <= x0 || y1 <= y0 { None } else { Some((x0, y0, x1, y1)) }
}

/// 1 on the sharp side, falling off along the aim. `amount` 0 stays round.
/// A zero aim stays round too: inventing a direction would bake a lip no later dab can cut.
fn lay_factor(dx: f64, dy: f64, radius: f64, aim_x: f64, aim_y: f64, amount: f64) -> f64 {
    if amount <= 0.0 || radius <= 0.0 {
        return 1.0;
    }
    let len = aim_x.hypot(aim_y);
    if !len.is_finite() || len < 1e-6 {
        return 1.0;
    }
    let (ax, ay) = (aim_x / len, aim_y / len);
    let t = (dx * ax + dy * ay) / radius;
    (1.0 - amount) + amount * pencil_profile(t)
}

/// `t` is -1 on the sharp side and +1 on the soft side.
fn pencil_profile(t: f64) -> f64 {
    if t < -0.80 {
        0.0
    } else if t < -0.72 {
        (t + 0.80) / 0.08
    } else if t < -0.05 {
        1.0
    } else {
        let u = ((t + 0.05) / 1.05).clamp(0.0, 1.0);
        let fade = 1.0 - u;
        fade * fade
    }
}

/// Paper tooth. The soft side of a mark loses more of it.
fn tooth_keep(tooth: f64, grain: f64, along: f64) -> f64 {
    let soft = (1.0 - along).clamp(0.0, 1.0);
    let eat = grain.clamp(0.0, 1.0) * (0.25 + 0.75 * soft);
    1.0 - eat * (1.0 - tooth.clamp(0.0, 1.0).powf(0.6))
}

/// Tooth locked to the page, not to the dab.
fn paper(x: i32, y: i32) -> f64 {
    let n = (x as u32).wrapping_mul(0x27BB_2EE6).wrapping_add((y as u32).wrapping_mul(0x1656_67B1));
    mix(n)
}

fn dab_seed(x: f64, y: f64) -> u32 {
    let qx = (x * 16.0).round() as i32;
    let qy = (y * 16.0).round() as i32;
    (qx as u32).wrapping_mul(0x9E37_79B9) ^ (qy as u32).wrapping_mul(0x85EB_CA6B)
}

fn hash(seed: u32, attempt: u32, lane: u32) -> f64 {
    mix(seed.wrapping_add(attempt.wrapping_mul(0x9E37_79B9)).wrapping_add(lane.wrapping_mul(0x85EB_CA6B)))
}

fn mix(mut z: u32) -> f64 {
    z = (z ^ (z >> 16)).wrapping_mul(0x7FEB_352D);
    z = (z ^ (z >> 15)).wrapping_mul(0x846C_A68B);
    z ^= z >> 16;
    f64::from(z) * (1.0 / f64::from(u32::MAX))
}

fn unit(value: f32) -> f64 {
    if value.is_finite() { f64::from(value).clamp(0.0, 1.0) } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dab::{Antialias, Tip, flow_dab};

    fn hard(radius: f32) -> Tip {
        Tip { radius, softness: 0.0, antialias: Antialias::Off, color: [0, 0, 0, 255], opacity: 1.0, erase: false }
    }

    fn at(mask: &[u8], x: i32, y: i32) -> u8 {
        mask[(y as usize) * 32 + (x as usize)]
    }

    #[test]
    fn a_plain_mark_matches_the_round_dab() {
        let tip = hard(6.0);
        let mut round = vec![0u8; 32 * 32];
        let mut marked = vec![0u8; 32 * 32];
        assert!(flow_dab(&mut round, 32, 32, 16.0, 16.0, tip, 1.0));
        let stamp = Stamp::plain(1.0, Grain::ROUND, 4.0, 1.0);
        assert!(stamp_mark(&mut marked, 32, 32, 16.0, 16.0, tip, stamp));
        assert_eq!(round, marked);
    }

    #[test]
    fn a_pencil_is_sharp_on_one_side_and_soft_the_other_way() {
        let tip = hard(8.0);
        let mut mask = vec![0u8; 32 * 32];
        let stamp = Stamp::plain(1.0, Grain { speckle: 0.0, grain: 0.0, wedge: 1.0 }, 1.0, 0.0);
        assert!(stamp_mark(&mut mask, 32, 32, 16.0, 16.0, tip, stamp));
        let sharp = at(&mask, 11, 16);
        let fade = at(&mask, 21, 16);
        assert!(sharp > 200, "sharp side {sharp}");
        assert!(fade < 40, "soft side {fade}");
        assert_eq!(at(&mask, 8, 16), 0, "the lip cuts the disc");
        let mut again = vec![0u8; 32 * 32];
        assert!(stamp_mark(&mut again, 32, 32, 16.0, 16.0, tip, stamp));
        assert_eq!(mask, again);
    }

    #[test]
    fn grain_stays_on_the_page_and_breaks_a_flat_disc() {
        let tip = hard(8.0);
        let grain = Grain { speckle: 0.0, grain: 1.0, wedge: 0.0 };
        let mut left = vec![0u8; 32 * 32];
        let mut right = vec![0u8; 32 * 32];
        let mut plain = vec![0u8; 32 * 32];
        assert!(stamp_mark(&mut left, 32, 32, 16.0, 16.0, tip, Stamp::plain(1.0, grain, 1.0, 0.0)));
        assert!(stamp_mark(&mut right, 32, 32, 18.0, 16.0, tip, Stamp::plain(1.0, grain, 1.0, 0.0)));
        assert!(flow_dab(&mut plain, 32, 32, 16.0, 16.0, tip, 1.0));
        assert_ne!(left, plain);
        let mut shared = false;
        for y in 0..32 {
            for x in 0..32 {
                let i = y * 32 + x;
                if plain[i] == 255 && left[i] == right[i] && left[i] < 255 {
                    shared = true;
                }
            }
        }
        assert!(shared, "a full pixel should keep the same tooth under two dabs");
    }

    #[test]
    fn spray_is_specks_inside_a_soft_brush() {
        let tip = Tip { radius: 12.0, softness: 1.0, antialias: Antialias::On, color: [0, 0, 0, 255], opacity: 1.0, erase: false };
        let mut soft = vec![0u8; 48 * 48];
        let mut dots = vec![0u8; 48 * 48];
        assert!(flow_dab(&mut soft, 48, 48, 24.0, 24.0, tip, 1.0));
        let stamp = Stamp::plain(1.0, Grain { speckle: 1.0, grain: 0.0, wedge: 0.0 }, 0.0, 0.0);
        assert!(stamp_mark(&mut dots, 48, 48, 24.0, 24.0, tip, stamp));
        let filled = |buf: &[u8]| buf.iter().filter(|b| **b > 0).count();
        assert!(filled(&dots) * 3 < filled(&soft), "specks {} soft {}", filled(&dots), filled(&soft));
        assert!(dots.iter().any(|b| *b > 20));
        let mut hole = false;
        for y in 16..32 {
            for x in 16..32 {
                let i = y * 48 + x;
                if soft[i] > 0 && dots[i] == 0 {
                    hole = true;
                }
            }
        }
        assert!(hole, "spray leaves paper between the dots");
    }

    #[test]
    fn a_pencil_with_no_direction_stays_round() {
        let tip = hard(8.0);
        let mut round = vec![0u8; 32 * 32];
        let mut lost = vec![0u8; 32 * 32];
        assert!(stamp_mark(&mut round, 32, 32, 16.0, 16.0, tip, Stamp::plain(1.0, Grain::ROUND, 0.0, 0.0)));
        let wedge = Stamp::plain(1.0, Grain { speckle: 0.0, grain: 0.0, wedge: 1.0 }, 0.0, 0.0);
        assert!(stamp_mark(&mut lost, 32, 32, 16.0, 16.0, tip, wedge));
        assert_eq!(round, lost);
    }

    #[test]
    fn spray_stays_inside_the_stroke_band() {
        let radius = 14.0f32;
        let tip = Tip { radius, softness: 1.0, antialias: Antialias::On, color: [0, 0, 0, 255], opacity: 1.0, erase: false };
        let mut mask = vec![0u8; 64 * 64];
        let stamp = Stamp::plain(1.0, Grain { speckle: 1.0, grain: 1.0, wedge: 1.0 }, 0.0, 1.0);
        assert!(stamp_mark(&mut mask, 64, 64, 32.0, 32.0, tip, stamp));
        let mut far = 0.0f64;
        for y in 0..64 {
            for x in 0..64 {
                if mask[y * 64 + x] == 0 {
                    continue;
                }
                let dx = (x as f64 + 0.5) - 32.0;
                let dy = (y as f64 + 0.5) - 32.0;
                far = far.max(dx.hypot(dy));
            }
        }
        // `band` pads the radius by 4. Ink past that would be left off the plate.
        assert!(far < f64::from(radius) + 3.5, "spray reached {far}");
    }
}
