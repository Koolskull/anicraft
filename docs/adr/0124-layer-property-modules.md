# ADR 0124: Layer properties are square modules and dials

* Status: accepted (2026-11-18)
* Relates to: ADR 0008 (rectangular controls, round dials), ADR 0123 (the Layer section)

## Context

The Layer section listed its fields as a form. The five layer types were a vertical stack
of radio buttons, so the section was taller than the panel and the other fields sat below
the fold.

## Decisions

1. **The fields are square modules in a wrapping grid.** Show, lock, outlines, onion skin,
   type, mask type and outline colour each occupy one square. A narrow panel wraps them;
   a wide one fits more on a row. The name stays a text field above the grid.
2. **A setting with several modes is one cell.** The type cell shows the current type.
   Click it to choose Normal, Guide, Guided, Mask or Masked. Masked stays unavailable
   unless a mask is directly above, and Guided unless a motion guide is. On a mask layer,
   mask type (Shape, Alpha, Luminance) is one cell the same way. A folder, the camera, an
   armature or a pixel layer stays a single label.
3. **Opacity is a dial. Layer height is not in this grid.** Drag the opacity dial up to
   raise it. Shift drags more finely. Double-click returns it to 100 %. The dial keeps its
   size. Layer height only changes how tall the timeline row is, so it stays on the layer's
   right-click menu: 100 %, 200 % or 300 %. The command is still `layer.set_properties`.
4. **Small squares fill the dial's square.** Show, lock, outlines, onion skin, colour,
   type and mask type are half a dial's side, less the gap, and at least 28 px. Two of
   them stacked are one dial tall, so four of them fill one knob. A run of them is a grid
   beside the opacity dial, filled from the top left, two rows high. The row is one dial
   tall. Lining the small squares up on the dial's baseline, with empty space above them,
   is the wrong layout.

## Consequences

* The selected layer's properties fit in the Properties panel without a long scroll.
* Layer type, and a mask's mask type, each take one square. The modes open from that square.
* Those squares pack two high beside the opacity dial. A row of layer modules is one dial tall. The dial stays the larger size. Layer height is the timeline menu, not a second dial.
* There is still no separate Layer Properties window.
