//! The four spray pictures, stamped whole and turned per dab.
//!
//! Each file is a square of coverage: width and height as little-endian `u32`,
//! then one byte per pixel, 0 clear and 255 solid. The bytes are the picture
//! the user supplied, cropped to the ink and centred. A dab samples that
//! picture, scaled to the brush diameter and spun by [`dab_turn`], so the
//! specks do not land in the same place twice. Grain and the round disc are
//! not consulted.

use std::path::Path;

use crate::dab::{blend_cover, ceil_i32, floor_i32};

/// Which supplied picture a dab uses. `None` is the round or grain stamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Picture {
    None,
    /// Sparse mist, darker in the middle.
    Fine,
    /// A solid core and a wide speckled edge.
    Heavy,
    /// A small hard dot with a short fringe.
    Tight,
    /// A solid core and a shorter speckled edge.
    Can,
}

impl Picture {
    /// `0` is none. `1` to `4` are fine, heavy, tight, can. Anything else is none.
    pub fn from_index(n: u32) -> Self {
        match n {
            1 => Self::Fine,
            2 => Self::Heavy,
            3 => Self::Tight,
            4 => Self::Can,
            _ => Self::None,
        }
    }

    /// The index [`Self::from_index`] reads back.
    pub fn index(self) -> u32 {
        match self {
            Self::None => 0,
            Self::Fine => 1,
            Self::Heavy => 2,
            Self::Tight => 3,
            Self::Can => 4,
        }
    }

    fn file(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Fine => Some("fine.bin"),
            Self::Heavy => Some("heavy.bin"),
            Self::Tight => Some("tight.bin"),
            Self::Can => Some("can.bin"),
        }
    }
}

/// Gap between picture dabs, in pixels. The same step as a round dab, so the
/// stroke is one mark. Overlaps keep the stronger sample.
pub fn picture_spacing(diameter: f32) -> f32 {
    crate::stroke::dab_spacing(diameter)
}

/// A turn for this dab, in radians. The same place and index always get the
/// same turn, and a neighbouring dab does not.
pub fn dab_turn(x: f32, y: f32, index: usize) -> f32 {
    let qx = if x.is_finite() { (f64::from(x) * 17.0).round() as i32 } else { 0 };
    let qy = if y.is_finite() { (f64::from(y) * 17.0).round() as i32 } else { 0 };
    let mut z = (qx as u32).wrapping_mul(0x9E37_79B9) ^ (qy as u32).wrapping_mul(0x85EB_CA6B) ^ (index as u32).wrapping_mul(0xC2B2_AE35);
    z = (z ^ (z >> 16)).wrapping_mul(0x7FEB_352D);
    z = (z ^ (z >> 15)).wrapping_mul(0x846C_A68B);
    z ^= z >> 16;
    (z as f64 / f64::from(u32::MAX) * std::f64::consts::TAU) as f32
}

struct Level {
    w: u32,
    h: u32,
    cover: Vec<u8>,
}

struct Map {
    levels: Vec<Level>,
}

struct Library {
    maps: [Option<Map>; 4],
}

fn empty_library() -> Library {
    Library { maps: [None, None, None, None] }
}

fn library() -> &'static std::sync::OnceLock<Library> {
    static LIB: std::sync::OnceLock<Library> = std::sync::OnceLock::new();
    &LIB
}

/// How many of the four pictures are loaded.
pub fn pictures_loaded() -> usize {
    library().get().map(|lib| lib.maps.iter().filter(|m| m.is_some()).count()).unwrap_or(0)
}

/// Read `fine.bin`, `heavy.bin`, `tight.bin` and `can.bin` from `dir`.
/// A second call does nothing and returns the count already loaded.
pub fn load_pictures(dir: &Path) -> usize {
    if let Some(lib) = library().get() {
        return lib.maps.iter().filter(|m| m.is_some()).count();
    }
    let mut lib = empty_library();
    for picture in [Picture::Fine, Picture::Heavy, Picture::Tight, Picture::Can] {
        let Some(name) = picture.file() else { continue };
        let Some(map) = read_tip(&dir.join(name)) else { continue };
        let Some(slot) = lib.maps.get_mut(picture.index().saturating_sub(1) as usize) else { continue };
        *slot = Some(map);
    }
    let n = lib.maps.iter().filter(|m| m.is_some()).count();
    let _ = library().set(lib);
    n
}

fn map_of(picture: Picture) -> Option<&'static Map> {
    let index = picture.index().checked_sub(1)? as usize;
    library().get()?.maps.get(index)?.as_ref()
}

/// Stamp `picture` into the coverage mask. The picture's own grey is the
/// coverage. `turn` is radians. `flow` of 1 keeps the stronger sample.
pub fn stamp_picture(mask: &mut [u8], width: u32, height: u32, x: f32, y: f32, diameter: f32, turn: f32, flow: f32, opacity: f32, picture: Picture) -> bool {
    if picture == Picture::None || !x.is_finite() || !y.is_finite() {
        return false;
    }
    let Some(map) = map_of(picture) else { return false };
    let Some(level) = level_for(map, f64::from(diameter)) else { return false };
    paint_level(mask, width, height, f64::from(x), f64::from(y), f64::from(diameter), f64::from(turn), flow, f64::from(opacity), level)
}

fn read_tip(path: &Path) -> Option<Map> {
    let bytes = std::fs::read(path).ok()?;
    let head = bytes.get(..8)?;
    let w = u32::from_le_bytes(head.get(..4)?.try_into().ok()?);
    let h = u32::from_le_bytes(head.get(4..8)?.try_into().ok()?);
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return None;
    }
    let n = (w as usize).checked_mul(h as usize)?;
    let cover = bytes.get(8..8 + n)?.to_vec();
    Map::from_cover(w, h, cover)
}

impl Map {
    fn from_cover(w: u32, h: u32, cover: Vec<u8>) -> Option<Self> {
        let n = (w as usize).checked_mul(h as usize)?;
        if cover.len() != n || w == 0 || h == 0 {
            return None;
        }
        let mut levels = vec![Level { w, h, cover }];
        push_mips(&mut levels);
        Some(Self { levels })
    }
}

fn push_mips(levels: &mut Vec<Level>) {
    loop {
        let prev_i = levels.len().saturating_sub(1);
        let Some(prev) = levels.get(prev_i) else { return };
        if prev.w <= 1 && prev.h <= 1 {
            return;
        }
        let pw = prev.w;
        let ph = prev.h;
        let nw = (pw / 2).max(1);
        let nh = (ph / 2).max(1);
        if nw == pw && nh == ph {
            return;
        }
        let mut cover = vec![0u8; (nw as usize).saturating_mul(nh as usize)];
        for y in 0..nh {
            let y0 = y * ph / nh;
            let y1 = ((y + 1) * ph / nh).max(y0 + 1);
            for x in 0..nw {
                let x0 = x * pw / nw;
                let x1 = ((x + 1) * pw / nw).max(x0 + 1);
                let mut sum = 0u64;
                let mut count = 0u64;
                for yy in y0..y1 {
                    for xx in x0..x1 {
                        let i = (yy as usize).saturating_mul(pw as usize).saturating_add(xx as usize);
                        if let Some(p) = levels.get(prev_i).and_then(|level| level.cover.get(i)) {
                            sum += u64::from(*p);
                            count += 1;
                        }
                    }
                }
                if count > 0 {
                    let i = (y as usize).saturating_mul(nw as usize).saturating_add(x as usize);
                    if let Some(dst) = cover.get_mut(i) {
                        *dst = ((sum + count / 2) / count) as u8;
                    }
                }
            }
        }
        levels.push(Level { w: nw, h: nh, cover });
    }
}

fn level_for(map: &Map, diameter: f64) -> Option<&Level> {
    if !diameter.is_finite() || diameter < 0.5 {
        return None;
    }
    let mut chosen = 0usize;
    for (i, level) in map.levels.iter().enumerate().skip(1) {
        if f64::from(level.w) >= diameter && f64::from(level.h) >= diameter {
            chosen = i;
        } else {
            break;
        }
    }
    map.levels.get(chosen)
}

fn paint_level(mask: &mut [u8], width: u32, height: u32, cx: f64, cy: f64, diameter: f64, turn: f64, flow: f32, opacity: f64, level: &Level) -> bool {
    let Some(need) = (width as usize).checked_mul(height as usize) else { return false };
    if mask.len() != need || need == 0 || !diameter.is_finite() || diameter < 0.5 {
        return false;
    }
    let opacity = if opacity.is_finite() { opacity.clamp(0.0, 1.0) } else { 0.0 };
    if opacity <= 0.0 {
        return false;
    }
    let turn = if turn.is_finite() { turn } else { 0.0 };
    let rad = diameter * 0.5;
    let Some(mut x0) = floor_i32(cx - rad) else { return false };
    let Some(mut y0) = floor_i32(cy - rad) else { return false };
    let Some(mut x1) = ceil_i32(cx + rad).and_then(|v| v.checked_add(1)) else { return false };
    let Some(mut y1) = ceil_i32(cy + rad).and_then(|v| v.checked_add(1)) else { return false };
    let Some(width_i) = i32::try_from(width).ok() else { return false };
    let Some(height_i) = i32::try_from(height).ok() else { return false };
    x0 = x0.max(0);
    y0 = y0.max(0);
    x1 = x1.min(width_i);
    y1 = y1.min(height_i);
    if x1 <= x0 || y1 <= y0 {
        return false;
    }
    let (sin, cos) = turn.sin_cos();
    let mut hit = false;
    for y in y0..y1 {
        for x in x0..x1 {
            let dx = (f64::from(x) + 0.5) - cx;
            let dy = (f64::from(y) + 0.5) - cy;
            // Inverse of the dab's turn, so the picture itself is what spins.
            let rx = dx * cos + dy * sin;
            let ry = -dx * sin + dy * cos;
            let u = rx / diameter + 0.5;
            let v = ry / diameter + 0.5;
            let sample = cover_at(level, u, v) * opacity;
            if sample <= 0.0 {
                continue;
            }
            hit |= blend_cover(mask, width, x, y, sample, flow);
        }
    }
    hit
}

fn cover_at(level: &Level, u: f64, v: f64) -> f64 {
    if !(u >= 0.0 && v >= 0.0 && u <= 1.0 && v <= 1.0) || level.w == 0 || level.h == 0 {
        return 0.0;
    }
    let x = u * f64::from(level.w.saturating_sub(1));
    let y = v * f64::from(level.h.saturating_sub(1));
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = x0.saturating_add(1).min(level.w.saturating_sub(1));
    let y1 = y0.saturating_add(1).min(level.h.saturating_sub(1));
    let tx = x - f64::from(x0);
    let ty = y - f64::from(y0);
    let at = |px: u32, py: u32| -> f64 {
        let i = (py as usize).saturating_mul(level.w as usize).saturating_add(px as usize);
        f64::from(level.cover.get(i).copied().unwrap_or(0)) / 255.0
    };
    let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx;
    let bot = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx;
    top + (bot - top) * ty
}

#[cfg(test)]
mod tests {
    use super::*;

    fn half_map(side: u32) -> Map {
        let mut cover = vec![0u8; (side as usize) * (side as usize)];
        for y in 0..side {
            for x in 0..side / 2 {
                let i = (y as usize) * (side as usize) + x as usize;
                if let Some(p) = cover.get_mut(i) {
                    *p = 255;
                }
            }
        }
        Map::from_cover(side, side, cover).unwrap_or_else(|| Map { levels: Vec::new() })
    }

    fn ink(mask: &[u8]) -> usize {
        mask.iter().filter(|b| **b > 0).count()
    }

    fn at(mask: &[u8], x: usize, y: usize) -> u8 {
        mask.get(y * 48 + x).copied().unwrap_or(0)
    }

    #[test]
    fn a_turned_picture_is_not_the_same_stamp() {
        let map = half_map(32);
        let level = level_for(&map, 24.0);
        assert!(level.is_some(), "the half picture has no level");
        let mut upright = vec![0u8; 48 * 48];
        let mut spun = vec![0u8; 48 * 48];
        if let Some(level) = level {
            assert!(paint_level(&mut upright, 48, 48, 24.0, 24.0, 24.0, 0.0, 1.0, 1.0, level));
            assert!(paint_level(&mut spun, 48, 48, 24.0, 24.0, 24.0, 1.2, 1.0, 1.0, level));
        }
        assert_ne!(upright, spun);
        assert!(ink(&upright) > 20 && ink(&upright) < 400, "upright ink {}", ink(&upright));
        // The square picture's clear half stays clear: the right side of an unturned stamp.
        assert_eq!(at(&upright, 32, 24), 0);
        assert!(at(&upright, 16, 24) > 200);
    }

    #[test]
    fn neighbouring_dabs_do_not_share_a_turn() {
        let a = dab_turn(10.0, 12.0, 0);
        let b = dab_turn(40.0, 12.0, 1);
        assert!(a.is_finite() && b.is_finite());
        assert!((a - b).abs() > 0.05, "{a} {b}");
        assert_eq!(dab_turn(10.0, 12.0, 0), a);
    }

    #[test]
    fn picture_spacing_matches_a_round_dab() {
        assert!((picture_spacing(100.0) - 7.0).abs() < 0.01);
        assert!((picture_spacing(0.2) - 0.5).abs() < 0.01);
    }

    #[test]
    fn overlapping_pictures_keep_the_stronger_sample() {
        let map = Map::from_cover(16, 16, vec![128u8; 16 * 16]);
        assert!(map.is_some(), "flat picture");
        let mut mask = vec![0u8; 48 * 48];
        if let Some(map) = map
            && let Some(level) = level_for(&map, 16.0)
        {
            assert!(paint_level(&mut mask, 48, 48, 16.0, 24.0, 16.0, 0.0, 1.0, 1.0, level));
            assert!(paint_level(&mut mask, 48, 48, 20.0, 24.0, 16.0, 0.7, 1.0, 1.0, level));
        }
        let peak = mask.iter().copied().max().unwrap_or(0);
        assert!(peak > 100 && peak <= 128, "overlap climbed to {peak}");
    }
}
