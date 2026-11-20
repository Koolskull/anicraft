//! The selected layer's properties, in the Properties panel (ADR 0123). The same fields the
//! Layer Properties window used to hold (ADR 0061, ADR 0070, ADR 0111): name, show and lock,
//! type, outline colour, view as outlines, onion skin, and opacity, and a mask layer's
//! mask type. Layer height stays on the timeline's layer menu. Each change is a `layer.set_properties`
//! as it happens. Double-clicking a layer's
//! icon, **Properties…**, or Modify › Timeline › Layer Properties shows the panel and selects
//! the name.

use crate::a11y::Named;
use crate::app::App;
use crate::icons::Icon;
use crate::theme::{self, Type, font, text};
use anicraft_engine::doc::{LayerId, LayerKind, Rgba};
use egui::{Key, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use serde_json::json;

/// Shows the Properties panel on `layer` (default: the active layer) and selects its name
/// once. Does not open a window, and does not select the layer's artwork.
pub fn open(app: &mut App, layer: Option<LayerId>) {
    let id = layer.unwrap_or(app.s.layer);
    if app.s.timeline().layer(id).is_none() {
        return;
    }
    if app.s.layer != id {
        app.run("layer.select", json!({"layer": id.0}));
    }
    app.show_panel(crate::dock::Panel::Properties);
    app.layer_name_focus = Some(id);
}

/// What the panel edits, copied off the layer before any command runs.
struct Snap {
    id: LayerId,
    name: String,
    visible: bool,
    locked: bool,
    kind: LayerKind,
    ty: String,
    color: [u8; 3],
    mask: String,
    outline: bool,
    onion: bool,
    opacity: f64,
    can_mask: bool,
    can_guide: bool,
}

fn snapshot(app: &App) -> Option<Snap> {
    let id = app.s.layer;
    let tl = app.s.timeline();
    let l = tl.layer(id)?;
    let i = tl.layers.iter().position(|x| x.id == id)?;
    let parent_kind = l.parent.and_then(|p| tl.layer(p)).map(|p| p.kind);
    let ty = match l.kind {
        LayerKind::Mask => "mask",
        LayerKind::Guide => "guide",
        LayerKind::Folder => "folder",
        LayerKind::Camera => "camera",
        LayerKind::Armature => "armature",
        LayerKind::Raster => "raster",
        LayerKind::Normal if parent_kind == Some(LayerKind::Mask) => "masked",
        LayerKind::Normal if parent_kind == Some(LayerKind::Guide) => "guided",
        LayerKind::Normal => "normal",
    };
    let c = crate::theme::to_color32(l.color);
    Some(Snap {
        id,
        name: l.name.clone(),
        visible: l.visible,
        locked: l.locked,
        kind: l.kind,
        ty: ty.to_string(),
        color: [c.r(), c.g(), c.b()],
        mask: l.mask_mode.key().to_string(),
        outline: l.outline,
        onion: l.onion,
        opacity: (f64::from(l.shown_opacity()) * 10_000.0).round() / 100.0,
        can_mask: tl.mask_above(i).is_some(),
        can_guide: tl.guide_above(i).is_some(),
    })
}

fn hex(c: [u8; 3]) -> String {
    Rgba::new(f32::from(c[0]) / 255.0, f32::from(c[1]) / 255.0, f32::from(c[2]) / 255.0, 1.0).to_hex()
}

/// The types a layer can be given here, with their names.
fn types() -> [(&'static str, &'static str); 5] {
    [("normal", tr!("Normal")), ("guide", tr!("Guide")), ("guided", tr!("Guided")), ("mask", tr!("Mask")), ("masked", tr!("Masked"))]
}

/// The mask types, with their names and what each does.
fn mask_types() -> [(&'static str, &'static str, &'static str); 3] {
    [
        ("coverage", tr!("Shape"), tr!("The mask's fills reveal fully, whatever their colour or opacity")),
        ("alpha", tr!("Alpha"), tr!("The mask's opacity reveals: gradients to transparent and faded shapes give soft edges")),
        ("luminance", tr!("Luminance"), tr!("The mask's brightness reveals: white shows, black hides, greys show partly")),
    ]
}

fn type_label(ty: &str) -> &'static str {
    match ty {
        "folder" => tr!("Folder"),
        "camera" => tr!("Camera"),
        "armature" => tr!("Armature"),
        "raster" => tr!("Raster"),
        other => types().into_iter().find(|(k, _)| *k == other).map_or(tr!("Normal"), |(_, n)| n),
    }
}

/// How far a drag travels for the whole range of a dial. Shift makes it finer.
fn drag_px(fine: bool) -> f64 {
    if fine { 280.0 } else { 90.0 }
}

/// A dial's new value after a vertical drag. Up increases. `step`, when set, lands on detents.
/// Otherwise the value keeps `places` digits after the point (none, for a whole number).
fn dragged_value(value: f64, delta_y: f32, start: f64, end: f64, step: Option<f64>, fine: bool, places: u32) -> f64 {
    let mut v = value - f64::from(delta_y) * (end - start) / drag_px(fine);
    v = v.clamp(start, end);
    if let Some(step) = step.filter(|s| *s > 0.0) {
        v = (v / step).round() * step;
        v = v.clamp(start, end);
    } else {
        let scale = 10f64.powi(places.min(6) as i32);
        v = (v * scale).round() / scale;
    }
    v
}

/// Angle of a dial pointer, in radians, y down: 0 is the low stop (lower left), 1 the high stop
/// (lower right), travelling over the top.
fn dial_angle(t: f32) -> f32 {
    let start = 140.0_f32.to_radians();
    let sweep = 260.0_f32.to_radians();
    start + sweep * t.clamp(0.0, 1.0)
}

pub(crate) fn cell_size(width: f32) -> f32 {
    let gap = 4.0;
    let cols = ((width + gap) / 68.0).floor().clamp(2.0, 5.0);
    ((width - gap * (cols - 1.0)) / cols).clamp(56.0, 84.0)
}

/// A toggle, colour, or type cell. Two of them plus the gap are one dial tall.
pub(crate) fn chip_size(dial: f32) -> f32 {
    let gap = 4.0;
    ((dial - gap) * 0.5).max(theme::TARGET)
}

/// How many small squares stack beside one dial. Two, when that stack is about the dial's height.
fn chip_rows(dial: f32, chip: f32, gap: f32) -> usize {
    let two = chip * 2.0 + gap;
    if two <= dial + 8.0 { 2 } else { 1 }
}

struct Packed {
    w: f32,
    h: f32,
    cells: Vec<(usize, f32, f32)>,
}

/// Top-left of each module. A run of small squares fills a grid two rows high beside the dials,
/// from the top left. The row is the dial's height. `dials[i]` is true for a dial.
fn pack(width: f32, dials: &[bool], dial: f32, chip: f32, gap: f32) -> (f32, Vec<(f32, f32)>) {
    let width = width.max(1.0);
    let rows = chip_rows(dial, chip, gap);
    let stride = (chip + gap).max(1.0);
    let max_cols = ((width + gap) / stride).floor().max(1.0) as usize;
    let mut blocks: Vec<Packed> = Vec::new();
    let mut i = 0;
    while i < dials.len() {
        if dials.get(i) == Some(&true) {
            blocks.push(Packed { w: dial, h: dial, cells: vec![(i, 0.0, 0.0)] });
            i += 1;
            continue;
        }
        let start = i;
        while i < dials.len() && dials.get(i) == Some(&false) {
            i += 1;
        }
        let mut k = start;
        while k < i {
            let n_left = i - k;
            let cols = max_cols.min((n_left + rows - 1) / rows).max(1);
            let count = n_left.min(cols * rows).max(1);
            let rows_used = (count + cols - 1) / cols;
            let w = cols as f32 * chip + (cols.saturating_sub(1) as f32) * gap;
            let h = rows_used as f32 * chip + (rows_used.saturating_sub(1) as f32) * gap;
            let mut cells = Vec::new();
            for n in 0..count {
                let col = n % cols;
                let row = n / cols;
                cells.push((k + n, col as f32 * stride, row as f32 * stride));
            }
            blocks.push(Packed { w, h, cells });
            k += count;
        }
    }
    let mut at = vec![(0.0, 0.0); dials.len()];
    let mut row_of: Vec<Vec<usize>> = vec![Vec::new()];
    let mut cursor = 0.0;
    for (bi, block) in blocks.iter().enumerate() {
        let starts = row_of.last().is_some_and(|r| r.is_empty());
        let add = if starts { block.w } else { gap + block.w };
        if !starts && cursor + add > width {
            row_of.push(Vec::new());
            cursor = 0.0;
        }
        let starts = row_of.last().is_some_and(|r| r.is_empty());
        cursor += if starts { block.w } else { gap + block.w };
        if let Some(row) = row_of.last_mut() {
            row.push(bi);
        }
    }
    let mut y = 0.0;
    for row in &row_of {
        if row.is_empty() {
            continue;
        }
        let h = row.iter().filter_map(|&bi| blocks.get(bi).map(|b| b.h)).fold(0.0_f32, f32::max);
        let mut x = 0.0;
        for &bi in row {
            let Some(block) = blocks.get(bi) else { continue };
            for &(idx, lx, ly) in &block.cells {
                if let Some(slot) = at.get_mut(idx) {
                    *slot = (x + lx, y + ly);
                }
            }
            x += block.w + gap;
        }
        y += h + gap;
    }
    let height = if dials.is_empty() { 0.0 } else { (y - gap).max(0.0) };
    (height, at)
}

pub(crate) fn place(ui: &mut egui::Ui, dials: &[bool], dial: f32, chip: f32, gap: f32) -> Vec<Rect> {
    let (height, at) = pack(ui.available_width(), dials, dial, chip, gap);
    let (_, area) = ui.allocate_space(vec2(ui.available_width(), height));
    at.iter().zip(dials).map(|(&(x, y), &is_dial)| {
        let side = if is_dial { dial } else { chip };
        Rect::from_min_size(pos2(area.left() + x, area.top() + y), vec2(side, side))
    }).collect()
}

pub(crate) fn hit(ui: &mut egui::Ui, scope: &str, salt: &str, rect: Rect, sense: Sense) -> egui::Response {
    ui.interact(rect, ui.id().with((scope, salt)), sense)
}

pub(crate) fn face(ui: &egui::Ui, rect: Rect, resp: &egui::Response, on: bool) {
    let p = ui.painter();
    let hot = resp.hovered() || resp.has_focus();
    p.rect_filled(rect, theme::T.rc(), if on { theme::T.accent_dim } else if hot { theme::T.header } else { theme::T.field });
    let (w, col) = if resp.has_focus() { (2.0, theme::T.accent) } else if on { (1.0, theme::T.accent) } else if hot { (1.0, theme::T.control_edge) } else { (1.0, theme::T.border) };
    p.rect_stroke(rect, theme::T.rc(), Stroke::new(w, col), StrokeKind::Inside);
}

fn label_at(ui: &egui::Ui, rect: Rect, label: &str, role: Type, on: bool, bottom: bool) {
    let ink = if on { theme::T.text } else { theme::T.text_dim };
    let inset = if rect.width() < 52.0 { 2.0 } else { 8.0 };
    let pad = if rect.height() < 44.0 { 2.0 } else { 5.0 };
    let mut job = egui::text::LayoutJob::simple_singleline(label.to_string(), font(role), ink);
    job.wrap = egui::text::TextWrapping { max_width: (rect.width() - inset).max(0.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let g = ui.painter().layout_job(job);
    let y = if bottom { rect.bottom() - g.rect.height() - pad } else { rect.center().y - g.rect.height() * 0.5 };
    ui.painter().galley(pos2(rect.center().x - g.rect.width() * 0.5, y), g, ink);
}

/// The icon or swatch, in the space above the caption.
pub(crate) fn mark_rect(rect: Rect) -> Rect {
    let reserve = 16.0_f32.min(rect.height() * 0.42);
    let room = (rect.height() - reserve).max(8.0);
    let side = (rect.width().min(room) * 0.62).clamp(10.0, 18.0);
    Rect::from_center_size(pos2(rect.center().x, rect.top() + room * 0.5), vec2(side, side))
}

pub(crate) fn caption(ui: &egui::Ui, rect: Rect, label: &str, on: bool) {
    label_at(ui, rect, label, Type::Caption, on, true);
}

/// A square toggle: icon or swatch, a short caption, on when `on`.
pub(crate) fn cube_toggle(ui: &mut egui::Ui, scope: &str, rect: Rect, on: bool, name: &str, caption_text: &str, tip: &str, paint_mark: impl FnOnce(&egui::Painter, Rect)) -> egui::Response {
    let resp = hit(ui, scope, name, rect, Sense::click());
    face(ui, rect, &resp, on);
    paint_mark(ui.painter(), mark_rect(rect));
    caption(ui, rect, caption_text, on);
    crate::a11y::control(ui, &resp, egui::WidgetType::Checkbox, name, Some(on));
    resp.on_hover_text(tip).named(name)
}

/// One setting that has several modes: the square shows the current mode, and a click lists the
/// others. A mode that the layer can't take stays in the list, unavailable.
pub(crate) struct Mode {
    pub key: &'static str,
    pub label: &'static str,
    pub allowed: bool,
    pub tip: &'static str,
}

/// The next allowed mode in `dir` (+1 or −1), skipping any the layer can't take.
fn next_mode(current: &str, modes: &[Mode], dir: i32) -> Option<&'static str> {
    if modes.iter().filter(|m| m.allowed).count() < 2 || modes.is_empty() {
        return None;
    }
    let start = modes.iter().position(|m| m.key == current).unwrap_or(0);
    let step = if dir < 0 { modes.len() - 1 } else { 1 };
    let mut i = start;
    for _ in 0..modes.len() {
        i = (i + step) % modes.len();
        if let Some(mode) = modes.get(i)
            && mode.allowed
            && mode.key != current
        {
            return Some(mode.key);
        }
    }
    None
}

/// A square for one setting. The current mode is the face; the caption names the setting.
/// Clicking opens the other modes. Arrow keys move among the ones this layer can take.
pub(crate) fn mode_cell(ui: &mut egui::Ui, scope: &str, rect: Rect, current: &str, value: &str, caption_text: &str, name: &str, tip: &str, modes: &[Mode]) -> Option<&'static str> {
    let resp = hit(ui, scope, name, rect, Sense::click());
    let popup_id = egui::Popup::default_response_id(&resp);
    let open = egui::Popup::is_id_open(ui.ctx(), popup_id);
    face(ui, rect, &resp, open);
    let reserve = 16.0_f32.min(rect.height() * 0.45);
    let band = Rect::from_min_max(rect.left_top(), pos2(rect.right(), (rect.bottom() - reserve).max(rect.top())));
    let role = if rect.height() < 52.0 { Type::Caption } else { Type::Body };
    label_at(ui, band, value, role, true, false);
    caption(ui, rect, caption_text, open);
    crate::a11y::control(ui, &resp, egui::WidgetType::ComboBox, name, None);
    let resp = resp.on_hover_text(tip).named(name);
    let mut picked = None;
    if !open && resp.has_focus() {
        let dir = ui.input_mut(|i| {
            let next = i.consume_key(egui::Modifiers::NONE, Key::ArrowDown) || i.consume_key(egui::Modifiers::NONE, Key::ArrowRight);
            let prev = i.consume_key(egui::Modifiers::NONE, Key::ArrowUp) || i.consume_key(egui::Modifiers::NONE, Key::ArrowLeft);
            if next { 1 } else if prev { -1 } else { 0 }
        });
        if dir != 0 {
            picked = next_mode(current, modes, dir);
        }
    }
    let shown = egui::Popup::menu(&resp).id(popup_id).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.set_min_width(156.0);
        for mode in modes {
            let text = if mode.key == current { format!("✔ {}", mode.label) } else { format!("   {}", mode.label) };
            let hit = ui.add_enabled(mode.allowed, egui::Button::new(text)).on_hover_text(mode.tip).named(mode.label);
            if mode.allowed && hit.clicked() && mode.key != current {
                ui.close();
                picked = Some(mode.key);
            }
        }
    });
    if let Some(menu) = shown {
        ui.ctx().accesskit_node_builder(menu.response.id, |node| {
            node.set_role(egui::accesskit::Role::Menu);
            node.set_label(name);
        });
    }
    picked
}

/// A round dial in a square. Drag up to raise it. `step`, when set, lands on detents.
/// `places` is how many digits the readout and a free drag keep after the point.
pub(crate) fn dial(ui: &mut egui::Ui, scope: &str, rect: Rect, value: f64, start: f64, end: f64, step: Option<f64>, reset: f64, places: u32, name: &str, caption_text: &str, tip: &str) -> Option<f64> {
    let resp = hit(ui, scope, name, rect, Sense::click_and_drag());
    face(ui, rect, &resp, false);
    let t = if end > start { ((value - start) / (end - start)) as f32 } else { 0.0 };
    let center = pos2(rect.center().x, rect.top() + rect.height() * 0.36);
    let radius = (rect.width() * 0.28).clamp(12.0, 26.0);
    let p = ui.painter();
    let mut track = Vec::new();
    let mut hot = Vec::new();
    for i in 0..=28 {
        let u = i as f32 / 28.0;
        let a = dial_angle(u);
        let at = pos2(center.x + a.cos() * radius, center.y + a.sin() * radius);
        track.push(at);
        if u <= t + f32::EPSILON {
            hot.push(at);
        }
    }
    if track.len() >= 2 {
        p.add(egui::Shape::line(track, Stroke::new(2.0, theme::T.border)));
    }
    if hot.len() >= 2 {
        p.add(egui::Shape::line(hot, Stroke::new(2.5, theme::T.accent)));
    }
    let a = dial_angle(t);
    let tip_at = pos2(center.x + a.cos() * radius, center.y + a.sin() * radius);
    let hub = pos2(center.x + a.cos() * (radius - 7.0).max(4.0), center.y + a.sin() * (radius - 7.0).max(4.0));
    p.line_segment([hub, tip_at], Stroke::new(2.0, theme::T.text));
    let readout = match places {
        0 => format!("{value:.0}"),
        1 => format!("{value:.1}"),
        _ => format!("{value:.2}"),
    };
    let g = p.layout_no_wrap(readout, font(Type::DataSmall), theme::T.text);
    p.galley(pos2(center.x - g.rect.width() * 0.5, center.y - g.rect.height() * 0.5), g, theme::T.text);
    caption(ui, rect, caption_text, false);
    crate::a11y::control(ui, &resp, egui::WidgetType::Slider, name, None);
    let resp = resp.on_hover_text(tip).named(name);
    let mut next = value;
    if resp.double_clicked() {
        next = reset;
    } else if resp.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        next = dragged_value(value, resp.drag_delta().y, start, end, step, fine, places);
    } else if resp.has_focus() {
        let (up, down) = ui.input_mut(|i| {
            let up = i.consume_key(egui::Modifiers::NONE, Key::ArrowUp) || i.consume_key(egui::Modifiers::NONE, Key::ArrowRight);
            let down = i.consume_key(egui::Modifiers::NONE, Key::ArrowDown) || i.consume_key(egui::Modifiers::NONE, Key::ArrowLeft);
            (up, down)
        });
        let bump = step.unwrap_or_else(|| 10f64.powi(-(places.min(6) as i32)));
        if up {
            next = dragged_value(value + bump, 0.0, start, end, step, false, places);
        } else if down {
            next = dragged_value(value - bump, 0.0, start, end, step, false, places);
        }
    }
    (next - value).abs().gt(&f64::EPSILON).then_some(next)
}

/// The Layer section of the Properties panel, for the active layer. Square modules in a
/// wrapping grid, with a dial for opacity. Show, lock, outlines, onion skin, colour,
/// type and mask type pack two high beside that dial (ADR 0124). A setting with several modes
/// is one cell. Layer height stays on the timeline's layer menu.
pub(crate) fn fields(app: &mut App, ui: &mut egui::Ui) {
    let Some(snap) = snapshot(app) else { return };
    let id = snap.id;
    let focus = app.layer_name_focus == Some(id);
    let fixed = matches!(snap.kind, LayerKind::Folder | LayerKind::Camera | LayerKind::Armature | LayerKind::Raster);
    let show_art = snap.kind != LayerKind::Camera && snap.kind != LayerKind::Folder;

    let mut name = snap.name.clone();
    ui.horizontal(|ui| {
        ui.label(text(Type::Label, tr!("Name")));
        let r = ui.add(egui::TextEdit::singleline(&mut name).char_limit(128).desired_width(ui.available_width())).named(tr!("Layer name"));
        if focus {
            r.scroll_to_me(Some(egui::Align::Center));
            theme::focus_selected(ui, &r, &name);
            app.layer_name_focus = None;
        }
        let next: String = name.chars().take(128).collect();
        let edited = r.changed() && next != snap.name;
        let blurred = r.lost_focus() && next.trim() != snap.name;
        let send = if edited { next } else if blurred { next.trim().to_string() } else { return };
        if !send.trim().is_empty() {
            crate::panels::live(app, tr_noop!("Layer name"), "layer.set_properties", json!({"layer": id.0, "name": send}));
        }
    });

    ui.add_space(4.0);
    let dial_side = cell_size(ui.available_width());
    let chip = chip_size(dial_side);
    let gap = 4.0;
    let mut dials = vec![false, false];
    if show_art {
        dials.push(false);
        dials.push(false);
    }
    if snap.kind != LayerKind::Camera {
        dials.push(true);
    }
    dials.push(false);
    dials.push(false);
    if snap.ty == "mask" {
        dials.push(false);
    }
    let rects = place(ui, &dials, dial_side, chip, gap);
    let mut n = 0usize;
    let mut take = || {
        let rect = rects.get(n).copied().unwrap_or(Rect::ZERO);
        n += 1;
        rect
    };
    let show_tip = tr!("Show this layer");
    if cube_toggle(ui, "layer", take(), snap.visible, tr!("Show"), tr!("Show"), &show_tip, |p, r| {
        crate::icons::paint(p, r, if snap.visible { Icon::Eye } else { Icon::EyeOff }, if snap.visible { theme::T.text } else { theme::T.text_faint });
    })
    .clicked()
    {
        app.run("layer.set_properties", json!({"layer": id.0, "visible": !snap.visible}));
    }
    let lock_tip = tr!("Lock this layer");
    if cube_toggle(ui, "layer", take(), snap.locked, tr!("Lock"), tr!("Lock"), &lock_tip, |p, r| {
        crate::icons::paint(p, r, if snap.locked { Icon::Lock } else { Icon::Unlock }, if snap.locked { theme::T.text } else { theme::T.text_faint });
    })
    .clicked()
    {
        app.run("layer.set_properties", json!({"layer": id.0, "locked": !snap.locked}));
    }
    if show_art {
        let outline_tip = tr!("Draw the layer's artwork as outlines in its colour, on the stage only (never in the movie or an export)");
        if cube_toggle(ui, "layer", take(), snap.outline, tr!("View layer as outlines"), tr!("Outlines"), &outline_tip, |p, r| {
            let c = theme::to_color32(Rgba::new(f32::from(snap.color[0]) / 255.0, f32::from(snap.color[1]) / 255.0, f32::from(snap.color[2]) / 255.0, 1.0));
            let sw = r.shrink(2.0);
            if snap.outline {
                p.rect_stroke(sw, theme::T.rc(), Stroke::new(2.0, c), StrokeKind::Inside);
            } else {
                p.rect_filled(sw, theme::T.rc(), c);
            }
        })
        .clicked()
        {
            app.run("layer.set_properties", json!({"layer": id.0, "outline": !snap.outline}));
        }
        let onion_tip = tr!("Show this layer's frames around the playhead while onion skin is on");
        if cube_toggle(ui, "layer", take(), snap.onion, tr!("Onion skin this layer"), tr!("Onion"), &onion_tip, |p, r| {
            crate::icons::paint(p, r, Icon::Onion, if snap.onion { theme::T.text } else { theme::T.text_faint });
            if !snap.onion {
                p.line_segment([r.left_bottom(), r.right_top()], Stroke::new(1.5, theme::T.text_faint));
            }
        })
        .clicked()
        {
            app.run("layer.set_properties", json!({"layer": id.0, "onion": !snap.onion}));
        }
    }
    if snap.kind != LayerKind::Camera {
        let tip = tr!("How strongly the layer shows, in the movie and every export. Its artwork fades together, so overlapping parts don't show through each other; a folder fades the layers in it. Drag up to raise it; Shift drags finer");
        if let Some(opacity) = dial(ui, "layer", take(), snap.opacity, 0.0, 100.0, None, 100.0, 0, tr!("Layer opacity"), tr!("Opacity"), &tip) {
            crate::panels::live(app, tr_noop!("Layer opacity"), "layer.set_properties", json!({"layer": id.0, "opacity": opacity}));
        }
    }
    let rect = take();
    let resp = hit(ui, "layer", "colour", rect, Sense::click());
    face(ui, rect, &resp, false);
    let sw = mark_rect(rect);
    let swatch = theme::to_color32(Rgba::new(f32::from(snap.color[0]) / 255.0, f32::from(snap.color[1]) / 255.0, f32::from(snap.color[2]) / 255.0, 1.0));
    ui.painter().rect_filled(sw, theme::T.rc(), swatch);
    ui.painter().rect_stroke(sw, theme::T.rc(), Stroke::new(1.0, theme::T.border), StrokeKind::Inside);
    caption(ui, rect, tr!("Colour"), false);
    let mut rgba = [snap.color[0], snap.color[1], snap.color[2], 255];
    let resp = resp.on_hover_text(tr!("The colour of the layer's outlines and of its bar on the timeline")).named(tr!("Outline colour"));
    if crate::colour::popup_for(ui, &resp, resp.id.with("colour_popup"), &mut rgba, false, None) {
        crate::panels::live(app, tr_noop!("Layer Properties"), "layer.set_properties", json!({"layer": id.0, "color": hex([rgba[0], rgba[1], rgba[2]])}));
    }
    if fixed {
        let rect = take();
        let resp = hit(ui, "layer", "fixed-type", rect, Sense::hover());
        face(ui, rect, &resp, true);
        label_at(ui, rect, type_label(&snap.ty), Type::Caption, true, false);
        let _ = resp.on_hover_text(type_label(&snap.ty)).named(type_label(&snap.ty));
    } else {
        let type_modes = types().map(|(k, label)| {
            let allowed = match k {
                "masked" => snap.can_mask || snap.ty == k,
                "guided" => snap.can_guide || snap.ty == k,
                _ => true,
            };
            let tip = if k == "masked" && !allowed {
                tr!("Only a layer right under a mask layer can be masked")
            } else if k == "guided" && !allowed {
                tr!("Only a layer right under a motion guide can be guided")
            } else {
                label
            };
            Mode { key: k, label, allowed, tip }
        });
        let type_tip = tr!("Choose this layer's type");
        if let Some(k) = mode_cell(ui, "layer", take(), &snap.ty, type_label(&snap.ty), tr!("Type"), tr!("Layer type"), &type_tip, &type_modes) {
            app.run("layer.set_properties", json!({"layer": id.0, "type": k}));
        }
    }
    if snap.ty == "mask" {
        let mask_modes = mask_types().map(|(k, label, what)| Mode { key: k, label, allowed: true, tip: what });
        let mask_value = mask_modes.iter().find(|m| m.key == snap.mask).map(|m| m.label).unwrap_or_else(|| tr!("Shape"));
        let mask_tip = tr!("Choose how this mask reveals");
        if let Some(k) = mode_cell(ui, "layer", take(), &snap.mask, mask_value, tr!("Mask type"), tr!("Mask type"), &mask_tip, &mask_modes) {
            app.run("layer.set_properties", json!({"layer": id.0, "mask": k}));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anicraft_engine::Session;

    fn screen() -> egui::RawInput {
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 940.0))), ..Default::default() }
    }

    fn paint(ctx: &egui::Context, app: &mut App) -> Vec<(egui::accesskit::NodeId, egui::accesskit::Node)> {
        let mut nodes = Vec::new();
        let mut out = ctx.run_ui(screen(), |ui| app.frame(ui));
        out.textures_delta.clear();
        if let Some(update) = out.platform_output.accesskit_update {
            nodes = update.nodes;
        }
        nodes
    }

    fn labels(nodes: &[(egui::accesskit::NodeId, egui::accesskit::Node)]) -> Vec<String> {
        nodes.iter().filter_map(|(_, n)| n.label().map(str::to_string)).collect()
    }

    #[test]
    fn the_layer_section_is_in_the_properties_panel_and_there_is_no_window() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut app = App::with_context(&ctx, Session::new(anicraft_engine::doc::Document::default()), Some(std::path::PathBuf::from("unused.png")));
        let id = app.s.layer;
        let original = app.s.timeline().layer(id).expect("a new document has a layer").name.clone();
        assert!(!original.is_empty());

        let mut nodes = Vec::new();
        for _ in 0..3 {
            nodes = paint(&ctx, &mut app);
        }
        let names = labels(&nodes);
        assert!(names.iter().any(|n| n == "Layer name"), "the Layer section is in the panel before any dialog is opened: {names:?}");
        assert!(!names.iter().any(|n| n == "Layer height"), "layer height stays on the timeline menu, not in this grid: {names:?}");
        assert!(names.iter().any(|n| n == "Layer opacity"), "opacity is a dial in the same grid");
        let ys: Vec<f64> = nodes.iter().filter_map(|(_, n)| {
            let label = n.label()?;
            matches!(label, "Layer opacity" | "View layer as outlines" | "Layer type").then(|| n.bounds().map(|b| b.y0)).flatten()
        }).collect();
        let (lo, hi) = ys.iter().copied().fold((f64::MAX, f64::MIN), |(lo, hi), y| (lo.min(y), hi.max(y)));
        let span = hi - lo;
        assert!(ys.len() >= 3 && span < 280.0, "the modules share one short band, not a tall form: span {span} from {ys:?}");
        let box_of = |label: &str| -> (f64, f64, f64, f64) {
            nodes.iter().find(|(_, n)| n.label() == Some(label)).and_then(|(_, n)| n.bounds()).map(|b| (b.x0, b.y0, b.x1, b.y1)).unwrap_or_else(|| panic!("no bounds for {label}"))
        };
        let show = box_of("Show");
        let outlines = box_of("View layer as outlines");
        let opacity = box_of("Layer opacity");
        let kind = box_of("Layer type");
        let width = |b: (f64, f64, f64, f64)| b.2 - b.0;
        let dial_w = width(opacity);
        assert!(dial_w >= 56.0 && dial_w <= 84.0, "a dial stays the large size, got {dial_w}");
        let expect = ((dial_w - 4.0) * 0.5).max(28.0);
        for (name, b) in [("Show", show), ("View layer as outlines", outlines), ("Layer type", kind)] {
            let w = width(b);
            assert!((w - expect).abs() < 1.5, "{name} fills half a dial: {w} vs {expect}");
        }
        assert!((outlines.0 - show.0).abs() < 1.5, "outlines sits under show, not beside it in a hole: {show:?} {outlines:?}");
        assert!(outlines.1 > show.3 - 1.0, "outlines is the second row of the small grid");
        let same_row = show.1 < opacity.3 && opacity.1 < show.3;
        if same_row {
            assert!((show.1 - opacity.1).abs() < 1.5, "the small grid starts at the dial's top: {show:?} {opacity:?}");
            assert!((outlines.3 - opacity.3).abs() < 2.0, "the small grid fills the dial's height: {outlines:?} {opacity:?}");
        }
        assert!(!names.iter().any(|n| n == "Masked" || n == "Guided"), "the other layer types stay inside the Type cell: {names:?}");
        assert!(ctx.memory(|m| m.area_rect(egui::Id::new("layer_props")).is_none()), "no Layer Properties window");
        assert!(app.layer_name_focus.is_none(), "selecting a layer does not steal the name field");

        app.invoke("layer.set_properties");
        assert!(app.panel_visible(crate::dock::Panel::Properties));
        assert_eq!(app.layer_name_focus, Some(id));

        for _ in 0..3 {
            paint(&ctx, &mut app);
        }
        assert!(ctx.memory(|m| m.area_rect(egui::Id::new("layer_props")).is_none()));
        assert!(app.layer_name_focus.is_none(), "the name is focused once");
        let focused = ctx.memory(|m| m.focused()).expect("the layer name field has focus");
        let state = egui::TextEdit::load_state(&ctx, focused).expect("the name field has editing state");
        let selection = state.cursor.char_range().expect("the layer name is selected");
        let a = selection.primary.index;
        let b = selection.secondary.index;
        assert_eq!(a.min(b), egui::text::CharIndex(0));
        assert_eq!(a.max(b), egui::text::CharIndex(original.chars().count()));

        // Opening on another layer makes that layer the one the panel shows.
        app.run("layer.new", json!({"name": "Sky"}));
        let sky = app.s.layer;
        app.run("layer.select", json!({"layer": id.0}));
        open(&mut app, Some(sky));
        assert_eq!(app.s.layer, sky);
        assert_eq!(app.layer_name_focus, Some(sky));
        assert!(app.s.selection.is_empty(), "opening the section does not select the layer's artwork");
    }

    #[test]
    fn editing_the_layer_in_the_panel_is_its_own_undo() {
        use egui::accesskit::{Action, ActionRequest, TreeId};
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut app = App::with_context(&ctx, Session::new(anicraft_engine::doc::Document::default()), Some(std::path::PathBuf::from("unused.png")));
        let id = app.s.layer;
        let mut nodes = Vec::new();
        for _ in 0..3 {
            nodes = paint(&ctx, &mut app);
        }
        let outline = nodes.iter().find(|(_, n)| n.label() == Some("View layer as outlines")).map(|(i, _)| *i).expect("the outlines checkbox is in the panel");
        let steps = app.s.history().undo_len();
        let mut input = screen();
        input.events.push(egui::Event::AccessKitActionRequest(ActionRequest { action: Action::Click, target_tree: TreeId::ROOT, target_node: outline, data: None }));
        let mut out = ctx.run_ui(input, |ui| app.frame(ui));
        out.textures_delta.clear();
        assert!(app.s.timeline().layer(id).expect("layer").outline);
        assert_eq!(app.s.history().undo_len(), steps + 1, "one undo step for the checkbox");

        app.invoke("layer.set_properties");
        for _ in 0..3 {
            paint(&ctx, &mut app);
        }
        let steps = app.s.history().undo_len();
        let mut input = screen();
        input.events.push(egui::Event::Text("Hero".into()));
        let mut out = ctx.run_ui(input, |ui| app.frame(ui));
        out.textures_delta.clear();
        assert_eq!(app.s.timeline().layer(id).expect("layer").name, "Hero");
        app.s.end_gesture();
        assert_eq!(app.s.history().undo_len(), steps + 1, "typing the name is one undo step");
    }

    fn node_named(nodes: &[(egui::accesskit::NodeId, egui::accesskit::Node)], label: &str) -> egui::accesskit::NodeId {
        nodes
            .iter()
            .find(|(_, n)| {
                n.label().is_some_and(|l| {
                    let t = l.trim().trim_start_matches('✔').trim();
                    t == label
                })
            })
            .map(|(i, _)| *i)
            .unwrap_or_else(|| panic!("no control named {label}"))
    }

    fn click_named(ctx: &egui::Context, app: &mut App, nodes: &[(egui::accesskit::NodeId, egui::accesskit::Node)], label: &str) -> Vec<(egui::accesskit::NodeId, egui::accesskit::Node)> {
        use egui::accesskit::{Action, ActionRequest, TreeId};
        let mut input = screen();
        input.events.push(egui::Event::AccessKitActionRequest(ActionRequest { action: Action::Click, target_tree: TreeId::ROOT, target_node: node_named(nodes, label), data: None }));
        let mut out = ctx.run_ui(input, |ui| app.frame(ui));
        out.textures_delta.clear();
        out.platform_output.accesskit_update.map(|u| u.nodes).unwrap_or_default()
    }

    #[test]
    fn the_type_and_mask_modes_share_one_cell() {
        let modes = [
            Mode { key: "normal", label: "Normal", allowed: true, tip: "" },
            Mode { key: "guided", label: "Guided", allowed: false, tip: "" },
            Mode { key: "mask", label: "Mask", allowed: true, tip: "" },
        ];
        assert_eq!(next_mode("normal", &modes, 1), Some("mask"));
        assert_eq!(next_mode("mask", &modes, 1), Some("normal"));
        assert_eq!(next_mode("normal", &modes, -1), Some("mask"));
        assert_eq!(next_mode("guided", &modes, 1), Some("mask"), "a mode the layer can't take is skipped");

        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut app = App::with_context(&ctx, Session::new(anicraft_engine::doc::Document::default()), Some(std::path::PathBuf::from("unused.png")));
        let id = app.s.layer;
        let mut nodes = Vec::new();
        for _ in 0..3 {
            nodes = paint(&ctx, &mut app);
        }
        nodes = click_named(&ctx, &mut app, &nodes, "Layer type");
        if !labels(&nodes).iter().any(|n| n == "Masked") {
            nodes = paint(&ctx, &mut app);
        }
        assert!(labels(&nodes).iter().any(|n| n == "Masked"), "the Type cell lists every mode, including ones this layer can't take");
        let steps = app.s.history().undo_len();
        nodes = click_named(&ctx, &mut app, &nodes, "Masked");
        assert_eq!(app.s.timeline().layer(id).expect("layer").kind, anicraft_engine::doc::LayerKind::Normal, "Masked is refused with no mask above");
        assert_eq!(app.s.history().undo_len(), steps, "a refused mode is not a command");
        if !labels(&nodes).iter().any(|n| n.trim().trim_start_matches('✔').trim() == "Mask") {
            nodes = paint(&ctx, &mut app);
            if !labels(&nodes).iter().any(|n| n == "Mask") {
                nodes = click_named(&ctx, &mut app, &nodes, "Layer type");
            }
        }
        click_named(&ctx, &mut app, &nodes, "Mask");
        assert_eq!(app.s.timeline().layer(id).expect("layer").kind, anicraft_engine::doc::LayerKind::Mask);
        for _ in 0..2 {
            nodes = paint(&ctx, &mut app);
        }
        nodes = click_named(&ctx, &mut app, &nodes, "Mask type");
        if !labels(&nodes).iter().any(|n| n == "Luminance") {
            nodes = paint(&ctx, &mut app);
        }
        click_named(&ctx, &mut app, &nodes, "Luminance");
        assert_eq!(app.s.timeline().layer(id).expect("layer").mask_mode, anicraft_engine::doc::MaskMode::Luminance);
    }

    #[test]
    fn a_dial_drag_up_raises_the_value_and_height_lands_on_a_detent() {
        assert_eq!(dragged_value(40.0, -30.0, 0.0, 100.0, None, false, 0), 73.0);
        assert_eq!(dragged_value(40.0, 30.0, 0.0, 100.0, None, false, 0), 7.0);
        assert_eq!(dragged_value(100.0, -20.0, 100.0, 300.0, Some(100.0), false, 0), 100.0);
        assert_eq!(dragged_value(100.0, -50.0, 100.0, 300.0, Some(100.0), false, 0), 200.0);
        assert_eq!(dragged_value(300.0, -40.0, 100.0, 300.0, Some(100.0), false, 0), 300.0);
        assert_eq!(dragged_value(1.25, -4.5, 0.0, 10.0, None, false, 2), 1.75);
        let low = dial_angle(0.0);
        let high = dial_angle(1.0);
        assert!(low.sin() > 0.0 && high.sin() > 0.0, "both stops sit below the centre");
        assert!(low.cos() < 0.0 && high.cos() > 0.0, "the low stop is left and the high stop is right");
    }

    #[test]
    fn chips_pack_into_the_dial_square() {
        assert_eq!(chip_size(80.0), 38.0);
        assert_eq!(chip_size(56.0), 28.0);
        assert_eq!(chip_size(40.0), 28.0, "a chip stays a 28 px target");
        let dial = cell_size(400.0);
        assert!((56.0..=84.0).contains(&dial), "a dial stays the large size");
        assert_eq!(chip_rows(80.0, 38.0, 4.0), 2);
        // Show, lock, outlines, onion, opacity, colour, type.
        let kinds = [false, false, false, false, true, false, false];
        let (height, at) = pack(400.0, &kinds, 80.0, 38.0, 4.0);
        assert_eq!(at[0], (0.0, 0.0));
        assert_eq!(at[1], (42.0, 0.0));
        assert_eq!(at[2], (0.0, 42.0), "outlines is under show");
        assert_eq!(at[3], (42.0, 42.0));
        assert_eq!(at[4], (84.0, 0.0), "the dial starts where the small grid ends");
        assert_eq!(at[5], (168.0, 0.0), "colour sits beside the dial");
        assert_eq!(at[6], (168.0, 42.0), "type stacks under colour");
        assert_eq!(height, 80.0, "the row is one dial tall, with no empty band");
        let (narrow, wrapped) = pack(100.0, &kinds, 80.0, 38.0, 4.0);
        assert_eq!(wrapped[4], (0.0, 84.0), "a dial that does not fit wraps under the small grid");
        assert!(narrow > 80.0);
    }
}
