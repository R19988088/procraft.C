# Legacy brushes

The brushes of ProCraft's earlier versions, before the stock library, as brush set files you can
import. There is a file for each set they first came in.

The app still comes with all of them, in the Legacy set after the stock sets. These files are an
extra. Use them to bring back one set on its own: into a library of your own, after deleting
brushes from the Legacy set, or to share a set with someone.

## The sets

204 brushes in 21 sets, in the order the Legacy set holds them:

| File | Brushes |
|---|---|
| `Sketching.brushset` | 12: Graphite, Soft Pencil, Technical Pencil, Lead Holder, Coloured Pencil, Sketch Pen, HB Pencil, 6B Pencil, Graphite Stick, Charcoal Pencil, Shading Pencil, Ballpoint |
| `Inking.brushset` | 12: Round Pen, Fine Liner, Brush Pen, Dip Pen, Marker, Dry Ink, Studio Ink, Gel Pen, Thick Liner, Felt Tip, Rough Ink, Ink Bleed |
| `Drawing.brushset` | 9: Chalk, Soft Pastel, Oil Pastel, Wax Crayon, Conté, Hard Pastel, Chalk Pencil, Pastel Pencil, Litho Crayon |
| `Painting.brushset` | 15: Round Brush, Flat Brush, Bristle Brush, Dry Brush, Gouache, Oil Paint, Palette Knife, Filbert, Fan Brush, Rigger, Sable, Impasto, Acrylic, Tempera, Scumble |
| `Watercolour.brushset` | 10: Wet Wash, Round Wash, Granulating Wash, Blot, Bleeding Wash, Flat Wash, Dry Wash, Salt, Splash, Mop Brush |
| `Calligraphy.brushset` | 9: Chisel Nib, Script Brush, Monoline, Ribbon, Pointed Nib, Broad Pen, Italic Nib, Felt Brush, Copperplate |
| `Airbrushing.brushset` | 9: Soft Airbrush, Hard Airbrush, Medium Airbrush, Fine Mist, Flat Airbrush, Grainy Airbrush, Stipple Airbrush, Feather Airbrush, Tight Airbrush |
| `Spray Paint.brushset` | 8: Spray Can, Fat Cap, Flick, Mist, Drips, Hard Spray, Splatter Can, Skinny Cap |
| `Charcoals.brushset` | 9: Charcoal Stick, Willow Charcoal, Compressed Charcoal, Vine Charcoal, Charcoal Dust, Charcoal Block, Burnt Stick, Smudged Charcoal, Soft Charcoal |
| `Textures.brushset` | 17: Speckles, Canvas Texture, Dot Screen, Crosshatch, Rough Wall, Grit, Stipple, Wood Grain, Brickwork, Fish Scales, Cell Wall, Marble, Pinstripe, Honeycomb, Leather, Sandpaper, Checkerboard |
| `Abstract.brushset` | 8: Hearts, Confetti, Triangles, Diamonds, Speed Lines, Scribbles, Dot Trail, Dashes |
| `Elements.brushset` | 8: Fire, Smoke Puff, Water Drops, Ripples, Lightning, Ice, Sea Foam, Fog |
| `Vintage.brushset` | 7: Worn Ink, Letterpress, Old Paper, Sepia Wash, Scratchy Pen, Rubber Stamp, Faded Print |
| `Luminance.brushset` | 10: Neon, Glimmer, Lens Dust, Flare, Halo, Light Leak, Aurora, Fairy Dust, Soft Glow, Overlay Light |
| `Industrial.brushset` | 8: Rusted, Cement, Grunge, Scratched Metal, Steel Wool, Rivets, Grime, Oil Stain |
| `Organic.brushset` | 8: Hair, Skin Pores, Freckles, Reptile, Coral, Lichen, Seeds, Veins |
| `Nature.brushset` | 13: Leaves, Grass, Clouds, Bark, Foliage, Pine, Twigs, Moss, Flowers, Fur, Pebbles, Feathers, Reeds |
| `Effects.brushset` | 11: Sparkle, Bokeh, Smoke, Snow, Light Trail, Glow, Stars, Bubbles, Embers, Rain, Snowflakes |
| `Touchups.brushset` | 7: Soft Blender, Grainy Blender, Retouch, Highlight, Shadow, Noise, Fine Detail |
| `Retro.brushset` | 7: Ben Day, Screen Tone, Comic Ink, Retro Stipple, Pop Dots, Retro Lines, Offset Print |
| `Pixel.brushset` | 7: Pixel, Dither, Hatching Pen, Pixel Checker, Pixel Spray, Pixel Round, Pixel Line |

## Importing a set

- **Android:** open the `.brushset` file with ProCraft, from a file manager or by sharing it to
  ProCraft. Or, in the brush library, tap **+**, then **Import from Files**, and pick the file.
- **Desktop:** in the brush library, click **+**, then **Import from Files**, and pick the file.

The set goes at the top of the library, under its own name. Importing a set again adds another
copy of it.

## Licence

These are ProCraft's own brushes, made by ProCraft contributors: their settings, shapes and grains
are drawn in code by the app's engine, and their previews are drawn by the app. They are under the
same licence as ProCraft, MIT OR Apache-2.0 (see [`LICENSE-MIT`](../LICENSE-MIT) and
[`LICENSE-APACHE`](../LICENSE-APACHE)).

## Making the files again

The files are written by a test that is skipped unless asked for, which needs a GPU for the
previews:

```
cargo test -p procraft-core --lib write_legacy_brushes -- --ignored
```

Each file comes out the same every time, apart from the previews, which another GPU may draw a
little differently. The test `the_legacy_brushes_folder_holds_each_set_as_the_legacy_set_has_it`
checks that the files still hold the brushes as the app has them.
