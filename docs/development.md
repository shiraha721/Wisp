# Development

Wisp is a Rust program with a [Slint](https://slint.dev) UI. Rust owns all the
state; the Slint windows are views that Rust fills in and that report keys and
clicks back.

- [Building](#building)
- [Testing](#testing)
- [Cross-compiling for Windows](#cross-compiling-for-windows)
- [How the code fits together](#how-the-code-fits-together)
- [Icons](#icons)
- [Releasing](#releasing)

## Building

You need a recent stable Rust (`rustup update`), and:

- **Linux**: `pkg-config` and the fontconfig headers
  (`sudo apt install pkg-config libfontconfig1-dev`, or `fontconfig-devel` on
  Fedora). Nothing else: the X11, D-Bus and tray libraries Wisp uses are loaded
  at run time or written in Rust.
- **macOS**: the Xcode command-line tools.
- **Windows**: the MSVC toolchain (Visual Studio Build Tools) with the Windows
  SDK, whose `rc.exe` compiles the program's icon and version details.

```sh
cargo build --release
./target/release/wisp               # Windows: target\release\wisp.exe
```

Release builds are optimised for size (`opt-level = "s"`, LTO, stripped): about
10 MB on Windows. They switch on starting with the computer the first time they run; debug
builds never do.

While developing, keep your real notes out of the way with `WISP_HOME`:

```sh
WISP_HOME=/tmp/wisp-dev cargo run
WISP_DEBUG=1 cargo run              # also prints window and focus events
```

## Testing

```sh
cargo test
cargo clippy --all-targets
```

The tests cover what's easy to get subtly wrong:

| Area | Tests |
| --- | --- |
| `dates.rs` | Date parsing in every language, labels, durations, clock times. |
| `model.rs` | Reminder rules, firing once and skipping stale moments, categories and `#` words, loading old and broken files. |
| `shortcuts.rs` | Reading key events (including input-method punctuation), parsing and printing combinations, hotkey codes. |
| `config.rs` | Default shortcuts being valid and distinct, overrides, upgrading old settings. |
| `i18n/tests.rs` | Reading catalogs, plural rules, system locales, and the catalogs matching the code (see [Translating](translating.md)). |
| `platform.rs` | Finding and creating the portable data folder. |
| `icon.rs` | Every icon size decoding. |

The windows themselves are tested by hand. On Linux without a desktop, Wisp
runs under Xvfb; `xdotool` or XTest can type into it and ImageMagick's `import`
takes screenshots.

## Cross-compiling for Windows

From Linux, with MinGW-w64 (`sudo apt install mingw-w64`):

```sh
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
```

and in `.cargo/config.toml` (or `~/.cargo/config.toml`):

```toml
[target.x86_64-pc-windows-gnu]
linker = "x86_64-w64-mingw32-gcc"
```

`build.rs` compiles the icon, manifest and version details with
`x86_64-w64-mingw32-windres`, which comes with MinGW-w64. The program ends up in
`target/x86_64-pc-windows-gnu/release/wisp.exe`.

## How the code fits together

```
 global-hotkey thread ──┐                       ┌── notification threads
 (the summon key)       │                       │   (one per reminder)
                        ▼                       │
 instance thread ──► UI thread: Slint event loop ──► App ──► Store ─► notes.json
 (wisp --toggle …)      ▲        timers (5 s)        │  └──► Config ─► settings.json
                        │                            ▼
                 AppWindow, SettingsWindow ◄── properties and models
                 (keys and clicks)
```

Everything happens on the UI thread. The other threads only hand events to it
(`slint::invoke_from_event_loop`): the summon key from global-hotkey, commands
from later launches (`wisp --toggle`), and a notification that couldn't be
shown.

### Source files

```
src/main.rs            start-up: single instance, windows, tray, hotkey, timers
src/app.rs             the controller: showing and hiding, keys, the list,
                       categories, undo, reminders, the shortcut list
src/app/editor.rs      the note editor
src/app/settings.rs    the settings window: pages, cards and their keys
src/model.rs           notes, reminder rules, categories and the JSON store
src/dates.rs           date parsing (English) and date labels
src/dates/words.rs     date words of other languages, rewritten into English
src/i18n.rs            translations: languages, catalogs, tr! and friends
src/i18n/tests.rs      keeps lang/ in step with the code
src/shortcuts.rs       key combinations ↔ text ↔ system-wide hotkeys
src/config.rs          settings and default shortcuts
src/platform.rs        data folder, autostart, notifications, single instance
src/icon.rs            the icon PNGs
ui/app.slint           what Rust sees, and the tray menu
ui/launcher.slint      the main window: list, editor, shortcut list
ui/settings.slint      the settings window
ui/theme.slint         colours, category colours, sizes and the icon set
ui/widgets.slint       shared pieces: keycaps, tiles, chips, cards, inputs
lang/                  translations (see translating.md)
assets/                the app icon and the Windows manifest
tools/icons/           renders the icon SVGs into PNGs and the .ico
build.rs               compiles the UI (with translations) and Windows resources
```

### The controller

`App` (in `app.rs`) holds the notes (`Store`), the settings (`Config`) and a
weak handle to each window. Slint callbacks call into it through `with_app`,
which borrows it for the length of the call; work that has to wait until the
current event is over goes through `app::later`.

After anything changes, `App::refresh` rebuilds the list from the store: it
filters by the `#category` word and the rest of the text in the box, sorts,
works out each row's labels and position, and hands the rows to the window.
The editor and the settings window have their own `update_editor` and
`render_settings`.

### Keys

Every key press in either window goes to Rust first. A `FocusScope` around each
window's content catches it before any text field does and calls
`key(text, ctrl, alt, shift, meta, context)`. `context` says where the
keyboard is (`"capture"`, `"edit-notes"`, `"edit-reminder-kind:2"`…). Rust turns
the event into a `Combo`, looks up whether it's a shortcut, and handles it for
the current view (`key_list`, `key_editor`, `key_help`, `settings_key`).
Returning `false` lets the text field have the key.

`shortcuts.rs` reads key events into `Combo`s, writes and parses them as text
(`"Ctrl+Shift+R"`), turns them into global-hotkey `HotKey`s and prints them for
people (`Ctrl+Shift+R`, `Strg+Umschalt+R`, `⌘⇧R`). "Ctrl" means ⌘ on macOS, so
one settings file works everywhere.

### Notes, categories and reminders

`model.rs` holds the data: an `Item` per note, `Category`s, and `Reminder`s,
each a `Rule` (at start, before due, every day, …) plus the moment it last rang.
A rule can say when it would ring next (`next_after`) and when it last should
have (`latest_at_or_before`); both work in any time zone, so the tests pin one.

Every five seconds `App::tick` asks each note which reminders are due
(`collect_due`). A reminder rings once per moment, and moments more than six
hours late (the computer was off) are skipped. Saving a note re-arms its
reminders, so changing a date never rings for moments that have passed.

The store writes `notes.json` whole on every change, through a temporary file.
See [Data files](data-format.md) for the format.

### Dates

`dates::parse` reads English. Input in other languages goes through
`dates/words.rs` first, which rewrites it into English words: `demain 14h`
becomes `tomorrow 14:00` and `明日の午後3時` becomes `tomorrow pm-prefix 3:00`.
Chinese, Japanese, Korean and Russian are always rewritten; languages written in
Latin letters only when Wisp is shown in them, because their words clash
("mar"). The same tables handle how each language writes numbers: `14h30`,
`20/10` with the day first, `20.10.` in German.

### Translations

Every piece of text shown is either `@tr("…")` in a `.slint` file or
`tr!("…")`, `trn!(…)` or `tr_noop!("…")` in Rust. One gettext catalog per
language holds both: the Slint compiler bundles the window text at build time,
and `i18n.rs` reads the Rust text at run time. The current language is
per-thread, which keeps the tests independent. Date formats are patterns in the
catalogs too (`%b %-d` in English, `%-m月%-d日` in Chinese). See
[Translating](translating.md).

### The settings window

Each page is a list of `Entry`s (`app/settings.rs`) turned into `SettingCard`s:
a struct with a `kind` (shortcut, switch, theme picker, button, information,
heading, category, choice) that `settings.slint` draws. Keys move a cursor
through the entries; the selected card has a highlight.

### Platform

`platform.rs` handles what differs between systems:

- **Data folder**: `WISP_HOME`, a `data` folder next to the program, or the
  per-user folder, created and filled on first start on Windows.
- **Starting with the computer**: through `auto-launch` (the registry's Run key
  on Windows, a login item on macOS, an autostart entry on Linux).
- **Notifications**: `notify-rust`. On Windows they come from the app id
  `Wisp.Notes`, registered under
  `HKCU\Software\Classes\AppUserModelId\Wisp.Notes` with Wisp's name and icon.
- **One Wisp per data folder**: the first one listens on a loopback port worked
  out from the data folder's path (41000–48999); later launches send their
  command (`show`, `toggle`, `settings`, `quit`) there and exit.

`main.rs` gives every window Wisp's icon through winit's window attributes; on
Windows the launcher also leaves the taskbar and gets rounded corners and a
shadow. On Linux only winit's X11 backend is built: a launcher has to place and
focus its own window and grab a key, which Wayland apps may not do, so Wisp
runs through XWayland there.

The tray icon is Slint's `SystemTrayIcon`. On Windows the image is rendered at
exactly the size the tray draws (`GetSystemMetrics(SM_CXSMICON)`), since Windows
scales icons down crudely.

## Icons

The icon is drawn in `assets/icon.svg`, with a simpler version for 16–32 px in
`assets/icon-small.svg`. After changing either, render the PNGs and the .ico:

```sh
cargo run --release --manifest-path tools/icons/Cargo.toml
```

`build.rs` embeds `assets/wisp.ico` into `wisp.exe` with the version details and
`assets/windows/wisp.manifest` (per-monitor DPI awareness, common controls 6).

## Releasing

1. Update `version` in `Cargo.toml`.
2. `cargo test` and `cargo clippy --all-targets`. The tests fail if a translation
   is missing; see [Translating](translating.md).
3. Build on (or for) each system and try it: open, write, schedule, let a
   reminder ring, switch language, quit and start again.
4. On Windows, ship `wisp.exe` alone (renamed `Wisp.exe` if you like). It makes
   its `data` folder on first start.
