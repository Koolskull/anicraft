# ADR 0115: Raster pixels select as a crop and convert to a symbol

* Status: accepted (2026-11-17)
* Relates to: ADR 0112 (the stage-sized plate), ADR 0113 (the raster stroke)

## Context

A raster layer is one picture the size of the stage. The Selection tool skipped
it, because a hit on that picture would be a hit everywhere. Vector artwork on
the same layer could not be moved either: the keyframe edit that moves shapes
refused a pixel layer.

The ink still has a shape. The clear margin around it is not part of that
shape. A click or a marquee can name the tight box, and the commands that
already move, turn, copy, delete and convert a shape can do the same to that
box. The pen is unchanged. The vector brushes are unchanged.

## Decisions

1. **A click selects the tight box of every pixel whose alpha is not clear on
   that key.** A marquee selects the tight box of the opaque pixels inside it.
   One crop at a time: a layer, the keyframe it starts on, and the rectangle.
   It is not an element until Convert to Symbol. Clear pixels inside the box
   travel with it. A second doodle on the same plate is included until a
   marquee narrows the box.
2. **Move, rotate, scale, duplicate and delete go through the commands they
   already use.** One undo step. A drag lifts the crop once and resamples that
   original picture, so the frames of the drag do not blur each other. An
   integer move copies bytes. The origin is cleared only after the destination
   is known. A copy leaves the original.
3. **Convert to Symbol cuts the crop into a nearest-filtered bitmap inside the
   symbol.** The dialog's type is used. A crop on its own opens that dialog on
   Movie Clip. The instance is a new vector layer immediately above the plate,
   covering the same frames, with its registration at the centre of the
   selection. The picture does not move. Mixed artwork keeps the instance on
   the topmost selected element layer and puts the bitmap on a Pixels layer at
   the bottom of the symbol.
4. **The plate only copies, clears and blits rectangles.** The engine module
   is the only place that knows what a crop is. A later tool calls that
   module. It does not scan the plate itself. Filtering is one function, so a
   later filter replaces that function and nothing else. Aligning a crop is
   the next call: the same stage-space delta the elements get.

## Consequences

* Camera-view picking and Edit Multiple Frames do not move plates yet.
* Copy and paste of a crop are not in this slice. Duplicate is.
* Releasing the pointer while the crop is fully off the stage commits the
  hole. Undo brings the ink back.
* Hovering does not measure the ink. Alpha at one pixel is enough for the
  cursor.
