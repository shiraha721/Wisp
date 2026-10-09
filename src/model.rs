//! Notes, their schedule and reminders, and the JSON file they live in.

use crate::dates;
use chrono::{DateTime, NaiveTime, TimeZone, Timelike};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// When a reminder goes off, relative to the item's start or due time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "rule", rename_all = "snake_case")]
pub enum Rule {
    AtStart,
    BeforeStart { minutes: i64 },
    AfterStart { minutes: i64 },
    BeforeDue { minutes: i64 },
    AtDue,
    /// Every day from the start until the item is done, at `minute_of_day`
    /// (or the start's own time of day).
    Daily { minute_of_day: Option<u32> },
}

/// Labels for the kind picker in the editor, indexed by [`Rule::kind`].
pub const RULE_KINDS: [&str; 6] = ["At start", "Before start", "After start", "Before due", "At due", "Every day"];

impl Rule {
    pub fn kind(&self) -> usize {
        match self {
            Rule::AtStart => 0,
            Rule::BeforeStart { .. } => 1,
            Rule::AfterStart { .. } => 2,
            Rule::BeforeDue { .. } => 3,
            Rule::AtDue => 4,
            Rule::Daily { .. } => 5,
        }
    }

    /// Builds a rule from the editor's kind index and its free-text amount.
    pub fn from_parts(kind: usize, amount: &str) -> Result<Rule, String> {
        let minutes = || {
            dates::parse_minutes(amount).ok_or_else(|| format!("“{amount}” isn't a duration. Try 15m, 2h or 1d."))
        };
        Ok(match kind {
            0 => Rule::AtStart,
            1 => Rule::BeforeStart { minutes: minutes()? },
            2 => Rule::AfterStart { minutes: minutes()? },
            3 => Rule::BeforeDue { minutes: minutes()? },
            4 => Rule::AtDue,
            _ => Rule::Daily {
                minute_of_day: if amount.trim().is_empty() {
                    None
                } else {
                    let t = dates::parse_clock(amount)
                        .ok_or_else(|| format!("“{amount}” isn't a time of day. Try 09:00."))?;
                    Some(t.hour() * 60 + t.minute())
                },
            },
        })
    }

    /// The editable amount for this rule ("15m", "09:00" or "").
    pub fn amount(&self) -> String {
        match *self {
            Rule::BeforeStart { minutes } | Rule::AfterStart { minutes } | Rule::BeforeDue { minutes } => {
                dates::format_minutes(minutes)
            }
            Rule::Daily { minute_of_day: Some(m) } => format!("{:02}:{:02}", m / 60, m % 60),
            _ => String::new(),
        }
    }

    /// The default amount shown when the user switches to `kind`.
    pub fn default_amount(kind: usize) -> &'static str {
        match kind {
            1 | 2 => "15m",
            3 => "1h",
            _ => "",
        }
    }

    pub fn needs_start(&self) -> bool {
        !matches!(self, Rule::BeforeDue { .. } | Rule::AtDue)
    }

    /// The single moment a one-shot rule fires at, if its anchor is set.
    fn moment(&self, start: Option<i64>, due: Option<i64>) -> Option<i64> {
        match *self {
            Rule::AtStart => start,
            Rule::BeforeStart { minutes } => start.map(|s| s - minutes * 60),
            Rule::AfterStart { minutes } => start.map(|s| s + minutes * 60),
            Rule::BeforeDue { minutes } => due.map(|d| d - minutes * 60),
            Rule::AtDue => due,
            Rule::Daily { .. } => None,
        }
    }

    /// The most recent time this rule was scheduled to fire at or before `now`.
    pub fn latest_at_or_before<Tz: TimeZone>(&self, tz: &Tz, start: Option<i64>, due: Option<i64>, now: i64) -> Option<i64> {
        match *self {
            Rule::Daily { minute_of_day } => {
                let daily = Daily::new(tz, start?, minute_of_day);
                let today = tz.timestamp_opt(now, 0).single()?.date_naive();
                let mut t = daily.on(today);
                if t > now {
                    t = daily.on(today.pred_opt()?);
                }
                (t >= daily.first).then_some(t)
            }
            _ => self.moment(start, due).filter(|&t| t <= now),
        }
    }

    /// The next time this rule fires strictly after `after`.
    pub fn next_after<Tz: TimeZone>(&self, tz: &Tz, start: Option<i64>, due: Option<i64>, after: i64) -> Option<i64> {
        match *self {
            Rule::Daily { minute_of_day } => {
                let daily = Daily::new(tz, start?, minute_of_day);
                if daily.first > after {
                    return Some(daily.first);
                }
                let day = tz.timestamp_opt(after, 0).single()?.date_naive();
                let t = daily.on(day);
                Some(if t > after { t } else { daily.on(day.succ_opt()?) })
            }
            _ => self.moment(start, due).filter(|&t| t > after),
        }
    }
}

/// Helper for "every day at HH:MM, beginning at the start".
struct Daily<'a, Tz: TimeZone> {
    tz: &'a Tz,
    time: NaiveTime,
    first: i64,
}

impl<'a, Tz: TimeZone> Daily<'a, Tz> {
    fn new(tz: &'a Tz, start: i64, minute_of_day: Option<u32>) -> Self {
        let start_local = tz.timestamp_opt(start, 0).single().map(|t| t.naive_local()).unwrap_or_default();
        let time = match minute_of_day {
            Some(m) => dates::hm(m / 60 % 24, m % 60),
            None => dates::hm(start_local.hour(), start_local.minute()),
        };
        let mut daily = Daily { tz, time, first: 0 };
        let on_start_day = daily.on(start_local.date());
        daily.first = if on_start_day >= start { on_start_day } else { on_start_day + 86_400 };
        daily
    }

    fn on(&self, day: chrono::NaiveDate) -> i64 {
        dates::resolve_local(self.tz, day.and_time(self.time)).timestamp()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reminder {
    #[serde(flatten)]
    pub rule: Rule,
    /// The scheduled moment of the last firing, so each one rings once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fired: Option<i64>,
}

impl Reminder {
    pub fn new(rule: Rule) -> Self {
        Reminder { rule, last_fired: None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: u64,
    pub text: String,
    /// Unix seconds.
    pub created: i64,
    #[serde(default)]
    pub updated: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reminders: Vec<Reminder>,
    /// Set when the item is marked done; done items live in the archive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_at: Option<i64>,
    /// A reminder went off and the user hasn't looked at the item since.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ringing: bool,
}

impl Item {
    pub fn is_done(&self) -> bool {
        self.done_at.is_some()
    }

    pub fn is_scheduled(&self) -> bool {
        self.start.is_some() || self.due.is_some()
    }

    /// First non-empty line, for the list.
    pub fn title(&self) -> &str {
        self.text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("")
    }

    pub fn has_more_lines(&self) -> bool {
        self.text.trim().lines().filter(|l| !l.trim().is_empty()).count() > 1
    }

    pub fn is_overdue(&self, now: i64) -> bool {
        !self.is_done() && self.due.is_some_and(|d| d < now)
    }

    /// The moment that matters most for ordering the schedule: the nearest upcoming
    /// start/due, or the latest one if both have passed.
    pub fn schedule_key(&self, now: i64) -> i64 {
        let times = [self.start, self.due];
        let upcoming = times.iter().flatten().filter(|&&t| t >= now).min();
        let past = times.iter().flatten().max();
        upcoming.or(past).copied().unwrap_or(i64::MAX)
    }

    /// Marks every occurrence up to `now` as already fired, so editing an item
    /// doesn't set off reminders for moments that have already passed.
    pub fn arm_reminders<Tz: TimeZone>(&mut self, tz: &Tz, now: i64) {
        let (start, due) = (self.start, self.due);
        for r in &mut self.reminders {
            r.last_fired = r.rule.latest_at_or_before(tz, start, due, now);
        }
    }

    /// Fires every reminder whose moment has come, returning what to announce.
    /// Moments missed by more than `grace` seconds (say, the computer was off) are
    /// skipped silently.
    pub fn collect_due<Tz: TimeZone>(&mut self, tz: &Tz, now: i64, grace: i64) -> Option<Fired> {
        if self.is_done() {
            return None;
        }
        let (start, due) = (self.start, self.due);
        let mut fired_at = None;
        for r in &mut self.reminders {
            let Some(moment) = r.rule.latest_at_or_before(tz, start, due, now) else { continue };
            if r.last_fired.is_some_and(|f| f >= moment) {
                continue;
            }
            r.last_fired = Some(moment);
            if now - moment <= grace {
                fired_at = Some(fired_at.map_or(moment, |m: i64| m.max(moment)));
            }
        }
        fired_at.map(|_| {
            self.ringing = true;
            Fired { id: self.id }
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fired {
    pub id: u64,
}

/// The text of a reminder notification for `item`.
pub fn notification_body<Tz: TimeZone>(item: &Item, now: &DateTime<Tz>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let tz = now.timezone();
    let at = |t: i64| tz.timestamp_opt(t, 0).single();
    let mut lines = Vec::new();
    for (time, upcoming, passed) in [(item.start, "Starts", "Started"), (item.due, "Due", "Was due")] {
        if let Some(t) = time.and_then(at) {
            let delta = t.timestamp() - now.timestamp();
            let verb = if delta > 0 { upcoming } else { passed };
            lines.push(format!("{verb} {} ({})", dates::day_label(&t, now), dates::relative(delta)));
        }
    }
    lines.join("\n")
}

// ---------------------------------------------------------------------------

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Data {
    #[serde(default)]
    pub next_id: u64,
    #[serde(default)]
    pub items: Vec<Item>,
}

pub struct Store {
    path: PathBuf,
    pub data: Data,
}

impl Store {
    /// Loads the store, or starts empty. A file that can't be read is moved aside
    /// (never overwritten) and the returned warning says where it went.
    pub fn load(path: &Path) -> (Store, Option<String>) {
        let mut warning = None;
        let data = match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<Data>(&text) {
                Ok(data) => data,
                Err(err) => {
                    let backup = path.with_extension(format!("broken-{}.json", chrono::Local::now().format("%Y%m%d-%H%M%S")));
                    let _ = std::fs::rename(path, &backup);
                    warning = Some(format!("Couldn't read {} ({err}). It was moved to {}.", path.display(), backup.display()));
                    Data::default()
                }
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Data::default(),
            Err(err) => {
                warning = Some(format!("Couldn't open {}: {err}", path.display()));
                Data::default()
            }
        };
        let mut store = Store { path: path.to_owned(), data };
        let max_id = store.data.items.iter().map(|i| i.id).max().unwrap_or(0);
        store.data.next_id = store.data.next_id.max(max_id + 1);
        (store, warning)
    }

    /// Writes atomically: a crash mid-save leaves the previous file intact.
    pub fn save(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&self.data).map_err(|e| e.to_string())?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| format!("{}: {e}", self.path.display()))
    }

    pub fn add(&mut self, text: &str, now: i64) -> u64 {
        let id = self.data.next_id.max(1);
        self.data.next_id = id + 1;
        self.data.items.push(Item {
            id,
            text: text.to_owned(),
            created: now,
            updated: now,
            start: None,
            due: None,
            reminders: Vec::new(),
            done_at: None,
            ringing: false,
        });
        id
    }

    pub fn get(&self, id: u64) -> Option<&Item> {
        self.data.items.iter().find(|i| i.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Item> {
        self.data.items.iter_mut().find(|i| i.id == id)
    }

    /// Removes an item, returning it with its position so it can be put back.
    pub fn remove(&mut self, id: u64) -> Option<(usize, Item)> {
        let pos = self.data.items.iter().position(|i| i.id == id)?;
        Some((pos, self.data.items.remove(pos)))
    }

    pub fn insert(&mut self, pos: usize, item: Item) {
        let pos = pos.min(self.data.items.len());
        self.data.items.insert(pos, item);
    }

    pub fn replace(&mut self, item: Item) {
        if let Some(slot) = self.get_mut(item.id) {
            *slot = item;
        }
    }
}

/// Space-separated words that must all appear (case-insensitively) in the text.
pub fn matches_query(text: &str, query: &str) -> bool {
    let haystack = text.to_lowercase();
    query.to_lowercase().split_whitespace().all(|w| haystack.contains(w))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, TimeZone};

    fn tz() -> FixedOffset {
        FixedOffset::east_opt(2 * 3600).unwrap()
    }
    fn ts(d: u32, h: u32, m: u32) -> i64 {
        tz().with_ymd_and_hms(2026, 10, d, h, m, 0).unwrap().timestamp()
    }
    fn item(start: Option<i64>, due: Option<i64>, rules: &[Rule]) -> Item {
        Item {
            id: 1,
            text: "Write report\nwith charts".into(),
            created: 0,
            updated: 0,
            start,
            due,
            reminders: rules.iter().copied().map(Reminder::new).collect(),
            done_at: None,
            ringing: false,
        }
    }

    #[test]
    fn one_shot_rules() {
        let (s, d) = (Some(ts(10, 9, 0)), Some(ts(12, 17, 0)));
        let tz = tz();
        assert_eq!(Rule::AtStart.next_after(&tz, s, d, ts(9, 0, 0)), Some(ts(10, 9, 0)));
        assert_eq!(Rule::BeforeStart { minutes: 15 }.next_after(&tz, s, d, ts(9, 0, 0)), Some(ts(10, 8, 45)));
        assert_eq!(Rule::AfterStart { minutes: 30 }.next_after(&tz, s, d, ts(9, 0, 0)), Some(ts(10, 9, 30)));
        assert_eq!(Rule::BeforeDue { minutes: 60 }.next_after(&tz, s, d, ts(9, 0, 0)), Some(ts(12, 16, 0)));
        assert_eq!(Rule::AtDue.latest_at_or_before(&tz, s, d, ts(13, 0, 0)), Some(ts(12, 17, 0)));
        assert_eq!(Rule::AtDue.latest_at_or_before(&tz, s, d, ts(11, 0, 0)), None);
        assert_eq!(Rule::AtStart.next_after(&tz, None, d, 0), None);
    }

    #[test]
    fn daily_rule_follows_start_time_or_its_own() {
        let tz = tz();
        let s = Some(ts(10, 9, 30));
        let daily = Rule::Daily { minute_of_day: None };
        assert_eq!(daily.next_after(&tz, s, None, ts(9, 0, 0)), Some(ts(10, 9, 30)));
        assert_eq!(daily.next_after(&tz, s, None, ts(10, 9, 30)), Some(ts(11, 9, 30)));
        assert_eq!(daily.latest_at_or_before(&tz, s, None, ts(12, 8, 0)), Some(ts(11, 9, 30)));
        assert_eq!(daily.latest_at_or_before(&tz, s, None, ts(10, 9, 0)), None);

        let at_eight = Rule::Daily { minute_of_day: Some(8 * 60) };
        // Start is 09:30, so the first 08:00 after it is the next day.
        assert_eq!(at_eight.next_after(&tz, s, None, ts(9, 0, 0)), Some(ts(11, 8, 0)));
        assert_eq!(at_eight.latest_at_or_before(&tz, s, None, ts(10, 23, 0)), None);
        assert_eq!(at_eight.latest_at_or_before(&tz, s, None, ts(13, 7, 0)), Some(ts(12, 8, 0)));
    }

    #[test]
    fn reminders_fire_once_and_skip_stale_moments() {
        let tz = tz();
        let mut it = item(Some(ts(10, 9, 0)), None, &[Rule::AtStart, Rule::Daily { minute_of_day: None }]);
        assert_eq!(it.collect_due(&tz, ts(10, 8, 59), 3600), None);
        assert_eq!(it.collect_due(&tz, ts(10, 9, 0), 3600), Some(Fired { id: 1 }));
        assert!(it.ringing);
        assert_eq!(it.collect_due(&tz, ts(10, 9, 5), 3600), None, "already fired");
        assert_eq!(it.collect_due(&tz, ts(11, 9, 1), 3600), Some(Fired { id: 1 }), "next day");
        // Off for two days: the old daily moment is too stale to announce.
        assert_eq!(it.collect_due(&tz, ts(13, 12, 0), 3600), None);
        it.done_at = Some(ts(13, 12, 0));
        assert_eq!(it.collect_due(&tz, ts(14, 9, 0), 3600), None, "done items stay quiet");
    }

    #[test]
    fn arming_skips_the_past() {
        let tz = tz();
        let mut it = item(Some(ts(10, 9, 0)), Some(ts(10, 18, 0)), &[Rule::AtStart, Rule::BeforeDue { minutes: 30 }]);
        it.arm_reminders(&tz, ts(10, 12, 0));
        assert_eq!(it.collect_due(&tz, ts(10, 12, 1), 3600), None);
        assert_eq!(it.collect_due(&tz, ts(10, 17, 30), 3600), Some(Fired { id: 1 }));
        assert!(it.reminders.iter().all(|r| r.rule.next_after(&tz, it.start, it.due, ts(10, 17, 30)).is_none()));
    }

    #[test]
    fn rule_parts_round_trip() {
        for rule in [
            Rule::AtStart,
            Rule::BeforeStart { minutes: 90 },
            Rule::AfterStart { minutes: 15 },
            Rule::BeforeDue { minutes: 1440 },
            Rule::AtDue,
            Rule::Daily { minute_of_day: None },
            Rule::Daily { minute_of_day: Some(8 * 60 + 5) },
        ] {
            assert_eq!(Rule::from_parts(rule.kind(), &rule.amount()), Ok(rule));
        }
        assert!(Rule::from_parts(1, "soon").is_err());
    }

    #[test]
    fn titles_and_queries() {
        let it = item(None, None, &[]);
        assert_eq!(it.title(), "Write report");
        assert!(it.has_more_lines());
        assert!(matches_query(&it.text, "CHARTS report"));
        assert!(!matches_query(&it.text, "report slides"));
    }

    #[test]
    fn store_round_trip_and_recovery() {
        let dir = std::env::temp_dir().join(format!("wisp-test-{}", std::process::id()));
        let path = dir.join("data.json");
        let _ = std::fs::remove_dir_all(&dir);
        let (mut store, warn) = Store::load(&path);
        assert!(warn.is_none());
        let id = store.add("hello", 5);
        store.get_mut(id).unwrap().reminders.push(Reminder::new(Rule::BeforeDue { minutes: 10 }));
        store.save().unwrap();
        let (again, _) = Store::load(&path);
        assert_eq!(again.data.items, store.data.items);
        assert!(again.data.next_id > id);

        std::fs::write(&path, "{ not json").unwrap();
        let (fresh, warn) = Store::load(&path);
        assert!(fresh.data.items.is_empty());
        assert!(warn.unwrap().contains("moved"));
        assert!(!path.exists(), "broken file is moved aside, not overwritten");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
