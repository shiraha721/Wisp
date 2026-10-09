//! User settings: the summon hotkey, in-app shortcuts and a few switches.

use crate::shortcuts::Combo;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The system-wide key that brings Wisp up: two keys, N for note. Neither Windows
/// nor the Linux desktops use Alt+N, launchers (Flow Launcher, PowerToys Run,
/// uTools) sit on Alt+Space, input methods switch with Ctrl+Space, Ctrl+Shift or
/// Win+Space, and AltGr+N (Ctrl+Alt, for ń and friends) is a different combination.
/// On macOS ⌥N types a tilde, so it's ⌃⌥N there.
pub const DEFAULT_SUMMON: &str = if cfg!(target_os = "macos") { "Alt+Super+N" } else { "Alt+N" };

/// Used when another program already holds the default, so a fresh Wisp can always
/// be summoned. Hardly anything claims all three modifiers.
pub const FALLBACK_SUMMON: &str = "Ctrl+Alt+Shift+N";

/// Bumped when stored settings need upgrading (see [`Config::upgrade`]).
const CONFIG_VERSION: u32 = 1;

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
    RemoveReminder,
    Quit,
}

pub struct ActionInfo {
    pub action: Action,
    pub id: &'static str,
    /// Settings groups shortcuts by what they're for.
    pub group: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub default: &'static str,
}

pub const ACTIONS: &[ActionInfo] = &[
    ActionInfo {
        action: Action::Schedule,
        id: "schedule",
        group: "Writing",
        label: "Save and add dates",
        description: "Saves what you typed and opens its start, due time and reminders",
        default: "Ctrl+Enter",
    },
    ActionInfo {
        action: Action::Open,
        id: "open",
        group: "Your notes",
        label: "Open the selected note",
        description: "Enter does the same",
        default: "Ctrl+E",
    },
    ActionInfo {
        action: Action::ToggleDone,
        id: "toggle_done",
        group: "Your notes",
        label: "Mark done / bring back",
        description: "Done notes move to the Done list instead of being deleted",
        default: "Ctrl+D",
    },
    ActionInfo {
        action: Action::Delete,
        id: "delete",
        group: "Your notes",
        label: "Delete",
        description: "Removes the note; Undo brings it back",
        default: "Ctrl+Delete",
    },
    ActionInfo {
        action: Action::Undo,
        id: "undo",
        group: "Your notes",
        label: "Undo",
        description: "Reverts the last change to your notes",
        default: "Ctrl+Z",
    },
    ActionInfo {
        action: Action::AddReminder,
        id: "add_reminder",
        group: "Editing a note",
        label: "Add a reminder",
        description: "Reminders follow the start or due time, so set one of those first",
        default: "Ctrl+R",
    },
    ActionInfo {
        action: Action::RemoveReminder,
        id: "remove_reminder",
        group: "Editing a note",
        label: "Remove a reminder",
        description: "Removes the reminder you're on, or the last one",
        default: "Ctrl+Shift+R",
    },
    ActionInfo {
        action: Action::Inbox,
        id: "inbox",
        group: "Moving around",
        label: "Show notes",
        description: "Your open notes and to-dos",
        default: "Ctrl+1",
    },
    ActionInfo {
        action: Action::Archive,
        id: "archive",
        group: "Moving around",
        label: "Show done",
        description: "Everything you've marked done",
        default: "Ctrl+2",
    },
    ActionInfo {
        action: Action::Help,
        id: "help",
        group: "Moving around",
        label: "Keyboard shortcuts",
        description: "Lists every key in the Wisp window",
        default: "F1",
    },
    ActionInfo {
        action: Action::Settings,
        id: "settings",
        group: "Moving around",
        label: "Open settings",
        description: "This window",
        default: "Ctrl+,",
    },
    ActionInfo {
        action: Action::Quit,
        id: "quit",
        group: "Wisp",
        label: "Quit Wisp",
        description: "Closes Wisp completely; the hotkey stops working until it starts again",
        default: "Ctrl+Q",
    },
];

pub fn default_summon() -> Combo {
    DEFAULT_SUMMON.parse().expect("the default summon key parses")
}

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
    /// The format these settings were saved in; files from before versions existed read as 0.
    #[serde(default)]
    pub version: u32,
    /// The summon key as the user set it, or empty for the default.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub summon: String,
    /// Action id → combo. Missing entries use the default.
    pub shortcuts: BTreeMap<String, String>,
    pub launch_at_startup: bool,
    pub hide_when_inactive: bool,
    /// Show the window when a reminder rings, not only a notification.
    pub open_on_reminder: bool,
    pub theme: Theme,
    /// Set after the first start, when autostart is switched on.
    pub initialized: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            version: CONFIG_VERSION,
            summon: String::new(),
            shortcuts: BTreeMap::new(),
            launch_at_startup: true,
            hide_when_inactive: true,
            open_on_reminder: false,
            theme: Theme::System,
            initialized: false,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> (Config, Option<String>) {
        match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<Config>(&text) {
                Ok(mut cfg) => {
                    if cfg.upgrade() {
                        let _ = cfg.save(path);
                    }
                    (cfg, None)
                }
                Err(err) => (Config::default(), Some(format!("Settings file has an error ({err}); using defaults."))),
            },
            Err(_) => (Config::default(), None),
        }
    }

    /// Brings settings saved by an earlier Wisp up to date. Returns whether anything changed.
    fn upgrade(&mut self) -> bool {
        if self.version >= CONFIG_VERSION {
            return false;
        }
        // Version 0 always wrote the summon key out, so its old default can't be told
        // apart from a choice; treat it as "the default", which is now easier to press.
        if self.version == 0 && self.summon == FALLBACK_SUMMON {
            self.summon.clear();
        }
        self.version = CONFIG_VERSION;
        true
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

    /// The summon key the user asked for (the default unless they changed it).
    pub fn summon_combo(&self) -> Combo {
        self.summon.parse().unwrap_or_else(|_| default_summon())
    }

    /// Stores `combo` as the summon key; the default is stored as "not set", so a
    /// better default in a later version reaches everyone who never changed it.
    pub fn set_summon(&mut self, combo: &Combo) {
        self.summon = if *combo == default_summon() { String::new() } else { combo.to_string() };
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
        let fallback: Combo = FALLBACK_SUMMON.parse().unwrap();
        assert!(fallback.validate_global().is_ok());
        assert!(!seen.contains(&fallback));
    }

    #[test]
    fn old_default_summon_key_is_upgraded() {
        let dir = std::env::temp_dir().join(format!("wisp-config-{}", std::process::id()));
        let path = dir.join("settings.json");
        std::fs::create_dir_all(&dir).unwrap();

        // Written by an earlier Wisp: the old default, and no version.
        std::fs::write(&path, r#"{"summon": "Ctrl+Alt+Shift+N", "theme": "dark", "initialized": true}"#).unwrap();
        let (cfg, warning) = Config::load(&path);
        assert!(warning.is_none());
        assert_eq!(cfg.summon_combo(), default_summon());
        assert_eq!(cfg.theme, Theme::Dark);
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(!saved.contains("summon"), "the upgrade is saved: {saved}");

        // Chosen on purpose, after the upgrade: kept.
        let mut cfg = Config::default();
        cfg.set_summon(&FALLBACK_SUMMON.parse().unwrap());
        cfg.save(&path).unwrap();
        assert_eq!(Config::load(&path).0.summon, FALLBACK_SUMMON);

        // Someone else's choice from before the upgrade: kept.
        std::fs::write(&path, r#"{"summon": "Ctrl+Shift+Space"}"#).unwrap();
        assert_eq!(Config::load(&path).0.summon, "Ctrl+Shift+Space");
        let _ = std::fs::remove_dir_all(&dir);
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
