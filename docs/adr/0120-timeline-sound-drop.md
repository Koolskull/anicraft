# ADR 0120: A sound dropped on the timeline joins the Library and the keyframe

* Status: accepted (2026-11-18)
* Relates to: ADR 0020 (sounds on keyframes), ADR 0065 (drops on the timeline), ADR 0112 (the raster layer)

## Context

A keyframe already holds one library sound, with Event, Start, Stop or Stream,
and the timeline already draws its waveform when the span is long enough. Dropping
a WAV, Ogg or MP3 on a normal layer's frame already imports it and places that
reference (`file.drop` with `to: "timeline"`, one undo step, ADR 0065).

Two things stop that from being the drop an artist actually makes. winit 0.30
reports a dragged file with no position, and macOS sends no pointer moves during
the drag. If none arrives within a third of a second, the file is placed at the
centre of the stage instead of on the cell under the cursor. And the pixel layer
the picture is drawn on refuses every drop, including a sound, even though
`sound.place` would accept it.

A 16-bus mixer, per-instance effects, bus effects, a master limiter and a
WebAssembly effect graph are not part of this. Playback, scrubbing and export
stay as they are.

## Decisions

1. **A sound dropped on an unlocked layer's frame joins the Library and a
   keyframe that starts on that cell.** The file is imported as a new library
   sound. If that cell is already the first frame of a keyframe, the sound goes
   on it. If the cell is inside a span, or past the layer's end, the drop
   inserts a keyframe there first, keeping the artwork the cell showed, and the
   sound starts on the new keyframe. `sound.place` itself is unchanged: Place
   Sound and the Properties panel still attach a sound to whichever keyframe
   already covers the frame. The drop places with `extend`, so when the
   keyframe is the layer's last span it grows to cover the sound and the
   waveform can show. This includes a raster layer. A drawing or an image on a
   raster layer is still refused. A folder, the camera, an armature, and any
   locked layer, including a locked raster layer, stay refused, and nothing is
   imported. One sound per drop. One undo step. The same rules the highlight
   draws are the rules the command enforces.
2. **On macOS the cursor is read while the files hover.** AppKit's
   `mouseLocationOutsideOfEventStream`, converted into the content view, is the
   point. The view's origin is the top left, matching the pointer events egui
   already uses, divided by the interface zoom. The highlight follows that
   point, and the drop lands on the cell it names. The read lives in the pen
   crate, which is the one place the shell may call AppKit. winit and egui are
   not patched. When the cursor is inside no window, or this is not macOS, the
   existing wait remains, and a drop with no point still lands at the centre
   of the view.
3. **Clicking the clip selects it, and Delete takes the sound off.** A click on
   a keyframe that has a sound selects that keyframe's whole span, the same way
   a click selects a motion tween. Ctrl-click stays one cell. Shift still
   extends the selection. When the timeline has the keyboard and the selection
   is exactly that one span, Delete and Backspace run `sound.remove` for that
   keyframe. The artwork stays. The sound stays in the Library. A selection
   that is not the whole span does not remove the sound. The frame menu has
   Remove Sound when the keyframe has a sound. There is no separate clip to
   drag: the hollow mark at the end of a span is still the span's length handle.

## Consequences

* Dragging a sound onto the timeline cell under the cursor adds it to the
  Library and starts the clip on that cell, including on the pixel layer under
  the 2kool grid, as long as that layer is unlocked. A cell that is not already
  a keyframe's first frame gets a keyframe there, and the drawing stays.
  Dropping on a keyframe's first cell does not split it.
* Click the waveform and press Delete to take the sound off that clip. The
  drawing stays. The sound remains in the Library. Right-click the cell and
  choose Remove Sound to do the same.
* The X-sheet draws that same waveform down the span. Time runs down the page
  and the samples run across the column. The file name sits on the wave (ADR 0122).
* A picture dropped on a pixel layer still says to paint it with the Raster tool.
* A desktop drop whose cursor cannot be read can still land in the middle of
  the view. The browser already reports the drag position and is unchanged.
* No second audio model. Sync stays Event unless the sound's settings change
  it later. Monitor volume, scrubbing and export are unchanged.
