# Data files

Wisp keeps everything in two JSON files in its data folder (see
[Where your notes are kept](user-guide.md#where-your-notes-are-kept)). Both are
plain UTF-8 JSON, written whole on every change: first to a `.json.tmp` file,
which then replaces the old one.

Quit Wisp before editing them by hand; a running Wisp would write over your
changes. Fields Wisp doesn't know are dropped the next time it saves. A file
Wisp can't read is moved aside, never overwritten (see
[Backups](user-guide.md#where-your-notes-are-kept)).

## notes.json

```json
{
  "next_id": 12,
  "items": [
    {
      "id": 1,
      "text": "Submit the Q3 report",
      "notes": "Numbers from finance are in the shared drive\nAdd the churn chart",
      "created": 1791520392,
      "updated": 1791520392,
      "start": 1791637200,
      "due": 1792184400,
      "reminders": [
        { "rule": "at_start" },
        { "rule": "before_due", "minutes": 120, "last_fired": 1792177200 }
      ],
      "category": 10
    },
    {
      "id": 8,
      "text": "Renew passport photo",
      "created": 1791263000,
      "updated": 1791349400,
      "done_at": 1791349400
    }
  ],
  "categories": [
    { "id": 10, "name": "Daily", "color": 1 },
    { "id": 11, "name": "Work", "color": 0 }
  ]
}
```

Times are Unix timestamps in seconds. Fields that are empty are left out.

| Field | Meaning |
| --- | --- |
| `next_id` | The next id Wisp hands out. Notes and categories share it, so an id is never used twice. |
| `items` | The notes, open and done. |
| `categories` | The categories, in the order they're shown. Missing in files from before categories existed; Wisp then adds Daily, Work and School once. An empty list means you removed them all. |

A note (`items`):

| Field | Meaning |
| --- | --- |
| `id` | The note's number. |
| `text` | The title: one line. |
| `notes` | The details: any number of lines. |
| `created`, `updated` | When the note was written and last changed. |
| `start`, `due` | The start and due times, if set. |
| `reminders` | The reminders, see below. |
| `category` | The id of the note's category, if it has one. |
| `done_at` | When the note was marked done. Done notes show on the Done tab. |
| `ringing` | `true` after a reminder rang, until the note is opened. |

A reminder has a `rule` and the rule's settings:

| Rule | Settings | Rings |
| --- | --- | --- |
| `at_start` | | at the start |
| `before_start` | `minutes` | that long before the start |
| `after_start` | `minutes` | that long after the start |
| `before_due` | `minutes` | that long before the due time |
| `at_due` | | at the due time |
| `daily` | `minute_of_day` (optional, 0–1439) | every day from the start until the note is done, at that minute of the day or at the start's time of day |

`last_fired` is the moment the reminder last rang for; Wisp uses it so each
moment rings once. Leave it out and a reminder whose moment has passed may ring
when Wisp starts (if it's less than six hours late).

A category has an `id`, a `name` and a `color`, which is one of Wisp's eight
category colours (0 blue, 1 green, 2 orange, 3 purple, 4 pink, 5 teal, 6 amber,
7 red).

Notes from earlier versions, where everything after a note's first line was
part of `text`, are split into `text` and `notes` when loaded.

## settings.json

```json
{
  "version": 1,
  "summon": "Ctrl+Alt+Space",
  "shortcuts": {
    "toggle_done": "Ctrl+K"
  },
  "launch_at_startup": true,
  "hide_when_inactive": true,
  "open_on_reminder": false,
  "theme": "system",
  "language": "en",
  "initialized": true
}
```

| Field | Meaning |
| --- | --- |
| `version` | The settings format; Wisp upgrades older files. |
| `summon` | The key that opens Wisp from anywhere. Left out while it's the default (`Alt+N`, or ⌃⌥N on macOS). |
| `shortcuts` | Shortcuts you changed, by name (below). The others use their defaults. |
| `launch_at_startup` | Start with the computer, in the tray. |
| `hide_when_inactive` | Hide the window when you click somewhere else. |
| `open_on_reminder` | Show the window when a reminder rings, as well as the notification. |
| `theme` | `system`, `light` or `dark`. |
| `language` | `en`, `de`, `es`, `eo`, `fr`, `it`, `pt_BR`, `ru`, `ja`, `ko`, `zh_CN`, `zh_TW`, or `system` to follow the system's language. |
| `initialized` | Set at the first start, which asks for the language and the summon key and switches on starting with the computer (release builds only). Set it to `false` to see that window again; starting with the computer is then switched on again too. |

The names in `shortcuts`, with their defaults:

| Name | Default | Does |
| --- | --- | --- |
| `schedule` | `Ctrl+Enter` | Save and add dates |
| `open` | `Ctrl+E` | Open the selected note |
| `toggle_done` | `Ctrl+D` | Mark done / bring back |
| `category` | `Ctrl+T` | Change category |
| `delete` | `Ctrl+Delete` | Delete |
| `undo` | `Ctrl+Z` | Undo |
| `add_reminder` | `Ctrl+R` | Add a reminder |
| `remove_reminder` | `Ctrl+Shift+R` | Remove a reminder |
| `inbox` | `Ctrl+1` | Show notes |
| `archive` | `Ctrl+2` | Show done |
| `help` | `F1` | Keyboard shortcuts |
| `settings` | `Ctrl+,` | Open settings |
| `quit` | `Ctrl+Q` | Quit Wisp |

Key combinations are written as modifiers and a key joined by `+`: `Ctrl`, `Alt`,
`Shift` and `Super` (the Windows key), then a letter, digit, punctuation mark,
`F1`–`F24`, or a name such as `Enter`, `Delete`, `Space`, `Tab`, `Up`, `PageDown`.
On macOS `Ctrl` stands for ⌘ and `Super` for ⌃, so the same file works on every
system.
