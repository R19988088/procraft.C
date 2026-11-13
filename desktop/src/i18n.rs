//! The interface's words in Chinese.
//!
//! Everything the interface draws goes through [`crate::look`]'s text helpers, and each of those
//! asks this module for the words first, so a screen is in Chinese wherever the table has it and
//! in English wherever it doesn't. The table itself lives in `desktop/i18n/zh.tsv` beside the
//! crate, apart from the code: adding a translation is a line in that file, and an upstream
//! string that arrives without one simply reads in English. That keeps this fork's localisation
//! out of the way of following upstream.
//!
//! Callers that are handed a language explicitly (the actions, the preferences and the brush
//! studio each take one) use [`tr`]; everything else uses [`say`], reading the language the
//! window's preferences have.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

use crate::prefs::Language;

/// The table: `english<TAB>中文` a line, `#` starting a comment.
const CHINESE: &str = include_str!("../i18n/zh.tsv");

/// The interface's language, as the last frame's preferences had it.
static NOW: AtomicU8 = AtomicU8::new(Language::Chinese as u8);

fn table() -> &'static HashMap<&'static str, &'static str> {
    static TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| CHINESE.lines().filter_map(entry).collect())
}

/// One line of the table, or nothing for a blank line, a comment, or a line with no translation.
fn entry(line: &'static str) -> Option<(&'static str, &'static str)> {
    let line = line.trim_end_matches('\r');
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (english, chinese) = line.split_once('\t')?;
    if english.is_empty() || chinese.is_empty() {
        return None;
    }
    Some((english, chinese))
}

/// The language the interface is drawn in, from the window's preferences.
pub fn set(language: Language) {
    NOW.store(language as u8, Ordering::Relaxed);
}

pub fn now() -> Language {
    Language::from_index(NOW.load(Ordering::Relaxed) as usize)
}

/// `english` as the interface shows it now.
pub fn say(english: &str) -> &str {
    tr(now(), english)
}

/// `english` in `language`: as it is for English, and its Chinese where the table has one.
pub fn tr(language: Language, english: &str) -> &str {
    if language == Language::English {
        return english;
    }
    match table().get(english) {
        Some(chinese) => *chinese,
        None => english,
    }
}

/// `english` with `count` written into its `%`, in the interface's language: the table's
/// `Delete % artworks?` reads 删除 3 件作品？. Where the table has no line for it, English stands.
pub fn counted(english: &str, count: usize) -> String {
    let chinese = say(english);
    if std::ptr::eq(chinese.as_ptr(), english.as_ptr()) {
        english.replace('%', &count.to_string())
    } else {
        chinese.replace('%', &count.to_string())
    }
}
