//! One raster crop at a time (ADR 0115).
//!
//! A plate is a stage-sized picture, not an element. The Selection tool treats the ink as one
//! rectangle: the tight box around pixels whose alpha is not clear. Move, rotate, scale,
//! duplicate, delete and Convert to Symbol go through the commands they already use.
//!
//! Later tools should call the functions here. They should not scan plate rows themselves.
//! The document crate only copies, clears and blits rectangles. This module is the only one
//! that knows what a crop is.
//!
//! A drag lifts the crop once and resamples that original. An integer move copies bytes.
//! Anything else uses [`sample`], one straight-alpha bilinear function, so a later filter
//! replaces that function and nothing else. Aligning a crop is the next hook: call
//! [`apply_affine`] with the same stage-space delta the elements get.
//!
//! Not in this slice: the clipboard (duplicate is), a crop on more than one layer, picking
//! through the camera, and Edit Multiple Frames. One rectangle includes the clear pixels
//! inside it. A marquee narrows it.

use crate::{EngineError, Result, Session};
use anicraft_doc::{Bitmap, BitmapId, ColorEffect, Element, ElementId, ElementKind, FolderId, LayerId, LayerKind, LibraryEntry, LibraryItem, Paint, PixelImage, PixelRect, RasterPlate, Shape};
use anicraft_geom::{Affine, FillRule, Path, Rect, Vec2};
use serde_json::{Value, json};
use std::sync::Arc;

/// The crop the Selection tool is holding. One, until it becomes a symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RasterPick {
    pub layer: LayerId,
    /// Keyframe start, so a later frame of the same span still owns this crop.
    pub key: u32,
    pub rect: PixelRect,
}

/// What a stage click landed on. A plate is not an element, so it is its own case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageHit {
    Element(LayerId, ElementId),
    Raster(LayerId),
}

/// The picture lifted for the open drag. Not an undo snapshot: the plate is.
pub(crate) struct Lift {
    layer: LayerId,
    key: u32,
    origin: PixelRect,
    sprite: PixelImage,
    /// Where the sprite is stamped, clipped to the plate. `None` before the first stamp
    /// and while the whole sprite is off the plate.
    placed: Option<PixelRect>,
    /// What was under `placed`, so the next move can put it back.
    under: PixelImage,
    /// The composed transform from `origin` to the current stamp.
    total: Affine,
    /// True when this drag copies. [`Lift::hole`] is the live flag (`!copy`). Kept so a
    /// later tool can tell a copy from a move without inferring it.
    #[allow(dead_code)]
    copy: bool,
    /// The origin still has to be cleared. A copy leaves it.
    hole: bool,
    stamp_x: i32,
    stamp_y: i32,
}

struct Dest {
    image: PixelImage,
    stamp_x: i32,
    stamp_y: i32,
    visible: Option<PixelRect>,
}

impl Session {
    pub fn raster_pick(&self) -> Option<&RasterPick> {
        self.raster_pick.as_ref()
    }

    /// Topmost thing under `p`. On a raster layer the plate is tested before that layer's
    /// elements, because the plate is drawn on top of them. A plate item is never a hit:
    /// its rectangle is the whole stage.
    pub fn stage_hit(&self, p: Vec2) -> Option<StageHit> {
        let ids: Vec<(LayerId, bool)> = self.timeline().layers.iter().map(|l| (l.id, l.kind == LayerKind::Raster)).collect();
        let dl = self.display_list();
        for (id, raster) in ids {
            if !self.selectable(id) {
                continue;
            }
            if raster && raster_alpha(self, id, p) {
                return Some(StageHit::Raster(id));
            }
            if let Some(i) = crate::pick_index(&dl, p, 2.0, |_, it| it.layer == id && it.plate.is_none())
                && let Some(it) = dl.items.get(i)
            {
                return Some(StageHit::Element(it.layer, it.element));
            }
        }
        None
    }

    /// Whether the pointer is on the crop, or on opaque ink. The cursor asks this every
    /// frame, so it does not scan a plate: alpha is one byte.
    pub fn raster_hot(&self, p: Vec2) -> bool {
        if self.raster_pick.is_some_and(|pick| pick.rect.contains_stage(p.x, p.y)) {
            return true;
        }
        let ids: Vec<LayerId> = self.timeline().layers.iter().filter(|l| l.kind == LayerKind::Raster).map(|l| l.id).collect();
        ids.into_iter().any(|id| self.selectable(id) && raster_alpha(self, id, p))
    }
}

fn raster_alpha(s: &Session, id: LayerId, p: Vec2) -> bool {
    let (x, y) = (p.x.floor(), p.y.floor());
    if x < 0.0 || y < 0.0 || x >= u32::MAX as f32 || y >= u32::MAX as f32 {
        return false;
    }
    s.timeline().layer(id).and_then(|l| l.keyframe_at(s.frame)).and_then(|k| k.plate.as_ref()).is_some_and(|plate| plate.alpha_at(x as u32, y as u32) != 0)
}

pub(crate) fn clear_pick(s: &mut Session) {
    s.raster_pick = None;
    s.raster_lift = None;
}

/// A click on opaque ink. The crop becomes the whole plate's ink, unless the click is
/// already inside the crop (a marquee's subset stays). Shift unions, or lets go when the
/// crop is already that whole box.
pub(crate) fn click(s: &mut Session, layer: LayerId, p: Vec2, add: bool) -> Value {
    let Some(key) = s.timeline().layer(layer).and_then(|l| l.keyframe_at(s.frame)).map(|k| k.start) else {
        return json!({"element": null});
    };
    let full = key_plate(s, layer, key).and_then(RasterPlate::opaque_bounds);
    let inside = s.raster_pick.is_some_and(|pick| pick.layer == layer && pick.key == key && pick.rect.contains_stage(p.x, p.y));
    if !add {
        s.selection.clear();
        if inside {
            s.layer = layer;
            return json!({"layer": layer.0, "raster": true});
        }
        s.raster_lift = None;
        return match full {
            Some(rect) => {
                s.raster_pick = Some(RasterPick { layer, key, rect });
                s.layer = layer;
                json!({"layer": layer.0, "raster": true, "w": rect.w, "h": rect.h})
            }
            None => {
                s.raster_pick = None;
                json!({"element": null})
            }
        };
    }
    let already = s.raster_pick.is_some_and(|pick| pick.layer == layer && pick.key == key && full.is_some_and(|full| pick.rect == full));
    if inside && already {
        s.raster_pick = None;
        s.raster_lift = None;
        return json!({"raster": false});
    }
    let Some(rect) = full else { return json!({"element": null}) };
    add_pick(s, RasterPick { layer, key, rect });
    s.layer = layer;
    json!({"layer": layer.0, "raster": true, "w": rect.w, "h": rect.h})
}

/// Opaque ink inside `stage`, on the topmost raster layer that has any.
pub(crate) fn bounds_in(s: &Session, stage: Rect) -> Option<RasterPick> {
    let ids: Vec<LayerId> = s.timeline().layers.iter().filter(|l| l.kind == LayerKind::Raster).map(|l| l.id).collect();
    for id in ids {
        if !s.selectable(id) {
            continue;
        }
        let Some(layer) = s.timeline().layer(id) else { continue };
        let Some(k) = layer.keyframe_at(s.frame) else { continue };
        let Some(plate) = k.plate.as_ref() else { continue };
        let Some(window) = pixel_window(stage, plate.width(), plate.height()) else { continue };
        if let Some(rect) = plate.opaque_bounds_in(window) {
            return Some(RasterPick { layer: id, key: k.start, rect });
        }
    }
    None
}

pub(crate) fn add_pick(s: &mut Session, next: RasterPick) {
    s.raster_lift = None;
    s.raster_pick = Some(match s.raster_pick {
        Some(cur) if cur.layer == next.layer && cur.key == next.key => RasterPick { rect: cur.rect.union(next.rect), ..next },
        _ => next,
    });
}

/// Drops the crop when its layer, key or ink is gone. A drag that has walked off the plate
/// keeps its lift, so coming back still has the original picture.
pub(crate) fn fix(s: &mut Session) {
    let Some(pick) = s.raster_pick else {
        if !s.in_gesture() {
            s.raster_lift = None;
        }
        return;
    };
    let dragging = s.in_gesture() && s.raster_lift.is_some();
    let frame = s.frame;
    let live = s.timeline().layer(pick.layer).is_some_and(|layer| {
        layer.kind == LayerKind::Raster
            && layer.keyframe_at(frame).is_some_and(|k| k.start == pick.key && (k.plate.as_ref().is_some_and(|p| p.has_ink()) || dragging))
    });
    if s.selectable(pick.layer) && live {
        return;
    }
    s.raster_pick = None;
    if !s.in_gesture() {
        s.raster_lift = None;
    }
}

/// Tightens the crop to the ink still inside it. Called when a drag ends, so a rotated
/// box loses the clear corners it grew.
pub(crate) fn shrink(s: &mut Session) {
    let Some(pick) = s.raster_pick else { return };
    match key_plate(s, pick.layer, pick.key).and_then(|plate| plate.opaque_bounds_in(pick.rect)) {
        Some(rect) => {
            if let Some(cur) = s.raster_pick.as_mut() {
                cur.rect = rect;
            }
        }
        None => s.raster_pick = None,
    }
}

/// `delta` in stage space, applied to the crop the way `modify.transform` applies it to
/// elements. Inside a gesture the original picture is resampled; outside, one resample.
pub(crate) fn apply_affine(s: &mut Session, delta: &Affine) -> Result<bool> {
    if s.raster_pick.is_none() {
        return Ok(false);
    }
    if near_identity(delta) {
        return Ok(false);
    }
    if !delta.is_finite() || delta.determinant().abs() < 1e-8 {
        return Err(EngineError::Param("the transform is degenerate (it would collapse the artwork)".into()));
    }
    if s.raster_lift.is_none() {
        begin_lift(s, false)?;
    }
    let total = s.raster_lift.as_ref().map(|lift| delta.mul(&lift.total)).ok_or_else(|| EngineError::State("the pixel crop is gone".into()))?;
    restamp(s, total)?;
    if !s.in_gesture() {
        s.raster_lift = None;
        shrink(s);
    }
    Ok(true)
}

pub(crate) fn nudge(s: &mut Session, dx: f32, dy: f32) -> Result<bool> {
    apply_affine(s, &Affine::translate(dx, dy))
}

/// A second crop. The original stays. Outside a gesture the lift is dropped in this command.
pub(crate) fn duplicate(s: &mut Session, dx: f32, dy: f32) -> Result<bool> {
    if s.raster_pick.is_none() {
        return Ok(false);
    }
    s.raster_lift = None;
    begin_lift(s, true)?;
    restamp(s, Affine::translate(dx, dy))?;
    if !s.in_gesture() {
        s.raster_lift = None;
        shrink(s);
    }
    Ok(true)
}

pub(crate) fn delete_pick(s: &mut Session) -> Result<bool> {
    let Some(pick) = s.raster_pick else { return Ok(false) };
    let fork = plate_shared(s, pick.layer, pick.key);
    if let Some(lift) = s.raster_lift.take() {
        if let Some(at) = lift.placed {
            clear_at(s, lift.layer, lift.key, at, fork)?;
        }
        if lift.hole {
            clear_at(s, lift.layer, lift.key, lift.origin, fork)?;
        }
    } else {
        clear_at(s, pick.layer, pick.key, pick.rect, fork)?;
    }
    s.raster_pick = None;
    Ok(true)
}

/// Cuts the crop into a nearest-filtered bitmap shape sitting on the plate where it was.
/// The library folder is applied when there is one. The crop is cleared either way.
pub(crate) fn take_shape(s: &mut Session, symbol_name: &str, folder: Option<FolderId>) -> Result<Option<(Element, BitmapId)>> {
    let Some(pick) = s.raster_pick else { return Ok(None) };
    let image = key_plate(s, pick.layer, pick.key).and_then(|plate| plate.copy_rect(pick.rect)).ok_or_else(|| EngineError::State("the pixel crop is gone".into()))?;
    let (w, h) = (image.width, image.height);
    let id = BitmapId(s.doc.alloc_id());
    let name = s.doc.unique_name(folder, &format!("{symbol_name} pixels"));
    let mut bitmap = Bitmap::from_rgba(id, name, w, h, image.rgba).ok_or_else(|| EngineError::State("the pixel crop cannot become a bitmap".into()))?;
    bitmap.smoothing = false;
    let fork = plate_shared(s, pick.layer, pick.key);
    // The lift's stamp is already on the plate. Drop it without putting the hole back.
    s.raster_lift = None;
    clear_at(s, pick.layer, pick.key, pick.rect, fork)?;
    s.doc.bitmaps.push(Arc::new(bitmap));
    if folder.is_some() {
        s.doc.place_entry(LibraryEntry::Item(LibraryItem::Bitmap(id)), folder);
    }
    s.raster_pick = None;
    let element = Element {
        id: ElementId(s.doc.alloc_id()),
        name: String::new(),
        transform: Affine::translate(pick.rect.x as f32, pick.rect.y as f32),
        color: ColorEffect::default(),
        kind: ElementKind::Shape(Shape {
            path: Arc::new(Path::rect(0.0, 0.0, w as f32, h as f32)),
            fill: Some(Paint::Bitmap { bitmap: id, repeat: false, matrix: Affine::IDENTITY }),
            stroke: None,
            fill_rule: FillRule::NonZero,
            merge: false,
            warp: None,
        }),
    };
    Ok(Some((element, id)))
}

fn begin_lift(s: &mut Session, copy: bool) -> Result<()> {
    if s.raster_lift.is_some() {
        return Ok(());
    }
    let pick = s.raster_pick.ok_or_else(|| EngineError::State("nothing is selected".into()))?;
    let sprite = key_plate(s, pick.layer, pick.key).and_then(|plate| plate.copy_rect(pick.rect)).ok_or_else(|| EngineError::State("the pixel crop is gone".into()))?;
    s.raster_lift = Some(Lift {
        layer: pick.layer,
        key: pick.key,
        origin: pick.rect,
        sprite,
        placed: None,
        under: PixelImage::empty(),
        total: Affine::IDENTITY,
        copy,
        hole: !copy,
        stamp_x: pick.rect.x as i32,
        stamp_y: pick.rect.y as i32,
    });
    Ok(())
}

/// Stamps `lift.sprite` through `total`. The picture is computed before the plate changes,
/// so a failure leaves the pixels alone.
fn restamp(s: &mut Session, total: Affine) -> Result<()> {
    let Some(mut lift) = s.raster_lift.take() else {
        return Err(EngineError::State("the pixel crop is gone".into()));
    };
    let (pw, ph) = match key_plate(s, lift.layer, lift.key) {
        Some(plate) => (plate.width(), plate.height()),
        None => {
            s.raster_lift = Some(lift);
            return Err(EngineError::State("the pixel frame is gone".into()));
        }
    };
    let dest = match dest_of(&lift, total, pw, ph) {
        Ok(dest) => dest,
        Err(e) => {
            s.raster_lift = Some(lift);
            return Err(e);
        }
    };
    let fork = plate_shared(s, lift.layer, lift.key);
    let layer = lift.layer;
    let key = lift.key;
    let origin = lift.origin;
    let hole = lift.hole;
    let placed = lift.placed;
    let under = std::mem::replace(&mut lift.under, PixelImage::empty());
    let image = dest.image;
    let stamp_x = dest.stamp_x;
    let stamp_y = dest.stamp_y;
    let visible = dest.visible;
    let stamped = with_plate(s, layer, key, |plate| {
        if hole {
            let _ = plate.clear_rect(origin, fork);
        }
        if let Some(at) = placed {
            let _ = plate.blit_at(at.x as i32, at.y as i32, &under, fork);
        }
        let Some(visible) = visible else { return None };
        let under = plate.copy_rect(visible)?;
        let _ = plate.blit_at(stamp_x, stamp_y, &image, fork);
        Some((under, visible))
    });
    let stamped = match stamped {
        Ok(stamped) => stamped,
        Err(e) => {
            lift.under = under;
            s.raster_lift = Some(lift);
            return Err(e);
        }
    };
    lift.hole = false;
    lift.total = total;
    lift.stamp_x = stamp_x;
    lift.stamp_y = stamp_y;
    match stamped {
        Some((under, visible)) => {
            lift.placed = Some(visible);
            lift.under = under;
            s.raster_pick = Some(RasterPick { layer, key, rect: visible });
        }
        None => {
            lift.placed = None;
            lift.under = PixelImage::empty();
            s.raster_pick = None;
        }
    }
    s.raster_lift = Some(lift);
    Ok(())
}

fn dest_of(lift: &Lift, total: Affine, pw: u32, ph: u32) -> Result<Dest> {
    if let Some((dx, dy)) = integer_translation(total) {
        let stamp_x = lift.origin.x as i32 + dx;
        let stamp_y = lift.origin.y as i32 + dy;
        let visible = clipped(stamp_x, stamp_y, lift.sprite.width, lift.sprite.height, pw, ph);
        return Ok(Dest { image: lift.sprite.clone(), stamp_x, stamp_y, visible });
    }
    let inv = total.inverse().ok_or_else(|| EngineError::Param("the transform is degenerate (it would collapse the artwork)".into()))?;
    let (ox, oy, ow, oh) = (lift.origin.x as f32, lift.origin.y as f32, lift.origin.w as f32, lift.origin.h as f32);
    let mut bounds = Rect::EMPTY;
    for c in [Vec2::new(ox, oy), Vec2::new(ox + ow, oy), Vec2::new(ox + ow, oy + oh), Vec2::new(ox, oy + oh)] {
        bounds.include(total.apply(c));
    }
    if bounds.is_empty() {
        return Ok(Dest { image: PixelImage::empty(), stamp_x: 0, stamp_y: 0, visible: None });
    }
    let x0 = (bounds.min.x.floor() as i32).saturating_sub(1).max(0);
    let y0 = (bounds.min.y.floor() as i32).saturating_sub(1).max(0);
    let x1 = ((bounds.max.x.ceil() as i32).saturating_add(1)).min(pw as i32);
    let y1 = ((bounds.max.y.ceil() as i32).saturating_add(1)).min(ph as i32);
    if x1 <= x0 || y1 <= y0 {
        return Ok(Dest { image: PixelImage::empty(), stamp_x: 0, stamp_y: 0, visible: None });
    }
    let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let Some(bytes) = w.checked_mul(h).and_then(|n| n.checked_mul(4)) else {
        return Err(EngineError::State("the pixel crop is too large to transform".into()));
    };
    let mut rgba = vec![0u8; bytes];
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (w, h, 0usize, 0usize);
    for y in 0..h {
        for x in 0..w {
            let src = inv.apply(Vec2::new(x0 as f32 + x as f32 + 0.5, y0 as f32 + y as f32 + 0.5));
            let px = sample(&lift.sprite, src.x - ox - 0.5, src.y - oy - 0.5);
            if px[3] != 0 {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x + 1);
                max_y = max_y.max(y + 1);
            }
            let i = (y * w + x) * 4;
            if let Some(dst) = rgba.get_mut(i..i + 4) {
                dst.copy_from_slice(&px);
            }
        }
    }
    if max_x <= min_x || max_y <= min_y {
        return Ok(Dest { image: PixelImage::empty(), stamp_x: 0, stamp_y: 0, visible: None });
    }
    let (cw, ch) = (max_x - min_x, max_y - min_y);
    let mut cropped = vec![0u8; cw * ch * 4];
    for row in 0..ch {
        let src_off = ((min_y + row) * w + min_x) * 4;
        let n = cw * 4;
        if let (Some(from), Some(to)) = (rgba.get(src_off..src_off + n), cropped.get_mut(row * n..(row + 1) * n)) {
            to.copy_from_slice(from);
        }
    }
    let stamp_x = x0 + min_x as i32;
    let stamp_y = y0 + min_y as i32;
    Ok(Dest {
        image: PixelImage { width: cw as u32, height: ch as u32, rgba: cropped },
        stamp_x,
        stamp_y,
        visible: Some(PixelRect { x: stamp_x as u32, y: stamp_y as u32, w: cw as u32, h: ch as u32 }),
    })
}

/// Straight-alpha bilinear. A pixel centre is an integer in this space, so an identity
/// sample is that pixel and nothing else. Out of the sprite is clear.
pub(crate) fn sample(src: &PixelImage, u: f32, v: f32) -> [u8; 4] {
    if !u.is_finite() || !v.is_finite() {
        return [0; 4];
    }
    let x0 = u.floor() as i32;
    let y0 = v.floor() as i32;
    let tx = u - x0 as f32;
    let ty = v - y0 as f32;
    let top = lerp4(texel(src, x0, y0), texel(src, x0 + 1, y0), tx);
    let bot = lerp4(texel(src, x0, y0 + 1), texel(src, x0 + 1, y0 + 1), tx);
    lerp4(top, bot, ty)
}

fn texel(src: &PixelImage, x: i32, y: i32) -> [u8; 4] {
    if x < 0 || y < 0 || x as u32 >= src.width || y as u32 >= src.height {
        return [0; 4];
    }
    let i = (y as usize * src.width as usize + x as usize) * 4;
    match src.rgba.get(i..i + 4) {
        Some(px) => [px[0], px[1], px[2], px[3]],
        None => [0; 4],
    }
}

fn lerp4(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    let mut out = [0u8; 4];
    for i in 0..4 {
        let v = a[i] as f32 + (b[i] as f32 - a[i] as f32) * t;
        out[i] = v.round().clamp(0.0, 255.0) as u8;
    }
    out
}

fn integer_translation(m: Affine) -> Option<(i32, i32)> {
    let [a, b, c, d, e, f] = m.m;
    if (a - 1.0).abs() > 1e-4 || b.abs() > 1e-4 || c.abs() > 1e-4 || (d - 1.0).abs() > 1e-4 {
        return None;
    }
    let (x, y) = (e.round(), f.round());
    if !x.is_finite() || !y.is_finite() || (e - x).abs() > 1e-3 || (f - y).abs() > 1e-3 {
        return None;
    }
    Some((x as i32, y as i32))
}

fn near_identity(m: &Affine) -> bool {
    m.m.iter().zip(Affine::IDENTITY.m).all(|(a, b)| (a - b).abs() <= 1e-4)
}

fn clipped(x: i32, y: i32, w: u32, h: u32, pw: u32, ph: u32) -> Option<PixelRect> {
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = x.saturating_add(w as i32).min(pw as i32);
    let y1 = y.saturating_add(h as i32).min(ph as i32);
    if x1 <= x0 || y1 <= y0 { None } else { Some(PixelRect { x: x0 as u32, y: y0 as u32, w: (x1 - x0) as u32, h: (y1 - y0) as u32 }) }
}

fn pixel_window(stage: Rect, width: u32, height: u32) -> Option<PixelRect> {
    if stage.is_empty() || width == 0 || height == 0 {
        return None;
    }
    let (x0, y0, x1, y1) = (stage.min.x.floor(), stage.min.y.floor(), stage.max.x.ceil(), stage.max.y.ceil());
    if ![x0, y0, x1, y1].into_iter().all(|v| v.is_finite()) {
        return None;
    }
    let x0 = x0.max(0.0) as u32;
    let y0 = y0.max(0.0) as u32;
    let x1 = x1.clamp(0.0, width as f32) as u32;
    let y1 = y1.clamp(0.0, height as f32) as u32;
    if x0 >= width || y0 >= height || x1 <= x0 || y1 <= y0 { None } else { Some(PixelRect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }) }
}

fn key_plate<'a>(s: &'a Session, layer: LayerId, key: u32) -> Option<&'a RasterPlate> {
    s.timeline().layer(layer)?.keyframes.iter().find(|k| k.start == key)?.plate.as_ref()
}

/// Another key in the document still shares this plate's rows. A render job does not count.
fn plate_shared(s: &Session, layer: LayerId, key: u32) -> bool {
    let Some(id) = key_plate(s, layer, key).map(|plate| plate.buffer_id()) else { return false };
    let mut n = 0u32;
    for timeline in s.doc.timelines() {
        for layer in &timeline.layers {
            for k in &layer.keyframes {
                if k.plate.as_ref().is_some_and(|plate| plate.buffer_id() == id) {
                    n += 1;
                    if n >= 2 {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn with_plate<T>(s: &mut Session, layer: LayerId, key: u32, f: impl FnOnce(&mut RasterPlate) -> T) -> Result<T> {
    let tl = crate::editing::edited_mut(&mut s.doc, s.scene, &s.editing)?;
    let layer = tl.layer_mut(layer).ok_or_else(|| EngineError::State("the pixel layer is gone".into()))?;
    if layer.locked {
        return Err(EngineError::State(format!("layer '{}' is locked", layer.name)));
    }
    let idx = layer.keyframes.iter().position(|k| k.start == key).ok_or_else(|| EngineError::State("the pixel frame is gone".into()))?;
    let kf = layer.keyframe_mut(idx).ok_or_else(|| EngineError::State("the pixel frame is gone".into()))?;
    let plate = kf.plate.as_mut().ok_or_else(|| EngineError::State("the pixel frame is empty".into()))?;
    Ok(f(plate))
}

fn clear_at(s: &mut Session, layer: LayerId, key: u32, rect: PixelRect, fork: bool) -> Result<bool> {
    with_plate(s, layer, key, |plate| plate.clear_rect(rect, fork))
}

#[cfg(test)]
mod tests {
    use super::*;
    use anicraft_doc::{Document, LayerKind, SymbolKind};
    use serde_json::json;

    fn session() -> Session {
        let mut s = Session::new(Document::default());
        s.execute("doc.set_stage", &json!({"width": 64, "height": 48})).expect("stage");
        s
    }

    fn paint(s: &mut Session, x: f32, y: f32) {
        s.style.set_fill_color(anicraft_doc::Rgba::new(1.0, 0.0, 0.0, 1.0));
        s.execute("paint.raster", &json!({"x": x, "y": y, "size": 2.0, "size_min": 2.0, "softness": 0.0, "antialias": 0.0, "color": "#ff0000"})).expect("paint");
    }

    fn alpha(s: &Session, x: u32, y: u32) -> u8 {
        s.active_layer().and_then(|l| l.keyframe_at(s.frame)).and_then(|k| k.plate.as_ref()).map(|p| p.alpha_at(x, y)).unwrap_or(0)
    }

    fn bytes(s: &Session, rect: PixelRect) -> Vec<u8> {
        s.active_layer().and_then(|l| l.keyframe_at(s.frame)).and_then(|k| k.plate.as_ref()).and_then(|p| p.copy_rect(rect)).expect("crop").rgba
    }

    #[test]
    fn sample_at_a_pixel_centre_is_that_pixel() {
        let image = PixelImage { width: 2, height: 1, rgba: vec![255, 0, 0, 255, 0, 0, 255, 255] };
        assert_eq!(sample(&image, 0.0, 0.0), [255, 0, 0, 255]);
        assert_eq!(sample(&image, 1.0, 0.0), [0, 0, 255, 255]);
        assert_eq!(sample(&image, -1.0, 0.0), [0, 0, 0, 0]);
    }

    #[test]
    fn a_click_crops_the_ink_and_an_empty_click_lets_go() {
        let mut s = session();
        paint(&mut s, 10.0, 12.0);
        s.execute("element.pick", &json!({"x": 10.5, "y": 12.5})).expect("pick");
        let pick = s.raster_pick().expect("crop");
        assert!(pick.rect.w < 64 && pick.rect.h < 48);
        assert!(pick.rect.w >= 1 && pick.rect.h >= 1);
        assert!(pick.rect.contains(10, 12));
        assert!(!pick.rect.contains(0, 0));
        s.execute("element.pick", &json!({"x": 1.5, "y": 1.5})).expect("miss");
        assert!(s.raster_pick().is_none());
    }

    #[test]
    fn a_marquee_takes_only_the_ink_inside_it() {
        let mut s = session();
        paint(&mut s, 8.0, 20.0);
        paint(&mut s, 40.0, 20.0);
        let before = s.history_len();
        s.execute("raw.select_rect", &json!({"x0": 0.0, "y0": 0.0, "x1": 20.0, "y1": 40.0})).expect("marquee");
        assert_eq!(s.history_len(), before);
        let rect = s.raster_pick().expect("crop").rect;
        assert!(rect.contains(8, 20));
        assert!(rect.x + rect.w <= 24);
        s.execute("element.pick", &json!({"x": 8.5, "y": 20.5})).expect("inside");
        assert_eq!(s.raster_pick().map(|p| p.rect), Some(rect));
    }

    #[test]
    fn two_integer_moves_in_one_gesture_are_exact_and_one_undo() {
        let mut s = session();
        paint(&mut s, 10.0, 12.0);
        s.execute("element.pick", &json!({"x": 10.5, "y": 12.5})).expect("pick");
        let rect = s.raster_pick().expect("crop").rect;
        let original = bytes(&s, rect);
        let before = s.history_len();
        s.begin_gesture("Move");
        s.execute("element.translate", &json!({"dx": 3.0, "dy": 0.0})).expect("nudge");
        s.execute("element.translate", &json!({"dx": 4.0, "dy": 0.0})).expect("nudge");
        s.end_gesture();
        assert_eq!(s.history_len(), before + 1);
        let moved = s.raster_pick().expect("moved").rect;
        assert_eq!((moved.x, moved.y, moved.w, moved.h), (rect.x + 7, rect.y, rect.w, rect.h));
        assert_eq!(bytes(&s, moved), original);
        for y in rect.y..rect.y + rect.h {
            for x in rect.x..rect.x + rect.w {
                assert_eq!(alpha(&s, x, y), 0, "the hole at {x},{y}");
            }
        }
        s.undo().expect("undo");
        assert_eq!(bytes(&s, rect), original);
        assert!(s.raster_pick().is_some());
    }

    #[test]
    fn a_copy_leaves_the_original_and_the_copy() {
        let mut s = session();
        paint(&mut s, 10.0, 12.0);
        s.execute("element.pick", &json!({"x": 10.5, "y": 12.5})).expect("pick");
        let rect = s.raster_pick().expect("crop").rect;
        let original = bytes(&s, rect);
        let before = s.history_len();
        s.begin_gesture("Copy");
        s.execute("edit.duplicate", &json!({"dx": 0.0, "dy": 0.0})).expect("copy");
        s.execute("element.translate", &json!({"dx": 8.0, "dy": 0.0})).expect("move");
        s.end_gesture();
        assert_eq!(s.history_len(), before + 1);
        assert_eq!(bytes(&s, rect), original);
        let copy = PixelRect { x: rect.x + 8, ..rect };
        assert_eq!(bytes(&s, copy), original);
        s.undo().expect("undo");
        assert_eq!(bytes(&s, rect), original);
        assert_eq!(alpha(&s, rect.x + 8, rect.y), 0);
    }

    #[test]
    fn delete_clears_the_ink_and_the_crop() {
        let mut s = session();
        paint(&mut s, 10.0, 12.0);
        s.execute("element.pick", &json!({"x": 10.5, "y": 12.5})).expect("pick");
        s.execute("edit.delete", &json!({})).expect("delete");
        assert!(s.raster_pick().is_none());
        assert_eq!(alpha(&s, 10, 12), 0);
        s.undo().expect("undo");
        assert_ne!(alpha(&s, 10, 12), 0);
    }

    #[test]
    fn rotate_turns_a_horizontal_pair_upright() {
        let mut s = session();
        paint(&mut s, 8.0, 24.0);
        paint(&mut s, 28.0, 24.0);
        s.execute("element.pick", &json!({"x": 8.5, "y": 24.5})).expect("pick");
        let before = s.raster_pick().expect("crop").rect;
        assert!(before.w > before.h);
        s.execute("modify.rotate_cw", &json!({})).expect("rotate");
        let after = s.raster_pick().expect("turned").rect;
        assert!(after.h > after.w, "rotated bounds {after:?} from {before:?}");
        assert!(s.active_layer().and_then(|l| l.keyframe_at(0)).and_then(|k| k.plate.as_ref()).is_some_and(|p| p.has_ink()));
        s.undo().expect("undo");
        assert!(s.raster_pick().is_some_and(|p| p.rect.w > p.rect.h));
    }

    #[test]
    fn convert_makes_a_movie_clip_and_leaves_a_hole() {
        let mut s = session();
        paint(&mut s, 10.0, 12.0);
        s.execute("element.pick", &json!({"x": 10.5, "y": 12.5})).expect("pick");
        let rect = s.raster_pick().expect("crop").rect;
        let before = s.history_len();
        s.execute("modify.convert_to_symbol", &json!({"kind": "movie_clip", "name": "Blob"})).expect("convert");
        assert_eq!(s.history_len(), before + 1);
        assert!(s.raster_pick().is_none());
        assert_eq!(alpha(&s, rect.x, rect.y), 0);
        assert_eq!(s.doc.library.last().map(|sym| sym.kind), Some(SymbolKind::MovieClip));
        let bitmap = s.doc.bitmaps.last().expect("bitmap");
        assert!(!bitmap.smoothing);
        assert_eq!((bitmap.width, bitmap.height), (rect.w, rect.h));
        assert_eq!(s.selection.len(), 1);
        assert_eq!(s.active_layer().map(|l| l.kind), Some(LayerKind::Normal));
        let drawn = anicraft_doc::evaluate(&s.doc, s.frame);
        let item = drawn.items.iter().find(|i| i.image.is_some() && i.plate.is_none()).expect("bitmap item");
        let bounds = item.path.bounds().transformed(&item.transform);
        assert!((bounds.min.x - rect.x as f32).abs() < 0.01);
        assert!((bounds.min.y - rect.y as f32).abs() < 0.01);
        assert!((bounds.width() - rect.w as f32).abs() < 0.01);
        assert!((bounds.height() - rect.h as f32).abs() < 0.01);
        s.undo().expect("undo");
        assert_ne!(alpha(&s, 10, 12), 0);
    }
}
