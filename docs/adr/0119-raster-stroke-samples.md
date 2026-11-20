# ADR 0119: A raster stroke keeps the pen's samples, and the scene draws the ink

* Status: accepted (2026-11-18)
* Relates to: ADR 0113 (the live plate), ADR 0117 (spray pictures), ADR 0118 (the baked grid)

## Context

ADR 0118 drew the plate on top of the scene and baked the 2kool grid into those
pixels so the lines would not pop when the pen lifted. On a 3840 stage that
still hitched. The first texture was the whole plate. Every later patch was a
full-width band, and any pixel that was not clear was forced opaque. The same
frame rendered the plate-free scene, grid included, on the UI thread.

A slow frame only keeps the latest pointer. The stroke from the press to that
point is one straight chord, so the beginning of the line loses the samples a
custom brush image needs. When the pen lifts, the baked plate (opaque, stage
colour underneath, nearest plate pixels) is replaced by the real composite
(multiply, at the view's resolution). Spray pictures are mostly partial alpha,
so the grid flickers as the stroke is registered.

## Decisions

1. **The pen's points are logged as they arrive.** On macOS the tablet monitor
   records each drag, including the points AppKit coalesced into one event.
   The raster tool drains that log. The vector brushes do not read it. A frame
   stamps a bounded run of those points into the plate and keeps the rest for
   the next frame. Lifting the pen stamps whatever is left, so the line is
   whole. Picture stamps keep a dab count for the gesture, so a later move
   does not reuse the first dab's turn.
2. **The scene tiles draw the ink.** While the pen is down the UI thread does
   not render, and it does not upload a plate or bake the grid. The worker
   renders the tiles the stroke has touched, from the real document, so
   multiply, masks and filters are the same picture the pen-up frame will
   show. Tiles from before this update stay on screen underneath until the
   new ones are the ones drawn. The overlay from ADR 0118 is not used for a
   stroke.

## Consequences

* The start of a stroke is in the plate even when the frame that took the
  press was slow. A fast picture stroke can trail the cursor by the tiles
  still in flight. It does not skip the samples.
* The 2kool grid is drawn by the same renderer during the stroke and after
  it. Partial-alpha brush images stay partial.
* Custom brush images still stamp through the raster crate into the document
  plate. One stroke is still one undo. The vector brushes are unchanged.
