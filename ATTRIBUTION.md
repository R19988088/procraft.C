# Attribution

Every non-code asset in ProCraft, with its author, source and licence. ProCraft's own code and
original assets are MIT OR Apache-2.0 (see [`LICENSE-MIT`](LICENSE-MIT),
[`LICENSE-APACHE`](LICENSE-APACHE) and [`NOTICE`](NOTICE)). When you add an asset, add a row here
in the same change; third-party assets must be openly licensed and keep their licence file next to
them.

## Bundled in the app

| Path | Title | Author | Source | Licence |
|---|---|---|---|---|
| `android/app/src/main/assets/fonts/Inter-{Regular,Medium,SemiBold,Bold}.ttf` | Inter 4.1 (interface font) | The Inter Project Authors | <https://github.com/rsms/inter/releases/tag/v4.1> (`Inter-4.1.zip`, `extras/ttf/`; `SOURCE.md` beside them) | SIL OFL 1.1, `Inter-OFL.txt` |
| `android/app/src/main/java/io/github/artcraftandroid/procraft/ui/LucideIcons.kt` (the interface icons as Lucide draws them, kept beside ours) | Lucide icons, lucide-static 1.63.0 | Lucide Icons and Contributors (those derived from Feather: Cole Bemis) | <https://github.com/lucide-icons/lucide>; each icon's paths moved and scaled into the frame it is shown in, at the size shown | ISC (Feather-derived also MIT), [`LICENSE-lucide.txt`](android/app/src/main/assets/icons/LICENSE-lucide.txt) |
| `assets/icons/procraft/*.svg`, and every icon in `android/app/src/main/java/io/github/artcraftandroid/procraft/ui/Icons.kt` (`PcIcons`: the bar, toolbars, panels and menus, the blend mode glyphs and the 48 brush set icons) but the seven below | ProCraft's own interface icons | ProCraft contributors | Original work, drawn from the idea to [`docs/icons/STYLE_GUIDE.md`](docs/icons/STYLE_GUIDE.md) by `tools/icons/draw_icons.py`; placed by the measured slots in `assets/icons/slots.tsv` | MIT OR Apache-2.0 |
| `assets/icons/contributed/lork/{adjustments,selection,brush,transform,smudge,erase,layers}.svg`, and from them `assets/icons/procraft/{adjustments,selection,brush,transform,smudge,erase,layers}.svg` and the bar's Adjustments, Selection, Transform, Paint, Smudge, Erase and Layers icons (`PcIcons.Adjustments`, `PcIcons.Selection`, `PcIcons.Transform`, `PcIcons.Brush`, `PcIcons.Smudge`, `PcIcons.Erase`, `PcIcons.Layers`) | The bar's Adjustments, Selection, Transform, Paint, Smudge, Erase and Layers icons | lork ([@lork-fork](https://github.com/lork-fork)); the changes to Transform, Erase and Layers chosen by Josh, ProCraft's maintainer | Original work, drawn from basic shapes in a vector editor and given in [#178](https://github.com/artcraft-android/procraft/pull/178); rounded and placed in their slots by `tools/icons/draw_icons.py`, which also shortens the Transform arrow's head and draws out its tail, draws the eraser's lower end whole and sets the back sheet of Layers further back, both sheets filled; how each was made is in [`assets/icons/contributed/lork/SOURCE.md`](assets/icons/contributed/lork/SOURCE.md) | MIT OR Apache-2.0 |
| `android/app/src/main/assets/environments/*.hdr` | 3D view light panoramas (11), halved to 512 x 256 and scaled by `tools/env/make_env.py` | Sergej Majboroda, Greg Zaal, Andreas Mischok, Rico Cilliers (per file in the folder's `README.md`) | <https://polyhaven.com> | CC0 1.0 |
| `assets/app-icon/procraft-{1024,512}.png`, `android/app/src/main/res/drawable-*dpi/ic_launcher_{foreground,monochrome}.png`, `android/app/src/main/res/drawable-nodpi/procraft_mark.png`, `android/app/src/main/res/mipmap-anydpi/ic_launcher.xml` | App icon: a paintbrush in a warm-to-cool blend on black | ProCraft contributors | Original work, drawn in code by `tools/app-icon/make_icon.py`; see [`assets/app-icon/README.md`](assets/app-icon/README.md) | MIT OR Apache-2.0 |
| `android/app/src/main/res/drawable/widget_back.xml`, `android/app/src/main/res/drawable-v31/widget_back.xml` | Recent-artwork widget background | ProCraft contributors | Original work | MIT OR Apache-2.0 |
| `docs/images/{canvas,layers,brushes,studio,colour,actions,adjust,transform}.jpg`, `docs/images/demo.webp`, `docs/images/demo.mp4` | The README's screenshots and demo: ProCraft on a tablet, painting *Dusk* | ProCraft contributors | Original work: the painting made in the app by `tools/showcase/paint_dusk2.py`, captured from the screen | MIT OR Apache-2.0 |
| The Legacy set's brushes, and every brush shape and grain (the stock brushes' too) | Made in code at run time (`crates/engine`, the stock brushes' shapes and grains by `crates/core/src/brush_files.rs`) | ProCraft contributors | Original work | MIT OR Apache-2.0 |
| `legacy-brushes/*.brushset` (not in the app: files to import) | The Legacy set's brushes as brush set files, one for each set they came in | ProCraft contributors | Original work: the brushes above, written out by `write_legacy_brushes` in `crates/core/src/stock_library.rs`, their shapes and grains drawn in code and their previews by the app; see [`legacy-brushes/README.md`](legacy-brushes/README.md) | MIT OR Apache-2.0 |
| `crates/core/src/stock_brushes.rs` (built into brushes by `crates/core/src/stock_library.rs`) | The stock brush library: 394 brushes in 37 sets | ProCraft contributors | ProCraft's own variants of familiar default brush types: each brush's settings modelled on its type, every value then moved by 1% to 3% so that each brush differs; distinctive names replaced by names of our own; shapes and grains drawn for ProCraft (above), previews drawn by the app. No brush file, picture, preview or text from elsewhere. How they are made: [`docs/stock-brushes.md`](docs/stock-brushes.md) | MIT OR Apache-2.0 |
| `docs/images/discord-badge.svg` | The README's "Join us on Discord" badge | ProCraft contributors; the Discord mark from Simple Icons | Drawn to match the download badges beside it; the mark's path from <https://simpleicons.org> (simple-icons 13.21.0, `icons/discord.svg`) | MIT OR Apache-2.0; the mark CC0 1.0 (Discord's trademark, shown to link to the project's server) |
| `dxcompiler.dll`, `dxil.dll` (beside `ProCraft.exe` in the Windows installer and portable zip; not in the repository) | DirectX Shader Compiler 1.9.2609.5 (Direct3D 12's shader compiler; x64 and arm64) | Microsoft Corporation | <https://github.com/microsoft/DirectXShaderCompiler/releases/tag/v1.9.2609> (`dxc_2026_09_29.zip`, `bin/<arch>/`), downloaded and checked against its SHA-256 by [`packaging/windows/package.ps1`](packaging/windows/package.ps1) | `dxcompiler.dll`: University of Illinois/NCSA Open Source License, [`LICENSE-LLVM.txt`](packaging/windows/dxc/LICENSE-LLVM.txt), and MIT, [`LICENCE-MIT.txt`](packaging/windows/dxc/LICENCE-MIT.txt); `dxil.dll`: Microsoft Software License Terms (distributable code), the release's `LICENSE-MS.txt`; all three as the release gives them, beside the DLLs as `DXC-*.txt` |

### Lucide icons in `LucideIcons.kt`

The same icons, each with the Lucide icon it is:

- `FullScreen`: `maximize-2`
- `Actions`: `wrench`
- `Adjustments`: `wand-sparkles`
- `Selection`: `lasso`
- `Transform`: `navigation-2`
- `Brush`: `brush`
- `Smudge`: `pointer`
- `Erase`: `eraser`
- `Layers`: `layers-2`
- `Undo`: `undo-2`
- `Redo`: `redo-2`
- `TransformReset`: `rotate-ccw`
- `SelectCopyPaste`: `copy`
- `StudioShape`: `shapes`
- `GestureCopyPaste`: `clipboard-copy`
- `MenuNewBrush`: `brush`
- `MenuNewSet`: `folder-plus`
- `MenuShare`: `share`
- `MenuChevron`: `chevron-right`
- `PieUndo`: `undo-2`
- `PieReset`: `rotate-ccw`
- `ShapeEquilateral`: `triangle`
- `ShapeSquare`: `square`
- `CopyCut`: `scissors`
- `CopyDuplicate`: `copy-plus`
- `DoneTick`: `check`
- `Add`: `plus`
- `InfoAbout`: `info`
- `InfoDimensions`: `ruler-dimension-line`
- `InfoLayers`: `layers-2`
- `InfoProfile`: `palette`
- `InfoVideo`: `video`
- `InfoStatistics`: `chart-column`
- `Keyboard`: `keyboard`
- `TextCut`: `scissors`
- `TextCopy`: `copy`
- `TextPaste`: `clipboard-paste`
- `AlignLeft`: `text-align-start`
- `AlignCenter`: `text-align-center`
- `AlignRight`: `text-align-end`
- `AlignJustify`: `text-align-justify`
- `TextUnderline`: `underline`
- `TextOutline`: `type-outline`
- `TextVertical`: `type` and `arrow-down`, side by side
- `CopyOne`: `copy`
- `CopyAll`: `copy-check`
- `CutPaste`: `clipboard-x`
- `PasteDown`: `clipboard-paste`
- `GestureAssisted`: `ruler`
- `GestureEyedropper`: `pipette`
- `GestureQuickShape`: `shapes`
- `GestureQuickMenu`: `chart-pie`
- `GestureFullScreen`: `maximize-2`
- `GestureClearLayer`: `eraser`
- `GestureLayerSelect`: `layers-2`
- `GestureHover`: `pen-line`
- `GestureGeneral`: `settings`
- `ModeDisc`: `disc`
- `ModeClassic`: `square-dot`
- `ModeHarmony`: `orbit`
- `ModeValue`: `sliders-horizontal`
- `ModePalettes`: `palette`
- `Sweep`: `brush-cleaning`
- `CardCompact`: `rows-3`
- `CardStack`: `gallery-vertical-end`
- `TabAdd`: `plus`
- `TabCanvas`: `frame`
- `TabShare`: `share`
- `TabVideo`: `video`
- `TabPrefs`: `toggle-right`
- `TabHelp`: `circle-question-mark`
- `FolderFill`: `folder`
- `Freeform`: `move-diagonal-2`
- `Uniform`: `scaling`
- `Distort`: `vector-square`
- `Warp`: `spline-pointer`
- `Snapping`: `magnet`
- `FlipHorizontal`: `triangles-centerline-dashed-vertical`
- `FlipVertical`: `triangles-centerline-dashed-horizontal`
- `Rotate45`: `rotate-cw`
- `FitToCanvas`: `fullscreen`
- `Interpolation`: `proportions`
- `AdjustHsb`: `sun-medium`
- `AdjustBlur`: `droplet`
- `AdjustBalance`: `scale`
- `AdjustCurves`: `spline`
- `AdjustGradient`: `blend`
- `AdjustMotion`: `wind`
- `AdjustPerspective`: `box`
- `AdjustNoise`: `sparkles`
- `StudioGrain`: `grip`
- `AdjustSharpen`: `diamond`
- `AdjustBloom`: `sun`
- `AdjustGlitch`: `zap`
- `AdjustHalftone`: `grid-3x3`
- `AdjustChromatic`: `aperture`
- `AdjustLiquify`: `droplets`
- `AdjustClone`: `stamp`
- `Disclose`: `chevron-down`
- `NoiseClouds`: `cloud`
- `NoiseBillows`: `cloud-fog`
- `NoiseRidges`: `mountain`
- `HalftoneFull`: `circle-dot`
- `HalftoneScreen`: `grip`
- `HalftoneNews`: `newspaper`
- `ChromaPerspective`: `focus`
- `ChromaDisplace`: `move-horizontal`
- `GuideGrid`: `grid-2x2`
- `GuideIsometric`: `axis-3d`
- `GuideSymmetry`: `square-split-horizontal`
- `ShapeLine`: `slash`
- `ShapePolyline`: `trending-up`
- `ShapeEllipse`: `ellipse`
- `ShapeCircle`: `circle`
- `ShapeTriangle`: `triangle`
- `ShapeQuad`: `diamond`
- `ShapeRect`: `rectangle-horizontal`
- `StudioStroke`: `spline`
- `StudioStabilize`: `activity`
- `StudioTaper`: `triangle-right`
- `StudioRendering`: `paintbrush`
- `StudioWet`: `droplets`
- `StudioColor`: `palette`
- `StudioDynamics`: `gauge`
- `StudioStylus`: `pen-line`
- `StudioProperties`: `settings-2`
- `StudioMaterials`: `box`
- `StudioPreview`: `eye`
- `StudioAbout`: `info`
- `GrainMoving`: `waves-horizontal`
- `GrainTexturized`: `grip-horizontal`
- `GlitchArtifact`: `square-dashed`
- `GlitchWave`: `audio-waveform`
- `GlitchSignal`: `signal`
- `GlitchDiverge`: `split`
- `BrushPaintedOn`: `paint-roller`
- `ModeLayer`: `layers-2`
- `ModePencil`: `pencil`
- `AdjustKnob`: `circle-gauge`
- `Apply`: `check`
- `Preview`: `eye`
- `CancelAdjust`: `ban`
- `Automatic`: `wand-sparkles`
- `Freehand`: `lasso`
- `RectangleShape`: `square-dashed`
- `EllipseShape`: `circle-dashed`
- `SelectAdd`: `plus`
- `SelectRemove`: `minus`
- `Invert`: `contrast`
- `Feather`: `feather`
- `SaveLoad`: `save`
- `ColorFill`: `paint-bucket`
- `ClearSelection`: `brush-cleaning`
- `SwipeShare`: `share`
- `SwipeDuplicate`: `copy-plus`
- `BlendGlyphs["Multiply"]`: `circle-x`
- `BlendGlyphs["Darken"]`: `moon`
- `BlendGlyphs["Shade"]`: `sunset`
- `BlendGlyphs["Color Burn"]`: `flame`
- `BlendGlyphs["Linear Burn"]`: `flame-kindling`
- `BlendGlyphs["Darker Color"]`: `moon-star`
- `BlendGlyphs["Normal"]`: `circle`
- `BlendGlyphs["Lighten"]`: `sun`
- `BlendGlyphs["Screen"]`: `sun-dim`
- `BlendGlyphs["Color Dodge"]`: `sparkle`
- `BlendGlyphs["Add"]`: `circle-plus`
- `BlendGlyphs["Lighter Color"]`: `sun-medium`
- `BlendGlyphs["Overlay"]`: `blend`
- `BlendGlyphs["Soft Light"]`: `lamp`
- `BlendGlyphs["Hard Light"]`: `lightbulb`
- `BlendGlyphs["Vivid Light"]`: `zap`
- `BlendGlyphs["Linear Light"]`: `flashlight`
- `BlendGlyphs["Pin Light"]`: `pin`
- `BlendGlyphs["Hard Mix"]`: `grid-2x2`
- `BlendGlyphs["Difference"]`: `circle-minus`
- `BlendGlyphs["Exclusion"]`: `circle-slash`
- `BlendGlyphs["Subtract"]`: `minus`
- `BlendGlyphs["Divide"]`: `divide`
- `BlendGlyphs["Hue"]`: `palette`
- `BlendGlyphs["Saturation"]`: `droplet`
- `BlendGlyphs["Color"]`: `paintbrush`
- `BlendGlyphs["Luminosity"]`: `sun-moon`
- `BlendGlyphs["Behind"]`: `send-to-back`
- `BlendGlyphs["Height"]`: `mountain`
- `BlendGlyphs["Linear Height"]`: `mountain-snow`
- `SetIcons[0]`: `brush`
- `SetIcons[1]`: `presentation`
- `SetIcons[2]`: `flower`
- `SetIcons[3]`: `bell`
- `SetIcons[4]`: `bookmark`
- `SetIcons[5]`: `paintbrush`
- `SetIcons[6]`: `sprout`
- `SetIcons[7]`: `badge-check`
- `SetIcons[8]`: `cloud`
- `SetIcons[9]`: `gamepad-2`
- `SetIcons[10]`: `target`
- `SetIcons[11]`: `gem`
- `SetIcons[12]`: `dice-5`
- `SetIcons[13]`: `droplet`
- `SetIcons[14]`: `paintbrush-vertical`
- `SetIcons[15]`: `film`
- `SetIcons[16]`: `fish`
- `SetIcons[17]`: `flag`
- `SetIcons[18]`: `flame`
- `SetIcons[19]`: `zap`
- `SetIcons[20]`: `clover`
- `SetIcons[21]`: `hammer`
- `SetIcons[22]`: `headphones`
- `SetIcons[23]`: `heart`
- `SetIcons[24]`: `house`
- `SetIcons[25]`: `moon`
- `SetIcons[26]`: `list-music`
- `SetIcons[27]`: `pen`
- `SetIcons[28]`: `pin`
- `SetIcons[29]`: `plane`
- `SetIcons[30]`: `puzzle`
- `SetIcons[31]`: `trophy`
- `SetIcons[32]`: `sparkle`
- `SetIcons[33]`: `star`
- `SetIcons[34]`: `sun`
- `SetIcons[35]`: `feather`
- `SetIcons[36]`: `pencil`
- `SetIcons[37]`: `ticket`
- `SetIcons[38]`: `tree-pine`
- `SetIcons[39]`: `umbrella`
- `SetIcons[40]`: `wand`
- `SetIcons[41]`: `hat-glasses`
- `SetIcons[42]`: `pen-tool`
- `SetIcons[43]`: `droplets`
- `SetIcons[44]`: `pencil-line`
- `SetIcons[45]`: `spray-can`
- `SetIcons[46]`: `circle-dot-dashed`
- `SetIcons[47]`: `grip`

Our own among the rest: `ShapeArc`, and the outlines and shapes not listed above
(chevrons, ticks, the keypad, the brush swipe's bin, the Liquify tools, the brush set glyphs...).

### Code from elsewhere

| Path | What | Author | Source | Licence |
|---|---|---|---|---|
| `plugins/krita-brushes/src/mypaint.rs` (in the bundled Krita brushes plugin) | MyPaint's brush engine (its settings, inputs, states and dabs), ported to Rust | Martin Renold and the libmypaint contributors | libmypaint 1.6.1, <https://github.com/mypaint/libmypaint> (`mypaint-brush.c`, `mypaint-mapping.c`, `helpers.c`, `brushsettings.json`) | ISC; the notice is in the file, in the plugin's `README.md` and in [`NOTICE`](NOTICE) |

## Tests

| Path | Title | Author | Source | Licence |
|---|---|---|---|---|
| `crates/engine/tests/models/*.usd{a,c,z}` | Test cube, moved cube and grid | ProCraft contributors (made by `tools/usd/make_usd.py`) | Original work, see the folder's `README.md` | MIT OR Apache-2.0 |

## Libraries

The libraries the app is built with, and their licences, are listed in
[`android/app/src/main/assets/licenses.txt`](android/app/src/main/assets/licenses.txt) (made by `tools/licenses.py`) and shown in the
app.
