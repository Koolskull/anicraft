# ADR 0118: The live plate stays under the grid, and only the new rows move

* Status: accepted (2026-11-17)
* Relates to: ADR 0113 (the live plate), ADR 0112 (the raster layer)

## Context

While the pen is down, the stage draws the plate itself and leaves it out of the
scene tiles, so the stroke is on screen this frame. Two things flickered.

The plate remembered every row of the stroke until a scene tile caught up, and
the tiles leave the plate out for the whole drag. Once a stroke reached the top
and the bottom of the stage, every move replaced the entire texture. On a
3840×3840 plate that is the whole window flashing, and the stroke with it.

The plate was also drawn after every scene tile. The 2kool grid is a multiply
layer above the ink, so during the stroke the ink covered the lines, and when
the pen lifted the tiles put the lines back on top.

## Decisions

1. **The live texture and the scene tiles keep separate dirty rows.** The live
   view records the generation it has shown. The next move patches only the rows
   that move wrote, in place. The scene tiles still receive every row of the
   stroke when the pen lifts, in one write. An existing texture is never
   replaced wholesale while the generation is moving forward.
2. **Multiply artwork above the raster layer is applied to the live ink.** Each
   bar is its own rectangle. Ink pixels are multiplied by those bars, over the
   stage colour, before the texture is patched. A clear plate pixel stays clear,
   so the scene tile keeps the grid there. The vector brushes are unchanged.

## Consequences

* A long stroke updates a band about as tall as the brush, not the whole stage.
* The 2kool lines stay on the ink while the pen is down, and they do not pop
  when the stroke is committed.
* Where something other than the stage colour sits under a half-transparent
  dab, the live multiply uses the stage colour until the tiles catch up.
