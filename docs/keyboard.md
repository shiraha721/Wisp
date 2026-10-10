# Keyboard reference

Everything in Wisp works from the keyboard. This page lists every key; the bar at
the bottom of Wisp's windows shows the ones that work at that moment, and `F1`
shows your own shortcuts inside Wisp.

Keys marked ✎ are shortcuts you can change in Settings → Hotkeys; the others are
fixed. On macOS, `Ctrl` here means ⌘, and the key that opens Wisp is ⌃⌥N.

## Anywhere

| Key | Does |
| --- | --- |
| `Alt+N` ✎ | Open Wisp, or hide it. If another program holds `Alt+N`, Wisp uses `Ctrl+Alt+Shift+N` and says so. |

## Writing in the box

| Key | Does |
| --- | --- |
| `Enter` | Save what you typed as a note. |
| `Shift+Enter` | New line. Lines after the first become the note's details. |
| `Ctrl+Enter` ✎ | Save, then open the note to add dates and reminders. |
| `Ctrl+T` ✎ | Step the `#category` word in the box to the next category (`Ctrl+Shift+T`: previous). |
| `Ctrl+Z` ✎ | Undo your typing; right after saving, take the saved note back into the box. |
| `↓` | Go to the list. |
| `Esc` | Hide Wisp. What you typed stays. |

Typing `#name` in the box shows only that category, and saves new notes there.

## The list

| Key | Does |
| --- | --- |
| `↓` / `↑` | Next / previous note. `↑` on the first note goes back to the box. |
| `Tab` / `Shift+Tab` | Next / previous note, wrapping around at the ends. |
| `PageDown` / `PageUp` | Six notes down / up. |
| `Enter`, `Ctrl+E` ✎ | Open the selected note. `Ctrl+E` opens the first note when none is selected. |
| `Ctrl+Enter` ✎ | Open the selected note at its start time. |
| `Ctrl+D` ✎ | Mark done; on the Done tab, bring back. |
| `Ctrl+T` ✎ | Move the selected note to the next category (`Ctrl+Shift+T`: previous). |
| `Ctrl+Delete` ✎ | Delete the selected note. |
| `Ctrl+Z` ✎ | Undo the last change to your notes. |
| `Ctrl+1` ✎ / `Ctrl+2` ✎ | Notes tab / Done tab. |
| `Esc` | Back to the box (with nothing selected: hide Wisp). |

## Editing a note

| Key | Does |
| --- | --- |
| `Tab` / `Shift+Tab` | Next / previous field. |
| `Enter` in the title | Go to the details. |
| `Enter` in the details | New line. |
| `Enter` in another field | Save and close. |
| `Esc` | Save and close. If something can't be read, the first `Esc` explains; a second saves what's valid and keeps the old value of the rest. |
| `Ctrl+Enter`, `Ctrl+E` ✎ | Save and close. |
| `Ctrl+D` ✎ | Save, close and mark the note done. |
| `Ctrl+T` ✎ | Next category (`Ctrl+Shift+T`: previous). |
| `Ctrl+R` ✎ | Add a reminder. |
| `Ctrl+Shift+R` ✎ | Remove the reminder you're on, or the last one. |
| `Ctrl+Delete` ✎ | Delete the note (saved first, so `Ctrl+Z` brings it back as you left it). In a text field, `Ctrl+Delete` and `Ctrl+Backspace` delete a word instead. |
| `Ctrl+Z` | Undo typing in the field you're in. |

On the category row:

| Key | Does |
| --- | --- |
| `←` / `→`, `Space` | Previous / next category. |
| `Delete`, `Backspace` | No category. |
| A letter | The next category whose name starts with it. |

On a reminder's kind:

| Key | Does |
| --- | --- |
| `←` / `→`, `Space` | Change the kind (at start, before start, after start, before due, at due, every day). |
| `Delete`, `Backspace` | Remove the reminder. |

On "Add a reminder", `Enter` adds one.

## Shortcut list (F1)

| Key | Does |
| --- | --- |
| `↓` / `↑`, `PageDown` / `PageUp` | Scroll. |
| `Esc`, `Enter`, `F1` | Back to the list. |

## Everywhere in Wisp's window

| Key | Does |
| --- | --- |
| `F1` ✎ | The shortcut list. |
| `Ctrl+,` ✎ | Settings. |
| `Ctrl+Q` ✎ | Quit Wisp. The key that opens Wisp stops working until Wisp starts again. |

These also work while editing a note: Wisp saves the note first.

## The first start

The window that asks for your language and the key that opens Wisp.

| Key | Does |
| --- | --- |
| `←` `→` `↑` `↓` | Choose a language. `↓` from the bottom row goes on to the key. |
| `Enter`, `Tab` | Next: from the languages to the key, then to **Start using Wisp**. |
| `Shift+Tab`, `↑` | Back. |
| Any combination, on the key | Open Wisp with it from now on. |
| `Backspace`, `Delete`, on the key | Back to `Alt+N`. |
| `Enter`, `Space`, on the button | Start using Wisp. |
| `Esc` | Start using Wisp with what's chosen. |

## Settings

The page list on the left:

| Key | Does |
| --- | --- |
| `↑` / `↓` | Previous / next page. |
| `Home` / `End` | First / last page. |
| `Enter`, `→`, `Space`, `Tab` | Go into the page. |
| `Esc` | Close Settings. |

On a page:

| Key | Does |
| --- | --- |
| `↑` / `↓` | Previous / next setting. |
| `PageUp` / `PageDown`, `Home` / `End` | Five settings up / down; first / last. |
| `Enter`, `Space` | Change the setting: flip a switch, start recording a shortcut, rename a category, choose a language. |
| `←` / `→` | Switch off / on; previous / next theme; previous / next colour of a category. On other settings, `←` goes back to the page list. |
| `Backspace`, `Delete` | Put a shortcut back to its default. On a category, press twice to remove it. |
| `Alt+↑` / `Alt+↓` | Move a category up / down. |
| `F2` | Rename a category. |
| `Tab`, `Esc` | Back to the page list. |

While recording a shortcut, press the new combination; `Esc` keeps the old one.
While typing a category's name, `Enter` or `Tab` saves it and `Esc` cancels.

`F1` jumps to the Hotkeys page, `Ctrl+,` closes Settings and `Ctrl+Q` quits Wisp.

## Changing shortcuts

In Settings → Hotkeys, select a shortcut, press `Enter`, then press the new
combination. Any mix of `Ctrl`, `Alt`, `Shift` and `Win` (⌘, ⌥, ⇧ and ⌃ on macOS)
with a key works. Wisp refuses a combination that would get in the way of
typing (it needs `Ctrl`, `Alt`, `Win` or a function key) or that another
shortcut already uses, and says why. `Backspace` restores a shortcut's default,
and "Reset all shortcuts" restores them all.

Shortcuts also work while a Chinese or Japanese input method is on: the
full-width punctuation those produce (such as `，`) counts as the ASCII key.
