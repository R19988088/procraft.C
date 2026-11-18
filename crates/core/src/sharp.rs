//! Plugins' layers drawn sharp when zoomed in. Their items are strokes kept as strokes, so once the
//! view settles magnified, the part of the canvas on screen is drawn again at the screen's
//! resolution on a canvas of its own: each plugin layer's items laid again there as much bigger (the
//! same stamps, scaled), every other layer's pixels redrawn there from the canvas's as the screen
//! shows them, and the whole composited as the canvas is (blend modes, groups, masks, clipping). That
//! picture shows over the canvas in its place. Anything that changes the canvas or the view takes it
//! away at once, and it is made again when things settle. The canvas's own pixels stay as they are:
//! saving, exporting, thumbnails and picking colours read those.
//!
//! A layer of many items takes longer to lay again than a frame lasts, so the picture is made a
//! slice at a time, a frame each ([`SLICE`]), and shows once it is whole: the canvas keeps drawing
//! and taking input meanwhile, and a change or a move of the view leaves what was begun. The canvas
//! it is made on is the target's size, the picture at its top left, and is kept from one to the
//! next: zooming in and out doesn't make a canvas for each picture.

use std::sync::Arc;
use std::time::{Duration, Instant};

use procraft_engine::brush::Brush;
use procraft_engine::document::{Document, LayerId};
use procraft_engine::geom::{Affine, Vec2};
use procraft_gpu::{Canvas, Resample};

use crate::State;
use crate::plugin_layers::Item;

/// Zoomed in to this many screen pixels a canvas pixel or more, plugins' layers are drawn sharp.
pub(crate) const SHARP_FROM: f32 = 1.5;
/// How long the canvas and the view stay as they are before the sharp picture is made.
const SETTLE: Duration = Duration::from_millis(120);
/// How long a frame spends laying items on the sharp picture's canvas, at most (and at least one).
const SLICE: Duration = Duration::from_millis(6);
/// The most pixels the sharp picture may have, and the most its canvas's layers may take.
const MOST_PIXELS: u64 = 24 << 20;
const MOST_BYTES: u64 = 384 << 20;

/// What the sharp picture shows: the canvas as composited (by its count of composites) through
/// the view, onto a target of a size, its pixels crisp or smoothed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Shown {
    to_screen: [f32; 6],
    size: (u32, u32),
    composites: u64,
    crisp: bool,
}

/// The canvas the sharp picture is made on, kept for the next.
pub(crate) struct SharpCanvas {
    canvas: Canvas,
    /// Its layers, as the artwork's were when it was made.
    layers: Vec<LayerId>,
}

/// A sharp picture being made, a slice a frame: what it is for, its canvas (lent to the session
/// while items are laid on it) and document, and how far along it is.
pub(crate) struct Making {
    key: Shown,
    canvas: Option<Canvas>,
    layers: Vec<LayerId>,
    doc: Document,
    /// Canvas pixels to the sharp canvas's, and back.
    to_sharp: Affine,
    from_sharp: Affine,
    /// The canvas rectangle it shows (left, top, width, height in canvas pixels), its size, and
    /// the canvas pixels it reaches.
    rect: [f32; 4],
    size: (u32, u32),
    visible: procraft_engine::geom::IRect,
    /// How raster layers' pixels are redrawn on it.
    resample: Resample,
    /// The layers shown, in order, and which is next.
    shown: Vec<LayerId>,
    at: usize,
    /// The plugin layer being laid: the items of it that reach the picture, whether it has been
    /// begun (held and wiped), and the next item.
    items: Option<Vec<Item>>,
    begun: bool,
    next: usize,
    /// The items' brushes, made once.
    brushes: Vec<(Arc<Vec<u8>>, Brush)>,
}

/// The sharp picture's state.
#[derive(Default)]
pub(crate) struct Sharp {
    held: Option<SharpCanvas>,
    shown: Option<Shown>,
    /// What showed last, and since when: made once it has shown so a while.
    pending: Option<(Shown, Instant)>,
    /// What it couldn't be made for (too big, say): not tried again until something changes.
    failed: Option<Shown>,
    /// The picture being made, a slice a frame.
    making: Option<Making>,
    /// How long a slice is, if not [`SLICE`] (the tests').
    #[cfg(test)]
    pub(crate) slice: Option<Duration>,
    /// How many sharp pictures were made, and how many canvases to make them on.
    pub(crate) made: u64,
    pub(crate) canvases: u64,
}

impl Sharp {
    /// How long a frame spends laying items.
    fn slice(&self) -> Duration {
        #[cfg(test)]
        if let Some(slice) = self.slice {
            return slice;
        }
        SLICE
    }
}

/// Where the part of the canvas on a target shows: the canvas rectangle's top left and the size
/// (the sharp canvas's pixels) at `scale` of them a canvas pixel. None if none of it shows.
fn region(to_screen: Affine, size: (u32, u32), canvas: (u32, u32)) -> Option<(Vec2, (u32, u32), f32)> {
    let back = to_screen.inverse()?;
    let (w, h) = (size.0 as f32, size.1 as f32);
    let corners = [Vec2::new(0.0, 0.0), Vec2::new(w, 0.0), Vec2::new(w, h), Vec2::new(0.0, h)].map(|p| back.apply(p));
    let (cw, ch) = (canvas.0 as f32, canvas.1 as f32);
    let x0 = corners.iter().map(|p| p.x).fold(f32::INFINITY, f32::min).max(0.0);
    let y0 = corners.iter().map(|p| p.y).fold(f32::INFINITY, f32::min).max(0.0);
    let x1 = corners.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max).min(cw);
    let y1 = corners.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max).min(ch);
    if !(x0 < x1 && y0 < y1) {
        return None;
    }
    let scale = to_screen.scale_factor();
    // unturned, its pixels are the screen's: its top left on a screen pixel (but at the canvas's
    // own edge)
    let straight = to_screen.b.abs() < 1e-6 && to_screen.c.abs() < 1e-6 && to_screen.a > 0.0 && to_screen.d > 0.0;
    let snap =
        |at: f32, a: f32, e: f32| if straight && at > 0.0 { (((a * at + e).floor() - e) / a).max(0.0) } else { at };
    let origin = Vec2::new(snap(x0, to_screen.a, to_screen.e), snap(y0, to_screen.d, to_screen.f));
    // to the canvas's far edges and no further (a sliver short shows the canvas beneath)
    let across = |from: f32, to: f32, edge: bool| {
        let n = (to - from) * scale;
        (if edge { n.floor() } else { n.ceil() }).max(1.0) as u32
    };
    let size = (across(origin.x, x1, x1 >= cw), across(origin.y, y1, y1 >= ch));
    Some((origin, size, scale))
}

impl State {
    /// Keeps the sharp picture over the canvas as it should be this frame, drawn into a target of
    /// `size` through `to_screen`: taken away as anything changes, made again once things settle
    /// (a frame asked for then), a slice a frame.
    pub(crate) fn keep_sharp(&mut self, size: (u32, u32), to_screen: Affine) {
        let crisp = to_screen.scale_factor() >= self.display_scale * 0.999;
        let wanted = self.sharp_wanted(to_screen);
        let Some(canvas) = self.canvas.as_ref() else { return };
        let quiet = !canvas.changes_queued() && !canvas.busy();
        let key = Shown {
            to_screen: [to_screen.a, to_screen.b, to_screen.c, to_screen.d, to_screen.e, to_screen.f],
            size,
            composites: canvas.composites(),
            crisp,
        };
        if !wanted || !quiet {
            self.drop_sharp(!wanted);
            // what is under way ends, and it settles again: a frame then to look
            if wanted {
                self.sharp.pending = None;
                self.ask_frame_at(Instant::now() + SETTLE);
            }
            return;
        }
        if self.sharp.shown == Some(key) || self.sharp.failed == Some(key) {
            return;
        }
        // being made for what shows now: on with it
        if let Some(making) = self.sharp.making.take() {
            if making.key == key {
                self.go_on_making(making);
                return;
            }
            self.leave_making(making);
        }
        self.drop_sharp(false);
        match self.sharp.pending {
            Some((pending, since)) if pending == key && since.elapsed() >= SETTLE => {
                self.sharp.pending = None;
                match self.begin_sharp(key, to_screen) {
                    Some(making) => self.go_on_making(making),
                    None => self.sharp.failed = Some(key),
                }
            }
            Some((pending, since)) if pending == key => {
                self.ask_frame_at(since + SETTLE);
            }
            _ => {
                let now = Instant::now();
                self.sharp.pending = Some((key, now));
                self.ask_frame_at(now + SETTLE);
            }
        }
    }

    /// Another frame by `at`.
    fn ask_frame_at(&mut self, at: Instant) {
        self.frame_due = Some(self.frame_due.map_or(at, |due| due.min(at)));
    }

    /// Takes the sharp picture away, and what was being made (and with `all`, the canvas it is
    /// made on).
    fn drop_sharp(&mut self, all: bool) {
        if self.sharp.shown.take().is_some()
            && let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref())
        {
            canvas.set_sharp(gpu, None);
        }
        if let Some(making) = self.sharp.making.take() {
            self.leave_making(making);
        }
        if all {
            self.sharp.held = None;
            self.sharp.pending = None;
        }
    }

    /// Whether plugins' layers would show sharper through `to_screen` now: zoomed in, one of them
    /// showing, nothing under way that the canvas shows as it goes, and the artwork composited as
    /// it plainly is (its background shown, no animation's frames).
    fn sharp_wanted(&self, to_screen: Affine) -> bool {
        let plain = self.doc.background_visible
            && self.doc.animation.is_none()
            && self.stroke.is_none()
            && self.recording.is_none()
            && self.shape.is_none()
            && self.transform.is_none()
            && !self.adjusting
            && self.reveal.is_none()
            && self.view_motion.is_none()
            && !self.scene_shown()
            && self.sphere.is_none();
        plain
            && to_screen.scale_factor() >= SHARP_FROM
            && self.doc.layers().iter().any(|l| l.plugin.is_some() && self.sharp_items(l.id))
    }

    /// Whether `layer` is a plugin's layer shown whose items can be laid again.
    fn sharp_items(&self, layer: LayerId) -> bool {
        self.doc.layer(layer).is_some_and(|l| l.plugin.as_ref().is_some_and(|p| p.data.is_some()))
            && self.doc.unseen(layer).is_none()
            && !self.plugin_layer_off(layer)
    }

    /// Begins the sharp picture `key` says (a target of its size through `to_screen`, raster
    /// layers' pixels crisp or smoothed as the screen shows them): its canvas, and the layers to
    /// draw on it. None if it can't be made.
    fn begin_sharp(&mut self, key: Shown, to_screen: Affine) -> Option<Making> {
        let (origin, (w, h), scale) = region(to_screen, key.size, (self.doc.width(), self.doc.height()))?;
        let gpu = self.gpu.clone()?;
        let layers: Vec<LayerId> = self.doc.pixel_layer_ids().collect();
        // the canvas it is made on, the picture its top left: the target's size and a pixel or two
        // (a picture's edges round out), or the picture's if bigger (the view turned), so zooming
        // doesn't make one of every size; one kept is used again while it is as big and has these
        // layers
        let kept = self.sharp.held.take().filter(|k| {
            let (kw, kh) = k.canvas.size();
            kw >= w && kh >= h && k.layers == layers
        });
        let fits = (w.max(key.size.0.saturating_add(2)), h.max(key.size.1.saturating_add(2)));
        let (cw, ch) = kept.as_ref().map_or(fits, |k| k.canvas.size());
        let pixels = u64::from(cw) * u64::from(ch);
        let most = gpu.max_texture_size();
        if pixels > MOST_PIXELS || cw > most || ch > most || pixels * 4 * (layers.len() as u64 + 8) > MOST_BYTES {
            return None;
        }
        let doc = self.doc.resized(cw, ch);
        let mut canvas = match kept {
            Some(kept) => kept.canvas,
            None => match Canvas::new(&gpu, &doc) {
                Ok(canvas) => {
                    self.sharp.canvases += 1;
                    canvas
                }
                Err(e) => {
                    log::warn!("the sharp picture's canvas: {e}");
                    return None;
                }
            },
        };
        let to_sharp = Affine { a: scale, b: 0.0, c: 0.0, d: scale, e: -origin.x * scale, f: -origin.y * scale };
        let from_sharp = to_sharp.inverse()?;
        let visible = procraft_engine::geom::IRect::new(
            origin.x.floor() as i32,
            origin.y.floor() as i32,
            (origin.x + w as f32 / scale).ceil() as i32,
            (origin.y + h as f32 / scale).ceil() as i32,
        );
        canvas.draw_scaled([origin.x, origin.y], scale);
        // each plugin layer's items that reach it laid there, and every other layer's pixels
        // redrawn there as shown
        let shown = layers.iter().copied().filter(|id| self.doc.unseen(*id).is_none()).collect();
        Some(Making {
            key,
            canvas: Some(canvas),
            layers,
            doc,
            to_sharp,
            from_sharp,
            rect: [origin.x, origin.y, w as f32 / scale, h as f32 / scale],
            size: (w, h),
            visible,
            resample: if key.crisp { Resample::Nearest } else { Resample::Bilinear },
            shown,
            at: 0,
            items: None,
            begun: false,
            next: 0,
            brushes: Vec::new(),
        })
    }

    /// Goes on making `making` for a slice of a frame, and shows the picture once it is whole (a
    /// frame asked for at once till then).
    fn go_on_making(&mut self, mut making: Making) {
        let slice = self.sharp.slice();
        let done = self.make_sharp(&mut making, slice);
        // the canvas's layers read for it were brought up to date: as it is now, it goes on
        let composites = self.canvas.as_ref().map_or(0, Canvas::composites);
        making.key.composites = composites;
        match done {
            Some(false) => {
                self.sharp.making = Some(making);
                self.needs_frame = true;
            }
            Some(true) => {
                let key = making.key;
                if self.show_made(making) {
                    self.sharp.shown = Some(key);
                    self.sharp.made += 1;
                } else {
                    self.sharp.failed = Some(key);
                }
            }
            None => {
                self.sharp.failed = Some(making.key);
                self.leave_making(making);
            }
        }
    }

    /// Draws `making`'s layers in turn on its canvas for `slice` (a layer's pixels, or an item of a
    /// plugin's layer, at least): true once all are, none if it can't go on.
    fn make_sharp(&mut self, making: &mut Making, slice: Duration) -> Option<bool> {
        let started = Instant::now();
        let gpu = self.gpu.clone()?;
        let mut done = true;
        let mut worked = false;
        while let Some(&id) = making.shown.get(making.at) {
            if worked && started.elapsed() >= slice {
                done = false;
                break;
            }
            if making.items.is_none() {
                if self.sharp_items(id)
                    && let Some(all) = self.layer_items(id)
                {
                    making.items = Some(self.items_reaching(id, all, making.visible));
                    (making.begun, making.next) = (false, 0);
                } else {
                    let (main, canvas) = (self.canvas.as_mut()?, making.canvas.as_mut()?);
                    if let Some(view) = main.layer_to_read(&gpu, &mut self.doc, id) {
                        canvas.redraw_from(id, view, main.size(), making.from_sharp, making.resample);
                    }
                    making.at += 1;
                    worked = true;
                    continue;
                }
            }
            if !self.lay_sharp(making, id, started, slice) {
                done = false;
                break;
            }
            (making.items, making.at, worked) = (None, making.at + 1, true);
        }
        // what was queued on it drawn now, a slice at a time
        if let Some(canvas) = making.canvas.as_mut() {
            canvas.render(&gpu, &mut making.doc, None);
        }
        Some(done)
    }

    /// Lays the items of plugin layer `id` that `making` holds in turn on its canvas, till the
    /// slice begun at `started` is over (one at least): true once all are.
    fn lay_sharp(&mut self, making: &mut Making, id: LayerId, started: Instant, slice: Duration) -> bool {
        // laid there as here, the artwork and its canvas lent to it meanwhile
        std::mem::swap(&mut self.doc, &mut making.doc);
        std::mem::swap(&mut self.canvas, &mut making.canvas);
        self.lay_to = Some(making.to_sharp);
        if !making.begun {
            if let Some(canvas) = self.canvas.as_mut() {
                canvas.hold_layer_quietly(id);
                canvas.wipe_layer(id);
            }
            making.begun = true;
        }
        let mut done = true;
        let mut laid = false;
        while let Some(item) = making.items.as_ref().and_then(|items| items.get(making.next)) {
            if laid && started.elapsed() >= slice {
                done = false;
                break;
            }
            self.lay_clipped(id, item, &mut making.brushes);
            making.next += 1;
            laid = true;
        }
        if done && let Some(canvas) = self.canvas.as_mut() {
            canvas.release_layer(true, None);
            making.begun = false;
        }
        self.lay_to = None;
        std::mem::swap(&mut self.canvas, &mut making.canvas);
        std::mem::swap(&mut self.doc, &mut making.doc);
        done
    }

    /// Shows the picture `making` made over the canvas, where it is on it, and keeps its canvas
    /// for the next: false if it couldn't.
    fn show_made(&mut self, making: Making) -> bool {
        let Making { canvas, layers, rect, size, .. } = making;
        let (Some(mut canvas), Some(gpu)) = (canvas, self.gpu.clone()) else { return false };
        canvas.forget_history();
        if let Some(main) = self.canvas.as_mut() {
            main.set_sharp(&gpu, Some((canvas.composite_texture(), size, rect)));
        }
        self.sharp.held = Some(SharpCanvas { canvas, layers });
        true
    }

    /// Leaves the picture `making` was making unfinished: its canvas kept for the next (the layer
    /// it was laying let go of, and what that kept forgotten).
    fn leave_making(&mut self, making: Making) {
        let Making { canvas, layers, mut doc, begun, .. } = making;
        let (Some(mut canvas), Some(gpu)) = (canvas, self.gpu.as_ref()) else { return };
        if begun {
            canvas.release_layer(true, None);
            canvas.render(gpu, &mut doc, None);
        }
        canvas.forget_history();
        self.sharp.held = Some(SharpCanvas { canvas, layers });
    }
}

#[cfg(test)]
impl Sharp {
    /// Whether it holds no canvas to make the picture on.
    pub(crate) fn held_none(&self) -> bool {
        self.held.is_none() && self.making.is_none()
    }

    /// Whether a picture is being made.
    pub(crate) fn making(&self) -> bool {
        self.making.is_some()
    }
}
