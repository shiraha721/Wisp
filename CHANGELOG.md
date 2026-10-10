# Changelog

Each version's section is also its release notes on GitHub.

## 0.1.0 (2026-10-10)

The first release.

- **One key from anywhere.** `Alt+N` (⌃⌥N on macOS) opens Wisp, or any key you
  choose when it first starts.
- **Type and go.** `Enter` saves a note; `Ctrl+Enter` adds a start, a due time
  and reminders, written in plain words: "tomorrow 9am", "fri 17:00", "+30m".
- **Reminders** at the start, before or after it, before the due time, at the
  due time, or every day until the note is done.
- **Categories** such as Daily, Work and School (`#work`), a Done list, undo,
  and a key for every action, each one changeable.
- **12 languages:** English, German, Spanish, Esperanto, French, Italian,
  Portuguese (Brazil), Russian, Japanese, Korean, and Chinese (Simplified and
  Traditional). The date fields understand each one's date words.
- Light and dark themes. On Windows, Wisp is portable: its notes live in a
  `data` folder next to `Wisp.exe`.

### Downloads

| File | For |
| --- | --- |
| `wisp-0.1.0-windows-x64.zip` | Windows 10 and 11. Unzip it into a folder of its own and start `Wisp.exe`. |
| `wisp-0.1.0-macos-arm64.tar.gz` | Macs with Apple silicon. |
| `wisp-0.1.0-macos-x64.tar.gz` | Macs with an Intel processor. |
| `wisp-0.1.0-linux-x64.tar.gz` | Linux with X11, or XWayland on Wayland desktops. |

`SHA256SUMS.txt` lists each file's checksum.

The macOS programs aren't signed. After unpacking, allow `wisp` under System
Settings → Privacy & Security (or run `xattr -d com.apple.quarantine wisp`),
then start it from a terminal with `./wisp`.
