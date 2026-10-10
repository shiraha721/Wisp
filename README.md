# Wisp

A small, keyboard-first place to catch ideas, memos and to-dos. Press a key from
any app, type, press Enter, and you're back to work. Written in Rust with a
[Slint](https://slint.dev) UI: one ~10 MB program for Windows, macOS and Linux,
about 25 MB of memory while it waits, and no GPU needed.

![Notes](docs/images/notes.png)

## What it does

- **One key from anywhere.** `Alt+N` (⌃⌥N on macOS) opens Wisp with the cursor
  in the box, like a launcher. Press it again, or `Esc`, and it's gone. The
  first start asks which language you want and lets you pick another key.
- **Type and go.** `Enter` saves the note. Lines after the first become its
  details, and what you type also searches your notes.
- **Dates in plain words.** `Ctrl+Enter` opens the note for a start and a due
  time: "tomorrow 9am", "fri 17:00", "+30m", "明天下午3点", "demain 14h".
- **Reminders** at the start, before or after it, before the due time, at the
  due time, or every day until the note is done.
- **Categories** such as Daily, Work and School: type `#work` to file a note or
  to see only Work, or press `Ctrl+T`.
- **Done means archived.** `Ctrl+D` moves a note to the Done tab; nothing is
  deleted unless you ask, and `Ctrl+Z` undoes.
- **Every action has a key**, and every key can be changed.
- **12 languages**: English, German, Spanish, Esperanto, French, Italian,
  Portuguese (Brazil), Russian, Japanese, Korean, and Chinese (Simplified and
  Traditional). The date fields understand each one's date words.
- **Portable.** On Windows, notes live in a `data` folder next to the program.
  Wisp starts with the computer and waits in the tray.
- Light and dark themes.

| Editing a note | Settings |
| --- | --- |
| ![Editor](docs/images/editor.png) | ![Settings](docs/images/settings.png) |

![Wisp in Japanese, German and Russian](docs/images/languages.png)

## Documentation

- [User guide](docs/user-guide.md): everything Wisp does, and where it keeps
  your notes.
- [Keyboard reference](docs/keyboard.md)
- [Typing dates](docs/dates.md), in every language
- [Development](docs/development.md): building, testing and how the code fits
  together.
- [Translating](docs/translating.md)
- [Data files](docs/data-format.md)

## Build

You need a recent stable Rust. On Linux also `pkg-config` and the fontconfig
headers (`sudo apt install pkg-config libfontconfig1-dev`); on Windows the MSVC
toolchain with the Windows SDK.

```sh
cargo build --release
./target/release/wisp          # Windows: target\release\wisp.exe
cargo test
```

See [Development](docs/development.md) for cross-compiling to Windows and more.

## License

MIT
