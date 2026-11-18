# Writing a plugin

A plugin is a `.procraft-plugin` file. Someone imports it on the Plugins page, and it has a card there
with Turn on, Turn off and Remove. With none on, the app is as it always was.

Making one doesn't change the app or its code here, and needs no access to this repository: you
build the file on your own computer (see [Building it](#building-it)) and share the file itself.

For now the samples in [`plugins/`](../plugins) also come inside the app, packed: see
[The plugins that come with the app](#the-plugins-that-come-with-the-app).

A plugin holds one or both of these:

- **Code**, which changes how the app behaves. It's a WebAssembly module, run under limits by an
  interpreter, that the app asks at named places called [hooks](#hooks). Code can also add buttons
  that change the artwork ([actions](#actions)), show a [panel](#panels) of its own, draw the colour
  panel's Classic picker ([`picker`](#picker)), make a kind of artwork of its own
  ([artworks of its own](#artworks-of-its-own)), or add a kind of layer whose strokes it keeps
  ([layers of its own](#layers-of-its-own)), or be a brush engine for brush files of other apps
  ([brush engines of its own](#brush-engines-of-its-own)).
- **A [filter](#filters)**, or [several](#several-in-one-plugin), each a new adjustment. A filter is
  a function the GPU runs over a layer.

The samples are in [`plugins/`](../plugins). `touch-offset` is the smallest with code and `vector-layers`
the largest. `effects` lists three filters, each under a switch of its own. `layer-fills` has an
action beside a filter, `colour-triangle` is a picker, `vector-layers` adds layers of its own, and
`keyboard-shortcuts` rebinds keys from a panel of its own and keeps what it binds. `interface`,
`layer-tools` and `colordrop-tools` each answer several hooks, one for each of their switches, and
`colour-triangle`'s picture draws its hue ring as its switch says.

## What you write

A folder under `plugins/` holding a `craft-extension.json`, and for each thing it holds:

- For code, a Rust crate. Any language that builds to a plain WebAssembly module will do, but
  `tools/pack_plugin.py` builds Rust.
- For a filter, a `filter.wgsl`; for several, a `filters/<key>.wgsl` for each.

`craft-extension.json` describes it. `tools/pack_plugin.py` lists the code in it for you.

```json
{
  "schema": 1,
  "id": "org.example.nudge",
  "name": "Nudge",
  "version": "1.0.0",
  "holds": ["hooks"],
  "publisher": { "name": "Example" },
  "description": "One sentence on what it does.",
  "compat": { "procraft": { "interface": 1 } },
  "permissions": [],
  "hooks": ["position"],
  "controls": [
    { "key": "across", "type": "slider", "label": "Across", "least": -100, "most": 100, "step": 1,
      "starts": 0, "unit": " px" }
  ]
}
```

`holds` says what's in the file, and the app checks it: `"hooks"` for code, `"filter"` for a filter,
or both. The file has to hold exactly that. The import screen tells people what each one lets the
plugin do.

`hooks` lists the hooks the code uses, and only a plugin that holds `"hooks"` may have it. Code
needs at least one hook, one action (see [Actions](#actions)), one kind of artwork (see
[Artworks of its own](#artworks-of-its-own)), one kind of layer (see
[Layers of its own](#layers-of-its-own)) or one brush engine (see
[Brush engines of its own](#brush-engines-of-its-own)). `interface` is 1 here; actions, permissions, panels,
the `picker` hook and kinds of artwork need 2, kinds of layer 3, brush engines 4, several filters
5, and more than eight controls, or a picker's picture that reads its controls, 6.

`id` must have at least two dotted parts, and can't start with `procraft.` or end with `.stopped`.
`permissions` lists what code may reach when someone taps one of its actions. Leave it empty unless
the plugin has actions. Only code may ask, and a permission this app doesn't give is refused.

A plugin that only holds a filter starts on, as that changes nothing already in the app. Any other
starts off, until someone taps Turn on. One the app adds by itself starts off, whatever it holds.

## Controls

A plugin can have up to eight, each a `slider` or a `switch`, or sixteen if it holds code written to
interface 6 and no filter; a plugin with code can have up to eight `file` controls besides.

| Field | Means |
|---|---|
| `key`, `label` | Its name in the file, and the words beside it |
| `starts` | Its value at first |
| `least`, `most`, `step` | A slider's range and steps |
| `unit` | Written after the value, such as `" px"` |
| `shown_times` | The value is shown multiplied by this: 100 shows .5 as 50 |
| `at_least` | Shown in place of the number at the slider's lowest, such as `"Off"` |
| `names` | A name for each of a slider's steps from its lowest, shown in place of the number |
| `live` | The value goes in as the slider moves, rather than as it lets go |
| `accepts` | What a `file` control takes: `"sound"`, a recording (the app reads it, keeps a minute of it at most with the plugin, and drops it with the plugin) |
| `under` | The `key` of a switch it comes under: on the card it shows after that switch, while the switch is on (its value stays as it was while it's hidden). That switch is at the top of the list, or comes under one there (interface 6): two deep at most |

A plugin that does several things can have a switch for each, at the top of the list, with that
thing's sliders `under` it. Turning the plugin on switches none of them on: each is the user's to
turn on, and the code answers for each only while its switch says so (it is told the switch as any
control). An action, and a filter of several, can come `under` a switch too.

A `file` control shows Import, or the name of the file picked and Clear. The plugin is told 1 while a
file is picked, 0 while none is, and is handed the file itself if it takes files (see below); what
the file is for is up to the plugin.

### Rows in the brush studio

A plugin with code can add up to eight rows to the brush studio's Properties page, each a choice of 2
to 16 (each 24 characters at most), kept with each brush:

```json
"brush_controls": [
  { "key": "sound", "label": "Sound", "choices": ["Default", "Eraser", "Pencil"] }
]
```

A brush starts at the first choice. The row shows under its `label` while the plugin is on; a brush
keeps its choice under `plugin:<id>:<key>`, so it stays with the brush when the plugin is off.

### What the code is told

Besides its hooks, code may export any of these, and is told:

- The current brush, before a sound is asked for when it changed: its name and its set's (UTF-8) into
  `procraft_text_buffer(len: i32) -> i32`, which answers where `len` bytes go, then
  `procraft_brush(name_len: i32, set_len: i32)`; and its rows' choices through
  `procraft_brush_control(index: i32, value: f32)`, `index` their place in `brush_controls`.
- The file picked for a `file` control, when it starts and whenever one is picked or let go: into
  `procraft_file_buffer(index: i32, len: i32) -> i32` (where `len` bytes go), then
  `procraft_file(index: i32, len: i32)`, `index` the control's place in `controls` and `len` 0 when
  it is let go. A sound comes as a WAV file of 32-bit floats, one channel, a minute at most. It's a
  long call, on fuel of its own.

A `key` is lowercase letters, digits and `_`, starting with a letter, and can't be `on`. A filter
reads it as a field, as does a picker's picture written to interface 6, so there it also has to be a
word WGSL allows, and not a keyword such as `loop`.

Where they show depends on what the plugin holds:

- With code, they're on the plugin's card while it's on. They're told to the code.
- With only a filter, they're on the filter's panel, and start at `starts` each time it opens.
- With both, each place has its own values: the card's go to the code, and the filter's panel still
  starts at `starts`.
- With [several filters](#several-in-one-plugin), they're on the card, and each filter's panel opens
  at the values its own have there.

## Hooks

A hook is a place in the app that asks a plugin something. At a hook, all the code can do is hand
back an answer, and the app checks that answer before using it. Changing the artwork is for
[actions](#actions), and for the two hooks that tell a plugin something happened,
[`stroke_kept` and `layer_added`](#stroke_kept-and-layer_added), which act as actions do.

### What code can reach

At a hook, nothing but the numbers the hook hands it (the two that tell of an event: the functions
its permissions give, as an action). A module imports only the app's functions its
`permissions` give (see [Actions](#actions)) and the [utilities](#utilities), so it has no files, no
network and no clock. A module that imports anything else is refused. A function it exports for a
hook it didn't list is never called.

### Utilities

Any module may import these from `"procraft"`, without a permission, and call them at a hook as in
an action (interface 4). They do work on bytes the module hands over, many times faster than the
module's own code would in the app:

| Function | Does |
|---|---|
| `inflate(from: i32, len: i32, kind: i32) -> i32` | Unpacks the `len` bytes at `from`, raw deflate (`kind` 0) or zlib (1): their length unpacked, -1 if they don't unpack or unpack to more than 256 MiB |
| `decode_image(from: i32, len: i32) -> i64` | Decodes the PNG or JPEG at `from`: its width, and its height times 2^32; 0 if it isn't one |
| `read_answer(to: i32) -> i32` | Writes what the last of the two made at `to` (unpacked bytes, or the picture as RGBA, 8 bits a channel, not premultiplied, rows from the top): 1 if it did |

What one makes waits for `read_answer` until the next call to either, so a module makes room for it
first, by its length or size.

### The crate

The plugin kit in `plugins/kit` writes the calls for you, so a plugin is ordinary Rust: its controls
kept as the app tells them, and a macro for each hook that hands pictures, sounds, text and files back
and forth.

```toml
[dependencies]
kit = { package = "procraft-plugin-kit", path = "../kit" }
```

```rust
/// The controls, by their place in the manifest, as they start.
static CONTROLS: kit::Controls<1> = kit::Controls::new([0.0]);
kit::controls!(CONTROLS);

// where a sample lands: moved across by the control
kit::position!(|x, y| (x + CONTROLS.get(0), y));
```

The macros: `controls!` (with a closure, also told each control as it changes), `yes!` for hooks that
only ask yes or no (`kit::yes!(procraft_pin_panels)`), `answer!` for a whole number, `position!`, `stroke_sound!` and
`sound_prepare!`, `brush!` and `brush_control!`, `files!`, `inbetween!`, `quickshape_curve!`, `events!`,
`key_shortcut!` and `key_shortcuts!`, `panel_key!` and `kept!` (the last four in `kit::keys`). The
kit uses the standard library; for the smallest plugins, `default-features = false` keeps only
`Controls`, `controls!`, `yes!`, `answer!` (a whole number) and `position!`, and the crate is `#![no_std]` with a panic handler of its own:

```rust
#![no_std]

static CONTROLS: kit::Controls<1> = kit::Controls::new([0.0]);
kit::controls!(CONTROLS);
kit::yes!(procraft_pin_panels, || CONTROLS.on(0));

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}
```

Without the kit, a plugin exports the functions itself, with exactly the types each hook below names.
A plugin built with the kit is packed again whenever the kit changes (its packed file is checked
against the kit too).

The crate is a `cdylib`, kept out of the app's workspace by an empty `[workspace]` table, and its name
is its folder's. Copy the `Cargo.toml` of a sample: its release profile keeps the module small, and a
module can be 1 MiB at most.

A plugin with code and any controls must export `procraft_control(index: i32, value: f32)`. It's told
each value by its place in the list as the code starts, then each as it changes. A switch is 0 or 1.

Every function a hook names has to be exported, with exactly these types, or the file is refused.

### `position`

Where a stroke's samples land, in window pixels.

```
procraft_position(x: f32, y: f32) -> i64
```

The result holds x's bits in the low half and y's in the high. An answer that isn't a finite number,
or that moves a sample more than 16,384 pixels, stops the plugin. Several plugins on this hook run
one after another, in order of id, each given what the last one said.

### `pick_layer`

Whether the eyedropper reads the current layer (not 0) or the canvas as it looks (0).

```
procraft_pick_layer() -> i32
```

### `interface_scale`

How big the interface shows, where 1 is its usual size, in a window of that many points at 1.

```
procraft_interface_scale(width: f32, height: f32) -> f32
```

An answer outside .5 to 2 stops the plugin. The app may show less than the answer, so that every page
stays in reach on a small window.

### `interface_part_scale`

How big a part of the interface shows next to the rest of it (interface 6), where 1 is as big as
the rest. It goes on top of [`interface_scale`](#interface_scale). The app asks about each part by
number:

| `part` | The part |
|---|---|
| 0 | Floating panels: the colour panel dragged out by its grabber, and a plugin's own panel |
| 1 | Popovers and menus: what drops down from a button or a row |
| 2 | The side bar |
| 3 | The top bar (what hangs from it hangs from it as it shows) |
| 4 | The panels that hang from the top bar's buttons (a menu from one shows at part 1's size) |

```
procraft_interface_part_scale(part: i32) -> f32
```

Answer 1 for a part you don't know: more may come. An answer outside .5 to 2 stops the plugin.
Several plugins on this hook each scale a part in turn, to .5 to 2 in all.

### `floating_picker_only`

Whether the floating colour window shows Classic's picker alone (not 0): its grabber, Close and the
picker with the hue slider under it (or the picker's own [ring](#picker)), and no title, other pickers
or mode icons.

```
procraft_floating_picker_only() -> i32
```

### `keep_clear`

Whether the interface keeps clear of the status bar and the screen's cutouts, such as a phone's camera
(not 0). The app then lays everything out inside them, the canvas too, and leaves the rest black. The
status bar's room is kept even while the bar is hidden, so a swipe that shows it for a moment moves
nothing. Where there is neither, as on a desktop, it changes nothing.

```
procraft_keep_clear() -> i32
```

### `paste_mask`

Whether a picture pasted with a mask current goes into the mask. If it does, the picture goes through
the plugin a row at a time, and the module must export its `memory`.

```
procraft_paste_mask() -> i32
procraft_row_buffer(bytes: i32) -> i32
procraft_paste_row(at: i32, width: i32)
```

`procraft_row_buffer` says where in `memory` a row of that many bytes can go. Each row is written
there as RGBA, premultiplied, top row first, and `procraft_paste_row` changes its `width` pixels in
place into what the mask should hold. Only one plugin has a picture: the first on the hook.

### `fill_mask`

What a ColorDrop fill keeps. The fill goes through as bits on a thread of its own, so this one may
take a while, and the module must export its `memory`.

```
procraft_fill_mask() -> i32
procraft_fill_begin(width: i32, height: i32, seed_x: i32, seed_y: i32) -> i32
procraft_fill_row(y: i32)
procraft_fill_run() -> i32
procraft_fill_out(y: i32)
```

`procraft_fill_mask` says whether there's anything to do. `procraft_fill_begin` says where in `memory`
a row goes: `width` bits in 64-bit words, low bit first, a set bit being filled. The box has a pixel of
no fill around it, and `seed` is where the fill started. `procraft_fill_row` takes each row as it's
written there, `procraft_fill_run` works it out and says whether anything comes out (not 0), and
`procraft_fill_out` puts each row of what comes out in the same place.

A plugin can only take away: a bit it sets comes out of the fill, and it can't add any. The seed has to
stay in the fill, or the plugin stops.

### `fill_all_layers`

Whether ColorDrop fills by every layer you see (not 0), as if they were one picture, rather than by the
current layer alone (or the reference layer, if one is set).

```
procraft_fill_all_layers() -> i32
```

It only says which to go by: the fill is still the app's, and still goes on the current layer.

### `radial_segments`

How many parts the Drawing Guide's radial symmetry makes, in place of its usual eight.

```
procraft_radial_segments() -> i32
```

An answer outside 2 to 64 stops the plugin. An odd count is made even unless the symmetry only turns
its copies (Rotational Symmetry), since the mirrored copies come in pairs. The count is never written
to the artwork: turn the plugin off, or open the artwork without it, and radial symmetry makes eight
again. With several plugins on this hook, the first by id that answers is used.

### `hide_undo_notice`

Whether undo and redo go by without the notice at the top that names what they took back or brought
back (not 0).

```
procraft_hide_undo_notice() -> i32
```

It only hides the notice: undo and redo work as before.

### `interface_colour`

The colour the interface draws a part of itself in, as 0xRRGGBB, or -1 to leave it as it is.

```
procraft_interface_colour(role: i32) -> i32
```

`role` 0 is the accent: the blue of what is chosen or switched on (a selected tab, a switch, a
slider's filled part, a picked layer). Other roles may come later; answer -1 for any you don't know.
An answer past 0xFFFFFF stops the plugin. With several plugins on this hook, the first by id that
answers is used.

### `pin_panels`

Whether a touch on the canvas with a panel open paints at once and leaves the panel up (not 0), rather
than closing it (a tap) or fading it away (a drag). The panel's own button still closes it.

```
procraft_pin_panels() -> i32
```

### `opacity_together`

Whether a layer's opacity changed while it is one of several picked layers sets every picked layer's
to the same (not 0), as one undo step. A layer that isn't picked still goes alone.

```
procraft_opacity_together() -> i32
```

### `randomise_brush`

While a plugin on this hook is on, the brush studio shows Randomise. Tapping it asks the plugin about
each setting below, by its number, with one `seed` for the whole tap:

```
procraft_randomise(setting: i32, seed: i32) -> f32
```

Answer where the setting goes as a share of its range, 0 to 1 (0 its least, 1 its most), or anything
below 0 to leave it as it is. An answer above 1, or not a number, stops the plugin. A whole-number
setting (a choice or a switch) is rounded. The plugin gets no randomness of its own: mix the seed and
the setting's number to get one.

| Number | Setting | Range |
|---|---|---|
| 0 | `spacing` | 0 to 1 |
| 1 | `streamline` | 0 to 1 |
| 2 | `streamline_pressure` | 0 to 1 |
| 3 | `pen_taper_start` | 0 to 1 |
| 4 | `pen_taper_end` | 0 to 1 |
| 5 | `pen_taper_size` | 0 to 1 |
| 6 | `pen_taper_opacity` | 0 to 1 |
| 7 | `touch_taper_start` | 0 to 1 |
| 8 | `touch_taper_end` | 0 to 1 |
| 9 | `touch_taper_size` | 0 to 1 |
| 10 | `touch_taper_opacity` | 0 to 1 |
| 11 | `input_style` | 0 to 2 (whole) |
| 12 | `shape_rotation` | -1 to 1 |
| 13 | `scatter` | 0 to 1 |
| 14 | `count` | 0 to 1 |
| 15 | `randomized` | 0 to 1 (whole) |
| 16 | `roundness` | 0 to 1 |
| 17 | `shape_angle` | 0 to 1 |
| 18 | `grain_behavior` | 0 to 1 (whole) |
| 19 | `grain_scale` | 0 to 1 |
| 20 | `grain_depth` | 0 to 1 |
| 21 | `rendering_mode` | 0 to 5 (whole) |
| 22 | `flow` | 0 to 1 |
| 23 | `jitter_size` | 0 to 1 |
| 24 | `jitter_opacity` | 0 to 1 |
| 25 | `pressure_size` | -1 to 1 |
| 26 | `pressure_opacity` | -1 to 1 |
| 27 | `tilt_angle` | 0 to 1 |
| 28 | `tilt_opacity` | 0 to 1 |
| 29 | `tilt_size` | 0 to 1 |
| 30 | `max_size` | 0 to 1 |
| 31 | `min_size` | 0 to 1 |
| 32 | `max_opacity` | 0 to 1 |
| 33 | `min_opacity` | 0 to 1 |
| 34 | `smudge_pull` | 0 to 1 |
| 35 | `wet_pull` | 0 to 1 |

Randomise is one change to the brush: Cancel or Reset brush takes it back.

### `colour_triangle`

Whether the colour panel's Classic tab shows a triangle in place of its square (not 0): saturation and
brightness between the pure hue (its right corner), white (top left) and black (bottom left). The hue
slider and the others under it stay as they are. It stays for plugins written to it; `picker` does
this and more.

```
procraft_colour_triangle() -> i32
```

### `picker`

The plugin draws the colour panel's Classic square its own way (interface 2). It brings the picture
as WGSL in `picture.wgsl`, beside its module, which the app draws on the GPU each time the hue changes.
Its code says where a touch lands and where the knob sits. Points go 0 to 1 across and down the
square, and saturation and brightness go 0 to 1. Each pair comes back packed as `position`'s do.

```
procraft_picker_pick(x: f32, y: f32) -> i64     // the saturation and brightness at (x, y)
procraft_picker_point(s: f32, v: f32) -> i64    // where they sit
```

`picture.wgsl` has one function, `picture`. It's asked for the colour (straight, not premultiplied)
at each pixel's centre, and given `hue()` (0 to 1 round the wheel from red) and `picture_size()` in
pixels:

```wgsl
fn picture(at: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(at.x, 1.0 - at.y, hue(), 1.0);
}
```

It's checked as a filter's function is (see [What is refused](#what-is-refused)): it reaches
nothing, and its loops pay from a budget. A picture that runs out of budget, or takes too long,
switches the plugin off. `tools/pack_plugin.py` packs `picture.wgsl` for a plugin on this hook. The
kit's `picker!` writes the two functions, and `plugins/colour-triangle` is one to start from.

Written to interface 6, the picture is given the plugin's controls too, as a filter is: each as
`v.<key>`, a switch 1 or 0. The app draws it again when they change.

From interface 6 a picker may also export these, each asked as the controls stand:

```
procraft_picker_on() -> i32                   // whether it draws Classic now (not 0); without it, always
procraft_picker_hue(x: f32, y: f32) -> f32     // the hue a touch starting at (x, y) picks, 0 to 1
procraft_picker_hue_point(h: f32) -> i64       // where hue h's knob sits, packed as above
```

- While `procraft_picker_on` answers 0, Classic is the app's own square, as with the plugin off.
- While `procraft_picker_hue_point` answers a point (both halves 0 or more), the picker has the hue
  too, as on a ring of its own: the app leaves out its hue slider and draws the hue's knob there. A
  touch that starts where `procraft_picker_hue` answers a hue (0 or more; below 0 elsewhere) moves
  the hue for the rest of its drag, asked again at each point (one below 0 leaves the hue as it was);
  any other moves saturation and brightness. Answer -1 for both halves to leave the hue to the slider.
- An answer that isn't a number stops the plugin.

### `inbetween`

Animation Assist's frame options get In-between while a plugin is on this hook: the plugin draws a
frame between the current frame and the next (each a single layer), and the app puts it just after
the current one and makes it current, as one undo step. Tapped again, it goes between the new frame
and the next, and so on. How the frame between looks is all the plugin's: the app only hands over the
two frames' pixels (the part either is drawn in) and takes back what it draws.

```
procraft_between_begin(width: i32, height: i32) -> i32
procraft_between_row(frame: i32, y: i32)
procraft_between_run() -> i32
procraft_between_out(y: i32)
```

`begin` starts one of `width` × `height` pixels and answers where in its memory a row of them goes
(`width` × 4 bytes, premultiplied RGBA). The app then writes each row of this frame (`frame` 0) and of
the next (`frame` 1) there, calling `row` after each; calls `run`, which draws the frame between (0 for
none); and for each row calls `out` and reads it back from the same place. It is a long call, on fuel
by the area, with up to 256 MB of memory; one that runs out stops. The Tweening plugin the app comes
with slides each drawing from its own place and size toward the other's and fades between them.

### `stroke_kept` and `layer_added`

Told each time a stroke is kept (`stroke_kept`), or a layer is added with the Layers panel's +
(`layer_added`), the plugin acts as an action does, with the functions its permissions give (see
[Actions](#actions)):

```
procraft_event(kind: i32) -> i32
```

`kind` is 1 for a stroke kept, 2 for a layer added. It runs at once, before the next touch is taken,
on a touch's budget (it should be quick: one that runs out goes off and says why), and what it changes
is one undo step of its own, after the stroke's or the layer's. What a plugin changes itself, here or
in an action, tells no plugin anything, so they can't set each other off. The kit's `events!` takes
a `fn(kit::app::Event)`.

### `key_shortcut`

A hardware keyboard's presses on the canvas, taken as other presses before the app's shortcuts see
them: the plugin holds the bindings, and the app's own shortcuts stay as they are. Typing in a text
field never comes here. The module must export its `memory`.

```
procraft_key_buffer(len: i32) -> i32
procraft_key_shortcut(len: i32) -> i64
procraft_key_shortcut_keys(len: i32) -> i64
procraft_key_shortcuts(len: i32)                 // if it exports it
```

A press is text: its modifiers, each with a `+` after it, then its key, as in `command+shift+z`,
`shift+]` or `space`. The modifiers are `command` (Ctrl, or ⌘ on a Mac), `shift` and `alt`, in that
order when the app writes them and in any order when the plugin does. The key is a letter or a digit,
a sign (`[`, `]`, `;`, `,`, `.`, `/`, `\`, `'`, `` ` ``, `-`, `=`), or a word: `space`, `plus`,
`delete`, `backspace`, `escape`, `left`, `right`, `up`, `down`, `f1` to `f12`. A key is at most 16
characters, lowercase, without `+` or a space. A platform may not have every key.

A few presses are the view's own moves, which no keyboard sends: `view:zoom-in` and `view:zoom-out`
(1.25 times nearer or further each time), `view:fit` (the whole canvas in the window), `view:turn:15`
(clockwise by that many degrees, below 0 the other way, at most 360), and `view:pan-left`,
`view:pan-right`, `view:pan-up` and `view:pan-down` (the view moves that way, the canvas the other).
They have no keys as the app comes, so a plugin gives them keys by taking presses as them, on every
platform.

The app writes the press where `procraft_key_buffer` says `len` bytes go, then asks:

- `procraft_key_shortcut`: what the press is taken as. 0 leaves it as it is; below 0 makes it no
  press at all, so it does nothing; anything else is where the other press's text is in `memory`
  (the low 32 bits) and how long it is (the high 32 bits). That press does what it does by default,
  and isn't asked about again.
- `procraft_key_shortcut_keys`: which presses now do what this press does by default, for the
  keyboard shortcuts' list. 0: still this press; below 0: none (the list leaves the row out if it has
  no press left); anything else: the presses' text, packed as above, a space between them.

An answer whose text isn't a press stops the plugin, as does more than one press for
`procraft_key_shortcut`. Only one plugin rebinds keys: the first on the hook, by id.

`procraft_key_shortcuts`, if the module exports it, is told the app's shortcuts as the plugin starts,
in every copy of its code, through `procraft_key_buffer` too. It's a line for each, of four fields a
tab apart: its group (`Edit`), what it does (`Undo`), the presses that do it by default (a space
between), and `1` if the app keeps those keys as they are (holding them moves the view, say) or `0` if
they may be rebound. A row of the list whose presses each do something of their own (one smaller, one
bigger) is a line for each press. The list is the platform's: a desktop's has its files, zooming and
turning too. The view's own moves are lines of the `View` group, their presses those above (but the
desktop's own zooming and fitting, which it has keys for): `view:turn:15` and `view:turn:-15` stand
for turning either way.

The kit's `key_shortcut!` takes two functions, from a `&kit::keys::Press` to a `kit::keys::Taken`
(`AsItIs`, `As(press)` or `Nothing`) and to an `Option<Vec<Press>>`; `key_shortcuts!` takes a
`fn(Vec<kit::keys::Shortcut>)`. The Keyboard shortcuts plugin the app comes with takes each key bound
anew as its shortcut's first key as it came, and a key a shortcut no longer has as none. It turns
the canvas by a step of its own, which you choose where the turning shortcuts take their keys.

### `quickshape_curve`

A curve QuickShape may snap to. When a held stroke snaps, the app hands the plugin the stroke's points
and the kind QuickShape found first; the plugin may offer a curve, which then comes first among the
kinds to switch to (what QuickShape found still there). When Curve is picked from the kinds again,
the plugin is asked for one by name. The app draws the curve and lets it be reshaped: dragging a
control moves its pair the other way, so the curve stays smooth there; tapping an anchor sets it free
(each control on its own), then a corner, then smooth again.

```
procraft_curve_begin(count: i32, best: i32) -> i32
procraft_curve_run() -> i32
procraft_curve_nodes() -> i32
```

`begin` answers where the stroke's `count` points go (x and y, 32-bit floats each, window pixels);
`best` is the kind QuickShape found first, by its place among line, polyline, arc, curve, ellipse,
circle, triangle, equilateral triangle, quadrilateral, rectangle and square, or -1 when a curve is
asked for by name. `run` answers how many nodes the curve has, 0 for none: an anchor, then two
controls and an anchor for each span (4, 7, 10 ...). `nodes` answers where they are, as the points
came. The QuickShape curves plugin the app comes with offers one for a polyline: an anchor at each
corner, each anchor's controls leaning the way the stroke came and went.

### `stroke_sound`

A sound as you draw. While a plugin on this hook is on, the app asks it for the stroke's sound every
few milliseconds, on a thread of its own, and plays what it hands back:

```
procraft_stroke_sound(frames: i32, rate: i32, speed: f32, pressure: f32, since_ms: f32,
                      state: i32, hover_x: f32, hover_y: f32, height: f32) -> i32
procraft_sound_prepare(rate: i32) -> i32
```

It answers where in its memory the next `frames` samples are (32-bit floats, -1 to 1, at `rate` a
second). The stroke moves at `speed` (window heights a second) with `pressure` (a finger as pressed
all the way); its last input came `since_ms` ago (below 0: no stroke is open); `state` says, as bits,
1 a stroke is open, 2 the pen hovers (at `hover_x`, `hover_y`, window pixels, in a window `height`
tall), 4 the tool erases. Before the first sound at a rate, `procraft_sound_prepare` (if it has one)
readies whatever it makes once, as a long call. A sound may spend fuel by the frame; there's 128 MB
of memory. It's told the brush and its rows as above, so a sound can go by the brush.

The Brush sounds plugin the app comes with behaves as Krita's Brush SFX plugin by davi133 does
(github.com/davi133/brush_sfx): six sounds (an eraser, a pencil, a pen, a paintbrush, an airbrush and
a spray can), each brush the one its kind takes (by its name, else its set's), louder the faster and
harder the stroke goes, the pen's and the paintbrush's ring rising with speed, the airbrush hissing
while pressed and the spray can while it moves, its ball rattling when the pen is shaken back and
forth above the canvas. Its controls set the volume, one sound for every brush, whether erasing makes
a sound and which, and a recording for any of the six; its row in the brush studio gives a brush its
own sound, or none. All of it is in the plugin, made sample by sample with a few filters.

### Limits

| | |
|---|---|
| The module | 1 MiB |
| Memory it may ask for | 16 MiB, or 64 MiB on `fill_mask` |
| Calls deep | 256 |
| Steps to start | About 1,000,000 |
| Steps for a sample, a control or a question | About 100,000 |
| Steps for a pasted row | 10,000, and 64 for each pixel |
| Steps for a fill | 20,000,000, and 1,024 for each pixel of its box, for at most 30 seconds |
| A fill's box | 8,194 × 8,194 pixels in area |
| What a plugin keeps | 64 KiB |
| Memory, with kinds of layer | 256 MiB |
| A layer's data | 64 MiB |
| Steps for a call on a layer | 800,000,000, for at most 5 seconds |

A pen sends a sample every 4 ms at 240 Hz, so a `position` call has to be far quicker than that. Use a
few hundred steps, not thousands.

A fill too big for the limit stays as it was, and the app says so.

Code keeps its memory while the plugin is on, between calls. It starts afresh when the plugin is
turned on, and on each start of the app, so anything it remembers lasts only that long, unless it
keeps it (see [What a plugin keeps](#what-a-plugin-keeps)).

## Actions

An action is something code does when tapped: a row on its card, under its controls, once it's on.
While it runs, it can call the app's functions its `permissions` give: read the artwork, add layers
and paint on them, set the brush, show a short message. Everything one action changes is one step
undo takes back.

```json
  "compat": { "procraft": { "interface": 2 } },
  "permissions": ["artwork.read", "artwork.write", "brush"],
  "hooks": [],
  "actions": [
    { "key": "add", "label": "Add a grid layer" }
  ]
```

A plugin can have up to eight actions. Each `key` is used once, and each `label` is the row's words.
Its card lists them all (one `under` a switch only while that switch is on, and only then does it
run); `"place"` lists one somewhere else too:

- `"adjustments"`: in the Adjustments menu's Plugins list, after the filters, for something done
  to the artwork as an adjustment is;
- `"layer"`: in the current layer's options, after the app's own rows;
- `"frame"`: in Animation Assist's frame options, after the app's own rows, run with that frame's
  layer current.

A place this app doesn't have makes the file one for a newer ProCraft. Actions need interface 2. The import screen
says what each permission lets the plugin do.

| Permission | The functions it gives |
|---|---|
| `artwork.read` | `canvas_size`, `layer_count`, `layer_at`, `current_layer`, `read_pixels` |
| `artwork.write` | `add_layer`, `write_pixels`, `set_layer_name`, `set_layer_opacity`, `set_layer_visible` |
| `brush` | `brush_colour`, `set_brush_colour`, `brush_size`, `set_brush_size`, `brush_opacity`, `set_brush_opacity` |
| `notice` | `notice` |
| `panel` | `show_panel`, `close_panel` (see [Panels](#panels)) |

The module imports them from `"procraft"`, and the app checks each one's types. It exports
`procraft_action(index: i32) -> i32`, called with the action's place in `actions`; it returns 0.
The kit writes all of this:

```rust
use kit::app;

kit::actions!(|_| {
    let (width, height) = app::canvas_size();
    if let Some(layer) = app::add_layer() {
        app::name_layer(layer, "Frame");
        let ink = [0, 0, 0, 255].repeat(width as usize * 4);
        app::paint(layer, 0, 0, width, 4, &ink);
        app::paint(layer, 0, height as i32 - 4, width, 4, &ink);
    }
});
```

- Layers go by the id the app gives each one, never 0. 0 means none.
- Pixels are premultiplied RGBA8, rows from the top. A rectangle has to be wholly on the canvas, at
  most 4,096 × 4,096 pixels. A write goes over what was there.
- Code can paint only on a pixel layer that isn't locked.
- Sizes and opacities are 0 to 1, as the sliders show them. A colour is `0xRRGGBB`.
- A name or a message is UTF-8 text, at most 200 characters.

An action runs in a copy of the code of its own, told the controls as they are, so its hooks go on
meanwhile. It may take 4,000,000,000 steps, for at most 20 seconds. Calls to the app's functions
answer only while an action runs. At a hook they do nothing.

`plugins/layer-fills` is one to start from.

### Panels

With the `panel` permission, an action can show a panel of the plugin's own. Each app draws it the
way its own cards are drawn: a title and Done, then rows. The rows are a slider, a switch, a choice,
some words, or a button. The plugin describes the panel as JSON text and hands it to
`show_panel(at, len)`, which answers 1 if the panel holds up and 0 if not:

```json
{ "title": "Halftone", "rows": [
  { "type": "slider", "label": "Dot size", "least": 1, "most": 50, "step": 1, "value": 8, "unit": " px" },
  { "type": "switch", "label": "Round dots", "on": true },
  { "type": "choice", "label": "Shape", "options": ["Dots", "Lines"], "chosen": 0 },
  { "type": "text", "text": "Works on the current layer." },
  { "type": "button", "label": "Apply" }
] }
```

- A panel has 1 to 16 rows, and a choice has 1 to 8 options.
- Labels and the title are at most 40 characters, an option at most 24, and words at most 400.
- A field the app doesn't know, or a kind of row it doesn't have, makes the panel not hold up.
- A `keys` row shows presses (written as for the [`key_shortcut`](#key_shortcut) hook) the way the
  platform writes them, "Ctrl+Z" or "⌘Z", or "None" for none, and is tapped as a button is:
  `{ "type": "keys", "label": "Undo", "keys": ["command+z"] }`. It has eight presses at most.
- The panel shows once the action ends, and stays open in the same copy of the code.
- Each touch calls the module's `procraft_panel_event(row: i32, value: f32) -> i32`, so a module
  with the `panel` permission has to export it. `row` is the row's place, from 0. The value is the
  slider's (on its steps), 1 or 0 for a switch, the option's place for a choice, or 1 for a button.
  A slider is told when it's let go.
- A touch can do what an action can, and what it changes is one step to undo. It can show the panel
  again, changed, or close it with `close_panel()`.
- Done closes the panel. So does another action, or the plugin being turned off.
- A panel with `"key_press": true` asks for the next key press: while it shows, that press goes to the
  plugin rather than to the canvas, Escape too, as
  `procraft_panel_key(len: i32) -> i32`, its text (as for the [`key_shortcut`](#key_shortcut) hook)
  written where `procraft_key_buffer(len: i32) -> i32` says. It acts as a touch does. The panel asked
  for that one press: to take another, show it again asking. A module whose panel asks needs both
  functions, or the panel doesn't hold up. The kit's `panel_key!` writes the one, and
  `key_shortcut!` or `key_buffer!` the other; `Panel::keys` and `Panel::key_press` write the JSON.

The kit writes the JSON for you:

```rust
use kit::app::{self, Panel};

kit::actions!(|_| {
    Panel::new("Halftone").slider("Dot size", 1.0, 50.0, 1.0, 8.0, " px").button("Apply").show();
});
kit::panel!(|row, value| {
    if row == 1 {
        apply_halftone();
        app::close_panel();
    }
});
```

### What a plugin keeps

Code can keep a little of its own from one start to the next, such as what someone set on its panel:
up to 64 KiB, kept with the plugin (an update keeps it, and it goes when the plugin is removed). A
module that keeps something exports these:

```
procraft_keep() -> i64
procraft_kept_buffer(len: i32) -> i32
procraft_kept(len: i32)
```

After each of its actions, a touch on its panel and a key press its panel asked for, the app asks
`procraft_keep`: below 0 for nothing new, otherwise where what to keep from now on is in `memory`
and how long, packed as a `key_shortcut` answer is. Every copy of its code is told what it keeps as it
starts, and again whenever another copy keeps something new: into `procraft_kept_buffer` (where `len`
bytes go), then `procraft_kept`. So what a panel sets reaches the copy on its hooks at once. The kit's
`kept!` takes a `fn(&[u8])`, and `kit::keys::keep(data)` hands the app something new.

An older ProCraft keeps nothing: there a plugin starts each time with nothing kept.

## Artworks of its own

Code can make a kind of artwork of its own: a vector drawing, say. The plugin keeps the artwork in
its own format. The app hands it the strokes, shows what it draws, and keeps what it saves after each
stroke as a step for undo and redo. The manifest lists the kinds (interface 2):

```json
  "kinds": [{ "key": "dots", "label": "Dots" }]
```

While the plugin is on, the gallery's New canvas list has a row for each kind under the presets,
the screen's size. Such an artwork's folder keeps the plugin's data in `plugin-data.bin`, its kind in
`artwork.json`, and what it last drew as its layer. Opened with the plugin off, it shows that, and the
plugin's data stays as it was. Share Image offers SVG for it too, written from its draw list, so its
shapes stay shapes. A plugin can list up to four kinds. Its module exports:

```
procraft_canvas_new(width: i32, height: i32) -> i32           // 0 if it made one that size
procraft_canvas_touch(phase: i32, x: f32, y: f32, pressure: f32) -> i32
procraft_canvas_draw() -> i32                                  // the draw list's length
procraft_canvas_save() -> i32                                  // its data's length
procraft_canvas_out() -> i32                                   // where either of those is
procraft_canvas_in(len: i32) -> i32                            // where `len` bytes of data go
procraft_canvas_load() -> i32                                  // 0 if it took them
procraft_canvas_tool(tool: i32, colour: i32, size: f32, opacity: f32)   // if it exports it
```

`procraft_canvas_tool` is told what each stroke starts with: the tool (0 paint, 1 erase, 2 smudge),
the colour (`0xRRGGBBAA`), the brush's width in canvas pixels, and its opacity (0 to 1).

- A touch's `phase` is 0 down, 1 moved, 2 up, or 3 called off (a second finger made it a gesture).
  Coordinates are canvas pixels. The touch answers not 0 if what it draws changed.
- The draw list is little-endian 32-bit words. `1 fill stroke width flags` starts a path. Fill and
  stroke are `0xRRGGBBAA`, straight, with alpha 0 for none. The width is a float. Flags 1 makes the
  ends and corners round, and 2 fills by the even-odd rule. Then `2 x y` moves, `3 x y` draws a line,
  `4 x1 y1 x2 y2 x y` draws a cubic curve, and `5` closes. A list that doesn't hold up stops the
  plugin.
- Data is at most 64 MiB. A touch may take 20,000,000 steps, for at most a quarter of a second; the
  other calls 800,000,000, for at most 5 seconds.

The kit's `canvas` module writes it: an `Artwork` (new, the brush a stroke starts with, touch, draw
into a `DrawList`, save, load) and `kit::canvas!(MyArtwork)`.

## Layers of its own

Code can add a kind of layer of its own (interface 3): a vector layer, say, whose strokes stay
strokes. It sits in the layer stack as any other layer: the app shows, blends, groups, moves,
duplicates and deletes it, and undoes every change to it. Every stroke on it is painted as on any
layer, with whatever brush is chosen, by the app's own brush engine. The app then records the stroke
as an item and hands it to the plugin, which keeps the layer's items in data of its own. When the
plugin changes items already drawn, the app draws the layer again from them. The manifest lists the
kinds:

```json
  "compat": { "procraft": { "interface": 3 } },
  "layer_kinds": [
    {
      "key": "vector",
      "label": "Vector layer",
      "badge": {
        "lines": [{ "points": [0.22, 0.72, 0.38, 0.05, 0.62, 0.95, 0.78, 0.28], "curved": true }],
        "dots": [0.22, 0.72, 0.78, 0.28]
      },
      "exports": [{ "key": "svg", "label": "SVG (vector layers)", "extension": "svg", "mime": "image/svg+xml" }]
    }
  ]
```

While the plugin is on, the Layers panel's + offers each kind beside a plain layer, named after its
`label`. A plugin can list up to four kinds. The import screen says the plugin can add kinds of layer.

`badge` is optional. It is what the kind's layers show in the corner of their thumbnails, drawn in
white over a small dark box, and what the + shows beside the kind's name, drawn larger in the menu's
colour. Its numbers are places in that box, 0 to 1 across and down:

- `lines`: up to eight, each up to 32 points, x then y. A line goes straight from point to point, or
  with `"curved": true` it is cubic curves: a start, then two handles and an end for each curve.
- `dots`: up to eight small squares, x then y.

Without a badge, the + and the kind's layers show a plug, as do its layers while the plugin is off or
gone.

`exports` is optional too: up to four files the plugin makes of an artwork's layers of the kind. While
the plugin is on and the artwork has a layer of the kind, Share Image lists each under its `label`,
beside the app's own formats. `extension` is the file's, a few lower-case letters or digits, and
`mime` its media type. The plugin makes the file with `procraft_layer_export` (below).

- The app keeps each such layer's data in the artwork beside the layer's pixels
  (`layers/<id>.layerdata` next to `layers/<id>.png`), written whole, and puts it back with the pixels
  when a stroke is undone or redone. Artworks saved by earlier versions have it in
  `layers/<id>.vector`, which is read when there is no `.layerdata` file and replaced by one at the next
  save.
- With its plugin off or removed, or its data file gone or unreadable, the layer shows its pixels, as
  locked: it takes no strokes, and its data is left as it is through saves.
- A file in another app's format, such as a layered PSD or the package format, has the layer as its
  pixels.
- Only its strokes change the layer. With a plugin that changes its items (below), Clear deletes the
  strokes a selection takes (all of them with nothing selected), the layer's options offer Recolour
  and Apply Brush for them, Transform moves them as strokes, and flipping or cropping the canvas
  moves every stroke with the pixels.
- ColorDrop on it goes to the plugin, if it exports `procraft_layer_fill` (below). Otherwise ColorDrop,
  and always Invert and adjustments, wait until it is rasterized, as with a text layer. Rasterize makes it
  a plain layer, and so does merging another layer into it. Flipping or cropping the canvas does too
  when its plugin can't move its items.
- A stroke drawn inside a selection is kept with that selection and is always laid again inside it.
- A stroke held still on it snaps with QuickShape as on any layer. It is kept as the shape: its
  samples along the shape's outline, its kind and nodes in field 26, laid along them exactly again.
  QuickShape on the last stroke, after it was kept, doesn't snap one on it.

Its module exports:

```
procraft_layer_in(len: i32) -> i32               // where `len` bytes go, for the next call
procraft_layer_out() -> i32                      // where an answer is
procraft_layer_load(kind: i32, len: i32) -> i32  // a layer's data (none: a new one): 0 if it took it
procraft_layer_add(len: i32) -> i32              // an item on that layer: 0 kept as drawn, 1 draw again
procraft_layer_save() -> i32                     // the layer's data now: its length
procraft_layer_draw() -> i32                     // the layer's items to draw: its length
procraft_layer_select(len: i32) -> i32           // if it exports it: the ids a selection takes
procraft_layer_edit(len: i32) -> i32             // if it exports it: changes to items, not 0 if made
procraft_layer_fill(len: i32) -> i32             // if it exports it: a colour dropped, not 0 if items changed
procraft_layer_export(len: i32) -> i32           // if it exports it: a file of an artwork's layers: its length
procraft_layer_hit(len: i32) -> i32              // if it exports it: the item at a point: 4 (its id), or 0
```

For each change, the app first hands over the layer's data with `procraft_layer_load`. `kind` is the
kind's place in `layer_kinds`, and `len` is 0 for a layer with no data yet. Then it calls one of the
others, so the plugin keeps nothing between changes. As a stroke ends:

1. The app calls `procraft_layer_add` with the item.
2. It asks `procraft_layer_save` for the data from then on.
3. If `procraft_layer_add` answered 1, it asks `procraft_layer_draw`. It then clears the layer and lays
   those items on it, in order, as one step with the stroke.

If the data afterwards is the same as before, nothing changes and no step is made. A plugin answers 1
when the stroke changed what was drawn before, such as an eraser taking strokes away whole. An
answer other than 0 or 1 stops the plugin.

A plugin that exports both of the last two has its items selected and changed as items. Each item to
draw then says the plugin's id for it (field 21, u32).

- `procraft_layer_select` is handed the selection as a record. Fields 1 to 4 hold its box, x, y,
  width and height (u32). Field 5 holds how much it takes of each pixel there, a byte each (0 none,
  255 all), rows from the top. The plugin answers with the ids of the items it takes, as u32 each.
- `procraft_layer_edit` is handed a record of changes, field 1 each. A change is a record too:
  - field 1, the item's id (u32);
  - field 2, u8 1 if the item goes;
  - field 3, the fields the item takes in place of its own (an item record: a new colour, size,
    brush or transform, say).

  The plugin answers not 0 if anything changed. The app then asks for the data and the items to draw,
  and lays the layer again as one step.

While the Selection tool is open on such a layer, a box like the transform box shows round the
strokes the selection takes, and the arrow keys move them a pixel at a time, the selection with them.
An edit lays the layer again only where what changed reaches. Zoomed in, once the view settles, the
layer's items are laid again at the screen's resolution over the canvas, so they stay sharp; the
layer's own pixels, which saving and exporting read, stay as they are.

`procraft_layer_fill` is handed a colour dropped on the layer with ColorDrop, as a record:

- fields 1 and 2, where it landed, x and y in canvas pixels (32-bit floats);
- field 3, the colour, three 32-bit floats, sRGB 0 to 1;
- field 4, the threshold ColorDrop is at, 0 to 1 (32-bit float);
- field 5, how many canvas pixels across each pixel of the picture in field 6 is (32-bit float);
- field 6, what bounds a fill there, as a selection is handed over (box and a byte a pixel), in that
  picture's pixels: how much of each is drawn. It is the layer's own pixels, every layer seen while a
  plugin says to fill from all of them, or the reference layer when another layer is it. A canvas
  over 2048 pixels a side comes shrunk by whole steps, each pixel the most of those it stands for.

The plugin answers not 0 if it changed its items. The app then asks for the data and the items to
draw, and lays the layer again as one step called ColorDrop.

`procraft_layer_hit` is handed a point as a record: x and y (fields 1 and 2) in canvas pixels, and
how far about it counts as there (field 3), each a 32-bit float. It answers 4 with the id of the
topmost item there at `procraft_layer_out()` (u32), or 0 for none. With it, a tap of the Selection
tool on the layer selects the item under it as a selection of that item alone would, joined to the
selection as Add or Remove says, and a tap on nothing lets the selection go (a freehand path being
tapped out point by point goes on as it does).

`procraft_layer_export` is handed what a file is to be made of, as a record, and needs no layer
loaded first:

- field 1, which of the kind's `exports` (u32, its place in the manifest);
- field 2, which kind (u32, its place in `layer_kinds`);
- fields 3 and 4, the canvas's width and height (u32);
- field 5 for each of the artwork's layers of the kind, bottom up: a record of its name (field 1,
  UTF-8), whether it shows (field 2, u8), its opacity (field 3, 32-bit float) and its data (field 4,
  none for a layer with nothing on it yet).

It answers with the file's length, the file at `procraft_layer_out()`, or below 0 for none. The app
writes the file whole.

An item is a record: fields one after another, each a 16-bit id, a 32-bit length and that many bytes,
all little-endian. A reader passes over ids it doesn't know, so a later app can add fields. Ids from
`0x8000` up are the plugin's own: the app ignores them in what it is handed back. The app writes:

| Id | Field |
|---|---|
| 1 | The tool, u8: 0 paint, 1 mask (the eraser, laid as it erased), 2 smudge |
| 2 | The samples, 32 bytes each: x, y (canvas pixels), pressure, tilt, azimuth and roll as 32-bit floats, then the milliseconds since the first sample as a 64-bit float |
| 3, 4 | The colour and the secondary colour, three 32-bit floats each, sRGB 0 to 1 |
| 5, 6 | The size and opacity sliders, 32-bit floats, 0 to 1 |
| 7 | The brush's width at full pressure, canvas pixels, 32-bit float: for hit tests |
| 8 | The brush, whole, in the app's own words (keep it as it is) |
| 9 | The brush's name, UTF-8 |
| 10 to 16, 18 to 20 | What else the app needs to lay the stroke again as it was laid: its randomness, the pointer, the brush scale, the view, alpha lock, the app's smoothing and pressure, symmetry, how its samples came, when, and the brush's exact sizes |
| 22 | Where it is laid, six 32-bit floats `a b c d e f`: each sample's (x, y) goes to (a·x + c·y + e, b·x + d·y + f), its brush scaled and its pen's lean turned with it. None: where it was drawn |
| 23 | The smoothing it is laid with in place of its brush's own, 32-bit float, 0 to 1 |
| 24 | The selection it was drawn inside, if any: its box (x, y, width, height, u32), then its coverage in runs, each a u32 count and the byte |
| 25 | A region it fills, in place of samples: u32 how many outlines, then each a u32 count of points and them, x and y as 32-bit floats (canvas pixels). The app fills them even-odd in the item's colour (field 3) at its opacity (field 6), smoothed, over what is under it, moved by its transform (field 22). Such an item needs no brush |
| 26 | The shape it snapped to with QuickShape: u8 its kind (0 line, 1 polyline, 2 arc, 3 curve, 4 ellipse, 5 circle, 6 triangle, 7 equilateral triangle, 8 quadrilateral, 9 rectangle, 10 square), then its nodes, x and y as 32-bit floats (canvas pixels). Its samples are laid exactly as they are, nothing smoothing them, and a dual brush's first alone |

The brush is kept as the brush library keeps one: the brush it came from among those the app comes
with, and every setting changed from there. An item draws the same however the brush is changed
later, or if it's deleted. The items to draw are a record too. Field 1 holds a brush's bytes, the
first 0, the next 1 and so on, so each brush is there once. Field 2 holds an item, which names its
brush by that number in field 17 (u32) in place of field 8.

The kit's `layer` module writes all of this. Implement `kit::layer::Layer` and add
`kit::layers!(MyLayer)`:

- The trait's calls are new, load, save, add an `Item`, draw into a `Drawn`, and, if you want them,
  select (an `Area`), edit (`Change`s), fill (a `Dropped` colour) and export (an `Export`).
- `Item` reads the tool, the width, the samples and where they are drawn (its transform applied, and
  its symmetry's copies, `paths`). It cuts samples into a new item (`with_samples`), and keeps the
  record as it came.

`plugins/vector-layers` is one to start from. It keeps each brush once, and its card chooses what
the eraser does, and what a selection takes:

- Eraser: Mask (as it starts) keeps an erasing stroke as an item that hides what is under it, so
  deleting it later brings the strokes back. Soft mask does the same, hiding only part of what is
  under it, as the eraser does at half its opacity.
- Eraser: Whole takes away every stroke the eraser's path comes within reach of, whole.
- Eraser: Cut takes away the parts of strokes the eraser passes over. What is left of each becomes
  strokes of its own, its samples as they were, cut where the eraser's edge crosses it.
- Erasing reaches a stroke's symmetry copies as well as the stroke itself.
- A selection takes the strokes wholly inside it. It takes those it touches too while "Selections
  take touching strokes" is on, and masks too while "Selections take masks" is on. A tap takes the
  stroke or fill under it.
- A colour dropped on the layer fills the region round where it lands, bounded by what is drawn, and
  kept out of gaps in the lines up to "Fills close gaps" wide. The fill is an item of its own, behind
  the strokes: it moves, recolours and deletes as a stroke does, and a selection takes it by its
  outline. Dropped on a stroke or a fill, the colour recolours that.
- With "Smudge nudges strokes" on (as it starts), the smudge tool pushes the strokes under it along
  as it goes, bending them, and keeps no smudge.
- Share Image offers "SVG (vector layers)": each vector layer a group, each stroke its outline (as
  wide as its brush where the pen pressed), each fill its region, and each mask hiding what is under
  it in its layer.

## Brush engines of its own

Code can be a brush engine (interface 4): it reads brush files of a kind the app doesn't draw as
their own app does, and draws the brushes it makes of them itself. The app hands it each stroke's
input and lays the dabs it answers on the GPU, as it lays its own stamps, so the size and opacity
sliders, the colour, the eraser and smudge tools, undo and every layer work as with any brush. The
manifest lists its engines:

```json
  "compat": { "procraft": { "interface": 4 } },
  "permissions": ["panel", "brush.data"],
  "actions": [{ "key": "studio", "label": "Its brush studio" }],
  "brush_engines": [
    { "key": "pens", "label": "Pen brushes", "accepts": ["pens"], "studio": "studio" }
  ]
```

A plugin can list up to four engines. `accepts` names the endings of the files each reads, one to
eight, each a few lower-case letters or digits. `studio` is optional: the key of one of its actions,
which needs the `panel` permission. The import screen says the plugin can read brush files and draw
their brushes its own way.

- A brush file whose name ends as an engine on accepts goes to that engine as the app imports it
  (a file with no name, to every engine on). Each brush the engine makes joins the brush of its name
  the app read of the file, or comes in on a round brush. The app keeps the engine's data with it, in
  the library, as it is.
- With the plugin off or gone, such a brush draws as the app read it.
- With `studio`, opening the brush studio on such a brush runs that action instead, its panel the
  brush's studio. The `brush.data` permission gives it the brush's data, and takes it back changed.

Its module exports:

```
procraft_brush_in(len: i32) -> i32                   // where `len` bytes go, for the next call
procraft_brush_out() -> i32                          // where an answer is
procraft_brush_import(engine: i32, len: i32) -> i32  // a file: the brushes made of it, its length
procraft_brush_load(engine: i32, len: i32) -> i32    // a brush: how it looks, its length; -1 not one
procraft_brush_begin(len: i32) -> i32                // a stroke starts: its number; -1 load it first
procraft_brush_add(stroke: i32, len: i32) -> i32     // its samples: how many dabs
procraft_brush_end(stroke: i32) -> i32               // it ends: how many dabs
procraft_brush_under(stroke: i32, len: i32) -> i32   // optional: the colours under dabs it asked about
```

`engine` is the engine's place in `brush_engines`. Everything goes as records, as layers' items do.

- `procraft_brush_import` is handed the file's name (field 1, UTF-8) and its bytes (field 2). It
  answers with a field 1 for each brush it makes: its name (field 1), its data (field 2) and, if it
  has one, its preview as a PNG (field 3). A long call, with 256 MiB of memory.
- `procraft_brush_load` is handed the app's id for a brush (field 1, u64), its data (field 2) and,
  if the app holds how the brush looks already, field 3 (u8 1): the engine reads the brush to draw
  it and answers nothing. Otherwise it answers how the brush looks, which the app keeps on the
  disk with the plugin, for the brush's data, until the plugin is imported again:
  - field 1 for each of its tips: its width (field 1, u32), height (field 2, u32) and pixels
    (field 3, a byte each, how much it paints; four with field 7), at most 255. Dabs pick one each.
  - field 7, what its tips are (u8): 0 masks, as above; else RGBA pictures (straight, rows from
    the top) that paint as far as they are opaque: 1 in their own colours, 2 in the colour at the
    lightness their grey says (a middle grey keeps the colour's own, as far as the dab's lightness
    strength goes), 3 in a colour between the colour (black) and the secondary colour (white).
  - field 9 (u8 1), its dabs move paint, as a smudge's do (see flag 16 below), whatever the tool
    but the eraser; with field 10 (u8 1) they pick it up from the image as last shown (every layer)
    rather than from the layer they lay it on.
  - field 11 (u32, at most 512), a stroke of the pen's wants the colours of its layer under where it
    starts, a square that many pixels a side around its first point: the begin is handed them.
  - field 12 (u8), with field 9, what its strokes keep with their layer between strokes (see [What
    a brush keeps with a layer](#what-a-brush-keeps-with-a-layer)): 1 the paint they lay and a
    height that lights it.
  - field 8, how often it wants the pen while it holds still (f32 ms, at least 4), as an airbrush
    lays dabs by time: until the pen lifts, the app hands it the pen as it last was, at the time
    then, every half of that.
  - field 2, how its dabs build up (u8): 0 each laid over the last, 1 each towards its opacity by its
    flow, never past it and never back.
  - field 3, the blend mode it lays its stroke with (u32, the layers' modes in the order the Layers
    panel lists them from Normal 0, Multiply 1, Darken 2 ...; 29 behind).
  - field 4, 1 if it erases whatever the tool.
  - field 5, a texture each dab reads where it lands: a picture as a tip's fields 1 to 3, how many
    canvas pixels each of its pixels takes (field 4, f32), and how it combines with a dab (field 5,
    u32): 32 multiply, 33 subtract, 34 darken, 35 overlay, 36 colour dodge, 37 colour burn, 38 linear
    dodge, 39 linear burn, 40 hard mix, 41 softer hard mix, 42 height, 43 linear height, 44 and 45
    the heights weighed as image editors weigh them, each at the dab's strength; 48 on the same,
    softly (the strength taking the texture away, not the paint); 46 its grey the dab's colour's
    lightness (as a lightness tip's), 47 its grey a colour between the colour and the secondary
    colour, mixed in at the dab's strength.
  - field 6, a second stroke along the same path whose coverage combines with the first's, as a
    dual brush's does: its tips (field 1 each), how they build up (field 2) and how it combines
    (field 3, u32, a blend mode's number, or 27 and 28 for heights).
- `procraft_brush_begin` is handed the brush's id (field 1), the colour and the secondary colour
  (fields 2 and 3, three f32, sRGB 0..1), the size the slider asks for (field 4, canvas pixels) and
  where it starts for the brush (field 10), the opacity slider (field 5), the tool (field 6, u8: 0
  paint, 1 erase, 2 smudge), the pointer (field 7, u8: 0 finger, 1 pen, 2 mouse), a seed (field 8,
  u32), how far the canvas is turned on screen (field 9, f32 radians) and the canvas's width and
  height (field 12, f32 canvas pixels); and, if its look asks
  (field 11 above), the colours under where it starts (field 11: the square's left and top, i32
  canvas pixels, its side, u32, then its pixels, RGBA8 straight, rows from the top, clear off the
  canvas).
- `procraft_brush_add` is handed samples, at most four at a time, 32 bytes each as layers' items
  hold them: x, y (canvas pixels), pressure (through the app's pressure curve), tilt, azimuth and
  roll (radians), then the milliseconds since the stroke's first sample as a 64-bit float.
- Dabs come back 96 bytes each, twenty-four 32-bit numbers: x, y, its diameter (the longer side of its
  tip, canvas pixels), its angle (radians, clockwise), its squash (0..1 across its angle, laid at
  0.05 at the least), its opacity and flow (0..1), its tip (u32), flags (u32: 1 mirrored across, 2
  mirrored down, 4 its own colour, 8 for the second stroke, 16 it moves the paint under where the
  stroke's last dab lay to where it lies, 32 that paint made one colour first, 64 that colour
  weighed by the dab's tip, 128
  the colour it mixes in clear, taking paint away, 256 what it moves laid over what is there, never
  thinning it, rather than in its place, 512 it picks up where it lies itself rather than where the
  last lay; with flag 16 too: 1024 it picks up from elsewhere, as far from where it lies as words
  20 and 21 say, as a clone does; 2048 what it picks up comes through a warp; 4096 what it picks up
  is blurred and sharpened; 8192 it picks up from the layer as it was before the stroke; 32768 with
  flags 4 and 16, its colour is as opaque as word 22 says (f32, 0..1, not with 2048 or 4096), so
  that laid in place it thins what is there towards that; 16384 it lays nothing, but asks for the
  colours under it, see below), its
  colour (three f32, sRGB 0..1, with flag 4), the texture's
  strength in it (0..1), how far the texture is moved where it reads it (x, y), how strongly a
  lightness tip shows (0..1), a threshold that makes its tip hard (0 none; 0..1: what is over it
  paints fully, what is under it by more than the next number's share of it not at all), that
  share; with flag 16, the share of what it lays that is its colour (the colour, or its own with
  flag 4) and, with flag 32, the share of its reach around where the last lay that the one colour
  is picked from (0 its middle, 1 all of it, 3 at most); then four words, 0 unless its flags say:
  with 1024, the canvas pixels across and down from where it lies to where it picks up (f32 each);
  with 2048, the warp's shift (f32 across, down), amount (f32) and kind (u32): within the dab's
  outline (r 0 at its middle, 1 at its rim, across and down its turn and squash) each pixel is taken
  from 1 / (amount (1 - r) + r) as far from the middle (1, scaled), turned by amount radians times
  1 - r (2), moved back by the shift times 1 - r (3), through a lens out from r (1 + amount r^2) of
  the radius (4) or in from r / (1 + amount r^2) (5), or from up to amount pixels away at random (6);
  16 added, the nearest pixel rather than a mix of the four around it; with 4096 (not with 2048),
  how much it is blurred (f32, 0..1, as a smudge's blur), then how much of its difference from that
  blur is added back, sharpening it (f32). With flag 16 the opacity is how far it lays what it
  moves; the first dab of such a stroke only marks its place, unless it picks up where it lies or
  from elsewhere.

- A dab with flag 16384 asks what lies under it: its layer's colours within its diameter around
  its place, each pixel weighed by 1 - (d / r)^2 at its middle (its tip word 0), or (its tip word 1)
  over the square of its diameter from its top left corner, the pixels whose corners lie within a
  pixel past its rim weighed so (less than nothing past the rim) and the rest fully. Before the
  stroke's next batch of samples the app hands `procraft_brush_under`, if the module has it, the
  colours under each dab asked about since, in order: four f32 each, the premultiplied red, green
  and blue and the alpha (0..1, clear off the canvas). They are the layer as the stroke has laid it
  by then, the dabs of the batch that asked included, since the GPU lays a batch's dabs together:
  an engine mixing the colours under its dabs into the next is a batch late at most. Strokes the
  app draws again (QuickShape, a layer's own, a 3D model, the preview) are told nothing.

A stroke's start, a batch of samples and its end may each take 40,000,000 steps, for at most a
quarter of a second; a load 2,000,000,000, for 10 seconds. A stroke laid this way shows no
prediction of where the pen goes next. The brush's preview in the library is a stroke the engine
draws, and the engine's stroke goes on through what redraws a stroke: QuickShape, a layer of its own
laying its strokes again, painting on a 3D model. A brush file the app writes keeps the engine's
name and data (in its folder `Engine`), so the brush comes back as it was.

The `brush.data` permission gives these functions, which answer while the brush studio of one of
the plugin's own brushes is open (or for the current brush, if the plugin's engine made it):

| Function | Does |
|---|---|
| `brush_data_len() -> i32` | The brush's data's length, -1 for none |
| `read_brush_data(to: i32) -> i32` | Writes the data at `to`: 1 if it did |
| `write_brush_data(from: i32, len: i32) -> i32` | The brush's data from now on: 1 if taken |

### What a brush keeps with a layer

A brush whose look says so (field 12) keeps what its strokes lay with their layer between strokes,
so that thick paint piles up and the strokes after meet it. The layer keeps it beside its pixels, in
planes the layer's size; the layer's pixels are then drawn from them. A layer without one costs
nothing, and one with it holds two more layers' worth of pixels.

Kind 1 keeps two: the paint, the colours its strokes lay before they are lit, and a height, a grey
as opaque as the paint is thick. A stroke's dabs (which move paint, flag 16) lay their paint into
the first, picking up from it, not from the layer. A dab with flag 65536 then lays its tip's grey
over the height, as opaque as word 20 says (f32, 0..1; a picture tip's grey is its red over its
alpha, a mask's a middle grey), first flattened towards a middle grey (127 of 255) as far as its
lightness strength (word 15) is under 1. Wherever its dabs reach, the layer is the paint at the
lightness the height says: the paint's own where the height is clear or a middle grey, lighter
where it is lighter, darker where it is darker, along the curve a lightness tip takes through the
paint's lightness (its grey, as far from the middle as the height is opaque). Its coverage is the
paint's.

It goes with the layer through undo and redo, and is saved with the artwork. A transform and a flip
move it with the layer. A stroke of a brush that doesn't keep the same, and any other change to the
layer's pixels (a fill, an adjustment, liquify, clearing it, merging it, pasting into it), dries the
paint: the layer stays as it shows and keeps nothing, until undoing that change brings it back.
Only a layer of plain pixels keeps anything, and only a stroke whose dabs move paint: the eraser, a
brush preview, a layer of a plugin's own and painting on a 3D model draw as they would without it.

`plugins/krita-brushes` is one to start from: it reads Krita's presets and bundles, draws Krita's
pixel brush, and edits a brush in a Krita brush studio of its own.

## Filters

A filter shows under Adjustments in a Plugins row while its plugin is on. It redraws the current layer
live, Apply keeps that as one undo step, and Cancel puts the layer back. What it did is pixels, so an
artwork never needs the plugin again.

### The function

`filter.wgsl` holds one function, and any helpers, constants or structs it needs:

```wgsl
fn pixel(colour: vec4<f32>, at: vec2<f32>) -> vec4<f32> {
    let dark = 1.0 - dot(colour.rgb, vec3<f32>(0.299, 0.587, 0.114));
    return vec4<f32>(vec3<f32>(0.0), step(v.cut, dark) * colour.a);
}
```

- `colour` is the layer's pixel, in straight colour, not premultiplied.
- `at` is where it is, in canvas pixels, top left at (0, 0). A pixel's centre is at .5.
- It returns the new pixel, in straight colour. The app clamps it to 0..1.
- It has to be called `pixel`. WGSL keeps the word `filter` for itself, so a function can't have that name.

You're given three things:

| | |
|---|---|
| `v.<key>` | Each control's value, as an `f32`. A switch is 0 or 1 |
| `layer_at(at)` | The layer as it was at `at`, straight colour. Past an edge, the edge's pixel |
| `canvas_size()` | The canvas's width and height, in pixels |

The app does the rest: premultiplying, the selection, the alpha lock and the copy of the layer. A
filter can't see the selection, and doesn't need to.

Its manifest is like the one above, with `"holds": ["filter"]`, no `hooks`, and its controls:

```json
"controls": [
  { "key": "cut", "type": "slider", "label": "Cut-off", "least": 0, "most": 1, "step": 0.01,
    "starts": 0.5, "shown_times": 100, "unit": "%", "live": true },
  { "key": "keep_colour", "type": "switch", "label": "Keep the colour", "starts": false }
]
```

### Several in one plugin

A plugin can list several filters (interface 5), each a row of its own under Adjustments, with its
code in `filters/<key>.wgsl`, the controls it reads, and the switch it comes under:

```json
"filters": [
  { "key": "outline", "label": "Outline", "controls": ["reach"], "under": "outline" },
  { "key": "posterise", "label": "Posterise", "controls": ["steps"], "under": "posterise" }
]
```

- Up to eight. Each `key` is used once, and `label` is its name under Adjustments.
- Its controls are the plugin's (sliders or switches, each once): its panel shows those, and its code
  reads them as `v.<key>`, no others.
- They show on the plugin's card, under their switches, and the panel opens at the values they have
  there. Reset goes back to those.
- One under a switch is listed only while that switch is on.
- Such a plugin has no `filter.wgsl`. It can hold code as well.

### What is refused

The file is read before anything runs. It's refused if the text is over 64 KiB, doesn't parse, or
has more than eight controls. So is anything a filter has no use for:

- Entry points, bindings, `var` at module scope, or `override`.
- Anything that reads or writes outside the function: textures, buffers, atomics, barriers.
- `discard`, and the derivatives `dpdx`, `dpdy` and `fwidth`.
- A module-scope name the app's own code uses, such as a function called `mix`, or any name
  starting with `host_`. Inside a function, a name is your own.
- A type over 16 KiB, or a function's variables over 64 KiB. Each pixel has its own, so large
  arrays stop a GPU.
- 64-bit and 16-bit numbers, which some GPUs don't have.

### How long it may take

A GPU can't be interrupted, and a shader that runs too long can stop every app on the device. So a
pixel has a budget: about 65,000 simple operations in loops, where `sin` or `pow` counts as four and
`layer_at()` as about twenty, and as much again outside them. A 31 by 31 neighbourhood fits. Every loop
pays for each round as it goes, and stops when the budget is spent.

Before a filter with a loop touches the layer, the app tries it on a few hundred pixels spread over
the layer. If one of them runs out, the plugin is switched off, saying it took too long, and the
layer isn't touched. The same goes for a draw that takes far longer than it can have, or a driver
that fails on it.

A filter with no loop is drawn in one go. One with a loop is drawn in bands, so the screen keeps
going. If a whole draw takes more than about 0.15 seconds, sliders stop following the finger and
redraw as it lets go. If it takes more than 4 seconds, the filter is closed and the layer put back.

## When a plugin stops

It's switched off at once, all of it, and its card says why. It stays off through a restart until
someone taps Turn on. An action that stops partway leaves what it did so far as one step to undo.

Code stops if it runs out of steps, asks for too much memory, traps, or gives an answer that makes no
sense. Whatever it was handed goes on as it came.

## Building it

1. Get a copy of this repository: `git clone https://github.com/artcraft-android/procraft`, or
   Code > Download ZIP on its GitHub page. It stays on your computer; nothing you do in it comes
   back here.
2. For code, install Rust (from rustup.rs). Python 3 runs the packer.
3. Put your plugin's folder under `plugins/` in that copy, then, from the copy's top folder:

```
rustup target add wasm32-unknown-unknown
python tools/pack_plugin.py nudge --out somewhere
```

That writes `nudge.procraft-plugin`. Only code needs the `rustup` line. To try it, import it through
the Plugins page, and tap Turn on if it starts off. A file packed again from the same folder comes out
the same, byte for byte.

## The plugins that come with the app

Every plugin under `plugins/` comes inside the app, packed, in `crates/core/bundled-plugins/`, and
`crates/core/src/bundled.rs` lists them. The first time the app comes with one, it adds it as if
someone had imported the file, but off, whatever it holds. From then on it's a plugin like any
other, with the same card, Turn on and Remove.

- The app writes down in its plugin folder (`added`) each one it added, so one that's removed stays
  removed, through restarts and updates. Importing its file brings it back, as any import.
- A later version of the app adds any it never added before, and puts a newer file in place of an
  older one that's there, keeping what was set, as an import of it would.
- One imported before the app came with it is left as it is, but for a newer file.
- A setting the app used to have and a plugin now `keeps` (ColorDrop tools' gap, Interface's size,
  Layer tools' eyedropper and paste switches) starts the plugin the app added on with it, if someone
  had changed it, and the switch it comes under. Once the plugin is turned on or off, that is what's
  kept.

### Taking the place of other plugins

A plugin can take the place of others, such as several small ones made into one, by their ids:

```json
"replaces": [
  { "id": "org.example.size", "switch": "size_on", "controls": { "clear": "clear" } },
  { "id": "org.example.notices", "switch": "notices" }
]
```

- Each id is beside the plugin's own: the same but for its last part. Up to sixteen.
- `switch` is the key of one of its switches, and `controls` maps the keys of the old one's controls
  to the keys of its own that take their values. Both can be left out. A control that `keeps` a
  setting the app used to have takes it as the old one did, under the same key.
- When it comes in, by an import or with the app, or on a start that finds both, the old ones go from
  the store at once, and it takes over what they had set: it's on if any of them was, with the switch
  of each that was, and their controls' values. The import screen names those it takes the place of.
- What was kept for the old ones, on a start, goes the same way until it has values of its own, which
  then say instead, read before or after. Removed, it takes what was kept for them with it.
- An old one can't be imported while it is here.

The packed files are committed, so building the app needs no WebAssembly target. Whenever anything
under `plugins/` changes, pack them again and commit what changed:

```
python tools/pack_plugin.py --bundle
```

That also writes `sources.txt` beside them, a digest of each folder as it was packed. The core's
tests (`cargo test -p procraft-core bundled`) check each packed file against its folder, the digest
too, so a change not packed again fails them. A new plugin needs its line in `bundled.rs` as well.

To ship the app without them, take out `bundled.rs` (its test with it), its `mod` line and the folder,
and have `Plugins::with_bundled` add none. What was added stays, as if imported.

## When the app changes

`interface` in `compat` is the version of all this: the hooks and what they call, the template around
a filter's function, what a filter is given, the kinds of control, and the app's functions. Interface
2 added actions, permissions, panels, the picture of the `picker` hook and kinds of artwork; later,
within it, the `key_shortcut` hook, panels that show keys or ask for a key press, and what a plugin
keeps. Interface 3 added kinds of layer, interface 4 brush engines, the `brush.data` permission and
the utilities, interface 5 several filters in one plugin, and interface 6 up to sixteen controls,
a picker's picture given its controls, a picker that goes quiet or picks the hue on a ring of its
own, a switch under a switch, and the `interface_part_scale` and `floating_picker_only` hooks.
What comes under a switch at the top of the list, and taking the place of other plugins, need no
newer interface: an older app shows every control, and leaves the others where they are. What a
hook calls stays as it's written here, and the app keeps every interface's template, so a plugin
goes on working as it was written.

Something that needs more uses a newer interface number, and an older app says it needs a newer
ProCraft. So does an app that doesn't have a hook a plugin lists, or something `holds` names.
