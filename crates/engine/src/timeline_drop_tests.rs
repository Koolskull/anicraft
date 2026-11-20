//! SVG into the Library and drops on the timeline (ADR 0065): a drawing becomes one graphic
//! symbol; media dropped on a layer's frame land there, in one undo step, and layers that can't
//! take them refuse without changing anything.

use super::*;
use serde_json::json;
use std::path::Path;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("anicraft-tldrop-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A 4 × 2 PNG, a 40 × 20 one-rectangle SVG, a short WAV and a masked SVG in `dir`.
fn media(dir: &Path) -> (String, String, String, String) {
    let png = dir.join("photo.png");
    std::fs::write(&png, anicraft_render::encode_png(4, 2, &[255u8; 4 * 2 * 4]).unwrap()).unwrap();
    let svg = dir.join("badge.svg");
    std::fs::write(&svg, r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="#f00"/></svg>"##).unwrap();
    let wav = dir.join("beep.wav");
    std::fs::write(&wav, crate::cmds_sound::test_wav(8000, &[0.25; 4000])).unwrap();
    let masked = dir.join("masked.svg");
    std::fs::write(&masked, r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><clipPath id="c"><circle cx="20" cy="20" r="10"/></clipPath><rect width="40" height="40" fill="#0f0" clip-path="url(#c)"/></svg>"##).unwrap();
    let s = |p: std::path::PathBuf| p.display().to_string();
    (s(png), s(svg), s(wav), s(masked))
}

#[test]
fn an_svg_imported_to_the_library_is_one_graphic_symbol_centred_on_its_registration_point() {
    let dir = temp_dir("svgsym");
    let (_, svg, _, masked) = media(&dir);
    let mut s = Session::default();
    let history = s.history_len();
    let v = run(&mut s, "file.import_to_library", json!({"path": svg}));
    assert_eq!(s.history_len(), history + 1, "one undo step");
    assert_eq!((s.doc.library.len(), v["name"].as_str()), (1, Some("badge")));
    let sym = s.doc.library[0].clone();
    assert_eq!(sym.kind, anicraft_doc::SymbolKind::Graphic);
    assert_eq!(sym.timeline.layers.len(), 1);
    // The 40 × 20 drawing sits around the registration point.
    let k = &sym.timeline.layers[0].keyframes[0];
    let b = k.elements.iter().fold(anicraft_geom::Rect::EMPTY, |r, e| match &e.kind {
        anicraft_doc::ElementKind::Shape(sh) => r.union(&sh.path.bounds().transformed(&e.transform)),
        _ => r,
    });
    assert!(b.center().x.abs() < 0.01 && b.center().y.abs() < 0.01 && (b.width() - 40.0).abs() < 0.01, "{b:?}");
    assert!(s.timeline().layers.iter().all(|l| l.keyframes.iter().all(|k| k.elements.is_empty())), "nothing on the stage");
    // Again: a free name. A masked drawing keeps its mask layer inside the symbol.
    assert_eq!(run(&mut s, "file.import_to_library", json!({"path": svg}))["name"], "badge 2");
    run(&mut s, "file.import_to_library", json!({"path": masked}));
    let m = s.doc.library.last().unwrap();
    assert!(m.timeline.layers.iter().any(|l| l.kind == anicraft_doc::LayerKind::Mask) && m.timeline.layers.len() == 2, "{:?}", m.timeline.layers.iter().map(|l| (&l.name, l.kind)).collect::<Vec<_>>());
    // Placed, it shows.
    run(&mut s, "library.place", json!({"symbol": sym.id.0, "x": 100, "y": 100}));
    assert!(!anicraft_doc::evaluate(&s.doc, 0).items.is_empty());
    let empty = dir.join("empty.svg");
    std::fs::write(&empty, r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"></svg>"#).unwrap();
    let before = s.doc.clone();
    assert!(s.execute("file.import_to_library", &json!({"path": empty.display().to_string()})).is_err(), "a drawing with no shapes adds nothing");
    assert!(s.doc == before);
}

/// A session whose one layer has a keyframe at 0, 12 frames long, holding a rectangle.
fn timeline_session() -> (Session, anicraft_doc::LayerId) {
    let mut s = Session::default();
    run(&mut s, "draw.rect", json!({"x": 0, "y": 0, "w": 10, "h": 10}));
    run(&mut s, "timeline.insert_frame", json!({"count": 11}));
    let layer = s.layer;
    (s, layer)
}

#[test]
fn an_image_dropped_on_a_layers_frame_goes_into_a_keyframe_there_in_one_step() {
    let dir = temp_dir("tlimg");
    let (png, svg, _, _) = media(&dir);
    let (mut s, layer) = timeline_session();
    let above = run(&mut s, "layer.new", json!({}))["layer"].as_u64().unwrap();
    let history = s.history_len();
    let v = run(&mut s, "file.drop", json!({"paths": [png, svg], "to": "timeline", "layer": layer.0, "frame": 5, "x": 200, "y": 100}));
    assert_eq!(s.history_len(), history + 1, "one undo step for the whole drop");
    assert_eq!((s.layer, s.frame, v["keyframe_made"].as_bool()), (layer, 5, Some(true)), "the playhead and layer go to the drop");
    let l = s.timeline().layer(layer).unwrap();
    let k = l.keyframe_at(5).unwrap();
    assert_eq!(k.start, 5, "a keyframe starting at the frame");
    assert_eq!(k.elements.len(), 3, "what the frame showed, the image and the drawing");
    assert_eq!(l.keyframe_at(0).unwrap().elements.len(), 1, "the frames before are untouched");
    assert_eq!(s.selection.len(), 2, "what was dropped is selected");
    for id in s.selection.ids().collect::<Vec<_>>() {
        let b = s.element_bounds(id);
        assert!((b.center().x - 200.0).abs() < 0.01 && (b.center().y - 100.0).abs() < 0.01, "{b:?}");
    }
    assert!(s.timeline().layer(anicraft_doc::LayerId(above)).unwrap().keyframes.iter().all(|k| k.elements.is_empty()), "the layer active before the drop is untouched");
    run(&mut s, "edit.undo", json!({}));
    let l = s.timeline().layer(layer).unwrap();
    assert!(l.keyframe_starting_at(5).is_none() && s.doc.bitmaps.is_empty());
    // On a keyframe's first frame, no new keyframe: the image joins it.
    let v = run(&mut s, "file.drop", json!({"paths": [media(&dir).0], "to": "timeline", "layer": layer.0, "frame": 0}));
    assert_eq!(v["keyframe_made"].as_bool(), Some(false));
    assert_eq!(s.timeline().layer(layer).unwrap().keyframes.len(), 1);
}

#[test]
fn a_sound_dropped_on_a_cell_starts_on_that_cell() {
    let dir = temp_dir("tlsnd");
    let wav = media(&dir).2;
    // Inside a span: a keyframe is made at that cell. The sound starts there, and the drawing stays.
    let (mut s, layer) = timeline_session();
    let history = s.history_len();
    let v = run(&mut s, "file.drop", json!({"path": &wav, "to": "timeline", "layer": layer.0, "frame": 7}));
    assert_eq!(s.history_len(), history + 1, "one undo step");
    assert_eq!(v["keyframe_made"].as_bool(), Some(true));
    let l = s.timeline().layer(layer).unwrap();
    assert_eq!(l.keyframes.len(), 2);
    assert!(l.keyframes[0].start == 0 && l.keyframes[0].sound.is_none() && l.keyframes[0].elements.len() == 1, "the earlier keyframe keeps the drawing and does not take the sound");
    let k = l.keyframe_at(7).unwrap();
    assert!(k.start == 7 && k.sound.is_some() && k.elements.len() == 1, "the clip starts on the cell, and the drawing stays");
    assert!(k.duration >= 12, "the new span covers the sound so its waveform can show, got {}", k.duration);
    run(&mut s, "edit.undo", json!({}));
    let l = s.timeline().layer(layer).unwrap();
    assert_eq!(l.keyframes.len(), 1);
    assert!(l.keyframes[0].sound.is_none() && s.doc.sounds.is_empty());
    // The first cell of a keyframe, including frame 0: no split.
    let v = run(&mut s, "file.drop", json!({"path": &wav, "to": "timeline", "layer": layer.0, "frame": 0}));
    assert_eq!(v["keyframe_made"].as_bool(), Some(false));
    let l = s.timeline().layer(layer).unwrap();
    assert_eq!(l.keyframes.len(), 1);
    assert!(l.keyframes[0].sound.is_some());
    // Past the layer's end, a keyframe is made there for it.
    run(&mut s, "edit.undo", json!({}));
    let v = run(&mut s, "file.drop", json!({"path": &wav, "to": "timeline", "layer": layer.0, "frame": 30}));
    assert_eq!(v["keyframe_made"].as_bool(), Some(true));
    let l = s.timeline().layer(layer).unwrap();
    assert!(l.keyframe_at(30).is_some_and(|k| k.start == 30 && k.sound.is_some()));
    assert!(l.keyframe_at(0).is_some_and(|k| k.sound.is_none()));
}

#[test]
fn a_sound_dropped_on_a_raster_layer_joins_the_library_and_the_keyframe() {
    let dir = temp_dir("tlraster");
    let (png, _, wav, _) = media(&dir);
    let mut s = Session::default();
    let id = run(&mut s, "layer.new_raster", json!({}))["layer"].as_u64().unwrap();
    let layer = anicraft_doc::LayerId(id);
    assert_eq!(s.timeline().layer(layer).map(|l| l.kind), Some(anicraft_doc::LayerKind::Raster));
    let history = s.history_len();
    let v = run(&mut s, "file.drop", json!({"path": wav, "to": "timeline", "layer": layer.0, "frame": 0}));
    assert_eq!(s.history_len(), history + 1, "one undo step");
    assert_eq!(v["keyframe_made"].as_bool(), Some(false), "a sound uses the keyframe already there");
    assert_eq!(s.doc.sounds.len(), 1, "the file is in the library");
    assert_eq!(s.doc.sounds[0].name, "beep.wav");
    let l = s.timeline().layer(layer).unwrap();
    let k = &l.keyframes[0];
    assert!(k.sound.as_ref().is_some_and(|fs| fs.sound == s.doc.sounds[0].id), "the keyframe references that library sound");
    assert!(k.duration >= 12, "the span covers the sound so its waveform can show, got {}", k.duration);
    assert!(s.timeline().layers.iter().filter(|l| l.id != layer).all(|l| l.keyframes.iter().all(|k| k.sound.is_none())), "the other layers are untouched");
    // A picture on the same pixel layer is still paint, not a drop. Nothing more is imported.
    let before = s.doc.clone();
    let e = s.execute("file.drop", &json!({"path": png, "to": "timeline", "layer": layer.0, "frame": 0})).unwrap_err();
    assert!(e.to_string().contains("pixel"), "{e}");
    assert!(s.doc == before);
    let mixed = dir.join("beep2.wav");
    std::fs::copy(dir.join("beep.wav"), &mixed).unwrap();
    let e = s.execute("file.drop", &json!({"paths": [dir.join("photo.png").display().to_string(), mixed.display().to_string()], "to": "timeline", "layer": layer.0, "frame": 0})).unwrap_err();
    assert!(e.to_string().contains("pixel"), "{e}");
    assert!(s.doc == before, "a picture beside a sound does not import either");
    // Locked, including the pixel layer under the 2kool grid: still refused.
    s.layer = layer;
    run(&mut s, "layer.toggle_lock", json!({}));
    let before = s.doc.clone();
    let e = s.execute("file.drop", &json!({"path": mixed.display().to_string(), "to": "timeline", "layer": layer.0, "frame": 0})).unwrap_err();
    assert!(e.to_string().contains("locked"), "{e}");
    assert!(s.doc == before);
}

#[test]
fn drops_on_the_timeline_refuse_layers_that_cant_take_them_and_change_nothing() {
    let dir = temp_dir("tlbad");
    let (png, _, wav, masked) = media(&dir);
    let (mut s, layer) = timeline_session();
    let folder = run(&mut s, "layer.new_folder", json!({}))["layer"].as_u64().unwrap();
    let layers = s.timeline().layers.len();
    // A drawing with masks can't go on one layer: a graphic symbol holding it goes there.
    run(&mut s, "file.drop", json!({"path": masked, "to": "timeline", "layer": layer.0, "frame": 2}));
    assert_eq!(s.timeline().layers.len(), layers, "no layers added");
    let k = s.timeline().layer(layer).unwrap().keyframe_at(2).unwrap();
    assert!(k.elements.iter().any(|e| matches!(e.kind, anicraft_doc::ElementKind::Symbol { .. })));
    assert_eq!(s.doc.library.len(), 1);
    let before = s.doc.clone();
    let wav2 = dir.join("beep2.wav");
    std::fs::copy(&wav, &wav2).unwrap();
    for p in [
        json!({"path": png.clone(), "to": "timeline", "layer": folder, "frame": 0}),
        json!({"path": png.clone(), "to": "timeline", "layer": u64::MAX, "frame": 0}),
        json!({"path": png.clone(), "to": "timeline", "layer": layer.0}),
        json!({"path": png.clone(), "to": "timeline", "frame": 0}),
        json!({"path": png.clone(), "to": "timeline", "layer": layer.0, "frame": u64::MAX}),
        json!({"path": png.clone(), "to": "timeline", "layer": layer.0, "frame": -1}),
        json!({"paths": [wav.clone(), wav2.display().to_string()], "to": "timeline", "layer": layer.0, "frame": 0}),
        json!({"paths": [png.clone(), dir.join("gone.png").display().to_string()], "to": "timeline", "layer": layer.0, "frame": 4}),
    ] {
        assert!(s.execute("file.drop", &p).is_err(), "{p}");
        assert!(s.doc == before, "{p} changed the document");
    }
    s.layer = layer;
    run(&mut s, "layer.toggle_lock", json!({}));
    let before = s.doc.clone();
    for f in [png, wav] {
        let e = s.execute("file.drop", &json!({"path": f, "to": "timeline", "layer": layer.0, "frame": 1})).unwrap_err();
        assert!(e.to_string().contains("locked"), "{e}");
        assert!(s.doc == before);
    }
}
