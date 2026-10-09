//! The settings window: pages in a sidebar, each setting a card.
//!
//! Keyboard: ↑/↓ move, Enter or Space changes, ←/→ flip switches and pick the
//! theme, Backspace restores a shortcut's default, Tab or Esc returns to the page
//! list, and Esc there closes the window.

use super::{on_wayland, App};
use crate::config::{self, Action, Theme, ACTIONS};
use crate::platform;
use crate::shortcuts::Combo;
use crate::{Glyph, NavItem, SettingCard};
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Entry {
    Heading(&'static str),
    Autostart,
    HideInactive,
    OpenOnReminder,
    Summon,
    Shortcut(usize),
    ResetShortcuts,
    Theme,
    Version,
    Folder,
}

struct Page {
    glyph: Glyph,
    title: &'static str,
    description: &'static str,
}

const PAGES: [Page; 4] = [
    Page { glyph: Glyph::Gear, title: "General", description: "How Wisp starts, and how its window behaves." },
    Page {
        glyph: Glyph::Keyboard,
        title: "Hotkeys",
        description: "Every key Wisp listens to. Select one, press Enter, then press the new combination.",
    },
    Page { glyph: Glyph::Moon, title: "Appearance", description: "Light, dark, or whatever your system uses." },
    Page { glyph: Glyph::Info, title: "About", description: "The version you're running and where your notes are kept." },
];

#[derive(Default)]
pub(super) struct SettingsState {
    page: usize,
    cursor: usize,
    nav_active: bool,
    recording: bool,
    error: Option<(Entry, String)>,
    entries: Vec<Entry>,
}

fn entries_for(page: usize) -> Vec<Entry> {
    match page {
        0 => vec![
            Entry::Heading("Startup"),
            Entry::Autostart,
            Entry::Heading("Window"),
            Entry::HideInactive,
            Entry::Heading("Reminders"),
            Entry::OpenOnReminder,
        ],
        1 => {
            let mut entries = vec![Entry::Heading("Anywhere"), Entry::Summon];
            let mut group = "";
            for (i, info) in ACTIONS.iter().enumerate() {
                if info.group != group {
                    group = info.group;
                    entries.push(Entry::Heading(group));
                }
                entries.push(Entry::Shortcut(i));
            }
            entries.extend([Entry::Heading("Defaults"), Entry::ResetShortcuts]);
            entries
        }
        2 => vec![Entry::Theme],
        _ => vec![Entry::Version, Entry::Folder],
    }
}

fn action_glyph(action: Action) -> Glyph {
    match action {
        Action::Schedule => Glyph::Calendar,
        Action::Open => Glyph::Open,
        Action::ToggleDone => Glyph::Check,
        Action::Delete => Glyph::Trash,
        Action::Undo => Glyph::Undo,
        Action::Inbox => Glyph::List,
        Action::Archive => Glyph::Archive,
        Action::Settings => Glyph::Gear,
        Action::Help => Glyph::Help,
        Action::AddReminder => Glyph::Bell,
        Action::RemoveReminder => Glyph::Cross,
        Action::Quit => Glyph::Quit,
    }
}

fn autostart_title() -> &'static str {
    if cfg!(windows) {
        "Start with Windows"
    } else if cfg!(target_os = "macos") {
        "Open at login"
    } else {
        "Start when you log in"
    }
}

impl App {
    pub fn open_settings(&mut self) {
        // The launcher steps aside, as Flow Launcher's does.
        if self.ui().window().is_visible() {
            self.dismiss();
        }
        self.settings.error = None;
        if self.settings.entries.is_empty() {
            self.go_to_page(0);
        }
        self.settings.nav_active = true;
        self.render_settings();
        let window = self.settings_ui.clone_strong();
        if let Err(err) = window.show() {
            eprintln!("wisp: couldn't show settings: {err}");
            return;
        }
        window.window().with_winit_window(|w| w.focus_window());
        window.invoke_take_focus();
        slint::Timer::single_shot(Duration::from_millis(90), move || {
            window.window().with_winit_window(|w| {
                if !w.has_focus() {
                    w.focus_window();
                }
            });
            window.invoke_take_focus();
        });
    }

    fn close_settings(&mut self) {
        self.stop_recording();
        let _ = self.settings_ui.hide();
    }

    /// The window was closed with its title-bar button.
    pub fn settings_closed(&mut self) {
        self.stop_recording();
    }

    pub(super) fn settings_toast(&mut self, message: impl Into<SharedString>, error: bool) {
        let window = &self.settings_ui;
        window.set_toast(message.into());
        window.set_toast_error(error);
        let weak = window.as_weak();
        self.settings_toast_timer.start(slint::TimerMode::SingleShot, Duration::from_secs(if error { 7 } else { 4 }), move || {
            if let Some(w) = weak.upgrade() {
                w.set_toast(SharedString::new());
            }
        });
    }

    fn go_to_page(&mut self, page: usize) {
        let s = &mut self.settings;
        s.page = page.min(PAGES.len() - 1);
        s.entries = entries_for(s.page);
        s.cursor = s.entries.iter().position(|e| !matches!(e, Entry::Heading(_))).unwrap_or(0);
        s.error = None;
    }

    fn render_settings(&mut self) {
        if self.settings.entries.is_empty() {
            self.go_to_page(self.settings.page);
        }
        let mut cards = Vec::new();
        let mut top = 0.0f32;
        for (i, &entry) in self.settings.entries.iter().enumerate() {
            let height = match entry {
                Entry::Heading(_) if i == 0 => 34.0,
                Entry::Heading(_) => 46.0,
                Entry::Theme => 236.0,
                _ => 70.0,
            };
            let mut card = self.card_for(entry);
            card.top = top;
            card.height = height;
            if let Some((e, message)) = &self.settings.error {
                if *e == entry {
                    card.error = message.as_str().into();
                }
            }
            cards.push(card);
            top += height;
        }
        let s = &self.settings;
        let current = s.entries.get(s.cursor).copied();
        let w = &self.settings_ui;
        w.set_pages(ModelRc::new(VecModel::from(
            PAGES.iter().map(|p| NavItem { glyph: p.glyph, label: p.title.into() }).collect::<Vec<_>>(),
        )));
        w.set_page(s.page as i32);
        w.set_page_title(PAGES[s.page].title.into());
        w.set_page_description(PAGES[s.page].description.into());
        w.set_cards(ModelRc::new(VecModel::from(cards)));
        w.set_cards_height(top);
        w.set_cursor(s.cursor as i32);
        w.set_nav_active(s.nav_active);
        w.set_recording(s.recording);
        w.set_cursor_on_shortcut(matches!(current, Some(Entry::Summon | Entry::Shortcut(_))));
        w.set_theme_choice(match self.config.theme {
            Theme::System => 0,
            Theme::Light => 1,
            Theme::Dark => 2,
        });
        w.set_version(format!("Version {}", env!("CARGO_PKG_VERSION")).into());
    }

    fn card_for(&self, entry: Entry) -> SettingCard {
        let keys = |combo: Combo| ModelRc::new(VecModel::from(combo.parts().into_iter().map(SharedString::from).collect::<Vec<_>>()));
        let switch = |glyph, title: &str, description: &str, on| SettingCard {
            kind: 1,
            glyph,
            title: title.into(),
            description: description.into(),
            on,
            ..Default::default()
        };
        match entry {
            Entry::Heading(title) => SettingCard { kind: 5, title: title.into(), ..Default::default() },
            Entry::Autostart => switch(Glyph::Power, autostart_title(), "Wisp waits in the tray, ready for your hotkey", self.config.launch_at_startup),
            Entry::HideInactive => switch(
                Glyph::Window,
                "Hide when I click elsewhere",
                "Like a launcher: the window gets out of the way when you switch apps",
                self.config.hide_when_inactive,
            ),
            Entry::OpenOnReminder => switch(
                Glyph::Bell,
                "Open Wisp when a reminder rings",
                "Shows the window as well as the system notification",
                self.config.open_on_reminder,
            ),
            Entry::Summon => {
                let description = match &self.summon_problem {
                    Some(problem) => problem.clone(),
                    None if on_wayland() => "On Wayland, also bind “wisp --toggle” to a key in your system settings".into(),
                    None => "Works in every app. Press it again to hide Wisp.".into(),
                };
                SettingCard {
                    kind: 0,
                    glyph: Glyph::Bolt,
                    title: "Open Wisp from anywhere".into(),
                    description: description.into(),
                    keys: keys(self.summon_keys()),
                    ..Default::default()
                }
            }
            Entry::Shortcut(i) => {
                let info = &ACTIONS[i];
                let combo = self.config.combo(info.action);
                let default: Combo = info.default.parse().expect("defaults parse");
                let description =
                    if combo == default { info.description.to_owned() } else { format!("{} (default: {})", info.description, default.label()) };
                SettingCard {
                    kind: 0,
                    glyph: action_glyph(info.action),
                    title: info.label.into(),
                    description: description.into(),
                    keys: keys(combo),
                    ..Default::default()
                }
            }
            Entry::ResetShortcuts => SettingCard {
                kind: 3,
                glyph: Glyph::Undo,
                title: "Reset all shortcuts".into(),
                description: "Puts every key, including the one that opens Wisp, back to its default".into(),
                value: "Reset".into(),
                ..Default::default()
            },
            Entry::Theme => SettingCard {
                kind: 2,
                glyph: Glyph::Moon,
                title: "Theme".into(),
                description: "Applies to the launcher and this window".into(),
                ..Default::default()
            },
            Entry::Version => SettingCard {
                kind: 4,
                glyph: Glyph::Info,
                title: "Wisp".into(),
                description: "A keyboard-first catcher for ideas, memos and to-dos".into(),
                value: format!("Version {}", env!("CARGO_PKG_VERSION")).into(),
                ..Default::default()
            },
            Entry::Folder => SettingCard {
                kind: 3,
                glyph: Glyph::Folder,
                title: if self.portable { "Notes folder (next to Wisp)" } else { "Notes folder" }.into(),
                description: self.data_dir.display().to_string().into(),
                value: "Open folder".into(),
                ..Default::default()
            },
        }
    }

    // -----------------------------------------------------------------------
    // Keys and clicks

    pub fn settings_key(&mut self, text: &str, ctrl: bool, alt: bool, shift: bool, meta: bool) -> bool {
        let Some(combo) = Combo::from_event(text, ctrl, alt, shift, meta) else {
            return self.settings.recording;
        };
        if self.settings.recording {
            self.record(combo);
            return true;
        }
        let bare = !combo.is_modified() && !combo.shift;
        let key = combo.key.as_str();
        let handled = if self.settings.nav_active {
            match key {
                "Up" | "Down" if bare => {
                    let page = self.settings.page as i32 + if key == "Up" { -1 } else { 1 };
                    self.go_to_page(page.clamp(0, PAGES.len() as i32 - 1) as usize);
                    true
                }
                "Home" | "End" if bare => {
                    self.go_to_page(if key == "Home" { 0 } else { PAGES.len() - 1 });
                    true
                }
                "Enter" | "Right" | "Space" if bare => {
                    self.settings.nav_active = false;
                    true
                }
                "Tab" if !combo.is_modified() => {
                    self.settings.nav_active = combo.shift;
                    true
                }
                "Escape" if bare => {
                    self.close_settings();
                    return true;
                }
                _ => false,
            }
        } else {
            let current = self.settings.entries.get(self.settings.cursor).copied();
            match key {
                "Escape" if bare => {
                    self.settings.nav_active = true;
                    self.settings.error = None;
                    true
                }
                "Tab" if !combo.is_modified() => {
                    self.settings.nav_active = true;
                    true
                }
                "Up" | "Down" if bare => {
                    self.move_cursor(if key == "Up" { -1 } else { 1 });
                    true
                }
                "PageUp" | "PageDown" if bare => {
                    self.move_cursor(if key == "PageUp" { -5 } else { 5 });
                    true
                }
                "Home" | "End" if bare => {
                    self.move_cursor(if key == "Home" { -1000 } else { 1000 });
                    true
                }
                "Enter" | "Space" if bare => {
                    self.activate_setting(self.settings.cursor);
                    true
                }
                "Left" | "Right" if bare => {
                    let forward = key == "Right";
                    match current {
                        Some(Entry::Theme) => self.cycle_theme(if forward { 1 } else { -1 }),
                        Some(Entry::Autostart) => self.set_autostart(forward),
                        Some(Entry::HideInactive) => {
                            self.config.hide_when_inactive = forward;
                            self.save_config();
                        }
                        Some(Entry::OpenOnReminder) => {
                            self.config.open_on_reminder = forward;
                            self.save_config();
                        }
                        _ if !forward => self.settings.nav_active = true,
                        _ => {}
                    }
                    true
                }
                "Backspace" | "Delete" if bare => {
                    self.reset_setting();
                    true
                }
                _ => false,
            }
        };
        if handled {
            self.render_settings();
            return true;
        }
        match self.config.action_for(&combo) {
            Some(Action::Settings) => self.close_settings(),
            Some(Action::Quit) => self.quit(),
            Some(Action::Help) => self.go_to_page(1),
            _ => return false,
        }
        self.render_settings();
        true
    }

    fn move_cursor(&mut self, delta: i32) {
        let s = &mut self.settings;
        let focusable: Vec<usize> =
            s.entries.iter().enumerate().filter(|(_, e)| !matches!(e, Entry::Heading(_))).map(|(i, _)| i).collect();
        let Some(here) = focusable.iter().position(|&i| i == s.cursor) else { return };
        let next = (here as i32 + delta).clamp(0, focusable.len() as i32 - 1) as usize;
        s.cursor = focusable[next];
        s.error = None;
    }

    pub fn page_clicked(&mut self, page: i32) {
        self.stop_recording();
        self.go_to_page(usize::try_from(page).unwrap_or(0));
        self.settings.nav_active = true;
        self.render_settings();
    }

    pub fn card_clicked(&mut self, index: i32) {
        let Ok(index) = usize::try_from(index) else { return };
        if matches!(self.settings.entries.get(index), None | Some(Entry::Heading(_))) {
            return;
        }
        self.stop_recording();
        self.settings.cursor = index;
        self.settings.nav_active = false;
        self.activate_setting(index);
        self.render_settings();
        self.settings_ui.invoke_take_focus();
    }

    pub fn theme_clicked(&mut self, mode: i32) {
        self.config.theme = match mode {
            1 => Theme::Light,
            2 => Theme::Dark,
            _ => Theme::System,
        };
        self.save_config();
        self.apply_config_to_ui();
        if let Some(i) = self.settings.entries.iter().position(|e| *e == Entry::Theme) {
            self.settings.cursor = i;
        }
        self.settings.nav_active = false;
        self.render_settings();
    }

    fn activate_setting(&mut self, index: usize) {
        self.settings.error = None;
        match self.settings.entries.get(index).copied() {
            Some(Entry::Summon) => {
                // Release the current key so pressing it again can be recorded.
                if let (Some(manager), Some((hk, _))) = (&self.hotkeys, &self.summon) {
                    let _ = manager.unregister(*hk);
                }
                self.settings.recording = true;
            }
            Some(Entry::Shortcut(_)) => self.settings.recording = true,
            Some(Entry::Autostart) => self.set_autostart(!self.config.launch_at_startup),
            Some(Entry::HideInactive) => {
                self.config.hide_when_inactive = !self.config.hide_when_inactive;
                self.save_config();
            }
            Some(Entry::OpenOnReminder) => {
                self.config.open_on_reminder = !self.config.open_on_reminder;
                self.save_config();
            }
            Some(Entry::Theme) => self.cycle_theme(1),
            Some(Entry::ResetShortcuts) => {
                self.config.shortcuts.clear();
                self.config.summon.clear();
                self.claim_summon();
                match self.summon_problem.clone() {
                    Some(problem) => self.settings.error = Some((Entry::ResetShortcuts, problem)),
                    None => self.settings_toast("Every shortcut is back to its default.", false),
                }
                self.save_config();
                self.apply_config_to_ui();
            }
            Some(Entry::Folder) => platform::reveal(&self.data_dir),
            Some(Entry::Version | Entry::Heading(_)) | None => {}
        }
    }

    fn reset_setting(&mut self) {
        match self.settings.entries.get(self.settings.cursor).copied() {
            Some(entry @ Entry::Shortcut(i)) => {
                let info = &ACTIONS[i];
                let combo: Combo = info.default.parse().expect("defaults parse");
                if let Some(other) = self.config.conflict(&combo, info.id) {
                    self.settings.error = Some((entry, format!("{} is taken by “{other}”", combo.label())));
                } else {
                    self.config.set_shortcut(info.id, &combo);
                    self.save_config();
                    self.apply_config_to_ui();
                    self.settings_toast(format!("{} is back to {}.", info.label, combo.label()), false);
                }
            }
            Some(Entry::Summon) => self.apply_summon(config::default_summon()),
            _ => {}
        }
    }

    fn stop_recording(&mut self) {
        if !self.settings.recording {
            return;
        }
        self.settings.recording = false;
        // Re-claim the summon key if recording had released it.
        if let (Some(manager), Some((hk, _))) = (&self.hotkeys, &self.summon) {
            let _ = manager.register(*hk);
        }
        self.render_settings();
    }

    /// Handles the keys pressed while a shortcut card is listening.
    fn record(&mut self, combo: Combo) {
        let Some(entry) = self.settings.entries.get(self.settings.cursor).copied() else { return };
        if combo.key == "Escape" && !combo.is_modified() && !combo.shift {
            self.settings.error = None;
            self.stop_recording();
            return;
        }
        let id = match entry {
            Entry::Summon => "summon",
            Entry::Shortcut(i) => ACTIONS[i].id,
            _ => return,
        };
        let valid = if entry == Entry::Summon { combo.validate_global() } else { combo.validate_local() };
        let problem = valid
            .err()
            .or_else(|| self.config.conflict(&combo, id).map(|other| format!("{} is taken by “{other}”", combo.label())));
        if let Some(problem) = problem {
            self.settings.error = Some((entry, problem));
            self.render_settings(); // keep listening
            return;
        }
        self.settings.recording = false;
        self.settings.error = None;
        match entry {
            Entry::Summon => self.apply_summon(combo),
            _ => {
                self.config.set_shortcut(id, &combo);
                self.save_config();
                self.apply_config_to_ui();
                self.settings_toast(format!("Saved: {}", combo.label()), false);
            }
        }
        self.render_settings();
    }

    fn apply_summon(&mut self, combo: Combo) {
        let previous = self.summon.clone();
        match self.register_summon(&combo) {
            Ok(()) => {
                self.config.set_summon(&combo);
                self.summon_problem = None;
                self.save_config();
                self.apply_config_to_ui();
                self.settings_toast(format!("{} now opens Wisp from anywhere.", combo.label()), false);
            }
            Err(err) => {
                self.settings.error = Some((Entry::Summon, err));
                if let (Some(manager), Some((hk, keys))) = (&self.hotkeys, previous) {
                    if manager.register(hk).is_ok() {
                        self.summon = Some((hk, keys));
                    }
                }
            }
        }
    }

    fn set_autostart(&mut self, enabled: bool) {
        match platform::set_autostart(enabled) {
            Ok(()) => {
                self.config.launch_at_startup = enabled;
                self.save_config();
            }
            Err(err) => self.settings.error = Some((Entry::Autostart, format!("Couldn't change it: {err}"))),
        }
    }

    fn cycle_theme(&mut self, delta: i32) {
        let order = [Theme::System, Theme::Light, Theme::Dark];
        let index = order.iter().position(|&t| t == self.config.theme).unwrap_or(0) as i32;
        self.config.theme = order[(index + delta).rem_euclid(3) as usize];
        self.save_config();
        self.apply_config_to_ui();
    }
}
