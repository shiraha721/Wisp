//! The settings window: pages in a sidebar, each setting a card.
//!
//! Keyboard: ↑/↓ move, Enter or Space changes, ←/→ flip switches and pick the
//! theme, Backspace restores a shortcut's default, Tab or Esc returns to the page
//! list, and Esc there closes the window. On a category, Enter renames it, ←/→
//! change its colour, Alt+↑/↓ move it and Delete (twice) removes it.

use super::{on_wayland, App};
use crate::config::{self, Action, Theme, ACTIONS};
use crate::model::CATEGORY_COLORS;
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
    /// A category, by id.
    Category(u64),
    /// The name of a category being added.
    NewCategory,
    AddCategory,
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

const GENERAL: usize = 0;
const CATEGORIES: usize = 1;
const HOTKEYS: usize = 2;
const APPEARANCE: usize = 3;

const PAGES: [Page; 5] = [
    Page { glyph: Glyph::Gear, title: "General", description: "How Wisp starts, and how its window behaves." },
    Page {
        glyph: Glyph::Tag,
        title: "Categories",
        description: "Lists like Daily, Work or School. In Wisp, type #name to file a note in one or to see only that one.",
    },
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
    /// The category whose name is being typed (or `NewCategory`).
    editing: Option<Entry>,
    /// The category that Delete was pressed on once; a second press removes it.
    confirm_remove: Option<u64>,
    error: Option<(Entry, String)>,
    entries: Vec<Entry>,
}

fn action_glyph(action: Action) -> Glyph {
    match action {
        Action::Schedule => Glyph::Calendar,
        Action::Open => Glyph::Open,
        Action::ToggleDone => Glyph::Check,
        Action::Category => Glyph::Tag,
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
        let current = self.settings.entries.get(self.settings.cursor).copied();
        self.reload_entries(current);
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
        self.settings_closed();
        let _ = self.settings_ui.hide();
    }

    /// The window was closed (with its title-bar button, or from the keyboard).
    pub fn settings_closed(&mut self) {
        self.stop_recording();
        if self.settings.editing.is_some() {
            self.finish_editing(false);
        }
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

    fn entries_for(&self, page: usize) -> Vec<Entry> {
        match page {
            GENERAL => vec![
                Entry::Heading("Startup"),
                Entry::Autostart,
                Entry::Heading("Window"),
                Entry::HideInactive,
                Entry::Heading("Reminders"),
                Entry::OpenOnReminder,
            ],
            CATEGORIES => {
                let mut entries: Vec<Entry> = self.store.categories().iter().map(|c| Entry::Category(c.id)).collect();
                if self.settings.editing == Some(Entry::NewCategory) {
                    entries.push(Entry::NewCategory);
                }
                entries.push(Entry::AddCategory);
                entries
            }
            HOTKEYS => {
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
            APPEARANCE => vec![Entry::Theme],
            _ => vec![Entry::Version, Entry::Folder],
        }
    }

    fn go_to_page(&mut self, page: usize) {
        if self.settings.editing.is_some() {
            // Keep a usable name; drop one that isn't.
            self.finish_editing(true);
            self.finish_editing(false);
        }
        self.settings.page = page.min(PAGES.len() - 1);
        self.settings.entries = self.entries_for(self.settings.page);
        self.settings.cursor = self.settings.entries.iter().position(|e| !matches!(e, Entry::Heading(_))).unwrap_or(0);
        self.settings.error = None;
        self.settings.confirm_remove = None;
    }

    /// Rebuilds the current page's entries (after categories change), keeping the
    /// cursor on `focus` if it's still there, or close to where it was.
    fn reload_entries(&mut self, focus: Option<Entry>) {
        self.settings.page = self.settings.page.min(PAGES.len() - 1);
        let entries = self.entries_for(self.settings.page);
        let s = &mut self.settings;
        s.entries = entries;
        let fallback = s.cursor.min(s.entries.len().saturating_sub(1));
        s.cursor = focus.and_then(|f| s.entries.iter().position(|e| *e == f)).unwrap_or(fallback);
        if matches!(s.entries.get(s.cursor), Some(Entry::Heading(_)) | None) {
            s.cursor = s.entries.iter().position(|e| !matches!(e, Entry::Heading(_))).unwrap_or(0);
        }
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
        w.set_editing(s.editing.is_some());
        w.set_cursor_on_shortcut(matches!(current, Some(Entry::Summon | Entry::Shortcut(_))));
        w.set_cursor_on_category(matches!(current, Some(Entry::Category(_))));
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
            Entry::Category(id) => {
                let Some(category) = self.store.category(id) else { return SettingCard::default() };
                let description = match self.store.category_counts(id) {
                    (0, 0) => "No notes yet".to_owned(),
                    (open, 0) => format!("{open} open"),
                    (0, done) => format!("{done} done"),
                    (open, done) => format!("{open} open, {done} done"),
                };
                SettingCard {
                    kind: 6,
                    title: category.name.as_str().into(),
                    description: description.into(),
                    tint: i32::from(category.color),
                    editing: self.settings.editing == Some(entry),
                    ..Default::default()
                }
            }
            Entry::NewCategory => {
                let used: Vec<u8> = self.store.categories().iter().map(|c| c.color).collect();
                let color = (0..CATEGORY_COLORS).find(|c| !used.contains(c)).unwrap_or(0);
                SettingCard {
                    kind: 6,
                    description: "Enter adds it, Esc cancels".into(),
                    tint: i32::from(color),
                    editing: true,
                    ..Default::default()
                }
            }
            Entry::AddCategory => SettingCard {
                kind: 3,
                glyph: Glyph::Plus,
                title: "Add a category".into(),
                description: format!("Then file notes with #name, or with {} on a note", self.label(Action::Category)).into(),
                value: "Add".into(),
                ..Default::default()
            },
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
        if self.settings.editing.is_some() {
            // Everything but these goes to the name box.
            match key {
                "Enter" | "Tab" if !combo.is_modified() => self.finish_editing(true),
                "Escape" if bare => self.finish_editing(false),
                _ => return false,
            }
            self.render_settings();
            return true;
        }
        // Delete on a category asks first; anything else forgets the question.
        let confirming = self.settings.confirm_remove.take();
        if confirming.is_some() {
            self.settings.error = None;
        }
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
            let only_alt = combo.alt && !combo.ctrl && !combo.meta && !combo.shift;
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
                "Up" | "Down" if only_alt => {
                    if let Some(Entry::Category(id)) = current {
                        self.move_category(id, if key == "Up" { -1 } else { 1 });
                    }
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
                "Enter" | "Space" | "F2" if bare => {
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
                        Some(Entry::Category(id)) => self.recolor_category(id, if forward { 1 } else { -1 }),
                        _ if !forward => self.settings.nav_active = true,
                        _ => {}
                    }
                    true
                }
                "Backspace" | "Delete" if bare => {
                    match current {
                        Some(Entry::Category(id)) => self.remove_category(id, confirming == Some(id), "Press Delete again"),
                        _ => self.reset_setting(),
                    }
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
            Some(Action::Help) => self.go_to_page(HOTKEYS),
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
        self.settings_ui.invoke_take_focus();
    }

    pub fn card_clicked(&mut self, index: i32) {
        let Ok(index) = usize::try_from(index) else { return };
        let clicked = self.settings.entries.get(index).copied();
        if matches!(clicked, None | Some(Entry::Heading(_) | Entry::NewCategory)) || clicked == self.settings.editing {
            return;
        }
        self.stop_recording();
        if self.settings.editing.is_some() {
            self.finish_editing(true);
            if self.settings.editing.is_some() {
                self.render_settings(); // the name needs fixing first
                return;
            }
        }
        self.settings.confirm_remove = None;
        self.settings.cursor = index;
        self.settings.nav_active = false;
        self.activate_setting(index);
        self.render_settings();
        // A name box takes the keyboard itself as it appears.
        if self.settings.editing.is_none() {
            self.settings_ui.invoke_take_focus();
        }
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

    /// A category's colour dot was clicked.
    pub fn swatch_clicked(&mut self, index: i32) {
        if let Some(&Entry::Category(id)) = usize::try_from(index).ok().and_then(|i| self.settings.entries.get(i)) {
            self.settings.cursor = index as usize;
            self.settings.nav_active = false;
            self.recolor_category(id, 1);
            self.render_settings();
        }
    }

    /// A category's bin button was clicked.
    pub fn remove_clicked(&mut self, index: i32) {
        if let Some(&Entry::Category(id)) = usize::try_from(index).ok().and_then(|i| self.settings.entries.get(i)) {
            let confirmed = self.settings.confirm_remove.take() == Some(id);
            self.settings.cursor = index as usize;
            self.settings.nav_active = false;
            self.remove_category(id, confirmed, "Click the bin again");
            self.render_settings();
            self.settings_ui.invoke_take_focus();
        }
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
            Some(entry @ Entry::Category(id)) => {
                let name = self.store.category(id).map(|c| c.name.clone()).unwrap_or_default();
                self.settings_ui.set_edit_text(name.into());
                self.settings.editing = Some(entry);
            }
            Some(Entry::AddCategory) => {
                self.settings_ui.set_edit_text(SharedString::new());
                self.settings.editing = Some(Entry::NewCategory);
                self.reload_entries(Some(Entry::NewCategory));
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
            Some(Entry::Version | Entry::Heading(_) | Entry::NewCategory) | None => {}
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

    // -----------------------------------------------------------------------
    // Categories

    /// Saves (or with `save` false, drops) the name being typed. A name that can't
    /// be used keeps the box open with the reason.
    fn finish_editing(&mut self, save: bool) {
        let Some(entry) = self.settings.editing else { return };
        let name = self.settings_ui.get_edit_text().to_string();
        let result = match entry {
            _ if !save => Ok(None),
            Entry::NewCategory => self.store.add_category(&name).map(Some),
            Entry::Category(id) => self.store.rename_category(id, &name).map(|()| Some(id)),
            _ => Ok(None),
        };
        match result {
            Err(problem) => self.settings.error = Some((entry, problem)),
            Ok(saved) => {
                self.settings.editing = None;
                self.settings.error = None;
                if saved.is_some() {
                    self.persist();
                    self.categories_changed();
                }
                let focus = match (saved, entry) {
                    (Some(id), _) => Entry::Category(id),
                    (None, Entry::NewCategory) => Entry::AddCategory,
                    (None, other) => other,
                };
                self.reload_entries(Some(focus));
                self.settings_ui.invoke_take_focus();
            }
        }
    }

    fn recolor_category(&mut self, id: u64, delta: i32) {
        self.store.recolor_category(id, delta);
        self.persist();
        self.categories_changed();
    }

    fn move_category(&mut self, id: u64, delta: i32) {
        if self.store.move_category(id, delta) {
            self.persist();
            self.categories_changed();
            self.reload_entries(Some(Entry::Category(id)));
        }
    }

    /// Removes a category once `confirmed`; until then, explains what will happen.
    fn remove_category(&mut self, id: u64, confirmed: bool, again: &str) {
        let Some(name) = self.store.category(id).map(|c| c.name.clone()) else { return };
        if !confirmed {
            let (open, done) = self.store.category_counts(id);
            let message = if open + done == 0 {
                format!("{again} to remove it.")
            } else {
                format!("{again} to remove it. Its notes stay, without a category.")
            };
            self.settings.confirm_remove = Some(id);
            self.settings.error = Some((Entry::Category(id), message));
            return;
        }
        self.store.remove_category(id);
        self.persist();
        self.categories_changed();
        self.settings.error = None;
        self.reload_entries(None);
        self.settings_toast(format!("Removed “{name}”."), false);
    }

    /// Brings the launcher up to date after categories were added, renamed or removed.
    fn categories_changed(&mut self) {
        self.push_categories();
        self.refresh();
    }

    // -----------------------------------------------------------------------
    // Shortcuts

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
