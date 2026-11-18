//! Crop and Resize as it is edited, the same in every app: the rectangle kept (canvas pixels), its
//! turn, the size it becomes (resampled, or the rectangle's), the DPI, and the rules a drag on the
//! frame follows: which part a press takes, snapping to the canvas's sides and middle, a locked
//! aspect following the width. The app draws the frame and the settings; Done crops (crop.rs).

use procraft_engine::document::canvas_edge_most;
use procraft_engine::geom::Vec2;

use crate::CanvasSession;

/// The canvas fitted to the screen and shrunk this much while cropping, so the frame's handles are
/// clear of the edges.
const CROP_SCALE: f32 = 0.597;
/// How near (points) a corner or a side's middle a press takes it, and how near (points) the frame
/// snaps.
const REACH: f32 = 28.0;
const SNAP: f32 = 10.0;
/// The size readout by the touch while a corner or side is dragged (points): its size, how far
/// above the touch its top is (twice its height), how far beside the touch it starts, as parts of
/// its width (right of the frame's middle, and this far past it, to the touch's right; else to its
/// left), and how near the window's sides it comes.
const CROP_READOUT_WIDTH: f32 = 148.0;
const CROP_READOUT_HEIGHT: f32 = 54.0;
const READOUT_RIGHT: f32 = 0.2;
const READOUT_LEFT: f32 = -1.2;
const READOUT_PAST_MIDDLE: f32 = 37.5;
const READOUT_MARGIN: f32 = 10.0;

/// Which part of the frame a drag takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Grab {
    Move,
    Left,
    Top,
    Right,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// The crop as its frame and settings show it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CropShown {
    /// The rectangle kept, canvas pixels.
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Its turn, radians.
    pub angle: f32,
    pub resample: bool,
    /// The size what is kept becomes, pixels.
    pub size_width: u32,
    pub size_height: u32,
    pub aspect_locked: bool,
    pub snapping: bool,
    pub dpi: f32,
    /// The frame's corners in the window (x, y each: top left, top right, bottom right, bottom
    /// left), turned with the crop.
    pub corners: Vec<f32>,
    /// While a corner or side is dragged: the size readout by the touch.
    pub readout: Option<CropReadout>,
}

/// The size a corner or side being dragged frames, as it lies on the screen (the frame's width and
/// height in canvas pixels, whole, swapped on a canvas shown a quarter turn round), and where the
/// readout showing it goes: its top left corner, window pixels.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct CropReadout {
    pub width: u32,
    pub height: u32,
    pub x: f32,
    pub y: f32,
}

#[derive(Default)]
pub(crate) struct CropEdit {
    /// Left, top, right, bottom; none outside Crop and Resize.
    rect: Option<[f32; 4]>,
    angle: f32,
    resample: bool,
    size: (u32, u32),
    aspect_locked: bool,
    snapping: bool,
    dpi: f32,
    /// A drag on the frame: what it took, the rectangle then, where it began (canvas pixels) and
    /// how far (canvas pixels) the frame snaps; and where the touch is now (window pixels).
    drag: Option<(Grab, [f32; 4], Vec2, f32)>,
    touch: Vec2,
    /// Why the last crop couldn't be made, until the app takes it to tell, and whether the GPU's
    /// device being lost was told.
    pub(crate) failure: Option<String>,
    pub(crate) lost_told: bool,
}

impl CropEdit {
    /// The frame's corners in canvas pixels, turned with the crop.
    fn corners(&self, [l, t, r, b]: [f32; 4]) -> [Vec2; 4] {
        let c = Vec2::new((l + r) / 2.0, (t + b) / 2.0);
        let (s, k) = self.angle.sin_cos();
        [Vec2::new(l, t), Vec2::new(r, t), Vec2::new(r, b), Vec2::new(l, b)].map(|p| {
            let d = p - c;
            Vec2::new(c.x + d.x * k - d.y * s, c.y + d.x * s + d.y * k)
        })
    }

    /// Where the frame goes when the drag reaches `at` (canvas pixels) on a canvas `canvas` big.
    fn dragged(&self, at: Vec2, canvas: (f32, f32)) -> Option<[f32; 4]> {
        let (grab, start, from, snap) = self.drag?;
        let d = at - from;
        let [mut l, mut t, mut r, mut b] = start;
        match grab {
            Grab::Move => {
                l += d.x;
                r += d.x;
                t += d.y;
                b += d.y;
            }
            Grab::Left => l += d.x,
            Grab::Right => r += d.x,
            Grab::Top => t += d.y,
            Grab::Bottom => b += d.y,
            Grab::TopLeft => {
                l += d.x;
                t += d.y;
            }
            Grab::TopRight => {
                r += d.x;
                t += d.y;
            }
            Grab::BottomLeft => {
                l += d.x;
                b += d.y;
            }
            Grab::BottomRight => {
                r += d.x;
                b += d.y;
            }
        }
        // snapping: the frame's sides to the canvas's sides and middle
        if self.snapping {
            let snap_to = |v: f32, targets: [f32; 3]| targets.into_iter().find(|&s| (s - v).abs() < snap).unwrap_or(v);
            let xs = [0.0, canvas.0 / 2.0, canvas.0];
            let ys = [0.0, canvas.1 / 2.0, canvas.1];
            if grab == Grab::Move {
                let (dl, dr) = (snap_to(l, xs) - l, snap_to(r, xs) - r);
                let dx = if dl != 0.0 { dl } else { dr };
                l += dx;
                r += dx;
                let (dt, db) = (snap_to(t, ys) - t, snap_to(b, ys) - b);
                let dy = if dt != 0.0 { dt } else { db };
                t += dy;
                b += dy;
            } else {
                (l, r, t, b) = (snap_to(l, xs), snap_to(r, xs), snap_to(t, ys), snap_to(b, ys));
            }
        }
        // a locked aspect follows the width
        let (sw, sh) = (start[2] - start[0], start[3] - start[1]);
        if self.aspect_locked && grab != Grab::Move && sw > 0.0 {
            let h = (r - l) * sh / sw;
            if matches!(grab, Grab::Top | Grab::TopLeft | Grab::TopRight) {
                t = b - h;
            } else {
                b = t + h;
            }
        }
        // unturned and not resampled, the sides are whole canvas pixels: what is kept is a plain
        // copy (moved, the frame keeps its size)
        if self.whole_pixels() {
            if grab == Grab::Move {
                let (w, h) = ((r - l).round(), (b - t).round());
                (l, t) = (l.round(), t.round());
                (r, b) = (l + w, t + h);
            } else {
                (l, t, r, b) = (l.round(), t.round(), r.round(), b.round());
            }
        }
        (r - l >= 1.0 && b - t >= 1.0).then_some([l, t, r, b])
    }

    /// Whether the frame's sides are kept to whole canvas pixels: while it is unturned and what
    /// it keeps isn't resampled.
    fn whole_pixels(&self) -> bool {
        self.angle == 0.0 && !self.resample
    }

    /// The readout while a corner or side is dragged: the frame's size as `turns` (quarter turns
    /// the canvas shows at) lays it on the screen, placed above the touch and beside it, away from
    /// the frame's middle (`middle`, window pixels), within a window `window_width` wide; `scale`
    /// window pixels a point.
    fn readout(&self, middle: Vec2, turns: u8, window_width: f32, scale: f32) -> Option<CropReadout> {
        let (grab, ..) = self.drag?;
        let [l, t, r, b] = self.rect?;
        if grab == Grab::Move {
            return None;
        }
        let (w, h) = ((r - l).max(0.0) as u32, (b - t).max(0.0) as u32);
        let (width, height) = if turns % 2 == 1 { (h, w) } else { (w, h) };
        let (box_w, box_h) = (CROP_READOUT_WIDTH * scale, CROP_READOUT_HEIGHT * scale);
        let at = self.touch;
        let right = at.x - middle.x - READOUT_PAST_MIDDLE * scale > 0.0;
        let x = at.x + box_w * if right { READOUT_RIGHT } else { READOUT_LEFT };
        let margin = READOUT_MARGIN * scale;
        let x = x.max(margin).min((window_width - box_w - margin).max(margin));
        let y = at.y - 2.0 * box_h;
        (x.is_finite() && y.is_finite()).then_some(CropReadout { width, height, x, y })
    }

    fn set_rect(&mut self, [l, t, r, b]: [f32; 4]) {
        let (w, h) = ((r - l).max(1.0), (b - t).max(1.0));
        self.rect = Some([l, t, l + w, t + h]);
        if !self.resample {
            self.size = (w.round() as u32, h.round() as u32);
        }
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Starts Crop and Resize (again): the whole canvas kept, unturned, at its size, the canvas
    /// fitted to the window and shrunk so the frame's handles are clear. Aspect lock and snapping
    /// stay as they were.
    pub fn crop_begin(&self) {
        self.fit_canvas_scaled(CROP_SCALE);
        let mut st = self.shared.lock();
        let (w, h, dpi) = (st.doc.width(), st.doc.height(), st.doc.dpi);
        let e = &mut st.crop_edit;
        e.rect = Some([0.0, 0.0, w as f32, h as f32]);
        e.angle = 0.0;
        e.resample = false;
        e.size = (w, h);
        e.dpi = dpi;
        e.drag = None;
    }

    /// The crop as it stands; none outside Crop and Resize.
    pub fn crop_shown(&self) -> Option<CropShown> {
        let st = self.shared.lock();
        let e = &st.crop_edit;
        let [l, t, r, b] = e.rect?;
        let corners = st.view.map_or_else(Vec::new, |v| {
            e.corners([l, t, r, b])
                .into_iter()
                .flat_map(|p| {
                    let w = v.to_screen().apply(p);
                    [w.x, w.y]
                })
                .collect()
        });
        let middle =
            corners.as_chunks::<2>().0.iter().fold(Vec2::new(0.0, 0.0), |m, &[x, y]| m + Vec2::new(x, y) * 0.25);
        let readout = e.readout(middle, st.shown_turns(), st.window_size.0 as f32, st.interface_scale());
        Some(CropShown {
            x: l,
            y: t,
            width: r - l,
            height: b - t,
            angle: e.angle,
            resample: e.resample,
            size_width: e.size.0,
            size_height: e.size.1,
            aspect_locked: e.aspect_locked,
            snapping: e.snapping,
            dpi: e.dpi,
            corners,
            readout,
        })
    }

    /// A press on the frame at window point (`x`, `y`): near a corner or a side's middle it takes
    /// that, anywhere else the whole frame.
    pub fn crop_press(&self, x: f32, y: f32) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(rect) = st.crop_edit.rect else { return };
        let (Some(view), Some(from)) = (st.view, st.window_to_canvas(x, y)) else { return };
        let reach = REACH * st.interface_scale();
        let at = Vec2::new(x, y);
        let cs = st.crop_edit.corners(rect).map(|p| view.to_screen().apply(p));
        let mid = |a: Vec2, b: Vec2| Vec2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
        let near = |p: Vec2| (p - at).length() < reach;
        let [tl, tr, br, bl] = cs;
        let grab = if near(tl) {
            Grab::TopLeft
        } else if near(tr) {
            Grab::TopRight
        } else if near(br) {
            Grab::BottomRight
        } else if near(bl) {
            Grab::BottomLeft
        } else if near(mid(tl, bl)) {
            Grab::Left
        } else if near(mid(tl, tr)) {
            Grab::Top
        } else if near(mid(tr, br)) {
            Grab::Right
        } else if near(mid(br, bl)) {
            Grab::Bottom
        } else {
            Grab::Move
        };
        // canvas pixels a few window points cover, for snapping
        let snap = st.window_to_canvas(x + SNAP * st.display_scale, y).map_or(0.0, |p| (p - from).length());
        st.crop_edit.drag = Some((grab, rect, from, snap));
        st.crop_edit.touch = at;
    }

    /// The drag on the frame reached window point (`x`, `y`).
    pub fn crop_drag(&self, x: f32, y: f32) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(at) = st.window_to_canvas(x, y) else { return };
        if x.is_finite() && y.is_finite() {
            st.crop_edit.touch = Vec2::new(x, y);
        }
        let canvas = (st.doc.width() as f32, st.doc.height() as f32);
        if let Some(rect) = st.crop_edit.dragged(at, canvas) {
            st.crop_edit.set_rect(rect);
        }
    }

    pub fn crop_release(&self) {
        self.shared.lock().crop_edit.drag = None;
    }

    /// A width or height typed in: resampling, it is the new size; otherwise the kept
    /// rectangle's. A locked aspect keeps the other in step.
    pub fn crop_set_size(&self, width: Option<u32>, height: Option<u32>) {
        let mut st = self.shared.lock();
        let e = &mut st.crop_edit;
        let Some([l, t, _, _]) = e.rect else { return };
        let (w0, h0) = e.size;
        let most = canvas_edge_most();
        let mut w = width.unwrap_or(w0).clamp(1, most);
        let mut h = height.unwrap_or(h0).clamp(1, most);
        if e.aspect_locked && w0 > 0 && h0 > 0 {
            if width.is_some() {
                h = ((w as f32 * h0 as f32 / w0 as f32).round() as u32).clamp(1, most);
            } else if height.is_some() {
                w = ((h as f32 * w0 as f32 / h0 as f32).round() as u32).clamp(1, most);
            }
        }
        e.size = (w, h);
        if !e.resample {
            e.rect = Some([l, t, l + w as f32, t + h as f32]);
        }
    }

    pub fn crop_set_dpi(&self, dpi: f32) {
        if dpi.is_finite() && dpi >= 1.0 {
            self.shared.lock().crop_edit.dpi = dpi.min(9999.0);
        }
    }

    /// The crop's turn, radians.
    pub fn crop_set_angle(&self, radians: f32) {
        if radians.is_finite() {
            self.shared.lock().crop_edit.angle = radians;
        }
    }

    /// Resampling on: what is kept is scaled to the size; off, the canvas is the rectangle's size.
    pub fn crop_set_resample(&self, on: bool) {
        let mut st = self.shared.lock();
        let e = &mut st.crop_edit;
        e.resample = on;
        if let Some([l, t, r, b]) = e.rect {
            e.size = ((r - l).round() as u32, (b - t).round() as u32);
        }
    }

    pub fn crop_set_snapping(&self, on: bool) {
        self.shared.lock().crop_edit.snapping = on;
    }

    pub fn crop_set_aspect_locked(&self, on: bool) {
        self.shared.lock().crop_edit.aspect_locked = on;
    }

    /// Ends Crop and Resize: `keep` crops as set (and sets the DPI), else the canvas goes back as
    /// it was; either way it is fitted to the window again (cancelled, easing back). True if the
    /// canvas changed; false too if it couldn't be cropped ([`CanvasSession::take_failure`]).
    pub fn crop_end(&self, keep: bool) -> bool {
        let (rect, angle, resample, size, dpi) = {
            let mut st = self.shared.lock();
            let e = &mut st.crop_edit;
            let Some(rect) = e.rect.take() else { return false };
            e.drag = None;
            (rect, e.angle, e.resample, e.size, e.dpi)
        };
        if !keep {
            self.ease_to_fit();
            return false;
        }
        self.fit_view();
        let (canvas_dpi, cw, ch) = {
            let st = self.shared.lock();
            (st.doc.dpi, st.doc.width(), st.doc.height())
        };
        let [l, t, r, b] = rect;
        let (w, h) = (r - l, b - t);
        // unturned and not resampled: a plain copy of whole pixels (turned back or resampled
        // and back since the last drag)
        let (l, t, w, h) =
            if angle == 0.0 && !resample { (l.round(), t.round(), w.round(), h.round()) } else { (l, t, w, h) };
        // nothing to crop: the whole canvas, unturned, at its size
        let same = l == 0.0
            && t == 0.0
            && w.round() as u32 == cw
            && h.round() as u32 == ch
            && angle == 0.0
            && size == (cw, ch);
        let cropped = !same && self.crop_canvas(l, t, w, h, angle, resample, size.0, size.1);
        // the DPI too, unless a crop was wanted and couldn't be made: then nothing changes
        if (same || cropped) && (dpi - canvas_dpi).abs() > 0.01 {
            self.set_dpi(dpi);
        }
        cropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(rect: [f32; 4], grab: Grab, from: (f32, f32)) -> CropEdit {
        CropEdit { rect: Some(rect), drag: Some((grab, rect, Vec2::new(from.0, from.1), 10.0)), ..CropEdit::default() }
    }

    #[test]
    fn a_drag_moves_the_part_it_took() {
        let canvas = (1000.0, 800.0);
        let e = edit([100.0, 100.0, 500.0, 400.0], Grab::Move, (300.0, 250.0));
        assert_eq!(e.dragged(Vec2::new(350.0, 270.0), canvas), Some([150.0, 120.0, 550.0, 420.0]));
        let e = edit([100.0, 100.0, 500.0, 400.0], Grab::TopLeft, (100.0, 100.0));
        assert_eq!(e.dragged(Vec2::new(80.0, 130.0), canvas), Some([80.0, 130.0, 500.0, 400.0]));
        // never inside out
        let e = edit([100.0, 100.0, 500.0, 400.0], Grab::Right, (500.0, 250.0));
        assert_eq!(e.dragged(Vec2::new(50.0, 250.0), canvas), None);
    }

    #[test]
    fn unturned_the_frame_keeps_to_whole_pixels() {
        let canvas = (1000.0, 800.0);
        let e = edit([100.0, 100.0, 500.0, 400.0], Grab::Left, (100.0, 250.0));
        assert_eq!(e.dragged(Vec2::new(110.4, 250.0), canvas), Some([110.0, 100.0, 500.0, 400.0]));
        let e = edit([100.0, 100.0, 500.0, 400.0], Grab::Move, (300.0, 250.0));
        assert_eq!(e.dragged(Vec2::new(310.6, 249.3), canvas), Some([111.0, 99.0, 511.0, 399.0]));
        // turned, or resampled, it follows the drag as it is
        let mut e = edit([100.0, 100.0, 500.0, 400.0], Grab::Left, (100.0, 250.0));
        e.angle = 0.1;
        assert_eq!(e.dragged(Vec2::new(110.4, 250.0), canvas), Some([110.4, 100.0, 500.0, 400.0]));
        let mut e = edit([100.0, 100.0, 500.0, 400.0], Grab::Left, (100.0, 250.0));
        e.resample = true;
        assert_eq!(e.dragged(Vec2::new(110.4, 250.0), canvas), Some([110.4, 100.0, 500.0, 400.0]));
    }

    #[test]
    fn a_corner_or_side_dragged_shows_its_size_above_the_touch_away_from_the_middle() {
        // 2 window pixels a point: the readout 296 x 108, 20 from the window's sides
        let middle = Vec2::new(1000.0, 700.0);
        let mut e = edit([0.0, 0.0, 1979.6, 2048.0], Grab::Right, (2048.0, 1024.0));
        e.touch = Vec2::new(1600.0, 800.0);
        // right of the middle: to the touch's right, its top two heights above it
        let r = e.readout(middle, 0, 2400.0, 2.0).unwrap();
        assert_eq!((r.width, r.height), (1979, 2048));
        assert!((r.x - (1600.0 + 0.2 * 296.0)).abs() < 1e-3 && (r.y - (800.0 - 216.0)).abs() < 1e-3, "{r:?}");
        // within the window
        e.touch = Vec2::new(2350.0, 800.0);
        assert_eq!(e.readout(middle, 0, 2400.0, 2.0).unwrap().x, 2400.0 - 296.0 - 20.0);
        // left of the middle (or not 37.5 points past it): to its left, kept off the window's edge
        e.touch = Vec2::new(1070.0, 800.0);
        assert!((e.readout(middle, 0, 2400.0, 2.0).unwrap().x - (1070.0 - 1.2 * 296.0)).abs() < 1e-3);
        e.touch = Vec2::new(100.0, 800.0);
        assert_eq!(e.readout(middle, 0, 2400.0, 2.0).unwrap().x, 20.0);
        // a quarter turn round, the width is what lies across the screen
        let r = e.readout(middle, 1, 2400.0, 2.0).unwrap();
        assert_eq!((r.width, r.height), (2048, 1979));
        // moving the whole frame, or not dragging, shows none
        assert!(edit([0.0, 0.0, 10.0, 10.0], Grab::Move, (5.0, 5.0)).readout(middle, 0, 2400.0, 2.0).is_none());
        assert!(
            CropEdit { rect: Some([0.0, 0.0, 10.0, 10.0]), ..CropEdit::default() }
                .readout(middle, 0, 2400.0, 2.0)
                .is_none()
        );
    }

    #[test]
    fn snapping_and_a_locked_aspect() {
        let canvas = (1000.0, 800.0);
        let mut e = edit([100.0, 100.0, 500.0, 400.0], Grab::Right, (500.0, 250.0));
        e.snapping = true;
        // near the canvas's middle (500): there
        assert_eq!(e.dragged(Vec2::new(505.0, 250.0), canvas), Some([100.0, 100.0, 500.0, 400.0]));
        // moved, the nearest side snaps and the other follows
        let mut e = edit([100.0, 100.0, 500.0, 400.0], Grab::Move, (300.0, 250.0));
        e.snapping = true;
        assert_eq!(e.dragged(Vec2::new(205.0, 250.0), canvas), Some([0.0, 100.0, 400.0, 400.0]));
        // a locked aspect follows the width, from the bottom (or the top, taken by it)
        let mut e = edit([0.0, 0.0, 400.0, 300.0], Grab::Right, (400.0, 150.0));
        e.aspect_locked = true;
        assert_eq!(e.dragged(Vec2::new(800.0, 150.0), canvas), Some([0.0, 0.0, 800.0, 600.0]));
        let mut e = edit([0.0, 100.0, 400.0, 400.0], Grab::TopRight, (400.0, 100.0));
        e.aspect_locked = true;
        assert_eq!(e.dragged(Vec2::new(200.0, 100.0), canvas), Some([0.0, 250.0, 200.0, 400.0]));
    }
}
