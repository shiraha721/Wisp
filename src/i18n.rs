//! Translations.
//!
//! English is written in the code. Every other language has a gettext catalog,
//! `lang/<code>/LC_MESSAGES/wisp.po`, that holds both kinds of text Wisp shows:
//! the window text marked with `@tr(…)` in the `.slint` files, which the Slint
//! compiler bundles at build time, and the text made in Rust, marked with the
//! macros below and looked up here at run time. `lang/wisp.pot` lists every
//! message; docs/translating.md explains how to add a language.
//!
//! The language is kept per thread. Wisp builds all its text on the UI thread;
//! tests can switch language without disturbing each other.

use std::cell::Cell;
use std::collections::HashMap;
use std::fmt::{Display, Write};
use std::sync::OnceLock;

/// Translates a message, with optional context and arguments:
/// `tr!("Saved.")`, `tr!("Saved to {}.", name)`, `tr!("note details" => "Notes")`.
/// Arguments fill `{}` in order, or `{0}`, `{1}`… by position (so translations can
/// reorder them); `{{` and `}}` are literal braces.
macro_rules! tr {
    ($ctx:literal => $msg:literal) => {
        $crate::i18n::tr_ctx($ctx, $msg)
    };
    ($ctx:literal => $msg:literal, $($arg:expr),+ $(,)?) => {
        $crate::i18n::format($crate::i18n::tr_ctx($ctx, $msg), &[$(&$arg as &dyn std::fmt::Display),+])
    };
    ($msg:literal) => {
        $crate::i18n::tr($msg)
    };
    ($msg:literal, $($arg:expr),+ $(,)?) => {
        $crate::i18n::format($crate::i18n::tr($msg), &[$(&$arg as &dyn std::fmt::Display),+])
    };
}

/// A message that depends on a count: `trn!("{} day", "{} days", n)`. The count
/// is the first argument (`{}` or `{0}`); more may follow.
macro_rules! trn {
    ($one:literal, $many:literal, $n:expr $(, $arg:expr)* $(,)?) => {{
        let n = $n;
        $crate::i18n::format($crate::i18n::trn($one, $many, n as u64), &[&n as &dyn std::fmt::Display $(, &$arg as &dyn std::fmt::Display)*])
    }};
}

/// Marks text for translation where it can't be translated yet (constants);
/// pass it through [`tr`] where it's shown.
macro_rules! tr_noop {
    ($ctx:literal => $msg:literal) => {
        $msg
    };
    ($msg:literal) => {
        $msg
    };
}

pub struct Language {
    /// The folder under `lang/` and the setting's value ("en" needs no folder).
    pub code: &'static str,
    /// The language's own name for itself, as the language list shows it.
    pub name: &'static str,
    /// Its English name, translated where it's shown.
    pub english: &'static str,
    /// Which plural form a count takes (the catalog's `Plural-Forms`).
    plural: fn(u64) -> usize,
    catalog: &'static str,
}

macro_rules! catalog {
    ($code:literal) => {
        include_str!(concat!("../lang/", $code, "/LC_MESSAGES/wisp.po"))
    };
}

fn one_other(n: u64) -> usize {
    usize::from(n != 1)
}
fn zero_one_other(n: u64) -> usize {
    usize::from(n > 1)
}
fn only(_: u64) -> usize {
    0
}
fn slavic(n: u64) -> usize {
    if n % 10 == 1 && n % 100 != 11 {
        0
    } else if (2..=4).contains(&(n % 10)) && !(12..=14).contains(&(n % 100)) {
        1
    } else {
        2
    }
}

/// Every language Wisp speaks, in the order the settings list them.
pub const LANGUAGES: &[Language] = &[
    Language { code: "en", name: "English", english: tr_noop!("English"), plural: one_other, catalog: "" },
    Language { code: "de", name: "Deutsch", english: tr_noop!("German"), plural: one_other, catalog: catalog!("de") },
    Language { code: "es", name: "Español", english: tr_noop!("Spanish"), plural: one_other, catalog: catalog!("es") },
    Language { code: "eo", name: "Esperanto", english: tr_noop!("Esperanto"), plural: one_other, catalog: catalog!("eo") },
    Language { code: "fr", name: "Français", english: tr_noop!("French"), plural: zero_one_other, catalog: catalog!("fr") },
    Language { code: "it", name: "Italiano", english: tr_noop!("Italian"), plural: one_other, catalog: catalog!("it") },
    Language {
        code: "pt_BR",
        name: "Português (Brasil)",
        english: tr_noop!("Portuguese (Brazil)"),
        plural: zero_one_other,
        catalog: catalog!("pt_BR"),
    },
    Language { code: "ru", name: "Русский", english: tr_noop!("Russian"), plural: slavic, catalog: catalog!("ru") },
    Language { code: "ja", name: "日本語", english: tr_noop!("Japanese"), plural: only, catalog: catalog!("ja") },
    Language { code: "ko", name: "한국어", english: tr_noop!("Korean"), plural: only, catalog: catalog!("ko") },
    Language { code: "zh_CN", name: "简体中文", english: tr_noop!("Chinese (Simplified)"), plural: only, catalog: catalog!("zh_CN") },
    Language { code: "zh_TW", name: "繁體中文", english: tr_noop!("Chinese (Traditional)"), plural: only, catalog: catalog!("zh_TW") },
];

/// The setting that follows the system's language.
pub const SYSTEM: &str = "system";

thread_local! {
    static CURRENT: Cell<usize> = const { Cell::new(0) };
}

pub fn current() -> &'static Language {
    &LANGUAGES[CURRENT.with(Cell::get)]
}

/// Switches the text made in Rust to `LANGUAGES[index]`.
pub fn set_current(index: usize) {
    CURRENT.with(|c| c.set(index.min(LANGUAGES.len() - 1)));
}

/// Switches Wisp to `choice` (a language code, or [`SYSTEM`]): the text made in
/// Rust and the window text both. Needs the first window to exist.
pub fn apply(choice: &str) -> &'static Language {
    let index = resolve(choice);
    set_current(index);
    let code = LANGUAGES[index].code;
    if let Err(err) = slint::select_bundled_translation(code) {
        eprintln!("wisp: couldn't switch the window text to {code}: {err}");
    }
    &LANGUAGES[index]
}

/// The language a setting stands for. Unknown codes mean English.
pub fn resolve(choice: &str) -> usize {
    if choice == SYSTEM {
        return sys_locale::get_locales().find_map(|l| for_locale(&l)).unwrap_or(0);
    }
    LANGUAGES.iter().position(|l| l.code == choice).unwrap_or(0)
}

/// Which entry of the language list a setting is: `None` for [`SYSTEM`], else the
/// language's index (unknown codes are English).
pub fn resolve_choice(choice: &str) -> Option<usize> {
    (choice != SYSTEM).then(|| resolve(choice))
}

/// Matches a BCP 47 locale ("zh-Hant-TW", "pt-PT", "fr_CA") to a language Wisp has.
fn for_locale(locale: &str) -> Option<usize> {
    let tag = locale.to_lowercase().replace('_', "-");
    let mut parts = tag.split(['-', '.', '@']);
    let base = parts.next()?;
    let code = match base {
        "zh" => {
            let traditional = tag.contains("hant") || ["tw", "hk", "mo"].iter().any(|r| tag.split('-').any(|p| p == *r));
            if traditional {
                "zh_TW"
            } else {
                "zh_CN"
            }
        }
        "pt" => "pt_BR",
        other => other,
    };
    LANGUAGES.iter().position(|l| l.code == code)
}

/// Translates `msgid` into the current language (or leaves it in English).
pub fn tr(msgid: &'static str) -> &'static str {
    lookup(msgid).and_then(|forms| forms.first()).map_or(msgid, String::as_str)
}

/// [`tr`] for a message with a context, which tells apart the same English
/// words used for different things.
pub fn tr_ctx(context: &'static str, msgid: &'static str) -> &'static str {
    let key = format!("{context}\u{4}{msgid}");
    lookup(&key).and_then(|forms| forms.first()).map_or(msgid, String::as_str)
}

/// The form of a counted message that suits `n`.
pub fn trn(one: &'static str, many: &'static str, n: u64) -> &'static str {
    let language = current();
    match lookup(one) {
        Some(forms) if !forms.is_empty() => forms.get((language.plural)(n)).unwrap_or(&forms[0]).as_str(),
        _ if n == 1 => one,
        _ => many,
    }
}

fn lookup(key: &str) -> Option<&'static Vec<String>> {
    static CATALOGS: OnceLock<Vec<HashMap<String, Vec<String>>>> = OnceLock::new();
    let catalogs = CATALOGS.get_or_init(|| LANGUAGES.iter().map(|l| parse_catalog(l.catalog)).collect());
    catalogs[CURRENT.with(Cell::get)].get(key)
}

/// Fills `{}` (in order) and `{0}`, `{1}`… (by position) with `args`.
pub fn format(template: &str, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut next = 0;
    let mut chars = template.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match c {
            '{' if chars.peek().map(|&(_, c)| c) == Some('{') => {
                chars.next();
                out.push('{');
            }
            '}' if chars.peek().map(|&(_, c)| c) == Some('}') => {
                chars.next();
                out.push('}');
            }
            '{' => {
                let Some(close) = template[i..].find('}') else {
                    out.push_str(&template[i..]);
                    break;
                };
                let inside = &template[i + 1..i + close];
                let index = if inside.is_empty() {
                    next += 1;
                    Some(next - 1)
                } else {
                    inside.parse::<usize>().ok()
                };
                match index.and_then(|n| args.get(n)) {
                    Some(arg) => {
                        let _ = write!(out, "{arg}");
                    }
                    None => out.push_str(&template[i..=i + close]),
                }
                while chars.peek().is_some_and(|&(j, _)| j <= i + close) {
                    chars.next();
                }
            }
            c => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Reading .po files

#[derive(Default)]
struct Entry {
    context: Option<String>,
    id: Option<String>,
    plural: Option<String>,
    strings: Vec<String>,
    fuzzy: bool,
}

#[derive(Clone, Copy)]
enum Field {
    None,
    Context,
    Id,
    Plural,
    Str(usize),
}

/// The translated messages of a catalog, keyed by msgid (or "context\u{4}msgid").
/// Untranslated and fuzzy entries are left out, so they fall back to English.
fn parse_catalog(text: &str) -> HashMap<String, Vec<String>> {
    let mut map = HashMap::new();
    for entry in parse_entries(text) {
        let Some(id) = entry.id else { continue };
        if id.is_empty() || entry.fuzzy || entry.strings.iter().all(String::is_empty) {
            continue; // the header, or not translated yet
        }
        let key = match entry.context {
            Some(context) => format!("{context}\u{4}{id}"),
            None => id,
        };
        map.insert(key, entry.strings);
    }
    map
}

fn parse_entries(text: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut entry = Entry::default();
    let mut field = Field::None;
    let finish = |entry: &mut Entry, entries: &mut Vec<Entry>| {
        if entry.id.is_some() {
            entries.push(std::mem::take(entry));
        } else {
            *entry = Entry::default();
        }
    };
    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            finish(&mut entry, &mut entries);
            field = Field::None;
        } else if line.starts_with("#~") {
            continue; // obsolete
        } else if let Some(flags) = line.strip_prefix("#,") {
            if entry.id.is_some() && !entry.strings.is_empty() {
                finish(&mut entry, &mut entries);
            }
            entry.fuzzy |= flags.split(',').any(|f| f.trim() == "fuzzy");
        } else if line.starts_with('#') {
            continue;
        } else if let Some(rest) = line.strip_prefix("msgctxt ") {
            if entry.id.is_some() {
                finish(&mut entry, &mut entries);
            }
            entry.context = Some(unquote(rest));
            field = Field::Context;
        } else if let Some(rest) = line.strip_prefix("msgid_plural ") {
            entry.plural = Some(unquote(rest));
            field = Field::Plural;
        } else if let Some(rest) = line.strip_prefix("msgid ") {
            if entry.id.is_some() {
                finish(&mut entry, &mut entries);
            }
            entry.id = Some(unquote(rest));
            field = Field::Id;
        } else if let Some(rest) = line.strip_prefix("msgstr[") {
            let Some((index, value)) = rest.split_once(']') else { continue };
            let index: usize = index.parse().unwrap_or(0);
            if entry.strings.len() <= index {
                entry.strings.resize(index + 1, String::new());
            }
            entry.strings[index] = unquote(value.trim());
            field = Field::Str(index);
        } else if let Some(rest) = line.strip_prefix("msgstr ") {
            entry.strings = vec![unquote(rest)];
            field = Field::Str(0);
        } else if line.starts_with('"') {
            let more = unquote(line);
            match field {
                Field::Context => entry.context.get_or_insert_with(String::new).push_str(&more),
                Field::Id => entry.id.get_or_insert_with(String::new).push_str(&more),
                Field::Plural => entry.plural.get_or_insert_with(String::new).push_str(&more),
                Field::Str(i) => {
                    if let Some(s) = entry.strings.get_mut(i) {
                        s.push_str(&more);
                    }
                }
                Field::None => {}
            }
        }
    }
    finish(&mut entry, &mut entries);
    entries
}

/// `"a \"b\"\n"` → `a "b"` and a line break.
fn unquote(quoted: &str) -> String {
    let inner = quoted.trim().strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(quoted);
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

#[cfg(test)]
mod tests;
