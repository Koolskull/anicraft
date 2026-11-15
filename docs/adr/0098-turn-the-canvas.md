# ADR 0098: Turn the canvas

* Status: proposed
* Relates to: the stage view (ADR 0007), rulers (ADR 0076)

## Context

Drawing a long stroke with the wrist parked in one place is easier when the paper can turn under
the hand. The turn is Shift+Space and a drag. Space alone still slides the paper. Shift+R stands
the paper back up. The drawing itself does not turn. A later export, a tween and a symbol still
see the artwork upright.

The pan is a slide in screen pixels: the hand tool, the space bar and the wheel move the picture
by the same distance the pointer moved, whichever way the paper is turned. The turn is around the
middle of the window. Turning must not jump the point that sits under that middle, and standing
the view back up must not jump it either, and must not change the zoom.

## Decision

1. **The turn is view state.** `StageView::rotation` is clockwise degrees. It is not a document
   command, it is not the camera's rotation, and it is not stored in the file.
2. **Pan stays outside the turn.** A hand drag adds screen pixels to the pan. A turn orbits that
   pan by the same angle, so the stage point under the canvas centre stays. Shift+R orbits the pan
   back and sets the angle to 0. Fit in Window stands the view upright and clears the pan, as a
   fitted view always has.
3. **Shift+Space and a drag** starts the turn, with the pen or the mouse, whichever tool is in
   hand. It does not start a stroke, a zoom click, a bucket or a pen knot, and it does not pan.
   Space alone still pans. The middle button still pans, and it wins over the turn. The Hand
   tool's drag pans, unless Shift is held with Space, which turns instead. Command+Shift still
   zooms and does not turn. A text field keeps the keys. Shift+R is taken before the Rectangle
   tool's R.
4. **Command+Shift and a drag** zooms about the middle of the window. Dragging up zooms in and
   dragging down zooms out, measured from the press, so returning to the press returns the zoom.
   The stage point under that middle stays. It does not start a stroke. The wheel still zooms
   around the pointer.
5. **Upright drawing stays as it was.** At 0° the screen mapping, the tile images, the selection
   boxes, the grid and the guides are the ones the stage already drew. A turn draws tiles and
   boxes as the same corners after the turn. Ruler ticks stay on the window edges.

## Consequences

* The zoom badge shows the angle while the view is turned.
* Ruler numbers do not follow the turned stage axes. The grid and the guides do.
* A rounded rectangle's preview is a sharp quad while the view is turned. The shape that lands
  still has its corner radius.
* Free-transform handles sit on the turned box. Their grab boxes stay aligned to the screen.
