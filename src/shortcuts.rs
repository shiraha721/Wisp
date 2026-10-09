//! Key combinations: reading them from Slint key events, reading and writing the
//! strings stored in the config ("Ctrl+Alt+Shift+N"), and turning them into
//! system-wide hotkeys.
//!
//! "Ctrl" always means the primary shortcut modifier, which is ⌘ on macOS (Slint
//! reports ⌘ as `control` there), so one config works on every platform.

use crate::i18n;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use std::fmt;
use std::str::FromStr;

#[derive(Clone, Debug, PartialEq, Eq, Default, Hash)]
pub struct Combo {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
    /// "N", "1", ",", "F5", "Enter", "Up", …
    pub key: String,
}

impl Combo {
    /// Builds a combo from a Slint `KeyEvent`. Returns `None` for lone modifier
    /// presses and for text that isn't a single key (input-method output).
    pub fn from_event(text: &str, ctrl: bool, alt: bool, shift: bool, meta: bool) -> Option<Combo> {
        let mut chars = text.chars();
        let c = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        let key = key_from_char(c, shift)?;
        Some(Combo { ctrl, alt, shift, meta, key })
    }

    pub fn is_modified(&self) -> bool {
        self.ctrl || self.alt || self.meta
    }

    pub fn is_function_key(&self) -> bool {
        self.key.len() > 1 && self.key.starts_with('F') && self.key[1..].parse::<u8>().is_ok()
    }

    /// Shortcuts inside the window must not steal plain typing.
    pub fn validate_local(&self) -> Result<(), String> {
        if self.is_modified() || self.is_function_key() || (self.shift && self.key == "Enter") {
            Ok(())
        } else {
            Err(tr!("{} would get in the way of typing. Add Ctrl or Alt.", self.label()))
        }
    }

    /// System-wide hotkeys need a modifier (or a function key) and a key the OS knows.
    pub fn validate_global(&self) -> Result<(), String> {
        if !self.is_modified() && !self.is_function_key() {
            return Err(tr!("{} alone is too easy to hit by accident. Add Ctrl, Alt or Shift+Ctrl.", self.label()));
        }
        self.to_hotkey().map(|_| ())
    }

    pub fn to_hotkey(&self) -> Result<HotKey, String> {
        let (primary, secondary) = if cfg!(target_os = "macos") {
            (Modifiers::SUPER, Modifiers::CONTROL)
        } else {
            (Modifiers::CONTROL, Modifiers::SUPER)
        };
        let mut mods = Modifiers::empty();
        for (on, m) in [(self.ctrl, primary), (self.alt, Modifiers::ALT), (self.shift, Modifiers::SHIFT), (self.meta, secondary)] {
            if on {
                mods |= m;
            }
        }
        let code = code_name(&self.key)
            .and_then(|name| Code::from_str(&name).ok())
            .ok_or_else(|| tr!("{} can't be used as a system-wide key.", self.key))?;
        Ok(HotKey::new(Some(mods), code))
    }

    /// How the combo is shown to people: "Ctrl+Alt+Shift+N" or "⌘⌥⇧N" on macOS.
    pub fn label(&self) -> String {
        let parts = self.parts();
        if cfg!(target_os = "macos") {
            parts.concat()
        } else {
            parts.join("+")
        }
    }

    /// The keys one by one, as printed on keycaps: ["Ctrl", "Alt", "N"], ["Strg", "Alt", "N"]
    /// in German, or ["⌘", "⌥", "N"] on macOS.
    pub fn parts(&self) -> Vec<String> {
        let key = match self.key.as_str() {
            "Up" => "↑".to_owned(),
            "Down" => "↓".to_owned(),
            "Left" => "←".to_owned(),
            "Right" => "→".to_owned(),
            "Delete" if cfg!(target_os = "macos") => "⌦".to_owned(),
            "Backspace" if cfg!(target_os = "macos") => "⌫".to_owned(),
            "Enter" if cfg!(target_os = "macos") => "↩".to_owned(),
            k => key_name(k),
        };
        let modifiers: [(bool, &str); 4] = if cfg!(target_os = "macos") {
            [(self.meta, "⌃"), (self.alt, "⌥"), (self.shift, "⇧"), (self.ctrl, "⌘")]
        } else {
            let meta = if cfg!(windows) { "Win" } else { "Super" };
            [(self.ctrl, "Ctrl"), (self.alt, "Alt"), (self.shift, "Shift"), (self.meta, meta)]
        };
        modifiers
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, name)| key_name(name))
            .chain(std::iter::once(key))
            .collect()
    }
}

/// The stored form: always "Ctrl+Alt+Shift+Super+Key", whatever the platform.
impl fmt::Display for Combo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (on, name) in [(self.ctrl, "Ctrl"), (self.alt, "Alt"), (self.shift, "Shift"), (self.meta, "Super")] {
            if on {
                write!(f, "{name}+")?;
            }
        }
        f.write_str(&self.key)
    }
}

impl FromStr for Combo {
    type Err = String;

    fn from_str(s: &str) -> Result<Combo, String> {
        let s = s.trim();
        // "Ctrl++" means the plus key.
        let (mods, key) = match s.strip_suffix("++") {
            Some(head) => (head, "+"),
            None => match s.rsplit_once('+') {
                Some((head, key)) => (head, key),
                None => ("", s),
            },
        };
        let mut combo = Combo::default();
        for m in mods.split('+').map(str::trim).filter(|m| !m.is_empty()) {
            match m.to_lowercase().as_str() {
                "ctrl" | "control" | "primary" | "cmdorctrl" => combo.ctrl = true,
                "cmd" | "command" if cfg!(target_os = "macos") => combo.ctrl = true,
                "alt" | "option" | "opt" => combo.alt = true,
                "shift" => combo.shift = true,
                "super" | "meta" | "win" | "windows" | "cmd" | "command" => combo.meta = true,
                _ => return Err(format!("Unknown modifier “{m}”.")),
            }
        }
        combo.key = key_from_name(key.trim()).ok_or_else(|| format!("Unknown key “{key}”."))?;
        Ok(combo)
    }
}

/// Key names as keyboards print them, which differs between languages ("Strg", "Entf", "Échap").
const KEY_NAMES: [&str; 17] = [
    tr_noop!("key" => "Ctrl"),
    tr_noop!("key" => "Alt"),
    tr_noop!("key" => "Shift"),
    tr_noop!("key" => "Win"),
    tr_noop!("key" => "Super"),
    tr_noop!("key" => "Enter"),
    tr_noop!("key" => "Escape"),
    tr_noop!("key" => "Tab"),
    tr_noop!("key" => "Space"),
    tr_noop!("key" => "Backspace"),
    tr_noop!("key" => "Delete"),
    tr_noop!("key" => "Insert"),
    tr_noop!("key" => "Home"),
    tr_noop!("key" => "End"),
    tr_noop!("key" => "PageUp"),
    tr_noop!("key" => "PageDown"),
    tr_noop!("key" => "Esc"),
];

/// How a key is printed in the language Wisp is shown in.
pub fn key_name(name: &str) -> String {
    KEY_NAMES.iter().find(|k| **k == name).map_or_else(|| name.to_owned(), |k| i18n::tr_ctx("key", k).to_owned())
}

const NAMED: &[(&str, &[&str])] = &[
    ("Enter", &["enter", "return"]),
    ("Escape", &["escape", "esc"]),
    ("Tab", &["tab"]),
    ("Space", &["space", "spacebar"]),
    ("Backspace", &["backspace"]),
    ("Delete", &["delete", "del"]),
    ("Insert", &["insert", "ins"]),
    ("Home", &["home"]),
    ("End", &["end"]),
    ("PageUp", &["pageup", "pgup"]),
    ("PageDown", &["pagedown", "pgdn"]),
    ("Up", &["up", "arrowup"]),
    ("Down", &["down", "arrowdown"]),
    ("Left", &["left", "arrowleft"]),
    ("Right", &["right", "arrowright"]),
    (",", &["comma"]),
    (".", &["period", "dot"]),
    ("=", &["equal", "plus"]),
    ("-", &["minus"]),
    ("/", &["slash"]),
    ("`", &["backquote", "backtick"]),
];

fn key_from_name(name: &str) -> Option<String> {
    let lower = name.to_lowercase();
    if let Some((canonical, _)) = NAMED.iter().find(|(_, aliases)| aliases.contains(&lower.as_str())) {
        return Some((*canonical).to_owned());
    }
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
        return (1..=24).contains(&n).then(|| format!("F{n}"));
    }
    let mut chars = name.chars();
    let c = chars.next()?;
    if chars.next().is_some() || c.is_control() {
        return None;
    }
    key_from_char(c, true)
}

/// Slint key text → key name. Special keys arrive as private-use code points.
fn key_from_char(c: char, shift: bool) -> Option<String> {
    let named = match c {
        '\u{8}' => "Backspace",
        '\t' | '\u{19}' => "Tab",
        '\n' | '\r' => "Enter",
        '\u{1b}' => "Escape",
        '\u{7f}' => "Delete",
        ' ' => "Space",
        '\u{F700}' => "Up",
        '\u{F701}' => "Down",
        '\u{F702}' => "Left",
        '\u{F703}' => "Right",
        '\u{F727}' => "Insert",
        '\u{F729}' => "Home",
        '\u{F72B}' => "End",
        '\u{F72C}' => "PageUp",
        '\u{F72D}' => "PageDown",
        '\u{F704}'..='\u{F71B}' => return Some(format!("F{}", c as u32 - 0xF704 + 1)),
        c if c.is_control() || ('\u{E000}'..='\u{F8FF}').contains(&c) => return None,
        c => {
            // Chinese and Japanese input methods turn punctuation into full-width forms.
            let c = narrow(c);
            // With Shift held, the event carries the shifted symbol ("<" for ","); store
            // the key itself so "Ctrl+Shift+," matches and maps to a physical key.
            let c = if shift { unshift(c) } else { c };
            return Some(c.to_uppercase().collect());
        }
    };
    Some(named.to_owned())
}

/// Full-width and CJK punctuation → the ASCII key that typed it ("，" → ",").
fn narrow(c: char) -> char {
    match c {
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
        '\u{3000}' => ' ',
        '。' => '.',
        '、' => '\\',
        '【' | '「' => '[',
        '】' | '」' => ']',
        '·' => '`',
        '《' => '<',
        '》' => '>',
        '‘' | '’' => '\'',
        '“' | '”' => '"',
        '￥' => '$',
        c => c,
    }
}

/// US-layout shifted symbol → its key.
fn unshift(c: char) -> char {
    const PAIRS: &str = "!1@2#3$4%5^6&7*8(9)0_-+={[}]|\\:;\"'<,>.?/~`";
    let chars: Vec<char> = PAIRS.chars().collect();
    chars.chunks(2).find(|p| p[0] == c).map_or(c, |p| p[1])
}

/// Key name → W3C `KeyboardEvent.code` name, which `global_hotkey::Code` parses.
fn code_name(key: &str) -> Option<String> {
    let mut chars = key.chars();
    let first = chars.next()?;
    if chars.next().is_none() {
        return Some(match first {
            'A'..='Z' => format!("Key{first}"),
            '0'..='9' => format!("Digit{first}"),
            ',' => "Comma".into(),
            '.' => "Period".into(),
            '/' => "Slash".into(),
            ';' => "Semicolon".into(),
            '\'' => "Quote".into(),
            '[' => "BracketLeft".into(),
            ']' => "BracketRight".into(),
            '\\' => "Backslash".into(),
            '-' => "Minus".into(),
            '=' => "Equal".into(),
            '`' => "Backquote".into(),
            _ => return None,
        });
    }
    Some(match key {
        "Up" => "ArrowUp".into(),
        "Down" => "ArrowDown".into(),
        "Left" => "ArrowLeft".into(),
        "Right" => "ArrowRight".into(),
        other => other.to_owned(), // Enter, Space, Tab, F5, Home, …
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(s: &str) -> Combo {
        s.parse().unwrap()
    }

    #[test]
    fn parse_and_print() {
        assert_eq!(c("ctrl+alt+shift+n").to_string(), "Ctrl+Alt+Shift+N");
        assert_eq!(c("Shift + Ctrl + Enter").to_string(), "Ctrl+Shift+Enter");
        assert_eq!(c("Ctrl+,").to_string(), "Ctrl+,");
        assert_eq!(c("Ctrl+comma"), c("Ctrl+,"));
        assert_eq!(c("Ctrl+Del").to_string(), "Ctrl+Delete");
        assert_eq!(c("f1").to_string(), "F1");
        assert_eq!(c("Ctrl++").key, "=");
        assert!("Hyper+X".parse::<Combo>().is_err());
        assert!("Ctrl+Bogus".parse::<Combo>().is_err());
    }

    #[test]
    fn events_match_config_strings() {
        // Ctrl+Shift+, arrives as "<" with shift held.
        assert_eq!(Combo::from_event("<", true, false, true, false), Some(c("Ctrl+Shift+,")));
        assert_eq!(Combo::from_event("d", true, false, false, false), Some(c("Ctrl+D")));
        assert_eq!(Combo::from_event("\n", true, false, false, false), Some(c("Ctrl+Enter")));
        assert_eq!(Combo::from_event("\u{F704}", false, false, false, false), Some(c("F1")));
        assert_eq!(Combo::from_event("\u{F701}", false, true, false, false), Some(c("Alt+Down")));
        assert_eq!(Combo::from_event("\u{11}", true, false, false, false), None, "lone modifier");
        assert_eq!(Combo::from_event("你好", false, false, false, false), None);
        // Chinese input methods send full-width punctuation.
        assert_eq!(Combo::from_event("，", true, false, false, false), Some(c("Ctrl+,")));
        assert_eq!(Combo::from_event("《", true, false, true, false), Some(c("Ctrl+Shift+,")));
        assert_eq!(Combo::from_event("、", true, false, false, false), Some(c("Ctrl+\\")));
    }

    #[test]
    fn keycap_parts() {
        let parts = c("Ctrl+Alt+Shift+N").parts();
        if cfg!(target_os = "macos") {
            assert_eq!(parts, ["⌘", "⌥", "⇧", "N"]);
        } else {
            assert_eq!(parts, ["Ctrl", "Alt", "Shift", "N"]);
            assert_eq!(c("Ctrl+Down").label(), "Ctrl+↓");
        }
    }

    #[test]
    fn validation() {
        assert!(c("Ctrl+D").validate_local().is_ok());
        assert!(c("F2").validate_local().is_ok());
        assert!(c("D").validate_local().is_err());
        assert!(c("Shift+D").validate_local().is_err());
        assert!(c("Ctrl+Alt+Shift+N").validate_global().is_ok());
        assert!(c("Super+Space").validate_global().is_ok());
        assert!(c("Shift+N").validate_global().is_err());
    }

    #[test]
    fn hotkey_codes() {
        let hk = c("Ctrl+Alt+Shift+N").to_hotkey().unwrap();
        assert_eq!(hk.key, Code::KeyN);
        assert_eq!(c("Alt+Up").to_hotkey().unwrap().key, Code::ArrowUp);
        assert_eq!(c("Ctrl+`").to_hotkey().unwrap().key, Code::Backquote);
        assert_eq!(c("Ctrl+F12").to_hotkey().unwrap().key, Code::F12);
    }
}
