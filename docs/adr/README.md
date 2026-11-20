# Architecture decision records

One file per decision: `NNNN-short-title.md`, numbered in order. The files in this folder are the
index.

## Numbers never move

A pull request keeps the ADR number it was given until it merges or closes, so its author never
has to renumber because another PR merged first.

* **Taking a number:** use the next number above every ADR in this folder **and** every number in
  the table below. Add your number to the table in the same PR (or ask a maintainer to reserve it
  for you before you start).
* **Reserved numbers are skipped.** No other PR may use a reserved number, even if a lower number
  looks free on `main`. Gaps in the sequence are expected.
* **When a PR merges,** its row leaves the table (the ADR file now holds the number). **When a PR
  closes without merging,** its row leaves the table and the number stays unused.
* Maintainers keep this table current for open pull requests from anyone, including outside
  contributors whose PR already contains an ADR: that PR's number is reserved as it is.

## Reserved for open pull requests

| Number | Pull request | Topic |
|---|---|---|
| 0037 | #38 (outside contribution) | Interface languages |
| 0096 | #336 | Raw brush, pen pressure |
| 0097 | #354 | Fill and stroke alpha |
| 0098 | feat/studio-chrome (local) | Canvas turn with Shift+Space, zoom drag with Ctrl+Space |
| 0099 | feat/studio-chrome (local) | Studio chrome and 2kool account files |
| 0100 | feat/studio-chrome (local) | Vertical X-sheet on the same timeline |
| 0101 | feat/studio-chrome (local) | Blender-style grab, rotate and scale |
| 0102 | feat/studio-chrome (local) | Pen pressure read in one place |
| 0103 | feat/studio-chrome (local) | Stroke fill alpha on the colour picker |
| 0112 | feat/raster-layer | Stage-sized raster layer and blend smudge |
| 0113 | feat/raster-layer | Hairline raster stroke without copying the plate each move |
| 0114 | feat/raster-layer | Raster flow: a dab deposits under the fill alpha |
| 0115 | feat/raster-layer | Raster pixels select as a crop and convert to a symbol |
| 0116 | feat/raster-layer | Raster spray, paper grain, and a pencil on its side |
| 0117 | feat/raster-layer | Spray pictures stamped and turned |
| 0118 | feat/raster-layer | Live raster ink stays smooth under the 2kool grid |
| 0119 | feat/raster-layer | Raster strokes keep pen samples; scene tiles draw the ink |
| 0120 | feat/raster-layer | A sound dropped on a timeline cell starts there, and Delete takes the clip off |
| 0121 | feat/raster-layer | Timeline show, lock, outline and onion toggles are a touch target |
| 0122 | feat/raster-layer | The file name sits on the waveform, and a close view shows the samples |
| 0123 | feat/raster-layer | Layer properties are in the Properties panel |
| 0124 | feat/raster-layer | Layer properties are square modules and dials |
| 0125 | feat/raster-layer | Raster tool properties use the same modules and dials |
