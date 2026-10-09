//! Forgiving date/time input ("tomorrow 9am", "fri 17:00", "+30m", "10-12 14:00",
//! "明天下午3点") and the short labels shown in the UI.
//!
//! Everything is generic over the time zone so tests can pin an offset; the app
//! itself always passes `chrono::Local`.

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike, Weekday};

/// Parses `input` relative to `now`.
///
/// * `Ok(None)` – the input is blank (the field was cleared).
/// * `Ok(Some(t))` – a point in time, truncated to the minute.
/// * `Err(msg)` – a short, user-facing explanation.
///
/// When only a day is given, `default_time` fills in the time of day.
pub fn parse<Tz: TimeZone>(
    input: &str,
    now: &DateTime<Tz>,
    default_time: NaiveTime,
) -> Result<Option<DateTime<Tz>>, String> {
    let normalized = normalize_cjk(&input.trim().to_lowercase());
    let tokens: Vec<String> = normalized
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect();
    if tokens.is_empty() {
        return Ok(None);
    }

    let now_local = truncate_to_minute(now.naive_local());
    let now_exact = now.naive_local();
    // "+30m" means at least 30 minutes from now: round up to the next whole minute.
    let from_now = |minutes: i64| ceil_to_minute(now_exact + Duration::minutes(minutes));
    let today = now_local.date();

    let mut date: Option<NaiveDate> = None;
    let mut time: Option<NaiveTime> = None;
    let mut relative: Option<NaiveDateTime> = None;
    let mut evening = false; // "tonight" → default to 20:00
    let mut meridiem_prefix: Option<Meridiem> = None;

    let mut i = 0;
    while i < tokens.len() {
        let tok = tokens[i].as_str();
        let next = tokens.get(i + 1).map(String::as_str);
        let mut consumed = 1;

        match tok {
            "at" | "on" | "@" | "by" | "the" => {}
            "now" | "现在" => relative = Some(now_local),
            "today" | "tod" => date = Some(today),
            "tonight" => {
                date = Some(today);
                evening = true;
            }
            "tomorrow" | "tmr" | "tmrw" | "tom" | "tomorow" => date = Some(today + Duration::days(1)),
            "overmorrow" => date = Some(today + Duration::days(2)),
            "yesterday" => date = Some(today - Duration::days(1)),
            "noon" | "midday" => time = Some(hm(12, 0)),
            "am-prefix" => meridiem_prefix = Some(Meridiem::Am),
            "pm-prefix" => meridiem_prefix = Some(Meridiem::Pm),
            "noon-prefix" => meridiem_prefix = Some(Meridiem::Noon),
            "next" => {
                let Some(n) = next else { return Err("“next” what? Try “next fri”.".into()) };
                if let Some(wd) = weekday(n) {
                    date = Some(next_weekday(today, wd, true));
                } else if matches!(n, "week" | "wk") {
                    date = Some(today + Duration::days(7));
                } else if n == "month" {
                    date = Some(add_months(today, 1));
                } else {
                    return Err(format!("Couldn't read “next {n}”."));
                }
                consumed = 2;
            }
            "in" | "+" => {
                // "in 30m", "in 2 hours", "in 3 days", "in 45"
                let Some(n) = next else { return Err("“in” how long? Try “in 30m”.".into()) };
                let unit = tokens.get(i + 2).map(String::as_str);
                let (minutes, used) = duration_tokens(n, unit)
                    .or_else(|| n.parse::<i64>().ok().map(|m| (m, 1)))
                    .ok_or_else(|| format!("Couldn't read “in {n}”."))?;
                relative = Some(from_now(minutes));
                consumed = 1 + used;
            }
            _ => {
                if let Some(wd) = weekday(tok) {
                    date = Some(next_weekday(today, wd, false));
                } else if let Some(d) = full_date(tok) {
                    date = Some(d);
                } else if let Some((m, d)) = month_day(tok) {
                    date = Some(upcoming_month_day(today, m, d)?);
                } else if let Some(m) = month_name(tok) {
                    // "oct 12" / "october 12th"
                    let day = next.and_then(day_number).ok_or_else(|| format!("Which day of {tok}?"))?;
                    date = Some(upcoming_month_day(today, m, day)?);
                    consumed = 2;
                } else if let (Some(day), Some(m)) = (day_number(tok), next.and_then(month_name)) {
                    // "12 oct"
                    date = Some(upcoming_month_day(today, m, day)?);
                    consumed = 2;
                } else if let Some(day) = tok.strip_prefix("day").and_then(|d| d.parse::<u32>().ok()) {
                    date = Some(upcoming_day_of_month(today, day)?);
                } else if tok.starts_with('+') || has_unit_suffix(tok) {
                    let rest = tok.trim_start_matches('+');
                    let (minutes, used) = duration_tokens(rest, next)
                        .ok_or_else(|| format!("Couldn't read “{tok}”."))?;
                    relative = Some(from_now(minutes));
                    consumed = used;
                } else if let Some((minutes, 2)) = duration_tokens(tok, next) {
                    // "30 minutes", "2 hours"
                    relative = Some(from_now(minutes));
                    consumed = 2;
                } else if let Some((t, used)) = clock(tok, next, meridiem_prefix.take()) {
                    time = Some(t);
                    consumed = used;
                } else {
                    return Err(format!("Couldn't read “{tok}”."));
                }
            }
        }
        i += consumed;
    }

    if let Some(base) = relative {
        date = date.or(Some(base.date()));
        time = time.or(Some(base.time()));
    }
    let naive = match (date, time) {
        (Some(d), Some(t)) => d.and_time(t),
        (Some(d), None) => d.and_time(if evening { hm(20, 0) } else { default_time }),
        (None, Some(t)) => {
            // A bare time means the next time the clock shows it.
            let candidate = today.and_time(t);
            if candidate > now_local { candidate } else { candidate + Duration::days(1) }
        }
        (None, None) => return Err("Add a day or a time, like “tomorrow 9am”.".into()),
    };
    Ok(Some(resolve_local(&now.timezone(), naive)))
}

/// `2026-10-12 09:30` – the form written back into an input field, which `parse` reads again.
pub fn canonical<Tz: TimeZone>(t: &DateTime<Tz>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    t.format("%Y-%m-%d %H:%M").to_string()
}

/// "Today 14:00", "Tomorrow 09:00", "Fri 17:00", "Oct 12 09:00", "Jan 3 2027 09:00".
pub fn day_label<Tz: TimeZone>(t: &DateTime<Tz>, now: &DateTime<Tz>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let day = t.date_naive();
    let today = now.date_naive();
    let clock = t.format("%H:%M");
    match (day - today).num_days() {
        0 => format!("Today {clock}"),
        1 => format!("Tomorrow {clock}"),
        -1 => format!("Yesterday {clock}"),
        2..=6 => format!("{} {clock}", t.format("%a")),
        _ if day.year() == today.year() => format!("{} {clock}", t.format("%b %-d")),
        _ => format!("{} {clock}", t.format("%b %-d %Y")),
    }
}

/// "in 25 min", "in 3 h", "in 2 days", "4 min ago", "now".
pub fn relative(delta_secs: i64) -> String {
    let future = delta_secs >= 0;
    let s = delta_secs.unsigned_abs();
    let body = if s < 60 {
        return "now".into();
    } else if s < 3600 {
        format!("{} min", s / 60)
    } else if s < 48 * 3600 {
        let h = s / 3600;
        let m = (s % 3600) / 60;
        if h < 10 && m != 0 { format!("{h} h {m} min") } else { format!("{h} h") }
    } else {
        format!("{} days", s / 86_400)
    };
    if future { format!("in {body}") } else { format!("{body} ago") }
}

/// How long ago something was written: "just now", "12 min ago", "3 h ago", "Yesterday", "Oct 3".
pub fn age_label<Tz: TimeZone>(t: &DateTime<Tz>, now: &DateTime<Tz>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let secs = (now.clone() - t.clone()).num_seconds();
    let days = (now.date_naive() - t.date_naive()).num_days();
    if secs < 60 {
        "just now".into()
    } else if secs < 3600 {
        format!("{} min ago", secs / 60)
    } else if days == 0 {
        format!("{} h ago", secs / 3600)
    } else if days == 1 {
        "Yesterday".into()
    } else if t.year() == now.year() {
        t.format("%b %-d").to_string()
    } else {
        t.format("%b %-d %Y").to_string()
    }
}

/// Reads a reminder offset: "15", "15m", "2h", "1h30m", "1 day", "90 min".
pub fn parse_minutes(input: &str) -> Option<i64> {
    let compact: String = input.trim().to_lowercase().split_whitespace().collect();
    if compact.is_empty() {
        return None;
    }
    if let Ok(m) = compact.parse::<i64>() {
        return (m >= 0).then_some(m);
    }
    compound_duration(&normalize_cjk(&compact).replace(' ', ""))
}

/// Writes minutes back the way `parse_minutes` reads them: "15m", "2h", "1h 30m", "1d", "1d 2h".
pub fn format_minutes(minutes: i64) -> String {
    let (d, h, m) = (minutes / 1440, (minutes % 1440) / 60, minutes % 60);
    let parts: Vec<String> = [(d, "d"), (h, "h"), (m, "m")]
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, u)| format!("{n}{u}"))
        .collect();
    if parts.is_empty() { "0m".into() } else { parts.join(" ") }
}

/// Reads a time of day: "9", "09:00", "9am", "9:30 pm", "21:30", "下午3点".
pub fn parse_clock(input: &str) -> Option<NaiveTime> {
    let normalized = normalize_cjk(&input.trim().to_lowercase());
    let tokens: Vec<&str> = normalized.split_whitespace().collect();
    let mut prefix = None;
    let mut rest = &tokens[..];
    match rest.first() {
        Some(&"am-prefix") => prefix = Some(Meridiem::Am),
        Some(&"pm-prefix") => prefix = Some(Meridiem::Pm),
        Some(&"noon-prefix") => prefix = Some(Meridiem::Noon),
        _ => {}
    }
    if prefix.is_some() {
        rest = &rest[1..];
    }
    let (t, used) = clock(rest.first()?, rest.get(1).copied(), prefix)?;
    (used == rest.len()).then_some(t)
}

pub fn hm(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).expect("valid time")
}

/// Maps a wall-clock time to an instant, stepping over DST gaps.
pub fn resolve_local<Tz: TimeZone>(tz: &Tz, naive: NaiveDateTime) -> DateTime<Tz> {
    tz.from_local_datetime(&naive)
        .earliest()
        .or_else(|| tz.from_local_datetime(&(naive + Duration::hours(1))).earliest())
        .unwrap_or_else(|| tz.from_utc_datetime(&naive))
}

// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Meridiem {
    Am,
    Pm,
    Noon,
}

fn truncate_to_minute(t: NaiveDateTime) -> NaiveDateTime {
    t.with_second(0).and_then(|t| t.with_nanosecond(0)).unwrap_or(t)
}

fn ceil_to_minute(t: NaiveDateTime) -> NaiveDateTime {
    let floor = truncate_to_minute(t);
    if floor == t { t } else { floor + Duration::minutes(1) }
}

/// A time of day from `tok` (and maybe an "am"/"pm" in `next`). Returns the time and
/// how many tokens it used.
fn clock(tok: &str, next: Option<&str>, prefix: Option<Meridiem>) -> Option<(NaiveTime, usize)> {
    let (body, mut suffix) = split_meridiem(tok);
    let mut used = 1;
    if suffix.is_none() {
        if let Some(m) = next.and_then(meridiem_word) {
            suffix = Some(m);
            used = 2;
        }
    }
    let (h, m) = match body.split_once(':') {
        Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None if !body.is_empty() && body.len() <= 2 => (body.parse::<u32>().ok()?, 0),
        None if body.len() == 4 && suffix.is_some() => (body[..2].parse().ok()?, body[2..].parse().ok()?),
        None => return None,
    };
    if m > 59 {
        return None;
    }
    let h = match suffix.or(prefix) {
        Some(Meridiem::Am) if (1..=12).contains(&h) => h % 12,
        Some(Meridiem::Pm) if (1..=12).contains(&h) => h % 12 + 12,
        Some(Meridiem::Noon) if (1..=12).contains(&h) => if h <= 2 { h % 12 + 12 } else { h },
        Some(_) if h > 12 && h < 24 => h, // "下午15点" — already 24h
        Some(_) => return None,
        None if h < 24 => h,
        None => return None,
    };
    Some((hm(h, m), used))
}

fn split_meridiem(tok: &str) -> (&str, Option<Meridiem>) {
    for (suffix, m) in [("am", Meridiem::Am), ("pm", Meridiem::Pm), ("a", Meridiem::Am), ("p", Meridiem::Pm)] {
        if let Some(body) = tok.strip_suffix(suffix) {
            if body.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return (body, Some(m));
            }
        }
    }
    (tok, None)
}

fn meridiem_word(tok: &str) -> Option<Meridiem> {
    match tok {
        "am" | "a.m." => Some(Meridiem::Am),
        "pm" | "p.m." => Some(Meridiem::Pm),
        _ => None,
    }
}

fn unit_minutes(unit: &str) -> Option<i64> {
    Some(match unit {
        "m" | "min" | "mins" | "minute" | "minutes" => 1,
        "h" | "hr" | "hrs" | "hour" | "hours" => 60,
        "d" | "day" | "days" => 1440,
        "w" | "wk" | "wks" | "week" | "weeks" => 10080,
        _ => return None,
    })
}

fn has_unit_suffix(tok: &str) -> bool {
    tok.starts_with(|c: char| c.is_ascii_digit()) && compound_duration(tok).is_some() && tok.parse::<i64>().is_err()
}

/// "1h30m" / "90m" / "2d" → minutes.
fn compound_duration(s: &str) -> Option<i64> {
    let mut total = 0i64;
    let mut rest = s;
    if rest.is_empty() {
        return None;
    }
    while !rest.is_empty() {
        let digits = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
        if digits == 0 {
            return None;
        }
        let n: i64 = rest[..digits].parse().ok()?;
        rest = &rest[digits..];
        let unit_len = rest.find(|c: char| c.is_ascii_digit()).unwrap_or(rest.len());
        let unit = unit_minutes(&rest[..unit_len])?;
        rest = &rest[unit_len..];
        total = total.checked_add(n.checked_mul(unit)?)?;
    }
    Some(total)
}

/// A duration spread over one token ("30m") or two ("30", "minutes").
fn duration_tokens(tok: &str, next: Option<&str>) -> Option<(i64, usize)> {
    if let Some(m) = compound_duration(tok) {
        return Some((m, 1));
    }
    let n: i64 = tok.parse().ok()?;
    let unit = unit_minutes(next?)?;
    Some((n.checked_mul(unit)?, 2))
}

fn weekday(tok: &str) -> Option<Weekday> {
    Some(match tok {
        "mon" | "monday" => Weekday::Mon,
        "tue" | "tues" | "tuesday" => Weekday::Tue,
        "wed" | "weds" | "wednesday" => Weekday::Wed,
        "thu" | "thur" | "thurs" | "thursday" => Weekday::Thu,
        "fri" | "friday" => Weekday::Fri,
        "sat" | "saturday" => Weekday::Sat,
        "sun" | "sunday" => Weekday::Sun,
        _ => return None,
    })
}

/// The coming `wd`: today counts unless `strictly_after` ("next fri").
fn next_weekday(today: NaiveDate, wd: Weekday, strictly_after: bool) -> NaiveDate {
    let ahead = (wd.num_days_from_monday() as i64 - today.weekday().num_days_from_monday() as i64).rem_euclid(7);
    let ahead = if ahead == 0 && strictly_after { 7 } else { ahead };
    today + Duration::days(ahead)
}

fn month_name(tok: &str) -> Option<u32> {
    let m = match tok.trim_end_matches('.') {
        "jan" | "january" => 1,
        "feb" | "february" => 2,
        "mar" | "march" => 3,
        "apr" | "april" => 4,
        "may" => 5,
        "jun" | "june" => 6,
        "jul" | "july" => 7,
        "aug" | "august" => 8,
        "sep" | "sept" | "september" => 9,
        "oct" | "october" => 10,
        "nov" | "november" => 11,
        "dec" | "december" => 12,
        _ => return None,
    };
    Some(m)
}

fn day_number(tok: &str) -> Option<u32> {
    let digits = tok.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let suffix = &tok[digits.len()..];
    if !matches!(suffix, "" | "st" | "nd" | "rd" | "th") {
        return None;
    }
    digits.parse().ok().filter(|d| (1..=31).contains(d))
}

/// `2026-10-12`, `2026/10/12`, `2026.10.12`.
fn full_date(tok: &str) -> Option<NaiveDate> {
    let parts: Vec<&str> = tok.split(['-', '/', '.']).collect();
    if parts.len() != 3 || parts[0].len() != 4 {
        return None;
    }
    NaiveDate::from_ymd_opt(parts[0].parse().ok()?, parts[1].parse().ok()?, parts[2].parse().ok()?)
}

/// `10-12` or `10/12` — month first, as in ISO dates (and in Chinese and US usage).
fn month_day(tok: &str) -> Option<(u32, u32)> {
    let (m, d) = tok.split_once(['-', '/'])?;
    if m.is_empty() || d.is_empty() || m.len() > 2 || d.len() > 2 {
        return None;
    }
    Some((m.parse().ok()?, d.parse().ok()?))
}

/// This year's `month/day`, or next year's once it has passed.
fn upcoming_month_day(today: NaiveDate, month: u32, day: u32) -> Result<NaiveDate, String> {
    let this_year = NaiveDate::from_ymd_opt(today.year(), month, day);
    let next_year = NaiveDate::from_ymd_opt(today.year() + 1, month, day);
    match (this_year, next_year) {
        (Some(d), _) if d >= today => Ok(d),
        (_, Some(d)) => Ok(d),
        (Some(d), None) => Ok(d),
        (None, None) => Err(format!("There's no day {month}-{day}.")),
    }
}

/// The coming `day`-th of a month (this month if it hasn't passed).
fn upcoming_day_of_month(today: NaiveDate, day: u32) -> Result<NaiveDate, String> {
    (0..3)
        .filter_map(|ahead| {
            let first = add_months(today.with_day(1)?, ahead);
            first.with_day(day)
        })
        .find(|d| *d >= today)
        .ok_or_else(|| format!("There's no day {day} coming up."))
}

fn add_months(d: NaiveDate, months: u32) -> NaiveDate {
    d.checked_add_months(chrono::Months::new(months)).unwrap_or(d)
}

/// Rewrites common Chinese date words into the English tokens the parser knows:
/// 明天下午3点半 → " tomorrow  pm-prefix 3:30 ".
fn normalize_cjk(s: &str) -> String {
    if s.is_ascii() {
        return s.to_owned();
    }
    const WEEKDAYS: [(&str, &str); 8] = [
        ("一", "mon"),
        ("二", "tue"),
        ("三", "wed"),
        ("四", "thu"),
        ("五", "fri"),
        ("六", "sat"),
        ("日", "sun"),
        ("天", "sun"),
    ];
    const WORDS: &[(&str, &str)] = &[
        ("大后天", " +3d "),
        ("后天", " overmorrow "),
        ("明天", " tomorrow "),
        ("明早", " tomorrow am-prefix "),
        ("明晚", " tomorrow pm-prefix "),
        ("今天", " today "),
        ("今晚", " tonight pm-prefix "),
        ("昨天", " yesterday "),
        ("下个月", " next month "),
        ("下周", " next week "),
        ("下星期", " next week "),
        ("凌晨", " am-prefix "),
        ("早上", " am-prefix "),
        ("上午", " am-prefix "),
        ("中午", " noon-prefix "),
        ("下午", " pm-prefix "),
        ("傍晚", " pm-prefix "),
        ("晚上", " pm-prefix "),
        ("分钟后", " minutes "),
        ("分钟", " minutes "),
        ("小时后", " hours "),
        ("小时", " hours "),
        ("天后", " days "),
        ("现在", " now "),
    ];

    let mut out = s.to_owned();
    // Weekdays first, so "下周一" isn't split up by the "下周" (next week) rule.
    for (prefix, next) in [("下周", true), ("下星期", true), ("下礼拜", true), ("星期", false), ("礼拜", false), ("周", false)] {
        for (zh, en) in WEEKDAYS {
            let replacement = if next { format!(" next {en} ") } else { format!(" {en} ") };
            out = out.replace(&format!("{prefix}{zh}"), &replacement);
        }
    }
    for (zh, en) in WORDS {
        out = out.replace(zh, en);
    }

    // Numeric patterns: 10月12日 / 10月12号 → 10-12, 3点半 → 3:30, 3点15(分) → 3:15, 3点 → 3:00.
    let chars: Vec<char> = out.chars().collect();
    let mut result = String::with_capacity(out.len());
    let mut i = 0;
    let read_num = |from: usize| -> (String, usize) {
        let mut j = from;
        let mut n = String::new();
        while j < chars.len() && chars[j].is_ascii_digit() {
            n.push(chars[j]);
            j += 1;
        }
        (n, j)
    };
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            result.push(chars[i]);
            i += 1;
            continue;
        }
        let (n, j) = read_num(i);
        match chars.get(j) {
            Some('月') => {
                let (d, k) = read_num(j + 1);
                if !d.is_empty() && matches!(chars.get(k), Some('日' | '号')) {
                    result.push_str(&format!(" {n}-{d} "));
                    i = k + 1;
                } else {
                    result.push_str(&n);
                    i = j;
                }
            }
            Some('点' | '时') => {
                if chars.get(j + 1) == Some(&'半') {
                    result.push_str(&format!(" {n}:30 "));
                    i = j + 2;
                } else {
                    let (m, k) = read_num(j + 1);
                    if m.is_empty() {
                        result.push_str(&format!(" {n}:00 "));
                        i = j + 1;
                    } else {
                        result.push_str(&format!(" {n}:{m:0>2} "));
                        i = if chars.get(k) == Some(&'分') { k + 1 } else { k };
                    }
                }
            }
            Some('号' | '日') => {
                // "12号": the coming 12th.
                result.push_str(&format!(" day{n} "));
                i = j + 1;
            }
            _ => {
                result.push_str(&n);
                i = j;
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn tz() -> FixedOffset {
        FixedOffset::east_opt(8 * 3600).unwrap()
    }

    /// Thursday 2026-10-08 10:15 (+08:00)
    fn now() -> DateTime<FixedOffset> {
        tz().with_ymd_and_hms(2026, 10, 8, 10, 15, 42).unwrap()
    }

    fn p(s: &str) -> String {
        match parse(s, &now(), hm(9, 0)) {
            Ok(Some(t)) => t.format("%Y-%m-%d %H:%M").to_string(),
            Ok(None) => "none".into(),
            Err(e) => format!("err: {e}"),
        }
    }

    #[test]
    fn blank_is_none() {
        assert_eq!(p("   "), "none");
    }

    #[test]
    fn day_words() {
        assert_eq!(p("today"), "2026-10-08 09:00");
        assert_eq!(p("tomorrow"), "2026-10-09 09:00");
        assert_eq!(p("Tomorrow 9am"), "2026-10-09 09:00");
        assert_eq!(p("tmr 21:30"), "2026-10-09 21:30");
        assert_eq!(p("tonight"), "2026-10-08 20:00");
        assert_eq!(p("9pm tomorrow"), "2026-10-09 21:00");
        assert_eq!(p("tomorrow at 5 pm"), "2026-10-09 17:00");
    }

    #[test]
    fn weekdays() {
        assert_eq!(p("fri"), "2026-10-09 09:00");
        assert_eq!(p("thu 17:00"), "2026-10-08 17:00");
        assert_eq!(p("next thu"), "2026-10-15 09:00");
        assert_eq!(p("monday 8:30"), "2026-10-12 08:30");
    }

    #[test]
    fn bare_times_roll_forward() {
        assert_eq!(p("17:00"), "2026-10-08 17:00");
        assert_eq!(p("9:00"), "2026-10-09 09:00");
        assert_eq!(p("noon"), "2026-10-08 12:00");
        assert_eq!(p("12am"), "2026-10-09 00:00");
    }

    #[test]
    fn relative_inputs() {
        assert_eq!(p("+30m"), "2026-10-08 10:46");
        assert_eq!(p("in 2 hours"), "2026-10-08 12:16");
        assert_eq!(p("in 90"), "2026-10-08 11:46");
        assert_eq!(p("1h30m"), "2026-10-08 11:46");
        assert_eq!(p("+2d"), "2026-10-10 10:16");
        assert_eq!(p("+1d 9:00"), "2026-10-09 09:00");
        assert_eq!(p("now"), "2026-10-08 10:15");
        assert_eq!(p("45 minutes"), "2026-10-08 11:01");
    }

    #[test]
    fn dates() {
        assert_eq!(p("2026-12-24 18:00"), "2026-12-24 18:00");
        assert_eq!(p("10-12"), "2026-10-12 09:00");
        assert_eq!(p("10/1 14:00"), "2027-10-01 14:00");
        assert_eq!(p("oct 20"), "2026-10-20 09:00");
        assert_eq!(p("20th oct 7pm"), "2026-10-20 19:00");
        assert_eq!(p("Dec 3rd"), "2026-12-03 09:00");
    }

    #[test]
    fn chinese() {
        assert_eq!(p("明天"), "2026-10-09 09:00");
        assert_eq!(p("明天下午3点"), "2026-10-09 15:00");
        assert_eq!(p("后天上午10点半"), "2026-10-10 10:30");
        assert_eq!(p("下周一 9:00"), "2026-10-12 09:00");
        assert_eq!(p("周五晚上8点"), "2026-10-09 20:00");
        assert_eq!(p("10月20日 14点15分"), "2026-10-20 14:15");
        assert_eq!(p("30分钟后"), "2026-10-08 10:46");
        assert_eq!(p("12号"), "2026-10-12 09:00");
        assert_eq!(p("5号 8点"), "2026-11-05 08:00");
        assert_eq!(p("下周"), "2026-10-15 09:00");
    }

    #[test]
    fn errors_are_reported() {
        assert!(p("someday").starts_with("err"));
        assert!(p("25:00").starts_with("err"));
        assert!(p("2026-02-30").starts_with("err"));
        assert!(p("next blah").starts_with("err"));
    }

    #[test]
    fn labels() {
        let n = now();
        let at = |d, h, m| tz().with_ymd_and_hms(2026, 10, d, h, m, 0).unwrap();
        assert_eq!(day_label(&at(8, 14, 0), &n), "Today 14:00");
        assert_eq!(day_label(&at(9, 9, 5), &n), "Tomorrow 09:05");
        assert_eq!(day_label(&at(12, 9, 0), &n), "Mon 09:00");
        assert_eq!(day_label(&at(20, 9, 0), &n), "Oct 20 09:00");
        assert_eq!(day_label(&at(7, 9, 0), &n), "Yesterday 09:00");
        assert_eq!(relative(25 * 60), "in 25 min");
        assert_eq!(relative(-3 * 3600), "3 h ago");
        assert_eq!(relative(3 * 86400 + 5), "in 3 days");
    }

    #[test]
    fn minutes_round_trip() {
        for s in ["15m", "2h", "1h 30m", "1d", "1d 2h"] {
            assert_eq!(format_minutes(parse_minutes(s).unwrap()), s);
        }
        assert_eq!(parse_minutes("90"), Some(90));
        assert_eq!(parse_minutes("1 hour"), Some(60));
        assert_eq!(parse_minutes("2小时"), Some(120));
        assert_eq!(parse_minutes("soon"), None);
    }

    #[test]
    fn clocks() {
        assert_eq!(parse_clock("9"), Some(hm(9, 0)));
        assert_eq!(parse_clock("9:30 pm"), Some(hm(21, 30)));
        assert_eq!(parse_clock("21:05"), Some(hm(21, 5)));
        assert_eq!(parse_clock("下午3点"), Some(hm(15, 0)));
        assert_eq!(parse_clock("noonish"), None);
    }
}
