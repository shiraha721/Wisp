//! The controller: owns the notes and settings, reacts to keys and clicks, and
//! keeps the window's properties in step with the data.

use crate::config::{self, Action, Config, Theme, ACTIONS};
use crate::dates;
use crate::model::{self, Item, Reminder, Rule, Store, RULE_KINDS};
use crate::platform;
use crate::shortcuts::Combo;
use crate::{AppWindow, HelpRow, Hint, ItemRow, Keys, ReminderRow, SettingRow, View};
use chrono::{DateTime, Local, TimeZone};
use global_hotkey::hotkey::HotKey;
use global_hotkey::GlobalHotKeyManager;
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// Reminders missed by more than this (the computer was off, say) stay quiet.
const REMINDER_GRACE_SECS: i64 = 6 * 3600;
const START_DEFAULT_TIME: (u32, u32) = (9, 0);
const DUE_DEFAULT_TIME: (u32, u32) = (18, 0);

/// `WISP_DEBUG=1` prints window and focus events to stderr.
macro_rules! debug_log {
    ($($arg:tt)*) => {
        if std::env::var_os("WISP_DEBUG").is_some() {
            eprintln!("wisp: {}", format_args!($($arg)*));
        }
    };
}

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

struct EditSession {
    id: u64,
    before: Item,
    kinds: Vec<usize>,
    return_to: View,
    auto_reminder_done: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingKind {
    Summon,
    Shortcut(usize),
    Autostart,
    HideInactive,
    Theme,
    Reset,
    Folder,
}

pub struct App {
    ui: slint::Weak<AppWindow>,
    store: Store,
    config: Config,
    config_path: PathBuf,
    data_dir: PathBuf,
    rows: Vec<u64>,
    selected_id: Option<u64>,
    listing: View,
    undo: Vec<UndoStep>,
    last_change: Option<Instant>,
    last_typed: Option<Instant>,
    editor: Option<EditSession>,
    reminders: Rc<VecModel<ReminderRow>>,
    settings: Vec<SettingKind>,
    setting_index: usize,
    recording: bool,
    setting_error: Option<(SettingKind, String)>,
    hotkeys: Option<GlobalHotKeyManager>,
    summon: Option<HotKey>,
    summon_error: Option<String>,
    toast_timer: slint::Timer,
    shown_at: Instant,
    /// The window got focus since it was last shown; only then may losing focus hide it.
    had_focus: bool,
    /// The window has been centred on a screen (needs the native window to exist).
    placed: bool,
    last_minute: i64,
    tray: Option<crate::Tray>,
}

impl App {
    pub fn new(ui: &AppWindow, data_dir: PathBuf, portable: bool) -> App {
        let (store, store_warning) = Store::load(&data_dir.join("notes.json"));
        let config_path = data_dir.join("settings.json");
        let (mut config, config_warning) = Config::load(&config_path);

        if !config.initialized {
            // Turn on "start with the computer" once, for installed release builds.
            config.launch_at_startup = !portable && !cfg!(debug_assertions);
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
            store,
            config,
            config_path,
            data_dir,
            rows: Vec::new(),
            selected_id: None,
            listing: View::Inbox,
            undo: Vec::new(),
            last_change: None,
            last_typed: None,
            editor: None,
            reminders,
            settings: Vec::new(),
            setting_index: 0,
            recording: false,
            setting_error: None,
            hotkeys: None,
            summon: None,
            summon_error: None,
            toast_timer: slint::Timer::default(),
            shown_at: Instant::now(),
            had_focus: false,
            placed: false,
            last_minute: 0,
            tray: None,
        };

        match GlobalHotKeyManager::new() {
            Ok(manager) => app.hotkeys = Some(manager),
            Err(err) => app.summon_error = Some(format!("System-wide keys aren't available here ({err}).")),
        }
        let summon = app.config.summon_combo();
        if let Err(err) = app.register_summon(&summon) {
            app.summon_error = Some(err);
        }

        app.apply_config_to_ui();
        app.refresh();
        if let Some(warning) = store_warning.or(config_warning) {
            app.toast(warning, true);
        } else if let Some(err) = app.summon_error.clone() {
            let settings = app.label(Action::Settings);
            app.toast_for(format!("{err} Pick another key in Settings ({settings})."), true, Duration::from_secs(20));
        }
        app
    }

    pub fn set_tray(&mut self, tray: Option<crate::Tray>) {
        if let Some(t) = &tray {
            t.set_summon_label(self.config.summon_combo().label().into());
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
            self.toast(format!("Couldn't save settings: {err}"), true);
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
        match ui.get_view() {
            View::Settings | View::Help => self.show_list(self.listing),
            _ => {}
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
            View::Editor => ui.invoke_focus_editor(0),
            View::Settings => ui.invoke_focus_settings(),
            View::Help => ui.invoke_focus_help(),
        }
    }

    /// Hides the window, saving an open note first. The draft in the input stays.
    pub fn dismiss(&mut self) {
        debug_log!("dismiss");
        let ui = self.ui();
        match ui.get_view() {
            View::Editor => {
                if let Err(err) = self.save_editor() {
                    // Keep the editor so nothing typed is lost; the message waits for the next visit.
                    self.toast(err, true);
                } else {
                    self.show_list(self.listing);
                }
            }
            View::Settings | View::Help => self.show_list(self.listing),
            _ => {}
        }
        self.stop_recording();
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
        if !self.config.hide_when_inactive || !ui.window().is_visible() || self.recording || !self.had_focus {
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
            "quit" => self.quit(),
            _ => {}
        }
    }

    pub fn quit(&mut self) {
        if self.editor.is_some() {
            let _ = self.save_editor();
        }
        self.persist();
        let _ = self.ui().window().hide();
        if let Some(tray) = &self.tray {
            let _ = tray.hide();
        }
        let _ = slint::quit_event_loop();
    }

    // -----------------------------------------------------------------------
    // Keys

    pub fn on_key(&mut self, text: &str, ctrl: bool, alt: bool, shift: bool, meta: bool, context: &str) -> bool {
        let Some(combo) = Combo::from_event(text, ctrl, alt, shift, meta) else {
            return self.recording; // lone modifier presses, IME text
        };
        if self.recording {
            self.record(combo);
            return true;
        }
        let action = self.config.action_for(&combo);
        match self.ui().get_view() {
            View::Inbox | View::Archive => self.key_list(&combo, action),
            View::Editor => self.key_editor(&combo, action, context),
            View::Settings => self.key_settings(&combo, action),
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
                    self.open_selected(0);
                } else if ui.get_view() == View::Inbox {
                    self.capture(false);
                }
                return true;
            }
            "Down" if bare && can_move => return self.move_selection(1),
            "Up" if bare && can_move => return self.move_selection(-1),
            "Tab" if !combo.is_modified() => return self.move_selection(if combo.shift { -1 } else { 1 }),
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
                    self.open_selected(1);
                } else {
                    self.toast("Type a note first, then add its dates.", false);
                }
            }
            Action::Open => {
                if selected < 0 && !self.rows.is_empty() {
                    self.select(Some(0));
                }
                self.open_selected(0);
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
            other => self.global_action(other),
        }
        true
    }

    fn key_editor(&mut self, combo: &Combo, action: Option<Action>, context: &str) -> bool {
        let bare = !combo.is_modified() && !combo.shift;
        match combo.key.as_str() {
            "Escape" if bare => {
                self.close_editor();
                return true;
            }
            "Enter" if bare && context == "editor" => {
                self.close_editor();
                return true;
            }
            _ => {}
        }
        let Some(action) = action else { return false };
        match action {
            Action::Schedule | Action::Open => {
                self.close_editor();
            }
            Action::AddReminder => self.add_reminder(),
            Action::ToggleDone => {
                if let Some(id) = self.editor.as_ref().map(|e| e.id) {
                    if self.close_editor() {
                        self.toggle_done(id);
                    }
                }
            }
            Action::Delete => {
                if let Some(session) = self.editor.take() {
                    let view = session.return_to;
                    self.delete(session.id);
                    self.show_list(view);
                }
            }
            Action::Undo => return false,
            other => {
                if self.close_editor() {
                    self.global_action(other);
                }
            }
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
            Action::Settings => {
                if self.ui().get_view() == View::Settings {
                    self.show_list(self.listing);
                } else {
                    self.open_settings();
                }
            }
            Action::Help => self.open_help(),
            Action::Quit => self.quit(),
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // The list

    fn show_list(&mut self, view: View) {
        let ui = self.ui();
        self.stop_recording();
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

    pub fn row_clicked(&mut self, index: i32) {
        self.select(usize::try_from(index).ok());
        self.ui().invoke_focus_capture();
    }

    pub fn row_opened(&mut self, index: i32) {
        self.select(usize::try_from(index).ok());
        self.open_selected(0);
    }

    pub fn row_toggled(&mut self, index: i32) {
        if let Some(&id) = usize::try_from(index).ok().and_then(|i| self.rows.get(i)) {
            self.toggle_done(id);
        }
        self.ui().invoke_focus_capture();
    }

    pub fn badge_clicked(&mut self) {
        let next = if self.listing == View::Inbox { View::Archive } else { View::Inbox };
        self.show_list(next);
    }

    fn open_selected(&mut self, field: i32) {
        if let Some(id) = self.selected_item_id() {
            self.open_editor(id, field);
        }
    }

    /// Saves what's in the input as a new note.
    fn capture(&mut self, then_edit: bool) {
        let ui = self.ui();
        let text = ui.get_query().trim().to_owned();
        if text.is_empty() {
            return;
        }
        let id = self.store.add(&text, Self::now().timestamp());
        self.remember(UndoStep::Created(id));
        self.persist();
        ui.set_query(SharedString::new());
        self.select(None);
        self.refresh();
        if then_edit {
            self.open_editor(id, 1);
        } else {
            let undo = self.label(Action::Undo);
            self.toast(format!("Saved. {undo} takes it back."), false);
        }
    }

    fn toggle_done(&mut self, id: u64) {
        let now = Self::now().timestamp();
        let Some(item) = self.store.get_mut(id) else { return };
        let before = item.clone();
        let message = if item.done_at.is_some() {
            item.done_at = None;
            "Back in your notes.".to_owned()
        } else {
            item.done_at = Some(now);
            item.ringing = false;
            format!("Done and archived. {} to undo.", self.config.combo(Action::Undo).label())
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
                        ui.set_query(item.text.into());
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

    /// Rebuilds the visible list from the store.
    pub fn refresh(&mut self) {
        let ui = self.ui();
        let now = Self::now();
        let now_ts = now.timestamp();
        self.last_minute = now_ts / 60;
        let archive = self.listing == View::Archive;
        let query = ui.get_query();
        let at = |t: i64| Local.timestamp_opt(t, 0).single().unwrap_or(now);

        let open_count = self.store.data.items.iter().filter(|i| !i.is_done()).count();
        let done_count = self.store.data.items.len() - open_count;
        let mut items: Vec<&Item> =
            self.store.data.items.iter().filter(|i| i.is_done() == archive && model::matches_query(&i.text, &query)).collect();

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
        let (row_h, section_h) = (42.0, 30.0);
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
            rows.push(ItemRow {
                id: item.id as i32,
                title: item.title().into(),
                more: item.has_more_lines(),
                section: section.as_str().into(),
                start: start.into(),
                due: due.into(),
                overdue: item.is_overdue(now_ts),
                bells: if archive { 0 } else { item.reminders.len() as i32 },
                ringing: item.ringing,
                note: note.into(),
                top: y,
            });
            y += row_h + if section.is_empty() { 0.0 } else { section_h };
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

        let filtered = !query.trim().is_empty();
        let shown = self.rows.len();
        let total = if archive { done_count } else { open_count };
        ui.set_badge(
            match (archive, filtered && total > 0) {
                (false, false) => format!("{open_count} open"),
                (true, false) => format!("{done_count} done"),
                (false, true) => format!("{shown} of {open_count}"),
                (true, true) => format!("{shown} of {done_count} done"),
            }
            .into(),
        );
        let summon = self.config.summon_combo().label();
        let (title, body) = match (archive, filtered) {
            (false, false) if self.store.data.items.is_empty() => {
                ("Nothing written yet.".to_owned(), format!("Type above and press Enter. {summon} opens Wisp from any app."))
            }
            (false, false) => ("All clear.".to_owned(), "Done notes are kept in the archive.".to_owned()),
            (false, true) if open_count == 0 => ("Press Enter to save it.".to_owned(), String::new()),
            (false, true) => ("No notes match.".to_owned(), "Press Enter to save this as a new note.".to_owned()),
            (true, false) => ("Nothing done yet.".to_owned(), format!("Mark a note done with {} and it lands here.", self.label(Action::ToggleDone))),
            (true, true) => ("Nothing done matches.".to_owned(), String::new()),
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
        if !fired.is_empty() || ts / 60 != self.last_minute {
            if self.ui().get_view() != View::Editor {
                self.refresh();
            } else {
                self.last_minute = ts / 60;
            }
        }
    }

    // -----------------------------------------------------------------------
    // The editor

    fn open_editor(&mut self, id: u64, field: i32) {
        let Some(item) = self.store.get_mut(id) else { return };
        if item.ringing {
            item.ringing = false;
            self.persist();
        }
        let Some(item) = self.store.get(id).cloned() else { return };
        let ui = self.ui();
        let now = Self::now();
        let at = |t: i64| Local.timestamp_opt(t, 0).single().map(|t| dates::canonical(&t)).unwrap_or_default();

        ui.set_edit_text(item.text.as_str().into());
        ui.set_edit_start(item.start.map(at).unwrap_or_default().into());
        ui.set_edit_due(item.due.map(at).unwrap_or_default().into());
        let written = Local.timestamp_opt(item.created, 0).single().unwrap_or(now);
        let age = dates::age_label(&written, &now);
        let age = match age.as_str() {
            "Yesterday" => "yesterday".to_owned(),
            a if a == "just now" || a.ends_with("ago") => age,
            _ => format!("on {age}"),
        };
        ui.set_edit_meta(format!("Written {age}").into());

        let kinds: Vec<usize> = item.reminders.iter().map(|r| r.rule.kind()).collect();
        let rows: Vec<ReminderRow> = item.reminders.iter().map(|r| reminder_row(r.rule.kind(), &r.rule.amount(), false)).collect();
        self.reminders.set_vec(rows);

        self.editor = Some(EditSession {
            id,
            before: item,
            kinds,
            return_to: self.listing,
            auto_reminder_done: false,
        });
        self.update_editor();
        ui.set_view(View::Editor);
        ui.invoke_focus_editor(field);
        let weak = self.ui.clone();
        slint::Timer::single_shot(Duration::from_millis(30), move || {
            if let Some(ui) = weak.upgrade() {
                ui.invoke_focus_editor(field);
            }
        });
    }

    fn parse_fields(&self) -> (Result<Option<i64>, String>, Result<Option<i64>, String>) {
        let ui = self.ui();
        let now = Self::now();
        let parse = |text: SharedString, (h, m): (u32, u32)| dates::parse(&text, &now, dates::hm(h, m)).map(|t| t.map(|t| t.timestamp()));
        (parse(ui.get_edit_start(), START_DEFAULT_TIME), parse(ui.get_edit_due(), DUE_DEFAULT_TIME))
    }

    /// Re-reads the editor fields and refreshes every hint under them.
    pub fn update_editor(&mut self) {
        let ui = self.ui();
        let now = Self::now();
        let now_ts = now.timestamp();
        let (start, due) = self.parse_fields();
        let at = |t: i64| Local.timestamp_opt(t, 0).single().unwrap_or(now);
        let describe = |t: i64| format!("{} ({})", at(t).format("%a %b %-d, %H:%M"), dates::relative(t - now_ts));
        let hint = |r: &Result<Option<i64>, String>| match r {
            Ok(Some(t)) => Hint { text: describe(*t).into(), error: false },
            Ok(None) => Hint::default(),
            Err(e) => Hint { text: e.as_str().into(), error: true },
        };
        ui.set_start_hint(hint(&start));
        let mut due_hint = hint(&due);
        if let (Ok(Some(s)), Ok(Some(d))) = (&start, &due) {
            if d < s {
                due_hint = Hint { text: "Due is before the start.".into(), error: true };
            }
        }
        ui.set_due_hint(due_hint);

        let start = start.ok().flatten();
        let due = due.ok().flatten();
        let Some(session) = self.editor.as_mut() else { return };

        // The first date gets a reminder to go with it, once per visit.
        if !session.auto_reminder_done && session.kinds.is_empty() && (start.is_some() || due.is_some()) {
            session.auto_reminder_done = true;
            let rule = if start.is_some() { Rule::AtStart } else { Rule::BeforeDue { minutes: 60 } };
            session.kinds.push(rule.kind());
            self.reminders.push(reminder_row(rule.kind(), &rule.amount(), false));
        }
        if !session.kinds.is_empty() {
            session.auto_reminder_done = true;
        }

        let hints: Vec<Hint> = session
            .kinds
            .iter()
            .enumerate()
            .map(|(i, &kind)| {
                let amount = self.reminders.row_data(i).map(|r| r.amount).unwrap_or_default();
                match Rule::from_parts(kind, &amount) {
                    Err(e) => Hint { text: e.into(), error: true },
                    Ok(rule) if rule.needs_start() && start.is_none() => Hint { text: "Needs a start time".into(), error: true },
                    Ok(rule) if !rule.needs_start() && due.is_none() => Hint { text: "Needs a due time".into(), error: true },
                    Ok(rule) => match rule.next_after(&Local, start, due, now_ts) {
                        Some(t) => Hint { text: dates::day_label(&at(t), &now).into(), error: false },
                        None => Hint { text: "Already passed".into(), error: false },
                    },
                }
            })
            .collect();
        ui.set_reminder_hints(ModelRc::new(VecModel::from(hints)));
    }

    pub fn reminder_cycle(&mut self, index: i32, delta: i32) {
        let Some(session) = self.editor.as_mut() else { return };
        let Some(kind) = usize::try_from(index).ok().and_then(|i| session.kinds.get_mut(i)) else { return };
        let old = *kind;
        let next = (old as i32 + delta).rem_euclid(RULE_KINDS.len() as i32) as usize;
        *kind = next;
        let index = index as usize;
        let amount = self.reminders.row_data(index).map(|r| r.amount.to_string()).unwrap_or_default();
        let minutes_kind = |k: usize| matches!(k, 1..=3);
        let amount = if minutes_kind(old) && minutes_kind(next) { amount } else { Rule::default_amount(next).to_owned() };
        self.reminders.set_row_data(index, reminder_row(next, &amount, false));
        self.update_editor();
    }

    pub fn reminder_remove(&mut self, index: i32) {
        let Some(session) = self.editor.as_mut() else { return };
        let Ok(index) = usize::try_from(index) else { return };
        if index >= session.kinds.len() {
            return;
        }
        session.kinds.remove(index);
        self.reminders.remove(index);
        self.update_editor();
        later(|app| app.ui().invoke_focus_editor(3));
    }

    pub fn add_reminder(&mut self) {
        let (start, due) = self.parse_fields();
        let has_start = matches!(start, Ok(Some(_)));
        let has_due = matches!(due, Ok(Some(_)));
        let Some(session) = self.editor.as_mut() else { return };
        let rule = if has_start && !session.kinds.contains(&Rule::AtStart.kind()) {
            Rule::AtStart
        } else if has_start {
            Rule::BeforeStart { minutes: 15 }
        } else if has_due {
            Rule::BeforeDue { minutes: 60 }
        } else {
            Rule::AtStart
        };
        session.kinds.push(rule.kind());
        session.auto_reminder_done = true;
        for i in 0..self.reminders.row_count() {
            if let Some(mut row) = self.reminders.row_data(i) {
                if row.fresh {
                    row.fresh = false;
                    self.reminders.set_row_data(i, row);
                }
            }
        }
        self.reminders.push(reminder_row(rule.kind(), &rule.amount(), true));
        self.update_editor();
    }

    /// Writes the editor back to the store. On error nothing is saved.
    fn save_editor(&mut self) -> Result<(), String> {
        let Some(session) = self.editor.as_ref() else { return Ok(()) };
        let ui = self.ui();
        let now = Self::now().timestamp();
        let (start, due) = self.parse_fields();
        let start = start.map_err(|e| {
            ui.invoke_focus_editor(1);
            format!("Starts: {e}")
        })?;
        let due = due.map_err(|e| {
            ui.invoke_focus_editor(2);
            format!("Due: {e}")
        })?;
        if let (Some(s), Some(d)) = (start, due) {
            if d < s {
                ui.invoke_focus_editor(2);
                return Err("The due time is before the start.".into());
            }
        }
        let mut reminders = Vec::new();
        for (i, &kind) in session.kinds.iter().enumerate() {
            let amount = self.reminders.row_data(i).map(|r| r.amount).unwrap_or_default();
            let rule = Rule::from_parts(kind, &amount).map_err(|e| format!("Reminder: {e}"))?;
            if rule.needs_start() && start.is_none() {
                return Err(format!("“{}” needs a start time.", RULE_KINDS[kind]));
            }
            if !rule.needs_start() && due.is_none() {
                return Err(format!("“{}” needs a due time.", RULE_KINDS[kind]));
            }
            if !reminders.iter().any(|r: &Reminder| r.rule == rule) {
                reminders.push(Reminder::new(rule));
            }
        }

        let id = session.id;
        let before = session.before.clone();
        let text = ui.get_edit_text().trim().to_owned();
        self.editor = None;
        if text.is_empty() {
            self.delete(id);
            return Ok(());
        }
        let Some(mut item) = self.store.get(id).cloned() else { return Ok(()) };
        item.text = text;
        item.start = start;
        item.due = due;
        // Keep the firing history of rules that didn't change.
        let previous = std::mem::take(&mut item.reminders);
        item.reminders = reminders;
        item.arm_reminders(&Local, now);
        for r in &mut item.reminders {
            if let Some(old) = previous.iter().find(|p| p.rule == r.rule) {
                if before.start == start && before.due == due {
                    r.last_fired = old.last_fired.max(r.last_fired);
                }
            }
        }
        let changed = item.text != before.text || item.start != before.start || item.due != before.due || item.reminders.iter().map(|r| r.rule).ne(before.reminders.iter().map(|r| r.rule));
        if changed {
            item.updated = now;
            self.remember(UndoStep::Changed(before));
            self.store.replace(item);
            self.persist();
        }
        self.selected_id = Some(id);
        Ok(())
    }

    /// Saves and returns to the list. Returns false (and says why) if the fields don't add up.
    fn close_editor(&mut self) -> bool {
        let Some(view) = self.editor.as_ref().map(|e| e.return_to) else { return true };
        match self.save_editor() {
            Ok(()) => {
                let keep = self.selected_id;
                self.show_list(view);
                self.selected_id = keep;
                self.refresh();
                true
            }
            Err(err) => {
                self.toast(err, true);
                false
            }
        }
    }

    // -----------------------------------------------------------------------
    // Settings & help

    fn open_settings(&mut self) {
        if self.editor.is_some() && !self.close_editor() {
            return;
        }
        let ui = self.ui();
        self.setting_index = 0;
        self.setting_error = None;
        self.render_settings();
        ui.set_view(View::Settings);
        ui.invoke_focus_settings();
    }

    pub fn open_settings_from_tray(&mut self) {
        self.summon();
        self.open_settings();
    }

    fn render_settings(&mut self) {
        let mut kinds = vec![SettingKind::Summon];
        kinds.extend((0..ACTIONS.len()).map(SettingKind::Shortcut));
        kinds.extend([SettingKind::Autostart, SettingKind::HideInactive, SettingKind::Theme, SettingKind::Reset, SettingKind::Folder]);
        let error_for = |k: SettingKind| match &self.setting_error {
            Some((kind, e)) if *kind == k => e.clone(),
            _ => String::new(),
        };
        let rows: Vec<SettingRow> = kinds
            .iter()
            .map(|&k| {
                let (kind, label, value, on, note): (i32, String, String, bool, String) = match k {
                    SettingKind::Summon => (
                        0,
                        "Open Wisp from anywhere".into(),
                        self.config.summon_combo().label(),
                        false,
                        self.summon_error.clone().unwrap_or_else(|| {
                            if on_wayland() {
                                "On Wayland, also bind “wisp --toggle” in system settings".into()
                            } else {
                                "Works in every app".into()
                            }
                        }),
                    ),
                    SettingKind::Shortcut(i) => {
                        let info = &ACTIONS[i];
                        (0, info.label.into(), self.config.combo(info.action).label(), false, String::new())
                    }
                    SettingKind::Autostart => (1, "Start with the computer".into(), String::new(), self.config.launch_at_startup, "Waits in the tray".into()),
                    SettingKind::HideInactive => (1, "Hide when I click elsewhere".into(), String::new(), self.config.hide_when_inactive, String::new()),
                    SettingKind::Theme => {
                        let name = match self.config.theme {
                            Theme::System => "Match system",
                            Theme::Light => "Light",
                            Theme::Dark => "Dark",
                        };
                        (2, "Appearance".into(), name.into(), false, String::new())
                    }
                    SettingKind::Reset => (3, "Reset all shortcuts".into(), "Reset".into(), false, String::new()),
                    SettingKind::Folder => (4, "Notes folder".into(), "Open".into(), false, self.data_dir.display().to_string()),
                };
                let error = error_for(k);
                let summon_problem = k == SettingKind::Summon && self.summon_error.is_some();
                SettingRow {
                    kind,
                    label: label.into(),
                    value: value.into(),
                    on,
                    note: if summon_problem { String::new().into() } else { note.into() },
                    error: if error.is_empty() && summon_problem { self.summon_error.clone().unwrap_or_default().into() } else { error.into() },
                }
            })
            .collect();
        self.settings = kinds;
        let ui = self.ui();
        ui.set_settings(ModelRc::new(VecModel::from(rows)));
        ui.set_setting_index(self.setting_index as i32);
        ui.set_recording(self.recording);
    }

    fn key_settings(&mut self, combo: &Combo, action: Option<Action>) -> bool {
        let bare = !combo.is_modified() && !combo.shift;
        let last = self.settings.len().saturating_sub(1);
        let current = self.settings.get(self.setting_index).copied();
        match combo.key.as_str() {
            "Escape" if bare => self.show_list(self.listing),
            "Down" if bare => self.move_setting(1),
            "Up" if bare => self.move_setting(-1),
            "Tab" if !combo.is_modified() => self.move_setting(if combo.shift { -1 } else { 1 }),
            "Home" if bare => self.move_setting(-(last as i32)),
            "End" if bare => self.move_setting(last as i32),
            "Enter" | "Space" if bare => self.activate_setting(self.setting_index),
            "Left" | "Right" if bare => {
                let forward = combo.key == "Right";
                match current {
                    Some(SettingKind::Theme) => self.cycle_theme(if forward { 1 } else { -1 }),
                    Some(SettingKind::Autostart) => self.set_autostart(forward),
                    Some(SettingKind::HideInactive) => {
                        self.config.hide_when_inactive = forward;
                        self.save_config();
                        self.render_settings();
                    }
                    _ => {}
                }
            }
            "Backspace" | "Delete" if bare => self.reset_setting(),
            _ => match action {
                Some(Action::Undo) | None => return action.is_some(),
                Some(a) => self.global_action(a),
            },
        }
        true
    }

    fn move_setting(&mut self, delta: i32) {
        let last = self.settings.len() as i32 - 1;
        self.setting_index = (self.setting_index as i32 + delta).clamp(0, last.max(0)) as usize;
        self.setting_error = None;
        self.render_settings();
    }

    pub fn setting_clicked(&mut self, index: i32) {
        let Ok(index) = usize::try_from(index) else { return };
        self.stop_recording();
        self.setting_index = index;
        self.activate_setting(index);
    }

    fn activate_setting(&mut self, index: usize) {
        self.setting_error = None;
        match self.settings.get(index).copied() {
            Some(SettingKind::Summon) => {
                // Release the current key so pressing it again can be recorded.
                if let (Some(manager), Some(hk)) = (&self.hotkeys, self.summon) {
                    let _ = manager.unregister(hk);
                }
                self.recording = true;
            }
            Some(SettingKind::Shortcut(_)) => self.recording = true,
            Some(SettingKind::Autostart) => self.set_autostart(!self.config.launch_at_startup),
            Some(SettingKind::HideInactive) => {
                self.config.hide_when_inactive = !self.config.hide_when_inactive;
                self.save_config();
            }
            Some(SettingKind::Theme) => self.cycle_theme(1),
            Some(SettingKind::Reset) => {
                self.config.shortcuts.clear();
                let default: Combo = config::DEFAULT_SUMMON.parse().expect("default parses");
                if let Err(err) = self.register_summon(&default) {
                    self.setting_error = Some((SettingKind::Reset, err));
                } else {
                    self.config.summon = default.to_string();
                }
                self.save_config();
                self.apply_config_to_ui();
                self.toast("Shortcuts are back to their defaults.", false);
            }
            Some(SettingKind::Folder) => platform::reveal(&self.data_dir),
            None => {}
        }
        self.render_settings();
    }

    fn reset_setting(&mut self) {
        match self.settings.get(self.setting_index).copied() {
            Some(SettingKind::Shortcut(i)) => {
                let info = &ACTIONS[i];
                let combo: Combo = info.default.parse().expect("defaults parse");
                if let Some(other) = self.config.conflict(&combo, info.id) {
                    self.setting_error = Some((SettingKind::Shortcut(i), format!("{} is taken by “{other}”", combo.label())));
                } else {
                    self.config.set_shortcut(info.id, &combo);
                    self.save_config();
                    self.apply_config_to_ui();
                }
            }
            Some(SettingKind::Summon) => {
                let combo: Combo = config::DEFAULT_SUMMON.parse().expect("default parses");
                self.apply_summon(combo);
            }
            _ => {}
        }
        self.render_settings();
    }

    fn stop_recording(&mut self) {
        if !self.recording {
            return;
        }
        self.recording = false;
        // Re-claim the summon key if recording had released it.
        if let (Some(manager), Some(hk)) = (&self.hotkeys, self.summon) {
            let _ = manager.register(hk);
        }
        if self.ui().get_view() == View::Settings {
            self.render_settings();
        }
    }

    /// Handles the key pressed while a shortcut row is listening.
    fn record(&mut self, combo: Combo) {
        let Some(kind) = self.settings.get(self.setting_index).copied() else { return };
        if combo.key == "Escape" && !combo.is_modified() && !combo.shift {
            self.setting_error = None;
            self.stop_recording();
            return;
        }
        let id = match kind {
            SettingKind::Summon => "summon",
            SettingKind::Shortcut(i) => ACTIONS[i].id,
            _ => return,
        };
        let valid = if kind == SettingKind::Summon { combo.validate_global() } else { combo.validate_local() };
        let problem = valid.err().or_else(|| self.config.conflict(&combo, id).map(|other| format!("{} is taken by “{other}”", combo.label())));
        if let Some(problem) = problem {
            self.setting_error = Some((kind, problem));
            self.render_settings(); // keep listening
            return;
        }
        self.recording = false;
        self.setting_error = None;
        match kind {
            SettingKind::Summon => self.apply_summon(combo),
            _ => {
                self.config.set_shortcut(id, &combo);
                self.save_config();
                self.apply_config_to_ui();
                self.toast(format!("Saved: {}", combo.label()), false);
            }
        }
        self.render_settings();
    }

    fn apply_summon(&mut self, combo: Combo) {
        let previous = self.summon;
        match self.register_summon(&combo) {
            Ok(()) => {
                self.config.summon = combo.to_string();
                self.summon_error = None;
                self.save_config();
                self.apply_config_to_ui();
                self.toast(format!("{} now opens Wisp from anywhere.", combo.label()), false);
            }
            Err(err) => {
                self.setting_error = Some((SettingKind::Summon, err));
                if let (Some(manager), Some(hk)) = (&self.hotkeys, previous) {
                    if manager.register(hk).is_ok() {
                        self.summon = Some(hk);
                    }
                }
            }
        }
    }

    fn register_summon(&mut self, combo: &Combo) -> Result<(), String> {
        let manager = self.hotkeys.as_ref().ok_or_else(|| "System-wide keys aren't available here.".to_owned())?;
        let hotkey = combo.to_hotkey()?;
        if let Some(old) = self.summon.take() {
            let _ = manager.unregister(old);
        }
        manager
            .register(hotkey)
            .map_err(|e| format!("Couldn't claim {} ({e}). Another app may use it.", combo.label()))?;
        self.summon = Some(hotkey);
        SUMMON_ID.store(hotkey.id(), Ordering::SeqCst);
        Ok(())
    }

    fn set_autostart(&mut self, enabled: bool) {
        match platform::set_autostart(enabled) {
            Ok(()) => {
                self.config.launch_at_startup = enabled;
                self.save_config();
            }
            Err(err) => self.setting_error = Some((SettingKind::Autostart, format!("Couldn't change it: {err}"))),
        }
        self.render_settings();
    }

    fn cycle_theme(&mut self, delta: i32) {
        let order = [Theme::System, Theme::Light, Theme::Dark];
        let index = order.iter().position(|&t| t == self.config.theme).unwrap_or(0) as i32;
        self.config.theme = order[(index + delta).rem_euclid(3) as usize];
        self.save_config();
        self.apply_config_to_ui();
        self.render_settings();
    }

    /// Pushes shortcut labels and the theme into the UI.
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
            quit: l(Action::Quit),
        });
        ui.global::<crate::Theme>().set_choice(match self.config.theme {
            Theme::System => 0,
            Theme::Light => 1,
            Theme::Dark => 2,
        });
        if let Some(tray) = &self.tray {
            tray.set_summon_label(self.config.summon_combo().label().into());
        }
        ui.set_help(ModelRc::new(VecModel::from(self.help_rows())));
    }

    fn open_help(&mut self) {
        let ui = self.ui();
        ui.set_help(ModelRc::new(VecModel::from(self.help_rows())));
        ui.set_view(View::Help);
        ui.invoke_focus_help();
    }

    fn help_rows(&self) -> Vec<HelpRow> {
        let l = |a| self.label(a);
        let groups: Vec<(&str, Vec<(String, &str)>)> = vec![
            ("Anywhere", vec![(self.config.summon_combo().label(), "Open or hide Wisp")]),
            (
                "Writing",
                vec![
                    ("Enter".into(), "Save the note"),
                    ("Shift+Enter".into(), "New line"),
                    (l(Action::Schedule), "Save, then add dates and reminders"),
                    ("Esc".into(), "Hide Wisp (what you typed stays)"),
                ],
            ),
            (
                "Your notes",
                vec![
                    ("↓ ↑".into(), "Move through notes"),
                    ("Enter".into(), "Open the selected note"),
                    (l(Action::ToggleDone), "Mark done (or bring back from the archive)"),
                    (l(Action::Delete), "Delete"),
                    (l(Action::Undo), "Undo the last change"),
                    (l(Action::Inbox), "Open notes"),
                    (l(Action::Archive), "Done notes"),
                    ("Esc".into(), "Back to typing"),
                ],
            ),
            (
                "Editing a note",
                vec![
                    ("Tab".into(), "Next field"),
                    ("Esc".into(), "Save and close"),
                    (l(Action::AddReminder), "Add a reminder"),
                    ("← →".into(), "Change the reminder type"),
                    ("Delete".into(), "Remove the reminder"),
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
            ("Wisp", vec![(l(Action::Settings), "Settings and shortcuts"), (l(Action::Help), "This list"), (l(Action::Quit), "Quit")]),
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

fn reminder_row(kind: usize, amount: &str, fresh: bool) -> ReminderRow {
    ReminderRow {
        kind_label: RULE_KINDS[kind.min(RULE_KINDS.len() - 1)].into(),
        amount: amount.into(),
        has_amount: matches!(kind, 1 | 2 | 3 | 5),
        placeholder: if kind == 5 { "start time".into() } else { "15m".into() },
        fresh,
    }
}
