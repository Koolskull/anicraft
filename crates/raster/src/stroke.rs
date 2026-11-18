//! Spacing along a pointer move. Pressure is interpolated between the samples.

use crate::cel::Cel;
use crate::dab::{Antialias, Tip};

/// One pointer sample. Pressure is 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub x: f32,
    pub y: f32,
    pub pressure: f32,
}

/// A round stroke. `size` is the diameter at full pressure.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stroke {
    pub size: f32,
    pub size_min: f32,
    pub softness: f32,
    pub antialias: Antialias,
    pub color: [u8; 4],
    pub erase: bool,
    /// When set, a dab's coverage is scaled from [`Self::opacity_min`] at no pressure to
    /// [`Self::opacity_max`] at full pressure. Otherwise the dab is fully covered and the
    /// fill's alpha is the only opacity.
    pub opacity_from_pressure: bool,
    /// Coverage at no pressure, 0 to 1, used when [`Self::opacity_from_pressure`] is set.
    pub opacity_min: f32,
    /// Coverage at full pressure, 0 to 1, used when [`Self::opacity_from_pressure`] is set.
    pub opacity_max: f32,
    /// 1 keeps the strongest dab. Below 1, each dab deposits that share and overlaps build up.
    /// The colour alpha is still applied once for a plate stroke.
    pub flow: f32,
}

/// Gap for the pen, in pixels. A wide brush steps in brush-widths.
pub fn pen_spacing(diameter: f32) -> f32 {
    let width = if diameter.is_finite() { diameter.max(1.0) } else { 1.0 };
    (width * 0.15).max(0.75)
}

/// Gap for a stage-plate dab. Tighter than [`pen_spacing`], which the cropped cel still uses.
/// A hairline steps half a pixel. A wide hard disc steps about 7% of its diameter.
pub fn dab_spacing(diameter: f32) -> f32 {
    let width = if diameter.is_finite() { diameter.max(0.0) } else { 1.0 };
    if width < 1.0 { 0.5 } else { (width * 0.07).max(0.35) }
}

/// Points strictly between the ends, then the end when the distance covers a whole step.
///
/// A move longer than [`MAX_STEPS`] widens the gap so the stamp count stays bounded.
pub fn stroke_steps(x0: f32, y0: f32, p0: f32, x1: f32, y1: f32, p1: f32, spacing: f32) -> Vec<Sample> {
    let values = [x0 as f64, y0 as f64, p0 as f64, x1 as f64, y1 as f64, p1 as f64, spacing as f64];
    if values.iter().any(|value| !value.is_finite()) {
        return Vec::new();
    }
    let (x0, y0, p0, x1, y1, p1) = (values[0], values[1], values[2], values[3], values[4], values[5]);
    let distance = (x1 - x0).hypot(y1 - y0);
    let mut step = values[6].max(0.5);
    if distance < step {
        return Vec::new();
    }
    let mut count = (distance / step) as i32;
    if count > MAX_STEPS {
        step = distance / f64::from(MAX_STEPS);
        count = MAX_STEPS;
    }
    if count <= 0 {
        return Vec::new();
    }
    let mut points = Vec::with_capacity(count as usize);
    for index in 1..=count {
        let mix = ((f64::from(index) * step) / distance).min(1.0);
        points.push(Sample { x: (x0 + (x1 - x0) * mix) as f32, y: (y0 + (y1 - y0) * mix) as f32, pressure: (p0 + (p1 - p0) * mix) as f32 });
    }
    points
}

/// The first sample, then evenly spaced dabs. `spacing_at` sees the average pressure of that segment.
pub fn sample_stroke(points: &[Sample], mut spacing_at: impl FnMut(f32) -> f32) -> Vec<Sample> {
    let mut out = Vec::new();
    let Some(first) = points.iter().copied().find(Sample::is_finite) else {
        return out;
    };
    out.push(first);
    for next in points.iter().copied().filter(Sample::is_finite).skip(1) {
        let Some(last) = out.last().copied() else {
            break;
        };
        let gap = spacing_at((last.pressure + next.pressure) * 0.5);
        out.extend(stroke_steps(last.x, last.y, last.pressure, next.x, next.y, next.pressure, gap));
        let Some(end) = out.last() else {
            break;
        };
        if (end.x - next.x).abs() > 0.01 || (end.y - next.y).abs() > 0.01 {
            out.push(next);
        }
    }
    out
}

/// Stamp `points` into `cel`. Returns whether any dab landed.
pub fn stamp_stroke(cel: &mut Cel, points: &[Sample], stroke: Stroke) -> bool {
    let sampled = sample_stroke(points, |mid| pen_spacing(diameter_at(stroke, mid)));
    let mut hit = false;
    for sample in sampled {
        let opacity = dab_opacity(stroke, sample.pressure);
        let tip = Tip {
            radius: diameter_at(stroke, sample.pressure) * 0.5,
            softness: stroke.softness,
            antialias: stroke.antialias,
            color: stroke.color,
            opacity,
            erase: stroke.erase,
        };
        hit |= cel.stamp(sample.x, sample.y, tip);
    }
    hit
}

const MAX_STEPS: i32 = 16_384;

/// How much of the dab's shape counts. The fill's alpha is applied once, later, for a plate stroke.
pub fn dab_opacity(stroke: Stroke, pressure: f32) -> f32 {
    if !stroke.opacity_from_pressure {
        return 1.0;
    }
    let pressure = if pressure.is_finite() { pressure.clamp(0.0, 1.0) } else { 0.0 };
    let mut low = if stroke.opacity_min.is_finite() { stroke.opacity_min.clamp(0.0, 1.0) } else { 0.0 };
    let mut high = if stroke.opacity_max.is_finite() { stroke.opacity_max.clamp(0.0, 1.0) } else { 1.0 };
    if low > high {
        std::mem::swap(&mut low, &mut high);
    }
    low + (high - low) * pressure
}

fn diameter_at(stroke: Stroke, pressure: f32) -> f32 {
    let pressure = if pressure.is_finite() { pressure.clamp(0.0, 1.0) } else { 0.0 };
    let size = if stroke.size.is_finite() { stroke.size.max(0.0) } else { 0.0 };
    let low = if stroke.size_min.is_finite() { stroke.size_min.clamp(0.0, size) } else { 0.0 };
    low + (size - low) * pressure
}

impl Sample {
    fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.pressure.is_finite()
    }
}
