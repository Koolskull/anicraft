# ADR 0116: Raster spray, paper grain, and a pencil on its side

* Status: accepted (2026-11-17)
* Relates to: ADR 0113 (the raster brush list), ADR 0114 (flow)

## Context

A soft raster dab is one smooth disc. Spray paint is not: it is specks of
mixed size and weight, denser in the middle of a soft brush. Ink and charcoal
need the page to break the edge. A woodless coloured pencil, laid over at an
angle, is a different shape again. One side of the lead is a hard lip. The
other side fades out in the direction the stroke is going, and the tooth of
the paper shows through that fade. Turning a corner swings the lip.

The round disc already matches a frozen stamp, and flow at 1 must stay that
stamp. The vector brushes stay as they are. This is another way of filling the
coverage mask, not a new kind of layer.

## Decisions

1. **Three amounts, each 0 to 1, default 0.** `speckle` sprays dots inside the
   brush. `grain` is paper tooth locked to the page, so a second pass catches
   the same spots, and it bites harder where the mark is already soft. `wedge`
   lays the tip on its side: a sharp edge, then a fade along the stroke. At 0,
   or when a value is omitted, the dab is the round stamp. Smudge is unchanged.
2. **The Raster tool shows Spray, Grain and Lay.** They are percents. They are
   off while Smudge is on, because a smudge does not lay down a new mark.
   Round stays all three at 0, so it is still the brush you get before picking
   a preset.
3. **The presets use them.** Airbrush is mostly spray. Pencil, Graphite, Ink,
   Paint and Charcoal take grain, and Charcoal takes a little spray. Colored
   Pencil is wide, with Lay near the top and paper grain. A light press on
   that brush is still a small mark. The aim is the direction of the stroke.
   A pen azimuth can replace that aim later without touching the disc.
4. **A pencil with no direction stays round.** The press has no aim yet.
   Painting it would bake a disc, or a lip that points to the right, into the
   coverage, and a later dab cannot cut that back. The Raster tool waits until
   the pointer moves, then lays the opening dab along that move. A click stays
   round. A continued step still takes its aim from the anchor it does not
   repaint.

## Consequences

* One plain dab is still one call to the old stamp. Spray, grain and the
   pencil profile live beside it, so a later brush changes those numbers.
* The tooth is a hash of the pixel, not a texture file. The same pixel keeps
   the same grain for the life of the build.
* Hover does not show tilt. The stroke direction is the aim until a pen
   reports one.
* The opening dab of a pencil drag is laid with the first move.
