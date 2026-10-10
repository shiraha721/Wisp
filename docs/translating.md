# Translating Wisp

Wisp's text lives in gettext catalogs, one per language:

```
lang/wisp.pot                  every message, untranslated: the template
lang/de/LC_MESSAGES/wisp.po    German
lang/zh_CN/LC_MESSAGES/wisp.po Chinese (Simplified)
…
```

A catalog holds both the text of the windows (marked `@tr("…")` in the `.slint`
files) and the text made in Rust (marked `tr!("…")`). Any `.po` editor works,
such as [Poedit](https://poedit.net), or a plain text editor.

## Improving a translation

1. Edit `lang/<code>/LC_MESSAGES/wisp.po`.
2. Run `cargo test catalogs`. It checks every catalog (see below).
3. Run Wisp in that language (Settings → Language) and look at what you changed.

## When the code changes

New or changed messages must reach the catalogs. `cargo test` fails until they
do, saying `lang/wisp.pot is out of date`. Then:

```sh
WISP_UPDATE_CATALOGS=1 cargo test catalogs
```

rewrites `lang/wisp.pot` from the code, adds new messages to every catalog
(untranslated) and drops ones the code no longer uses. It prints how many
messages each language still needs. Translate those, then run `cargo test`
again.

## Adding a language

1. Create `lang/<code>/LC_MESSAGES/wisp.po`. Use a gettext code: `nl`, `pt_PT`,
   `zh_HK`. Start with the header below, filling in the plural rule for your
   language (the gettext manual and Poedit both list them):

   ```
   msgid ""
   msgstr ""
   "Project-Id-Version: Wisp\n"
   "Language: nl\n"
   "MIME-Version: 1.0\n"
   "Content-Type: text/plain; charset=UTF-8\n"
   "Content-Transfer-Encoding: 8bit\n"
   "Plural-Forms: nplurals=2; plural=(n != 1);\n"
   ```

2. Add the language to `LANGUAGES` in `src/i18n.rs`: its code, its name in
   itself ("Nederlands"), its English name, the matching plural rule (a small
   function next to the list), and its catalog. The English name ("Dutch")
   becomes a new message in every catalog, since the language list shows it in
   the reader's language; the [CLDR](https://cldr.unicode.org) has every
   language's name in every other.
3. If system locales name it differently, teach `for_locale` in `src/i18n.rs`
   (Chinese, for example, tells Simplified from Traditional by region).
4. Run `WISP_UPDATE_CATALOGS=1 cargo test catalogs` to fill the new catalog with
   every message, then translate them.
5. Teach the date fields your language's date words: add a table to
   `src/dates/words.rs` (the French, German or Spanish ones are good models) and
   examples to `dates_in_every_language` in `src/dates.rs`. Without one, the
   date fields still read English, so keep your date examples in English.
6. Add your language to the lists in `README.md`, `docs/user-guide.md` and
   `docs/dates.md`.

## What the tests check

`cargo test catalogs` fails, listing every problem, when:

- a message in the code is missing from a catalog, untranslated, or marked
  fuzzy;
- a catalog has messages the code no longer uses;
- a translation's placeholders (`{}`, `{0}`, `{1}`…) don't match the original;
- a plural message has a different number of forms than the header says;
- a date pattern (context `date format`) isn't one chrono understands;
- a date example (context `date example`) isn't one Wisp reads in that
  language.

## Writing the translations

**Placeholders.** `{}` is filled in order; you can number them to change the
order: `{1}.{0}.` turns "month, day" into "day.month.". `{{` and `}}` are
literal braces.

**Contexts.** A `msgctxt` tells apart the same English used for different
things, and says what kind of text it is:

| Context | What it is |
| --- | --- |
| `date format` | A chrono pattern for showing dates. `%a` is the weekday and `%b` the month, both from this catalog; `%-d` is the day, `%-m` the month as a number, `%Y` the year and `%H:%M` the time. Write dates the way your language does: `%-d. %b` (German), `%-m月%-d日` (Chinese). |
| `date example` | Dates shown as examples, which people will copy. They must be dates Wisp reads in your language; separate several with commas. |
| `weekday`, `month` | Short names used in the date patterns. If your language changes a month's form inside a date (Russian "окт."), use that form. |
| `key` | A key's name as keyboards in your language print it: `Strg` for Ctrl in German, `Échap` for Esc in French. Keep the English name if that's what keyboards show. |
| `note details` | The label of a note's details field, which may differ from "Notes", the list of notes. |
| `language` | The badge on the system's own language in the window of the first start. |

**Counts.** Messages with a count (`{} day` / `{} days`) have a form for each
plural case of your language; the header's `Plural-Forms` says how many.

**Names.** Keep "Wisp" as it is. The category names "Daily", "Work" and "School"
are only where new notes files start; people rename them freely.

**Tone.** Wisp's English is short and plain: sentence case, everyday words, no
exclamation marks. Errors say what happened and what to do. Prefer your
language's usual informal or neutral register for software.

## How it works

The Slint compiler reads the catalogs at build time (`build.rs` passes it
`lang/`) and bundles the window text into the program. `src/i18n.rs` includes the
same files and looks up Rust's messages at run time. Messages carry no context
unless they name one, so both sides find the same entries.

Switching language calls `slint::select_bundled_translation`, which updates the
windows, and then rebuilds the text Rust made: the list, the shortcut list and
the settings cards.
