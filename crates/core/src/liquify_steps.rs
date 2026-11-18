//! Liquify's undo steps. While it is open each stroke (with what it set going), each Adjust and each
//! Reset is a step of its own, undone and redone one at a time; undone past its first, Liquify
//! closes, and a redo opens it again. Once it closes its whole work is one step, which undone opens
//! it again with its steps as they were (and redone past its last, keeps it and closes it again).
//! A stroke taken back as more fingers land (to tap undo or redo, or to move the view) is no step,
//! and leaves the field as it was and what was undone to redo. The field's side of each step is
//! the canvas's (`Canvas::liquify_mark`); here are their names and the Adjust amount on either side
//! of each.

use procraft_engine::document::LayerId;
use procraft_gpu::Filter;

use crate::adjust::LiquifyKind;
use crate::liquify::LiquifyRun;
use crate::{CanvasSession, State};

/// What an undo or redo of Liquify's opening and closing is called.
pub(crate) const LIQUIFY_STEP: &str = "Liquify";
const ADJUST_STEP: &str = "Adjust";
const RESET_STEP: &str = "Reset";

/// The kinds of step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StepKind {
    Stroke,
    Adjust,
    Reset,
}

/// A step: its kind, what undo and redo call it, and the Adjust amount before and after it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Step {
    kind: StepKind,
    name: &'static str,
    before: f32,
    after: f32,
}

/// Liquify's steps while it is open (or set aside, closed).
#[derive(Clone, Debug, Default)]
pub(crate) struct Steps {
    /// The layers liquified: the first, then those pushed along with it (others chosen, masks).
    pub(crate) layers: Vec<LayerId>,
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// The step under way, closed by the next (or by an undo, a redo or closing).
    open: Option<Step>,
    /// Opened again by undoing the canvas step that kept it: that step, while redo can still keep
    /// it again.
    origin: Option<u64>,
}

impl Steps {
    pub(crate) fn new(layers: Vec<LayerId>) -> Steps {
        Steps { layers, ..Steps::default() }
    }

    /// Whether the field holds anything liquified: a stroke since the last Reset.
    pub(crate) fn liquified(&self) -> bool {
        self.undo
            .iter()
            .chain(&self.open)
            .rev()
            .take_while(|s| s.kind != StepKind::Reset)
            .any(|s| s.kind == StepKind::Stroke)
    }
}

/// Liquify closed, kept to come back: its steps, its Adjust amount, and how it closed.
pub(crate) struct Kept {
    steps: Steps,
    amount: f32,
    closed: Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Closed {
    /// Its work kept as canvas step `made`: undoing that opens it again.
    Kept(u64),
    /// Undone past its first step, the canvas's history then so many steps made, with this one to
    /// undo: a redo while it still is opens it again.
    Undone { made: u64, top: Option<u64> },
}

/// What undo and redo call a stroke of `kind`.
fn way(kind: LiquifyKind) -> &'static str {
    match kind {
        LiquifyKind::Push => "Push",
        LiquifyKind::TwirlRight => "Twirl Right",
        LiquifyKind::TwirlLeft => "Twirl Left",
        LiquifyKind::Pinch => "Pinch",
        LiquifyKind::Expand => "Expand",
        LiquifyKind::Crystals => "Crystals",
        LiquifyKind::Edge => "Edge",
        LiquifyKind::Reconstruct => "Reconstruct",
    }
}

impl State {
    /// Liquify is open (begun, and not yet ended).
    pub(crate) fn liquify_open(&self) -> bool {
        self.adjusting && !self.liquify_steps.layers.is_empty()
    }

    /// The Adjust amount Liquify shows its field at now.
    pub(crate) fn liquify_amount_now(&self) -> f32 {
        match self.last_filter {
            Some(Filter::Liquify { amount }) => amount,
            _ => 1.0,
        }
    }

    /// Shows the field at Adjust `amount`, as its slider says.
    fn show_liquify_amount(&mut self, amount: f32) {
        if self.adjusting
            && let Some(canvas) = self.canvas.as_mut()
        {
            self.last_filter = Some(Filter::Liquify { amount });
            canvas.set_filter(Filter::Liquify { amount });
        }
        let liquified = self.liquify_steps.liquified();
        self.adjust.liquify_follows(amount, liquified);
    }

    /// Closes the step under way (its field's side too): it can be undone now, and nothing is left
    /// to redo, in Liquify or on the canvas.
    pub(crate) fn close_liquify_step(&mut self) {
        let Some(mut step) = self.liquify_steps.open.take() else { return };
        step.after = self.liquify_amount_now();
        self.liquify_steps.undo.push(step);
        self.liquify_steps.redo.clear();
        self.liquify_steps.origin = None;
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.liquify_mark();
            canvas.forget_redo();
        }
    }

    /// A step of `kind` begins (the one under way closed). What was undone can still be redone
    /// until it closes: a stroke taken back as more fingers land leaves it to redo.
    fn open_liquify_step(&mut self, kind: StepKind, name: &'static str) {
        if !self.liquify_open() {
            return;
        }
        self.close_liquify_step();
        let before = self.liquify_amount_now();
        self.liquify_steps.open = Some(Step { kind, name, before, after: before });
    }

    /// The stroke under way taken back, what it did and set going with it: fingers landing after
    /// it make the touch a gesture (two tapped undo, three redo), or it was called off. The field
    /// and the Adjust amount are as the last step left them, and what was undone is still there
    /// to redo. False if no stroke was under way.
    pub(crate) fn take_back_liquify_stroke(&mut self) -> bool {
        if !self.liquify_open() {
            return false;
        }
        let Some(step) = self.liquify_steps.open.filter(|s| s.kind == StepKind::Stroke) else { return false };
        self.liquify_steps.open = None;
        self.liquify.halt();
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.liquify_take_back();
        }
        self.show_liquify_amount(step.before);
        true
    }

    /// A stroke of `kind` begins.
    pub(crate) fn liquify_stroke_begins(&mut self, kind: LiquifyKind) {
        self.open_liquify_step(StepKind::Stroke, way(kind));
    }

    /// The Adjust slider moves: one step for as long as its drag goes on.
    pub(crate) fn liquify_adjusts(&mut self) {
        if self.liquify_steps.open.is_none_or(|s| s.kind != StepKind::Adjust) {
            self.open_liquify_step(StepKind::Adjust, ADJUST_STEP);
        }
    }

    /// The Adjust slider let go: its drag is a step of its own, and the next another.
    pub(crate) fn liquify_adjust_ends(&mut self) {
        if self.liquify_steps.open.is_some_and(|s| s.kind == StepKind::Adjust) {
            self.close_liquify_step();
        }
    }

    /// Reset: a step of its own, done at once.
    pub(crate) fn liquify_resets(&mut self) {
        self.open_liquify_step(StepKind::Reset, RESET_STEP);
    }

    /// Liquify's session as it ends: `kept` sets it aside (its work kept as one step, `commit`, or
    /// undone past its first step) to come back; otherwise it goes. `made` is how many canvas
    /// steps there were before it ended.
    pub(crate) fn liquify_ended(&mut self, commit: bool, kept: bool, made: u64) {
        let steps = std::mem::take(&mut self.liquify_steps);
        let amount = self.liquify_amount_now();
        self.liquify = LiquifyRun::default();
        let Some(canvas) = self.canvas.as_mut() else { return };
        if !kept {
            canvas.forget_liquify();
            self.liquify_kept = None;
            return;
        }
        self.flush();
        let Some(canvas) = self.canvas.as_ref() else { return };
        let closed = if commit {
            // nothing changed, no step: nothing to come back to
            if canvas.steps_made() == made {
                self.liquify_kept = None;
                if let Some(canvas) = self.canvas.as_mut() {
                    canvas.forget_liquify();
                }
                return;
            }
            Closed::Kept(canvas.steps_made())
        } else {
            Closed::Undone { made: canvas.steps_made(), top: canvas.undo_step() }
        };
        self.liquify_kept = Some(Kept { steps, amount, closed });
    }

    /// Whether a redo would open Liquify again (undone past its first step, nothing since).
    pub(crate) fn liquify_redo_ready(&self) -> bool {
        let Some(canvas) = self.canvas.as_ref() else { return false };
        match self.liquify_kept.as_ref().map(|k| k.closed) {
            Some(Closed::Undone { made, top }) => canvas.steps_made() == made && canvas.undo_step() == top,
            _ => false,
        }
    }

    /// Open, whether there is a step to redo (or the step that kept it, to keep it again).
    pub(crate) fn liquify_can_redo(&self) -> bool {
        let s = &self.liquify_steps;
        s.open.is_none()
            && (!s.redo.is_empty()
                || s.origin.is_some_and(|id| self.canvas.as_ref().and_then(|c| c.redo_step()) == Some(id)))
    }
}

impl CanvasSession {
    /// Takes back the liquify stroke under way (see [`State::take_back_liquify_stroke`]); false if
    /// there was none.
    pub(crate) fn take_back_liquify(&self) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let taken = st.take_back_liquify_stroke();
        if taken {
            self.shared.frame(st);
        }
        taken
    }

    /// Undo (`undo`) or redo while Liquify is open: a step at a time; undone past its first, it
    /// closes; redone past its last after it came back by an undo, its work is kept again and it
    /// closes. Returns the step's name, or none when Liquify isn't open.
    pub(crate) fn step_liquify(&self, undo: bool) -> Option<Option<String>> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if !st.liquify_open() {
            return None;
        }
        // what it set going stops, and the step under way closes
        st.liquify.halt();
        st.close_liquify_step();
        st.flush();
        // the oldest steps the field let go (they held too much) are gone here too
        if let Some(canvas) = st.canvas.as_ref() {
            let (kept, _) = canvas.liquify_steps();
            let s = &mut st.liquify_steps;
            let over = s.undo.len().saturating_sub(kept);
            s.undo.drain(..over);
        }
        let s = &mut st.liquify_steps;
        let step = if undo { s.undo.pop() } else { s.redo.pop() };
        if let Some(step) = step {
            if undo {
                s.redo.push(step);
            } else {
                s.undo.push(step);
            }
            if let Some(canvas) = st.canvas.as_mut() {
                if undo {
                    canvas.liquify_undo();
                } else {
                    canvas.liquify_redo();
                }
            }
            st.show_liquify_amount(if undo { step.before } else { step.after });
            st.flush();
            st.revision += 1;
            self.shared.frame(st);
            return Some(Some(step.name.to_owned()));
        }
        let origin = st.liquify_steps.origin.filter(|&id| st.canvas.as_ref().and_then(|c| c.redo_step()) == Some(id));
        drop(guard);
        if undo {
            // undone past its first step: it closes, to come back on a redo
            self.close_liquify(false);
            return Some(Some(LIQUIFY_STEP.to_owned()));
        }
        let Some(id) = origin else { return Some(None) };
        // redone past its last: its work kept again, as the step that kept it, and it closes
        self.close_liquify(false);
        self.edit(procraft_gpu::Canvas::redo);
        if let Some(kept) = self.shared.lock().liquify_kept.as_mut() {
            kept.closed = Closed::Kept(id);
            kept.steps.origin = None;
        }
        Some(Some(LIQUIFY_STEP.to_owned()))
    }

    /// Closes Liquify without keeping its work, setting it aside with its steps.
    fn close_liquify(&self, commit: bool) {
        self.shared.lock().liquify_set_aside = true;
        if !self.adjustment_end(commit) {
            self.adjust_end(commit);
        }
        self.shared.lock().liquify_set_aside = false;
    }

    /// Undo (`undo`) or redo with Liquify closed: undoing the step that kept its work opens it
    /// again with its steps; redoing it after it was undone past its first step opens it again.
    /// Returns the step's name, or none when neither is so (or it can't open again).
    pub(crate) fn liquify_again(&self, undo: bool) -> Option<Option<String>> {
        {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            if st.liquify_open() || st.liquify_kept.is_none() {
                return None;
            }
            st.flush();
            let canvas = st.canvas.as_ref()?;
            let closed = st.liquify_kept.as_ref().map(|k| k.closed)?;
            let fits = canvas.keeps_liquify()
                && st.liquify_kept.as_ref().is_some_and(|k| {
                    k.steps.layers.iter().all(|&id| st.doc.layer(id).is_some() && !st.doc.is_locked(id))
                });
            let wanted = match closed {
                Closed::Kept(id) => undo && canvas.undo_step() == Some(id),
                Closed::Undone { .. } => !undo && st.liquify_redo_ready(),
            };
            // it can't come back once the canvas has let its step go, or made a new one since it
            // was undone past its first: what it holds goes
            let gone = match closed {
                Closed::Kept(id) => !canvas.keeps_step(id),
                Closed::Undone { made, .. } => canvas.steps_made() != made,
            };
            if !wanted && !gone {
                return None;
            }
            if gone || !fits {
                // gone from under it: the step undoes or redoes as any other
                st.liquify_kept = None;
                if let Some(canvas) = st.canvas.as_mut() {
                    canvas.forget_liquify();
                }
                return None;
            }
        }
        let kept = self.shared.lock().liquify_kept.take()?;
        let origin = match kept.closed {
            Closed::Kept(id) => {
                // its work taken back first
                self.edit(procraft_gpu::Canvas::undo);
                Some(id)
            }
            Closed::Undone { .. } => kept.steps.origin,
        };
        if !self.liquify_open_again(kept, origin) {
            return Some(None);
        }
        Some(Some(LIQUIFY_STEP.to_owned()))
    }

    /// Opens Liquify again on the layers `kept` liquified, its field and steps as they were.
    fn liquify_open_again(&self, kept: Kept, origin: Option<u64>) -> bool {
        {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            st.finish_stroke();
            st.settle();
            let Kept { mut steps, amount, .. } = kept;
            let Some((&first, along)) = steps.layers.split_first() else { return false };
            let alpha_locked = steps.layers.len() == 1 && st.doc.layer(first).is_some_and(|l| l.alpha_lock);
            let Some(canvas) = st.canvas.as_mut() else { return false };
            canvas.mark_steps();
            canvas.set_filter_keeps_alpha(alpha_locked);
            canvas.begin_transform(first);
            for &id in along {
                canvas.transform_also(id);
            }
            canvas.resume_liquify();
            steps.origin = origin;
            st.liquify_steps = steps;
            st.adjusting = true;
            st.last_filter = None;
            st.liquify = LiquifyRun::default();
            st.show_liquify_amount(amount);
            st.adjust.liquify_open_again();
            st.scene_flat_for_tool();
            st.revision += 1;
            self.shared.frame(st);
        }
        true
    }
}
