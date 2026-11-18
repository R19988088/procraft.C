# The stock brush library

ProCraft comes with 394 stock brushes in 37 sets, and with the brushes of its earlier versions in a
set of their own, Legacy, after them.

## How the stock brushes are made

- **Settings.** Each stock brush is ProCraft's own variant of a familiar default brush type. Its
  settings are the brush studio's controls, modelled on that type, and every value is then moved by
  1% to 3%, up or down, so that no two brushes share a setting by chance and none copies another's.
  How far each value moves is fixed by the brush's name, so the library is the same in every build.
  Switches and choices stay as they are: blend modes, rendering modes and the like. So do curves,
  values of none, and values at either end of their control, where moving would change what the
  control does (no jitter stays no jitter). The brushes therefore draw a little differently from
  the types they are modelled on, as far as those small changes take them.
- **Names.** Brushes with plain descriptive names (pencil grades, Technical Pen, Soft Airbrush,
  Charcoal Block and the like) keep them. Every brush with a distinctive name has a name of its own.
- **Shapes and grains.** Every shape and grain is drawn for ProCraft, in code: our own shapes and
  patterns, given the proportions, tones and coverage each brush needs. Each is drawn the first time
  a brush needs it, and brushes that use the same one share it.
- **Previews.** The library draws each brush's preview with ProCraft's own engine, from the brush's
  own settings.

No brush file, shape or grain picture, preview or text from elsewhere is in ProCraft.

The settings live in `crates/core/src/stock_brushes.rs`, written by `write_stock_brushes` in
`crates/core/src/stock_library.rs`, which builds the brushes from them.

## Your brushes and the Legacy set

Brushes are known by where they came from, so the update changes nothing about them:

- The brushes of earlier versions ship as they were, in the Legacy set.
- A brush you changed keeps its changes.
- Artworks that remember a brush still find it.
- The first time the stock sets join a library kept by an earlier version, its sets of earlier
  brushes go into the Legacy set, after the stock sets. This only applies to sets left as they
  shipped: under their own name, and holding nothing but brushes that shipped in them. Sets you
  renamed or added brushes to stay where they are. Your tools, Recent brushes and QuickMenu
  buttons follow the brushes they were on.
- Restore default brushes puts deleted stock and Legacy brushes back where they ship.
- The Legacy set's brushes are also in [`legacy-brushes/`](../legacy-brushes), a brush set file
  for each set they first came in, to import on their own.
