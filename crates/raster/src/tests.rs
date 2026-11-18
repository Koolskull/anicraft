//! Numeric checks. One grid is frozen from VMI Studio's `dab`, so the fringe cannot drift.

use crate::{Antialias, Cel, STRETCH_CAP, Sample, Smear, Stroke, Tip, blend_smudge, clip_alpha, cover_dab, dab, dab_opacity, dab_spacing, flow_dab, paint_cover, pen_spacing, sample_stroke, smudge, stamp_stroke, stroke_steps};

fn tip(radius: f32, softness: f32, antialias: Antialias, color: [u8; 4], opacity: f32, erase: bool) -> Tip {
    Tip { radius, softness, antialias, color, opacity, erase }
}

fn plate(width: u32, height: u32) -> Vec<u8> {
    vec![0; (width as usize) * (height as usize) * 4]
}

fn alphas(buf: &[u8], width: u32, x0: u32, y0: u32, n: u32) -> Vec<u8> {
    let mut out = Vec::new();
    for y in y0..y0 + n {
        for x in x0..x0 + n {
            let o = ((y * width + x) * 4 + 3) as usize;
            out.push(buf.get(o).copied().unwrap_or(255));
        }
    }
    out
}

fn pixel(buf: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let o = ((y * width + x) * 4) as usize;
    [
        buf.get(o).copied().unwrap_or(0),
        buf.get(o + 1).copied().unwrap_or(0),
        buf.get(o + 2).copied().unwrap_or(0),
        buf.get(o + 3).copied().unwrap_or(0),
    ]
}

#[test]
fn a_hard_disc_of_radius_4_is_49_pixels_with_a_square_edge() {
    let mut buf = plate(32, 32);
    let hit = dab(&mut buf, 32, 32, 15.5, 15.5, tip(4.0, 0.0, Antialias::Off, [10, 20, 30, 255], 1.0, false));
    assert!(hit);
    let mut ink = 0;
    for alpha in buf.iter().skip(3).step_by(4) {
        assert!(*alpha == 0 || *alpha == 255);
        if *alpha == 255 {
            ink += 1;
        }
    }
    assert_eq!(ink, 49);
    assert_eq!(pixel(&buf, 32, 15, 15), [10, 20, 30, 255]);
    assert_eq!(pixel(&buf, 32, 19, 15), [10, 20, 30, 255]);
    assert_eq!(pixel(&buf, 32, 19, 16), [0, 0, 0, 0]);
}

/// Diameter 0.05 (radius 0.025) stores alpha 1 in the containing pixel. A slightly
/// thinner radius rounds to 0 and is rejected. The radius-4 disc above is unchanged.
#[test]
fn a_hairline_is_one_faint_pixel() {
    let mut buf = plate(16, 16);
    assert!(dab(&mut buf, 16, 16, 4.5, 4.5, tip(0.025, 0.0, Antialias::Off, [10, 20, 30, 255], 1.0, false)));
    assert_eq!(pixel(&buf, 16, 4, 4), [10, 20, 30, 1]);
    for y in 3..6 {
        for x in 3..6 {
            if (x, y) != (4, 4) {
                assert_eq!(pixel(&buf, 16, x, y), [0, 0, 0, 0], "{x},{y}");
            }
        }
    }
    let mut thin = plate(16, 16);
    assert!(!dab(&mut thin, 16, 16, 4.5, 4.5, tip(0.02, 0.0, Antialias::Off, [10, 20, 30, 255], 1.0, false)));
    assert_eq!(pixel(&thin, 16, 4, 4), [0, 0, 0, 0]);
    assert!((dab_spacing(0.05) - 0.5).abs() < 1e-6);
    assert!((dab_spacing(400.0) - 28.0).abs() < 1e-6);
    assert!((pen_spacing(24.0) - 3.6).abs() < 1e-6);
}

/// VMI `dab` on a 32×32 plate, centre (15.5, 15.5), radius 4, softness 0,
/// antialias 0.5, colour (10, 20, 30, 255). The window starts at pixel (11, 11).
const VMI_AA_ALPHA: [[u8; 9]; 9] = [
    [0, 0, 7, 96, 128, 96, 7, 0, 0],
    [0, 66, 228, 255, 255, 255, 228, 66, 0],
    [7, 228, 255, 255, 255, 255, 255, 228, 7],
    [96, 255, 255, 255, 255, 255, 255, 255, 96],
    [128, 255, 255, 255, 255, 255, 255, 255, 128],
    [96, 255, 255, 255, 255, 255, 255, 255, 96],
    [7, 228, 255, 255, 255, 255, 255, 228, 7],
    [0, 66, 228, 255, 255, 255, 228, 66, 0],
    [0, 0, 7, 96, 128, 96, 7, 0, 0],
];

#[test]
fn antialias_half_matches_the_vmi_fringe() {
    let mut buf = plate(32, 32);
    assert!(dab(&mut buf, 32, 32, 15.5, 15.5, tip(4.0, 0.0, Antialias::Px(0.5), [10, 20, 30, 255], 1.0, false)));
    let got = alphas(&buf, 32, 11, 11, 9);
    let mut expected = Vec::new();
    for row in VMI_AA_ALPHA {
        expected.extend_from_slice(&row);
    }
    assert_eq!(got, expected);
    assert_eq!(pixel(&buf, 32, 15, 19), [10, 20, 30, 128]);
    assert_eq!(pixel(&buf, 32, 15, 20), [0, 0, 0, 0]);
    let mut ink = 0;
    for (i, alpha) in buf.iter().skip(3).step_by(4).enumerate() {
        let px = i * 4;
        if *alpha == 0 {
            assert_eq!(&buf[px..px + 3], &[0, 0, 0]);
        } else {
            ink += 1;
            assert_eq!(&buf[px..px + 3], &[10, 20, 30]);
        }
    }
    assert_eq!(ink, 69);

    let mut on = plate(32, 32);
    assert!(dab(&mut on, 32, 32, 15.5, 15.5, tip(4.0, 0.0, Antialias::On, [10, 20, 30, 255], 1.0, false)));
    assert_eq!(on, buf);
}

/// VMI `dab`, centre (11.5, 11.5), radius 5, softness 0.75, antialias 0.5, colour (8, 16, 32, 255).
const VMI_SOFT_ALPHA: [[u8; 9]; 9] = [
    [0, 0, 36, 60, 68, 60, 36, 0, 0],
    [0, 52, 95, 125, 136, 125, 95, 52, 0],
    [36, 95, 148, 188, 204, 188, 148, 95, 36],
    [60, 125, 188, 244, 255, 244, 188, 125, 60],
    [68, 136, 204, 255, 255, 255, 204, 136, 68],
    [60, 125, 188, 244, 255, 244, 188, 125, 60],
    [36, 95, 148, 188, 204, 188, 148, 95, 36],
    [0, 52, 95, 125, 136, 125, 95, 52, 0],
    [0, 0, 36, 60, 68, 60, 36, 0, 0],
];

#[test]
fn softness_matches_the_vmi_falloff() {
    let mut buf = plate(24, 24);
    assert!(dab(&mut buf, 24, 24, 11.5, 11.5, tip(5.0, 0.75, Antialias::On, [8, 16, 32, 255], 1.0, false)));
    let got = alphas(&buf, 24, 7, 7, 9);
    let mut expected = Vec::new();
    for row in VMI_SOFT_ALPHA {
        expected.extend_from_slice(&row);
    }
    assert_eq!(got, expected);
    assert_eq!(pixel(&buf, 24, 11, 11), [8, 16, 32, 255]);
}

#[test]
fn two_overlapping_covers_stay_at_the_stronger_dab() {
    let mut mask = vec![0u8; 8 * 8];
    let half = tip(2.0, 0.0, Antialias::Off, [255, 0, 0, 128], 1.0, false);
    assert!(cover_dab(&mut mask, 8, 8, 3.5, 3.5, half));
    let once = mask[3 * 8 + 3];
    let _ = cover_dab(&mut mask, 8, 8, 3.5, 3.5, half);
    assert_eq!(mask[3 * 8 + 3], once, "a second dab raised the coverage");
    assert_eq!(once, 255);
    let mut pixels = vec![0u8; 8 * 8 * 4];
    assert!(paint_cover(&mut pixels, &mask, [255, 0, 0, 128], false));
    let px = pixel(&pixels, 8, 3, 3);
    assert_eq!(px, [255, 0, 0, 128], "fill alpha should scale the stroke once, got {px:?}");
}

#[test]
fn two_overlapping_dabs_composite_source_over() {
    let mut buf = plate(8, 8);
    let white = tip(2.0, 0.0, Antialias::Off, [255, 255, 255, 255], 0.5, false);
    assert!(dab(&mut buf, 8, 8, 3.5, 3.5, white));
    assert_eq!(pixel(&buf, 8, 3, 3), [255, 255, 255, 128]);
    assert!(dab(&mut buf, 8, 8, 3.5, 3.5, white));
    assert_eq!(pixel(&buf, 8, 3, 3), [255, 255, 255, 192]);
}

#[test]
fn erase_lowers_alpha_and_leaves_rgb() {
    let mut buf = plate(8, 8);
    assert!(dab(&mut buf, 8, 8, 3.5, 3.5, tip(2.0, 0.0, Antialias::Off, [200, 10, 20, 255], 1.0, false)));
    assert_eq!(pixel(&buf, 8, 3, 3), [200, 10, 20, 255]);
    assert!(dab(&mut buf, 8, 8, 3.5, 3.5, tip(2.0, 0.0, Antialias::Off, [0, 255, 0, 255], 0.5, true)));
    assert_eq!(pixel(&buf, 8, 3, 3), [200, 10, 20, 128]);
}

#[test]
fn smudge_carries_red_and_does_not_invent_green() {
    let mut cel = Cel::blank(0, 0, 24, 16).unwrap_or_else(Cel::empty);
    assert!(cel.set(2, 6, [255, 0, 0, 255]));
    let before = cel.pixels().to_vec();
    assert!(!smudge(&mut cel, 2.5, 6.5, 12.5, 6.5, 1.5, 0.0));
    assert_eq!(cel.pixels(), before.as_slice());

    assert!(smudge(&mut cel, 2.5, 6.5, 12.5, 6.5, 1.5, 1.0));
    assert_eq!(cel.pixel(2, 6).unwrap_or([0; 4]), [255, 0, 0, 255]);
    assert_eq!(cel.pixel(12, 6).unwrap_or([0; 4]), [255, 0, 0, 255]);
    for px in cel.pixels().as_chunks::<4>().0 {
        assert_eq!(px[1], 0);
        assert_eq!(px[2], 0);
    }
}

#[test]
fn clip_keeps_the_base_alpha() {
    let mut base = Cel::blank(0, 0, 16, 16).unwrap_or_else(Cel::empty);
    base.fill([255, 0, 0, 128]);
    let mut blue = Cel::blank(0, 0, 16, 16).unwrap_or_else(Cel::empty);
    assert!(blue.stamp(4.5, 4.5, tip(2.0, 0.0, Antialias::Off, [0, 0, 255, 255], 1.0, false)));
    let out = clip_alpha(&base, &blue);
    assert_eq!(out.size(), (16, 16));
    assert_eq!(out.pixel(4, 4).unwrap_or([0; 4]), [0, 0, 255, 128]);
    assert_eq!(out.pixel(15, 15).unwrap_or([0; 4]), [255, 0, 0, 128]);
    for px in out.pixels().as_chunks::<4>().0 {
        assert_eq!(px[3], 128);
    }
}

#[test]
fn pressure_is_interpolated_between_samples() {
    let steps = stroke_steps(0.0, 0.0, 0.0, 10.0, 0.0, 1.0, 5.0);
    assert_eq!(steps.len(), 2);
    assert_eq!(steps.first().copied().unwrap_or(Sample { x: -1.0, y: -1.0, pressure: -1.0 }), Sample { x: 5.0, y: 0.0, pressure: 0.5 });
    assert_eq!(steps.get(1).copied().unwrap_or(Sample { x: -1.0, y: -1.0, pressure: -1.0 }), Sample { x: 10.0, y: 0.0, pressure: 1.0 });

    let sampled = sample_stroke(&[Sample { x: 0.0, y: 0.0, pressure: 0.2 }, Sample { x: 12.0, y: 0.0, pressure: 1.0 }], |_| 4.0);
    assert_eq!(sampled.len(), 4);
    let pressures: Vec<f32> = sampled.iter().map(|sample| sample.pressure).collect();
    assert!((pressures.first().copied().unwrap_or(0.0) - 0.2).abs() < 1e-6);
    assert!((pressures.get(1).copied().unwrap_or(0.0) - 0.46666667).abs() < 1e-5);
    assert!((pressures.get(2).copied().unwrap_or(0.0) - 0.73333335).abs() < 1e-5);
    assert!((pressures.get(3).copied().unwrap_or(0.0) - 1.0).abs() < 1e-6);
    assert!((pen_spacing(24.0) - 3.6).abs() < 1e-6);
    assert!((pen_spacing(2.0) - 0.75).abs() < 1e-6);
}

#[test]
fn a_stroke_at_zero_pressure_stays_clear_unless_opacity_ignores_pressure() {
    let point = [Sample { x: 5.0, y: 5.0, pressure: 0.0 }];
    let stroke = Stroke {
        size: 8.0,
        size_min: 8.0,
        softness: 0.0,
        antialias: Antialias::Off,
        color: [1, 2, 3, 255],
        erase: false,
        opacity_from_pressure: true,
        opacity_min: 0.0,
        opacity_max: 1.0,
        flow: 1.0,
    };
    let mut cel = Cel::empty();
    assert!(!stamp_stroke(&mut cel, &point, stroke));
    assert!(cel.pixels().is_empty());

    let mut painted = Cel::empty();
    assert!(stamp_stroke(&mut painted, &point, Stroke { opacity_from_pressure: false, ..stroke }));
    assert!(painted.pixel(5, 5).unwrap_or([0; 4])[3] > 0);
}

#[test]
fn opacity_range_maps_pressure_and_ignores_it_when_off() {
    let stroke = Stroke { size: 8.0, size_min: 8.0, softness: 0.0, antialias: Antialias::Off, color: [255, 0, 0, 255], erase: false, opacity_from_pressure: true, opacity_min: 0.25, opacity_max: 0.75, flow: 1.0 };
    assert!((dab_opacity(stroke, 0.0) - 0.25).abs() < 1e-6);
    assert!((dab_opacity(stroke, 0.5) - 0.5).abs() < 1e-6);
    assert!((dab_opacity(stroke, 1.0) - 0.75).abs() < 1e-6);
    assert!((dab_opacity(Stroke { opacity_from_pressure: false, ..stroke }, 0.0) - 1.0).abs() < 1e-6);
}

#[test]
fn a_low_flow_builds_and_a_full_flow_does_not() {
    let tip = tip(4.0, 0.0, Antialias::Off, [255, 0, 0, 255], 1.0, false);
    let mut mask = vec![0u8; 16 * 16];
    assert!(flow_dab(&mut mask, 16, 16, 8.0, 8.0, tip, 0.45));
    let once = mask[8 * 16 + 8];
    assert!(flow_dab(&mut mask, 16, 16, 8.0, 8.0, tip, 0.45));
    let twice = mask[8 * 16 + 8];
    assert!(twice > once && twice < 255, "flow should build under a full stamp, {once} then {twice}");
    let mut full = vec![0u8; 16 * 16];
    assert!(flow_dab(&mut full, 16, 16, 8.0, 8.0, tip, 1.0));
    let solid = full[8 * 16 + 8];
    let _ = flow_dab(&mut full, 16, 16, 8.0, 8.0, tip, 1.0);
    assert_eq!(full[8 * 16 + 8], solid, "flow 1 should keep the stronger dab");
    assert_eq!(solid, 255);
}

#[test]
fn the_cel_grows_to_the_dab_and_refuses_a_huge_plate() {
    let mut cel = Cel::empty();
    assert!(!cel.stamp(0.0, 0.0, tip(3000.0, 0.0, Antialias::Off, [0, 0, 0, 255], 1.0, false)));
    assert!(cel.pixels().is_empty());

    assert!(cel.stamp(15.5, 15.5, tip(4.0, 0.0, Antialias::Off, [10, 20, 30, 255], 1.0, false)));
    assert_eq!(cel.origin(), (11, 11));
    assert_eq!(cel.size(), (10, 10));
    let kept = cel.pixels().to_vec();
    let origin = cel.origin();
    assert!(!cel.stamp(0.0, 0.0, tip(3000.0, 0.0, Antialias::Off, [9, 9, 9, 255], 1.0, false)));
    assert_eq!(cel.pixels(), kept.as_slice());
    assert_eq!(cel.origin(), origin);

    cel.crop_to_ink();
    assert_eq!(cel.origin(), (11, 11));
    assert_eq!(cel.size(), (9, 9));
    assert_eq!(cel.pixel(15, 15).unwrap_or([0; 4]), [10, 20, 30, 255]);
    assert_eq!(cel.pixel(19, 15).unwrap_or([0; 4])[3], 255);
    assert!(cel.pixel(20, 15).is_none());
}

#[test]
fn a_smudge_of_strength_zero_leaves_the_buffer() {
    let mut buf = plate(16, 16);
    buf[0] = 9;
    let hit = blend_smudge(&mut buf, 16, 16, 4.0, 4.0, 12.0, 4.0, Smear { radius: 4.0, softness: 0.0, strength: 0.0 });
    assert!(!hit);
    assert_eq!(buf[0], 9);
    assert!(buf[1..].iter().all(|b| *b == 0));
}

fn smear_over_blue(strength: f32) -> Vec<[u8; 4]> {
    let mut buf = plate(48, 16);
    for px in buf.as_chunks_mut::<4>().0 {
        px[2] = 255;
        px[3] = 255;
    }
    assert!(dab(&mut buf, 48, 16, 6.0, 8.0, tip(4.0, 0.0, Antialias::Off, [255, 0, 0, 255], 1.0, false)));
    assert!(blend_smudge(&mut buf, 48, 16, 6.0, 8.0, 36.0, 8.0, Smear { radius: 4.0, softness: 0.0, strength }));
    (0..48).map(|x| pixel(&buf, 48, x, 8)).collect()
}

#[test]
fn a_capped_stretch_fades_before_a_full_replace() {
    let full = smear_over_blue(1.0);
    let capped = smear_over_blue(STRETCH_CAP);
    let dry = smear_over_blue(0.08);
    let full_far = full.get(30).copied().unwrap_or([0; 4]);
    let cap_near = capped.get(10).copied().unwrap_or([0; 4]);
    let cap_far = capped.get(30).copied().unwrap_or([0; 4]);
    let dry_near = dry.get(10).copied().unwrap_or([0; 4]);
    assert!(full_far[0] > 200 && full_far[2] < 20, "a full replace should carry red the whole way, got {full_far:?}");
    assert!(cap_near[0] > 60 && cap_near[2] > 60, "the top of Stretch should mix both colours, got {cap_near:?}");
    assert!(cap_far[0] < 40, "the top of Stretch should fade along the stroke, got {cap_far:?}");
    assert!(dry_near[0] > 20 && dry_near[0] < 80, "a dry drag should only tint, got {dry_near:?}");
}

#[test]
fn a_hard_smudge_pulls_red_and_adds_no_other_hue() {
    let mut buf = plate(48, 16);
    assert!(dab(&mut buf, 48, 16, 6.0, 8.0, tip(4.0, 0.0, Antialias::Off, [255, 0, 0, 255], 1.0, false)));
    assert!(blend_smudge(&mut buf, 48, 16, 6.0, 8.0, 36.0, 8.0, Smear { radius: 4.0, softness: 0.0, strength: 1.0 }));
    let mut pulled = false;
    for y in 0..16 {
        for x in 0..48 {
            let px = pixel(&buf, 48, x, y);
            assert_eq!((px[1], px[2]), (0, 0), "smudge at {x},{y} added a hue: {px:?}");
            if x >= 24 && px[0] > 200 && px[3] > 200 {
                pulled = true;
            }
        }
    }
    assert!(pulled, "the red did not travel with the brush");
}

#[test]
fn a_soft_smudge_fades_at_the_rim() {
    let mut buf = plate(32, 32);
    assert!(dab(&mut buf, 32, 32, 16.0, 16.0, tip(3.0, 0.0, Antialias::Off, [255, 0, 0, 255], 1.0, false)));
    assert!(blend_smudge(&mut buf, 32, 32, 16.0, 16.0, 16.0, 16.0, Smear { radius: 8.0, softness: 1.0, strength: 1.0 }));
    let mut partial = false;
    for px in buf.as_chunks::<4>().0 {
        let alpha = px.get(3).copied().unwrap_or(0);
        if alpha > 0 && alpha < 255 {
            partial = true;
        }
    }
    assert!(partial, "a soft smudge should not be only clear or opaque");
}

#[test]
fn an_in_place_smudge_mixes_red_with_blue() {
    let mut buf = plate(32, 32);
    assert!(dab(&mut buf, 32, 32, 12.0, 16.0, tip(4.0, 0.0, Antialias::Off, [255, 0, 0, 255], 1.0, false)));
    assert!(dab(&mut buf, 32, 32, 20.0, 16.0, tip(4.0, 0.0, Antialias::Off, [0, 0, 255, 255], 1.0, false)));
    assert!(blend_smudge(&mut buf, 32, 32, 16.0, 16.0, 16.0, 16.0, Smear { radius: 8.0, softness: 1.0, strength: 1.0 }));
    let px = pixel(&buf, 32, 16, 16);
    assert!(px[0] > 20 && px[2] > 20 && px[1] == 0, "expected red and blue mixed, got {px:?}");
}
