//! Operating-system glue: where files live, starting with the computer,
//! notifications, and handing commands to an already running Wisp.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where notes and settings live.
pub struct DataDir {
    pub path: PathBuf,
    /// Next to the program, rather than in the user's profile.
    pub portable: bool,
    /// Notes copied from the old per-user folder on this start, if any.
    pub copied_from: Option<PathBuf>,
}

/// Finds (or sets up) the data folder:
///
/// 1. `WISP_HOME`, if set.
/// 2. A `data` folder next to the executable (portable mode) that is empty or
///    already holds Wisp's files. `wisp-data`, the earlier name, still works.
/// 3. On Windows, Wisp is portable by default: it creates that `data` folder,
///    copying any notes from the per-user folder, unless the program's folder
///    can't be written to (as under Program Files).
/// 4. The per-user folder: `%APPDATA%\Wisp`, `~/Library/Application Support/Wisp`
///    or `~/.local/share/Wisp`.
pub fn data_dir() -> DataDir {
    let user_dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("Wisp");
    if let Some(home) = std::env::var_os("WISP_HOME") {
        return DataDir { path: PathBuf::from(home), portable: false, copied_from: None };
    }
    let exe_dir = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_owned));
    if let Some(found) = exe_dir.and_then(|dir| portable_dir(&dir, &user_dir, cfg!(windows))) {
        return found;
    }
    DataDir { path: user_dir, portable: false, copied_from: None }
}

const DATA_FILES: [&str; 2] = ["notes.json", "settings.json"];

/// The portable data folder in `exe_dir`, creating it when `create` is set.
fn portable_dir(exe_dir: &Path, user_dir: &Path, create: bool) -> Option<DataDir> {
    let is_ours = |dir: &Path| {
        dir.is_dir()
            && (DATA_FILES.iter().any(|f| dir.join(f).exists())
                || std::fs::read_dir(dir).map(|mut entries| entries.next().is_none()).unwrap_or(false))
    };
    for name in ["data", "wisp-data"] {
        let dir = exe_dir.join(name);
        if is_ours(&dir) {
            return Some(DataDir { path: dir, portable: true, copied_from: None });
        }
    }
    let dir = exe_dir.join("data");
    if !create || dir.exists() || std::fs::create_dir(&dir).is_err() {
        return None; // someone else's folder, or nowhere to write
    }
    // Bring the notes along from the per-user folder; the originals stay as they are.
    let mut copied_from = None;
    for file in DATA_FILES {
        let from = user_dir.join(file);
        if from.is_file() && std::fs::copy(&from, dir.join(file)).is_ok() {
            copied_from = Some(user_dir.to_owned());
        }
    }
    Some(DataDir { path: dir, portable: true, copied_from })
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

/// The identity Windows shows on Wisp's notifications (see [`register_app_identity`]).
#[cfg(windows)]
const APP_ID: &str = "Wisp.Notes";

/// Shows a desktop notification without blocking the UI thread; `on_failure` runs
/// (on that thread) if the desktop has no notification service.
pub fn notify(summary: String, body: String, on_failure: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        let mut n = notify_rust::Notification::new();
        n.summary(&summary).body(&body).appname("Wisp");
        #[cfg(windows)]
        n.app_id(APP_ID);
        // Reminders stay on screen until dismissed where the desktop allows it.
        #[cfg(all(unix, not(target_os = "macos")))]
        n.urgency(notify_rust::Urgency::Critical).timeout(notify_rust::Timeout::Never);
        if let Err(err) = n.show() {
            eprintln!("wisp: couldn't show a notification: {err}");
            on_failure();
        }
    });
}

/// Registers Wisp's name and icon with Windows, so notifications say "Wisp" with
/// its icon instead of coming from "Windows PowerShell". Windows needs the icon as
/// a file; it's written to the data folder.
#[cfg(windows)]
pub fn register_app_identity(data_dir: &Path) -> Result<(), String> {
    let icon = data_dir.join("wisp.png");
    if std::fs::read(&icon).ok().as_deref() != Some(crate::icon::PNG_256) {
        std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
        std::fs::write(&icon, crate::icon::PNG_256).map_err(|e| e.to_string())?;
    }
    let key = windows_registry::CURRENT_USER
        .create(format!(r"Software\Classes\AppUserModelId\{APP_ID}"))
        .map_err(|e| e.to_string())?;
    key.set_string("DisplayName", "Wisp").map_err(|e| e.to_string())?;
    key.set_string("IconUri", icon.to_string_lossy()).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(not(windows))]
pub fn register_app_identity(_data_dir: &Path) -> Result<(), String> {
    Ok(())
}

/// The size, in pixels, the system tray draws icons at. Handing over an image of
/// exactly that size keeps the icon sharp; Windows' own downscaling is crude.
pub fn tray_icon_size() -> u32 {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSMICON};
        // Safety: a plain query without pointers.
        let size = unsafe { GetSystemMetrics(SM_CXSMICON) };
        if size > 0 {
            return size as u32;
        }
        16
    }
    #[cfg(not(windows))]
    {
        // Linux trays and the macOS menu bar scale a larger image smoothly.
        64
    }
}

/// Small (title bar) and large (taskbar, Alt+Tab) window icon sizes on Windows.
#[cfg(windows)]
pub fn window_icon_sizes() -> (u32, u32) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXICON, SM_CXSMICON};
    // Safety: plain queries without pointers.
    let (small, big) = unsafe { (GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CXICON)) };
    (if small > 0 { small as u32 } else { 16 }, if big > 0 { big as u32 } else { 32 })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wisp-platform-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn portable_folder_is_created_and_notes_come_along() {
        let (exe, user) = (temp("exe"), temp("user"));
        std::fs::write(user.join("notes.json"), "{}").unwrap();
        let found = portable_dir(&exe, &user, true).unwrap();
        assert_eq!(found.path, exe.join("data"));
        assert!(found.portable);
        assert_eq!(found.copied_from.as_deref(), Some(user.as_path()));
        assert_eq!(std::fs::read_to_string(exe.join("data/notes.json")).unwrap(), "{}");
        assert!(user.join("notes.json").exists(), "the original stays");

        // Next start: the folder is found, nothing is copied again.
        let again = portable_dir(&exe, &user, true).unwrap();
        assert_eq!(again.copied_from, None);
    }

    #[test]
    fn foreign_or_absent_folders_are_left_alone() {
        let (exe, user) = (temp("exe2"), temp("user2"));
        assert!(portable_dir(&exe, &user, false).is_none(), "not created unless asked");
        std::fs::create_dir_all(exe.join("data")).unwrap();
        std::fs::write(exe.join("data/readme.txt"), "someone else's").unwrap();
        assert!(portable_dir(&exe, &user, true).is_none(), "a folder with other files isn't ours");
        std::fs::create_dir_all(exe.join("wisp-data")).unwrap();
        assert_eq!(portable_dir(&exe, &user, true).unwrap().path, exe.join("wisp-data"), "the old name still works");
    }
}
