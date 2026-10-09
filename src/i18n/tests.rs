//! The translation machinery's tests, and the check that keeps the catalogs in
//! step with the code: every message marked in `src/` and `ui/` must be in
//! `lang/wisp.pot`, and every catalog must translate all of them with the same
//! placeholders, date patterns chrono understands and date examples Wisp reads.
//!
//! `WISP_UPDATE_CATALOGS=1 cargo test catalogs` rewrites `lang/wisp.pot` and
//! brings every catalog up to date (new messages untranslated, old ones gone)
//! instead of failing.

use super::*;
use chrono::TimeZone;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

#[test]
fn placeholder_filling() {
    assert_eq!(format("{} of {}", &[&3, &10]), "3 of 10");
    assert_eq!(format("{1} 中的 {0}", &[&3, &10]), "10 中的 3");
    assert_eq!(format("{{literal}} {}", &[&"x"]), "{literal} x");
    assert_eq!(format("{} and {9}", &[&1]), "1 and {9}", "a missing argument stays visible");
}

#[test]
fn catalogs_are_read() {
    let po = r#"
msgid ""
msgstr ""
"Plural-Forms: nplurals=3; plural=0;\n"

#. a note
#: src/app.rs
msgid "Saved."
msgstr "Gespeichert."

msgctxt "key"
msgid "Ctrl"
msgstr "Strg"

msgid "{} day"
msgid_plural "{} days"
msgstr[0] "{} день"
msgstr[1] "{} дня"
msgstr[2] "{} дней"

#, fuzzy
msgid "Undone."
msgstr "Rückgängig."

msgid "A long "
"message"
msgstr "Eine lange "
"Nachricht \"mit\" Zeilen\n"

msgid "Untranslated"
msgstr ""
"#;
    let catalog = parse_catalog(po);
    assert_eq!(catalog["Saved."], ["Gespeichert."]);
    assert_eq!(catalog["key\u{4}Ctrl"], ["Strg"]);
    assert_eq!(catalog["{} day"], ["{} день", "{} дня", "{} дней"]);
    assert_eq!(catalog["A long message"], ["Eine lange Nachricht \"mit\" Zeilen\n"]);
    assert!(!catalog.contains_key("Undone."), "fuzzy entries wait for review");
    assert!(!catalog.contains_key("Untranslated"));
    assert!(!catalog.contains_key(""), "the header isn't a message");
}

#[test]
fn languages_follow_the_system_locale() {
    let code = |locale| for_locale(locale).map(|i| LANGUAGES[i].code);
    assert_eq!(code("zh-CN"), Some("zh_CN"));
    assert_eq!(code("zh-Hans-SG"), Some("zh_CN"));
    assert_eq!(code("zh-Hant-TW"), Some("zh_TW"));
    assert_eq!(code("zh-HK"), Some("zh_TW"));
    assert_eq!(code("pt-PT"), Some("pt_BR"));
    assert_eq!(code("fr_CA.UTF-8"), Some("fr"));
    assert_eq!(code("en-GB"), Some("en"));
    assert_eq!(code("sv-SE"), None);
    assert_eq!(resolve("ja"), LANGUAGES.iter().position(|l| l.code == "ja").unwrap());
    assert_eq!(resolve("klingon"), 0, "unknown settings fall back to English");
    assert_eq!(resolve_choice(SYSTEM), None);
}

#[test]
fn plural_rules_match_the_catalog_headers() {
    for (i, language) in LANGUAGES.iter().enumerate().skip(1) {
        let forms = (0..1000).map(language.plural).max().unwrap() + 1;
        let header = header_plurals(language.catalog);
        assert_eq!(Some(forms), header, "{}: the rule gives {forms} forms", language.code);
        set_current(i);
        assert!(!trn("{} day", "{} days", 1).is_empty());
    }
    set_current(0);
    assert_eq!(slavic(1), 0);
    assert_eq!(slavic(3), 1);
    assert_eq!(slavic(12), 2);
    assert_eq!(slavic(21), 0);
    assert_eq!(slavic(25), 2);
}

#[test]
fn messages_are_translated() {
    set_current(LANGUAGES.iter().position(|l| l.code == "de").unwrap());
    assert_eq!(tr("Saved."), "Gespeichert.");
    assert_eq!(tr_ctx("key", "Ctrl"), "Strg");
    assert_eq!(tr("not a message"), "not a message", "unknown text stays English");
    set_current(0);
    assert_eq!(tr("Saved."), "Saved.");
    assert_eq!(trn("{} day", "{} days", 1), "{} day");
    assert_eq!(trn("{} day", "{} days", 2), "{} days");
}

fn header_plurals(catalog: &str) -> Option<usize> {
    let at = catalog.find("nplurals=")? + "nplurals=".len();
    catalog[at..].chars().take_while(char::is_ascii_digit).collect::<String>().parse().ok()
}

// ---------------------------------------------------------------------------
// The catalogs and the code

/// A message as marked in the code, in its escaped (source and .po) form.
#[derive(Debug, Clone)]
struct Message {
    context: Option<String>,
    id: String,
    plural: Option<String>,
    files: BTreeSet<String>,
    notes: Vec<String>,
}

impl Message {
    fn key(&self) -> (Option<String>, String) {
        (self.context.as_deref().map(unescape), unescape(&self.id))
    }
}

fn unescape(raw: &str) -> String {
    unquote(&format!("\"{raw}\""))
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\t', "\\t")
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn files_in(dir: &Path, extension: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_in(&path, extension, out);
        } else if path.extension().is_some_and(|e| e == extension) {
            out.push(path);
        }
    }
}

/// Every message marked in the code, in the order they first appear.
fn source_messages() -> Vec<Message> {
    let mut files = Vec::new();
    files_in(&root().join("src"), "rs", &mut files);
    files_in(&root().join("ui"), "slint", &mut files);
    files.sort();
    let mut messages: Vec<Message> = Vec::new();
    for path in files {
        let relative = path.strip_prefix(root()).unwrap().to_string_lossy().replace('\\', "/");
        if relative == "src/i18n/tests.rs" {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let slint = relative.ends_with(".slint");
        for found in extract(&text, slint) {
            let mut found = found;
            found.files.insert(relative.clone());
            match messages.iter_mut().find(|m| m.context == found.context && m.id == found.id) {
                Some(known) => {
                    known.files.extend(found.files);
                    for note in found.notes {
                        if !known.notes.contains(&note) {
                            known.notes.push(note);
                        }
                    }
                }
                None => messages.push(found),
            }
        }
    }
    messages
}

/// The messages in one file: `tr!(…)`, `trn!(…)` and `tr_noop!(…)` in Rust,
/// `@tr(…)` in Slint, each with its "Translators:" comment, if any.
fn extract(text: &str, slint: bool) -> Vec<Message> {
    let markers: &[&str] = if slint { &["@tr("] } else { &["tr!(", "trn!(", "tr_noop!("] };
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    let mut at = 0;
    while let Some((pos, marker)) = markers.iter().filter_map(|m| text[at..].find(m).map(|p| (at + p, *m))).min_by_key(|(p, _)| *p) {
        at = pos + marker.len();
        if text[..pos].chars().last().is_some_and(|c| c.is_alphanumeric() || c == '_') {
            continue; // part of a longer name
        }
        let line_start = text[..pos].rfind('\n').map_or(0, |n| n + 1);
        if text[line_start..pos].contains("//") {
            continue; // in a comment
        }
        let Some((first, mut cursor)) = literal(text, at) else { continue };
        let (context, id) = match skip_space(text, cursor).strip_prefix("=>") {
            Some(_) => {
                let arrow = text.len() - skip_space(text, cursor).len() + 2;
                let Some((second, after)) = literal(text, arrow) else { continue };
                cursor = after;
                (Some(first), second)
            }
            None => (None, first),
        };
        let plural = if marker == "trn!(" {
            let rest = skip_space(text, cursor);
            let comma = text.len() - rest.len();
            rest.starts_with(',').then(|| literal(text, comma + 1)).flatten().map(|(p, _)| p)
        } else {
            None
        };
        // A "Translators:" comment right above the line explains the message.
        let line = text[..pos].matches('\n').count();
        let mut comment = Vec::new();
        for l in lines[..line].iter().rev() {
            match l.trim().strip_prefix("//") {
                Some(c) => comment.push(c.trim().to_owned()),
                None => break,
            }
        }
        comment.reverse();
        let notes = match comment.iter().position(|c| c.starts_with("Translators:")) {
            Some(from) => vec![comment[from..].join(" ").trim_start_matches("Translators:").trim().to_owned()],
            None => Vec::new(),
        };
        found.push(Message { context, id, plural, files: BTreeSet::new(), notes });
    }
    found
}

fn skip_space(text: &str, at: usize) -> &str {
    text[at..].trim_start()
}

/// The string literal starting at `at` (after spaces): its escaped content and where it ends.
fn literal(text: &str, at: usize) -> Option<(String, usize)> {
    let rest = skip_space(text, at);
    let start = text.len() - rest.len();
    let mut chars = rest.char_indices();
    if chars.next()?.1 != '"' {
        return None;
    }
    let mut escaped = false;
    for (i, c) in chars {
        match c {
            '\\' if !escaped => escaped = true,
            '"' if !escaped => return Some((rest[1..i].to_owned(), start + i + 1)),
            _ => escaped = false,
        }
    }
    None
}

/// Notes every message of a kind gets, so translators know the rules.
fn context_note(context: Option<&str>) -> Option<&'static str> {
    match context? {
        "date format" => Some(
            "A chrono date pattern: %a is the weekday and %b the month (both from this catalog), %-d the day, %-m the month as a number, %Y the year, %H:%M the time.",
        ),
        "date example" => Some(
            "Dates Wisp must read in your language (the tests check). Separate several with commas. Words for dates are in src/dates/words.rs.",
        ),
        "key" => Some("A key's name as keyboards in your language print it."),
        "weekday" => Some("A short weekday name, as in “Fri 17:00”."),
        "month" => Some("A short month name, as in “Oct 12”. Use the form a date takes, if your language changes it."),
        _ => None,
    }
}

fn write_entry(out: &mut String, message: &Message, translations: &[String], forms: usize) {
    out.push('\n');
    for note in context_note(message.context.as_deref()).into_iter().chain(message.notes.iter().map(String::as_str)) {
        out.push_str(&format!("#. {note}\n"));
    }
    out.push_str(&format!("#: {}\n", message.files.iter().cloned().collect::<Vec<_>>().join(" ")));
    if let Some(context) = &message.context {
        out.push_str(&format!("msgctxt \"{context}\"\n"));
    }
    out.push_str(&format!("msgid \"{}\"\n", message.id));
    let text = |i: usize| translations.get(i).map(|t| escape(t)).unwrap_or_default();
    match &message.plural {
        Some(plural) => {
            out.push_str(&format!("msgid_plural \"{plural}\"\n"));
            for i in 0..forms {
                out.push_str(&format!("msgstr[{i}] \"{}\"\n", text(i)));
            }
        }
        None => out.push_str(&format!("msgstr \"{}\"\n", text(0))),
    }
}

fn template(messages: &[Message]) -> String {
    let mut out = String::from(
        "# Wisp's messages: the template every translation starts from.\n\
         # Made by `WISP_UPDATE_CATALOGS=1 cargo test catalogs`; see docs/translating.md.\n\
         msgid \"\"\n\
         msgstr \"\"\n\
         \"Project-Id-Version: Wisp\\n\"\n\
         \"MIME-Version: 1.0\\n\"\n\
         \"Content-Type: text/plain; charset=UTF-8\\n\"\n\
         \"Content-Transfer-Encoding: 8bit\\n\"\n\
         \"Plural-Forms: nplurals=2; plural=(n != 1);\\n\"\n",
    );
    for message in messages {
        write_entry(&mut out, message, &[], 2);
    }
    out
}

/// A catalog's header block (comments and the `msgid ""` entry).
fn header(catalog: &str) -> &str {
    let entry = catalog.find("msgid \"\"").unwrap_or(0);
    match catalog[entry..].find("\n\n") {
        Some(end) => &catalog[..entry + end + 1],
        None => catalog,
    }
}

type Translations = HashMap<(Option<String>, String), (Vec<String>, bool)>;

fn translations(catalog: &str) -> Translations {
    parse_entries(catalog)
        .into_iter()
        .filter(|e| e.id.as_deref().is_some_and(|id| !id.is_empty()))
        .map(|e| ((e.context, e.id.unwrap_or_default()), (e.strings, e.fuzzy)))
        .collect()
}

fn merged(catalog: &str, messages: &[Message]) -> String {
    let known = translations(catalog);
    let forms = header_plurals(catalog).unwrap_or(2);
    let mut out = header(catalog).to_owned();
    for message in messages {
        let strings = known.get(&message.key()).map(|(s, _)| s.clone()).unwrap_or_default();
        write_entry(&mut out, message, &strings, forms);
    }
    out
}

/// `{}`, `{0}`, `{1}`… in a message.
fn placeholders(text: &str) -> BTreeSet<usize> {
    let mut found = BTreeSet::new();
    let mut next = 0;
    let mut rest = text.replace("{{", "").replace("}}", "");
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else { break };
        let inside = &rest[open + 1..open + close];
        if inside.is_empty() {
            found.insert(next);
            next += 1;
        } else if let Ok(n) = inside.parse() {
            found.insert(n);
        }
        rest = rest[open + close + 1..].to_owned();
    }
    found
}

/// What's wrong with one catalog.
fn problems(index: usize, catalog: &str, messages: &[Message]) -> Vec<String> {
    let language = &LANGUAGES[index];
    let mut problems = Vec::new();
    let known = translations(catalog);
    let forms = header_plurals(catalog).unwrap_or(0);
    let wanted: BTreeSet<_> = messages.iter().map(Message::key).collect();
    for key in known.keys().filter(|k| !wanted.contains(*k)) {
        problems.push(format!("no longer used: {key:?}"));
    }
    set_current(index);
    let sample = chrono::FixedOffset::east_opt(3600).unwrap().with_ymd_and_hms(2026, 10, 9, 17, 5, 0).unwrap();
    for message in messages {
        let key = message.key();
        let Some((strings, fuzzy)) = known.get(&key) else {
            problems.push(format!("missing: {key:?}"));
            continue;
        };
        if *fuzzy || strings.is_empty() || strings.iter().any(String::is_empty) {
            problems.push(format!("not translated: {key:?}"));
            continue;
        }
        let expected = placeholders(&key.1);
        match &message.plural {
            Some(plural) => {
                let allowed: BTreeSet<usize> = expected.union(&placeholders(&unescape(plural))).copied().collect();
                if strings.len() != forms {
                    problems.push(format!("{key:?} has {} plural forms, the header says {forms}", strings.len()));
                }
                for s in strings {
                    if !placeholders(s).is_subset(&allowed) {
                        problems.push(format!("{key:?}: “{s}” has other placeholders"));
                    }
                }
            }
            None if placeholders(&strings[0]) != expected => {
                problems.push(format!("{key:?}: “{}” has other placeholders", strings[0]));
            }
            None => {}
        }
        let text = &strings[0];
        match key.0.as_deref() {
            Some("date format") if crate::dates::try_format(&sample, text).is_none() => {
                problems.push(format!("{key:?}: chrono can't read “{text}”"));
            }
            Some("date example") => {
                for example in text.split([',', '、', '，', ';', '；']).map(str::trim).filter(|e| !e.is_empty()) {
                    let now = chrono::Local::now();
                    match crate::dates::parse(example, &now, crate::dates::hm(9, 0)) {
                        Ok(Some(_)) => {}
                        other => problems.push(format!("{key:?}: Wisp can't read “{example}” in {}: {other:?}", language.code)),
                    }
                }
            }
            _ => {}
        }
    }
    set_current(0);
    problems
}

#[test]
fn catalogs_match_the_code() {
    let messages = source_messages();
    assert!(messages.len() > 100, "found only {} messages; is the extractor broken?", messages.len());
    let template_path = root().join("lang/wisp.pot");
    let update = std::env::var_os("WISP_UPDATE_CATALOGS").is_some();
    if update {
        std::fs::write(&template_path, template(&messages)).unwrap();
    } else {
        let current = std::fs::read_to_string(&template_path).unwrap_or_default();
        assert!(
            current == template(&messages),
            "lang/wisp.pot is out of date. Run `WISP_UPDATE_CATALOGS=1 cargo test catalogs` and translate what's new."
        );
    }

    // The English examples have to work too.
    let english = problems(0, &template_with_english(&messages), &messages);
    assert!(english.is_empty(), "English:\n{}", english.join("\n"));

    let mut report = Vec::new();
    for (index, language) in LANGUAGES.iter().enumerate().skip(1) {
        let path = root().join(format!("lang/{}/LC_MESSAGES/wisp.po", language.code));
        let catalog = std::fs::read_to_string(&path).unwrap();
        if update {
            let fresh = merged(&catalog, &messages);
            if fresh != catalog {
                std::fs::write(&path, fresh).unwrap();
            }
            let missing = translations(&std::fs::read_to_string(&path).unwrap())
                .values()
                .filter(|(s, fuzzy)| *fuzzy || s.iter().all(String::is_empty))
                .count();
            eprintln!("{}: {missing} to translate", language.code);
            continue;
        }
        for problem in problems(index, &catalog, &messages) {
            report.push(format!("{}: {problem}", language.code));
        }
    }
    assert!(report.is_empty(), "{} problems:\n{}", report.len(), report.join("\n"));
}

/// The template with each message "translated" into itself, to check the English.
fn template_with_english(messages: &[Message]) -> String {
    let mut out = String::from("msgid \"\"\nmsgstr \"\"\n\"Plural-Forms: nplurals=2; plural=(n != 1);\\n\"\n");
    for message in messages {
        let mut strings = vec![unescape(&message.id)];
        if let Some(plural) = &message.plural {
            strings.push(unescape(plural));
        }
        write_entry(&mut out, message, &strings, 2);
    }
    out
}
