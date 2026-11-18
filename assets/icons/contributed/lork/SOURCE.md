# lork's icons

`adjustments.svg`, `selection.svg`, `brush.svg`, `transform.svg`, `smudge.svg`, `erase.svg` and
`layers.svg` are by lork (https://github.com/lork-fork), drawn from basic shapes in a vector editor
and given in https://github.com/artcraft-android/procraft/pull/178. They are kept as given;
`tools/icons/draw_icons.py` rounds them and places them in the bar. It also shortens the Transform
arrow's head and draws out its tail, draws the eraser's lower end whole (the file cuts it off at the
grid's edge), and sets the back sheet of Layers further back. Licence: MIT OR Apache-2.0, as the
rest of ProCraft.

## How the bar's icons were made

lork drew them; Josh, ProCraft's maintainer, chose each one and asked for the changes below, which
`tools/icons/draw_icons.py` makes from lork's files:

| Icon | Made from |
|---|---|
| Adjustments | lork's wand, as given |
| Selection | lork's dashed curve, as given |
| Transform | lork's arrow, its head shortened and its tail drawn out (Josh) |
| Paint | lork's brush, as given |
| Smudge | lork's pointing hand, as given |
| Erase | lork's eraser, its lower end drawn whole rather than cut off at the grid's edge (Josh) |
| Layers | lork's two sheets, the back one set further back and both filled solid (Josh) |

Actions keeps ProCraft's own spanner.
