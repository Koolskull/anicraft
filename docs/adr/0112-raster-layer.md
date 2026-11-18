# ADR 0112: A raster layer is one stage-sized plate

* Status: accepted (2026-11-16)
* Relates to: ADR 0050 (bitmap smoothing), ADR 0070 (mask layers)

## Context

Vector drawing stays the document. Painting wants a different surface: a transparent
buffer with one texel per stage pixel, more than one of those layers, and a smudge that
mixes colours already on that layer. A cropped stamp floating over the view is not that
surface. The document crate cannot depend on the raster crate (they share a layer), so
the plate is stored as bytes and the engine paints them.

## Decisions

1. **A raster layer holds one straight RGBA8 plate per keyframe, the size of the stage.**
   Empty bytes are transparent. The plate is allocated on the first stroke, not when the
   layer is created. One stage pixel is one texel. It is drawn unsmoothed, so each texel
   is a hard square. The plate is not a library bitmap.
2. **One stroke is one undo step.** A drag is one gesture. Undo of the first stroke on a
   new layer removes the layer as well, when the layer was created by that stroke.
3. **The Raster tool paints the plate. Vector tools refuse a raster layer, and the Raster
   tool paints only a raster layer.** If the active layer is not a raster layer, the first
   stroke adds one. Smudge blends colours already on the plate and does not lay down a new
   hue. Softness 0 is a hard disc and a short pull. Softness 1 fades at the rim and pulls
   from further back.
4. **A plate is not shape-tweened.** F6 copies the plate. F7 clears it. The span holds
   the same picture until the next key. A plate above 64 MiB, or an edge above the bitmap
   edge cap, is refused. A plate on any other layer kind is dropped on load.
5. **HTML and SVG export of the plate is later.** Publish looks up library bitmaps. This
   slice does not add the plate to that path.

## Consequences

* An older file has no plate field and opens unchanged.
* The full-stage rectangle is not a hit target, so it does not steal clicks from the
  shapes on other layers.
* Changing the stage size keeps the old picture in the top-left and clears the rest.
* Clipping one raster layer to the pixels of another is a later decision. Animate's mask
  layer is a different tool and is not reused for that.
