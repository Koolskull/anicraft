# ADR 0122: The file name sits on the waveform

* Status: accepted (2026-11-18)
* Relates to: ADR 0020 (sounds on keyframes), ADR 0120 (the waveform on the span)

## Context

A sound on a keyframe already draws its waveform across the filmstrip and down
the X-sheet. The file name was missing on the filmstrip, and on the X-sheet it
was drawn in the same ink as the wave, so it disappeared into the graphic. The
wave itself was one min and max per 256 samples, so a close view showed wide
steps instead of the shape.

## Decisions

1. **The file name is drawn on the wave**, on the filmstrip and on the X-sheet,
   in white with a 1 px black edge, after the wave, so it stays readable.
2. **A short column is the samples under it.** A column of a few thousand
   frames or fewer is the lowest and highest sample in that window. A longer
   column still uses the block peaks, and those blocks are finer than before.

## Consequences

* The name of the audio file can be read on top of the waveform.
* Zooming in on a clip shows the shape of the sound, on the filmstrip, the
  X-sheet, the Library preview and the envelope.
* Playback, scrubbing and export are unchanged.
