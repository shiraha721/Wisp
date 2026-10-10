//! The window of the first start: the language, and the key that opens Wisp.
//! Both take effect as soon as they're chosen; "Start using Wisp", Esc or
//! closing the window puts the launcher up.
//!
//! Keyboard: the arrows choose a language, Enter or Tab moves on and Shift+Tab
//! goes back. On the hotkey card, any combination becomes the new hotkey (if it
//! can be one) and Backspace restores the default.

use super::{first_categories, later, with_app, App};
use crate::i18n::{self, LANGUAGES};
use crate::{LanguageChoice, WelcomeWindow};
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::time::Duration;

/// Where the keyboard is.
const LANGUAGE: i32 = 0;
const HOTKEY: i32 = 1;
const START: i32 = 2;
/// The language tiles are laid out in rows of this many.
const COLUMNS: usize = 3;

pub(super) struct Welcome {
    window: WelcomeWindow,
    zone: i32,
    /// What the hotkey card says instead of its usual line, and whether it's an error.
    message: Option<(String, bool)>,
}

impl Welcome {
    pub(super) fn set_theme(&self, choice: i32) {
        self.window.global::<crate::Theme>().set_choice(choice);
    }
}

impl App {
    pub fn open_welcome(&mut self) {
        if self.welcome.is_none() {
            let window = match WelcomeWindow::new() {
                Ok(window) => window,
                Err(err) => {
                    eprintln!("wisp: couldn't create the welcome window: {err}");
                    self.first_run = false;
                    self.summon();
                    return;
                }
            };
            window.set_logo(crate::icon::image(128));
            window.on_key(|text, ctrl, alt, shift, meta| with_app(|app| app.welcome_key(&text, ctrl, alt, shift, meta)).unwrap_or(false));
            window.on_language_clicked(|i| {
                with_app(|app| app.welcome_language(i as usize));
            });
            window.on_keys_clicked(|| {
                with_app(|app| app.welcome_zone(HOTKEY));
            });
            window.on_start_clicked(|| later(|app| app.finish_welcome()));
            window.window().on_close_requested(|| {
                later(|app| app.finish_welcome());
                slint::CloseRequestResponse::HideWindow
            });
            self.welcome = Some(Welcome { window, zone: LANGUAGE, message: None });
            self.apply_config_to_ui();
        }
        self.render_welcome();
        let Some(welcome) = &self.welcome else { return };
        let window = welcome.window.clone_strong();
        if let Err(err) = window.show() {
            eprintln!("wisp: couldn't show the welcome window: {err}");
            self.finish_welcome();
            return;
        }
        window.window().with_winit_window(|w| w.focus_window());
        window.invoke_take_focus();
        // Some window managers only honour focus requests once the window is mapped.
        slint::Timer::single_shot(Duration::from_millis(90), move || {
            window.window().with_winit_window(|w| {
                if !w.has_focus() {
                    w.focus_window();
                }
            });
            window.invoke_take_focus();
        });
    }

    /// The summon key was pressed (or the tray clicked) while the welcome window is up.
    pub(super) fn welcome_summoned(&mut self) {
        let focused = self.welcome.as_ref().is_some_and(|w| w.window.window().with_winit_window(|w| w.has_focus()).unwrap_or(false));
        let keys = self.summon_keys().label();
        if let Some(welcome) = self.welcome.as_mut().filter(|w| focused && w.zone == HOTKEY) {
            // Pressed on the hotkey card: the system hands it to Wisp's hotkey, not the window.
            welcome.message = Some((tr!("{} opens Wisp already.", keys), false));
        }
        self.open_welcome();
    }

    /// Puts the launcher up, ready for the first note.
    pub(super) fn finish_welcome(&mut self) {
        if self.welcome.is_none() {
            return;
        }
        self.close_welcome();
        self.summon();
    }

    pub(super) fn close_welcome(&mut self) {
        self.first_run = false;
        if let Some(welcome) = self.welcome.take() {
            let _ = welcome.window.hide();
        }
    }

    fn render_welcome(&mut self) {
        let note = self.summon_note();
        let keys = self.summon_keys();
        let Some(welcome) = &self.welcome else { return };
        let system = i18n::system_language();
        let languages: Vec<LanguageChoice> = LANGUAGES
            .iter()
            .enumerate()
            .map(|(i, language)| {
                let detail = i18n::tr(language.english);
                LanguageChoice {
                    name: language.name.into(),
                    detail: if detail == language.name { SharedString::new() } else { detail.into() },
                    system: system == Some(i),
                }
            })
            .collect();
        let (message, error) = match &welcome.message {
            Some((message, error)) => (message.clone(), *error),
            None if self.summon_problem.is_some() => (note, self.summon.is_none()),
            None if welcome.zone == HOTKEY => (tr!("Press other keys to use them instead.").to_owned(), false),
            None => (note, false),
        };
        let w = &welcome.window;
        w.set_languages(ModelRc::new(VecModel::from(languages)));
        w.set_chosen(i18n::resolve(&self.config.language) as i32);
        w.set_zone(welcome.zone);
        w.set_keys(ModelRc::new(VecModel::from(keys.parts().into_iter().map(SharedString::from).collect::<Vec<_>>())));
        w.set_keys_message(message.into());
        w.set_keys_error(error);
    }

    fn welcome_zone(&mut self, zone: i32) {
        if let Some(welcome) = &mut self.welcome {
            welcome.zone = zone.clamp(LANGUAGE, START);
            welcome.message = None;
        }
        self.render_welcome();
    }

    /// Shows Wisp in `LANGUAGES[index]`, along with the first categories if
    /// they're still as they were made.
    fn welcome_language(&mut self, index: usize) {
        if index >= LANGUAGES.len() {
            return;
        }
        let before = first_categories();
        self.set_language(Some(index));
        if self.store.rename_seeded(&before, &first_categories()) {
            self.persist();
            self.push_categories();
            self.refresh();
        }
        if let Some(welcome) = &mut self.welcome {
            welcome.zone = LANGUAGE;
            welcome.message = None;
        }
        self.render_welcome();
    }

    fn welcome_key(&mut self, text: &str, ctrl: bool, alt: bool, shift: bool, meta: bool) -> bool {
        let Some(combo) = crate::shortcuts::Combo::from_event(text, ctrl, alt, shift, meta) else {
            return false;
        };
        let Some(zone) = self.welcome.as_ref().map(|w| w.zone) else { return false };
        let bare = !combo.is_modified() && !combo.shift;
        let key = combo.key.as_str();
        match key {
            "Tab" if !combo.is_modified() => self.welcome_zone(zone + if combo.shift { -1 } else { 1 }),
            "Escape" if bare => later(|app| app.finish_welcome()),
            "Enter" | "Space" if bare && zone == START => later(|app| app.finish_welcome()),
            "Enter" if bare => self.welcome_zone(zone + 1),
            "Left" | "Right" | "Up" | "Down" | "Home" | "End" if bare && zone == LANGUAGE => {
                let here = i18n::resolve(&self.config.language);
                let last = LANGUAGES.len() - 1;
                let next = match key {
                    "Left" => here.checked_sub(1),
                    "Right" => Some((here + 1).min(last)),
                    "Up" => here.checked_sub(COLUMNS),
                    "Down" if here + COLUMNS <= last => Some(here + COLUMNS),
                    "Down" => None,
                    "Home" => Some(0),
                    _ => Some(last),
                };
                match next {
                    Some(next) if next != here => self.welcome_language(next),
                    Some(_) => {}
                    // Down from the last row goes on to the hotkey.
                    None if key == "Down" => self.welcome_zone(HOTKEY),
                    None => {}
                }
            }
            "Up" | "Down" if bare => self.welcome_zone(zone + if key == "Up" { -1 } else { 1 }),
            "Left" | "Right" if bare => {}
            "Backspace" | "Delete" if bare && zone == HOTKEY => {
                self.config.summon.clear();
                self.claim_summon();
                self.save_config();
                self.apply_config_to_ui();
                let message = (self.summon_problem.is_none()).then(|| (tr!("{} now opens Wisp from anywhere.", self.summon_keys().label()), false));
                if let Some(welcome) = &mut self.welcome {
                    welcome.message = message;
                }
                self.render_welcome();
            }
            _ if zone == HOTKEY => {
                let result = match self.combo_problem(&combo, "summon") {
                    Some(problem) => Err(problem),
                    None => self.change_summon(&combo),
                };
                let message = match result {
                    Ok(()) => (tr!("{} now opens Wisp from anywhere.", combo.label()), false),
                    Err(err) => (err, true),
                };
                if let Some(welcome) = &mut self.welcome {
                    welcome.message = Some(message);
                }
                self.render_welcome();
            }
            _ => return false,
        }
        true
    }
}
