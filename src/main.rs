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
Wisp — catch ideas, memos and to-dos.

Usage: wisp [--hidden | --toggle | --quit]

  (no option)  open Wisp, starting it if needed
  --hidden     start in the tray without opening the window
  --toggle     open or hide Wisp (bind this to a key on Wayland desktops)
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
    } else if has("--hidden") {
        "none"
    } else {
        "show"
    };

    let (data_dir, portable) = platform::data_dir();
    let instance = platform::claim_instance(&data_dir, command);
    if matches!(instance, platform::Instance::Forwarded) || command == "quit" {
        return; // the running Wisp took the command (or there's nothing to quit)
    }

    if let Err(err) = select_backend() {
        eprintln!("wisp: {err}");
    }

    let ui = match AppWindow::new() {
        Ok(ui) => ui,
        Err(err) => {
            eprintln!("wisp: couldn't create the window: {err}");
            std::process::exit(1);
        }
    };
    let icon = icon::app_icon(64);
    ui.set_app_icon(icon.clone());

    app::install(app::App::new(&ui, data_dir, portable));
    wire_window(&ui);

    let tray = Tray::new().ok();
    if let Some(tray) = &tray {
        tray.set_app_icon(icon);
        tray.on_open_app(|| {
            with_app(|app| app.summon());
        });
        tray.on_open_settings(|| {
            with_app(|app| app.open_settings_from_tray());
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
        slint::Timer::single_shot(Duration::ZERO, || {
            with_app(|app| app.summon());
        });
    }

    if let Err(err) = slint::run_event_loop_until_quit() {
        eprintln!("wisp: {err}");
    }
}

fn wire_window(ui: &AppWindow) {
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
    ui.on_badge_clicked(|| {
        with_app(|app| app.badge_clicked());
    });
    ui.on_editor_edited(|| {
        with_app(|app| app.update_editor());
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
    ui.on_setting_clicked(|i| {
        with_app(|app| app.setting_clicked(i));
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

/// Launcher-style window tweaks: no taskbar button on Windows, no Dock icon on macOS.
fn select_backend() -> Result<(), slint::PlatformError> {
    let selector = slint::BackendSelector::new();
    #[cfg(windows)]
    let selector = selector.with_winit_window_attributes_hook(|attributes| {
        use winit::platform::windows::WindowAttributesExtWindows;
        attributes.with_skip_taskbar(true)
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
