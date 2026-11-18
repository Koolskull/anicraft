//! Pixel layers (ADR 0112): a transparent plate the size of the stage, painted by dabs
//! or blended by a smudge. One command is one undo step; a drag holds a gesture around it.

use crate::params::*;
use crate::{CommandSpec, EngineError, KeyEdit, Result, Session};
use anicraft_doc::{LayerKind, RasterPlate, Rgba};
use anicraft_raster::{Antialias, Grain, HAIRLINE_DIAMETER, Picture, Sample, Smear, Stamp, Stroke, Tip, blend_smudge, dab_opacity, dab_spacing, dab_turn, paint_cover, picture_spacing, sample_stroke, stamp_mark, stroke_aim};
use serde_json::{Value, json};

pub fn specs() -> Vec<CommandSpec> {
    let c = |id, label, menu, shortcut, params, mutates, run| CommandSpec { id, label, menu, shortcut, params, mutates, run };
    vec![c(
        "paint.raster",
        "Paint Pixels",
        "",
        None,
        "{x, y, pressure?} or {samples: [{x, y, pressure?}, …]}, size?, size_min?, softness?, antialias?: 0|0.5|1|2, color? (default: the fill colour, alpha included), erase?: bool, smudge?: bool, strength?, opacity_from_pressure?: bool (default true: coverage runs from opacity_min at no pressure to opacity_max at full pressure), opacity_min?, opacity_max? (0..1, default 0 and 1), flow?: 0..1 (default 1: the stronger dab wins; below 1 each dab deposits that share), speckle?: 0..1 (default 0: one solid dab; higher sprays tiny dots of mixed size and opacity inside the brush), grain?: 0..1 (default 0: paper tooth that stays on the page), wedge?: 0..1 (default 0: a round dab; higher lays the tip on its side, a sharp edge and a soft fade in the direction of the stroke, and a point with no direction stays round), picture?: 0|1|2|3|4 (default 0: the round or grain dab; 1 fine spray, 2 heavy spray, 3 tight dot, 4 spray can; each dab is that picture, turned), continued?: bool}. On a vector layer this adds a raster layer first. Smudge blends colours already on the plate and does not add a hue. strength 1 replaces the disc for the whole stroke. The Raster tool's Stretch slider tops out lower. The colour's alpha is the opacity of the whole stroke, not of each dab. Flow 1 keeps that wash. A lower flow builds up inside it and still stops at the fill's alpha. The Raster tool sends the fill colour and leaves pressure opacity off unless its Opacity switch is on. continued skips the first sample, which the previous move already painted",
        true,
        paint,
    )]
}

fn paint(s: &mut Session, p: &Value) -> Result<Value> {
    let (width, height) = (s.doc.stage.width, s.doc.stage.height);
    if !RasterPlate::fits(width, height) {
        return Err(EngineError::State(format!("this stage is {width}×{height}, which is too large for a pixel layer")));
    }
    let samples = samples(p)?;
    let smudge = opt_bool(p, "smudge")?.unwrap_or(false);
    let erase = opt_bool(p, "erase")?.unwrap_or(false) && !smudge;
    let softness = opt_f32(p, "softness")?.unwrap_or(0.0).clamp(0.0, 1.0);
    let strength = opt_f32(p, "strength")?.unwrap_or(1.0).clamp(0.0, 1.0);
    let size = opt_f32(p, "size")?.unwrap_or(24.0).clamp(1.0, 400.0);
    let size_min = opt_f32(p, "size_min")?.unwrap_or(1.0).clamp(HAIRLINE_DIAMETER, size);
    let continued = opt_bool(p, "continued")?.unwrap_or(false);
    let color = bytes(opt_color(p, "color")?.unwrap_or(s.style.fill));
    let opacity_from_pressure = opt_bool(p, "opacity_from_pressure")?.unwrap_or(true);
    let opacity_max = opt_f32(p, "opacity_max")?.unwrap_or(1.0).clamp(0.0, 1.0);
    let opacity_min = opt_f32(p, "opacity_min")?.unwrap_or(0.0).clamp(0.0, opacity_max);
    let flow = opt_f32(p, "flow")?.unwrap_or(1.0).clamp(0.0, 1.0);
    let grain = Grain {
        speckle: opt_f32(p, "speckle")?.unwrap_or(0.0).clamp(0.0, 1.0),
        grain: opt_f32(p, "grain")?.unwrap_or(0.0).clamp(0.0, 1.0),
        wedge: opt_f32(p, "wedge")?.unwrap_or(0.0).clamp(0.0, 1.0),
    };
    let picture = Picture::from_index(opt_u32(p, "picture")?.unwrap_or(0));
    let stroke = Stroke { size, size_min, softness, antialias: antialias(opt_f32(p, "antialias")?.unwrap_or(0.5)), color, erase, opacity_from_pressure, opacity_min, opacity_max, flow };
    let pass = Pass { stroke, grain, picture };
    if !s.active_layer().is_some_and(|l| l.kind == LayerKind::Raster) {
        crate::cmds_timeline::new_raster(s, &json!({}))?;
    }
    if s.active_layer().is_some_and(|l| l.locked) {
        return Err(EngineError::State("the pixel layer is locked".into()));
    }
    let fork = other_plate_shares(s);
    let frame = s.frame;
    let (y0, y1) = band(&samples, &stroke, height, smudge);
    let (base, cover) = if smudge { (None, Vec::new()) } else { (pre_stroke_band(s, y0, y1, width), lay(s, width, height, &samples, pass, continued, y0, y1)?) };
    let kf = s.editable_keyframe_in(KeyEdit::Pixels)?;
    let plate = ensure_plate(kf, width, height)?;
    let hit = plate.paint_band(y0, y1, fork, |buf| {
        let h = y1 - y0;
        if smudge {
            smear(buf, width, h, &samples, stroke, strength, y0)
        } else {
            if let Some(base) = &base
                && base.len() == buf.len()
            {
                buf.copy_from_slice(base);
            }
            paint_cover(buf, &cover, stroke.color, stroke.erase)
        }
    });
    Ok(json!({"hit": hit, "frame": frame, "width": width, "height": height, "smudge": smudge}))
}

/// True when another key still shares the active plate. A render job does not count.
fn other_plate_shares(s: &Session) -> bool {
    let Some(mine) = s.active_layer().and_then(|l| l.keyframe_at(s.frame)).and_then(|k| k.plate.as_ref()) else {
        return false;
    };
    let mut n = 0u32;
    for timeline in s.doc.timelines() {
        for layer in &timeline.layers {
            for key in &layer.keyframes {
                if key.plate.as_ref().is_some_and(|p| p.same_rows(mine)) {
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

/// Full-width rows the dab or the smudge pull can touch.
fn band(samples: &[Sample], stroke: &Stroke, height: u32, smudge: bool) -> (u32, u32) {
    if samples.is_empty() || height == 0 {
        return (0, 0);
    }
    let mut lo = f32::MAX;
    let mut hi = f32::MIN;
    for (i, sample) in samples.iter().enumerate() {
        let radius = diameter(*stroke, sample.pressure) * 0.5;
        let mut pad = radius + 4.0;
        if smudge && i > 0 {
            let Some(prev) = samples.get(i - 1) else { continue };
            pad += (sample.x - prev.x).hypot(sample.y - prev.y) * (1.0 + stroke.softness);
        }
        lo = lo.min(sample.y - pad);
        hi = hi.max(sample.y + pad);
    }
    if !lo.is_finite() || !hi.is_finite() {
        return (0, 0);
    }
    let top = height as f32;
    let y0 = lo.floor().clamp(0.0, top) as u32;
    let y1 = hi.ceil().clamp(0.0, top) as u32;
    (y0.min(y1), y0.max(y1))
}

fn ensure_plate(kf: &mut anicraft_doc::Keyframe, width: u32, height: u32) -> Result<&mut RasterPlate> {
    let same = kf.plate.as_ref().is_some_and(|p| p.is_ok() && p.width() == width && p.height() == height);
    if !same {
        let mut fresh = RasterPlate::transparent(width, height).ok_or_else(|| EngineError::State(format!("this stage is {width}×{height}, which is too large for a pixel layer")))?;
        if let Some(old) = kf.plate.take() {
            fresh.copy_from_corner(&old);
        }
        kf.plate = Some(fresh);
    }
    kf.plate.as_mut().ok_or_else(|| EngineError::State("the pixel layer has no plate".into()))
}

/// Coverage kept for one open raster drag. Not part of the document.
pub(crate) struct RasterCover {
    width: u32,
    height: u32,
    bytes: Vec<u8>,
}

/// The plate rows from before this drag, for `y0..y1`. `None` outside a gesture, where the
/// buffer `paint_band` hands over is already the picture from before this command.
fn pre_stroke_band(s: &Session, y0: u32, y1: u32, width: u32) -> Option<Vec<u8>> {
    if !s.in_gesture() || y1 <= y0 {
        return None;
    }
    let stride = (width as usize).saturating_mul(4);
    let n = (y1 - y0) as usize;
    let Some(bytes) = n.checked_mul(stride) else { return None };
    let mut out = vec![0u8; bytes];
    let Some(plate) = s.pre_stroke_plate() else { return Some(out) };
    if plate.width() != width {
        return Some(out);
    }
    for (i, y) in (y0..y1).enumerate() {
        let Some(row) = plate.row(y) else { continue };
        let Some(dst) = out.get_mut(i * stride..(i + 1) * stride) else { continue };
        if row.len() == stride {
            dst.copy_from_slice(row);
        }
    }
    Some(out)
}

/// A round stroke plus the spray, grain and pencil lay. Omitted amounts stay a round dab.
#[derive(Clone, Copy)]
struct Pass {
    stroke: Stroke,
    grain: Grain,
    picture: Picture,
}

/// One byte of coverage per pixel of the band `y0..y1`. Inside a gesture the bytes accumulate,
/// so a later move does not paint over the stroke it already laid down.
fn lay(s: &mut Session, width: u32, height: u32, samples: &[Sample], pass: Pass, continued: bool, y0: u32, y1: u32) -> Result<Vec<u8>> {
    let band_h = y1.saturating_sub(y0);
    let band_n = (width as usize).checked_mul(band_h as usize).ok_or_else(|| EngineError::State("this stage is too large for a stroke".into()))?;
    if s.in_gesture() {
        ensure_cover(s, width, height)?;
        let Some(cover) = s.raster_cover.as_mut() else {
            return Err(EngineError::State("this stage is too large for a stroke".into()));
        };
        lay_into(&mut cover.bytes, width, height, samples, pass, continued, 0.0);
        return Ok(band_of(&cover.bytes, width, y0, y1));
    }
    let mut local = vec![0u8; band_n];
    lay_into(&mut local, width, band_h, samples, pass, continued, y0 as f32);
    Ok(local)
}

fn ensure_cover(s: &mut Session, width: u32, height: u32) -> Result<()> {
    let n = (width as usize).checked_mul(height as usize).ok_or_else(|| EngineError::State("this stage is too large for a stroke".into()))?;
    let ready = s.raster_cover.as_ref().is_some_and(|cover| cover.width == width && cover.height == height && cover.bytes.len() == n);
    if ready {
        return Ok(());
    }
    let mut bytes = Vec::new();
    if bytes.try_reserve_exact(n).is_err() {
        return Err(EngineError::State("this stage is too large for a stroke".into()));
    }
    bytes.resize(n, 0);
    s.raster_cover = Some(RasterCover { width, height, bytes });
    Ok(())
}

fn lay_into(mask: &mut [u8], width: u32, height: u32, samples: &[Sample], pass: Pass, continued: bool, shift: f32) {
    let stroke = pass.stroke;
    let spaced = sample_stroke(samples, |mid| {
        let width = diameter(stroke, mid);
        if pass.picture == Picture::None { dab_spacing(width) } else { picture_spacing(width) }
    });
    // The anchor is already in the gesture's coverage. Max would ignore it; skipping it
    // keeps a continued move from walking the same disc twice. The aim still sees that
    // anchor, so a step shorter than the spacing points along the stroke.
    let origin = usize::from(continued);
    let dabs = spaced.get(origin..).unwrap_or(&[]);
    for (i, sample) in dabs.iter().enumerate() {
        let opacity = dab_opacity(stroke, sample.pressure);
        let tip = Tip { radius: diameter(stroke, sample.pressure) * 0.5, softness: stroke.softness, antialias: stroke.antialias, color: stroke.color, opacity, erase: false };
        let (aim_x, aim_y) = stroke_aim(&spaced, i + origin);
        let turn = if pass.picture == Picture::None { 0.0 } else { dab_turn(sample.x, sample.y - shift, i) };
        let _ = stamp_mark(mask, width, height, sample.x, sample.y - shift, tip, Stamp { flow: stroke.flow, grain: pass.grain, aim_x, aim_y, picture: pass.picture, turn });
    }
}

fn band_of(mask: &[u8], width: u32, y0: u32, y1: u32) -> Vec<u8> {
    let w = width as usize;
    let mut out = Vec::new();
    for y in y0..y1 {
        let start = y as usize * w;
        let Some(row) = mask.get(start..start + w) else { break };
        out.extend_from_slice(row);
    }
    out
}

fn smear(buf: &mut [u8], width: u32, height: u32, samples: &[Sample], stroke: Stroke, strength: f32, y0: u32) -> bool {
    let mut hit = false;
    let shift = y0 as f32;
    if samples.len() < 2 {
        if let Some(sample) = samples.first() {
            let radius = diameter(stroke, sample.pressure) * 0.5;
            hit |= blend_smudge(buf, width, height, sample.x, sample.y - shift, sample.x, sample.y - shift, Smear { radius, softness: stroke.softness, strength });
        }
        return hit;
    }
    for pair in samples.windows(2) {
        let (Some(from), Some(to)) = (pair.first(), pair.get(1)) else { continue };
        let radius = diameter(stroke, (from.pressure + to.pressure) * 0.5) * 0.5;
        hit |= blend_smudge(buf, width, height, from.x, from.y - shift, to.x, to.y - shift, Smear { radius, softness: stroke.softness, strength });
    }
    hit
}

fn diameter(stroke: Stroke, pressure: f32) -> f32 {
    let pressure = if pressure.is_finite() { pressure.clamp(0.0, 1.0) } else { 0.0 };
    let size = if stroke.size.is_finite() { stroke.size.max(0.0) } else { 0.0 };
    let low = if stroke.size_min.is_finite() { stroke.size_min.clamp(0.0, size) } else { 0.0 };
    low + (size - low) * pressure
}

fn antialias(fringe: f32) -> Antialias {
    if !fringe.is_finite() || fringe <= 0.0 {
        Antialias::Off
    } else if (fringe - 0.5).abs() < 0.01 {
        Antialias::On
    } else {
        Antialias::Px(fringe)
    }
}

fn bytes(c: Rgba) -> [u8; 4] {
    let b = |v: f32| if v.is_finite() { (v.clamp(0.0, 1.0) * 255.0).round() as u8 } else { 0 };
    [b(c.r), b(c.g), b(c.b), b(c.a)]
}

fn samples(p: &Value) -> Result<Vec<Sample>> {
    match p.get("samples") {
        None | Some(Value::Null) => {
            let pressure = opt_f32(p, "pressure")?.unwrap_or(1.0).clamp(0.0, 1.0);
            Ok(vec![Sample { x: f32_req(p, "x")?, y: f32_req(p, "y")?, pressure }])
        }
        Some(Value::Array(rows)) => {
            if rows.is_empty() {
                return Err(EngineError::Param("param 'samples' needs at least one point".into()));
            }
            if rows.len() > 4_096 {
                return Err(EngineError::Param("param 'samples' is limited to 4096 points".into()));
            }
            let mut out = Vec::with_capacity(rows.len());
            for row in rows {
                let x = row.get("x").and_then(Value::as_f64).filter(|v| v.is_finite() && v.abs() <= 1e7).ok_or_else(|| EngineError::Param("each sample needs a finite 'x'".into()))? as f32;
                let y = row.get("y").and_then(Value::as_f64).filter(|v| v.is_finite() && v.abs() <= 1e7).ok_or_else(|| EngineError::Param("each sample needs a finite 'y'".into()))? as f32;
                let pressure = row.get("pressure").and_then(Value::as_f64).filter(|v| v.is_finite()).map(|v| v as f32).unwrap_or(1.0).clamp(0.0, 1.0);
                out.push(Sample { x, y, pressure });
            }
            Ok(out)
        }
        Some(_) => Err(EngineError::Param("param 'samples' must be a list of points".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anicraft_doc::Document;

    fn session() -> Session {
        let mut s = Session::new(Document::default());
        s.execute("doc.set_stage", &json!({"width": 64, "height": 48})).expect("stage");
        s
    }

    fn pixel(s: &Session, x: u32, y: u32) -> Option<[u8; 4]> {
        let plate = s.active_layer()?.keyframe_at(s.frame)?.plate.as_ref()?;
        let image = plate.image();
        let o = ((y as usize) * (image.width as usize) + x as usize) * 4;
        let px = image.rgba.get(o..o + 4)?;
        Some([px[0], px[1], px[2], px[3]])
    }

    #[test]
    fn a_stroke_is_stage_pixels_and_one_undo() {
        let mut s = session();
        s.style.set_fill_color(Rgba::new(1.0, 0.0, 0.0, 1.0));
        let before = s.history_len();
        s.execute("paint.raster", &json!({"x": 10.0, "y": 12.0, "size": 2.0, "size_min": 2.0, "softness": 0.0, "antialias": 0.0})).expect("paint");
        assert_eq!(s.history_len(), before + 1);
        assert_eq!(s.active_layer().map(|l| l.kind), Some(LayerKind::Raster));
        let plate = s.active_layer().and_then(|l| l.keyframe_at(0)).and_then(|k| k.plate.as_ref()).expect("plate");
        assert_eq!((plate.width(), plate.height()), (64, 48));
        assert_eq!(pixel(&s, 10, 12), Some([255, 0, 0, 255]));
        let drawn = anicraft_doc::evaluate(&s.doc, s.frame);
        assert!(drawn.items.iter().any(|i| i.image.is_some() && !i.smooth));
        s.undo().expect("undo");
        assert!(s.timeline().layers.iter().all(|l| l.kind != LayerKind::Raster));
    }

    #[test]
    fn vector_draw_refuses_a_pixel_layer_and_smudge_keeps_the_hue() {
        let mut s = session();
        s.execute("paint.raster", &json!({"x": 8.0, "y": 8.0, "size": 4.0, "size_min": 4.0, "softness": 0.0, "antialias": 0.0, "color": "#ff0000"})).expect("dab");
        let refused = s.execute("draw.rect", &json!({"x": 1.0, "y": 1.0, "w": 8.0, "h": 8.0}));
        let message = refused.expect_err("a pixel layer refuses a rectangle").to_string();
        assert!(message.contains("pixel layer"), "{message}");
        s.execute("paint.raster", &json!({"samples": [{"x": 8.0, "y": 8.0}, {"x": 20.0, "y": 8.0}], "size": 4.0, "size_min": 4.0, "softness": 0.0, "antialias": 0.0, "smudge": true})).expect("smudge");
        let mut saw_red = false;
        let plate = s.active_layer().and_then(|l| l.keyframe_at(0)).and_then(|k| k.plate.as_ref()).expect("plate");
        for px in plate.image().rgba.as_chunks::<4>().0 {
            assert_eq!(px.get(1).copied().unwrap_or(1), 0, "smudge added green");
            assert_eq!(px.get(2).copied().unwrap_or(1), 0, "smudge added blue");
            if px.first().copied().unwrap_or(0) > 200 && px.get(3).copied().unwrap_or(0) > 200 {
                saw_red = true;
            }
        }
        assert!(saw_red);
    }

    #[test]
    fn a_light_press_is_one_pixel_and_a_second_key_keeps_its_picture() {
        let mut s = session();
        s.execute("paint.raster", &json!({"x": 4.5, "y": 4.5, "pressure": 0.02, "size": 24.0, "size_min": 0.05, "softness": 0.0, "antialias": 0.0, "color": "#ff0000"})).expect("hairline");
        let hair = pixel(&s, 4, 4).expect("pixel");
        assert_eq!(hair[3], 1, "{hair:?}");
        assert_eq!(pixel(&s, 5, 4), Some([0, 0, 0, 0]));
        s.undo().expect("undo hairline");

        s.execute("paint.raster", &json!({"x": 10.0, "y": 12.0, "size": 4.0, "size_min": 4.0, "softness": 0.0, "antialias": 0.0, "color": "#ff0000"})).expect("base");
        let before = s.active_layer().and_then(|l| l.keyframe_at(0)).and_then(|k| k.plate.as_ref()).expect("plate").image().rgba.clone();
        s.execute("timeline.insert_keyframe", &json!({"frame": 5})).expect("f6");
        s.set_frame(5);
        let steps = s.history_len();
        s.begin_gesture("Paint Pixels");
        s.execute("paint.raster", &json!({"x": 40.0, "y": 20.0, "size": 4.0, "size_min": 4.0, "softness": 0.0, "antialias": 0.0, "color": "#0000ff"})).expect("dab");
        let id = s.active_layer().and_then(|l| l.keyframe_at(5)).and_then(|k| k.plate.as_ref()).expect("painted").buffer_id();
        s.execute("paint.raster", &json!({"samples": [{"x": 40.0, "y": 20.0, "pressure": 1.0}, {"x": 48.0, "y": 20.0, "pressure": 1.0}], "continued": true, "size": 4.0, "size_min": 4.0, "softness": 0.0, "antialias": 0.0, "color": "#0000ff"})).expect("move");
        let id_again = s.active_layer().and_then(|l| l.keyframe_at(5)).and_then(|k| k.plate.as_ref()).expect("painted").buffer_id();
        assert_eq!(id, id_again, "a continued stroke copies the plate once");
        s.end_gesture();
        assert_eq!(s.history_len(), steps + 1);
        let kept = s.active_layer().and_then(|l| l.keyframe_at(0)).and_then(|k| k.plate.as_ref()).expect("f6").image();
        assert_eq!(kept.rgba, before, "the copied key stays as it was");
        assert_ne!(s.active_layer().and_then(|l| l.keyframe_at(0)).and_then(|k| k.plate.as_ref()).map(|p| p.buffer_id()), s.active_layer().and_then(|l| l.keyframe_at(5)).and_then(|k| k.plate.as_ref()).map(|p| p.buffer_id()));
        s.undo().expect("undo stroke");
        let restored = s.active_layer().and_then(|l| l.keyframe_at(5)).and_then(|k| k.plate.as_ref()).expect("restored").image();
        assert_eq!(restored.rgba, before);
        let still = s.active_layer().and_then(|l| l.keyframe_at(0)).and_then(|k| k.plate.as_ref()).expect("f6").image();
        assert_eq!(still.rgba, before);
    }

    #[test]
    fn a_wash_keeps_one_opacity_for_the_whole_stroke() {
        let mut s = session();
        let dab = |s: &mut Session, continued: bool| {
            let params = if continued {
                json!({"samples": [{"x": 16.0, "y": 16.0}, {"x": 22.0, "y": 16.0}], "continued": true, "size": 16.0, "size_min": 16.0, "softness": 0.0, "antialias": 0.0, "color": "#ff000080"})
            } else {
                json!({"x": 16.0, "y": 16.0, "size": 16.0, "size_min": 16.0, "softness": 0.0, "antialias": 0.0, "color": "#ff000080"})
            };
            s.execute("paint.raster", &params).expect("paint");
        };
        s.begin_gesture("Paint Pixels");
        dab(&mut s, false);
        let once = pixel(&s, 16, 16).expect("first dab");
        dab(&mut s, true);
        let twice = pixel(&s, 16, 16).expect("overlap");
        s.end_gesture();
        assert_eq!(once[3], twice[3], "the overlap got darker: {once:?} -> {twice:?}");
        assert!((120..160).contains(&twice[3]), "the stroke should stay near the fill alpha, got {twice:?}");
        s.begin_gesture("Paint Pixels");
        dab(&mut s, false);
        s.end_gesture();
        let again = pixel(&s, 16, 16).expect("second stroke");
        assert!(again[3] > twice[3], "a later stroke should be able to build on the first: {again:?} vs {twice:?}");
    }

    #[test]
    fn a_fill_alpha_stays_put_until_pressure_opacity_sets_a_range() {
        let mut s = session();
        s.style.set_fill_color(Rgba::new(1.0, 0.0, 0.0, 0.5));
        let wash = json!({"x": 16.0, "y": 16.0, "pressure": 0.15, "size": 16.0, "size_min": 16.0, "softness": 0.0, "antialias": 0.0, "opacity_from_pressure": false});
        s.execute("paint.raster", &wash).expect("wash");
        let px = pixel(&s, 16, 16).expect("fill alpha");
        assert_eq!(px, [255, 0, 0, 128], "the fill's alpha should be the stroke, got {px:?}");

        let pinned = json!({"x": 40.0, "y": 16.0, "pressure": 0.1, "size": 16.0, "size_min": 16.0, "softness": 0.0, "antialias": 0.0, "color": "#0000ff", "opacity_from_pressure": true, "opacity_min": 0.2, "opacity_max": 0.2});
        s.execute("paint.raster", &pinned).expect("pinned");
        let px = pixel(&s, 40, 16).expect("pinned opacity");
        assert_eq!(px[3], 51, "minimum and maximum should set the coverage, got {px:?}");

        let full = json!({"x": 16.0, "y": 36.0, "pressure": 0.05, "size": 8.0, "size_min": 8.0, "softness": 0.0, "antialias": 0.0, "color": "#00ff00", "opacity_from_pressure": true, "opacity_min": 1.0, "opacity_max": 1.0});
        s.execute("paint.raster", &full).expect("full");
        let px = pixel(&s, 16, 36).expect("full opacity");
        assert_eq!(px[3], 255, "a light press at minimum 100% should stay opaque, got {px:?}");
    }

    #[test]
    fn a_low_flow_builds_and_stays_under_the_fill() {
        let mut s = session();
        let mark = |x: f64, y: f64, flow: Option<f64>| {
            let mut params = json!({"x": x, "y": y, "size": 16.0, "size_min": 16.0, "softness": 0.0, "antialias": 0.0, "color": "#ff000080", "opacity_from_pressure": false});
            if let Some(flow) = flow {
                params["flow"] = json!(flow);
            }
            params
        };
        s.begin_gesture("Paint Pixels");
        s.execute("paint.raster", &mark(16.0, 16.0, Some(1.0))).expect("full");
        let full_once = pixel(&s, 16, 16).expect("full once");
        s.execute(
            "paint.raster",
            &json!({"samples": [{"x": 16.0, "y": 16.0}, {"x": 20.0, "y": 16.0}], "continued": true, "size": 16.0, "size_min": 16.0, "softness": 0.0, "antialias": 0.0, "color": "#ff000080", "opacity_from_pressure": false, "flow": 1.0}),
        )
        .expect("overlap");
        let full_twice = pixel(&s, 16, 16).expect("full twice");
        s.end_gesture();
        assert_eq!(full_once[3], full_twice[3], "flow 1 should keep the stronger dab: {full_once:?} -> {full_twice:?}");
        assert!((120..160).contains(&full_twice[3]), "flow 1 should stay near the fill alpha, got {full_twice:?}");

        s.execute("paint.raster", &mark(40.0, 16.0, None)).expect("omitted");
        let omitted = pixel(&s, 40, 16).expect("omitted flow");
        assert_eq!(omitted[3], full_once[3], "an omitted flow should match flow 1, {omitted:?} vs {full_once:?}");

        s.begin_gesture("Paint Pixels");
        s.execute("paint.raster", &mark(16.0, 40.0, Some(0.45))).expect("light");
        let light = pixel(&s, 16, 40).expect("light")[3];
        s.execute(
            "paint.raster",
            &json!({"samples": [{"x": 16.0, "y": 40.0}, {"x": 20.0, "y": 40.0}], "continued": true, "size": 16.0, "size_min": 16.0, "softness": 0.0, "antialias": 0.0, "color": "#ff000080", "opacity_from_pressure": false, "flow": 0.45}),
        )
        .expect("build");
        let built = pixel(&s, 16, 40).expect("built")[3];
        s.end_gesture();
        assert!(built > light && built < full_once[3], "flow should build under the fill alpha, {light} then {built}, wash {}", full_once[3]);
    }

    #[test]
    fn a_pencil_point_with_no_direction_stays_round() {
        let mut s = session();
        s.execute(
            "paint.raster",
            &json!({"x": 32.0, "y": 24.0, "size": 16.0, "size_min": 16.0, "softness": 0.0, "antialias": 0.0, "color": "#ff0000", "opacity_from_pressure": false, "wedge": 1.0}),
        )
        .expect("point");
        let left = pixel(&s, 24, 24).expect("left")[3];
        let right = pixel(&s, 39, 24).expect("right")[3];
        let below = pixel(&s, 32, 31).expect("below")[3];
        assert!(left > 200 && right > 200 && below > 200, "a click stays a disc, left {left} right {right} below {below}");
    }

    #[test]
    fn a_short_pencil_step_points_along_the_move() {
        let mut s = session();
        s.begin_gesture("Paint Pixels");
        // Diameter 16 spaces at 1.12. Half a pixel adds one dab, with the anchor as its only neighbour.
        s.execute(
            "paint.raster",
            &json!({"samples": [{"x": 32.0, "y": 24.0}, {"x": 32.0, "y": 23.5}], "continued": true, "size": 16.0, "size_min": 16.0, "softness": 0.0, "antialias": 0.0, "color": "#ff0000", "opacity_from_pressure": false, "wedge": 1.0, "flow": 1.0}),
        )
        .expect("step");
        s.end_gesture();
        let below = pixel(&s, 32, 30).expect("below")[3];
        let side = pixel(&s, 24, 23).expect("side")[3];
        assert!(below < 20, "the lip should face back along the stroke, below was {below}");
        assert!(side > 200, "the side of an upward pencil stays wide, side was {side}");
    }

    #[test]
    fn a_picture_dab_keeps_the_graphic() {
        let dir = std::env::temp_dir().join(format!("anicraft-spray-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let side = 16u32;
        let mut cover = vec![0u8; (side as usize) * (side as usize)];
        for y in 0..side {
            for x in 0..side / 2 {
                let i = (y as usize) * (side as usize) + x as usize;
                if let Some(p) = cover.get_mut(i) {
                    *p = 255;
                }
            }
        }
        let mut blob = Vec::new();
        blob.extend_from_slice(&side.to_le_bytes());
        blob.extend_from_slice(&side.to_le_bytes());
        blob.extend_from_slice(&cover);
        for name in ["fine.bin", "heavy.bin", "tight.bin", "can.bin"] {
            std::fs::write(dir.join(name), &blob).expect("tip");
        }
        assert_eq!(anicraft_raster::load_pictures(&dir), 4);
        let mut s = session();
        s.execute(
            "paint.raster",
            &json!({"x": 32.0, "y": 24.0, "size": 16.0, "size_min": 16.0, "softness": 0.0, "antialias": 0.0, "color": "#ff0000", "opacity_from_pressure": false, "picture": 1}),
        )
        .expect("picture");
        let mut ink = 0u32;
        for y in 16..32u32 {
            for x in 24..40u32 {
                if pixel(&s, x, y).is_some_and(|p| p[3] > 0) {
                    ink += 1;
                }
            }
        }
        assert!(ink > 20 && ink < 200, "a half picture should not fill the disc, ink {ink}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
