//! Text layers: text added and edited where it sits. The app lays the text out and draws it into
//! the layer's pixels; the layer keeps the text so it can be edited again, until it is rasterized.
//! An edit is one undo step, back to the text and the pixels as it began. Inside an edit, each
//! drag of a side of its box (its width) is a step of its own while the edit lasts.

use procraft_engine::document::{Document, LayerId, LayerText, TextAlign, TextRun};
use procraft_engine::geom::{Affine, IRect, Vec2};

use crate::text_runs::{self, TextStyleChange, Typing};
use crate::{CanvasSession, State};

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TextAlignment {
    Left,
    Center,
    Right,
    Justify,
}

/// A text layer's text and how it looks.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TextInfo {
    pub text: String,
    /// The typeface's family and style names.
    pub font: String,
    pub style: String,
    /// Letter height, canvas pixels.
    pub size: f32,
    /// Fractions of the size, -1..1.
    pub kerning: f32,
    pub tracking: f32,
    pub leading: f32,
    pub baseline: f32,
    pub opacity: f32,
    pub align: TextAlignment,
    pub underline: bool,
    pub outline: bool,
    pub vertical: bool,
    pub all_caps: bool,
    /// sRGB 0..1.
    pub r: f32,
    pub g: f32,
    pub b: f32,
    /// The box's top left corner, canvas pixels, and the width it wraps at (0: no wrapping).
    pub x: f32,
    pub y: f32,
    pub width: f32,
    /// The look of each stretch of the text in turn, covering it all, when it isn't one look
    /// throughout (the look above is then the first's); none when it is. The alignment, the
    /// leading, upper case and running down are the whole text's.
    pub runs: Vec<TextRunInfo>,
}

/// A stretch of text in a look of its own (see [`TextInfo::runs`]): how many characters it covers
/// (as the core counts them) and its look.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TextRunInfo {
    pub len: u32,
    pub font: String,
    pub style: String,
    pub size: f32,
    pub kerning: f32,
    pub tracking: f32,
    pub baseline: f32,
    pub opacity: f32,
    pub underline: bool,
    pub outline: bool,
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl From<&TextRun> for TextRunInfo {
    fn from(r: &TextRun) -> Self {
        TextRunInfo {
            len: r.len,
            font: r.font.clone(),
            style: r.style.clone(),
            size: r.size,
            kerning: r.kerning,
            tracking: r.tracking,
            baseline: r.baseline,
            opacity: r.opacity,
            underline: r.underline,
            outline: r.outline,
            r: r.color[0],
            g: r.color[1],
            b: r.color[2],
        }
    }
}

impl From<TextRunInfo> for TextRun {
    fn from(r: TextRunInfo) -> Self {
        TextRun {
            len: r.len,
            font: r.font,
            style: r.style,
            size: r.size,
            kerning: r.kerning,
            tracking: r.tracking,
            baseline: r.baseline,
            opacity: r.opacity,
            underline: r.underline,
            outline: r.outline,
            color: [r.r, r.g, r.b],
        }
    }
}

impl TextInfo {
    /// Each stretch of the text in its look, in order, covering it all (one when it keeps none).
    pub(crate) fn each_run(&self) -> Vec<TextRun> {
        LayerText::from(self.clone()).runs()
    }
}

impl From<&LayerText> for TextInfo {
    fn from(t: &LayerText) -> Self {
        TextInfo {
            text: t.text.clone(),
            font: t.font.clone(),
            style: t.style.clone(),
            size: t.size,
            kerning: t.kerning,
            tracking: t.tracking,
            leading: t.leading,
            baseline: t.baseline,
            opacity: t.opacity,
            align: match t.align {
                TextAlign::Left => TextAlignment::Left,
                TextAlign::Center => TextAlignment::Center,
                TextAlign::Right => TextAlignment::Right,
                TextAlign::Justify => TextAlignment::Justify,
            },
            underline: t.underline,
            outline: t.outline,
            vertical: t.vertical,
            all_caps: t.all_caps,
            r: t.color[0],
            g: t.color[1],
            b: t.color[2],
            x: t.origin[0],
            y: t.origin[1],
            width: t.width,
            runs: t.runs.iter().map(TextRunInfo::from).collect(),
        }
    }
}

impl From<TextInfo> for LayerText {
    fn from(t: TextInfo) -> Self {
        LayerText {
            text: t.text,
            font: t.font,
            style: t.style,
            size: t.size,
            kerning: t.kerning,
            tracking: t.tracking,
            leading: t.leading,
            baseline: t.baseline,
            opacity: t.opacity,
            align: match t.align {
                TextAlignment::Left => TextAlign::Left,
                TextAlignment::Center => TextAlign::Center,
                TextAlignment::Right => TextAlign::Right,
                TextAlignment::Justify => TextAlign::Justify,
            },
            underline: t.underline,
            outline: t.outline,
            vertical: t.vertical,
            all_caps: t.all_caps,
            color: [t.r, t.g, t.b],
            origin: [t.x, t.y],
            width: t.width,
            runs: t.runs.into_iter().map(TextRun::from).collect(),
        }
    }
}

/// How many finished text edits are remembered for undo and redo to edit again, and how many
/// width changes inside an edit.
const TEXT_STEPS_KEPT: usize = 256;

/// A side of the text box, whose dot is dragged to change its width.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TextSide {
    Left,
    Right,
}

/// The name of a change to the box's width, undone inside the edit.
const WIDTH_STEP: &str = "Transform";
/// The narrowest a side's drag makes the box: a letter or so (the text's size).
const NARROWEST: f32 = 1.0;

/// Text being edited: its layer, the document as the edit began, the changes to its box's width
/// made since (to undo and redo one at a time while it lasts; the edit's one step takes them all
/// once it ends), a side being dragged, and what is typed (where, in which look).
pub(crate) struct TextEdit {
    layer: LayerId,
    before: Document,
    steps: Vec<WidthStep>,
    undone: Vec<WidthStep>,
    side: Option<SideDrag>,
    typing: Typing,
}

impl TextEdit {
    /// Whether a width change can be undone (`undo`) or redone inside the edit.
    pub(crate) fn has_step(&self, undo: bool) -> bool {
        !if undo { &self.steps } else { &self.undone }.is_empty()
    }
}

/// A change to the box's width: its left (canvas pixels) and its width (0: as wide as the text),
/// before and after.
#[derive(Clone, Copy, Debug, PartialEq)]
struct WidthStep {
    before: (f32, f32),
    after: (f32, f32),
}

/// A side being dragged: which, the canvas point the drag began at, and the box then: its left,
/// its width as laid out and as kept (0: as wide as the text).
#[derive(Clone, Copy, Debug)]
struct SideDrag {
    side: TextSide,
    from: Vec2,
    x: f32,
    laid: f32,
    kept: f32,
}

/// Window pixel (`x`, `y`) on the canvas, through the view (as [`CanvasSession::view_matrix`]
/// gives it).
fn on_canvas(st: &State, x: f32, y: f32) -> Option<Vec2> {
    let m = st.view.map_or(Affine::IDENTITY, |v| v.to_screen());
    m.inverse().map(|m| m.apply(Vec2::new(x, y))).filter(|p| p.is_finite())
}

/// The text being edited, and its layer.
fn edited(st: &State) -> Option<(LayerId, TextInfo)> {
    let layer = st.text_edit.as_ref()?.layer;
    st.doc.layer(layer)?.text.as_ref().map(|t| (layer, TextInfo::from(t)))
}

/// Ends a side's drag: what it changed becomes a step inside the edit.
fn end_side(st: &mut State) {
    let Some(edit) = st.text_edit.as_mut() else { return };
    let Some(drag) = edit.side.take() else { return };
    let Some(now) = st.doc.layer(edit.layer).and_then(|l| l.text.as_ref()) else { return };
    if now.origin[0] != drag.x || now.width != drag.kept {
        edit.steps.push(WidthStep { before: (drag.x, drag.kept), after: (now.origin[0], now.width) });
        let over = edit.steps.len().saturating_sub(TEXT_STEPS_KEPT);
        edit.steps.drain(..over);
        edit.undone.clear();
    }
}

/// New text as it looks on screen whatever the zoom (as measured): letters this many points
/// tall, its box this many points wide, centred across the window this far down it.
const NEW_TEXT_POINTS: f32 = 64.0;
const NEW_TEXT_BOX_POINTS: f32 = 488.448;
const NEW_TEXT_DOWN: f32 = 0.25;

#[uniffi::export]
impl CanvasSession {
    /// `base` (its words and look) placed as new text in a window `width` × `height` pixels,
    /// `density` pixels a point: its letters and box sized so they look the same at any zoom,
    /// the box centred across the window, its middle a quarter of the way down, kept on the
    /// canvas.
    pub fn new_text_placed(&self, base: TextInfo, width: f32, height: f32, density: f32) -> TextInfo {
        let m = self.view_matrix();
        let [a, b, c, d, e, f] =
            <[f32; 6]>::try_from(m.get(..6).unwrap_or_default()).unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        // canvas pixels a window point, at the view's zoom (turned or not)
        let shown = a.hypot(b).max(1e-6);
        let per_point = density.max(0.1) / shown;
        // the window point the box is centred on, on the canvas
        let det = a * d - b * c;
        let (x, y) = (width / 2.0 - e, height * NEW_TEXT_DOWN - f);
        let (cx, cy) = if det.abs() < 1e-9 { (0.0, 0.0) } else { ((d * x - c * y) / det, (a * y - b * x) / det) };
        let size = NEW_TEXT_POINTS * per_point;
        let box_width = NEW_TEXT_BOX_POINTS * per_point;
        let mut info = TextInfo { size, width: box_width, x: cx - box_width / 2.0, y: 0.0, ..base };
        // in the last text's typeface, style, alignment and direction
        if let Some(style) = self.shared.lock().doc.text_style.clone() {
            let mut look: LayerText = info.into();
            (look.font, look.style, look.align, look.vertical) = (style.font, style.style, style.align, style.vertical);
            info = TextInfo::from(&look);
        }
        let tall = crate::text_draw::text_box(info.clone()).get(3).copied().unwrap_or(size);
        let canvas_height = self.shared.lock().doc.height() as f32;
        info.y = (cy - tall / 2.0).clamp(0.0, (canvas_height - tall).max(0.0));
        info
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Starts editing text: a new text layer above the current one with `info` (`id` none), or
    /// text layer `id`. Returns the layer being edited, or none if `id` isn't a text layer.
    pub fn text_begin(&self, id: Option<u64>, info: TextInfo) -> Option<u64> {
        self.text_end(true);
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        let before = st.doc.clone();
        let layer = match id {
            Some(id) => {
                let id = LayerId(id);
                st.doc.layer(id).filter(|l| l.text.is_some() && !st.doc.is_locked(id))?;
                st.doc.set_current(id);
                id
            }
            None if st.doc.has_room(1) => st.doc.add_text_layer(info.into()),
            None => return None,
        };
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.hold_layer(layer);
        }
        // all of it chosen as it begins; what is typed takes the colour chosen now
        let count = st.doc.layer(layer).and_then(|l| l.text.as_ref()).map_or(0, |t| t.text.chars().count());
        let typing = Typing { chosen: (0, count), picked: None, colour: Some(st.color) };
        st.text_edit = Some(TextEdit { layer, before, steps: Vec::new(), undone: Vec::new(), side: None, typing });
        // a new layer is clear: its first drawing needn't wipe the rest of it
        st.text_drawn = id.is_none().then_some(IRect::EMPTY);
        st.revision += 1;
        self.shared.frame(st);
        Some(layer.0)
    }

    /// Changes the text being edited (one undo step when the edit ends): its words, box and the
    /// whole text's look as `info` has them. Its words changed, the characters kept keep their
    /// looks and those typed take the look where they went (see [`CanvasSession::text_select`]);
    /// otherwise its runs are `info`'s (none: its look above throughout). The text as it now
    /// stands; none when no text is being edited.
    pub fn set_text(&self, info: TextInfo) -> Option<TextInfo> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let edit = st.text_edit.as_mut()?;
        let layer = edit.layer;
        let mut before = st.doc.layer(layer)?.text.clone()?;
        let mut now = LayerText::from(info);
        if now.text != before.text {
            // text of one look given another with its words: what it had takes that look too
            if before.runs.is_empty() && now.runs.is_empty() {
                before.set_look(&now.run(0));
            }
            now.set_runs(text_runs::edited(&before, &now.text, &edit.typing));
            // what was picked at the caret has been typed
            edit.typing.picked = None;
        } else {
            let runs = now.runs();
            now.set_runs(runs);
        }
        if st.doc.set_text(layer, now) {
            st.revision += 1;
        }
        edited(st).map(|(_, now)| now)
    }

    /// The characters chosen in the text being edited (`start`..`end`, as the core counts them;
    /// the same for a caret): what is typed next goes there, in the look there. Told after each
    /// change of the text, not before: a change is found against where they were.
    pub fn text_select(&self, start: u32, end: u32) {
        if let Some(edit) = self.shared.lock().text_edit.as_mut() {
            edit.typing.choose(start as usize, end as usize);
        }
    }

    /// Changes the look of characters `start`..`end` of the text being edited (as the core counts
    /// them) by `change`; with none chosen (`start` == `end`), the look of what is typed there next
    /// (and the whole text's, while it has no characters). The text as it now stands; none when no
    /// text is being edited.
    pub fn text_restyle(&self, start: u32, end: u32, change: TextStyleChange) -> Option<TextInfo> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let edit = st.text_edit.as_mut()?;
        let layer = edit.layer;
        let mut text = st.doc.layer(layer)?.text.clone()?;
        let count = text.text.chars().count();
        let (start, end) = ((start.min(end) as usize).min(count), (start.max(end) as usize).min(count));
        if start < end {
            text.set_runs(text_runs::restyled(&text.runs(), start, end, &change));
        } else {
            let mut look = match &edit.typing.picked {
                Some(picked) if edit.typing.chosen == (start, start) => picked.clone(),
                _ => text_runs::look_at(&text, start, start),
            };
            change.apply(&mut look);
            if count == 0 {
                text.set_look(&look);
            }
            edit.typing.chosen = (start, start);
            edit.typing.picked = Some(look);
        }
        if st.doc.set_text(layer, text) {
            st.revision += 1;
        }
        edited(st).map(|(_, now)| now)
    }

    /// The look the style sheet shows for characters `start`..`end` of the text being edited (as
    /// the core counts them): the first one's, underline and outline on only when every one has
    /// them; with none chosen, the look typed there next. Its length is how many are chosen; none
    /// when no text is being edited.
    pub fn text_look(&self, start: u32, end: u32) -> Option<TextRunInfo> {
        let st = self.shared.lock();
        let edit = st.text_edit.as_ref()?;
        let text = st.doc.layer(edit.layer)?.text.as_ref()?;
        let (start, end) = (start.min(end) as usize, start.max(end) as usize);
        let look = match &edit.typing.picked {
            Some(picked) if start == end && edit.typing.chosen == (start, start) => picked.clone(),
            _ => text_runs::look_at(text, start, end),
        };
        Some(TextRunInfo::from(&look))
    }

    /// Draws the text being edited into its layer, here (a caller may run it off its main
    /// thread): only where it was drawn last and where it now reaches (the whole layer the first
    /// time an edit of a text layer already there draws), nothing handed across. A drawing finished after the text changed
    /// again is dropped (the next one draws it). False when no text is being edited.
    pub fn draw_text_into_layer(&self) -> bool {
        self.text_draw_start().is_some_and(|job| self.text_draw_put(job.draw()))
    }

    /// The text being edited, drawn into its layer: premultiplied RGBA8, the canvas's size.
    pub fn put_text_pixels(&self, rgba: Vec<u8>) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(layer) = st.text_edit.as_ref().map(|e| e.layer) else { return false };
        let Some(canvas) = st.canvas.as_mut() else { return false };
        canvas.put_layer(layer, rgba);
        self.shared.frame(st);
        true
    }

    /// Ends editing text: `keep` makes the edit one undo step; otherwise the layer goes back to
    /// how it was (a new one goes away). New text left empty goes away too, and an edit that
    /// changed nothing makes no step.
    pub fn text_end(&self, keep: bool) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        // the width changes made inside it are all part of its one step
        let Some(TextEdit { layer, before, .. }) = st.text_edit.take() else { return };
        let new = before.layer(layer).is_none();
        let empty = st.doc.layer(layer).and_then(|l| l.text.as_ref()).is_none_or(|t| t.text.trim().is_empty());
        let unchanged = !new && before.layer(layer).map(|l| &l.text) == st.doc.layer(layer).map(|l| &l.text);
        let keep = keep && !(new && empty) && !unchanged;
        if let Some(canvas) = st.canvas.as_mut() {
            if keep {
                canvas.name_next_step("Text editing");
            }
            canvas.release_layer(keep, keep.then(|| before.clone()));
        }
        if !keep {
            let edited = std::mem::replace(&mut st.doc, before);
            st.doc.keep_ids_from(&edited);
        }
        if keep {
            // new text starts with this look
            st.doc.text_style = st.doc.layer(layer).and_then(|l| l.text.as_ref()).map(LayerText::look);
            // the step made, so undo and redo of it can go back to editing this text
            st.flush();
            if let Some(step) = st.canvas.as_ref().and_then(procraft_gpu::Canvas::undo_step) {
                st.text_steps.push((step, layer));
                let over = st.text_steps.len().saturating_sub(TEXT_STEPS_KEPT);
                st.text_steps.drain(..over);
            }
        }
        st.revision += 1;
        self.shared.frame(st);
    }

    /// The text layer undo would take back to editing first: the next step undone is a finished
    /// text edit, its layer still text. None while text is being edited.
    pub fn text_undo_layer(&self) -> Option<u64> {
        let st = self.shared.lock();
        if st.text_edit.is_some() {
            return None;
        }
        let step = st.canvas.as_ref()?.undo_step()?;
        let layer = st.text_steps.iter().rev().find(|(s, _)| *s == step)?.1;
        st.doc.layer(layer).filter(|l| l.text.is_some() && !st.doc.is_locked(layer)).map(|_| layer.0)
    }

    /// The text layer the next redo brings back, when that step is a finished text edit: after
    /// the redo it is edited again.
    pub fn text_redo_layer(&self) -> Option<u64> {
        let st = self.shared.lock();
        let step = st.canvas.as_ref()?.redo_step()?;
        st.text_steps.iter().rev().find(|(s, _)| *s == step).map(|(_, l)| l.0)
    }

    /// The text layer being edited, if any.
    pub fn editing_text(&self) -> Option<u64> {
        self.shared.lock().text_edit.as_ref().map(|e| e.layer.0)
    }

    /// The side of the text box being edited whose dot is within `reach` window pixels of window
    /// pixel (`x`, `y`), the nearer if both are; none when no text is being edited.
    pub fn text_side_at(&self, x: f32, y: f32, reach: f32) -> Option<TextSide> {
        let st = self.shared.lock();
        let (_, info) = edited(&st)?;
        let m = st.view.map_or(Affine::IDENTITY, |v| v.to_screen());
        let [left, top, width, height] = <[f32; 4]>::try_from(crate::text_draw::text_box(info)).ok()?;
        let at = Vec2::new(x, y);
        [(TextSide::Left, left), (TextSide::Right, left + width)]
            .into_iter()
            .map(|(side, sx)| (side, m.apply(Vec2::new(sx, top + height / 2.0)).distance(at)))
            .filter(|(_, d)| *d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(side, _)| side)
    }

    /// Starts dragging `side` of the text box being edited from window pixel (`x`, `y`): the side
    /// follows the drag along the box (whichever way the canvas is turned or flipped), the other
    /// staying put. False when no text is being edited.
    pub fn text_side_begin(&self, side: TextSide, x: f32, y: f32) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        end_side(st);
        let (Some(from), Some((_, info))) = (on_canvas(st, x, y), edited(st)) else { return false };
        let laid = crate::text_draw::text_box(info.clone()).get(2).copied().unwrap_or(info.width);
        let Some(edit) = st.text_edit.as_mut() else { return false };
        edit.side = Some(SideDrag { side, from, x: info.x, laid, kept: info.width });
        true
    }

    /// The side being dragged follows the drag to window pixel (`x`, `y`): the text wraps to the
    /// box's new width, which is never narrower than a letter or so (nor than the box began).
    /// The text as it now stands, or none when no side is being dragged.
    pub fn text_side_drag(&self, x: f32, y: f32) -> Option<TextInfo> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let drag = st.text_edit.as_ref()?.side?;
        let to = on_canvas(st, x, y)?;
        let (layer, mut info) = edited(st)?;
        // along the box's own axis: the canvas's across, the box upright on it
        let moved = (to.x - drag.from.x).clamp(-1.0e6, 1.0e6);
        let narrowest = (info.size * NARROWEST).min(drag.laid).max(1.0);
        let (left, right) = (drag.x, drag.x + drag.laid);
        match drag.side {
            TextSide::Right => (info.x, info.width) = (left, (right + moved - left).max(narrowest)),
            TextSide::Left => {
                let x = (left + moved).min(right - narrowest);
                (info.x, info.width) = (x, right - x);
            }
        }
        if st.doc.set_text(layer, info.into()) {
            st.revision += 1;
        }
        edited(st).map(|(_, now)| now)
    }

    /// Ends a side's drag: the change to the width is one undo step while the edit lasts.
    pub fn text_side_end(&self) {
        end_side(&mut self.shared.lock());
    }

    /// Inside editing text, undoes the last change to its box's width, the text still edited:
    /// the step's name; none if there is none (undo then ends the edit and takes it all back).
    pub fn text_box_undo(&self) -> Option<String> {
        self.text_box_step(true)
    }

    /// Inside editing text, redoes the last change to its box's width undone: the step's name;
    /// none if there is none.
    pub fn text_box_redo(&self) -> Option<String> {
        self.text_box_step(false)
    }

    /// A text layer's text, or none for another layer.
    pub fn layer_text(&self, id: u64) -> Option<TextInfo> {
        let st = self.shared.lock();
        st.doc.layer(LayerId(id))?.text.as_ref().map(TextInfo::from)
    }

    /// Makes text layer `id` an ordinary layer, its pixels kept, as one undo step.
    pub fn rasterize_layer(&self, id: u64) -> bool {
        self.change("rasterize", |d| d.rasterize(LayerId(id)).then_some(())).is_some()
    }
}

impl CanvasSession {
    /// The colour chosen while text is edited: what is typed from now on takes it, and the
    /// characters chosen (when any are) change to it, as text with none yet does. Whether the
    /// text changed (to draw again).
    pub(crate) fn text_colour_chosen(&self, colour: [f32; 3]) -> bool {
        let (start, end, empty) = {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            let Some(edit) = st.text_edit.as_mut() else { return false };
            edit.typing.colour = Some(colour);
            let (start, end) = edit.typing.chosen;
            let empty = st.doc.layer(edit.layer).and_then(|l| l.text.as_ref()).is_none_or(|t| t.text.is_empty());
            (start, end, empty)
        };
        if start == end && !empty {
            return false;
        }
        let [r, g, b] = colour;
        self.text_restyle(start as u32, end as u32, TextStyleChange::Colour { r, g, b }).is_some()
    }

    /// A width change inside the edit undone (`undo`) or redone: the side it moved goes back (or
    /// again) by as much, wherever the text has since been moved.
    fn text_box_step(&self, undo: bool) -> Option<String> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        end_side(st);
        let edit = st.text_edit.as_mut()?;
        let step = if undo { edit.steps.pop() } else { edit.undone.pop() }?;
        let (was, now) = if undo { (step.after, step.before) } else { (step.before, step.after) };
        if let Some(mut text) = st.doc.layer(edit.layer).and_then(|l| l.text.clone()) {
            text.origin[0] += now.0 - was.0;
            text.width = now.1;
            st.doc.set_text(edit.layer, text);
        }
        if undo { &mut edit.undone } else { &mut edit.steps }.push(step);
        st.revision += 1;
        self.shared.frame(st);
        Some(WIDTH_STEP.to_owned())
    }
}

/// The text being edited, to be drawn into its layer within `region` of the canvas (as
/// [`CanvasSession::text_draw_start`] finds it): drawn on any thread, then put in.
pub struct TextDraw {
    layer: LayerId,
    info: TextInfo,
    region: IRect,
}

/// A [`TextDraw`] drawn: the region's pixels, and where in it the text landed.
pub struct TextDrawn {
    job: TextDraw,
    pixels: Vec<u8>,
    painted: Option<IRect>,
}

impl TextDraw {
    /// The letters drawn over transparency within the region (the slow part).
    pub fn draw(self) -> TextDrawn {
        let region = self.region;
        let pixels =
            if region.is_empty() { Vec::new() } else { crate::text_draw::draw_text_within(&self.info, region) };
        let painted = crate::layer_ops::painted_rect(&pixels, region.width())
            .map(|r| IRect::new(r.x0 + region.x0, r.y0 + region.y0, r.x1 + region.x0, r.y1 + region.y0));
        TextDrawn { job: self, pixels, painted }
    }
}

impl CanvasSession {
    /// The text being edited and the part of its layer a drawing of it takes: where it was drawn
    /// last and where it now reaches (all of it the first time on a layer already there); none
    /// when no text is edited.
    pub fn text_draw_start(&self) -> Option<TextDraw> {
        let st = self.shared.lock();
        let (layer, info) = edited(&st)?;
        let full = IRect::from_size(st.doc.width(), st.doc.height());
        let region = st.text_drawn.map_or(full, |d| d.union(crate::text_draw::text_reach(&info))).intersect(full);
        Some(TextDraw { layer, info, region })
    }

    /// Puts a drawing into its layer, unless the text changed since it began (a newer one follows)
    /// or isn't edited any more.
    pub fn text_draw_put(&self, drawn: TextDrawn) -> bool {
        let TextDrawn { job, pixels, painted } = drawn;
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let now = st.doc.layer(job.layer).and_then(|l| l.text.as_ref()).map(TextInfo::from);
        if st.text_edit.as_ref().map(|e| e.layer) != Some(job.layer) || now.as_ref() != Some(&job.info) {
            return false;
        }
        let region = job.region;
        if !region.is_empty()
            && let Some(canvas) = st.canvas.as_mut()
        {
            let (x, y) = (region.x0.max(0) as u32, region.y0.max(0) as u32);
            canvas.put_layer_region(job.layer, x, y, region.width(), region.height(), pixels);
        }
        st.text_drawn = Some(painted.unwrap_or(IRect::EMPTY));
        self.shared.frame(st);
        true
    }
}

#[cfg(test)]
mod tests {
    use procraft_engine::geom::Affine;
    use procraft_engine::view::View;
    use procraft_gpu::Gpu;

    use procraft_engine::document::{LayerText, TextRun};

    use super::{TextAlignment, TextInfo, TextSide};
    use crate::CanvasSession;
    use crate::TextStyleChange;

    /// A session on a headless GPU, its canvas shown through `view`; none without a GPU.
    fn session(view: Affine) -> Option<std::sync::Arc<CanvasSession>> {
        let gpu = Gpu::headless().map_err(|e| eprintln!("skipped: {e}")).ok()?;
        let s = CanvasSession::new(400, 400);
        {
            let mut st = s.shared.lock();
            st.window_size = (400, 400);
            st.view = Some(View::from_affine(view));
            st.gpu = Some(gpu);
            st.ensure_canvas();
        }
        Some(s)
    }

    fn text(words: &str) -> TextInfo {
        TextInfo {
            text: words.to_owned(),
            font: "inter".to_owned(),
            style: "Regular".to_owned(),
            size: 20.0,
            kerning: 0.0,
            tracking: 0.0,
            leading: 0.0,
            baseline: 0.0,
            opacity: 1.0,
            align: TextAlignment::Left,
            underline: false,
            outline: false,
            vertical: false,
            all_caps: false,
            r: 0.0,
            g: 0.0,
            b: 0.0,
            x: 100.0,
            y: 50.0,
            width: 200.0,
            runs: Vec::new(),
        }
    }

    /// The box being edited: its left, width and height.
    fn shape(s: &CanvasSession) -> (f32, f32, f32) {
        let info = s.layer_text(s.editing_text().unwrap()).unwrap();
        let b = crate::text_box(info.clone());
        (info.x, info.width, b[3])
    }

    #[test]
    fn a_side_dragged_sets_the_width_each_drag_a_step_inside_the_edit() {
        crate::add_text_font(include_bytes!("../../../android/app/src/main/assets/fonts/Inter-Regular.ttf").to_vec());
        // shown twice as large and turned half round: canvas (x, y) at window (600 - 2x, 400 - 2y)
        let view = Affine { a: -2.0, b: 0.0, c: 0.0, d: -2.0, e: 600.0, f: 400.0 };
        let Some(s) = session(view) else { return };
        let id = s.text_begin(None, text("Hello there")).unwrap();
        let (_, _, tall) = shape(&s);
        // the dots, mid-side: the right one shows on the window's left
        let mid = 400.0 - 2.0 * (50.0 + tall / 2.0);
        let (left_dot, right_dot) = ((600.0 - 2.0 * 100.0, mid), (600.0 - 2.0 * 300.0, mid));
        assert_eq!(s.text_side_at(left_dot.0, left_dot.1 + 5.0, 20.0), Some(TextSide::Left));
        assert_eq!(s.text_side_at(right_dot.0 - 10.0, right_dot.1, 20.0), Some(TextSide::Right));
        assert_eq!(s.text_side_at(300.0, mid, 20.0), None);
        assert_eq!(s.text_side_at(left_dot.0, left_dot.1 + 40.0, 20.0), None);

        // the right side dragged 60 window pixels to the window's right: 30 canvas pixels in
        assert!(s.text_side_begin(TextSide::Right, right_dot.0, right_dot.1));
        let now = s.text_side_drag(right_dot.0 + 60.0, right_dot.1 + 30.0).unwrap();
        assert_eq!((now.x, now.width), (100.0, 170.0));
        s.text_side_end();
        assert_eq!((shape(&s).0, shape(&s).1), (100.0, 170.0));
        // the left side, from where it shows: the right side stays where it is
        assert!(s.text_side_begin(TextSide::Left, left_dot.0, left_dot.1));
        let now = s.text_side_drag(left_dot.0 - 40.0, left_dot.1).unwrap();
        assert_eq!((now.x, now.width), (120.0, 150.0));
        // never narrower than a letter or so, nor past the other side
        let now = s.text_side_drag(left_dot.0 - 2000.0, left_dot.1).unwrap();
        assert_eq!((now.x, now.width), (250.0, 20.0));
        let now = s.text_side_drag(left_dot.0 + 300.0, left_dot.1).unwrap();
        assert_eq!((now.x, now.width), (-50.0, 320.0));
        s.text_side_drag(left_dot.0 - 2000.0, left_dot.1).unwrap();
        s.text_side_end();
        // narrow, the words wrap (a word broken by letters) and the box grows down
        let (x, width, tall_now) = shape(&s);
        assert_eq!((x, width), (250.0, 20.0));
        assert!(tall_now > 3.0 * tall, "{tall_now} for a line {tall}");
        assert!(s.info().can_undo);

        // undo inside the edit takes one drag back at a time, still editing
        assert_eq!(s.undo().as_deref(), Some("Transform"));
        assert_eq!(s.editing_text(), Some(id));
        assert_eq!(shape(&s), (100.0, 170.0, tall));
        assert!(s.info().can_redo);
        assert_eq!(s.redo().as_deref(), Some("Transform"));
        assert_eq!((shape(&s).0, shape(&s).1), (250.0, 20.0));
        assert_eq!(s.undo().as_deref(), Some("Transform"));
        // moved since, a drag undone moves its side back by as much
        let moved = TextInfo { x: 110.0, ..s.layer_text(id).unwrap() };
        assert!(s.set_text(moved).is_some());
        assert_eq!(s.text_box_undo().as_deref(), Some("Transform"));
        assert_eq!((shape(&s).0, shape(&s).1), (110.0, 200.0));
        assert_eq!(s.text_box_undo(), None);
        // a new drag drops what could be redone
        assert!(s.text_side_begin(TextSide::Right, 0.0, 0.0));
        s.text_side_drag(-20.0, 0.0).unwrap();
        s.text_side_end();
        assert_eq!(shape(&s).1, 210.0);
        assert!(!s.info().can_redo);
        // a press that doesn't move makes no step
        assert!(s.text_side_begin(TextSide::Left, 0.0, 0.0));
        s.text_side_end();
        assert_eq!(s.text_box_undo().as_deref(), Some("Transform"));
        assert_eq!(s.text_box_undo(), None);

        // ending the edit makes it one step; its width changes go with it
        assert!(s.text_side_begin(TextSide::Right, 0.0, 0.0));
        s.text_side_drag(-100.0, 0.0).unwrap();
        s.text_side_end();
        s.text_end(true);
        assert_eq!(s.text_box_undo(), None);
        assert_eq!(s.layer_text(id).map(|t| (t.x, t.width)), Some((110.0, 250.0)));
        assert_eq!(s.text_undo_layer(), Some(id));
        assert_eq!(s.undo().as_deref(), Some("Text editing"));
        assert!(s.layer_text(id).is_none());
        // no side to drag with nothing edited
        assert!(!s.text_side_begin(TextSide::Left, 0.0, 0.0));
        assert_eq!(s.text_side_drag(10.0, 0.0), None);
        assert_eq!(s.text_side_at(0.0, 0.0, 1.0e6), None);
    }

    #[test]
    fn a_box_as_wide_as_its_text_takes_the_width_it_is_dragged_to() {
        crate::add_text_font(include_bytes!("../../../android/app/src/main/assets/fonts/Inter-Regular.ttf").to_vec());
        let Some(s) = session(Affine::IDENTITY) else { return };
        let natural = TextInfo { width: 0.0, ..text("Hello") };
        let laid = crate::text_box(natural.clone())[2];
        let id = s.text_begin(None, natural).unwrap();
        assert!(s.text_side_begin(TextSide::Left, 100.0, 60.0));
        let now = s.text_side_drag(90.0, 60.0).unwrap();
        assert_eq!((now.x, now.width), (90.0, laid + 10.0));
        s.text_side_end();
        // undone, as wide as its text again
        assert_eq!(s.undo().as_deref(), Some("Transform"));
        assert_eq!(s.layer_text(id).map(|t| (t.x, t.width)), Some((100.0, 0.0)));
        // vertical, the box is as wide as its letters and its sides change that too
        let tall = TextInfo { vertical: true, ..s.layer_text(id).unwrap() };
        assert!(s.set_text(tall).is_some());
        let column = crate::text_box(s.layer_text(id).unwrap())[2];
        assert!(s.text_side_begin(TextSide::Right, 0.0, 0.0));
        let now = s.text_side_drag(40.0, 0.0).unwrap();
        assert_eq!(now.width, column + 40.0);
        s.text_end(true);
    }

    /// Each run's length and family.
    fn fonts(t: &TextInfo) -> Vec<(u32, String)> {
        t.each_run().into_iter().map(|r| (r.len, r.font)).collect()
    }

    #[test]
    fn a_look_picked_with_letters_chosen_is_theirs_alone_and_what_is_typed_takes_the_look_there() {
        let s = CanvasSession::new(400, 400);
        let id = s.text_begin(None, text("ab rd")).unwrap();
        // "rd" chosen (a double-tap chooses a word): another typeface for it alone
        s.text_select(3, 5);
        let now = s.text_restyle(3, 5, TextStyleChange::Font { family: "serif".to_owned() }).unwrap();
        assert_eq!(now.font, "inter");
        assert_eq!(fonts(&now), [(3, "inter".to_owned()), (2, "serif".to_owned())]);
        assert_eq!(s.layer_text(id), Some(now.clone()));
        // the style sheet shows the letters chosen's look, and with a caret the look typed there
        assert_eq!(s.text_look(3, 5).map(|l| (l.font, l.len)), Some(("serif".to_owned(), 2)));
        assert_eq!(s.text_look(0, 2).map(|l| l.font).as_deref(), Some("inter"));
        assert_eq!(s.text_look(5, 5).map(|l| l.font).as_deref(), Some("serif"));
        // underline shows on only while every letter chosen has it
        s.text_restyle(4, 5, TextStyleChange::Underline { on: true });
        assert!(s.text_look(4, 5).is_some_and(|l| l.underline));
        assert!(s.text_look(3, 5).is_some_and(|l| !l.underline));
        s.text_restyle(3, 5, TextStyleChange::Underline { on: false });

        // with only a caret, a look picked is what is typed there next
        s.text_select(1, 1);
        s.text_restyle(1, 1, TextStyleChange::Size { size: 40.0 });
        assert_eq!(s.text_look(1, 1).map(|l| l.size), Some(40.0));
        assert_eq!(s.layer_text(id).map(|t| t.each_run().len()), Some(2), "the text itself unchanged");
        let typed = s.set_text(TextInfo { text: "axb rd".to_owned(), ..now }).unwrap();
        s.text_select(2, 2);
        let sizes: Vec<(u32, f32)> = typed.each_run().into_iter().map(|r| (r.len, r.size)).collect();
        assert_eq!(sizes, [(1, 20.0), (1, 40.0), (2, 20.0), (2, 20.0)]);
        // typing on, after it: its look
        let typed = s.set_text(TextInfo { text: "axyb rd".to_owned(), ..typed }).unwrap();
        s.text_select(3, 3);
        assert_eq!(typed.each_run().get(1).map(|r| (r.len, r.size)), Some((2, 40.0)));

        // a colour chosen: the letters chosen take it, and so does what is typed from then on
        s.text_select(0, 1);
        s.set_color(1.0, 0.0, 0.0);
        let red = s.layer_text(id).unwrap();
        assert_eq!(red.each_run().first().map(|r| (r.len, r.color)), Some((1, [1.0, 0.0, 0.0])));
        assert_eq!(red.each_run().get(1).map(|r| r.color), Some([0.0, 0.0, 0.0]));
        s.text_select(7, 7);
        let more = s.set_text(TextInfo { text: "axyb rdz".to_owned(), ..red }).unwrap();
        let last = more.each_run().last().cloned().unwrap();
        assert_eq!((last.len, last.font.as_str(), last.color), (1, "serif", [1.0, 0.0, 0.0]));

        // all of it chosen and given one look: one look throughout, kept as no runs
        s.text_select(0, 8);
        for change in [
            TextStyleChange::Font { family: "inter".to_owned() },
            TextStyleChange::Size { size: 20.0 },
            TextStyleChange::Colour { r: 0.0, g: 0.0, b: 0.0 },
        ] {
            s.text_restyle(0, 8, change);
        }
        let one = s.layer_text(id).unwrap();
        assert!(one.runs.is_empty() && one.font == "inter" && one.size == 20.0, "{one:?}");
        s.text_end(true);
    }

    #[test]
    fn a_text_kept_before_it_could_have_runs_reads_as_one_look() {
        let old = r#"{"text":"Hello","font":"serif","style":"Bold","size":30.0,"color":[1.0,0.0,0.0]}"#;
        let kept: LayerText = serde_json::from_str(old).unwrap();
        let runs = kept.runs();
        assert!(kept.runs.is_empty());
        assert_eq!(runs.len(), 1);
        assert_eq!(
            runs.first().map(|r| (r.len, r.font.as_str(), r.size, r.color)),
            Some((5, "serif", 30.0, [1.0, 0.0, 0.0]))
        );
        // and runs kept with it come back as they were
        let mut two = kept.clone();
        let base = kept.run(0);
        two.set_runs(vec![TextRun { len: 2, ..base.clone() }, TextRun { len: 3, size: 12.0, ..base }]);
        let back: LayerText = serde_json::from_str(&serde_json::to_string(&two).unwrap()).unwrap();
        assert_eq!(back.runs().iter().map(|r| (r.len, r.size)).collect::<Vec<_>>(), [(2, 30.0), (3, 12.0)]);
        // runs that don't cover the text are let go, its look above kept
        let wrong: LayerText =
            serde_json::from_str(r#"{"text":"Hi","size":30.0,"runs":[{"len":5,"size":9.0}]}"#).unwrap();
        assert_eq!(wrong.sanitized().runs().first().map(|r| (r.len, r.size)), Some((2, 30.0)));
    }

    #[test]
    fn an_edit_with_runs_goes_back_whole_on_undo() {
        crate::add_text_font(include_bytes!("../../../android/app/src/main/assets/fonts/Inter-Regular.ttf").to_vec());
        let Some(s) = session(Affine::IDENTITY) else { return };
        let id = s.text_begin(None, text("Hello there")).unwrap();
        s.text_end(true);
        s.text_begin(Some(id), s.layer_text(id).unwrap()).unwrap();
        s.text_select(6, 11);
        s.text_restyle(6, 11, TextStyleChange::Size { size: 40.0 });
        s.draw_text_into_layer();
        s.text_end(true);
        assert_eq!(s.layer_text(id).map(|t| t.each_run().len()), Some(2));
        assert_eq!(s.undo().as_deref(), Some("Text editing"));
        assert_eq!(s.layer_text(id).map(|t| (t.runs.len(), t.size)), Some((0, 20.0)));
        assert_eq!(s.redo().as_deref(), Some("Text editing"));
        assert_eq!(s.layer_text(id).map(|t| t.each_run().len()), Some(2));
    }
}
