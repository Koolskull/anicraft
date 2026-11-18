//! A cropped RGBA plate. It grows to the dabs and refuses a plate past the cap.

use crate::dab::{self, Tip};

/// Longest side of a cel, in pixels.
pub const MAX_EDGE: u32 = 4096;
/// Most bytes one cel may hold. A square past 2048 px hits this before [`MAX_EDGE`].
pub const MAX_BYTES: usize = 16 * 1024 * 1024;

/// Document-space pixels. Empty means no ink yet. The origin is the top-left of `pixels`.
#[derive(Clone, Debug)]
pub struct Cel {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Cel {
    pub fn empty() -> Self {
        Self { x: 0, y: 0, width: 0, height: 0, rgba: Vec::new() }
    }

    /// A transparent plate. `None` when a side is 0 or the plate is over the cap.
    pub fn blank(x: i32, y: i32, width: u32, height: u32) -> Option<Self> {
        let rgba = alloc(width, height)?;
        Some(Self { x, y, width, height, rgba })
    }

    pub fn origin(&self) -> (i32, i32) {
        (self.x, self.y)
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn pixels(&self) -> &[u8] {
        &self.rgba
    }

    pub fn pixel(&self, x: i32, y: i32) -> Option<[u8; 4]> {
        let o = self.offset(x, y)?;
        let px = self.rgba.get(o..o + 4)?;
        Some([px[0], px[1], px[2], px[3]])
    }

    pub fn set(&mut self, x: i32, y: i32, px: [u8; 4]) -> bool {
        let Some(o) = self.offset(x, y) else {
            return false;
        };
        let Some(slot) = self.rgba.get_mut(o..o + 4) else {
            return false;
        };
        slot.copy_from_slice(&px);
        true
    }

    pub fn fill(&mut self, px: [u8; 4]) {
        for slot in self.rgba.as_chunks_mut::<4>().0 {
            slot.copy_from_slice(&px);
        }
    }

    /// Stamp in document pixels. Grows the cel to the dab. A plate past the cap is left unchanged.
    pub fn stamp(&mut self, x: f32, y: f32, tip: Tip) -> bool {
        if !x.is_finite() || !y.is_finite() || !tip.opacity.is_finite() || tip.opacity <= 0.0 {
            return false;
        }
        let softness = tip.softness as f64;
        let fringe = tip.antialias.fringe() as f64;
        let Some(reach) = dab::reach(tip.radius as f64, softness, fringe) else {
            return false;
        };
        let Some(x0) = dab::floor_i32(x as f64 - reach) else {
            return false;
        };
        let Some(y0) = dab::floor_i32(y as f64 - reach) else {
            return false;
        };
        let Some(x1) = dab::ceil_i32(x as f64 + reach).and_then(|v| v.checked_add(1)) else {
            return false;
        };
        let Some(y1) = dab::ceil_i32(y as f64 + reach).and_then(|v| v.checked_add(1)) else {
            return false;
        };
        if !self.ensure(x0, y0, x1, y1) {
            return false;
        }
        let local_x = x as f64 - self.x as f64;
        let local_y = y as f64 - self.y as f64;
        dab::dab_f64(&mut self.rgba, self.width, self.height, local_x, local_y, tip)
    }

    /// Drop the transparent margin. A fully clear cel becomes empty.
    pub fn crop_to_ink(&mut self) {
        if self.width == 0 || self.height == 0 {
            self.clear();
            return;
        }
        let mut min_x = self.width;
        let mut min_y = self.height;
        let mut max_x = 0u32;
        let mut max_y = 0u32;
        let mut any = false;
        for y in 0..self.height {
            for x in 0..self.width {
                let Some(o) = (y as usize)
                    .checked_mul(self.width as usize)
                    .and_then(|row| row.checked_add(x as usize))
                    .and_then(|i| i.checked_mul(4))
                    .and_then(|i| i.checked_add(3))
                else {
                    return;
                };
                let Some(alpha) = self.rgba.get(o).copied() else {
                    return;
                };
                if alpha > 0 {
                    any = true;
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }
        if !any {
            self.clear();
            return;
        }
        let Some(grown) = self.cropped(min_x, min_y, max_x, max_y) else {
            return;
        };
        *self = grown;
    }

    fn cropped(&self, min_x: u32, min_y: u32, max_x: u32, max_y: u32) -> Option<Self> {
        let width = max_x.checked_sub(min_x)?.checked_add(1)?;
        let height = max_y.checked_sub(min_y)?.checked_add(1)?;
        let mut next = Self::blank(self.x.checked_add(i32::try_from(min_x).ok()?)?, self.y.checked_add(i32::try_from(min_y).ok()?)?, width, height)?;
        for y in 0..height {
            let src_y = min_y.checked_add(y)?;
            let src = self.row(min_x, src_y, width)?;
            let dst = next.row_mut(0, y, width)?;
            dst.copy_from_slice(src);
        }
        Some(next)
    }

    fn clear(&mut self) {
        *self = Self::empty();
    }

    fn ensure(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) -> bool {
        if x1 <= x0 || y1 <= y0 {
            return false;
        }
        if self.width == 0 || self.height == 0 {
            return self.replace_with(x0, y0, x1, y1, None);
        }
        let Some(right0) = span_end(self.x, self.width) else {
            return false;
        };
        let Some(bottom0) = span_end(self.y, self.height) else {
            return false;
        };
        let left = self.x.min(x0);
        let top = self.y.min(y0);
        let right = right0.max(x1);
        let bottom = bottom0.max(y1);
        if left == self.x && top == self.y && right == right0 && bottom == bottom0 {
            return true;
        }
        let old = self.clone();
        self.replace_with(left, top, right, bottom, Some(&old))
    }

    fn replace_with(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, old: Option<&Cel>) -> bool {
        let Some(width) = x1.checked_sub(x0).and_then(|v| u32::try_from(v).ok()) else {
            return false;
        };
        let Some(height) = y1.checked_sub(y0).and_then(|v| u32::try_from(v).ok()) else {
            return false;
        };
        let Some(mut next) = Self::blank(x0, y0, width, height) else {
            return false;
        };
        if let Some(old) = old {
            let Some(ox) = u32::try_from(old.x.saturating_sub(x0)).ok().filter(|_| old.x >= x0) else {
                return false;
            };
            let Some(oy) = u32::try_from(old.y.saturating_sub(y0)).ok().filter(|_| old.y >= y0) else {
                return false;
            };
            for y in 0..old.height {
                let Some(src) = old.row(0, y, old.width) else {
                    return false;
                };
                let Some(dy) = oy.checked_add(y) else {
                    return false;
                };
                let Some(dst) = next.row_mut(ox, dy, old.width) else {
                    return false;
                };
                dst.copy_from_slice(src);
            }
        }
        *self = next;
        true
    }

    fn offset(&self, x: i32, y: i32) -> Option<usize> {
        let lx = u32::try_from(x.checked_sub(self.x)?).ok()?;
        let ly = u32::try_from(y.checked_sub(self.y)?).ok()?;
        if lx >= self.width || ly >= self.height {
            return None;
        }
        (ly as usize).checked_mul(self.width as usize)?.checked_add(lx as usize)?.checked_mul(4)
    }

    fn row(&self, x: u32, y: u32, width: u32) -> Option<&[u8]> {
        let start = self.local_offset(x, y)?;
        let bytes = (width as usize).checked_mul(4)?;
        self.rgba.get(start..start.checked_add(bytes)?)
    }

    fn row_mut(&mut self, x: u32, y: u32, width: u32) -> Option<&mut [u8]> {
        let start = self.local_offset(x, y)?;
        let bytes = (width as usize).checked_mul(4)?;
        self.rgba.get_mut(start..start.checked_add(bytes)?)
    }

    fn local_offset(&self, x: u32, y: u32) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }
        (y as usize).checked_mul(self.width as usize)?.checked_add(x as usize)?.checked_mul(4)
    }
}

fn alloc(width: u32, height: u32) -> Option<Vec<u8>> {
    if width == 0 || height == 0 || width > MAX_EDGE || height > MAX_EDGE {
        return None;
    }
    let bytes = (width as usize).checked_mul(height as usize)?.checked_mul(4)?;
    if bytes > MAX_BYTES {
        return None;
    }
    Some(vec![0; bytes])
}

fn span_end(origin: i32, size: u32) -> Option<i32> {
    origin.checked_add(i32::try_from(size).ok()?)
}
