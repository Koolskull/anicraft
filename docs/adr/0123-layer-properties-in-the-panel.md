# ADR 0123: Layer properties live in the Properties panel

* Status: accepted (2026-11-18)
* Relates to: ADR 0061 (the layer property fields), ADR 0070 (mask type), ADR 0111 (opacity)

## Context

A layer's name, type, outline colour, outlines, onion skin, opacity and height were edited
in a window of their own, opened from the layer's icon, its menu, or Modify › Timeline.
The Properties panel, which already holds the frame and the selection, did not show them.
Selecting a layer also selects its artwork, so that panel showed the artwork.

## Decisions

1. **The Properties panel has a Layer section for the selected layer**, with the same
   fields the window had. A folder, the camera, an armature or a pixel layer still cannot
   change type. Masked still needs a mask above, and Guided a guide above. The camera
   still has no opacity, and the camera and folders still have no outlines or onion skin.
2. **Each change applies as it is made**, through `layer.set_properties`. Typing a name
   or dragging opacity is one undo step. There is no OK and no Cancel.
3. **Modify › Timeline › Layer Properties, the layer menu, and a double-click on the
   layer's icon show that section and select the name.** A click on the layer does not
   take the keyboard. Double-clicking the name on the timeline still renames it there.

## Consequences

* The layer's properties are in the same panel as the frame and the selection.
* There is no Layer Properties window.
* The command is unchanged.
