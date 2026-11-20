# ADR 0125: Raster tool properties use the same modules and dials

* Status: accepted (2026-11-18)
* Relates to: ADR 0124 (square modules and dials), ADR 0112 (the raster layer)

## Context

The Raster tool's properties were a form: one labelled row for size, thinnest, flow,
softness, spray, grain, lay, the pressure range, antialias, erase, smudge and stretch.
The section was taller than the panel, so the later rows sat below the fold. The Layer
section had already left that shape (ADR 0124).

## Decisions

1. **The Raster tool uses that same face.** Fill, pressure opacity, erase and smudge are
   small squares. Antialias is one cell: it shows the current amount, and a click lists
   Off, 0.5, 1 and 2. Size, thinnest, flow and softness are dials. Drag up to raise a
   dial. Shift drags more finely. Double-click returns size to 24, thinnest to the
   hairline, flow to 100 %, and the other percents to 0 %. Stretch returns to 25 %.
2. **A dial that does not apply stays off the face.** Spray, grain and lay show when the
   dab is a round mark. A picture brush hides them, and one line says each dab is that
   picture, turned. Smudge hides them and shows stretch. Minimum and maximum show only
   while pressure opacity is on.
3. **The values are unchanged.** The squares and dials write the same tool state the rows
   wrote. New Raster Layer stays a button above the grid. The fill square opens the same
   fill picker.

## Consequences

* The Raster tool's properties fit in a short band instead of a scrolling form.
* Other tool sections that are still a long list of rows are not this face yet. They
  follow the same modules when they are next.
