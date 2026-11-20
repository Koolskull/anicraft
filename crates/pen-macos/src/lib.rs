//! Pen pressure on macOS (ADR 0096).
//!
//! winit 0.30 delivers a mouse event for a stylus and drops the tablet point, and egui never
//! sees trackpad force either. This crate reads that one tablet point and hands back
//! `0.0..=1.0`. The shell stays free of `unsafe`. A mouse is not a pen: turning Speed as
//! pressure off leaves a real pen alone and paints a mouse at full width.
//!
//! Hover is not a sample. A tablet point with the tip up and no pressure is ignored, so a pen
//! resting nearby does not thin a mouse stroke. The stroke is drawn on a later frame, so while
//! the tip is down the last pressure is kept. A Huion pen's force is on its HID report: the
//! driver hands the app a mouse event whose pressure is 0 or 1.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::PoisonError;

/// How long a tablet point still belongs to the mouse event the system sends with it.
pub const PEN_WINDOW_SECS: f64 = 0.03;

/// `NSEventTypeTabletPoint`.
const TABLET_POINT: u64 = 23;
/// `NSEventSubtypeTabletPoint` on a mouse event. The same number means something else on other events.
const SUBTYPE_TABLET_POINT: i16 = 1;

static HAVE: AtomicBool = AtomicBool::new(false);
static BITS: AtomicU32 = AtomicU32::new(0);
static AT: AtomicU64 = AtomicU64::new(0);
/// The tip is down. The frame is drawn after the mouse event, so the pressure has to outlive
/// the 30 ms window until the button comes up.
static TIP_DOWN: AtomicBool = AtomicBool::new(false);

/// How long a tip-down reading is kept while the frame catches up.
const HOLD_SECS: f64 = 2.0;

fn mono() -> f64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
}

/// Remember a tip-down tablet point. `at` is seconds on the same clock as the read.
fn remember(pressure: f32, at: f64) {
    if !(pressure.is_finite() && at.is_finite()) {
        return;
    }
    BITS.store(pressure.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    AT.store(at.to_bits(), Ordering::Relaxed);
    HAVE.store(true, Ordering::Relaxed);
}

/// The tablet point remembered just now, for the frame that draws it.
fn live() -> Option<f32> {
    pressure_as_of(mono())
}

fn tip_down() -> bool {
    TIP_DOWN.load(Ordering::Relaxed)
}

/// The stored reading, when there is one: pressure and the time it was taken.
fn stored() -> Option<(f32, f64)> {
    if !HAVE.load(Ordering::Relaxed) {
        return None;
    }
    let p = f32::from_bits(BITS.load(Ordering::Relaxed));
    let at = f64::from_bits(AT.load(Ordering::Relaxed));
    if p.is_finite() && at.is_finite() { Some((p, at)) } else { None }
}

fn set_tip_down(down: bool) {
    TIP_DOWN.store(down, Ordering::Relaxed);
}

/// The tablet point at `now`. Fresh for [`PEN_WINDOW_SECS`], and for [`HOLD_SECS`] while the
/// tip is still down, because the stroke is drawn on a later frame.
pub fn pressure_as_of(now: f64) -> Option<f32> {
    if !HAVE.load(Ordering::Relaxed) {
        return None;
    }
    let at = f64::from_bits(AT.load(Ordering::Relaxed));
    if !(at.is_finite() && now.is_finite()) || now + 1e-3 < at {
        return None;
    }
    let age = now - at;
    let keep = age <= PEN_WINDOW_SECS || (tip_down() && age <= HOLD_SECS);
    if !keep {
        return None;
    }
    let p = f32::from_bits(BITS.load(Ordering::Relaxed));
    if p.is_finite() { Some(p.clamp(0.0, 1.0)) } else { None }
}

fn is_up(kind: u64) -> bool {
    // Left, right and other mouse up.
    matches!(kind, 2 | 4 | 26)
}

fn is_drag(kind: u64) -> bool {
    matches!(kind, 6 | 7 | 27)
}

/// The candidate that is actually a force.
///
/// A mouse button is 0 or 1, and a missing tablet field is 0. A stylus in the middle of its
/// range is the one to keep. When every candidate sits on an end, 1 wins over 0, so a mouse
/// stays a mouse.
pub fn prefer_pressure(candidates: &[f32]) -> f32 {
    let mut end = 1.0;
    let mut saw = false;
    for &raw in candidates {
        if !raw.is_finite() {
            continue;
        }
        let p = raw.clamp(0.0, 1.0);
        if p > 0.02 && p < 0.98 {
            return p;
        }
        if !saw || (p - 1.0).abs() <= 0.02 {
            end = p;
            saw = true;
        }
    }
    end
}

/// Whether `pressure` may replace the reading stored at `prev` (`(pressure, time)`).
///
/// `authoritative` is an `NSEventTypeTabletPoint`. A mouse event that says 0 or 1 in the same
/// instant is the copy the system sends beside the pen, and it must not erase a real force.
pub fn accept_sample(authoritative: bool, pressure: f32, prev: Option<(f32, f64)>, now: f64) -> bool {
    if authoritative {
        return true;
    }
    let Some((prev_p, prev_at)) = prev else {
        return true;
    };
    if !(prev_at.is_finite() && now.is_finite() && pressure.is_finite()) {
        return false;
    }
    let fresh = now >= prev_at && now - prev_at <= PEN_WINDOW_SECS;
    let incoming_end = pressure <= 0.02 || pressure >= 0.98;
    let prev_mid = prev_p > 0.02 && prev_p < 0.98;
    !(fresh && incoming_end && prev_mid)
}

/// What one event contributes. `(tip still down, pressure to remember)`.
///
/// A mouse button is 0 or 1 and is not a pen. A pressure between those ends is, even when the
/// driver leaves off the tablet subtype. Once the tip is down, 0 or 1 on the drag is a light
/// or a hard press and is kept.
pub fn reported(down: bool, kind: u64, subtype: i16, tip: bool, pressure: f32) -> (bool, Option<f32>) {
    if is_up(kind) {
        return (false, classify(kind, subtype, tip, pressure));
    }
    if let Some(p) = classify(kind, subtype, tip, pressure) {
        return (true, Some(p));
    }
    if is_mouse(kind) && pressure.is_finite() {
        let p = pressure.clamp(0.0, 1.0);
        let mid = p > 0.02 && p < 0.98;
        if mid || (down && is_drag(kind)) {
            return (true, Some(p));
        }
    }
    (down, None)
}

/// Pressure from one AppKit event, or nothing when it is a mouse, a hover, or not finite.
///
/// `kind` is `NSEventType`, `subtype` is `NSEventSubtype`, `tip_down` is the pen-tip button
/// (tablet points only; pass false for a mouse event).
pub fn classify(kind: u64, subtype: i16, tip_down: bool, pressure: f32) -> Option<f32> {
    if !pressure.is_finite() {
        return None;
    }
    let p = pressure.clamp(0.0, 1.0);
    if kind == TABLET_POINT {
        // Hover reports pressure 0 with the tip up. A light touch is either the tip button or
        // a pressure above none, including a driver that leaves the button mask empty.
        if tip_down || p > 0.0 { Some(p) } else { None }
    } else if is_mouse(kind) && subtype == SUBTYPE_TABLET_POINT {
        Some(p)
    } else {
        None
    }
}

fn is_mouse(kind: u64) -> bool {
    // Left/right/other mouse down, up, and dragged. Not moved, not entered.
    matches!(kind, 1 | 2 | 3 | 4 | 6 | 7 | 25 | 26 | 27)
}

mod hid;

/// One drag point, in window points with the origin at the top left.
/// `pressure` is the pen's force when this event carried one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrokePoint {
    pub x: f64,
    pub y: f64,
    pub pressure: Option<f32>,
}

/// How many drag points to keep. Older ones fall off so a stuck monitor cannot grow.
const POINT_CAP: usize = 2048;

static POINTS: Mutex<VecDeque<StrokePoint>> = Mutex::new(VecDeque::new());

/// Remember one drag point. Not finite values are ignored. The raster tool drains these.
pub fn push_point(x: f64, y: f64, pressure: Option<f32>) {
    if !x.is_finite() || !y.is_finite() {
        return;
    }
    let pressure = pressure.filter(|p| p.is_finite()).map(|p| p.clamp(0.0, 1.0));
    let mut q = POINTS.lock().unwrap_or_else(PoisonError::into_inner);
    if q.len() >= POINT_CAP {
        q.pop_front();
    }
    q.push_back(StrokePoint { x, y, pressure });
}

/// Take every point recorded since the last drain, oldest first.
pub fn drain_points() -> Vec<StrokePoint> {
    let mut q = POINTS.lock().unwrap_or_else(PoisonError::into_inner);
    q.drain(..).collect()
}

/// A content-view point with its origin at the top left (ADR 0120). `local_x` and `local_y`
/// are already in the view's own coordinates, measured from the view's origin. An unflipped
/// view grows upward, so its y is turned over. `None` when the point is outside the view.
pub fn content_point(local_x: f64, local_y: f64, width: f64, height: f64, flipped: bool) -> Option<(f64, f64)> {
    if !(local_x.is_finite() && local_y.is_finite() && width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
        return None;
    }
    let y = if flipped { local_y } else { height - local_y };
    if local_x >= 0.0 && y >= 0.0 && local_x <= width && y <= height { Some((local_x, y)) } else { None }
}

/// Start listening. Safe to call more than once. No-op off the main thread and off macOS.
pub fn install() {
    #[cfg(target_os = "macos")]
    {
        imp::install();
        hid::install();
    }
}

/// The pen's pressure for the event being handled, when it is a stylus.
pub fn current() -> Option<f32> {
    #[cfg(target_os = "macos")]
    {
        imp::current()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// The cursor, in the content view that contains it (ADR 0120). View points, origin top left.
/// `None` off macOS, off the main thread, or when the cursor is inside no window. A file drag
/// does not deliver pointer moves, so the shell asks here instead of waiting on winit.
pub fn cursor_in_content() -> Option<(f64, f64)> {
    #[cfg(target_os = "macos")]
    {
        imp::cursor_in_content()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{accept_sample, content_point, live, prefer_pressure, remember};
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSEvent, NSEventButtonMask, NSEventMask, NSEventType};
    use objc2_core_graphics::{CGEvent, CGEventField};
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicBool, Ordering};

    pub fn install() {
        static STARTED: AtomicBool = AtomicBool::new(false);
        if STARTED.swap(true, Ordering::Relaxed) {
            return;
        }
        if MainThreadMarker::new().is_none() {
            STARTED.store(false, Ordering::Relaxed);
            return;
        }
        let block = block2::RcBlock::new(|event: NonNull<NSEvent>| -> *mut NSEvent {
            // A throw from a tablet field must not leave this callback. Rust cannot unwind through it.
            let _ = objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
                // SAFETY: AppKit calls the monitor with the event it is dispatching, alive for this call.
                let ev = unsafe { event.as_ref() };
                note(ev);
            }));
            event.as_ptr()
        });
        let mask = NSEventMask::LeftMouseDown
            | NSEventMask::LeftMouseUp
            | NSEventMask::LeftMouseDragged
            | NSEventMask::RightMouseDown
            | NSEventMask::RightMouseUp
            | NSEventMask::RightMouseDragged
            | NSEventMask::OtherMouseDown
            | NSEventMask::OtherMouseUp
            | NSEventMask::OtherMouseDragged
            | NSEventMask::TabletPoint;
        // SAFETY: the block returns the same event pointer it was given, so delivery is unchanged.
        // AppKit removes the monitor if the token is dropped, so both live for the process.
        let token = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &block) };
        std::mem::forget(block);
        std::mem::forget(token);
    }

    /// Frontmost window whose content view contains the cursor. A drag from Finder does not
    /// make the window key, and it does not send pointer moves, so this is read on purpose.
    pub fn cursor_in_content() -> Option<(f64, f64)> {
        let mtm = MainThreadMarker::new()?;
        let caught = objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
            let app = NSApplication::sharedApplication(mtm);
            // `windows` is front to back. The first view that contains the cursor wins.
            for window in app.windows().to_vec() {
                let Some(view) = window.contentView() else { continue };
                let base = window.mouseLocationOutsideOfEventStream();
                let local = view.convertPoint_fromView(base, None);
                let bounds = view.bounds();
                if let Some(p) = content_point(local.x - bounds.origin.x, local.y - bounds.origin.y, bounds.size.width, bounds.size.height, view.isFlipped()) {
                    return Some(p);
                }
            }
            None
        }));
        caught.ok().flatten()
    }

    pub fn current() -> Option<f32> {
        install();
        super::hid::install();
        // The Huion pen's force is not on the mouse event. Its HID report wins while the tip is down.
        if let Some(p) = super::hid::fresh(super::mono()) {
            return Some(p);
        }
        // The monitor already stored the drag. Reading `currentEvent` again during a redraw
        // sees a mouse copy at 0 or 1 and was wiping that reading (ADR 0102).
        live()
    }

    fn note(event: &NSEvent) {
        let _ = objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
            let ty = event.r#type();
            let kind = ty.0 as u64;
            // A key or a scroll has no pressure field. Reading it aborts the process.
            if ty != NSEventType::TabletPoint && !super::is_mouse(kind) {
                return;
            }
            // `subtype` is for mouse events. On a tablet point the same call can raise.
            let mouse = super::is_mouse(kind);
            let mut subtype = if mouse { event.subtype().0 } else { 0 };
            let tip = ty == NSEventType::TabletPoint && event.buttonMask().contains(NSEventButtonMask::PenTip);
            let ns_pressure = event.pressure();
            let field = tablet_field(event);
            if super::hid::tracing_events() && matches!(kind, 1 | 6 | 23) {
                let cg = field.map(|(p, sub)| format!("{p:.3} tablet={sub}")).unwrap_or_else(|| "-".to_string());
                super::hid::trace_event(&format!("ns kind={kind} sub={subtype} ns={ns_pressure:.3} cg={cg}"));
            }
            let mut pressure = ns_pressure;
            if let Some((p, sub)) = field {
                pressure = prefer_pressure(&[pressure, p]);
                if sub {
                    subtype = super::SUBTYPE_TABLET_POINT;
                }
            }
            let (down, sample) = super::reported(super::tip_down(), kind, subtype, tip, pressure);
            super::set_tip_down(down);
            if let Some(p) = sample {
                let now = super::mono();
                let prev = super::stored();
                // Only the tablet-point event itself may replace a real force with 0 or 1.
                // The mouse copy sent beside it is often full pressure.
                if accept_sample(ty == NSEventType::TabletPoint, p, prev, now) {
                    remember(p, now);
                }
            }
            // Left drag and tablet points only. A right-click or a scroll is not a stroke.
            if matches!(kind, 1 | 6 | 23) {
                record_points(event, sample);
            }
        }));
    }

    /// Window points for this event and any AppKit coalesced into it (ADR 0119).
    fn record_points(event: &NSEvent, pressure: Option<f32>) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let Some(window) = event.window(mtm) else { return };
        let height = window.frame().size.height;
        if !(height.is_finite() && height > 0.0) {
            return;
        }
        let mut wrote = false;
        for ev in coalesced(event) {
            let loc = ev.locationInWindow();
            if !loc.x.is_finite() || !loc.y.is_finite() {
                continue;
            }
            let own = ev.pressure();
            let p = if own.is_finite() && own > 0.02 && own < 0.98 { Some(own.clamp(0.0, 1.0)) } else { pressure.filter(|n| n.is_finite()) };
            super::push_point(loc.x, height - loc.y, p);
            wrote = true;
        }
        if !wrote {
            let loc = event.locationInWindow();
            if loc.x.is_finite() && loc.y.is_finite() {
                super::push_point(loc.x, height - loc.y, pressure.filter(|n| n.is_finite()));
            }
        }
    }

    /// The event itself, or each event AppKit folded into it. A missing selector is just this one.
    fn coalesced(event: &NSEvent) -> Vec<&NSEvent> {
        let sel = objc2::sel!(coalescedEvents);
        let answers: objc2::runtime::Bool = unsafe { objc2::msg_send![event, respondsToSelector: sel] };
        if !answers.as_bool() {
            return vec![event];
        }
        let arr: *mut objc2::runtime::AnyObject = unsafe { objc2::msg_send![event, coalescedEvents] };
        if arr.is_null() {
            return vec![event];
        }
        let count: usize = unsafe { objc2::msg_send![arr, count] };
        if count == 0 {
            return vec![event];
        }
        let mut out = Vec::with_capacity(count.min(64));
        for i in 0..count.min(64) {
            let item: *mut objc2::runtime::AnyObject = unsafe { objc2::msg_send![arr, objectAtIndex: i] };
            if item.is_null() {
                continue;
            }
            // The array keeps the event for this call.
            out.push(unsafe { &*item.cast::<NSEvent>() });
        }
        if out.is_empty() { vec![event] } else { out }
    }

    /// CoreGraphics tablet pressure, when the mouse event's own pressure is 0 or 1.
    /// A throw here leaves the AppKit reading in place.
    fn tablet_field(event: &NSEvent) -> Option<(f32, bool)> {
        let read = objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
            let cg = event.CGEvent()?;
            let tablet = CGEvent::double_value_field(Some(&cg), CGEventField::TabletEventPointPressure);
            let mouse = CGEvent::double_value_field(Some(&cg), CGEventField::MouseEventPressure);
            let sub = CGEvent::integer_value_field(Some(&cg), CGEventField::MouseEventSubtype);
            let p = prefer_pressure(&[tablet as f32, mouse as f32]);
            Some((p, sub == i64::from(super::SUBTYPE_TABLET_POINT)))
        }));
        read.ok().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_content_point_is_top_left_and_stays_inside_the_view() {
        assert_eq!(content_point(10.0, 4.0, 100.0, 50.0, true), Some((10.0, 4.0)));
        // Unflipped: y grows up, so 4 from the bottom is 46 from the top.
        assert_eq!(content_point(10.0, 4.0, 100.0, 50.0, false), Some((10.0, 46.0)));
        assert_eq!(content_point(0.0, 50.0, 100.0, 50.0, false), Some((0.0, 0.0)), "the top of an unflipped view");
        assert_eq!(content_point(0.0, 0.0, 100.0, 50.0, false), Some((0.0, 50.0)), "the bottom edge still belongs");
        assert_eq!(content_point(100.0, 0.0, 100.0, 50.0, true), Some((100.0, 0.0)), "the far edge still belongs");
        assert!(content_point(-1.0, 4.0, 100.0, 50.0, true).is_none());
        assert!(content_point(10.0, 60.0, 100.0, 50.0, true).is_none());
        assert!(content_point(f64::NAN, 0.0, 100.0, 50.0, true).is_none());
        assert!(content_point(0.0, 0.0, 0.0, 50.0, true).is_none());
    }

    #[test]
    fn a_stroke_log_keeps_order_and_drops_the_oldest() {
        let _ = drain_points();
        push_point(1.0, 2.0, Some(0.25));
        push_point(3.0, 4.0, None);
        push_point(f64::NAN, 0.0, Some(1.0));
        let pts = drain_points();
        assert_eq!(pts.len(), 2);
        assert_eq!(pts[0], StrokePoint { x: 1.0, y: 2.0, pressure: Some(0.25) });
        assert_eq!(pts[1].pressure, None);
        assert!(drain_points().is_empty());
        for i in 0..POINT_CAP + 5 {
            push_point(i as f64, 0.0, None);
        }
        let pts = drain_points();
        assert_eq!(pts.len(), POINT_CAP);
        assert_eq!(pts[0].x, 5.0, "the oldest past the cap are gone");
        assert_eq!(pts[pts.len() - 1].x, (POINT_CAP + 4) as f64);
    }

    #[test]
    fn a_pen_is_a_tablet_point_and_a_mouse_is_not() {
        assert_eq!(classify(TABLET_POINT, 0, true, 0.0), Some(0.0), "tip down, no force: thinnest");
        assert_eq!(classify(TABLET_POINT, 0, true, 0.12), Some(0.12));
        assert_eq!(classify(TABLET_POINT, 0, false, 0.2), Some(0.2), "a driver that omits the button");
        assert_eq!(classify(TABLET_POINT, 0, false, 0.0), None, "hover");
        assert_eq!(classify(TABLET_POINT, 0, true, f32::NAN), None);
        assert_eq!(classify(TABLET_POINT, 0, true, 2.0), Some(1.0));
        // Left mouse dragged, carrying a tablet point.
        assert_eq!(classify(6, SUBTYPE_TABLET_POINT, false, 0.05), Some(0.05));
        // The same drag as an ordinary mouse, even if something left a pressure in the event.
        assert_eq!(classify(6, 0, false, 0.4), None);
        assert_eq!(classify(1, 0, false, 1.0), None, "mouse down is full width, not a pen");
        assert_eq!(classify(10, SUBTYPE_TABLET_POINT, false, 0.5), None, "a key is not a pen");
    }

    #[test]
    fn a_tablet_point_covers_the_mouse_copy_and_then_expires() {
        set_tip_down(false);
        remember(0.12, 10.0);
        assert_eq!(pressure_as_of(10.02), Some(0.12));
        assert_eq!(pressure_as_of(10.0 + PEN_WINDOW_SECS + 0.01), None);
        set_tip_down(true);
        assert_eq!(pressure_as_of(10.4), Some(0.12), "the frame is drawn while the tip is still down");
        set_tip_down(false);
        assert_eq!(pressure_as_of(10.4), None);
        remember(0.0, 20.0);
        assert_eq!(pressure_as_of(20.0), Some(0.0), "a light touch is a real sample");
    }

    #[test]
    fn a_drag_carries_the_pen_when_no_tablet_event_arrives() {
        // Left mouse dragged, subtype tablet point.
        assert_eq!(reported(false, 6, SUBTYPE_TABLET_POINT, false, 0.33), (true, Some(0.33)));
        // The same drag from a driver that omits the subtype, lighter than a mouse button.
        assert_eq!(reported(false, 6, 0, false, 0.33), (true, Some(0.33)));
        // A mouse with no sensor reports 0 or 1. Neither is a pen.
        assert_eq!(reported(false, 6, 0, false, 0.0), (false, None));
        assert_eq!(reported(false, 6, 0, false, 1.0), (false, None));
        // Once that pen is down, a hard press and a light drag are real samples.
        assert_eq!(reported(true, 6, 0, false, 1.0), (true, Some(1.0)));
        assert_eq!(reported(true, 6, 0, false, 0.0), (true, Some(0.0)));
        // Left mouse up puts the tip up.
        assert_eq!(reported(true, 2, 0, false, 1.0).0, false);
    }

    #[test]
    fn a_mouse_copy_does_not_erase_a_real_force() {
        // The tablet field in the middle of the range wins over a mouse event at full pressure.
        assert_eq!(prefer_pressure(&[1.0, 0.0, 0.33]), 0.33);
        assert_eq!(prefer_pressure(&[0.0, 1.0]), 1.0, "a mouse stays a mouse");
        assert_eq!(prefer_pressure(&[f32::NAN]), 1.0);
        // A tablet point may replace the force. The mouse copy at 1, in the same instant, may not.
        assert!(accept_sample(true, 1.0, Some((0.33, 10.0)), 10.01));
        assert!(!accept_sample(false, 1.0, Some((0.33, 10.0)), 10.01));
        assert!(!accept_sample(false, 0.0, Some((0.33, 10.0)), 10.01));
        assert!(accept_sample(false, 0.5, Some((0.33, 10.0)), 10.01), "a real change on the drag is kept");
        assert!(accept_sample(false, 1.0, Some((0.33, 10.0)), 10.0 + PEN_WINDOW_SECS + 0.01), "a later hard press is real");
    }
}
