//! Operating-system glue: where files live, starting with the computer,
//! notifications, and handing commands to an already running Wisp.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where notes and settings are kept.
///
/// * A folder called `wisp-data` next to the executable turns on portable mode:
///   everything stays beside the program (handy on a USB stick).
/// * `WISP_HOME` overrides the location.
/// * Otherwise the platform's data folder: `%APPDATA%\Wisp`,
///   `~/Library/Application Support/Wisp` or `~/.local/share/Wisp`.
pub fn data_dir() -> (PathBuf, bool) {
    if let Some(dir) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|d| d.join("wisp-data"))) {
        if dir.is_dir() {
            return (dir, true);
        }
    }
    if let Some(home) = std::env::var_os("WISP_HOME") {
        return (PathBuf::from(home), false);
    }
    (dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("Wisp"), false)
}

// ---------------------------------------------------------------------------
// Starting with the computer

fn auto_launch() -> Result<auto_launch::AutoLaunch, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = exe.to_str().ok_or("The program path isn't valid Unicode.")?;
    auto_launch::AutoLaunchBuilder::new()
        .set_app_name("Wisp")
        .set_app_path(exe)
        .set_args(&["--hidden"])
        .build()
        .map_err(|e| e.to_string())
}

pub fn set_autostart(enabled: bool) -> Result<(), String> {
    let launcher = auto_launch()?;
    let current = launcher.is_enabled().unwrap_or(false);
    if enabled {
        // Re-register even when already on, so a moved executable keeps working.
        if current {
            let _ = launcher.disable();
        }
        launcher.enable().map_err(|e| e.to_string())
    } else if current {
        launcher.disable().map_err(|e| e.to_string())
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Notifications

/// Shows a desktop notification without blocking the UI thread; `on_failure` runs
/// (on that thread) if the desktop has no notification service.
pub fn notify(summary: String, body: String, on_failure: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        let mut n = notify_rust::Notification::new();
        n.summary(&summary).body(&body).appname("Wisp");
        // Reminders stay on screen until dismissed where the desktop allows it.
        #[cfg(all(unix, not(target_os = "macos")))]
        n.urgency(notify_rust::Urgency::Critical).timeout(notify_rust::Timeout::Never);
        if let Err(err) = n.show() {
            eprintln!("wisp: couldn't show a notification: {err}");
            on_failure();
        }
    });
}

/// Opens a folder in the system file manager.
pub fn reveal(path: &Path) {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if let Err(err) = std::process::Command::new(program).arg(path).spawn() {
        eprintln!("wisp: couldn't open {}: {err}", path.display());
    }
}

// ---------------------------------------------------------------------------
// One Wisp per data folder
//
// The first instance listens on a loopback port derived from its data folder.
// Later launches pass their command line along ("show", "toggle", "quit") and exit,
// which also lets desktop environments without global-hotkey support (Wayland)
// bind `wisp --toggle` to a key of their own.

const MAGIC: &str = "wisp/1";

pub enum Instance {
    /// We're the first; serve commands from later launches.
    Primary(TcpListener),
    /// Another Wisp took the command.
    Forwarded,
    /// The port belongs to something else; run without single-instance support.
    Alone,
}

fn instance_addr(data_dir: &Path) -> SocketAddr {
    // FNV-1a: stable across builds, unlike std's hasher.
    let mut hash: u32 = 0x811c_9dc5;
    for b in data_dir.to_string_lossy().bytes() {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    SocketAddr::from((Ipv4Addr::LOCALHOST, 41_000 + (hash % 8_000) as u16))
}

pub fn claim_instance(data_dir: &Path, command: &str) -> Instance {
    let addr = instance_addr(data_dir);
    if let Ok(listener) = TcpListener::bind(addr) {
        return Instance::Primary(listener);
    }
    let handed_over = (|| -> std::io::Result<bool> {
        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(400))?;
        stream.set_read_timeout(Some(Duration::from_millis(800)))?;
        writeln!(stream, "{MAGIC} {command}")?;
        let mut reply = String::new();
        BufReader::new(stream).read_line(&mut reply)?;
        Ok(reply.trim() == "ok")
    })();
    if handed_over.unwrap_or(false) {
        Instance::Forwarded
    } else {
        Instance::Alone
    }
}

/// Serves commands from later launches on a background thread.
pub fn serve_instance(listener: TcpListener, on_command: impl Fn(String) + Send + 'static) {
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(Duration::from_millis(800)));
            let mut line = String::new();
            let Ok(mut writer) = stream.try_clone() else { continue };
            if BufReader::new(stream).read_line(&mut line).is_err() {
                continue;
            }
            if let Some(command) = line.trim().strip_prefix(MAGIC) {
                let _ = writer.write_all(b"ok\n");
                on_command(command.trim().to_owned());
            }
        }
    });
}
