//! A pixel layer's plate: one straight-alpha buffer the size of the stage.
//!
//! Empty bytes are transparent. The plate is the keyframe's picture for every
//! frame that keyframe covers. The raster crate paints into the bytes; this
//! type only stores them, because the document cannot depend on that crate.
//!
//! Rows are shared until a stroke writes that row. A drag therefore does not
//! copy the whole stage on every move, and a copied key (F6) keeps the rows
//! the stroke did not touch. `stamp` is the GPU texture's identity. It changes
//! only when this plate forks away from another key. `generation` moves forward on
//! each write and back on undo, so the GPU can refresh the same texture.

use crate::Pixels;
use crate::bitmap::MAX_BITMAP_EDGE;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Largest plate, in bytes. A 3840×3840 stage fits. A larger one is refused.
pub const MAX_PLATE_BYTES: usize = 64 * 1024 * 1024;

/// What the GPU needs to reuse one plate texture (ADR 0113). Not saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlateDraw {
    /// Stable texture identity. A forked key gets its own.
    pub stamp: u64,
    /// Bumps on each write. Undo restores an older value.
    pub generation: u64,
    /// First row of the band written since the GPU caught up. Full width.
    pub y0: u32,
    /// One past the last row of that band. Equal to `y0` when nothing is pending.
    pub y1: u32,
}

/// A rectangle of plate texels, `x` and `y` inclusive, `w` by `h` half-open.
///
/// Tools ask for this instead of walking rows. A crop is one of these (ADR 0115).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl PixelRect {
    pub fn is_empty(self) -> bool {
        self.w == 0 || self.h == 0
    }

    /// The part that lies on a plate of `width` by `height`.
    pub fn clip(self, width: u32, height: u32) -> Option<Self> {
        if self.is_empty() || self.x >= width || self.y >= height {
            return None;
        }
        let w = self.w.min(width - self.x);
        let h = self.h.min(height - self.y);
        (w > 0 && h > 0).then_some(Self { x: self.x, y: self.y, w, h })
    }

    pub fn contains(self, x: u32, y: u32) -> bool {
        x >= self.x && y >= self.y && x - self.x < self.w && y - self.y < self.h
    }

    /// `x`, `y` in stage space. The texel is `floor` of each, the same cell a dab writes.
    pub fn contains_stage(self, x: f32, y: f32) -> bool {
        if !x.is_finite() || !y.is_finite() {
            return false;
        }
        let (xi, yi) = (x.floor() as i32, y.floor() as i32);
        xi >= self.x as i32 && yi >= self.y as i32 && xi < self.x as i32 + self.w as i32 && yi < self.y as i32 + self.h as i32
    }

    /// The box around both. An empty side is ignored, so this unions like a bounds box.
    pub fn union(self, other: Self) -> Self {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = self.x.saturating_add(self.w).max(other.x.saturating_add(other.w));
        let y1 = self.y.saturating_add(self.h).max(other.y.saturating_add(other.h));
        Self { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
    }
}

/// Straight RGBA8, row by row. One value so a blit stays under the argument cap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PixelImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl PixelImage {
    pub fn empty() -> Self {
        Self { width: 0, height: 0, rgba: Vec::new() }
    }

    pub fn is_ok(&self) -> bool {
        let Some(n) = (self.width as usize).checked_mul(self.height as usize).and_then(|n| n.checked_mul(4)) else {
            return false;
        };
        self.width > 0 && self.height > 0 && self.rgba.len() == n
    }
}

/// Pixels for one keyframe of a raster layer. Shared until a stroke writes.
#[derive(Debug)]
pub struct RasterPlate {
    width: u32,
    height: u32,
    /// One tightly packed RGBA row per scanline.
    rows: Arc<Vec<Arc<Vec<u8>>>>,
    stamp: u64,
    generation: u64,
    /// Rows touched since the tile renderer last caught this `generation`. `None` once it has.
    dirty: Option<(u32, u32)>,
    /// Rows touched since the live stroke texture last showed this `generation`.
    /// Separate from `dirty`: the tiles still need every row of the stroke when the pen lifts.
    preview_dirty: Option<(u32, u32)>,
    /// A flat copy for renderers, rebuilt when `generation` moves. Not part of equality.
    flat: Mutex<Option<(u64, Arc<Pixels>)>>,
    /// Pixels whose alpha is not clear. The timeline's filled dot reads this. Not saved:
    /// loading counts again, and equality follows the pixels.
    ink_count: u32,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    width: u32,
    height: u32,
    #[serde(with = "crate::bitmap::base64_bytes")]
    rgba: Arc<Vec<u8>>,
}

impl Serialize for RasterPlate {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Stored { width: self.width, height: self.height, rgba: Arc::new(self.flatten_bytes()) }.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RasterPlate {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let stored = Stored::deserialize(deserializer)?;
        let pixels = Pixels { width: stored.width, height: stored.height, rgba: (*stored.rgba).clone() };
        RasterPlate::from_pixels(pixels).ok_or_else(|| serde::de::Error::custom("pixel layer is the wrong size"))
    }
}

impl Clone for RasterPlate {
    fn clone(&self) -> Self {
        let flat = self.flat.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        Self { width: self.width, height: self.height, rows: Arc::clone(&self.rows), stamp: self.stamp, generation: self.generation, dirty: self.dirty, preview_dirty: self.preview_dirty, flat: Mutex::new(flat), ink_count: self.ink_count }
    }
}

impl PartialEq for RasterPlate {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width
            && self.height == other.height
            && (Arc::ptr_eq(&self.rows, &other.rows) || self.rows.iter().zip(other.rows.iter()).all(|(a, b)| Arc::ptr_eq(a, b) || a.as_slice() == b.as_slice()))
    }
}

static NEXT_STAMP: AtomicU64 = AtomicU64::new(1);
static UPLOADED: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());

fn fresh_stamp() -> u64 {
    NEXT_STAMP.fetch_add(1, Ordering::Relaxed)
}

/// The GPU has the plate `stamp` at `generation`. A later paint can drop the band it already showed.
pub fn plate_catch_up(stamp: u64, generation: u64) {
    let mut book = UPLOADED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(slot) = book.iter_mut().find(|(id, _)| *id == stamp) {
        if generation > slot.1 {
            slot.1 = generation;
        }
    } else {
        book.push((stamp, generation));
    }
}

fn plate_seen(stamp: u64, generation: u64) -> bool {
    UPLOADED.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().any(|(id, got)| *id == stamp && *got == generation)
}

static PREVIEWED: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());

/// The live stroke texture is showing plate `stamp` at `generation`. The next paint's preview
/// band is only the rows that paint writes. The tile renderer's dirty band is left alone.
pub fn preview_catch_up(stamp: u64, generation: u64) {
    let mut book = PREVIEWED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(slot) = book.iter_mut().find(|(id, _)| *id == stamp) {
        if generation > slot.1 {
            slot.1 = generation;
        }
    } else {
        book.push((stamp, generation));
    }
}

fn preview_seen(stamp: u64, generation: u64) -> bool {
    PREVIEWED.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().any(|(id, got)| *id == stamp && *got == generation)
}

impl RasterPlate {
    /// Whether a plate of this size is allowed. Does not allocate.
    pub fn fits(width: u32, height: u32) -> bool {
        let Some(bytes) = (width as usize).checked_mul(height as usize).and_then(|n| n.checked_mul(4)) else {
            return false;
        };
        bytes > 0 && bytes <= MAX_PLATE_BYTES && width <= MAX_BITMAP_EDGE && height <= MAX_BITMAP_EDGE
    }

    /// A transparent plate. `None` when the size is empty or over the cap.
    pub fn transparent(width: u32, height: u32) -> Option<Self> {
        if !Self::fits(width, height) {
            return None;
        }
        let stride = (width as usize).saturating_mul(4);
        let mut rows = Vec::with_capacity(height as usize);
        for _ in 0..height {
            rows.push(Arc::new(vec![0u8; stride]));
        }
        Some(Self { width, height, rows: Arc::new(rows), stamp: fresh_stamp(), generation: 0, dirty: None, preview_dirty: None, flat: Mutex::new(None), ink_count: 0 })
    }

    /// `None` when the buffer does not match its size or exceeds the cap.
    pub fn from_pixels(pixels: Pixels) -> Option<Self> {
        let (width, height) = (pixels.width, pixels.height);
        let rows = split_rows(pixels)?;
        let ink_count = ink_in_rows(&rows);
        let plate = Self { width, height, rows: Arc::new(rows), stamp: fresh_stamp(), generation: 0, dirty: None, preview_dirty: None, flat: Mutex::new(None), ink_count };
        plate.is_ok().then_some(plate)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// One tightly packed RGBA row, or nothing when `y` is outside the plate.
    pub fn row(&self, y: u32) -> Option<&[u8]> {
        self.rows.get(y as usize).map(|row| row.as_slice())
    }

    /// Whether any pixel has alpha. A clear plate is still an empty frame.
    pub fn has_ink(&self) -> bool {
        self.ink_count > 0
    }

    /// Whether the buffer is a real plate this build will keep.
    pub fn is_ok(&self) -> bool {
        let stride = (self.width as usize).saturating_mul(4);
        self.width > 0
            && self.height > 0
            && self.width <= MAX_BITMAP_EDGE
            && self.height <= MAX_BITMAP_EDGE
            && self.rows.len() == self.height as usize
            && stride > 0
            && self.rows.iter().all(|row| row.len() == stride)
            && stride.saturating_mul(self.height as usize) <= MAX_PLATE_BYTES
    }

    /// Whether `other` still uses this plate's row list. F6 does, until one of them is painted.
    pub fn same_rows(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.rows, &other.rows)
    }

    /// Identity of the row list. A second dab in one stroke keeps it.
    pub fn buffer_id(&self) -> usize {
        Arc::as_ptr(&self.rows) as usize
    }

    /// Bytes of rows that `other` does not already share. The undo budget counts these.
    pub fn unique_row_bytes(&self, other: Option<&Self>) -> usize {
        if other.is_some_and(|o| Arc::ptr_eq(&o.rows, &self.rows)) {
            return 0;
        }
        self.rows.iter().enumerate().filter(|(i, row)| !other.is_some_and(|o| o.rows.get(*i).is_some_and(|r| Arc::ptr_eq(r, row)))).map(|(_, row)| row.len()).sum()
    }

    /// The tile renderer's view of this plate: which texture, which generation, which rows.
    pub fn draw(&self) -> PlateDraw {
        let (y0, y1) = self.dirty.unwrap_or((0, 0));
        PlateDraw { stamp: self.stamp, generation: self.generation, y0, y1 }
    }

    /// Rows the live stroke texture has not shown yet. Empty when it is current.
    pub fn preview(&self) -> PlateDraw {
        let (y0, y1) = self.preview_dirty.unwrap_or((0, 0));
        PlateDraw { stamp: self.stamp, generation: self.generation, y0, y1 }
    }

    /// A flat RGBA buffer. Renderers share one copy until the next write.
    ///
    /// A write that follows the cached generation copies only the dirty rows.
    pub fn image(&self) -> Arc<Pixels> {
        let mut slot = self.flat.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((generation, px)) = slot.as_ref()
            && *generation == self.generation
        {
            return Arc::clone(px);
        }
        let px = self.patch_flat(slot.take()).unwrap_or_else(|| Arc::new(Pixels { width: self.width, height: self.height, rgba: self.flatten_bytes() }));
        *slot = Some((self.generation, Arc::clone(&px)));
        px
    }

    /// New texture identity. Undo and redo call this so a stale render cannot keep the other picture.
    pub fn reissue(&mut self) {
        self.stamp = fresh_stamp();
        self.dirty = Some((0, self.height));
        self.preview_dirty = Some((0, self.height));
    }

    /// Copy the dirty band into the previous flat image when it is exactly one generation behind.
    fn patch_flat(&self, prev: Option<(u64, Arc<Pixels>)>) -> Option<Arc<Pixels>> {
        let (generation, px) = prev?;
        if generation.wrapping_add(1) != self.generation || px.width != self.width || px.height != self.height {
            return None;
        }
        let (y0, y1) = self.dirty?;
        if y1 <= y0 || y1 > self.height {
            return None;
        }
        let stride = (self.width as usize).saturating_mul(4);
        if stride == 0 || px.rgba.len() != stride.saturating_mul(self.height as usize) {
            return None;
        }
        let mut owned = Arc::try_unwrap(px).unwrap_or_else(|px| Pixels { width: px.width, height: px.height, rgba: px.rgba.clone() });
        for y in y0..y1 {
            let Some(src) = self.rows.get(y as usize) else { return None };
            let Some(at) = (y as usize).checked_mul(stride) else { return None };
            let Some(dst) = owned.rgba.get_mut(at..at + stride) else { return None };
            if src.len() != stride {
                return None;
            }
            dst.copy_from_slice(src);
        }
        Some(Arc::new(owned))
    }

    /// Copy the overlapping top-left of `old` into this plate. Used when the stage size changes.
    pub fn copy_from_corner(&mut self, old: &Self) {
        {
            let rows = Arc::make_mut(&mut self.rows);
            let rows_n = (old.height as usize).min(rows.len());
            let cols = (old.width as usize).min(self.width as usize).saturating_mul(4);
            for y in 0..rows_n {
                let Some(src_row) = old.rows.get(y) else { continue };
                let Some(src) = src_row.get(..cols) else { continue };
                let Some(dst) = rows.get_mut(y) else { continue };
                if Arc::strong_count(dst) > 1 {
                    let mut next = dst.as_slice().to_vec();
                    if let Some(out) = next.get_mut(..cols) {
                        out.copy_from_slice(src);
                    }
                    *dst = Arc::new(next);
                } else if let Some(out) = Arc::get_mut(dst).and_then(|buf| buf.get_mut(..cols)) {
                    out.copy_from_slice(src);
                }
            }
        }
        self.ink_count = ink_in_rows(&self.rows);
    }

    /// Run `paint` on rows `y0..y1` (full width). The closure sees a contiguous band whose
    /// first row is `y0` of the plate. Nothing is kept when `paint` returns false.
    ///
    /// `fork_stamp` is set when another key still shares this row list: the write takes a
    /// new GPU texture so that key keeps the old one. A render job holding the list does not.
    pub fn paint_band(&mut self, y0: u32, y1: u32, fork_stamp: bool, paint: impl FnOnce(&mut [u8]) -> bool) -> bool {
        let y0 = y0.min(self.height);
        let y1 = y1.min(self.height);
        if y1 <= y0 {
            return false;
        }
        let stride = (self.width as usize).saturating_mul(4);
        let n = (y1 - y0) as usize;
        let Some(bytes) = n.checked_mul(stride) else { return false };
        let mut tmp = vec![0u8; bytes];
        for (i, y) in (y0..y1).enumerate() {
            let Some(row) = self.rows.get(y as usize) else { return false };
            let Some(dst) = tmp.get_mut(i * stride..(i + 1) * stride) else { return false };
            if row.len() != stride {
                return false;
            }
            dst.copy_from_slice(row);
        }
        let before = count_ink(&tmp);
        if !paint(&mut tmp) {
            return false;
        }
        let after = count_ink(&tmp);
        let shared_vec = Arc::strong_count(&self.rows) > 1;
        let rows = Arc::make_mut(&mut self.rows);
        for (i, y) in (y0..y1).enumerate() {
            let Some(src) = tmp.get(i * stride..(i + 1) * stride) else { continue };
            let Some(row) = rows.get_mut(y as usize) else { continue };
            if row.as_slice() == src {
                continue;
            }
            // A render job can clone the row between the count and the write. A failed
            // in-place write still has to land, or that scanline stays blank.
            if let Some(buf) = Arc::get_mut(row) {
                buf.copy_from_slice(src);
            } else {
                *row = Arc::new(src.to_vec());
            }
        }
        if shared_vec && fork_stamp {
            self.stamp = fresh_stamp();
            self.generation = 0;
            self.dirty = Some((0, self.height));
            self.preview_dirty = None;
        }
        self.ink_count = self.ink_count.saturating_sub(before).saturating_add(after);
        self.mark(y0, y1);
        true
    }

    fn mark(&mut self, y0: u32, y1: u32) {
        if plate_seen(self.stamp, self.generation) {
            self.dirty = None;
        }
        if preview_seen(self.stamp, self.generation) {
            self.preview_dirty = None;
        }
        self.generation = self.generation.wrapping_add(1);
        let grow = |band: Option<(u32, u32)>| {
            Some(match band {
                Some((a, b)) => (a.min(y0), b.max(y1)),
                None => (y0, y1),
            })
        };
        self.dirty = grow(self.dirty);
        self.preview_dirty = grow(self.preview_dirty);
    }

    fn flatten_bytes(&self) -> Vec<u8> {
        let stride = (self.width as usize).saturating_mul(4);
        let mut rgba = Vec::with_capacity(stride.saturating_mul(self.height as usize));
        for row in self.rows.iter() {
            rgba.extend_from_slice(row);
        }
        rgba
    }

    /// Alpha at one texel, or 0 when it is outside the plate. O(1): safe on a hover.
    pub fn alpha_at(&self, x: u32, y: u32) -> u8 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        self.rows.get(y as usize).and_then(|row| row.get(x as usize * 4 + 3)).copied().unwrap_or(0)
    }

    /// Tight box around texels whose alpha is not clear. `None` when the plate is clear.
    ///
    /// Call this when the user selects, not on every frame: a full plate is the whole stage.
    pub fn opaque_bounds(&self) -> Option<PixelRect> {
        if self.ink_count == 0 {
            return None;
        }
        self.opaque_bounds_in(PixelRect { x: 0, y: 0, w: self.width, h: self.height })
    }

    /// [`RasterPlate::opaque_bounds`] inside `window` only. The clear margin is left out.
    /// Alpha 0 does not count, even when the colour channels are set.
    pub fn opaque_bounds_in(&self, window: PixelRect) -> Option<PixelRect> {
        let window = window.clip(self.width, self.height)?;
        let mut min_x = u32::MAX;
        let mut min_y = u32::MAX;
        let mut max_x = 0u32;
        let mut max_y = 0u32;
        for y in window.y..window.y + window.h {
            let Some(row) = self.rows.get(y as usize) else { continue };
            for x in window.x..window.x + window.w {
                if row.get(x as usize * 4 + 3).copied().unwrap_or(0) == 0 {
                    continue;
                }
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x + 1);
                max_y = max_y.max(y + 1);
            }
        }
        if min_x == u32::MAX {
            None
        } else {
            Some(PixelRect { x: min_x, y: min_y, w: max_x - min_x, h: max_y - min_y })
        }
    }

    /// A copy of `rect`. `rect` must already lie on the plate, so the image's origin is `rect`.
    pub fn copy_rect(&self, rect: PixelRect) -> Option<PixelImage> {
        if rect.is_empty() || rect.x.checked_add(rect.w)? > self.width || rect.y.checked_add(rect.h)? > self.height {
            return None;
        }
        let row_bytes = rect.w as usize * 4;
        let mut rgba = vec![0u8; row_bytes * rect.h as usize];
        for row in 0..rect.h as usize {
            let src = self.rows.get(rect.y as usize + row)?;
            let start = rect.x as usize * 4;
            let bytes = src.get(start..start + row_bytes)?;
            let dst = rgba.get_mut(row * row_bytes..row * row_bytes + row_bytes)?;
            dst.copy_from_slice(bytes);
        }
        Some(PixelImage { width: rect.w, height: rect.h, rgba })
    }

    /// Sets `rect` to clear. False when it misses the plate or was already clear.
    pub fn clear_rect(&mut self, rect: PixelRect, fork_stamp: bool) -> bool {
        let Some(rect) = rect.clip(self.width, self.height) else { return false };
        let plate_w = self.width;
        self.paint_band(rect.y, rect.y.saturating_add(rect.h), fork_stamp, |band| {
            let stride = plate_w as usize * 4;
            let mut changed = false;
            for row in 0..rect.h as usize {
                let start = row * stride + rect.x as usize * 4;
                let n = rect.w as usize * 4;
                let Some(px) = band.get_mut(start..start + n) else { return false };
                if px.iter().any(|b| *b != 0) {
                    px.fill(0);
                    changed = true;
                }
            }
            changed
        })
    }

    /// Writes `src` so its top-left is `(x, y)`. Negative coordinates clip. False when the
    /// image is the wrong size, misses the plate, or matches what is already there.
    pub fn blit_at(&mut self, x: i32, y: i32, src: &PixelImage, fork_stamp: bool) -> bool {
        if !src.is_ok() {
            return false;
        }
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = x.saturating_add(src.width as i32).min(self.width as i32);
        let y1 = y.saturating_add(src.height as i32).min(self.height as i32);
        if x1 <= x0 || y1 <= y0 {
            return false;
        }
        let src_x = (x0 - x) as u32;
        let src_y = (y0 - y) as u32;
        let dw = (x1 - x0) as u32;
        let dh = (y1 - y0) as u32;
        let plate_w = self.width;
        self.paint_band(y0 as u32, y1 as u32, fork_stamp, |band| {
            let stride = plate_w as usize * 4;
            let sw = src.width as usize;
            let mut changed = false;
            for row in 0..dh as usize {
                let sy = src_y as usize + row;
                let src_off = sy * sw * 4 + src_x as usize * 4;
                let n = dw as usize * 4;
                let Some(from) = src.rgba.get(src_off..src_off + n) else { return false };
                let dst_off = row * stride + x0 as usize * 4;
                let Some(to) = band.get_mut(dst_off..dst_off + n) else { return false };
                if to != from {
                    to.copy_from_slice(from);
                    changed = true;
                }
            }
            changed
        })
    }
}

fn count_ink(bytes: &[u8]) -> u32 {
    let n = bytes.as_chunks::<4>().0.iter().filter(|px| px[3] != 0).count();
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn ink_in_rows(rows: &[Arc<Vec<u8>>]) -> u32 {
    rows.iter().fold(0u32, |n, row| n.saturating_add(count_ink(row)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(plate: &mut RasterPlate, x: u32, y: u32, px: [u8; 4]) {
        let _ = plate.paint_band(y, y + 1, false, |band| {
            let i = x as usize * 4;
            let Some(dst) = band.get_mut(i..i + 4) else { return false };
            dst.copy_from_slice(&px);
            true
        });
    }

    #[test]
    fn a_clear_margin_is_left_out_and_alpha_zero_does_not_count() {
        let mut plate = RasterPlate::transparent(8, 6).expect("plate");
        put(&mut plate, 1, 1, [9, 0, 0, 0]);
        assert!(!plate.has_ink());
        assert!(plate.opaque_bounds().is_none());
        put(&mut plate, 3, 2, [255, 0, 0, 255]);
        put(&mut plate, 4, 4, [0, 255, 0, 255]);
        assert_eq!(plate.opaque_bounds(), Some(PixelRect { x: 3, y: 2, w: 2, h: 3 }));
        assert_eq!(plate.opaque_bounds_in(PixelRect { x: 0, y: 0, w: 4, h: 4 }), Some(PixelRect { x: 3, y: 2, w: 1, h: 1 }));
    }

    #[test]
    fn clear_and_blit_move_the_ink_and_the_count_follows() {
        let mut plate = RasterPlate::transparent(8, 8).expect("plate");
        put(&mut plate, 1, 2, [255, 0, 0, 255]);
        let bounds = plate.opaque_bounds().expect("ink");
        let image = plate.copy_rect(bounds).expect("copy");
        assert!(plate.clear_rect(bounds, false));
        assert!(!plate.has_ink());
        assert_eq!(plate.alpha_at(1, 2), 0);
        assert!(plate.blit_at(5, 6, &image, false));
        assert!(plate.has_ink());
        assert_eq!(plate.alpha_at(5, 6), 255);
        assert_eq!(plate.opaque_bounds(), Some(PixelRect { x: 5, y: 6, w: 1, h: 1 }));
        let wide = PixelImage { width: 2, height: 1, rgba: vec![0, 0, 255, 255, 0, 255, 0, 255] };
        assert!(plate.blit_at(-1, 0, &wide, false));
        assert_eq!(plate.alpha_at(0, 0), 255);
        assert!(!plate.blit_at(0, 0, &PixelImage { width: 1, height: 1, rgba: vec![1, 2, 3] }, false));
    }

    #[test]
    fn the_live_preview_drops_rows_it_already_showed_and_the_tiles_keep_the_stroke() {
        let mut plate = RasterPlate::transparent(4, 8).expect("plate");
        put(&mut plate, 1, 1, [0, 0, 0, 255]);
        let first = plate.preview();
        assert_eq!((first.y0, first.y1), (1, 2));
        preview_catch_up(first.stamp, first.generation);
        put(&mut plate, 1, 4, [0, 0, 0, 255]);
        let next = plate.preview();
        assert_eq!((next.y0, next.y1, next.generation), (4, 5, first.generation + 1));
        let owed = plate.draw();
        assert_eq!((owed.y0, owed.y1), (1, 5), "the tile renderer still owes every row of the stroke");
    }
}

fn split_rows(pixels: Pixels) -> Option<Vec<Arc<Vec<u8>>>> {
    let stride = (pixels.width as usize).checked_mul(4)?;
    let need = stride.checked_mul(pixels.height as usize)?;
    if stride == 0 || pixels.rgba.len() != need {
        return None;
    }
    let mut rows = Vec::with_capacity(pixels.height as usize);
    for y in 0..pixels.height as usize {
        let start = y * stride;
        let Some(row) = pixels.rgba.get(start..start + stride) else { return None };
        rows.push(Arc::new(row.to_vec()));
    }
    Some(rows)
}
