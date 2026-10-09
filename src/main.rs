// No console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod config;
mod dates;
mod icon;
mod model;
mod platform;
mod shortcuts;

use app::with_app;
use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use slint::winit_030::{winit, EventResult, WinitWindowAccessor};
use slint::ComponentHandle;
use std::sync::atomic::Ordering;
use std::time::Duration;

slint::include_modules!();

const USAGE: &str = "\
Wisp: catch ideas, memos and to-dos.

Usage: wisp [--hidden | --toggle | --settings | --quit]

  (no option)  open Wisp, starting it if needed
  --hidden     start in the tray without opening the window
  --toggle     open or hide Wisp (bind this to a key on Wayland desktops)
  --settings   open the settings window
  --quit       quit the running Wisp
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    if has("--help") || has("-h") {
        print!("{USAGE}");
        return;
    }
    let command = if has("--quit") {
        "quit"
    } else if has("--toggle") {
        "toggle"
    } else if has("--settings") {
        "settings"
    } else if has("--hidden") {
        "none"
    } else {
        "show"
    };

    let data = platform::data_dir();
    let instance = platform::claim_instance(&data.path, command);
    if matches!(instance, platform::Instance::Forwarded) || command == "quit" {
        return; // the running Wisp took the command (or there's nothing to quit)
    }

    if let Err(err) = select_backend() {
        eprintln!("wisp: {err}");
    }

    // The launcher must be created first: the window hook styles the first window.
    let ui = match AppWindow::new() {
        Ok(ui) => ui,
        Err(err) => {
            eprintln!("wisp: couldn't create the window: {err}");
            std::process::exit(1);
        }
    };
    let settings = match SettingsWindow::new() {
        Ok(settings) => settings,
        Err(err) => {
            eprintln!("wisp: couldn't create the settings window: {err}");
            std::process::exit(1);
        }
    };
    settings.set_logo(icon::image(128));
    if let Err(err) = platform::register_app_identity(&data.path) {
        eprintln!("wisp: couldn't register for notifications: {err}");
    }

    app::install(app::App::new(&ui, settings.clone_strong(), data.path.clone(), data.portable));
    if let Some(from) = &data.copied_from {
        let (from, to) = (from.display().to_string(), data.path.display().to_string());
        with_app(|app| app.announce_move(&from, &to));
    }
    wire_launcher(&ui);
    wire_settings(&settings);

    let tray = Tray::new().ok();
    if let Some(tray) = &tray {
        tray.set_app_icon(icon::image(platform::tray_icon_size()));
        tray.on_open_app(|| {
            with_app(|app| app.summon());
        });
        tray.on_open_settings(|| {
            with_app(|app| app.open_settings());
        });
        tray.on_quit_app(|| {
            with_app(|app| app.quit());
        });
    }
    with_app(|app| app.set_tray(tray));

    // The system-wide summon key arrives on global-hotkey's thread.
    GlobalHotKeyEvent::set_event_handler(Some(|event: GlobalHotKeyEvent| {
        if event.state == HotKeyState::Pressed && event.id == app::SUMMON_ID.load(Ordering::SeqCst) {
            let _ = slint::invoke_from_event_loop(|| {
                with_app(|app| app.toggle());
            });
        }
    }));

    // Later launches hand their command to this one.
    if let platform::Instance::Primary(listener) = instance {
        platform::serve_instance(listener, |command| {
            let _ = slint::invoke_from_event_loop(move || {
                with_app(|app| app.remote(&command));
            });
        });
    }

    let reminder_timer = slint::Timer::default();
    reminder_timer.start(slint::TimerMode::Repeated, Duration::from_secs(5), || {
        with_app(|app| app.tick());
    });
    slint::Timer::single_shot(Duration::from_millis(300), || {
        with_app(|app| app.tick());
    });

    if command != "none" {
        slint::Timer::single_shot(Duration::ZERO, move || {
            with_app(|app| app.remote(command));
        });
    }

    if let Err(err) = slint::run_event_loop_until_quit() {
        eprintln!("wisp: {err}");
    }
}

fn wire_launcher(ui: &AppWindow) {
    ui.on_key(|text, ctrl, alt, shift, meta, context| {
        with_app(|app| app.on_key(&text, ctrl, alt, shift, meta, &context)).unwrap_or(false)
    });
    ui.on_query_edited(|| {
        with_app(|app| app.query_edited());
    });
    ui.on_row_clicked(|i| {
        with_app(|app| app.row_clicked(i));
    });
    ui.on_row_opened(|i| {
        with_app(|app| app.row_opened(i));
    });
    ui.on_row_toggled(|i| {
        with_app(|app| app.row_toggled(i));
    });
    ui.on_tab_clicked(|i| {
        with_app(|app| app.tab_clicked(i));
    });
    ui.on_settings_clicked(|| {
        // After the click is handled, so the launcher can hide cleanly.
        app::later(|app| app.open_settings());
    });
    ui.on_back_clicked(|| {
        with_app(|app| app.back_clicked());
    });
    ui.on_editor_edited(|| {
        with_app(|app| app.editor_edited());
    });
    ui.on_reminder_cycle(|i, delta| {
        with_app(|app| app.reminder_cycle(i, delta));
    });
    ui.on_reminder_remove(|i| {
        with_app(|app| app.reminder_remove(i));
    });
    ui.on_reminder_add(|| {
        with_app(|app| app.add_reminder());
    });

    // Closing (Alt+F4, ⌘W) hides to the tray instead of quitting.
    ui.window().on_close_requested(|| {
        app::later(|app| app.dismiss());
        slint::CloseRequestResponse::KeepWindowShown
    });
    ui.window().on_winit_window_event(|_, event| {
        match event {
            winit::event::WindowEvent::Focused(true) => app::later(|app| app.focus_gained()),
            winit::event::WindowEvent::Focused(false) => {
                slint::Timer::single_shot(Duration::from_millis(150), || {
                    with_app(|app| app.focus_lost());
                });
            }
            _ => {}
        }
        EventResult::Propagate
    });
}

fn wire_settings(settings: &SettingsWindow) {
    settings.on_key(|text, ctrl, alt, shift, meta| with_app(|app| app.settings_key(&text, ctrl, alt, shift, meta)).unwrap_or(false));
    settings.on_page_clicked(|i| {
        with_app(|app| app.page_clicked(i));
    });
    settings.on_card_clicked(|i| {
        with_app(|app| app.card_clicked(i));
    });
    settings.on_theme_clicked(|mode| {
        with_app(|app| app.theme_clicked(mode));
    });
    settings.window().on_close_requested(|| {
        app::later(|app| app.settings_closed());
        slint::CloseRequestResponse::HideWindow
    });
}

/// Sets up the windowing backend. Every window gets Wisp's icon as it's created
/// (see [`style_window`]); on macOS Wisp also stays out of the Dock.
fn select_backend() -> Result<(), slint::PlatformError> {
    let launcher_done = std::cell::Cell::new(false);
    let selector = slint::BackendSelector::new().with_winit_window_attributes_hook(move |attributes| {
        // The launcher is created first.
        let launcher = !launcher_done.replace(true);
        style_window(attributes, launcher)
    });
    #[cfg(target_os = "macos")]
    let selector = {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        let mut builder = winit::event_loop::EventLoop::with_user_event();
        builder.with_activation_policy(ActivationPolicy::Accessory);
        selector.with_winit_event_loop_builder(builder)
    };
    selector.select()
}

/// Gives a window Wisp's icon. On Windows the icon comes from wisp.exe's own
/// resources (see build.rs) at exactly the sizes Windows draws, and the launcher
/// also loses its taskbar button and gains a drop shadow and rounded corners.
fn style_window(attributes: winit::window::WindowAttributes, launcher: bool) -> winit::window::WindowAttributes {
    #[cfg(windows)]
    {
        use winit::platform::windows::{CornerPreference, IconExtWindows, WindowAttributesExtWindows};
        let (small, big) = platform::window_icon_sizes();
        let icon = |size: u32| {
            winit::window::Icon::from_resource(1, Some(winit::dpi::PhysicalSize::new(size, size)))
                .ok()
                .or_else(|| png_icon(size))
        };
        let attributes = attributes.with_window_icon(icon(small)).with_taskbar_icon(icon(big));
        if !launcher {
            return attributes; // the settings window keeps the standard frame
        }
        attributes.with_skip_taskbar(true).with_undecorated_shadow(true).with_corner_preference(CornerPreference::Round)
    }
    #[cfg(not(windows))]
    {
        let _ = launcher;
        attributes.with_window_icon(png_icon(128))
    }
}

fn png_icon(size: u32) -> Option<winit::window::Icon> {
    let (rgba, width, height) = icon::rgba(size)?;
    winit::window::Icon::from_rgba(rgba, width, height).ok()
}
