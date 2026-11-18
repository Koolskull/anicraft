//! Clip paint to the coverage of the plate under it.
//!
//! This is the Clip Studio / VMI direction: the upper paint keeps the lower
//! layer's alpha. It is not an Animate mask, which reveals the layers below it.

use crate::cel::Cel;
use crate::dab::rint_byte;

/// Blend `source` over `base` in normal mode, then replace alpha with the base alpha.
///
/// The result has the base's origin and size. Source pixels outside the base are dropped.
/// Where the base is clear, the source colour is discarded.
pub fn clip_alpha(base: &Cel, source: &Cel) -> Cel {
    let mut out = base.clone();
    let (bw, bh) = base.size();
    let (sw, sh) = source.size();
    if bw == 0 || bh == 0 || sw == 0 || sh == 0 {
        return out;
    }
    let (bx, by) = base.origin();
    let (sx, sy) = source.origin();
    let Some(bright) = edge(bx, bw) else {
        return out;
    };
    let Some(bbottom) = edge(by, bh) else {
        return out;
    };
    let Some(sright) = edge(sx, sw) else {
        return out;
    };
    let Some(sbottom) = edge(sy, sh) else {
        return out;
    };
    let left = bx.max(sx);
    let top = by.max(sy);
    let right = bright.min(sright);
    let bottom = bbottom.min(sbottom);
    if right <= left || bottom <= top {
        return out;
    }
    for y in top..bottom {
        for x in left..right {
            let Some(base_px) = base.pixel(x, y) else {
                continue;
            };
            let Some(src_px) = source.pixel(x, y) else {
                continue;
            };
            if !out.set(x, y, blend_clip(base_px, src_px)) {
                return base.clone();
            }
        }
    }
    out
}

fn edge(origin: i32, size: u32) -> Option<i32> {
    origin.checked_add(i32::try_from(size).ok()?)
}

fn blend_clip(base: [u8; 4], src: [u8; 4]) -> [u8; 4] {
    let ca = base[3] as f64 / 255.0;
    let sa = src[3] as f64 / 255.0;
    let mut color = [0.0; 3];
    let ao = sa + ca * (1.0 - sa);
    if ao > 1e-6 {
        for i in 0..3 {
            let cb = base[i] as f64 / 255.0;
            let cs = src[i] as f64 / 255.0;
            let premul = sa * (1.0 - ca) * cs + sa * ca * cs + (1.0 - sa) * ca * cb;
            color[i] = premul / ao;
        }
    }
    if ca <= 0.0 {
        color = [base[0] as f64 / 255.0, base[1] as f64 / 255.0, base[2] as f64 / 255.0];
    }
    [rint_byte(color[0] * 255.0), rint_byte(color[1] * 255.0), rint_byte(color[2] * 255.0), rint_byte(ca * 255.0)]
}
