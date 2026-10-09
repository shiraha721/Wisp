//! The controller: owns the notes and settings, reacts to keys and clicks, and
//! keeps the windows' properties in step with the data.
//!
//! The note editor lives in `app/editor.rs`, the settings window in `app/settings.rs`.

mod editor;
mod settings;

use crate::config::{self, Action, Config, Theme};
use crate::dates;
use crate::model::{self, Item, Store};
use crate::platform;
use crate::shortcuts::Combo;
use crate::{AppWindow, HelpRow, ItemRow, Keys, ReminderRow, SettingsWindow, View};
use chrono::{DateTime, Local, TimeZone};
use global_hotkey::hotkey::HotKey;
use global_hotkey::GlobalHotKeyManager;
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// Reminders missed by more than this (the computer was off, say) stay quiet.
const REMINDER_GRACE_SECS: i64 = 6 * 3600;
/// Row geometry, matching `Theme.row-h` and `Theme.section-h` in the UI.
const ROW_H: f32 = 58.0;
const SECTION_H: f32 = 30.0;

/// `WISP_DEBUG=1` prints window and focus events to stderr.
macro_rules! debug_log {
    ($($arg:tt)*) => {
        if std::env::var_os("WISP_DEBUG").is_some() {
            eprintln!("wisp: {}", format_args!($($arg)*));
        }
    };
}
use debug_log;

/// The id of the registered summon hotkey, read from global-hotkey's thread.
pub static SUMMON_ID: AtomicU32 = AtomicU32::new(0);

thread_local! {
    static APP: RefCell<Option<Rc<RefCell<App>>>> = const { RefCell::new(None) };
}

pub fn install(app: App) {
    APP.with(|slot| *slot.borrow_mut() = Some(Rc::new(RefCell::new(app))));
}

/// Runs `f` on the app (on the UI thread). Nested calls are dropped rather than panicking.
pub fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    let app = APP.with(|slot| slot.borrow().clone())?;
    let mut guard = app.try_borrow_mut().ok()?;
    Some(f(&mut guard))
}

/// Runs `f` once the current event has been handled.
pub fn later(f: impl FnOnce(&mut App) + 'static) {
    slint::Timer::single_shot(Duration::ZERO, move || {
        with_app(f);
    });
}

enum UndoStep {
    Created(u64),
    Removed { pos: usize, item: Item },
    Changed(Item),
}

pub struct App {
    ui: slint::Weak<AppWindow>,
    settings_ui: SettingsWindow,
    store: Store,
    config: Config,
    config_path: PathBuf,
    data_dir: PathBuf,
    /// The data folder sits next to the program.
    portable: bool,
    rows: Vec<u64>,
    selected_id: Option<u64>,
    listing: View,
    undo: Vec<UndoStep>,
    last_change: Option<Instant>,
    last_typed: Option<Instant>,
    editor: Option<editor::EditSession>,
    reminders: Rc<VecModel<ReminderRow>>,
    settings: settings::SettingsState,
    hotkeys: Option<GlobalHotKeyManager>,
    /// The registered summon hotkey and the keys it stands for.
    summon: Option<(HotKey, Combo)>,
    /// Why the summon key isn't the one in the settings, or doesn't work at all.
    summon_problem: Option<String>,
    toast_timer: slint::Timer,
    settings_toast_timer: slint::Timer,
    shown_at: Instant,
    /// The window got focus since it was last shown; only then may losing focus hide it.
    had_focus: bool,
    /// The window has been centred on a screen (needs the native window to exist).
    placed: bool,
    last_minute: i64,
    tray: Option<crate::Tray>,
}

impl App {
    pub fn new(ui: &AppWindow, settings_ui: SettingsWindow, data_dir: PathBuf, portable: bool) -> App {
        let (store, store_warning) = Store::load(&data_dir.join("notes.json"));
        let config_path = data_dir.join("settings.json");
        let (mut config, config_warning) = Config::load(&config_path);

        if !config.initialized {
            // Turn on "start with the computer" once, for release builds.
            config.launch_at_startup = !cfg!(debug_assertions);
            config.initialized = true;
            let _ = config.save(&config_path);
        }
        if config.launch_at_startup {
            if let Err(err) = platform::set_autostart(true) {
                eprintln!("wisp: couldn't register autostart: {err}");
            }
        }

        let reminders = Rc::new(VecModel::default());
        ui.set_reminders(ModelRc::from(reminders.clone()));

        let mut app = App {
            ui: ui.as_weak(),
            settings_ui,
            store,
            config,
            config_path,
            data_dir,
            portable,
            rows: Vec::new(),
            selected_id: None,
            listing: View::Inbox,
            undo: Vec::new(),
            last_change: None,
            last_typed: None,
            editor: None,
            reminders,
            settings: settings::SettingsState::default(),
            hotkeys: None,
            summon: None,
            summon_problem: None,
            toast_timer: slint::Timer::default(),
            settings_toast_timer: slint::Timer::default(),
            shown_at: Instant::now(),
            had_focus: false,
            placed: false,
            last_minute: 0,
            tray: None,
        };

        match GlobalHotKeyManager::new() {
            Ok(manager) => app.hotkeys = Some(manager),
            Err(err) => app.summon_problem = Some(format!("System-wide keys aren't available here ({err}).")),
        }
        app.claim_summon();

        app.apply_config_to_ui();
        app.refresh();
        if let Some(warning) = store_warning.or(config_warning) {
            app.toast(warning, true);
        } else if let Some(problem) = app.summon_problem.clone() {
            let settings = app.label(Action::Settings);
            let working = app.summon.is_some();
            let message = if working {
                format!("{problem} Change it in Settings ({settings}).")
            } else {
                format!("{problem} Pick another key in Settings ({settings}).")
            };
            app.toast_for(message, !working, Duration::from_secs(20));
        }
        app
    }

    /// Tells the user where their notes went when they were copied to the portable folder.
    pub fn announce_move(&mut self, from: &str, to: &str) {
        debug_log!("copied notes from {from} to {to}");
        self.toast_for(format!("Your notes now live next to Wisp, in {to}"), false, Duration::from_secs(12));
    }

    pub fn set_tray(&mut self, tray: Option<crate::Tray>) {
        if let Some(t) = &tray {
            t.set_summon_label(self.summon_label().into());
        }
        self.tray = tray;
    }

    fn ui(&self) -> AppWindow {
        self.ui.upgrade().expect("the window outlives the app")
    }

    fn now() -> DateTime<Local> {
        Local::now()
    }

    fn persist(&mut self) {
        if let Err(err) = self.store.save() {
            self.toast(format!("Couldn't save notes: {err}"), true);
        }
    }

    fn save_config(&mut self) {
        if let Err(err) = self.config.save(&self.config_path) {
            self.settings_toast(format!("Couldn't save settings: {err}"), true);
        }
    }

    fn toast(&mut self, message: impl Into<SharedString>, error: bool) {
        self.toast_for(message, error, Duration::from_millis(if error { 6000 } else { 3500 }));
    }

    fn toast_for(&mut self, message: impl Into<SharedString>, error: bool, duration: Duration) {
        let ui = self.ui();
        ui.set_toast(message.into());
        ui.set_toast_error(error);
        let weak = self.ui.clone();
        self.toast_timer.start(slint::TimerMode::SingleShot, duration, move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_toast(SharedString::new());
            }
        });
    }

    fn label(&self, action: Action) -> String {
        self.config.combo(action).label()
    }

    /// The keys that open Wisp right now: the registered ones, else the configured ones.
    fn summon_keys(&self) -> Combo {
        self.summon.as_ref().map_or_else(|| self.config.summon_combo(), |(_, combo)| combo.clone())
    }

    fn summon_label(&self) -> String {
        self.summon_keys().label()
    }

    // -----------------------------------------------------------------------
    // Showing and hiding

    pub fn is_active(&self) -> bool {
        let ui = self.ui();
        ui.window().is_visible() && ui.window().with_winit_window(|w| w.has_focus()).unwrap_or(false)
    }

    pub fn toggle(&mut self) {
        if self.is_active() {
            self.dismiss();
        } else {
            self.summon();
        }
    }

    /// Brings the window up, centred near the top of the screen, ready to type.
    pub fn summon(&mut self) {
        let ui = self.ui();
        if ui.get_view() == View::Help {
            self.show_list(self.listing);
        }
        if let Err(err) = ui.window().show() {
            eprintln!("wisp: couldn't show the window: {err}");
            return;
        }
        self.placed = place_and_focus(&ui);
        debug_log!("summon");
        self.shown_at = Instant::now();
        self.had_focus = false;
        self.focus_current();
        self.refresh();
        // Some window managers only honour focus requests once the window is mapped.
        slint::Timer::single_shot(Duration::from_millis(90), || {
            with_app(|app| {
                let ui = app.ui();
                if ui.window().is_visible() {
                    if !app.placed {
                        app.placed = place_and_focus(&ui);
                    } else {
                        ui.window().with_winit_window(|w| {
                            if !w.has_focus() {
                                w.focus_window();
                            }
                        });
                    }
                    app.focus_current();
                }
            });
        });
    }

    fn focus_current(&self) {
        let ui = self.ui();
        match ui.get_view() {
            View::Inbox | View::Archive => ui.invoke_focus_capture(),
            View::Editor => ui.invoke_focus_editor(editor::FIELD_TITLE),
            View::Help => ui.invoke_focus_help(),
        }
    }

    /// Hides the window, saving an open note first. The draft in the input stays.
    pub fn dismiss(&mut self) {
        debug_log!("dismiss");
        let ui = self.ui();
        match ui.get_view() {
            View::Editor => {
                if let Err(err) = self.save_editor(false) {
                    // Keep the editor so nothing typed is lost; the message waits for the next visit.
                    self.toast(err, true);
                } else {
                    self.show_list(self.listing);
                }
            }
            View::Help => self.show_list(self.listing),
            _ => {}
        }
        self.select(None);
        let _ = ui.window().hide();
    }

    pub fn focus_gained(&mut self) {
        debug_log!("focus gained");
        self.had_focus = true;
    }

    /// Called a moment after the window reports losing focus.
    pub fn focus_lost(&mut self) {
        let ui = self.ui();
        debug_log!("focus lost (had focus: {})", self.had_focus);
        if !self.config.hide_when_inactive || !ui.window().is_visible() || !self.had_focus {
            return;
        }
        if self.shown_at.elapsed() < Duration::from_millis(500) {
            return; // focus can bounce while the window is being mapped
        }
        if ui.window().with_winit_window(|w| w.has_focus()).unwrap_or(true) {
            return;
        }
        self.dismiss();
    }

    pub fn remote(&mut self, command: &str) {
        match command {
            "show" => self.summon(),
            "toggle" => self.toggle(),
            "settings" => self.open_settings(),
            "quit" => self.quit(),
            _ => {}
        }
    }

    pub fn quit(&mut self) {
        if self.editor.is_some() {
            let _ = self.save_editor(true);
        }
        self.persist();
        let _ = self.ui().window().hide();
        let _ = self.settings_ui.hide();
        if let Some(tray) = &self.tray {
            let _ = tray.hide();
        }
        let _ = slint::quit_event_loop();
    }

    // -----------------------------------------------------------------------
    // Keys

    pub fn on_key(&mut self, text: &str, ctrl: bool, alt: bool, shift: bool, meta: bool, context: &str) -> bool {
        let Some(combo) = Combo::from_event(text, ctrl, alt, shift, meta) else {
            return false; // lone modifier presses, input-method text
        };
        let action = self.config.action_for(&combo);
        match self.ui().get_view() {
            View::Inbox | View::Archive => self.key_list(&combo, action),
            View::Editor => self.key_editor(&combo, action, context),
            View::Help => self.key_help(&combo, action),
        }
    }

    fn key_list(&mut self, combo: &Combo, action: Option<Action>) -> bool {
        let ui = self.ui();
        let selected = ui.get_selected();
        let query = ui.get_query();
        let bare = !combo.is_modified() && !combo.shift;
        let can_move = selected >= 0 || !query.contains('\n');
        match combo.key.as_str() {
            "Escape" if bare => {
                if selected >= 0 {
                    self.select(None);
                } else {
                    self.dismiss();
                }
                return true;
            }
            "Enter" if bare => {
                if selected >= 0 {
                    self.open_selected(editor::FIELD_TITLE);
                } else if ui.get_view() == View::Inbox {
                    self.capture(false);
                }
                return true;
            }
            "Down" if bare && can_move => return self.move_selection(1),
            "Up" if bare && can_move => return self.move_selection(-1),
            "Tab" if !combo.is_modified() => return self.cycle_selection(if combo.shift { -1 } else { 1 }),
            "PageDown" if bare => return self.move_selection(6),
            "PageUp" if bare => return self.move_selection(-6),
            _ => {}
        }
        let Some(action) = action else { return false };
        match action {
            Action::Schedule => {
                if ui.get_view() == View::Inbox && !query.trim().is_empty() {
                    self.capture(true);
                } else if selected >= 0 {
                    self.open_selected(editor::FIELD_START);
                } else {
                    self.toast("Type a note first, then add its dates.", false);
                }
            }
            Action::Open => {
                if selected < 0 && !self.rows.is_empty() {
                    self.select(Some(0));
                }
                self.open_selected(editor::FIELD_TITLE);
            }
            Action::ToggleDone => match self.selected_item_id() {
                Some(id) => self.toggle_done(id),
                None => self.toast("Pick a note with ↓ first.", false),
            },
            Action::Delete => match self.selected_item_id() {
                Some(id) => self.delete(id),
                None => self.toast("Pick a note with ↓ first.", false),
            },
            Action::Undo => {
                // Undo whatever happened last: typing in the box, or a change to a note.
                let typed_last = self.last_typed.is_some_and(|t| self.last_change.is_none_or(|c| t > c));
                if selected < 0 && !query.is_empty() && (typed_last || self.undo.is_empty()) {
                    return false; // the text input undoes the typing
                }
                self.undo();
            }
            Action::AddReminder | Action::RemoveReminder => {
                self.toast("Open a note (Enter) to change its reminders.", false);
            }
            other => self.global_action(other),
        }
        true
    }

    fn key_help(&mut self, combo: &Combo, action: Option<Action>) -> bool {
        let bare = !combo.is_modified() && !combo.shift;
        if (bare && matches!(combo.key.as_str(), "Escape" | "Enter")) || action == Some(Action::Help) {
            self.show_list(self.listing);
            return true;
        }
        match action {
            Some(a) if a != Action::Undo => {
                self.global_action(a);
                true
            }
            _ => false,
        }
    }

    /// Actions that work the same everywhere.
    fn global_action(&mut self, action: Action) {
        match action {
            Action::Inbox => self.show_list(View::Inbox),
            Action::Archive => self.show_list(View::Archive),
            Action::Settings => self.open_settings(),
            Action::Help => self.open_help(),
            Action::Quit => self.quit(),
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // The list

    fn show_list(&mut self, view: View) {
        let ui = self.ui();
        if ui.get_view() != view {
            self.select(None);
        }
        self.listing = view;
        ui.set_view(view);
        self.refresh();
        ui.invoke_focus_capture();
    }

    pub fn query_edited(&mut self) {
        self.last_typed = Some(Instant::now());
        self.select(None);
        self.refresh();
    }

    fn select(&mut self, index: Option<usize>) {
        let ui = self.ui();
        self.selected_id = index.and_then(|i| self.rows.get(i).copied());
        ui.set_selected(index.filter(|&i| i < self.rows.len()).map_or(-1, |i| i as i32));
    }

    fn selected_item_id(&self) -> Option<u64> {
        let index = self.ui().get_selected();
        (index >= 0).then(|| self.rows.get(index as usize).copied()).flatten()
    }

    fn move_selection(&mut self, delta: i32) -> bool {
        if self.rows.is_empty() {
            return true;
        }
        let current = self.ui().get_selected();
        let next = (current + delta).clamp(-1, self.rows.len() as i32 - 1);
        self.select((next >= 0).then_some(next as usize));
        true
    }

    /// Tab and Shift+Tab step through the notes and wrap around at either end.
    fn cycle_selection(&mut self, delta: i32) -> bool {
        let count = self.rows.len() as i32;
        if count > 0 {
            let current = self.ui().get_selected();
            let next = match current {
                c if c >= 0 => (c + delta).rem_euclid(count),
                _ if delta > 0 => 0,
                _ => count - 1,
            };
            self.select(Some(next as usize));
        }
        true
    }

    pub fn row_clicked(&mut self, index: i32) {
        self.select(usize::try_from(index).ok());
        self.ui().invoke_focus_capture();
    }

    pub fn row_opened(&mut self, index: i32) {
        self.select(usize::try_from(index).ok());
        self.open_selected(editor::FIELD_TITLE);
    }

    pub fn row_toggled(&mut self, index: i32) {
        if let Some(&id) = usize::try_from(index).ok().and_then(|i| self.rows.get(i)) {
            self.toggle_done(id);
        }
        self.ui().invoke_focus_capture();
    }

    pub fn tab_clicked(&mut self, index: i32) {
        self.show_list(if index == 1 { View::Archive } else { View::Inbox });
    }

    pub fn back_clicked(&mut self) {
        match self.ui().get_view() {
            View::Editor => {
                self.close_editor(false);
            }
            View::Help => self.show_list(self.listing),
            _ => {}
        }
    }

    fn open_selected(&mut self, field: i32) {
        if let Some(id) = self.selected_item_id() {
            self.open_editor(id, field);
        }
    }

    /// Saves what's in the input as a new note: the first line is the title, the rest its notes.
    fn capture(&mut self, then_edit: bool) {
        let ui = self.ui();
        let text = ui.get_query();
        if text.trim().is_empty() {
            return;
        }
        let (title, notes) = model::split_title(&text);
        let id = self.store.add(&title, &notes, Self::now().timestamp());
        self.remember(UndoStep::Created(id));
        self.persist();
        ui.set_query(SharedString::new());
        self.select(None);
        self.refresh();
        if then_edit {
            self.open_editor(id, editor::FIELD_START);
        } else {
            let undo = self.label(Action::Undo);
            self.toast(format!("Saved. {undo} takes it back."), false);
        }
    }

    fn toggle_done(&mut self, id: u64) {
        let now = Self::now().timestamp();
        let undo = self.label(Action::Undo);
        let Some(item) = self.store.get_mut(id) else { return };
        let before = item.clone();
        let message = if item.done_at.is_some() {
            item.done_at = None;
            "Back in your notes.".to_owned()
        } else {
            item.done_at = Some(now);
            item.ringing = false;
            format!("Done, and moved to the Done list. {undo} to undo.")
        };
        self.remember(UndoStep::Changed(before));
        self.persist();
        self.refresh();
        self.toast(message, false);
    }

    fn delete(&mut self, id: u64) {
        let Some((pos, item)) = self.store.remove(id) else { return };
        self.remember(UndoStep::Removed { pos, item });
        self.persist();
        self.refresh();
        let undo = self.label(Action::Undo);
        self.toast(format!("Deleted. {undo} brings it back."), false);
    }

    fn remember(&mut self, step: UndoStep) {
        const KEEP: usize = 50;
        if self.undo.len() >= KEEP {
            self.undo.remove(0);
        }
        self.undo.push(step);
        self.last_change = Some(Instant::now());
    }

    fn undo(&mut self) {
        let Some(step) = self.undo.pop() else {
            self.toast("Nothing to undo.", false);
            return;
        };
        let restored = match step {
            UndoStep::Created(id) => {
                if let Some((_, item)) = self.store.remove(id) {
                    // Put the words back where they were typed.
                    let ui = self.ui();
                    if ui.get_query().trim().is_empty() {
                        let text = if item.notes.is_empty() { item.text } else { format!("{}\n{}", item.text, item.notes) };
                        ui.set_query(text.into());
                    }
                }
                self.toast("Unsaved. The text is back in the box.", false);
                None
            }
            UndoStep::Removed { pos, item } => {
                let id = item.id;
                self.store.insert(pos, item);
                self.toast("Restored.", false);
                Some(id)
            }
            UndoStep::Changed(before) => {
                let id = before.id;
                self.store.replace(before);
                self.toast("Undone.", false);
                Some(id)
            }
        };
        self.persist();
        if let Some(id) = restored {
            let done = self.store.get(id).is_some_and(Item::is_done);
            let view = if done { View::Archive } else { View::Inbox };
            if self.ui().get_view() != view {
                self.show_list(view);
            }
            self.selected_id = Some(id);
        }
        self.refresh();
        self.ui().invoke_focus_capture();
    }

    /// Rebuilds the visible list (and the clock and counters) from the store.
    pub fn refresh(&mut self) {
        let ui = self.ui();
        let now = Self::now();
        let now_ts = now.timestamp();
        self.last_minute = now_ts / 60;
        ui.set_clock_time(now.format("%H:%M").to_string().into());
        ui.set_clock_date(now.format("%a, %b %-d").to_string().into());

        let archive = self.listing == View::Archive;
        let query = ui.get_query();
        let at = |t: i64| Local.timestamp_opt(t, 0).single().unwrap_or(now);

        let open_count = self.store.data.items.iter().filter(|i| !i.is_done()).count();
        let done_count = self.store.data.items.len() - open_count;
        let mut items: Vec<&Item> = self.store.data.items.iter().filter(|i| i.is_done() == archive && i.matches(&query)).collect();

        let mut sections: Vec<String> = Vec::new();
        if archive {
            items.sort_by_key(|i| std::cmp::Reverse(i.done_at));
            for i in &items {
                let day = at(i.done_at.unwrap_or(0));
                sections.push(match (now.date_naive() - day.date_naive()).num_days() {
                    0 => "Done today".into(),
                    1 => "Done yesterday".into(),
                    _ => format!("Done {}", dates::age_label(&day, &now)),
                });
            }
        } else {
            let rank = |i: &Item| if i.ringing { 0 } else if i.is_overdue(now_ts) { 1 } else { 2 };
            let (mut scheduled, mut notes): (Vec<&Item>, Vec<&Item>) = items.into_iter().partition(|i| i.is_scheduled());
            scheduled.sort_by_key(|i| (rank(i), i.schedule_key(now_ts)));
            notes.sort_by_key(|i| std::cmp::Reverse((i.created, i.id)));
            sections.extend(scheduled.iter().map(|_| "Scheduled".to_owned()));
            sections.extend(notes.iter().map(|_| "Notes".to_owned()));
            items = scheduled.into_iter().chain(notes).collect();
        }

        let mut rows = Vec::with_capacity(items.len());
        let mut y = 0.0f32;
        for (index, item) in items.iter().enumerate() {
            let section = if index == 0 || sections[index] != sections[index - 1] { sections[index].clone() } else { String::new() };
            let start = match item.start {
                Some(s) if !archive && (s > now_ts || item.due.is_none()) => {
                    format!("{} {}", if s > now_ts { "Starts" } else { "Started" }, mid_sentence(dates::day_label(&at(s), &now)))
                }
                _ => String::new(),
            };
            let due = match item.due {
                Some(d) if !archive => format!("Due {}", mid_sentence(dates::day_label(&at(d), &now))),
                _ => String::new(),
            };
            let note = if archive {
                at(item.done_at.unwrap_or(0)).format("%H:%M").to_string()
            } else if item.is_scheduled() {
                String::new()
            } else {
                dates::age_label(&at(item.created), &now)
            };
            let kind = if archive {
                4
            } else if item.ringing {
                2
            } else if item.is_overdue(now_ts) {
                3
            } else if item.is_scheduled() {
                1
            } else {
                0
            };
            rows.push(ItemRow {
                id: item.id as i32,
                kind,
                title: item.title().into(),
                preview: item.notes_preview().into(),
                section: section.as_str().into(),
                start: start.into(),
                due: due.into(),
                overdue: item.is_overdue(now_ts),
                bells: if archive { 0 } else { item.reminders.len() as i32 },
                ringing: item.ringing,
                note: note.into(),
                top: y,
            });
            y += ROW_H + if section.is_empty() { 0.0 } else { SECTION_H };
        }

        let previous = ui.get_selected();
        self.rows = items.iter().map(|i| i.id).collect();
        ui.set_items(ModelRc::new(VecModel::from(rows)));
        ui.set_list_height(y);

        // Keep the same note selected; if it left this list, stay at the same spot.
        let index = match self.selected_id {
            Some(id) => self.rows.iter().position(|&r| r == id).or_else(|| {
                (previous >= 0 && !self.rows.is_empty()).then(|| (previous as usize).min(self.rows.len() - 1))
            }),
            None => None,
        };
        self.selected_id = index.map(|i| self.rows[i]);
        ui.set_selected(index.map_or(-1, |i| i as i32));

        ui.set_open_count(open_count as i32);
        ui.set_done_count(done_count as i32);
        let filtered = !query.trim().is_empty();
        let total = if archive { done_count } else { open_count };
        let shown = self.rows.len();
        ui.set_status(if filtered && total > 0 { format!("{shown} of {total} match").into() } else { SharedString::new() });

        let summon = self.summon_label();
        let (title, body) = match (archive, filtered) {
            (false, false) if self.store.data.items.is_empty() => {
                ("Nothing written yet".to_owned(), format!("Type above and press Enter. {summon} opens Wisp from any app."))
            }
            (false, false) => ("All clear".to_owned(), "Notes you mark done wait in the Done tab.".to_owned()),
            (false, true) if open_count == 0 => ("Press Enter to save it".to_owned(), String::new()),
            (false, true) => ("No notes match".to_owned(), "Press Enter to save this as a new note.".to_owned()),
            (true, false) => ("Nothing done yet".to_owned(), format!("Mark a note done with {} and it moves here.", self.label(Action::ToggleDone))),
            (true, true) => ("Nothing done matches".to_owned(), String::new()),
        };
        ui.set_empty_title(title.into());
        ui.set_empty_body(body.into());
    }

    // -----------------------------------------------------------------------
    // Reminders

    /// Fires due reminders; called every few seconds.
    pub fn tick(&mut self) {
        let now = Self::now();
        let ts = now.timestamp();
        let mut fired = Vec::new();
        let mut changed = false;
        for item in &mut self.store.data.items {
            let before: Vec<Option<i64>> = item.reminders.iter().map(|r| r.last_fired).collect();
            if let Some(f) = item.collect_due(&Local, ts, REMINDER_GRACE_SECS) {
                fired.push(f.id);
            }
            changed |= item.reminders.iter().map(|r| r.last_fired).ne(before);
        }
        if changed {
            self.persist();
        }
        for id in &fired {
            if let Some(item) = self.store.get(*id) {
                let title = item.title().to_owned();
                // If the desktop can't show notifications, Wisp shows itself instead.
                platform::notify(title.clone(), model::notification_body(item, &now), || {
                    let _ = slint::invoke_from_event_loop(|| {
                        with_app(|app| {
                            if !app.ui().window().is_visible() {
                                app.summon();
                            }
                        });
                    });
                });
                self.toast_for(format!("Reminder: {title}"), false, Duration::from_secs(20));
            }
        }
        if !fired.is_empty() && self.config.open_on_reminder && !self.ui().window().is_visible() {
            self.summon();
        }
        if !fired.is_empty() || ts / 60 != self.last_minute {
            if self.ui().get_view() != View::Editor {
                self.refresh();
            } else {
                self.last_minute = ts / 60;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Hotkey, theme, help

    /// Registers the summon key from the settings. When that's the default and
    /// another program holds it, the fallback stands in, so Wisp can always be opened.
    fn claim_summon(&mut self) {
        if self.hotkeys.is_none() {
            return; // `summon_problem` already says why
        }
        let wanted = self.config.summon_combo();
        self.summon_problem = match self.register_summon(&wanted) {
            Ok(()) => None,
            Err(err) if self.config.summon.is_empty() => {
                let fallback: Combo = config::FALLBACK_SUMMON.parse().expect("the fallback summon key parses");
                match self.register_summon(&fallback) {
                    Ok(()) => Some(format!(
                        "{} is taken by another program, so Wisp opens with {} for now.",
                        wanted.label(),
                        fallback.label()
                    )),
                    Err(_) => Some(err),
                }
            }
            Err(err) => Some(err),
        };
    }

    fn register_summon(&mut self, combo: &Combo) -> Result<(), String> {
        let manager = self.hotkeys.as_ref().ok_or_else(|| "System-wide keys aren't available here.".to_owned())?;
        let hotkey = combo.to_hotkey()?;
        if let Some((old, _)) = self.summon.take() {
            let _ = manager.unregister(old);
        }
        manager
            .register(hotkey)
            .map_err(|e| format!("Couldn't claim {} ({e}). Another app may use it.", combo.label()))?;
        self.summon = Some((hotkey, combo.clone()));
        SUMMON_ID.store(hotkey.id(), Ordering::SeqCst);
        Ok(())
    }

    /// Pushes shortcut labels and the theme into both windows.
    fn apply_config_to_ui(&mut self) {
        let ui = self.ui();
        let l = |a| self.config.combo(a).label().into();
        ui.set_keys(Keys {
            schedule: l(Action::Schedule),
            open: l(Action::Open),
            done: l(Action::ToggleDone),
            delete: l(Action::Delete),
            undo: l(Action::Undo),
            inbox: l(Action::Inbox),
            archive: l(Action::Archive),
            settings: l(Action::Settings),
            help: l(Action::Help),
            add_reminder: l(Action::AddReminder),
            remove_reminder: l(Action::RemoveReminder),
            quit: l(Action::Quit),
        });
        let choice = match self.config.theme {
            Theme::System => 0,
            Theme::Light => 1,
            Theme::Dark => 2,
        };
        ui.global::<crate::Theme>().set_choice(choice);
        self.settings_ui.global::<crate::Theme>().set_choice(choice);
        if let Some(tray) = &self.tray {
            tray.set_summon_label(self.summon_label().into());
        }
        ui.set_help(ModelRc::new(VecModel::from(self.help_rows())));
    }

    fn open_help(&mut self) {
        let ui = self.ui();
        ui.set_help(ModelRc::new(VecModel::from(self.help_rows())));
        ui.set_back_label(if self.listing == View::Archive { "Done" } else { "Notes" }.into());
        ui.set_view(View::Help);
        ui.invoke_focus_help();
    }

    fn help_rows(&self) -> Vec<HelpRow> {
        let l = |a| self.label(a);
        let groups: Vec<(&str, Vec<(String, &str)>)> = vec![
            ("Anywhere", vec![(self.summon_label(), "Open or hide Wisp")]),
            (
                "Writing",
                vec![
                    ("Enter".into(), "Save the note"),
                    ("Shift+Enter".into(), "New line (lines after the first become its notes)"),
                    (l(Action::Schedule), "Save, then add dates and reminders"),
                    ("Esc".into(), "Hide Wisp (what you typed stays)"),
                ],
            ),
            (
                "Your notes",
                vec![
                    ("↓ ↑".into(), "Move through notes"),
                    ("Tab  Shift+Tab".into(), "Next or previous note, round and round"),
                    ("Enter".into(), "Open the selected note"),
                    (l(Action::ToggleDone), "Mark done (or bring back from Done)"),
                    (l(Action::Delete), "Delete"),
                    (l(Action::Undo), "Undo the last change"),
                    (l(Action::Inbox), "Notes tab"),
                    (l(Action::Archive), "Done tab"),
                    ("Esc".into(), "Back to typing"),
                ],
            ),
            (
                "Editing a note",
                vec![
                    ("Enter".into(), "From the title, go to the notes"),
                    ("Tab".into(), "Next field"),
                    ("Esc".into(), "Save and close (twice: keep what's valid)"),
                    (l(Action::AddReminder), "Add a reminder"),
                    ("← →".into(), "Change the selected reminder"),
                    ("Delete".into(), "Remove the selected reminder"),
                    (l(Action::RemoveReminder), "Remove a reminder from anywhere"),
                ],
            ),
            (
                "Dates you can type",
                vec![
                    ("tomorrow 9am".into(), "Day and time"),
                    ("fri 17:00".into(), "This coming Friday"),
                    ("+30m  in 2 hours".into(), "From now"),
                    ("10-20 14:00".into(), "Month and day"),
                    ("next mon".into(), "Monday next week"),
                    ("明天下午3点".into(), "Chinese works too"),
                ],
            ),
            ("Wisp", vec![(l(Action::Settings), "Settings"), (l(Action::Help), "This list"), (l(Action::Quit), "Quit")]),
        ];
        let mut rows = Vec::new();
        for (section, entries) in groups {
            for (i, (keys, label)) in entries.into_iter().enumerate() {
                rows.push(HelpRow {
                    keys: keys.into(),
                    label: label.into(),
                    section: if i == 0 { section.into() } else { SharedString::new() },
                });
            }
        }
        rows
    }
}

/// Centres the window horizontally, a quarter of the way down the screen it's on,
/// and asks for keyboard focus. Returns false if the native window doesn't exist yet.
fn place_and_focus(ui: &AppWindow) -> bool {
    ui.window()
        .with_winit_window(|w| {
            if let Some(monitor) = w.current_monitor().or_else(|| w.primary_monitor()) {
                let (area, origin, size) = (monitor.size(), monitor.position(), w.outer_size());
                let x = origin.x + (area.width as i32 - size.width as i32) / 2;
                let y = origin.y + (area.height as i32 - size.height as i32) / 4;
                w.set_outer_position(slint::winit_030::winit::dpi::PhysicalPosition::new(x, y.max(origin.y)));
            }
            w.focus_window();
        })
        .is_some()
}

/// Wayland sessions only deliver grabbed keys while an X11 window has focus.
fn on_wayland() -> bool {
    cfg!(target_os = "linux") && std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t.eq_ignore_ascii_case("wayland"))
}

/// "Tomorrow 09:00" → "tomorrow 09:00", for use after a verb.
fn mid_sentence(label: String) -> String {
    for word in ["Today", "Tomorrow", "Yesterday"] {
        if let Some(rest) = label.strip_prefix(word) {
            return format!("{}{rest}", word.to_lowercase());
        }
    }
    label
}
