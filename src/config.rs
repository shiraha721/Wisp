//! User settings: the summon hotkey, in-app shortcuts and a few switches.

use crate::shortcuts::Combo;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The system-wide key that brings Wisp up. Ctrl+Alt+Shift is left alone by
/// Windows, macOS (as ⌘⌥⇧), GNOME, KDE, browsers, office suites and IDEs, and it
/// doesn't collide with input-method switching (Ctrl+Space, Ctrl+Shift, Win+Space).
pub const DEFAULT_SUMMON: &str = "Ctrl+Alt+Shift+N";

/// Something a shortcut can do inside the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Schedule,
    Open,
    ToggleDone,
    Delete,
    Undo,
    Inbox,
    Archive,
    Settings,
    Help,
    AddReminder,
    Quit,
}

pub struct ActionInfo {
    pub action: Action,
    pub id: &'static str,
    pub label: &'static str,
    pub default: &'static str,
}

pub const ACTIONS: &[ActionInfo] = &[
    ActionInfo { action: Action::Schedule, id: "schedule", label: "Save and add dates", default: "Ctrl+Enter" },
    ActionInfo { action: Action::Open, id: "open", label: "Open the selected note", default: "Ctrl+E" },
    ActionInfo { action: Action::ToggleDone, id: "toggle_done", label: "Mark done / bring back", default: "Ctrl+D" },
    ActionInfo { action: Action::Delete, id: "delete", label: "Delete", default: "Ctrl+Delete" },
    ActionInfo { action: Action::Undo, id: "undo", label: "Undo", default: "Ctrl+Z" },
    ActionInfo { action: Action::Inbox, id: "inbox", label: "Show notes", default: "Ctrl+1" },
    ActionInfo { action: Action::Archive, id: "archive", label: "Show done", default: "Ctrl+2" },
    ActionInfo { action: Action::Settings, id: "settings", label: "Settings", default: "Ctrl+," },
    ActionInfo { action: Action::Help, id: "help", label: "Keyboard shortcuts", default: "F1" },
    ActionInfo { action: Action::AddReminder, id: "add_reminder", label: "Add a reminder", default: "Ctrl+R" },
    ActionInfo { action: Action::Quit, id: "quit", label: "Quit Wisp", default: "Ctrl+Q" },
];

pub fn action_info(action: Action) -> &'static ActionInfo {
    ACTIONS.iter().find(|a| a.action == action).expect("every action is listed")
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub summon: String,
    /// Action id → combo. Missing entries use the default.
    pub shortcuts: BTreeMap<String, String>,
    pub launch_at_startup: bool,
    pub hide_when_inactive: bool,
    pub theme: Theme,
    /// Set after the first start, when autostart is switched on.
    pub initialized: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            summon: DEFAULT_SUMMON.into(),
            shortcuts: BTreeMap::new(),
            launch_at_startup: true,
            hide_when_inactive: true,
            theme: Theme::System,
            initialized: false,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> (Config, Option<String>) {
        match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(cfg) => (cfg, None),
                Err(err) => (Config::default(), Some(format!("Settings file has an error ({err}); using defaults."))),
            },
            Err(_) => (Config::default(), None),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp: PathBuf = path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    pub fn summon_combo(&self) -> Combo {
        self.summon.parse().unwrap_or_else(|_| DEFAULT_SUMMON.parse().expect("default parses"))
    }

    /// The combo bound to `action` (falling back to the default if the stored one is unreadable).
    pub fn combo(&self, action: Action) -> Combo {
        let info = action_info(action);
        self.shortcuts
            .get(info.id)
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| info.default.parse().expect("defaults parse"))
    }

    pub fn action_for(&self, combo: &Combo) -> Option<Action> {
        ACTIONS.iter().map(|a| a.action).find(|&a| self.combo(a) == *combo)
    }

    /// Which binding (action id or "summon") already uses `combo`, other than `except`.
    pub fn conflict(&self, combo: &Combo, except: &str) -> Option<&'static str> {
        if except != "summon" && self.summon_combo() == *combo {
            return Some("Summon Wisp");
        }
        ACTIONS.iter().find(|a| a.id != except && self.combo(a.action) == *combo).map(|a| a.label)
    }

    pub fn set_shortcut(&mut self, id: &str, combo: &Combo) {
        let default = ACTIONS.iter().find(|a| a.id == id).map(|a| a.default);
        if default.and_then(|d| d.parse::<Combo>().ok()).as_ref() == Some(combo) {
            self.shortcuts.remove(id);
        } else {
            self.shortcuts.insert(id.to_owned(), combo.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_distinct() {
        let cfg = Config::default();
        assert!(cfg.summon_combo().validate_global().is_ok());
        let mut seen = std::collections::HashSet::new();
        for a in ACTIONS {
            let combo: Combo = a.default.parse().unwrap();
            assert!(combo.validate_local().is_ok(), "{}", a.id);
            assert!(seen.insert(combo), "duplicate default for {}", a.id);
        }
        assert!(!seen.contains(&cfg.summon_combo()));
    }

    #[test]
    fn overrides_and_conflicts() {
        let mut cfg = Config::default();
        let ctrl_k: Combo = "Ctrl+K".parse().unwrap();
        cfg.set_shortcut("toggle_done", &ctrl_k);
        assert_eq!(cfg.action_for(&ctrl_k), Some(Action::ToggleDone));
        assert_eq!(cfg.action_for(&"Ctrl+D".parse().unwrap()), None);
        assert_eq!(cfg.conflict(&ctrl_k, "undo"), Some("Mark done / bring back"));
        assert_eq!(cfg.conflict(&ctrl_k, "toggle_done"), None);
        cfg.set_shortcut("toggle_done", &"Ctrl+D".parse().unwrap());
        assert!(cfg.shortcuts.is_empty(), "back to default drops the override");
    }
}
