# ADR 0117: A spray dab is the supplied picture, turned

* Status: accepted (2026-11-17)
* Relates to: ADR 0113 (the raster brush list), ADR 0116 (procedural spray, grain, and lay)

## Context

The procedural spray is a handful of dots inside a disc. The four pictures
already drawn for this brush are the mark: a fine mist, a heavy can, a tight
dot, and a spray can. They are high resolution. Sampling them and turning each
dab is the whole trick. Inventing a new speckle field throws that drawing away.

The pictures stay in the portable data folder, `data/spray/`, as
`fine.bin`, `heavy.bin`, `tight.bin` and `can.bin`. They are not part of the
upstream tree. A byte is coverage, 0 clear and 255 solid, after the white
margin is cropped and the ink is centred in a square so a turn does not
stretch it.

## Decisions

1. **`paint.raster` takes `picture`, 0 to 4, default 0.** 0 is the round or
   grain dab from ADR 0116. 1 is fine, 2 heavy, 3 tight, 4 the spray can.
   A picture dab ignores speckle, grain, wedge, softness and the fringe. The
   picture's own grey is the coverage, and the fill colour's alpha is still
   the opacity of the whole stroke.
2. **Each dab is turned, and the step is the round dab's step.** The turn
   comes from the dab's place and its index, so the same stroke paints the
   same way again and a neighbour does not share the angle. The gap is about
   7% of the diameter, the same as a round dab, so the stroke does not break
   into separate puffs. A picture dab always keeps the stronger sample. The
   fill colour's alpha is applied once for the stroke, so an overlap does not
   get darker.
3. **The Raster list grows three brushes and Airbrush changes.** Fine Spray,
   Heavy Spray and Tight Spray are the first three pictures. Airbrush is the
   spray-can picture. The vector brushes are unchanged. Round, pencil, ink
   and the rest stay discs.

## Consequences

* A click lays one whole picture. A drag is a chain of that picture, each copy
  turned. Size scales it. The stored plate is still the undo.
* If the four files are missing, a picture dab lays nothing. The launch log
  counts how many loaded.
* Procedural spray, grain and lay stay for the brushes that do not name a
  picture.
