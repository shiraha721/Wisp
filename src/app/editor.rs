//! The note editor: title, notes, start and due times, reminders.
//!
//! Closing never traps the user. Reminders that have no start or due time to
//! follow are dropped with a note saying so. Unreadable dates or amounts stop
//! the first Esc with an explanation; a second Esc saves what is valid and keeps
//! the previous value of anything that isn't.

use super::{debug_log, later, App};
use crate::config::Action;
use crate::dates::{self, Age};
use crate::i18n;
use crate::model::{self, Item, Reminder, Rule, RULE_KINDS};
use crate::shortcuts::Combo;
use crate::{Hint, ReminderRow, View};
use chrono::{Local, TimeZone};
use slint::{Model, ModelRc, SharedString, VecModel};

const START_DEFAULT_TIME: (u32, u32) = (9, 0);
const DUE_DEFAULT_TIME: (u32, u32) = (18, 0);

/// Fields of the editor, in focus order.
pub(super) const FIELD_TITLE: i32 = 0;
pub(super) const FIELD_NOTES: i32 = 1;
pub(super) const FIELD_START: i32 = 2;
pub(super) const FIELD_DUE: i32 = 3;
pub(super) const FIELD_ADD_REMINDER: i32 = 4;

pub(super) struct EditSession {
    id: u64,
    before: Item,
    /// The kind of each reminder row, in order (see `RULE_KINDS`).
    kinds: Vec<usize>,
    return_to: View,
    auto_reminder_done: bool,
    /// The last close attempt stopped on a problem; closing again saves what it can.
    escape_armed: bool,
    category: Option<u64>,
}

impl App {
    pub(super) fn open_editor(&mut self, id: u64, field: i32) {
        let Some(item) = self.store.get_mut(id) else { return };
        if item.ringing {
            item.ringing = false;
            self.persist();
        }
        let Some(item) = self.store.get(id).cloned() else { return };
        let ui = self.ui();
        let now = Self::now();
        let canonical = |t: i64| Local.timestamp_opt(t, 0).single().map(|t| dates::canonical(&t)).unwrap_or_default();

        ui.set_edit_title(item.text.as_str().into());
        ui.set_edit_notes(item.notes.as_str().into());
        ui.set_edit_start(item.start.map(canonical).unwrap_or_default().into());
        ui.set_edit_due(item.due.map(canonical).unwrap_or_default().into());
        let written = Local.timestamp_opt(item.created, 0).single().unwrap_or(now);
        let meta = match dates::age(&written, &now) {
            Age::JustNow => tr!("Written just now").to_owned(),
            Age::Minutes(m) => tr!("Written {} min ago", m),
            Age::Hours(h) => tr!("Written {} h ago", h),
            Age::Yesterday => tr!("Written yesterday").to_owned(),
            Age::On(day) => tr!("Written on {}", day),
        };
        ui.set_edit_meta(meta.into());
        ui.set_edit_category(self.category_index(item.category));
        ui.set_back_label(if self.listing == View::Archive { tr!("Done") } else { tr!("Notes") }.into());

        let kinds: Vec<usize> = item.reminders.iter().map(|r| r.rule.kind()).collect();
        let rows: Vec<ReminderRow> = item.reminders.iter().map(|r| reminder_row(r.rule.kind(), &r.rule.amount(), false)).collect();
        self.reminders.set_vec(rows);
        ui.set_focused_reminder(-1);

        self.editor = Some(EditSession {
            id,
            category: item.category,
            before: item,
            kinds,
            return_to: self.listing,
            auto_reminder_done: false,
            escape_armed: false,
        });
        self.update_editor();
        ui.set_view(View::Editor);
        ui.invoke_focus_editor(field);
        let weak = self.ui.clone();
        slint::Timer::single_shot(std::time::Duration::from_millis(30), move || {
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

    /// The user changed something: a pending "Esc again" no longer applies.
    pub fn editor_edited(&mut self) {
        if let Some(session) = self.editor.as_mut() {
            session.escape_armed = false;
        }
        self.update_editor();
    }

    /// Re-reads the fields and refreshes every hint under them.
    pub(super) fn update_editor(&mut self) {
        let ui = self.ui();
        let now = Self::now();
        let now_ts = now.timestamp();
        let (start, due) = self.parse_fields();
        let at = |t: i64| Local.timestamp_opt(t, 0).single().unwrap_or(now);
        let describe = |t: i64| format!("{} ({})", dates::full_label(&at(t)), dates::relative(t - now_ts));
        let hint = |r: &Result<Option<i64>, String>| match r {
            Ok(Some(t)) => Hint { text: describe(*t).into(), error: false },
            Ok(None) => Hint::default(),
            Err(e) => Hint { text: e.as_str().into(), error: true },
        };
        ui.set_start_hint(hint(&start));
        let mut due_hint = hint(&due);
        if let (Ok(Some(s)), Ok(Some(d))) = (&start, &due) {
            if d < s {
                due_hint = Hint { text: tr!("Due is before the start").into(), error: true };
            }
        }
        ui.set_due_hint(due_hint);

        let start = start.ok().flatten();
        let due = due.ok().flatten();
        ui.set_can_add_reminder(start.is_some() || due.is_some());
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
                    Ok(rule) if rule.needs_start() && start.is_none() => {
                        Hint { text: tr!("Needs a start time, or it's dropped").into(), error: true }
                    }
                    Ok(rule) if !rule.needs_start() && due.is_none() => {
                        Hint { text: tr!("Needs a due time, or it's dropped").into(), error: true }
                    }
                    Ok(rule) => match rule.next_after(&Local, start, due, now_ts) {
                        Some(t) => Hint { text: dates::day_label(&at(t), &now).into(), error: false },
                        None => Hint { text: tr!("Already passed").into(), error: false },
                    },
                }
            })
            .collect();
        ui.set_reminder_hints(ModelRc::new(VecModel::from(hints)));
    }

    pub fn reminder_cycle(&mut self, index: i32, delta: i32) {
        let Some(session) = self.editor.as_mut() else { return };
        let Some(kind) = usize::try_from(index).ok().and_then(|i| session.kinds.get_mut(i)) else { return };
        session.escape_armed = false;
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

    /// The picker chose category `index` (-1: none).
    pub fn category_picked(&mut self, index: i32) {
        let id = usize::try_from(index).ok().and_then(|i| self.store.categories().get(i)).map(|c| c.id);
        self.set_editor_category(id);
    }

    pub fn category_cycled(&mut self, step: i32) {
        let Some(current) = self.editor.as_ref().map(|e| e.category) else { return };
        if self.no_categories_yet() {
            return;
        }
        self.set_editor_category(super::step_category(self.store.categories(), current, step));
    }

    fn set_editor_category(&mut self, id: Option<u64>) {
        let Some(session) = self.editor.as_mut() else { return };
        session.category = id;
        session.escape_armed = false;
        self.ui().set_edit_category(self.category_index(id));
    }

    /// Typing a letter on the picker jumps to the next category that starts with it.
    fn category_by_letter(&mut self, letter: &str) {
        let Some(current) = self.editor.as_ref().map(|e| e.category) else { return };
        let categories = self.store.categories();
        let start = current.and_then(|id| categories.iter().position(|c| c.id == id)).map_or(0, |i| i + 1);
        let letter = letter.to_lowercase();
        let found = (0..categories.len())
            .map(|k| &categories[(start + k) % categories.len()])
            .find(|c| c.name.to_lowercase().starts_with(&letter))
            .map(|c| c.id);
        if found.is_some() {
            self.set_editor_category(found);
        }
    }

    /// Removes reminder `index` and moves the keyboard to its neighbour.
    pub fn reminder_remove(&mut self, index: i32) {
        let Some(session) = self.editor.as_mut() else { return };
        let Ok(index) = usize::try_from(index) else { return };
        if index >= session.kinds.len() {
            return;
        }
        session.escape_armed = false;
        session.kinds.remove(index);
        let remaining = session.kinds.len();
        // Rebuilding the rows lets the one taking this place grab focus as it appears.
        let target = (remaining > 0).then(|| index.min(remaining - 1));
        let rows: Vec<ReminderRow> = (0..self.reminders.row_count())
            .filter(|&i| i != index)
            .filter_map(|i| self.reminders.row_data(i))
            .enumerate()
            .map(|(i, mut row)| {
                row.fresh = Some(i) == target;
                row
            })
            .collect();
        self.reminders.set_vec(rows);
        self.ui().set_focused_reminder(-1);
        self.update_editor();
        self.toast(tr!("Reminder removed."), false);
        if target.is_none() {
            later(|app| app.ui().invoke_focus_editor(FIELD_ADD_REMINDER));
        }
    }

    /// Removes the reminder the keyboard is on, or else the last one.
    fn remove_reminder_here(&mut self, context: &str) {
        let index = reminder_index(context).or_else(|| self.reminders.row_count().checked_sub(1));
        match index {
            Some(i) => self.reminder_remove(i as i32),
            None => self.toast(tr!("This note has no reminders."), false),
        }
    }

    pub fn add_reminder(&mut self) {
        let (start, due) = self.parse_fields();
        let has_start = matches!(start, Ok(Some(_)));
        let has_due = matches!(due, Ok(Some(_)));
        if !has_start && !has_due {
            self.toast(tr!("Reminders ring relative to the start or due time. Add one of those first."), false);
            self.ui().invoke_focus_editor(FIELD_START);
            return;
        }
        let Some(session) = self.editor.as_mut() else { return };
        let rule = if has_start && !session.kinds.contains(&Rule::AtStart.kind()) {
            Rule::AtStart
        } else if has_start {
            Rule::BeforeStart { minutes: 15 }
        } else {
            Rule::BeforeDue { minutes: 60 }
        };
        session.kinds.push(rule.kind());
        session.auto_reminder_done = true;
        session.escape_armed = false;
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

    /// Writes the editor back to the store.
    ///
    /// `Ok(Some(message))` reports something that was left out (dropped reminders,
    /// kept dates). With `force`, problems never stop the save.
    pub(super) fn save_editor(&mut self, force: bool) -> Result<Option<String>, String> {
        let Some(session) = self.editor.as_ref() else { return Ok(None) };
        let ui = self.ui();
        let now = Self::now().timestamp();
        let mut left_out = Vec::new();

        let (start, due) = self.parse_fields();
        let start = match start {
            Ok(t) => t,
            Err(_) if force => {
                left_out.push(tr!("The old start time was kept.").to_owned());
                session.before.start
            }
            Err(e) => {
                ui.invoke_focus_editor(FIELD_START);
                return Err(tr!("Starts: {} Fix it, or press Esc again to keep the old time.", e));
            }
        };
        let due = match due {
            Ok(t) => t,
            Err(_) if force => {
                left_out.push(tr!("The old due time was kept.").to_owned());
                session.before.due
            }
            Err(e) => {
                ui.invoke_focus_editor(FIELD_DUE);
                return Err(tr!("Due: {} Fix it, or press Esc again to keep the old time.", e));
            }
        };
        if let (Some(s), Some(d)) = (start, due) {
            if d < s && !force {
                ui.invoke_focus_editor(FIELD_DUE);
                return Err(tr!("The due time is before the start. Fix it, or press Esc again to save it anyway.").into());
            }
        }

        let mut reminders: Vec<Reminder> = Vec::new();
        let mut dropped = 0;
        for (i, &kind) in session.kinds.iter().enumerate() {
            let amount = self.reminders.row_data(i).map(|r| r.amount).unwrap_or_default();
            let rule = match Rule::from_parts(kind, &amount) {
                Ok(rule) => rule,
                Err(_) if force => {
                    dropped += 1;
                    continue;
                }
                Err(e) => return Err(tr!("Reminder: {} Fix it, or press Esc again to drop it.", e)),
            };
            // A reminder with nothing to follow can't ring; leave it out rather than nag.
            let anchored = if rule.needs_start() { start.is_some() } else { due.is_some() };
            if !anchored {
                dropped += 1;
                continue;
            }
            if !reminders.iter().any(|r| r.rule == rule) {
                reminders.push(Reminder::new(rule));
            }
        }

        let id = session.id;
        let before = session.before.clone();
        let category = session.category;
        let mut title = ui.get_edit_title().trim().to_owned();
        let mut notes = ui.get_edit_notes().trim().replace("\r\n", "\n");
        self.editor = None;
        if title.is_empty() && notes.is_empty() {
            self.delete(id);
            return Ok(None);
        }
        if title.is_empty() {
            // The first line of the notes becomes the title.
            (title, notes) = model::split_title(&notes);
        }
        let Some(mut item) = self.store.get(id).cloned() else { return Ok(None) };
        item.text = title;
        item.notes = notes;
        item.category = category;
        item.start = start;
        item.due = due;
        let previous = std::mem::take(&mut item.reminders);
        item.reminders = reminders;
        item.arm_reminders(&Local, now);
        // Keep the firing history of rules that didn't change.
        if before.start == start && before.due == due {
            for r in &mut item.reminders {
                if let Some(old) = previous.iter().find(|p| p.rule == r.rule) {
                    r.last_fired = old.last_fired.max(r.last_fired);
                }
            }
        }
        let changed = item.text != before.text
            || item.notes != before.notes
            || item.category != before.category
            || item.start != before.start
            || item.due != before.due
            || item.reminders.iter().map(|r| r.rule).ne(before.reminders.iter().map(|r| r.rule));
        if changed {
            item.updated = now;
            self.remember(super::UndoStep::Changed(before));
            self.store.replace(item);
            self.persist();
        }
        self.selected_id = Some(id);

        if dropped > 0 {
            left_out.push(trn!(
                "A reminder with no start or due time to follow was left out.",
                "{} reminders with no start or due time to follow were left out.",
                dropped
            ));
        }
        Ok((!left_out.is_empty()).then(|| format!("{} {}", tr!("Saved."), left_out.join(" "))))
    }

    /// Saves and returns to the list. Returns false (and says why) if something
    /// needs fixing; a second attempt with `force` always succeeds.
    pub(super) fn close_editor(&mut self, force: bool) -> bool {
        let Some(view) = self.editor.as_ref().map(|e| e.return_to) else { return true };
        match self.save_editor(force) {
            Ok(message) => {
                let keep = self.selected_id;
                self.show_list(view);
                self.selected_id = keep;
                self.refresh();
                if let Some(message) = message {
                    self.toast_for(message, false, std::time::Duration::from_secs(6));
                }
                true
            }
            Err(err) => {
                debug_log!("editor stays open: {err}");
                if let Some(session) = self.editor.as_mut() {
                    session.escape_armed = true;
                }
                self.toast_for(err, true, std::time::Duration::from_secs(10));
                false
            }
        }
    }

    pub(super) fn key_editor(&mut self, combo: &Combo, action: Option<Action>, context: &str) -> bool {
        let bare = !combo.is_modified() && !combo.shift;
        let on_kind = context.starts_with("edit-reminder-kind:");
        if context == "edit-category" && bare {
            match combo.key.as_str() {
                "Left" | "Right" | "Space" => {
                    self.category_cycled(if combo.key == "Left" { -1 } else { 1 });
                    return true;
                }
                "Delete" | "Backspace" => {
                    self.set_editor_category(None);
                    return true;
                }
                key if key.chars().count() == 1 && key.chars().all(char::is_alphanumeric) => {
                    self.category_by_letter(key);
                    return true;
                }
                _ => {}
            }
        }
        match combo.key.as_str() {
            "Escape" if bare => {
                let force = self.editor.as_ref().is_some_and(|e| e.escape_armed);
                self.close_editor(force);
                return true;
            }
            "Enter" if !combo.is_modified() => match context {
                "edit-title" => {
                    self.ui().invoke_focus_editor(FIELD_NOTES);
                    return true;
                }
                "edit-notes" => return false, // a new line
                "edit-add" => {
                    self.add_reminder();
                    return true;
                }
                _ if !combo.shift => {
                    self.close_editor(false);
                    return true;
                }
                _ => {}
            },
            "Left" | "Right" | "Space" if bare && on_kind => {
                if let Some(i) = reminder_index(context) {
                    self.reminder_cycle(i as i32, if combo.key == "Left" { -1 } else { 1 });
                }
                return true;
            }
            "Delete" | "Backspace" if bare && on_kind => {
                self.remove_reminder_here(context);
                return true;
            }
            _ => {}
        }
        let Some(action) = action else { return false };
        match action {
            Action::Schedule | Action::Open => {
                self.close_editor(false);
            }
            Action::AddReminder => self.add_reminder(),
            Action::RemoveReminder => self.remove_reminder_here(context),
            Action::Category => self.category_cycled(self.category_step(combo)),
            Action::ToggleDone => {
                if let Some(id) = self.editor.as_ref().map(|e| e.id) {
                    if self.close_editor(false) {
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
            Action::Undo => return false, // undo typing in the field
            other => {
                if self.close_editor(false) {
                    self.global_action(other);
                }
            }
        }
        true
    }
}

/// The reminder row named in a focus context such as "edit-reminder-kind:2".
fn reminder_index(context: &str) -> Option<usize> {
    context
        .strip_prefix("edit-reminder-kind:")
        .or_else(|| context.strip_prefix("edit-reminder-amount:"))
        .and_then(|i| i.parse().ok())
}

fn reminder_row(kind: usize, amount: &str, fresh: bool) -> ReminderRow {
    ReminderRow {
        kind_label: i18n::tr(RULE_KINDS[kind.min(RULE_KINDS.len() - 1)]).into(),
        amount: amount.into(),
        has_amount: matches!(kind, 1 | 2 | 3 | 5),
        placeholder: if kind == 5 { tr!("start time").into() } else { "15m".into() },
        fresh,
    }
}
