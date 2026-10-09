//! Date words in the languages Wisp speaks, rewritten into the English ones the
//! parser reads: "demain 14h" → "tomorrow 14:00", "明日の午後3時" → "tomorrow pm-prefix 3:00".
//!
//! Chinese, Japanese, Korean and Russian are always understood: their words can't
//! be mistaken for English ones. Languages written in Latin letters share
//! spellings ("mar" is March in English and Tuesday in Spanish), so of those only
//! the one Wisp is shown in is read. How numbers are written ("20/10" with the day
//! first, "14.30" as a time) also follows the language Wisp is shown in.

/// Rewrites `input` (already lower-cased) for the parser, given the language Wisp is shown in.
pub fn normalize(input: &str, language: &str) -> String {
    let mut s = input.replace(['’', 'ʼ', '`'], "'");
    if !s.is_ascii() {
        s = cjk(&s);
    }
    // Russian words are read always; Russian dates ("20.10") only in Russian.
    if !s.is_ascii() || language == "ru" {
        s = rewrite(&s, &RUSSIAN, language == "ru");
    }
    let table = match language {
        "de" => &GERMAN,
        "es" => &SPANISH,
        "eo" => {
            s = x_system(&s);
            &ESPERANTO
        }
        "fr" => &FRENCH,
        "it" => &ITALIAN,
        "pt_BR" => &PORTUGUESE,
        _ => return s,
    };
    rewrite(&s, table, true)
}

/// Unit words in an amount ("2時間", "15분", "3 дня") → the parser's m/h/d/w.
pub fn amount_units(input: &str) -> String {
    const UNITS: &[(&str, &str)] = &[
        ("小时", "h"),
        ("小時", "h"),
        ("時間", "h"),
        ("时间", "h"),
        ("시간", "h"),
        ("分钟", "m"),
        ("分鐘", "m"),
        ("分", "m"),
        ("분", "m"),
        ("天", "d"),
        ("日", "d"),
        ("일", "d"),
        ("週間", "w"),
        ("周", "w"),
        ("週", "w"),
        ("주", "w"),
    ];
    let mut s = input.to_owned();
    for (word, unit) in UNITS {
        s = s.replace(word, unit);
    }
    s
}

// ---------------------------------------------------------------------------
// Languages written with spaces between words

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dots {
    /// Only "20.10.2026", which every language with the day first reads.
    None,
    /// "20.10." is a date, "14.30" a time and "20." a day of the month.
    German,
    /// "20.10" is a date (times use a colon).
    Russian,
}

struct Table {
    /// Phrases rewritten before single words, longest first.
    phrases: &'static [(&'static str, &'static str)],
    /// Words → English words ("" drops a filler word).
    words: &'static [(&'static str, &'static str)],
    /// Units, which win over `words` right after a number ("2 ore" is two hours, "ora" alone is now).
    units: &'static [(&'static str, &'static str)],
    /// Words for "at": a number after one is a time of day, even with an hour word
    /// after it ("à 14 heures", "в 3 часа"), which would otherwise be a duration.
    at: &'static [&'static str],
    /// Words that mean "in" only before an amount ("em 30 minutos", not "em 20 de outubro").
    in_before_amount: &'static [&'static str],
    /// Endings of ordinal numbers ("1er", "20-an", "1º").
    ordinals: &'static [&'static str],
    /// "14h30" is 14:30.
    h_clock: bool,
    /// "20/10" is the 20th of October.
    day_first: bool,
    dots: Dots,
}

fn lookup(list: &[(&'static str, &'static str)], word: &str) -> Option<&'static str> {
    list.iter().find(|(w, _)| *w == word).map(|&(_, en)| en)
}

fn is_number(word: &str) -> bool {
    !word.is_empty() && word.bytes().all(|b| b.is_ascii_digit())
}

fn is_month(word: &str) -> bool {
    super::month_name(word).is_some()
}

/// What "next" can come before.
fn takes_next(word: &str) -> bool {
    super::weekday(word).is_some() || matches!(word, "week" | "month")
}

/// `table` applied to `s`. Without `structural`, only words are read: numbers keep
/// their English meaning (the Russian words are read whatever Wisp is shown in).
fn rewrite(s: &str, table: &Table, structural: bool) -> String {
    let mut padded = format!(" {} ", s.split_whitespace().collect::<Vec<_>>().join(" "));
    for (phrase, english) in table.phrases {
        padded = padded.replace(&format!(" {phrase} "), &format!(" {english} "));
    }
    let tokens: Vec<&str> = padded.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()).collect();
    let filler = |w: &str| lookup(table.words, w) == Some("");
    // Is the word before `i` (past fillers) one meaning "at"?
    let after_at = |i: usize| {
        for &w in tokens[..i].iter().rev() {
            if table.at.contains(&w) {
                return true;
            }
            if !filler(w) {
                return false;
            }
        }
        false
    };
    // Does a month come next (past fillers)?
    let month_follows = |i: usize| {
        for &w in &tokens[i + 1..] {
            let word = w.trim_end_matches('.');
            if is_month(word) || lookup(table.words, word).is_some_and(is_month) {
                return true;
            }
            if !filler(w) {
                return false;
            }
        }
        false
    };

    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let token = tokens[i];
        let next = tokens.get(i + 1).copied();
        let word = token.trim_end_matches('.');
        let after_number = out.last().is_some_and(|w| is_number(w));

        // "à 14 heures", "в 3 часа": a time of day.
        if is_number(token) && after_at(i) && next.is_some_and(|n| lookup(table.units, n) == Some("h")) {
            out.push(format!("{token}:00"));
            i += 2;
            continue;
        }
        // "1er", "20-an", "20.": a day of the month, or after "at" (Esperanto's "je la 9a") an hour.
        let ordinal = table
            .ordinals
            .iter()
            .find_map(|end| token.strip_suffix(end).filter(|n| is_number(n)))
            .or_else(|| (table.dots == Dots::German && structural).then(|| token.strip_suffix('.').filter(|n| is_number(n))).flatten());
        if let Some(n) = ordinal {
            out.push(if after_at(i) {
                format!("{n}:00")
            } else if month_follows(i) {
                n.to_owned()
            } else {
                format!("day{n}")
            });
            i += 1;
            continue;
        }
        if structural {
            if let Some(form) = number_form(token, table, out.last().map(String::as_str)) {
                out.push(form);
                i += 1;
                continue;
            }
        }
        if after_number {
            if let Some(unit) = lookup(table.units, token).or_else(|| lookup(table.units, word)) {
                out.push(unit.to_owned());
                i += 1;
                continue;
            }
        }
        if table.in_before_amount.contains(&token) {
            let amount = next.is_some_and(is_number) && tokens.get(i + 2).is_some_and(|u| lookup(table.units, u).is_some());
            if amount {
                out.push("in".to_owned());
            }
            i += 1;
            continue;
        }
        match lookup(table.words, token).or_else(|| lookup(table.words, word)) {
            Some("") => {}
            Some(english) => out.extend(english.split(' ').map(str::to_owned)),
            None => out.push(token.to_owned()),
        }
        i += 1;
    }
    // "lundi prochain", "semana próxima": "next" goes before what it's about.
    for k in 1..out.len() {
        if out[k] == "next" && takes_next(&out[k - 1]) && (k < 2 || out[k - 2] != "next") {
            out.swap(k - 1, k);
        }
    }
    out.join(" ")
}

/// Numbers written the language's way: "14h30", "20/10", "20.10.2026", "14.30".
fn number_form(token: &str, table: &Table, previous: Option<&str>) -> Option<String> {
    let digits = |s: &str| is_number(s) && s.len() <= 2;
    if table.h_clock && previous != Some("in") {
        if let Some((h, m)) = token.split_once('h') {
            if digits(h) && (m.is_empty() || (m.len() == 2 && is_number(m))) {
                return Some(format!("{h}:{}", if m.is_empty() { "00" } else { m }));
            }
        }
    }
    let parts: Vec<&str> = token.split(['/', '.']).collect();
    let slash = token.contains('/');
    match parts.as_slice() {
        [d, m, y] if table.day_first && digits(d) && digits(m) && y.len() == 4 && is_number(y) => Some(format!("{y}-{m}-{d}")),
        [d, m] if slash && table.day_first && digits(d) && digits(m) => Some(format!("{m}-{d}")),
        [d, m, ""] if !slash && table.dots == Dots::German && digits(d) && digits(m) => Some(format!("{m}-{d}")),
        [h, m] if !slash && table.dots == Dots::German && digits(h) && m.len() == 2 && is_number(m) => Some(format!("{h}:{m}")),
        [d, m] if !slash && table.dots == Dots::Russian && digits(d) && digits(m) => Some(format!("{m}-{d}")),
        _ => None,
    }
}

/// Esperanto typed without its letters: "morgaux" → "morgaŭ".
fn x_system(s: &str) -> String {
    [("cx", "ĉ"), ("gx", "ĝ"), ("hx", "ĥ"), ("jx", "ĵ"), ("sx", "ŝ"), ("ux", "ŭ")]
        .iter()
        .fold(s.to_owned(), |s, (x, letter)| s.replace(x, letter))
}

const FRENCH: Table = Table {
    phrases: &[
        ("dans une minute", "in 1 min"),
        ("dans une heure", "in 1 h"),
        ("dans un jour", "in 1 d"),
        ("dans une semaine", "in 1 week"),
        ("la semaine prochaine", "next week"),
        ("le mois prochain", "next month"),
        ("de l'après-midi", "pm"),
        ("de l'apres-midi", "pm"),
        ("ce soir", "tonight"),
        ("cette nuit", "tonight"),
        ("du matin", "am"),
        ("du soir", "pm"),
        ("à midi", "noon"),
    ],
    words: &[
        ("aujourd'hui", "today"),
        ("aujourdhui", "today"),
        ("demain", "tomorrow"),
        ("après-demain", "overmorrow"),
        ("apres-demain", "overmorrow"),
        ("hier", "yesterday"),
        ("maintenant", "now"),
        ("midi", "noon"),
        ("minuit", "0:00"),
        ("matin", "am-prefix"),
        ("soir", "pm-prefix"),
        ("après-midi", "pm-prefix"),
        ("apres-midi", "pm-prefix"),
        ("l'après-midi", "pm-prefix"),
        ("lundi", "mon"),
        ("lun", "mon"),
        ("mardi", "tue"),
        ("mercredi", "wed"),
        ("mer", "wed"),
        ("jeudi", "thu"),
        ("jeu", "thu"),
        ("vendredi", "fri"),
        ("ven", "fri"),
        ("samedi", "sat"),
        ("sam", "sat"),
        ("dimanche", "sun"),
        ("dim", "sun"),
        ("prochain", "next"),
        ("prochaine", "next"),
        ("semaine", "week"),
        ("mois", "month"),
        ("dans", "in"),
        ("à", ""),
        ("a", ""),
        ("le", ""),
        ("la", ""),
        ("l'", ""),
        ("du", ""),
        ("de", ""),
        ("vers", ""),
        ("pour", ""),
        ("janvier", "jan"),
        ("janv", "jan"),
        ("février", "feb"),
        ("fevrier", "feb"),
        ("févr", "feb"),
        ("fevr", "feb"),
        ("mars", "mar"),
        ("avril", "apr"),
        ("avr", "apr"),
        ("mai", "may"),
        ("juin", "jun"),
        ("juillet", "jul"),
        ("juil", "jul"),
        ("août", "aug"),
        ("aout", "aug"),
        ("septembre", "sep"),
        ("sept", "sep"),
        ("octobre", "oct"),
        ("novembre", "nov"),
        ("décembre", "dec"),
        ("decembre", "dec"),
        ("déc", "dec"),
    ],
    units: &[
        ("minute", "min"),
        ("minutes", "min"),
        ("min", "min"),
        ("mn", "min"),
        ("heure", "h"),
        ("heures", "h"),
        ("h", "h"),
        ("jour", "d"),
        ("jours", "d"),
        ("j", "d"),
        ("semaine", "week"),
        ("semaines", "week"),
        ("sem", "week"),
    ],
    at: &["à", "a", "vers"],
    in_before_amount: &[],
    ordinals: &["er", "re", "ème", "eme", "e"],
    h_clock: true,
    day_first: true,
    dots: Dots::None,
};

const GERMAN: Table = Table {
    phrases: &[
        ("in einer minute", "in 1 min"),
        ("in einer stunde", "in 1 h"),
        ("in einem tag", "in 1 d"),
        ("in einer woche", "in 1 week"),
        ("heute abend", "tonight"),
        ("heute nacht", "tonight"),
    ],
    words: &[
        ("heute", "today"),
        ("morgen", "tomorrow"),
        ("übermorgen", "overmorrow"),
        ("uebermorgen", "overmorrow"),
        ("gestern", "yesterday"),
        ("jetzt", "now"),
        ("mittag", "noon"),
        ("mittags", "noon"),
        ("mitternacht", "0:00"),
        ("früh", "am-prefix"),
        ("frueh", "am-prefix"),
        ("morgens", "am-prefix"),
        ("vormittag", "am-prefix"),
        ("vormittags", "am-prefix"),
        ("nachmittag", "pm-prefix"),
        ("nachmittags", "pm-prefix"),
        ("abend", "pm-prefix"),
        ("abends", "pm-prefix"),
        ("montag", "mon"),
        ("mo", "mon"),
        ("dienstag", "tue"),
        ("di", "tue"),
        ("mittwoch", "wed"),
        ("mi", "wed"),
        ("donnerstag", "thu"),
        ("do", "thu"),
        ("freitag", "fri"),
        ("fr", "fri"),
        ("samstag", "sat"),
        ("sonnabend", "sat"),
        ("sa", "sat"),
        ("sonntag", "sun"),
        ("so", "sun"),
        ("nächsten", "next"),
        ("nächster", "next"),
        ("nächste", "next"),
        ("nächstes", "next"),
        ("naechsten", "next"),
        ("naechster", "next"),
        ("naechste", "next"),
        ("kommenden", "next"),
        ("kommender", "next"),
        ("kommende", "next"),
        ("woche", "week"),
        ("monat", "month"),
        ("um", ""),
        ("am", ""),
        ("ab", ""),
        ("bis", ""),
        ("gegen", ""),
        ("uhr", ""),
        ("den", ""),
        ("dem", ""),
        ("der", ""),
        ("januar", "jan"),
        ("jänner", "jan"),
        ("februar", "feb"),
        ("märz", "mar"),
        ("maerz", "mar"),
        ("mär", "mar"),
        ("mrz", "mar"),
        ("mai", "may"),
        ("juni", "jun"),
        ("juli", "jul"),
        ("oktober", "oct"),
        ("okt", "oct"),
        ("dezember", "dec"),
        ("dez", "dec"),
    ],
    units: &[
        ("minute", "min"),
        ("minuten", "min"),
        ("min", "min"),
        ("stunde", "h"),
        ("stunden", "h"),
        ("std", "h"),
        ("h", "h"),
        ("tag", "d"),
        ("tage", "d"),
        ("tagen", "d"),
        ("woche", "week"),
        ("wochen", "week"),
    ],
    at: &["um", "gegen"],
    in_before_amount: &[],
    ordinals: &[],
    h_clock: false,
    day_first: true,
    dots: Dots::German,
};

const SPANISH: Table = Table {
    phrases: &[
        ("en un minuto", "in 1 min"),
        ("en una hora", "in 1 h"),
        ("en un día", "in 1 d"),
        ("en un dia", "in 1 d"),
        ("en una semana", "in 1 week"),
        ("pasado mañana", "overmorrow"),
        ("pasado manana", "overmorrow"),
        ("la semana que viene", "next week"),
        ("el mes que viene", "next month"),
        ("de la mañana", "am"),
        ("de la manana", "am"),
        ("por la mañana", "am-prefix"),
        ("por la manana", "am-prefix"),
        ("de la tarde", "pm"),
        ("por la tarde", "pm-prefix"),
        ("de la noche", "pm"),
        ("por la noche", "pm-prefix"),
        ("esta noche", "tonight"),
        ("esta tarde", "today pm-prefix"),
        ("al mediodía", "noon"),
        ("al mediodia", "noon"),
        ("dentro de", "in"),
        ("que viene", "next"),
    ],
    words: &[
        ("hoy", "today"),
        ("mañana", "tomorrow"),
        ("manana", "tomorrow"),
        ("ayer", "yesterday"),
        ("ahora", "now"),
        ("mediodía", "noon"),
        ("mediodia", "noon"),
        ("medianoche", "0:00"),
        ("lunes", "mon"),
        ("lun", "mon"),
        ("martes", "tue"),
        ("miércoles", "wed"),
        ("miercoles", "wed"),
        ("mié", "wed"),
        ("mie", "wed"),
        ("jueves", "thu"),
        ("jue", "thu"),
        ("viernes", "fri"),
        ("vie", "fri"),
        ("sábado", "sat"),
        ("sabado", "sat"),
        ("sáb", "sat"),
        ("sab", "sat"),
        ("domingo", "sun"),
        ("dom", "sun"),
        ("próximo", "next"),
        ("próxima", "next"),
        ("proximo", "next"),
        ("proxima", "next"),
        ("semana", "week"),
        ("mes", "month"),
        ("a", ""),
        ("las", ""),
        ("la", ""),
        ("el", ""),
        ("los", ""),
        ("de", ""),
        ("del", ""),
        ("para", ""),
        ("hacia", ""),
        ("enero", "jan"),
        ("ene", "jan"),
        ("febrero", "feb"),
        ("marzo", "mar"),
        ("abril", "apr"),
        ("abr", "apr"),
        ("mayo", "may"),
        ("junio", "jun"),
        ("julio", "jul"),
        ("agosto", "aug"),
        ("ago", "aug"),
        ("septiembre", "sep"),
        ("setiembre", "sep"),
        ("octubre", "oct"),
        ("noviembre", "nov"),
        ("diciembre", "dec"),
        ("dic", "dec"),
    ],
    units: &[
        ("minuto", "min"),
        ("minutos", "min"),
        ("min", "min"),
        ("hora", "h"),
        ("horas", "h"),
        ("h", "h"),
        ("día", "d"),
        ("días", "d"),
        ("dia", "d"),
        ("dias", "d"),
        ("semana", "week"),
        ("semanas", "week"),
    ],
    at: &["a", "las", "la"],
    in_before_amount: &["en"],
    ordinals: &["º", "°"],
    h_clock: false,
    day_first: true,
    dots: Dots::None,
};

const PORTUGUESE: Table = Table {
    phrases: &[
        ("daqui a uma hora", "in 1 h"),
        ("em uma hora", "in 1 h"),
        ("daqui a um dia", "in 1 d"),
        ("em uma semana", "in 1 week"),
        ("depois de amanhã", "overmorrow"),
        ("depois de amanha", "overmorrow"),
        ("hoje à noite", "tonight"),
        ("hoje a noite", "tonight"),
        ("hoje de noite", "tonight"),
        ("semana que vem", "next week"),
        ("mês que vem", "next month"),
        ("mes que vem", "next month"),
        ("de manhã", "am"),
        ("da manhã", "am"),
        ("de manha", "am"),
        ("da manha", "am"),
        ("da tarde", "pm"),
        ("à tarde", "pm-prefix"),
        ("de tarde", "pm-prefix"),
        ("da noite", "pm"),
        ("à noite", "pm-prefix"),
        ("de noite", "pm-prefix"),
        ("meio dia", "noon"),
        ("daqui a", "in"),
        ("que vem", "next"),
    ],
    words: &[
        ("hoje", "today"),
        ("amanhã", "tomorrow"),
        ("amanha", "tomorrow"),
        ("ontem", "yesterday"),
        ("agora", "now"),
        ("meio-dia", "noon"),
        ("meia-noite", "0:00"),
        ("segunda", "mon"),
        ("segunda-feira", "mon"),
        ("seg", "mon"),
        ("terça", "tue"),
        ("terça-feira", "tue"),
        ("terca", "tue"),
        ("terca-feira", "tue"),
        ("ter", "tue"),
        ("quarta", "wed"),
        ("quarta-feira", "wed"),
        ("qua", "wed"),
        ("quinta", "thu"),
        ("quinta-feira", "thu"),
        ("qui", "thu"),
        ("sexta", "fri"),
        ("sexta-feira", "fri"),
        ("sex", "fri"),
        ("sábado", "sat"),
        ("sabado", "sat"),
        ("sáb", "sat"),
        ("sab", "sat"),
        ("domingo", "sun"),
        ("dom", "sun"),
        ("próximo", "next"),
        ("próxima", "next"),
        ("proximo", "next"),
        ("proxima", "next"),
        ("semana", "week"),
        ("mês", "month"),
        ("mes", "month"),
        ("às", ""),
        ("as", ""),
        ("a", ""),
        ("o", ""),
        ("de", ""),
        ("do", ""),
        ("da", ""),
        ("no", ""),
        ("na", ""),
        ("para", ""),
        ("até", ""),
        ("ate", ""),
        ("janeiro", "jan"),
        ("fevereiro", "feb"),
        ("fev", "feb"),
        ("março", "mar"),
        ("marco", "mar"),
        ("abril", "apr"),
        ("abr", "apr"),
        ("maio", "may"),
        ("mai", "may"),
        ("junho", "jun"),
        ("julho", "jul"),
        ("agosto", "aug"),
        ("ago", "aug"),
        ("setembro", "sep"),
        ("set", "sep"),
        ("outubro", "oct"),
        ("out", "oct"),
        ("novembro", "nov"),
        ("dezembro", "dec"),
        ("dez", "dec"),
    ],
    units: &[
        ("minuto", "min"),
        ("minutos", "min"),
        ("min", "min"),
        ("hora", "h"),
        ("horas", "h"),
        ("h", "h"),
        ("dia", "d"),
        ("dias", "d"),
        ("semana", "week"),
        ("semanas", "week"),
    ],
    at: &["às", "as", "a"],
    in_before_amount: &["em"],
    ordinals: &["º", "°"],
    h_clock: true,
    day_first: true,
    dots: Dots::None,
};

const ITALIAN: Table = Table {
    phrases: &[
        ("tra un minuto", "in 1 min"),
        ("fra un minuto", "in 1 min"),
        ("tra un'ora", "in 1 h"),
        ("fra un'ora", "in 1 h"),
        ("tra un giorno", "in 1 d"),
        ("fra un giorno", "in 1 d"),
        ("tra una settimana", "in 1 week"),
        ("fra una settimana", "in 1 week"),
        ("di mattina", "am"),
        ("del mattino", "am"),
        ("di pomeriggio", "pm"),
        ("del pomeriggio", "pm"),
        ("di sera", "pm"),
        ("della sera", "pm"),
        ("all'una", "1:00"),
    ],
    words: &[
        ("oggi", "today"),
        ("domani", "tomorrow"),
        ("dopodomani", "overmorrow"),
        ("ieri", "yesterday"),
        ("adesso", "now"),
        ("ora", "now"),
        ("stasera", "tonight"),
        ("stanotte", "tonight"),
        ("stamattina", "today am-prefix"),
        ("stamani", "today am-prefix"),
        ("mattina", "am-prefix"),
        ("mattino", "am-prefix"),
        ("pomeriggio", "pm-prefix"),
        ("sera", "pm-prefix"),
        ("mezzogiorno", "noon"),
        ("mezzanotte", "0:00"),
        ("lunedì", "mon"),
        ("lunedi", "mon"),
        ("lun", "mon"),
        ("martedì", "tue"),
        ("martedi", "tue"),
        ("mercoledì", "wed"),
        ("mercoledi", "wed"),
        ("mer", "wed"),
        ("giovedì", "thu"),
        ("giovedi", "thu"),
        ("gio", "thu"),
        ("venerdì", "fri"),
        ("venerdi", "fri"),
        ("ven", "fri"),
        ("sabato", "sat"),
        ("sab", "sat"),
        ("domenica", "sun"),
        ("dom", "sun"),
        ("prossimo", "next"),
        ("prossima", "next"),
        ("settimana", "week"),
        ("mese", "month"),
        ("tra", "in"),
        ("fra", "in"),
        ("alle", ""),
        ("all'", ""),
        ("a", ""),
        ("il", ""),
        ("lo", ""),
        ("la", ""),
        ("le", ""),
        ("di", ""),
        ("del", ""),
        ("della", ""),
        ("per", ""),
        ("entro", ""),
        ("gennaio", "jan"),
        ("gen", "jan"),
        ("febbraio", "feb"),
        ("marzo", "mar"),
        ("aprile", "apr"),
        ("maggio", "may"),
        ("mag", "may"),
        ("giugno", "jun"),
        ("giu", "jun"),
        ("luglio", "jul"),
        ("lug", "jul"),
        ("agosto", "aug"),
        ("ago", "aug"),
        ("settembre", "sep"),
        ("set", "sep"),
        ("ottobre", "oct"),
        ("ott", "oct"),
        ("novembre", "nov"),
        ("dicembre", "dec"),
        ("dic", "dec"),
    ],
    units: &[
        ("minuto", "min"),
        ("minuti", "min"),
        ("min", "min"),
        ("ora", "h"),
        ("ore", "h"),
        ("h", "h"),
        ("giorno", "d"),
        ("giorni", "d"),
        ("settimana", "week"),
        ("settimane", "week"),
    ],
    at: &["alle", "le"],
    in_before_amount: &[],
    ordinals: &["°", "º"],
    h_clock: false,
    day_first: true,
    dots: Dots::None,
};

const ESPERANTO: Table = Table {
    phrases: &[
        ("post unu minuto", "in 1 min"),
        ("post unu horo", "in 1 h"),
        ("post unu tago", "in 1 d"),
        ("post unu semajno", "in 1 week"),
        ("ĉi vespere", "tonight"),
        ("ĉi nokte", "tonight"),
    ],
    words: &[
        ("hodiaŭ", "today"),
        ("hodiau", "today"),
        ("morgaŭ", "tomorrow"),
        ("morgau", "tomorrow"),
        ("postmorgaŭ", "overmorrow"),
        ("postmorgau", "overmorrow"),
        ("hieraŭ", "yesterday"),
        ("hierau", "yesterday"),
        ("nun", "now"),
        ("ĉi-vespere", "tonight"),
        ("ĉi-nokte", "tonight"),
        ("tagmeze", "noon"),
        ("tagmezo", "noon"),
        ("noktomeze", "0:00"),
        ("matene", "am-prefix"),
        ("vespere", "pm-prefix"),
        ("posttagmeze", "pm-prefix"),
        ("lundo", "mon"),
        ("lundon", "mon"),
        ("lu", "mon"),
        ("mardo", "tue"),
        ("mardon", "tue"),
        ("ma", "tue"),
        ("merkredo", "wed"),
        ("merkredon", "wed"),
        ("me", "wed"),
        ("ĵaŭdo", "thu"),
        ("ĵaŭdon", "thu"),
        ("jaudo", "thu"),
        ("jaudon", "thu"),
        ("ĵa", "thu"),
        ("vendredo", "fri"),
        ("vendredon", "fri"),
        ("ve", "fri"),
        ("sabato", "sat"),
        ("sabaton", "sat"),
        ("sa", "sat"),
        ("dimanĉo", "sun"),
        ("dimanĉon", "sun"),
        ("dimanco", "sun"),
        ("dimancon", "sun"),
        ("di", "sun"),
        ("venonta", "next"),
        ("venontan", "next"),
        ("sekva", "next"),
        ("sekvan", "next"),
        ("semajno", "week"),
        ("semajnon", "week"),
        ("monato", "month"),
        ("monaton", "month"),
        ("post", "in"),
        ("unu", "1"),
        ("je", ""),
        ("la", ""),
        ("en", ""),
        ("ĝis", ""),
        ("gis", ""),
        ("de", ""),
        ("januaro", "jan"),
        ("januaron", "jan"),
        ("februaro", "feb"),
        ("februaron", "feb"),
        ("marto", "mar"),
        ("marton", "mar"),
        ("aprilo", "apr"),
        ("aprilon", "apr"),
        ("majo", "may"),
        ("majon", "may"),
        ("junio", "jun"),
        ("junion", "jun"),
        ("julio", "jul"),
        ("julion", "jul"),
        ("aŭgusto", "aug"),
        ("aŭguston", "aug"),
        ("augusto", "aug"),
        ("aŭg", "aug"),
        ("septembro", "sep"),
        ("septembron", "sep"),
        ("oktobro", "oct"),
        ("oktobron", "oct"),
        ("okt", "oct"),
        ("novembro", "nov"),
        ("novembron", "nov"),
        ("decembro", "dec"),
        ("decembron", "dec"),
    ],
    units: &[
        ("minuto", "min"),
        ("minutoj", "min"),
        ("minutojn", "min"),
        ("min", "min"),
        ("horo", "h"),
        ("horoj", "h"),
        ("horojn", "h"),
        ("h", "h"),
        ("tago", "d"),
        ("tagoj", "d"),
        ("tagojn", "d"),
        ("semajno", "week"),
        ("semajnoj", "week"),
        ("semajnojn", "week"),
    ],
    at: &["je"],
    in_before_amount: &[],
    ordinals: &["-an", "-a", "an", "a"],
    h_clock: false,
    day_first: true,
    dots: Dots::None,
};

/// Read whatever Wisp is shown in: Cyrillic can't be mistaken for anything else.
const RUSSIAN: Table = Table {
    phrases: &[
        ("через минуту", "in 1 min"),
        ("через час", "in 1 h"),
        ("через день", "in 1 d"),
        ("через неделю", "in 1 week"),
        ("сегодня вечером", "tonight"),
        ("на следующей неделе", "next week"),
        ("в следующем месяце", "next month"),
        ("в полдень", "noon"),
        ("в полночь", "0:00"),
    ],
    words: &[
        ("сегодня", "today"),
        ("завтра", "tomorrow"),
        ("послезавтра", "overmorrow"),
        ("вчера", "yesterday"),
        ("сейчас", "now"),
        ("полдень", "noon"),
        ("полночь", "0:00"),
        ("утром", "am-prefix"),
        ("утра", "am"),
        ("ночи", "am"),
        ("днём", "pm-prefix"),
        ("днем", "pm-prefix"),
        ("дня", "pm"),
        ("вечером", "pm-prefix"),
        ("вечера", "pm"),
        ("понедельник", "mon"),
        ("пн", "mon"),
        ("вторник", "tue"),
        ("вт", "tue"),
        ("среда", "wed"),
        ("среду", "wed"),
        ("ср", "wed"),
        ("четверг", "thu"),
        ("чт", "thu"),
        ("пятница", "fri"),
        ("пятницу", "fri"),
        ("пт", "fri"),
        ("суббота", "sat"),
        ("субботу", "sat"),
        ("сб", "sat"),
        ("воскресенье", "sun"),
        ("вс", "sun"),
        ("следующий", "next"),
        ("следующую", "next"),
        ("следующая", "next"),
        ("следующее", "next"),
        ("следующей", "next"),
        ("следующем", "next"),
        ("следующего", "next"),
        ("неделя", "week"),
        ("неделю", "week"),
        ("неделе", "week"),
        ("месяц", "month"),
        ("через", "in"),
        ("в", ""),
        ("во", ""),
        ("на", ""),
        ("к", ""),
        ("до", ""),
        ("января", "jan"),
        ("январь", "jan"),
        ("янв", "jan"),
        ("февраля", "feb"),
        ("февраль", "feb"),
        ("фев", "feb"),
        ("марта", "mar"),
        ("март", "mar"),
        ("мар", "mar"),
        ("апреля", "apr"),
        ("апрель", "apr"),
        ("апр", "apr"),
        ("мая", "may"),
        ("май", "may"),
        ("июня", "jun"),
        ("июнь", "jun"),
        ("июн", "jun"),
        ("июля", "jul"),
        ("июль", "jul"),
        ("июл", "jul"),
        ("августа", "aug"),
        ("август", "aug"),
        ("авг", "aug"),
        ("сентября", "sep"),
        ("сентябрь", "sep"),
        ("сент", "sep"),
        ("сен", "sep"),
        ("октября", "oct"),
        ("октябрь", "oct"),
        ("окт", "oct"),
        ("ноября", "nov"),
        ("ноябрь", "nov"),
        ("ноя", "nov"),
        ("декабря", "dec"),
        ("декабрь", "dec"),
        ("дек", "dec"),
    ],
    units: &[
        ("минута", "min"),
        ("минуты", "min"),
        ("минут", "min"),
        ("минуту", "min"),
        ("мин", "min"),
        ("час", "h"),
        ("часа", "h"),
        ("часов", "h"),
        ("ч", "h"),
        ("день", "d"),
        ("дня", "d"),
        ("дней", "d"),
        ("дн", "d"),
        ("неделя", "week"),
        ("неделю", "week"),
        ("недели", "week"),
        ("недель", "week"),
        ("нед", "week"),
    ],
    at: &["в", "к"],
    in_before_amount: &[],
    ordinals: &["-го", "-ого", "-е"],
    h_clock: false,
    day_first: true,
    dots: Dots::Russian,
};

// ---------------------------------------------------------------------------
// Chinese, Japanese and Korean, written without spaces between words

/// Traditional Chinese (and Japanese) forms of the characters the rules read → simplified.
fn simplify(c: char) -> char {
    match c {
        '點' => '点',
        '後' => '后',
        '個' => '个',
        '禮' => '礼',
        '鐘' => '钟',
        '號' => '号',
        '時' => '时',
        '現' => '现',
        '週' => '周',
        '間' => '间',
        '晩' => '晚',
        '來' => '来',
        c => c,
    }
}

const JA_WEEKDAYS: [(&str, &str); 14] = [
    ("月曜日", "mon"),
    ("火曜日", "tue"),
    ("水曜日", "wed"),
    ("木曜日", "thu"),
    ("金曜日", "fri"),
    ("土曜日", "sat"),
    ("日曜日", "sun"),
    ("月曜", "mon"),
    ("火曜", "tue"),
    ("水曜", "wed"),
    ("木曜", "thu"),
    ("金曜", "fri"),
    ("土曜", "sat"),
    ("日曜", "sun"),
];

/// Japanese, after `simplify` (後 is 后, 時 is 时, 週 is 周).
const JAPANESE: &[(&str, &str)] = &[
    ("明后日", " overmorrow "),
    ("明日", " tomorrow "),
    ("今日", " today "),
    ("昨日", " yesterday "),
    ("今夜", " tonight "),
    ("今晚", " tonight "),
    ("来周", " next week "),
    ("来月", " next month "),
    ("午前", " am-prefix "),
    ("午后", " pm-prefix "),
    ("正午", " noon "),
    ("夕方", " pm-prefix "),
    ("朝", " am-prefix "),
    ("夜", " pm-prefix "),
    ("分后", " minutes "),
    ("时间后", " hours "),
    ("时间", " hours "),
    ("周间后", " weeks "),
    ("日后", " days "),
    ("ごろ", " "),
    ("頃", " "),
    ("の", " "),
    ("に", " "),
];

const KO_WEEKDAYS: [(&str, &str); 7] = [
    ("월요일", "mon"),
    ("화요일", "tue"),
    ("수요일", "wed"),
    ("목요일", "thu"),
    ("금요일", "fri"),
    ("토요일", "sat"),
    ("일요일", "sun"),
];

const KOREAN: &[(&str, &str)] = &[
    ("다음 주", " next week "),
    ("다음주", " next week "),
    ("다음 달", " next month "),
    ("다음달", " next month "),
    ("오늘 밤", " tonight "),
    ("오늘밤", " tonight "),
    ("모레", " overmorrow "),
    ("내일", " tomorrow "),
    ("오늘", " today "),
    ("어제", " yesterday "),
    ("지금", " now "),
    ("오전", " am-prefix "),
    ("오후", " pm-prefix "),
    ("정오", " noon "),
    ("아침", " am-prefix "),
    ("저녁", " pm-prefix "),
    ("밤", " pm-prefix "),
    ("분 후", " minutes "),
    ("분후", " minutes "),
    ("시간 후", " hours "),
    ("시간후", " hours "),
    ("시간", " hours "),
    ("일 후", " days "),
    ("일후", " days "),
    ("주 후", " weeks "),
    ("주후", " weeks "),
    ("까지", " "),
    ("에", " "),
];

const ZH_WEEKDAYS: [(&str, &str); 8] = [
    ("一", "mon"),
    ("二", "tue"),
    ("三", "wed"),
    ("四", "thu"),
    ("五", "fri"),
    ("六", "sat"),
    ("日", "sun"),
    ("天", "sun"),
];

const CHINESE: &[(&str, &str)] = &[
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

/// Rewrites Chinese, Japanese and Korean date words into the English tokens the
/// parser knows: 明天下午3点半 → " tomorrow  pm-prefix 3:30 ".
fn cjk(s: &str) -> String {
    let mut out: String = s.chars().map(simplify).collect();
    // Weekdays first, so "来週月曜" and "下周一" aren't split up by the "next week" words.
    for (prefix, next) in [("来周の", true), ("来周", true), ("今周の", false), ("今周", false)] {
        for (ja, en) in JA_WEEKDAYS {
            out = out.replace(&format!("{prefix}{ja}"), &format!(" {}{en} ", if next { "next " } else { "" }));
        }
    }
    for (ja, en) in JA_WEEKDAYS {
        out = out.replace(ja, &format!(" {en} "));
    }
    for prefix in ["다음 주 ", "다음주 ", "다음 주", "다음주"] {
        for (ko, en) in KO_WEEKDAYS {
            out = out.replace(&format!("{prefix}{ko}"), &format!(" next {en} "));
        }
    }
    for (ko, en) in KO_WEEKDAYS {
        out = out.replace(ko, &format!(" {en} "));
    }
    for (prefix, next) in [("下周", true), ("下星期", true), ("下礼拜", true), ("星期", false), ("礼拜", false), ("周", false)] {
        for (zh, en) in ZH_WEEKDAYS {
            let replacement = if next { format!(" next {en} ") } else { format!(" {en} ") };
            out = out.replace(&format!("{prefix}{zh}"), &replacement);
        }
    }
    for (word, english) in JAPANESE.iter().chain(KOREAN).chain(CHINESE) {
        out = out.replace(word, english);
    }

    // Numbers: 10月12日 / 10월 12일 → 10-12, 3点半 / 3시 반 → 3:30, 3点15(分) / 3시 15분 → 3:15,
    // 3点 / 3시 → 3:00, 12号 / 12日 / 12일 → the coming 12th, 15分 / 15분 → 15 minutes.
    let chars: Vec<char> = out.chars().collect();
    let skip_spaces = |mut j: usize| {
        while chars.get(j) == Some(&' ') {
            j += 1;
        }
        j
    };
    let digits_at = |mut j: usize| {
        let mut n = String::new();
        while let Some(c) = chars.get(j).filter(|c| c.is_ascii_digit()) {
            n.push(*c);
            j += 1;
        }
        (n, j)
    };
    let mut result = String::with_capacity(out.len());
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            result.push(chars[i]);
            i += 1;
            continue;
        }
        let (n, j) = digits_at(i);
        match chars.get(j) {
            Some('月' | '월') => {
                let (d, k) = digits_at(skip_spaces(j + 1));
                if !d.is_empty() && matches!(chars.get(k), Some('日' | '号' | '일')) {
                    result.push_str(&format!(" {n}-{d} "));
                    i = k + 1;
                } else {
                    result.push_str(&n);
                    i = j;
                }
            }
            Some('点' | '时' | '시') => {
                let after = skip_spaces(j + 1);
                if matches!(chars.get(after), Some('半' | '반')) {
                    result.push_str(&format!(" {n}:30 "));
                    i = after + 1;
                    continue;
                }
                // Minutes right after the hour ("3点15"), or after a space when marked ("3시 15분").
                let (m, k) = digits_at(after);
                let marked = matches!(chars.get(k), Some('分' | '분'));
                if !m.is_empty() && (after == j + 1 || marked) {
                    result.push_str(&format!(" {n}:{m:0>2} "));
                    i = if marked { k + 1 } else { k };
                } else {
                    result.push_str(&format!(" {n}:00 "));
                    i = j + 1;
                }
            }
            Some('号' | '日' | '일') => {
                result.push_str(&format!(" day{n} "));
                i = j + 1;
            }
            Some('分' | '분') => {
                result.push_str(&format!(" {n} minutes "));
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
