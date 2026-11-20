//! Raster tool: dabs on a pixel layer the size of the stage.
//!
//! The plate is in the document, so a stroke is one undo step and it is saved
//! with the file. A continued move stamps only the new dabs. A pencil waits
//! for that move, so the opening dab can point along it. Smudge blends
//! colours already on the plate.

use crate::a11y;
use crate::app::{App, Drag, Tool};
use crate::dock::Side;
use crate::theme::{T, Type, font, text};
use anicraft_engine::geom::Vec2;
use anicraft_raster::{Antialias, Grain, HAIRLINE_DIAMETER, Picture, STRETCH_CAP, Sample, Stamp, Stroke, Tip, dab_opacity, dab_spacing, paint_cover, sample_stroke, stamp_mark, stroke_aim};
use egui::{Align2, Rect, Sense, WidgetType, pos2, vec2};
use serde_json::{Value, json};

/// Diameter at full pressure, in stage pixels.
pub const SIZE_MIN: f32 = 1.0;
pub const SIZE_MAX: f32 = 400.0;
/// Diameter at a light pen. 0.05 is the finest mark that still stores a pixel.
pub const THIN_MIN: f32 = anicraft_raster::HAIRLINE_DIAMETER;
/// Stretch at rest. A quarter of the slider, which is a short dry drag.
pub const STRETCH_DEFAULT: f32 = 0.25;

/// One raster brush the list beside the tools can apply. The fill colour stays the palette's.
pub struct RasterPreset {
    pub name: &'static str,
    pub hint: &'static str,
    pub size: f32,
    pub size_min: f32,
    pub softness: f32,
    pub fringe: f32,
    pub pressure_opacity: bool,
    pub opacity_min: f32,
    pub opacity_max: f32,
    /// 1 keeps the strongest dab. Below 1, overlaps build up under the fill alpha.
    pub flow: f32,
    /// 0 is one solid dab. 1 is only speckles.
    pub speckle: f32,
    /// 0 is smooth. 1 is paper tooth.
    pub grain: f32,
    /// 0 is round. 1 lays the tip on its side.
    pub wedge: f32,
    /// A supplied spray picture. None keeps the disc and the grain.
    pub picture: Picture,
}

/// Brushes for the raster layer. Each picture is a stroke of that preset, not a drawing of an icon.
pub const PRESETS: &[RasterPreset] = &[
    RasterPreset { name: tr_noop!("Round"), hint: tr_noop!("A plain disc. The fill colour's alpha is the opacity of the whole stroke."), size: 24.0, size_min: THIN_MIN, softness: 0.0, fringe: 0.5, pressure_opacity: false, opacity_min: 0.0, opacity_max: 1.0, flow: 1.0, speckle: 0.0, grain: 0.0, wedge: 0.0, picture: Picture::None },
    RasterPreset { name: tr_noop!("Pencil"), hint: tr_noop!("A thin line on paper. A light press is faint, and overlaps build up under the fill alpha."), size: 7.0, size_min: 0.4, softness: 0.25, fringe: 0.5, pressure_opacity: true, opacity_min: 0.12, opacity_max: 1.0, flow: 0.35, speckle: 0.0, grain: 0.62, wedge: 0.42, picture: Picture::None },
    RasterPreset { name: tr_noop!("Graphite"), hint: tr_noop!("A hard pencil on paper grain. A hairline when light. Overlaps build the darkness."), size: 4.0, size_min: THIN_MIN, softness: 0.05, fringe: 0.0, pressure_opacity: true, opacity_min: 0.3, opacity_max: 1.0, flow: 0.28, speckle: 0.0, grain: 0.78, wedge: 0.2, picture: Picture::None },
    RasterPreset { name: tr_noop!("Ink"), hint: tr_noop!("A solid mark with a dry, broken edge. Thin at a light press."), size: 12.0, size_min: 0.6, softness: 0.0, fringe: 0.5, pressure_opacity: false, opacity_min: 0.0, opacity_max: 1.0, flow: 1.0, speckle: 0.0, grain: 0.28, wedge: 0.0, picture: Picture::None },
    RasterPreset { name: tr_noop!("Marker"), hint: tr_noop!("A broad, slightly clear stroke. Pressure changes it only a little."), size: 32.0, size_min: 22.0, softness: 0.08, fringe: 0.5, pressure_opacity: true, opacity_min: 0.55, opacity_max: 0.8, flow: 1.0, speckle: 0.0, grain: 0.0, wedge: 0.0, picture: Picture::None },
    RasterPreset { name: tr_noop!("Paint"), hint: tr_noop!("A soft loaded brush. A light press is thinner and more transparent."), size: 36.0, size_min: 10.0, softness: 0.45, fringe: 1.0, pressure_opacity: true, opacity_min: 0.3, opacity_max: 1.0, flow: 1.0, speckle: 0.0, grain: 0.18, wedge: 0.0, picture: Picture::None },
    RasterPreset { name: tr_noop!("Fine Spray"), hint: tr_noop!("The fine spray picture. Each dab is that graphic, turned."), size: 140.0, size_min: 40.0, softness: 0.0, fringe: 0.0, pressure_opacity: false, opacity_min: 0.0, opacity_max: 1.0, flow: 1.0, speckle: 0.0, grain: 0.0, wedge: 0.0, picture: Picture::Fine },
    RasterPreset { name: tr_noop!("Heavy Spray"), hint: tr_noop!("The heavy spray picture. Each dab is that graphic, turned."), size: 150.0, size_min: 48.0, softness: 0.0, fringe: 0.0, pressure_opacity: false, opacity_min: 0.0, opacity_max: 1.0, flow: 1.0, speckle: 0.0, grain: 0.0, wedge: 0.0, picture: Picture::Heavy },
    RasterPreset { name: tr_noop!("Tight Spray"), hint: tr_noop!("The tight spray picture. Each dab is that graphic, turned."), size: 64.0, size_min: 12.0, softness: 0.0, fringe: 0.0, pressure_opacity: false, opacity_min: 0.0, opacity_max: 1.0, flow: 1.0, speckle: 0.0, grain: 0.0, wedge: 0.0, picture: Picture::Tight },
    RasterPreset { name: tr_noop!("Airbrush"), hint: tr_noop!("The spray-can picture. Each dab is that graphic, turned."), size: 120.0, size_min: 36.0, softness: 0.0, fringe: 0.0, pressure_opacity: false, opacity_min: 0.0, opacity_max: 1.0, flow: 1.0, speckle: 0.0, grain: 0.0, wedge: 0.0, picture: Picture::Can },
    RasterPreset { name: tr_noop!("Charcoal"), hint: tr_noop!("A dry stick. Grain and a little scatter. Pressure builds the darkness without going solid."), size: 20.0, size_min: 3.0, softness: 0.75, fringe: 1.0, pressure_opacity: true, opacity_min: 0.08, opacity_max: 0.7, flow: 1.0, speckle: 0.35, grain: 0.82, wedge: 0.15, picture: Picture::None },
    RasterPreset { name: tr_noop!("Coloured Pencil"), hint: tr_noop!("Laid on its side. A sharp edge and a soft fade in the direction of the stroke, with paper grain."), size: 52.0, size_min: 1.5, softness: 0.62, fringe: 1.0, pressure_opacity: true, opacity_min: 0.08, opacity_max: 0.9, flow: 0.42, speckle: 0.08, grain: 0.8, wedge: 0.92, picture: Picture::None },
];

/// Copies a preset onto the raster brush. The fill colour, and the vector brushes, stay put.
pub fn apply_preset(pixel: &mut PixelPaint, preset: &RasterPreset) {
    pixel.size = preset.size;
    pixel.size_min = preset.size_min.clamp(THIN_MIN, preset.size);
    pixel.softness = preset.softness.clamp(0.0, 1.0);
    pixel.fringe = preset.fringe;
    pixel.pressure_opacity = preset.pressure_opacity;
    pixel.opacity_min = preset.opacity_min.clamp(0.0, preset.opacity_max.clamp(0.0, 1.0));
    pixel.opacity_max = preset.opacity_max.clamp(pixel.opacity_min, 1.0);
    pixel.flow = if preset.flow.is_finite() { preset.flow.clamp(0.0, 1.0) } else { 1.0 };
    pixel.speckle = if preset.speckle.is_finite() { preset.speckle.clamp(0.0, 1.0) } else { 0.0 };
    pixel.grain = if preset.grain.is_finite() { preset.grain.clamp(0.0, 1.0) } else { 0.0 };
    pixel.wedge = if preset.wedge.is_finite() { preset.wedge.clamp(0.0, 1.0) } else { 0.0 };
    pixel.picture = preset.picture;
    pixel.smudge = false;
    pixel.erase = false;
}

/// True when the brush still has this preset's settings.
pub fn is_preset(pixel: &PixelPaint, preset: &RasterPreset) -> bool {
    let close = |a: f32, b: f32| (a - b).abs() < 0.02;
    !pixel.smudge && !pixel.erase && close(pixel.size, preset.size) && close(pixel.size_min, preset.size_min) && close(pixel.softness, preset.softness) && close(pixel.fringe, preset.fringe) && pixel.pressure_opacity == preset.pressure_opacity && close(pixel.opacity_min, preset.opacity_min) && close(pixel.opacity_max, preset.opacity_max) && close(pixel.flow, preset.flow) && close(pixel.speckle, preset.speckle) && close(pixel.grain, preset.grain) && close(pixel.wedge, preset.wedge) && pixel.picture == preset.picture
}

const PREVIEW_W: u32 = 72;
const PREVIEW_H: u32 = 36;

/// A short stroke of `preset`, straight RGBA, so the list can show the brush itself.
pub fn preview_rgba(preset: &RasterPreset) -> Vec<u8> {
    if preset.picture != Picture::None {
        let mut mask = vec![0u8; (PREVIEW_W as usize) * (PREVIEW_H as usize)];
        let tip = Tip { radius: 14.0, softness: 0.0, antialias: Antialias::Off, color: [255, 255, 255, 255], opacity: 1.0, erase: false };
        let _ = stamp_mark(&mut mask, PREVIEW_W, PREVIEW_H, 36.0, 18.0, tip, Stamp { flow: 1.0, grain: Grain::ROUND, aim_x: 0.0, aim_y: 0.0, picture: preset.picture, turn: 0.4 });
        let mut pixels = vec![0u8; mask.len() * 4];
        let _ = paint_cover(&mut pixels, &mask, [255, 255, 255, 255], false);
        return pixels;
    }
    let show = preset.size.clamp(5.0, 16.0);
    let scale = show / preset.size.max(1.0);
    let size_min = (preset.size_min * scale).clamp(HAIRLINE_DIAMETER, show);
    let stroke = Stroke { size: show, size_min, softness: preset.softness, antialias: fringe_aa(preset.fringe), color: [255, 255, 255, 255], erase: false, opacity_from_pressure: preset.pressure_opacity, opacity_min: preset.opacity_min, opacity_max: preset.opacity_max, flow: preset.flow };
    let samples = [Sample { x: 6.0, y: 26.0, pressure: 0.15 }, Sample { x: 36.0, y: 12.0, pressure: 1.0 }, Sample { x: 66.0, y: 24.0, pressure: 0.35 }];
    let spaced = sample_stroke(&samples, |mid| dab_spacing(preview_diameter(stroke, mid)));
    let mut mask = vec![0u8; (PREVIEW_W as usize) * (PREVIEW_H as usize)];
    let grain = Grain { speckle: preset.speckle, grain: preset.grain, wedge: preset.wedge };
    for (i, sample) in spaced.iter().enumerate() {
        let tip = Tip { radius: preview_diameter(stroke, sample.pressure) * 0.5, softness: stroke.softness, antialias: stroke.antialias, color: stroke.color, opacity: dab_opacity(stroke, sample.pressure), erase: false };
        let (aim_x, aim_y) = stroke_aim(&spaced, i);
        let _ = stamp_mark(&mut mask, PREVIEW_W, PREVIEW_H, sample.x, sample.y, tip, Stamp::plain(stroke.flow, grain, aim_x, aim_y));
    }
    let mut pixels = vec![0u8; mask.len() * 4];
    let _ = paint_cover(&mut pixels, &mask, [255, 255, 255, 255], false);
    pixels
}

fn fringe_aa(fringe: f32) -> Antialias {
    if !fringe.is_finite() || fringe <= 0.0 {
        Antialias::Off
    } else if (fringe - 0.5).abs() < 0.01 {
        Antialias::On
    } else {
        Antialias::Px(fringe)
    }
}

fn preview_diameter(stroke: Stroke, pressure: f32) -> f32 {
    let pressure = if pressure.is_finite() { pressure.clamp(0.0, 1.0) } else { 0.0 };
    let size = if stroke.size.is_finite() { stroke.size.max(0.0) } else { 0.0 };
    let low = if stroke.size_min.is_finite() { stroke.size_min.clamp(0.0, size) } else { 0.0 };
    low + (size - low) * pressure
}

fn preview_texture(ctx: &egui::Context, preset: &RasterPreset) -> egui::TextureHandle {
    let id = egui::Id::new(("raster-preset-preview", preset.name));
    if let Some(tex) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return tex;
    }
    let pixels = preview_rgba(preset);
    let image = egui::ColorImage::from_rgba_unmultiplied([PREVIEW_W as usize, PREVIEW_H as usize], &pixels);
    let tex = ctx.load_texture(format!("raster-preset-{}", preset.name), image, egui::TextureOptions::NEAREST);
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    tex
}

/// The raster brush list, on the stage side of the tools. Hidden for every other tool.
pub fn preset_bar(app: &mut App, ui: &mut egui::Ui, side: Side) {
    if app.tool != Tool::Pixel {
        return;
    }
    let panel = if side == Side::Right { egui::Panel::right("raster_presets") } else { egui::Panel::left("raster_presets") };
    let frame = egui::Frame::NONE.fill(T.panel).inner_margin(egui::Margin::symmetric(6, 8));
    panel.resizable(false).exact_size(104.0).frame(frame).show(ui, |ui| {
        ui.label(text(Type::Caption, tr!("Brushes")).color(T.text_dim));
        ui.add_space(4.0);
        egui::ScrollArea::vertical().id_salt("raster_presets").auto_shrink([false, false]).show(ui, |ui| {
            for preset in PRESETS {
                preset_row(app, ui, preset);
                ui.add_space(4.0);
            }
        });
    });
}

fn preset_row(app: &mut App, ui: &mut egui::Ui, preset: &RasterPreset) {
    let on = is_preset(&app.pixel, preset);
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 56.0), Sense::click());
    let bg = if on {
        T.active
    } else if resp.hovered() {
        T.header
    } else {
        T.field
    };
    ui.painter().rect_filled(rect, T.rc(), bg);
    let tex = preview_texture(ui.ctx(), preset);
    let img = Rect::from_min_size(rect.min + vec2(6.0, 4.0), vec2((rect.width() - 12.0).max(1.0), PREVIEW_H as f32));
    let ink = if on { T.on_active } else { T.text };
    ui.painter().image(tex.id(), img, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), ink);
    let name = crate::i18n::t(preset.name);
    ui.painter().text(pos2(rect.left() + 6.0, rect.bottom() - 3.0), Align2::LEFT_BOTTOM, name, font(Type::Caption), ink);
    let resp = resp.on_hover_text(crate::i18n::t(preset.hint));
    a11y::control(ui, &resp, WidgetType::Button, name, Some(on));
    if resp.clicked() {
        apply_preset(&mut app.pixel, preset);
    }
}

/// View-state for the Raster tool. The pixels themselves live on the layer.
pub struct PixelPaint {
    pub size: f32,
    pub size_min: f32,
    pub softness: f32,
    /// 0 is a hard edge. 0.5 is the half-pixel fringe. 1 and 2 are wider fringes.
    pub fringe: f32,
    pub erase: bool,
    /// Blend colours already on the layer instead of laying down the fill.
    pub smudge: bool,
    /// 0 is dry paint that stays put. 1 is the strongest drag, still below a full replace.
    pub stretch: f32,
    /// Pen pressure scales the stroke's opacity between [`Self::opacity_min`] and [`Self::opacity_max`].
    /// Off: the fill colour's alpha is the opacity of the whole stroke.
    pub pressure_opacity: bool,
    /// How clear a light press is, as a part of the fill colour's alpha. 0 is clear.
    pub opacity_min: f32,
    /// How opaque a full press is, as a part of the fill colour's alpha. 1 is the fill's alpha.
    pub opacity_max: f32,
    /// How much of the stroke one dab lays down, 0 to 1. 1 keeps the strongest dab.
    /// Below 1, overlaps build up and the fill colour's alpha still caps the stroke.
    pub flow: f32,
    /// 0 is one solid dab. 1 sprays tiny dots of mixed size and opacity.
    pub speckle: f32,
    /// 0 is smooth. 1 is paper tooth, locked to the page.
    pub grain: f32,
    /// 0 is round. 1 lays the tip on its side: a sharp edge and a soft fade along the stroke.
    pub wedge: f32,
    /// A supplied spray picture. None keeps the disc and the grain.
    pub picture: Picture,
}

impl Default for PixelPaint {
    fn default() -> Self {
        Self { size: 24.0, size_min: THIN_MIN, softness: 0.0, fringe: 0.5, erase: false, smudge: false, stretch: STRETCH_DEFAULT, pressure_opacity: false, opacity_min: 0.0, opacity_max: 1.0, flow: 1.0, speckle: 0.0, grain: 0.0, wedge: 0.0, picture: Picture::None }
    }
}

/// Engine mix for a Stretch slider value. The top of the slider is [`STRETCH_CAP`], not a full replace.
pub fn stretch_strength(stretch: f32) -> f32 {
    let t = if stretch.is_finite() { stretch.clamp(0.0, 1.0) } else { 0.0 };
    t * STRETCH_CAP
}

/// A mouse (`None`) paints at full pressure. A pen uses its reading.
pub fn unit(pressure: Option<f32>) -> f32 {
    match pressure {
        Some(p) if p.is_finite() => p.clamp(0.0, 1.0),
        _ => 1.0,
    }
}

/// One dab where the press landed, unless the tip is laid on its side. That
/// press has no direction yet, so the first dab waits for [`move_to`]. The
/// whole drag is one undo step.
pub fn begin(app: &mut App, p: Vec2, pressure: Option<f32>) -> Drag {
    let pressure = unit(pressure);
    app.s.begin_gesture(if app.pixel.smudge { tr_noop!("Smudge") } else { tr_noop!("Paint Pixels") });
    app.pixel_queue.clear();
    app.pixel_span = None;
    grow_span(app, p.x, p.y);
    let pending = lays_on_its_side(app);
    if !pending {
        let sample = Sample { x: p.x, y: p.y, pressure };
        if !stamp(app, &[sample], false) {
            app.s.end_gesture();
            return Drag::None;
        }
    }
    Drag::Pixel { x: p.x, y: p.y, pressure, pending }
}

/// True when the next dab is a pencil on its side. Smudge ignores that lay.
fn lays_on_its_side(app: &App) -> bool {
    !app.pixel.smudge && app.pixel.wedge.is_finite() && app.pixel.wedge > 0.0
}

/// A click, with no drag left behind. One undo step.
pub fn tap(app: &mut App, p: Vec2, pressure: Option<f32>) {
    let _ = stamp(app, &[Sample { x: p.x, y: p.y, pressure: unit(pressure) }], false);
}

/// How many new samples one frame stamps. The rest stay queued so a hitch cannot
/// walk a whole custom brush in one go (ADR 0119).
pub const STAMPS_PER_FRAME: usize = 24;

/// Dabs from the last sample to `to`. Spacing happens once, in the command.
/// The first move of a pencil paints the press as well, aimed along this step.
pub fn move_to(app: &mut App, from: (f32, f32, f32), to: egui::Pos2, pressure: Option<f32>) {
    if !matches!(app.drag, Drag::Pixel { .. }) {
        app.drag = Drag::Pixel { x: from.0, y: from.1, pressure: from.2, pending: false };
    }
    feed(app, &[Sample { x: to.x, y: to.y, pressure: unit(pressure) }]);
}

/// Queue `fresh` (stage space) and stamp a bounded run. The pen-up path calls [`finish`],
/// which stamps whatever is still queued.
pub fn feed(app: &mut App, fresh: &[Sample]) {
    if !matches!(app.drag, Drag::Pixel { .. }) {
        return;
    }
    for sample in fresh {
        if !sample.x.is_finite() || !sample.y.is_finite() {
            continue;
        }
        let pressure = if sample.pressure.is_finite() { sample.pressure.clamp(0.0, 1.0) } else { 1.0 };
        let same = |x: f32, y: f32, p: f32| (x - sample.x).abs() <= 0.01 && (y - sample.y).abs() <= 0.01 && (p - pressure).abs() <= 0.001;
        let dup = app.pixel_queue.last().is_some_and(|&(x, y, p)| same(x, y, p))
            || (app.pixel_queue.is_empty() && matches!(app.drag, Drag::Pixel { x, y, pressure: p, .. } if same(x, y, p)));
        if dup {
            continue;
        }
        app.pixel_queue.push((sample.x, sample.y, pressure));
        grow_span(app, sample.x, sample.y);
    }
    let _ = flush(app, STAMPS_PER_FRAME);
}

/// Ends the drag. A pencil press that never moved lays a round dab.
/// Samples still queued are stamped first, so the end of the line is in the plate.
pub fn finish(app: &mut App) {
    while !app.pixel_queue.is_empty() {
        if !flush(app, app.pixel_queue.len()) {
            break;
        }
    }
    if let Drag::Pixel { x, y, pressure, pending: true } = app.drag {
        grow_span(app, x, y);
        let _ = stamp(app, &[Sample { x, y, pressure }], false);
    }
    app.pixel_queue.clear();
    app.s.end_gesture();
}

fn grow_span(app: &mut App, x: f32, y: f32) {
    let pad = app.pixel.size.abs().max(app.pixel.size_min.abs()) * 0.5 + 4.0;
    if !pad.is_finite() {
        return;
    }
    let (x0, y0, x1, y1) = (x - pad, y - pad, x + pad, y + pad);
    app.pixel_span = Some(match app.pixel_span {
        Some([a, b, c, d]) => [a.min(x0), b.min(y0), c.max(x1), d.max(y1)],
        None => [x0, y0, x1, y1],
    });
}

/// Stamp up to `limit` queued samples. The drag's point is the last one that landed.
fn flush(app: &mut App, limit: usize) -> bool {
    let Drag::Pixel { x, y, pressure, pending } = app.drag else { return false };
    let n = limit.min(app.pixel_queue.len());
    if n == 0 {
        return true;
    }
    let mut samples = Vec::with_capacity(n + 1);
    samples.push(Sample { x, y, pressure });
    let taken: Vec<(f32, f32, f32)> = app.pixel_queue.drain(..n).collect();
    for (sx, sy, sp) in taken {
        samples.push(Sample { x: sx, y: sy, pressure: sp });
        grow_span(app, sx, sy);
    }
    if !stamp(app, &samples, !pending) {
        app.s.end_gesture();
        app.drag = Drag::None;
        app.pixel_queue.clear();
        return false;
    }
    if let Some(last) = samples.last() {
        app.drag = Drag::Pixel { x: last.x, y: last.y, pressure: last.pressure, pending: false };
    }
    true
}

/// The fill colour, alpha included, so the colour picker's Alpha is the stroke's opacity.
fn fill_hex(c: anicraft_engine::doc::Rgba) -> String {
    let b = |v: f32| if v.is_finite() { (v.clamp(0.0, 1.0) * 255.0).round() as u8 } else { 0 };
    crate::colour::to_hex([b(c.r), b(c.g), b(c.b), b(c.a)], true)
}

fn stamp(app: &mut App, points: &[Sample], continued: bool) -> bool {
    if points.is_empty() {
        return true;
    }
    let samples: Vec<Value> = points.iter().map(|s| json!({"x": s.x, "y": s.y, "pressure": s.pressure})).collect();
    app.run(
        "paint.raster",
        json!({
            "samples": samples,
            "size": app.pixel.size,
            "size_min": app.pixel.size_min,
            "softness": app.pixel.softness,
            "antialias": app.pixel.fringe,
            "color": fill_hex(app.s.style.fill),
            "erase": app.pixel.erase && !app.pixel.smudge,
            "smudge": app.pixel.smudge,
            "strength": stretch_strength(app.pixel.stretch),
            "opacity_from_pressure": app.pixel.pressure_opacity,
            "opacity_min": app.pixel.opacity_min,
            "opacity_max": app.pixel.opacity_max,
            "flow": app.pixel.flow,
            "speckle": app.pixel.speckle,
            "grain": app.pixel.grain,
            "wedge": app.pixel.wedge,
            "picture": app.pixel.picture.index(),
            "continued": continued,
        }),
    )
    .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use anicraft_engine::Session;
    use anicraft_engine::doc::sample;
    use anicraft_engine::doc::LayerKind;

    #[test]
    fn a_raster_stroke_paints_the_stage_and_undoes_in_one_step() {
        let ctx = egui::Context::default();
        let mut app = App::with_context(&ctx, Session::new(sample::demo()), Some(std::path::PathBuf::from("unused.png")));
        app.run("doc.set_stage", json!({"width": 64, "height": 48})).expect("stage");
        app.s.style.set_fill_color(anicraft_engine::doc::Rgba::new(1.0, 0.0, 0.0, 1.0));
        let before = app.s.history_len();
        let drag = begin(&mut app, Vec2::new(10.0, 12.0), None);
        assert!(matches!(drag, Drag::Pixel { .. }));
        move_to(&mut app, (10.0, 12.0, 1.0), egui::pos2(40.0, 12.0), None);
        app.s.end_gesture();
        assert_eq!(app.s.history_len(), before + 1);
        let plate = app.s.active_layer().and_then(|l| l.keyframe_at(0)).and_then(|k| k.plate.as_ref()).expect("plate");
        assert_eq!(app.s.active_layer().map(|l| l.kind), Some(LayerKind::Raster));
        assert_eq!((plate.width(), plate.height()), (64, 48));
        let image = plate.image();
        let o = (12 * 64 + 10) * 4;
        let bytes = image.rgba.get(o..o + 4).expect("pixel");
        assert!(bytes[0] > 200 && bytes[3] > 200, "{bytes:?}");
        app.s.undo().expect("undo");
        assert!(app.s.timeline().layers.iter().all(|l| l.kind != LayerKind::Raster));
    }

    fn alpha_at(app: &App, x: u32, y: u32) -> u8 {
        let plate = app.s.active_layer().and_then(|l| l.keyframe_at(0)).and_then(|k| k.plate.as_ref());
        let Some(plate) = plate else { return 0 };
        let image = plate.image();
        let o = ((y as usize) * (image.width as usize) + x as usize) * 4;
        image.rgba.get(o + 3).copied().unwrap_or(0)
    }

    #[test]
    fn a_round_press_inks_before_the_drag_moves() {
        let ctx = egui::Context::default();
        let mut app = App::with_context(&ctx, Session::new(sample::demo()), Some(std::path::PathBuf::from("unused.png")));
        app.run("doc.set_stage", json!({"width": 64, "height": 48})).expect("stage");
        app.s.style.set_fill_color(anicraft_engine::doc::Rgba::new(1.0, 0.0, 0.0, 1.0));
        let drag = begin(&mut app, Vec2::new(32.0, 24.0), None);
        assert!(matches!(drag, Drag::Pixel { pending: false, .. }));
        assert!(alpha_at(&app, 32, 24) > 200);
    }

    #[test]
    fn a_pencil_drag_waits_for_a_direction() {
        let ctx = egui::Context::default();
        let mut app = App::with_context(&ctx, Session::new(sample::demo()), Some(std::path::PathBuf::from("unused.png")));
        app.run("doc.set_stage", json!({"width": 64, "height": 48})).expect("stage");
        app.s.style.set_fill_color(anicraft_engine::doc::Rgba::new(1.0, 0.0, 0.0, 1.0));
        app.pixel.wedge = 1.0;
        app.pixel.size = 16.0;
        app.pixel.size_min = 16.0;
        app.pixel.softness = 0.0;
        app.pixel.fringe = 0.0;
        app.pixel.flow = 1.0;
        app.pixel.pressure_opacity = false;
        let before = app.s.history_len();
        app.drag = begin(&mut app, Vec2::new(32.0, 40.0), None);
        assert!(matches!(app.drag, Drag::Pixel { pending: true, .. }));
        assert!(app.s.timeline().layers.iter().all(|l| l.kind != LayerKind::Raster));
        move_to(&mut app, (32.0, 40.0, 1.0), egui::pos2(32.0, 16.0), None);
        finish(&mut app);
        assert_eq!(app.s.history_len(), before + 1);
        let side = alpha_at(&app, 24, 40);
        let trail = alpha_at(&app, 32, 47);
        assert!(side > 200, "the pencil should be wide across the stroke, side {side}");
        assert!(trail < 20, "the start should be cut on the trailing side, trail {trail}");
    }

    #[test]
    fn a_pencil_press_without_a_drag_is_a_round_dot() {
        let ctx = egui::Context::default();
        let mut app = App::with_context(&ctx, Session::new(sample::demo()), Some(std::path::PathBuf::from("unused.png")));
        app.run("doc.set_stage", json!({"width": 64, "height": 48})).expect("stage");
        app.s.style.set_fill_color(anicraft_engine::doc::Rgba::new(1.0, 0.0, 0.0, 1.0));
        app.pixel.wedge = 1.0;
        app.pixel.size = 16.0;
        app.pixel.size_min = 16.0;
        app.pixel.softness = 0.0;
        app.pixel.fringe = 0.0;
        app.pixel.flow = 1.0;
        app.pixel.pressure_opacity = false;
        let before = app.s.history_len();
        app.drag = begin(&mut app, Vec2::new(32.0, 24.0), None);
        assert_eq!(alpha_at(&app, 32, 24), 0);
        finish(&mut app);
        assert_eq!(app.s.history_len(), before + 1);
        let left = alpha_at(&app, 24, 24);
        let right = alpha_at(&app, 39, 24);
        let below = alpha_at(&app, 32, 31);
        assert!(left > 200 && right > 200 && below > 200, "a click stays a disc, left {left} right {right} below {below}");
    }

    #[test]
    fn a_hook_is_ink_and_the_straight_chord_is_not() {
        let ctx = egui::Context::default();
        let mut app = App::with_context(&ctx, Session::new(sample::demo()), Some(std::path::PathBuf::from("unused.png")));
        app.run("doc.set_stage", json!({"width": 64, "height": 48})).expect("stage");
        app.s.style.set_fill_color(anicraft_engine::doc::Rgba::new(1.0, 0.0, 0.0, 1.0));
        app.pixel.size = 3.0;
        app.pixel.size_min = 3.0;
        app.pixel.fringe = 0.0;
        app.pixel.softness = 0.0;
        app.drag = begin(&mut app, Vec2::new(8.0, 8.0), None);
        feed(&mut app, &[Sample { x: 8.0, y: 28.0, pressure: 1.0 }, Sample { x: 40.0, y: 28.0, pressure: 1.0 }]);
        finish(&mut app);
        assert!(alpha_at(&app, 8, 28) > 200, "the corner of the hook is ink");
        assert_eq!(alpha_at(&app, 24, 18), 0, "a chord from the press to the end would have painted here");
    }

    #[test]
    fn a_long_move_stamps_the_start_and_keeps_the_tail() {
        let ctx = egui::Context::default();
        let mut app = App::with_context(&ctx, Session::new(sample::demo()), Some(std::path::PathBuf::from("unused.png")));
        app.run("doc.set_stage", json!({"width": 80, "height": 24})).expect("stage");
        app.s.style.set_fill_color(anicraft_engine::doc::Rgba::new(1.0, 0.0, 0.0, 1.0));
        app.pixel.size = 2.0;
        app.pixel.size_min = 2.0;
        app.pixel.fringe = 0.0;
        app.pixel.softness = 0.0;
        app.drag = begin(&mut app, Vec2::new(2.0, 10.0), None);
        let pts: Vec<Sample> = (0..40).map(|i| Sample { x: 4.0 + i as f32, y: 10.0, pressure: 1.0 }).collect();
        feed(&mut app, &pts);
        assert!(alpha_at(&app, 4, 10) > 0, "the first queued sample is ink");
        assert_eq!(alpha_at(&app, 43, 10), 0, "the tail waits for the next frame");
        assert!(!app.pixel_queue.is_empty());
        finish(&mut app);
        assert!(alpha_at(&app, 43, 10) > 0, "lifting the pen stamps the rest");
        assert!(app.pixel_queue.is_empty());
    }

    #[test]
    fn stretch_defaults_dry_and_never_reaches_a_full_replace() {
        assert!(STRETCH_DEFAULT < 0.5);
        assert_eq!(stretch_strength(0.0), 0.0);
        let dry = stretch_strength(STRETCH_DEFAULT);
        assert!(dry > 0.05 && dry < 0.12, "{dry}");
        assert!(stretch_strength(1.0) < 0.5);
        assert_eq!(stretch_strength(1.0), STRETCH_CAP);
    }

    #[test]
    fn the_fill_alpha_is_the_stroke_until_pressure_opacity_is_on() {
        let ctx = egui::Context::default();
        let mut app = App::with_context(&ctx, Session::new(sample::demo()), Some(std::path::PathBuf::from("unused.png")));
        app.run("doc.set_stage", json!({"width": 64, "height": 48})).expect("stage");
        app.s.style.set_fill_color(anicraft_engine::doc::Rgba::new(1.0, 0.0, 0.0, 0.5));
        app.pixel.size = 16.0;
        app.pixel.size_min = 16.0;
        app.pixel.fringe = 0.0;
        assert!(!app.pixel.pressure_opacity);
        let drag = begin(&mut app, Vec2::new(16.0, 16.0), Some(0.2));
        assert!(matches!(drag, Drag::Pixel { .. }));
        app.s.end_gesture();
        let plate = app.s.active_layer().and_then(|l| l.keyframe_at(0)).and_then(|k| k.plate.as_ref()).expect("plate");
        let image = plate.image();
        let o = (16 * 64 + 16) * 4;
        let bytes = image.rgba.get(o..o + 4).expect("pixel");
        assert_eq!(bytes, &[255, 0, 0, 128], "a light press should keep the fill alpha, got {bytes:?}");

        app.pixel.pressure_opacity = true;
        app.pixel.opacity_min = 0.0;
        app.pixel.opacity_max = 0.5;
        app.s.style.set_fill_color(anicraft_engine::doc::Rgba::new(0.0, 0.0, 1.0, 1.0));
        let drag = begin(&mut app, Vec2::new(40.0, 16.0), Some(1.0));
        assert!(matches!(drag, Drag::Pixel { .. }));
        app.s.end_gesture();
        let plate = app.s.active_layer().and_then(|l| l.keyframe_at(0)).and_then(|k| k.plate.as_ref()).expect("plate");
        let image = plate.image();
        let o = (16 * 64 + 40) * 4;
        let bytes = image.rgba.get(o..o + 4).expect("pixel");
        assert_eq!(bytes[2], 255, "{bytes:?}");
        assert_eq!(bytes[3], 128, "full pressure at maximum 50% should be half the fill, got {bytes:?}");
    }

    #[test]
    fn presets_preview_as_strokes_and_round_is_the_default() {
        let names: Vec<&str> = PRESETS.iter().map(|p| p.name).collect();
        assert!(names.len() >= 6);
        assert_eq!(names.iter().collect::<std::collections::BTreeSet<_>>().len(), names.len(), "two presets share a name");
        let pixel = PixelPaint::default();
        let round = PRESETS.iter().find(|p| p.name == "Round").expect("round");
        assert!((pixel.flow - 1.0).abs() < 1e-6);
        assert!((round.flow - 1.0).abs() < 1e-6);
        assert!(is_preset(&pixel, round));
        let mut next = pixel;
        let pencil = PRESETS.iter().find(|p| p.name == "Pencil").expect("pencil");
        assert!(pencil.flow < 0.5);
        let graphite = PRESETS.iter().find(|p| p.name == "Graphite").expect("graphite");
        assert!(graphite.flow < pencil.flow);
        apply_preset(&mut next, pencil);
        assert!(is_preset(&next, pencil));
        assert!(!is_preset(&next, round));
        let ink = preview_rgba(PRESETS.iter().find(|p| p.name == "Ink").expect("ink"));
        let airbrush = PRESETS.iter().find(|p| p.name == "Airbrush").expect("air");
        assert_eq!(airbrush.picture, Picture::Can);
        let fine = PRESETS.iter().find(|p| p.name == "Fine Spray").expect("fine");
        let heavy = PRESETS.iter().find(|p| p.name == "Heavy Spray").expect("heavy");
        let tight = PRESETS.iter().find(|p| p.name == "Tight Spray").expect("tight");
        assert_eq!([fine.picture, heavy.picture, tight.picture, airbrush.picture], [Picture::Fine, Picture::Heavy, Picture::Tight, Picture::Can]);
        let stick = PRESETS.iter().find(|p| p.name == "Coloured Pencil").expect("coloured pencil");
        assert!(stick.wedge > 0.5 && stick.grain > 0.5);
        apply_preset(&mut next, stick);
        assert!(is_preset(&next, stick));
        apply_preset(&mut next, airbrush);
        assert!(is_preset(&next, airbrush));
        assert!(!is_preset(&next, round));
        let air = preview_rgba(airbrush);
        assert_eq!(ink.len(), (72 * 36 * 4) as usize);
        assert_eq!(air.len(), ink.len());
        let peak = |buf: &[u8]| buf.iter().skip(3).step_by(4).copied().max().unwrap_or(0);
        assert!(peak(&ink) > 200, "ink preview peak {}", peak(&ink));
    }

    #[test]
    fn the_raster_properties_are_dials_in_one_band() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut app = App::with_context(&ctx, Session::new(anicraft_engine::doc::Document::default()), Some(std::path::PathBuf::from("unused.png")));
        app.tool = Tool::Pixel;
        let screen = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 940.0))), ..Default::default() };
        let mut nodes = Vec::new();
        for _ in 0..3 {
            let mut out = ctx.run_ui(screen.clone(), |ui| app.frame(ui));
            out.textures_delta.clear();
            if let Some(update) = out.platform_output.accesskit_update {
                nodes = update.nodes;
            }
        }
        let names: Vec<String> = nodes.iter().filter_map(|(_, n)| n.label().map(str::to_string)).collect();
        for need in ["Fill colour", "Pressure opacity", "Erase", "Smudge", "Pixel brush antialias", "Pixel brush size", "Pixel brush thinnest", "Raster flow", "Pixel brush softness", "Raster spray", "Raster grain", "Raster pencil lay"] {
            assert!(names.iter().any(|n| n == need), "missing {need} in {names:?}");
        }
        assert!(!names.iter().any(|n| n == "Minimum opacity" || n == "Paint stretch"), "pressure range and stretch stay hidden until those modes are on: {names:?}");
        let box_of = |label: &str| -> (f64, f64, f64, f64) {
            nodes.iter().find(|(_, n)| n.label() == Some(label)).and_then(|(_, n)| n.bounds()).map(|b| (b.x0, b.y0, b.x1, b.y1)).unwrap_or_else(|| panic!("no bounds for {label}"))
        };
        let size = box_of("Pixel brush size");
        let spray = box_of("Raster spray");
        let aa = box_of("Pixel brush antialias");
        let width = |b: (f64, f64, f64, f64)| b.2 - b.0;
        let dial_w = width(size);
        assert!(dial_w >= 56.0 && dial_w <= 84.0, "a raster dial stays the large size, got {dial_w}");
        assert!((width(spray) - dial_w).abs() < 1.0, "spray is a dial like size");
        let expect = ((dial_w - 4.0) * 0.5).max(28.0);
        assert!((width(aa) - expect).abs() < 1.5, "antialias is a small square: {} vs {expect}", width(aa));
        let ys = [size.1, spray.1, aa.1];
        let span = ys.iter().copied().fold(f64::MIN, f64::max) - ys.iter().copied().fold(f64::MAX, f64::min);
        assert!(span < 280.0, "the raster face is a short band, not a tall form: span {span}");

        app.pixel.pressure_opacity = true;
        app.pixel.smudge = true;
        for _ in 0..2 {
            let mut out = ctx.run_ui(screen.clone(), |ui| app.frame(ui));
            out.textures_delta.clear();
            if let Some(update) = out.platform_output.accesskit_update {
                nodes = update.nodes;
            }
        }
        let names: Vec<String> = nodes.iter().filter_map(|(_, n)| n.label().map(str::to_string)).collect();
        assert!(names.iter().any(|n| n == "Minimum opacity") && names.iter().any(|n| n == "Maximum opacity"), "pressure opens its range: {names:?}");
        assert!(names.iter().any(|n| n == "Paint stretch"), "smudge opens stretch: {names:?}");
        assert!(!names.iter().any(|n| n == "Raster spray"), "smudge hides the mark dials: {names:?}");
    }
}
