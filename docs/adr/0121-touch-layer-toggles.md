# ADR 0121: The timeline's layer toggles are a touch target

* Status: accepted (2026-11-18)
* Relates to: ADR 0008 (click targets), ADR 0061 (layer row toggles)

## Context

Show, lock, outline and onion skin sit at the right of each timeline row, and the
same three sit in the ruler above them. They were 24 px squares with a 15 px
glyph, which meets the pointer minimum and is small for a finger. Mute, when a
layer has a sound, is one of those squares.

## Decisions

1. **Each of those toggles is 44 × 44**, with 8 px between them, and the icon
   inside is drawn at 24 px. The row and the ruler grow to hold that square.
   Frame View › Short stays a short row, and its toggles shrink with it.
2. **The layer column's narrowest width keeps the four squares whole.** The
   default width also leaves room for a short layer name. A longer name is cut
   at the toggles.

## Consequences

* A finger can hit show, lock, outline, onion skin, or mute without catching
  the one beside it.
* The timeline shows fewer rows at the same height. Frame cells stay as they are.
