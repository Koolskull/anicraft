# ADR 0113: A raster stroke stays one plate

* Status: accepted (2026-11-16)
* Relates to: ADR 0112 (raster layer), ADR 0032 (bitmap textures)

## Context

The first plate painted, and a light pen did not. A radius under half a pixel was
rejected, so the thinnest mark that showed was a full pixel. Each pointer move also
copied the whole stage and uploaded a new texture, because undo took a snapshot that
shared the buffer and the GPU cache kept that buffer alive.

A copied key (F6) shares the plate until one of the keys is painted. Writing through
that share would paint both keys and would make undo see no change.

## Decisions

1. **The light end of the brush is a hairline, diameter 0.05.** That is the finest
   mark that still rounds to a stored alpha. It covers the one pixel under the pen,
   by `(radius / 0.5)²`, with no softness and no fringe. A radius of half a pixel or
   more stays on the existing disc. The cropped cel's spacing is unchanged. The plate
   uses a tighter step, and a continued move does not stamp its first sample again.
2. **One stroke copies the plate once.** While the gesture is open, `paint.raster`
   does not snapshot or compare the document. The gesture already holds the pre-stroke
   rows. Later dabs write those rows in place when nothing else holds them. Rows are
   separate, so a render of the previous frame only forces a copy of the rows the new
   dab touches. An error restores the gesture and records no step.
3. **The GPU keeps one texture per plate stamp.** It does not keep the plate's bytes.
   A newer generation rewrites the dirty rows of that texture. An older generation
   is ignored, so a render that started earlier cannot wipe the stroke. Undo and
   redo give the plate a new stamp and the texture is uploaded again. Forking away
   from another key takes a new stamp, so the copied key keeps the old texture.
   Library bitmaps stay immutable and keyed by pointer.
4. **While the pen is down, the plate is drawn on the stage directly.** Scene tiles
   for that drag leave the plate out and stay on the picture from before the stroke.
   The line is on screen the same frame. When the pen comes up, the tiles catch up
   and the direct plate goes away.
5. **Stretch is how dry a smudge is.** The Raster tool shows it while Smudge is on.
   0 does not move paint. 1 is the strongest drag the tool will make, a mix of 0.32,
   not a full replace. The default is 0.25, a short dry drag. A caller of `paint.raster` can still pass
   strength 1, which carries the colour the whole stroke. Softness still sets the
   edge and how far back the brush picks up.
6. **The fill's alpha is the opacity of the whole stroke.** Dabs keep the strongest
   coverage at each pixel. That coverage is composited over the picture from before
   the drag, once, at the fill's alpha. A later stroke is its own wash and can build
   on the first. Pressure still changes the size, and it scales a dab's coverage, but
   overlapping dabs do not add their alpha. Smudge is unchanged.
7. **The fill colour's alpha is that opacity, from the same colour picker the vector
   brush uses.** The Raster tool sends that colour with the stroke. Pressure leaves
   the opacity alone unless Opacity is on. Then a dab's coverage runs from Minimum
   at no pressure to Maximum at full pressure, each as a part of the fill's alpha,
   and the strongest coverage at a pixel still wins. The tool starts with Opacity
   off, Minimum at 0% and Maximum at 100%. A caller of `paint.raster` that omits
   the flag still scales coverage by pressure, from 0 to 1. Smudge is unchanged.
8. **The Raster tool has a brush list beside the tools.** Each preset sets the size,
   the thinnest end, the softness, the edge, and whether pressure changes opacity,
   between a minimum and a maximum. The picture on the button is a stroke of that
   preset. Choosing one leaves the fill colour alone, and it leaves the vector
   brushes alone. Brush-tip pictures, scatter and a generative vector pen are not
   part of this list.

## Consequences

* A mouse still paints at full size. Only a pen uses Thinnest.
* The direct plate is the whole stage, nearest-neighbour. ADR 0118 keeps multiply
   artwork above the raster layer (the 2kool grid) on top of that plate, and the
   live texture rewrites only the rows of the latest move.
* The first dab of a stroke, and a stroke on a copied key, still copy what they must
   so undo and the other key stay intact.
* HTML and SVG export of the plate stays later, as ADR 0112 left it.
