# ADR 0114: Raster flow deposits under the fill alpha

* Status: accepted (2026-11-17)
* Relates to: ADR 0113 (whole-stroke opacity and the raster brush list)

## Context

A raster stroke already keeps one opacity for the whole drag. Dabs record the
strongest coverage, and that coverage is painted once at the fill colour's alpha.
That matches a loaded marker. A pencil does not work that way: a second pass over
the same line should get darker, and it should still stop at the fill's alpha.

OpenToonz and Tahoma 2D keep that split. The standard raster brush is a round
stamp (and, later, a tip image). MyPaint's "opacity linearize" is the idea that
opacity belongs to the finished stroke. AniCraft already does that half. Flow is
the other half: how much of the stroke one dab lays down.

The vector brushes stay as they are. This control belongs to the raster tool.

## Decisions

1. **`paint.raster` takes `flow`, from 0 to 1, default 1.** At 1, or when the
   value is omitted, a dab keeps the stronger coverage, the same wash as ADR 0113.
   Below 1, each dab source-overs that share into the coverage mask. The fill's
   alpha is still applied once, so the stroke cannot pass the colour you picked.
   Smudge is unchanged.
2. **The Raster tool shows Flow beside the other pixel settings.** It is a
   percent, on for every brush, and it does not depend on the Opacity switch.
   100% is today's wash. A lower value lets overlaps build up. The tool starts
   at 100%.
3. **Pencil and Graphite use a low flow.** Pencil is 35%. Graphite is 28%.
   Round, Ink, Marker, Paint, Airbrush and Charcoal stay at 100%, so Round still
   matches a brush that has not picked a preset. Choosing a preset still leaves
   the fill colour alone, and it still leaves the vector brushes alone. The
   preview stroke uses the same deposit as the plate.

## Consequences

* A second gesture still stacks, as it did before. Flow only changes how one
  drag fills its own coverage.
* Brush-tip pictures, spacing, scatter, rotation and pressure curves are later.
  They stamp into this same mask.
* A caller that already omits `flow` paints the wash it painted before.
