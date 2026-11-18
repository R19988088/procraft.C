# Krita brushes

A ProCraft plugin that is a brush engine (see the guide's "Brush engines of its own"): it reads
Krita's brush presets (`.kpp`) and bundles (`.bundle`) as they are imported, and draws their
brushes as Krita's pixel brush and colour smudge brush draw them, its MyPaint brushes (`.myb`) as
MyPaint's engine draws them in Krita, and its quick, tangent normal, sketch, spray, bristle,
hatching, curve, particle, grid, clone, deform, filter and shape brushes as far as dabs draw them.
It comes with the app, off.

## What it draws

- Tips: auto tips (circle or rectangle; default, soft-by-curve or Gaussian edge; ratio, fades,
  spikes, density and randomness, drawn afresh from a few variants), and pictures: PNGs, SVG
  pictures (their filled shapes drawn as Krita draws an SVG tip: grey over white, 1000 pixels
  across), GIMP
  brushes (`.gbr`) and pipes (`.gih`, a cell picked per dab by each dimension's selection: constant,
  incremental, random, pressure, angular, velocity, tilt), applied as the preset says: as a mask,
  stamped in their own colours, as a lightness map (its strength an option of its own) or as a
  gradient map (from the colour to the secondary colour), a colour picture's grey through its
  middle point, brightness and contrast; older presets' scale and adjustments read as Krita reads
  them.
- MyPaint brushes (Krita's MyPaint engine: a `.myb` in a bundle, or a preset keeping one): MyPaint's
  engine, ported (see below), handed the pen where Krita hands it (at the start, then every square
  root of the brush's diameter), its size as the slider says over the brush's own and its colour the
  app's; all its settings and their inputs' curves as MyPaint reads them, laid as Krita lays MyPaint's
  dabs (its round dab at each hardness, squashed and turned; an eraser as Krita's eraser mode). A
  brush that smudges asks the app what lies under each dab (the layer as the stroke has laid it so
  far, weighed as Krita weighs it) and mixes it in as MyPaint does, its dabs laid in place, as
  opaque as what it carries. Its studio has Krita's pages for MyPaint's settings, each its base
  value.
- Spacing: fixed or automatic, by the dab's size across and down its own frame (or the same every
  way), scaled by the spacing option.
- Options and their sensors: size, opacity, flow, rotation, ratio, spacing, scatter (both ways,
  along or across), mirror, softness, darken, mix, hue, saturation and value, the texture's strength;
  sensors pressure, the most pressure so far, X and Y tilt, tilt direction and elevation, speed,
  drawing angle (the way from the last dab, offset, locked at the start, or fanning dabs round a
  corner), rotation, distance, time, fade, fuzzy per dab and per stroke (perspective counts as 1);
  each through its curve (one they share, or each its own; an older preset's kept in its sensor or
  apart, as Krita reads them), several combined by multiply, add, max, min or difference. Dabs turn
  with the canvas on screen, as Krita's do. Each engine takes only the options Krita's takes.
- Painting: wash (dabs build towards their opacity, the stroke keeping near the opacity it has
  averaged) or build-up; the preset's blend mode; eraser presets erase. Sharpness: each dab's mask
  made hard at its threshold (what lies within its softness kept), dabs moved onto whole pixels as
  far as it says. Airbrush: dabs by time as well as by spacing (or by time alone), the app handing
  the pen again while it holds still. Precision: below its highest, the last dab drawn afresh is
  laid again while the next is near enough to it (its size, turn, place within a pixel and the rest,
  as near as the level says), as Krita keeps and lays its dabs.
- Texture: the pattern made into Krita's mask (brightness, contrast, invert, neutral point,
  cutoff), scaled, offset (or at random per stroke), combined with each dab by its texturing mode at
  the strength its sensors give, softly where it says.
- Masking brushes: a second brush along the same path, its size a share of the main brush's, its
  coverage combined with the main stroke's by its composite op.
- Colour smudge: each dab moves the paint under where the last lay to where it lies (smearing), or
  that paint made one colour first (dulling, from the share of it its smudge radius says, weighed by
  the tip in the newer engine), the colour mixed in at its colour rate (the gradient option moving
  it towards the secondary colour; an eraser's is clear, taking the paint away), laid as far as its
  smudge rate says (in place of what is there, or over it without smearing alpha), picked up from
  every layer in overlay mode, by the older engine's sums or the newer's; the first dab only marks where the
  next picks up from. With a lightness tip its paint is kept with the layer between strokes (the
  app's layer store), lit by a thickness each dab's tip lays: in its place, flattened as far as the
  paint thickness option is low, or over what is there as far as the smudge length and the thickness
  take it; the layer shows the paint at the lightness the thickness says, and any other change to
  the layer dries it.
- Other engines: the quick brush (hard round dabs of its diameter and spacing), the tangent normal
  brush (a pixel brush painting the pen's lean, or the stroke's way, or the pen's turn, as a normal
  map's colour, its channels swizzled as it says), the sketch brush (at each move a line from where
  it was, and lines towards the points it passed within its tip's reach, as likely as its density
  and their distance say, offset, magnetised, their colour and opacity at random if it says, each
  line a thin dab) and the spray brush (at each of its dabs its particles, as many as its count or
  as its coverage says, scattered by its distribution, squashed, turned and scaled, each an
  ellipse, a rectangle, a pixel or the brush's own tip, sized, turned and coloured at random as it
  says); the bristle brush (each pixel of its tip a bristle, kept as likely as its density says, as
  long as the tip is opaque there; at each move each bristle a line a pixel wide from where it was,
  or would have been, to where it is (covering each pixel as far as a line a pixel wide does, or the
  pixels nearest it when it isn't smoothed, half a pixel on as Krita lays them), the bristles spread by its scale and size, shaken by its
  randomness, sheared as the pen presses, turned by the rotation option, cut by the pressure's
  threshold, their ink running out pixel by pixel along its curve (soaked up from the colours
  under where the stroke starts if it says), taking their opacity and saturation, by its
  weights or not); the hatching brush (at each dab of its tip, the lines of a hatching anchored to
  the canvas at its origin, a separation apart, half as wide across as its thickness says (as
  Krita's own strokes come out), at its angle, crosshatched as
  its style and the crosshatching option say, its separation stepped by the separation option, on
  whole pixels unless it says, over the secondary colour if it says, kept inside the tip); the curve
  brush (a line from where the pen was, and a curve through the points it last passed, at the curves'
  opacity); the particle brush (its particles pulled towards the pen by their pull and gravity, a
  dot for each at each of its steps) and the grid brush (the cells of a grid anchored to the canvas
  within its diameter, divided, inset by their borders, each an ellipse, a box or a line, in the
  colour moved at random or picking up the colour under their middle). As in Krita, the quick brush
  takes only its size and spacing options; the sketch, spray and bristle brushes their size,
  opacity and rotation; the hatching brush its size and opacity besides its own; the curve brush its
  opacity besides its own; the particle and grid brushes none.
- The clone brush (each dab lays the layer's pixels from elsewhere, from the layer as it was before
  the stroke or from every layer, over what is there: from as far away as the stroke started from
  its source, the source moving with the brush, or from the source itself; a tap sets the source),
  the deform brush (each dab, an ellipse of its own diameter, squash, scale, turn and density, takes
  the pixels under it from elsewhere under it: grown or shrunk, swirled, moved with the pen, through
  a lens or scattered, by its amount or its counter, mixed from the pixels around or not) and the
  filter brush (each dab lays the layer as it was before the stroke blurred, by a Gaussian blur or a
  blur, or sharpened by an unsharp mask, under its tip); the shape brush (the outline the stroke
  makes, its corners smoothed or its places drawn on by its speed as it says, filled as the stroke
  ends by the even-odd or the winding rule).
- Krita 3's chalk and dyna brushes (Krita dropped them in version 4): the chalk brush's speckled round
  tip at every pixel of the path, its ink running out with the log of its dabs if it says; the dyna
  brush's weight on a spring (its mass and drag, in the canvas as 1 each way) pulled towards where
  the pen was, drawing circles growing with its speed, a ribbon as wide as it is slow (filled or its
  outline) or lines.

The app draws each brush's preview with the plugin, and QuickShape, layers of a plugin's own laying
their strokes again and painting on a 3D model go through it too; brush files the app writes keep
the plugin's data.

The Krita brush studio (the plugin's action, opened by the app's brush studio for its brushes) shows
a brush's Krita settings a page at a time, the pages its engine has: a pixel brush's tip, spacing,
size, opacity and flow, turn and squash, scatter and mirror, colour and texture; a colour smudge
brush's smudge length and mode, smudge radius, colour rate, gradient and paint thickness; a tangent
normal brush's tilt; a sketch brush's lines; a spray brush's area, particles, their dynamics and
colours; a quick brush's diameter and spacing; a bristle, hatching, curve, particle, grid, chalk or
dyna brush's own.
Each setting is named and ranged as Krita's own option widgets name and range it, and each option
comes with the sensor it follows and its curve.

The app keeps how each brush looks (its tips drawn, its texture's mask) with the plugin, so only a
brush's first stroke ever waits for them; after that the plugin only reads the brush's settings.
In the app, the app's utilities unpack bundles and decode pictures, much faster than the plugin's own
readers (which its tests use) run in it.

## Not yet

- A MyPaint brush that smudges is told what lies under its dabs a batch of the pen's samples late
  (until then it takes the newest it was told, and what it carries is worked out again once told);
  its colours mixed in RGB (MyPaint's older path, as Krita drives it: no spectral mixing, colorizing
  or posterizing); its dabs drawn from its tips at 64 hardnesses (Krita draws each at its own pixels,
  so a dab a few pixels across is a little softer and wider here). A `.myb` on its own comes in only
  where the app hands such files to brush engines; in a bundle, always.
- The clone brush's source is set by a tap
  (Krita's with a key held), and it doesn't heal; the filter brush draws blurs and the unsharp mask
  only (others lay nothing), with the app's blur matched to theirs; the deform brush's lens is as
  wide as each dab (Krita's as the brush's own diameter) and its spacing scales with its size; the
  shape brush fills its outline as the stroke ends (Krita as it goes) without its displacement, its
  edges whole pixels. The chalk brush picks among a few speckled tips (Krita speckles
  each dab afresh). The dyna brush's weight is pulled at each sample the app hands it (Krita at each
  of the tablet's events), so the more samples, the smaller and closer its circles.
- The bristle brush keeps at most 4,096 bristles, lays its lines over what is there whatever its
  compositing says, and takes its ink as it runs out in pieces within a 64th of it (Krita at
  each pixel). The hatching brush keeps its lines inside each dab's tip in
  pieces, each as opaque as the tip where it lies, halved down to a pixel where the tip's edge
  crosses them (Krita masks each pixel), so a thick line's end at the edge is cut square across it.
  The curve brush lays a move's line and curve over each
  other; the particle brush lays nothing more while the pen holds still; the grid brush taking the
  colour under its cells lays them in no other colour and without a background.
- The sketch brush draws each move's lines over each other (Krita draws them into one dab first,
  then lays that at the opacity option's), keeps the last 4,096 points it passed, and takes its tip
  as round for its reach (Krita's mode that isn't simple takes the tip's own shape); hard lines (its
  antialiasing off, as it is unless it says, and the bristle brush's) are the pixels nearest the
  line; the spray brush's particle pictures, its sampled colours and its
  background fill aren't drawn.
- The lightness and gradient texturing modes take the texture's grey alone (a pattern's
  see-through parts are taken as opaque).
- Colour smudge's kept paint is 8 bits a channel (Krita keeps its own at 16). Its overlay mode picks
  up from the image as last shown, so within one batch of the pen's moves it doesn't see what it
  has just laid (Krita keeps its own copy of the image up to date dab by dab).
- Pictures a preset takes from Krita's own resources without carrying them (nor its bundle): a
  tip of one is Krita's default tip, as Krita stands it in (round, 40 px across, the studio saying
  so); a masking brush or texture of one is left out.
- The 1-pixel brush's sharp lines, drawn as dabs.
- An SVG tip's strokes, text, pictures and gradients (its filled shapes only). Tips drawn from
  text, and the pixel brush's colour sources other than the plain
  colour (gradient, random, pattern). The hatching brush's mirror and texture.
- A drawing angle locked at the start takes the way the stroke first goes (Krita takes the way the
  pen moved just before it touched, which the app doesn't hand).
- Perspective counts as 1 (the app has no perspective assistant), as in Krita without one; a
  gradient map takes the gradient from the colour to the secondary colour (the app has no current
  gradient).
- Dabs are rotated, scaled and textured on the GPU from tips drawn once, not drawn afresh at each
  dab's own pixels as Krita draws them.

## MyPaint's brush engine

Its MyPaint brushes are drawn by a port of libmypaint's brush engine to Rust (`src/mypaint.rs`), from
libmypaint 1.6.1 (https://github.com/mypaint/libmypaint), under libmypaint's licence:

```text
libmypaint - The MyPaint Brush Library
Copyright (C) 2007-2011 Martin Renold <martinxyz@gmx.ch>
Copyright (C) 2008-2011 Martin Renold and contributors

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
```

## Where its knowledge comes from

Krita is free software under the GNU GPL. This plugin's code is its own: what it does was learned
from Krita's manual (the brush engines, options, sensors and texture pages) and from reading Krita's
sources for facts only (formulas, defaults and file layouts), none of it copied or translated:

- presets, sensors and curve options: how options read their settings, how sensors' values go
  through curves and combine, and each sensor's range;
- the auto tip's mask generators and how density and randomness thin a dab;
- spacing (fixed, automatic as a share of the square root of the size, anisotropic) and the walk
  from one dab to the next;
- the pixel brush's dab: size, rotation, ratio, scatter, mirror, opacity and flow (wash vs build-up,
  the averaged opacity of a washed stroke), darken, mix and HSV;
- GIMP brush, pipe and pattern files and how pipes pick cells;
- the texture's mask and its texturing modes; the masking brush and its composite ops;
- the colour smudge brush: its smudge rate, colour rate, smudge radius and gradient options, its
  dulling and smearing modes and how its older and newer engines combine them; with a lightness tip,
  the paint and the thickness it keeps with the layer, how each dab lays its thickness, and how the
  layer shows the paint lit by it;
- the quick, tangent normal, sketch, spray, bristle, hatching, curve, particle and grid brushes:
  their settings, and how they lay their dabs, lines, particles and cells.

The file formats (PNG, zip, base64, a small XML reader) are written here from their public
specifications.
