//! The look of each stretch of a text layer's characters, as it is typed and restyled: what is
//! typed takes the look where it goes (the first character's it replaces, else the one before the
//! caret's, or the one after it at the start of a line) in the colour chosen; a look picked with
//! characters chosen changes theirs alone, and with only a caret it is what is typed there next.
//! The style sheet shows the first chosen character's look, underline and outline on only when
//! every chosen one has them.

use procraft_engine::document::{LayerText, TextRun};

/// A change to the look of characters (see [`crate::CanvasSession::text_restyle`]): the typeface,
/// the style, the size (canvas pixels), the kerning, tracking and baseline (fractions of the size,
/// -1..1), the opacity, underline, outline, or the colour (sRGB 0..1).
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum TextStyleChange {
    Font { family: String },
    Style { name: String },
    Size { size: f32 },
    Kerning { value: f32 },
    Tracking { value: f32 },
    Baseline { value: f32 },
    Opacity { value: f32 },
    Underline { on: bool },
    Outline { on: bool },
    Colour { r: f32, g: f32, b: f32 },
}

impl TextStyleChange {
    pub(crate) fn apply(&self, run: &mut TextRun) {
        match self {
            TextStyleChange::Font { family } => run.font = family.clone(),
            TextStyleChange::Style { name } => run.style = name.clone(),
            TextStyleChange::Size { size } => run.size = *size,
            TextStyleChange::Kerning { value } => run.kerning = *value,
            TextStyleChange::Tracking { value } => run.tracking = *value,
            TextStyleChange::Baseline { value } => run.baseline = *value,
            TextStyleChange::Opacity { value } => run.opacity = *value,
            TextStyleChange::Underline { on } => run.underline = *on,
            TextStyleChange::Outline { on } => run.outline = *on,
            TextStyleChange::Colour { r, g, b } => run.color = [*r, *g, *b],
        }
    }
}

/// What is typed into text being edited: where the characters chosen are (first to last; the
/// same for a caret), a look picked there with only the caret (what is typed there next), and the
/// colour chosen (what is typed from now on takes it).
#[derive(Clone, Debug, Default)]
pub(crate) struct Typing {
    pub(crate) chosen: (usize, usize),
    pub(crate) picked: Option<TextRun>,
    pub(crate) colour: Option<[f32; 3]>,
}

impl Typing {
    /// The characters chosen move to `start`..`end`: a look picked for a caret elsewhere goes.
    pub(crate) fn choose(&mut self, start: usize, end: usize) {
        let chosen = (start.min(end), start.max(end));
        if chosen != self.chosen {
            self.picked = None;
        }
        self.chosen = chosen;
    }
}

/// The part of `runs` over characters `from`..`to`.
fn cut(runs: &[TextRun], from: usize, to: usize) -> Vec<TextRun> {
    let mut out = Vec::new();
    let mut at = 0usize;
    for r in runs {
        let (a, b) = (at, at + r.len as usize);
        at = b;
        let (x, y) = (a.max(from), b.min(to));
        if y > x {
            out.push(TextRun { len: (y - x) as u32, ..r.clone() });
        }
    }
    out
}

/// The look of character `index` among `runs`.
fn run_at(runs: &[TextRun], index: usize) -> Option<&TextRun> {
    let mut at = 0usize;
    runs.iter().find(|r| {
        at += r.len as usize;
        index < at
    })
}

/// `runs` with characters `start`..`end` changed by `change`.
pub(crate) fn restyled(runs: &[TextRun], start: usize, end: usize, change: &TextStyleChange) -> Vec<TextRun> {
    let count: usize = runs.iter().map(|r| r.len as usize).sum();
    let mut chosen = cut(runs, start, end);
    for r in &mut chosen {
        change.apply(r);
    }
    let mut out = cut(runs, 0, start);
    out.extend(chosen);
    out.extend(cut(runs, end, count));
    out
}

/// The look of characters `start`..`end` of `t` as the style sheet shows it: the first one's,
/// underline and outline on only when every one has them; with none chosen, the look typed at
/// `start` (see [`typed_at`]). Its length is how many are chosen.
pub(crate) fn look_at(t: &LayerText, start: usize, end: usize) -> TextRun {
    let runs = t.runs();
    let count = t.text.chars().count();
    let (start, end) = (start.min(count), end.min(count));
    if end <= start {
        return TextRun { len: 0, ..typed_at(t, &runs, start) };
    }
    let chosen = cut(&runs, start, end);
    let mut look = chosen.first().cloned().unwrap_or_else(|| t.run(0));
    look.underline = chosen.iter().all(|r| r.underline);
    look.outline = chosen.iter().all(|r| r.outline);
    look.len = (end - start) as u32;
    look
}

/// The look typed at caret `at` of `t` (its runs `runs`): the character before's, unless that
/// ends a line with more after it (the next one's then), or the first's at the start; the text's
/// own when it has none.
fn typed_at(t: &LayerText, runs: &[TextRun], at: usize) -> TextRun {
    let count = t.text.chars().count();
    if count == 0 {
        return t.run(0);
    }
    let line_ends = |i: usize| t.text.chars().nth(i).is_some_and(|c| c == '\n' || c == '\u{2029}');
    let index = match at {
        0 => 0,
        _ if line_ends(at - 1) && at < count => at,
        _ => at - 1,
    };
    run_at(runs, index.min(count - 1)).cloned().unwrap_or_else(|| t.run(0))
}

/// `before`'s runs kept as its text becomes `now`, typed with `typing`: the characters kept keep
/// their looks; those typed take the look where they went (the first's of those they replace,
/// else as [`typed_at`] says, or the look picked at the caret), in the colour chosen. Where the
/// text changed is found from what it shares at its start and end, the change put no later than
/// where the characters chosen began (typing a letter beside the same letter).
pub(crate) fn edited(before: &LayerText, now: &str, typing: &Typing) -> Vec<TextRun> {
    let old: Vec<char> = before.text.chars().collect();
    let new: Vec<char> = now.chars().collect();
    let shared = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let start = shared.min(typing.chosen.0);
    let room = old.len().min(new.len()) - start;
    let end = old.iter().rev().zip(new.iter().rev()).take(room).take_while(|(a, b)| a == b).count();
    let (gone, typed) = (old.len() - end, new.len() - end);
    let runs = before.runs();
    let mut out = cut(&runs, 0, start);
    if typed > start {
        let mut look = if gone > start {
            run_at(&runs, start).cloned().unwrap_or_else(|| before.run(0))
        } else {
            match &typing.picked {
                Some(picked) if typing.chosen == (start, start) => picked.clone(),
                _ => typed_at(before, &runs, start),
            }
        };
        if let Some(colour) = typing.colour {
            look.color = colour;
        }
        look.len = (typed - start) as u32;
        out.push(look);
    }
    out.extend(cut(&runs, gone, old.len()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(words: &str, runs: &[(u32, &str)]) -> LayerText {
        let mut t = LayerText { text: words.to_owned(), font: "Helvetica Neue".to_owned(), ..LayerText::default() };
        if !runs.is_empty() {
            let base = t.run(0);
            t.set_runs(
                runs.iter().map(|&(len, font)| TextRun { len, font: font.to_owned(), ..base.clone() }).collect(),
            );
        }
        t
    }

    fn fonts(t: &LayerText) -> Vec<(u32, String)> {
        t.runs().into_iter().map(|r| (r.len, r.font)).collect()
    }

    #[test]
    fn a_font_picked_for_chosen_letters_changes_theirs_alone() {
        let mut t = text("ab rd", &[]);
        let font = TextStyleChange::Font { family: "Gill Sans".to_owned() };
        t.set_runs(restyled(&t.runs(), 3, 5, &font));
        assert_eq!(fonts(&t), [(3, "Helvetica Neue".to_owned()), (2, "Gill Sans".to_owned())]);
        // the look above is the first stretch's
        assert_eq!(t.font, "Helvetica Neue");
        // all of it alike again: one look throughout, kept as none
        let back = TextStyleChange::Font { family: "Helvetica Neue".to_owned() };
        t.set_runs(restyled(&t.runs(), 0, 5, &back));
        assert!(t.runs.is_empty());
        assert_eq!(t.runs().len(), 1);
    }

    #[test]
    fn what_is_typed_takes_the_look_where_it_goes() {
        let t = text("ab\ncd", &[(3, "A"), (2, "B")]);
        let at = |caret: usize| Typing { chosen: (caret, caret), ..Typing::default() };
        // after "b": its look; at the start of the second line: the next one's
        let mut now = LayerText { text: "abx\ncd".to_owned(), ..t.clone() };
        now.set_runs(edited(&t, "abx\ncd", &at(2)));
        assert_eq!(fonts(&now), [(4, "A".to_owned()), (2, "B".to_owned())]);
        let typed = edited(&t, "ab\nxcd", &at(3));
        assert_eq!(typed.iter().map(|r| (r.len, r.font.as_str())).collect::<Vec<_>>(), [(3, "A"), (1, "B"), (2, "B")]);
        // at the end: the last one's
        let typed = edited(&t, "ab\ncdx", &at(5));
        assert_eq!(typed.last().map(|r| (r.len, r.font.as_str())), Some((1, "B")));
        // typed over chosen letters: the first one's
        let over = Typing { chosen: (2, 4), ..Typing::default() };
        let typed = edited(&t, "abxd", &over);
        assert_eq!(typed.iter().map(|r| (r.len, r.font.as_str())).collect::<Vec<_>>(), [(2, "A"), (1, "A"), (1, "B")]);
        // a letter beside the same letter goes where the caret is
        let t = text("aa", &[(1, "A"), (1, "B")]);
        let typed = edited(&t, "aaa", &at(1));
        assert_eq!(typed.iter().map(|r| (r.len, r.font.as_str())).collect::<Vec<_>>(), [(1, "A"), (1, "A"), (1, "B")]);
        // letters taken out take their looks with them
        let typed = edited(&text("abcd", &[(2, "A"), (2, "B")]), "ad", &Typing { chosen: (1, 3), ..Typing::default() });
        assert_eq!(typed.iter().map(|r| (r.len, r.font.as_str())).collect::<Vec<_>>(), [(1, "A"), (1, "B")]);
    }

    #[test]
    fn a_look_picked_at_the_caret_and_the_colour_chosen_are_what_is_typed_next() {
        let t = text("ab", &[]);
        let mut typing = Typing { chosen: (1, 1), ..Typing::default() };
        let mut picked = look_at(&t, 1, 1);
        TextStyleChange::Font { family: "Gill Sans".to_owned() }.apply(&mut picked);
        typing.picked = Some(picked);
        typing.colour = Some([0.0, 0.0, 1.0]);
        let typed = edited(&t, "axb", &typing);
        let middle = &typed[1];
        assert_eq!((middle.len, middle.font.as_str(), middle.color), (1, "Gill Sans", [0.0, 0.0, 1.0]));
        assert_eq!(typed[0].color, [0.0; 3]);
        // the caret moved, what was picked goes (the colour stays chosen)
        typing.choose(2, 2);
        assert!(typing.picked.is_none() && typing.colour.is_some());
    }

    #[test]
    fn the_style_sheet_shows_the_first_chosen_letter_and_underline_only_when_all_have_it() {
        let mut t = text("abcd", &[]);
        t.set_runs(restyled(&t.runs(), 1, 3, &TextStyleChange::Underline { on: true }));
        t.set_runs(restyled(&t.runs(), 2, 4, &TextStyleChange::Size { size: 30.0 }));
        let look = look_at(&t, 1, 3);
        assert!(look.underline && look.size == 64.0 && look.len == 2);
        let look = look_at(&t, 1, 4);
        assert!(!look.underline);
        let look = look_at(&t, 2, 4);
        assert_eq!(look.size, 30.0);
        // a caret: the letter before it
        assert_eq!(look_at(&t, 3, 3).size, 30.0);
        assert_eq!(look_at(&t, 0, 0).size, 64.0);
    }
}
