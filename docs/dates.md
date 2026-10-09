# Typing dates

A note's **Starts** and **Due** fields take dates written the way you'd say
them. Next to the field Wisp shows how it read what you typed, so you can check
before saving. The same goes for the time of an "every day" reminder.

Which languages work:

- **English**, always.
- **The language Wisp is shown in** (Settings → Language).
- **Chinese, Japanese, Korean and Russian**, always: their words can't be
  mistaken for anything else.

Other languages written in Latin letters share spellings ("mar" is March in
English and Tuesday in Spanish), so their words work only when Wisp is shown in
that language.

## The rules

- **A day without a time** means 09:00 for a start and 18:00 for a due time.
  "Tonight" means 20:00.
- **A time without a day** means the next time the clock shows it: `17:00` typed
  at 10:00 is today, `9:00` typed at 10:00 is tomorrow.
- **A weekday** means the coming one, today included: `fri` on a Friday morning
  is today. **"Next"** skips today: `next fri` on a Friday is a week later.
- **From now**: `+30m`, `in 2 hours`, `+3d` count from this minute, rounded up to
  the next whole minute.
- **Numbers for dates**: in English, Chinese, Japanese and Korean the month comes
  first (`10-20`, `10/20`: October 20). In French, Spanish, Italian, Portuguese
  and Esperanto the day comes first (`20/10`), in German `20.10.`, in Russian
  `20.10`. A dash always means month-day (`10-20`), whatever the language.
- **A date that has passed** this year means next year: `1/5` typed in December
  is next January.
- **A full date** always works in every language: `2026-12-24 18:00`.
- **Clearing the field** removes the date.

In the tables below, "today" is Thursday 8 October 2026 at 10:15, and the
results are what you'd get in a Starts field.

## English

| You type | Wisp reads |
| --- | --- |
| `today`, `tonight` | today 09:00, today 20:00 |
| `tomorrow 9am`, `tmr 21:30` | tomorrow 09:00, tomorrow 21:30 |
| `9pm tomorrow`, `tomorrow at 5 pm` | tomorrow 21:00, tomorrow 17:00 |
| `fri`, `thu 17:00` | Friday 9 Oct 09:00, today 17:00 |
| `next thu`, `monday 8:30` | Thursday 15 Oct 09:00, Monday 12 Oct 08:30 |
| `17:00`, `9:00`, `noon`, `12am` | today 17:00, tomorrow 09:00, today 12:00, tomorrow 00:00 |
| `+30m`, `in 2 hours`, `45 minutes` | 10:46, 12:16, 11:01 today |
| `in 90`, `1h30m` | 90 minutes from now |
| `+2d`, `+1d 9:00` | Saturday 10:16, tomorrow 09:00 |
| `10-12`, `10/1 14:00` | 12 Oct 09:00, 1 Oct next year 14:00 |
| `oct 20`, `20th oct 7pm`, `Dec 3rd` | 20 Oct 09:00, 20 Oct 19:00, 3 Dec 09:00 |
| `2026-12-24 18:00` | 24 Dec 2026 18:00 |
| `now` | this minute |

## Chinese 中文

Simplified and Traditional characters both work.

| You type | Wisp reads |
| --- | --- |
| `明天`, `明天下午3点` | tomorrow 09:00, tomorrow 15:00 |
| `后天上午10点半`, `後天上午10點半` | Saturday 10:30 |
| `下周一 9:00`, `下週一 9:00` | Monday 12 Oct 09:00 |
| `周五晚上8点`, `週五晚上8點` | Friday 20:00 |
| `10月20日 14点15分` | 20 Oct 14:15 |
| `30分钟后`, `30分鐘後` | 10:46 today |
| `12号`, `5号 8点` | 12 Oct 09:00, 5 Nov 08:00 |
| `下周` | Thursday 15 Oct 09:00 |

## Japanese 日本語

| You type | Wisp reads |
| --- | --- |
| `明日 9時` | tomorrow 09:00 |
| `明日の午後3時半` | tomorrow 15:30 |
| `明後日 午前10時` | Saturday 10:00 |
| `金曜日 17:00` | Friday 17:00 |
| `来週月曜日` | Monday 12 Oct 09:00 |
| `2時間後`, `30分後` | 12:16, 10:46 today |
| `10月20日 14時30分` | 20 Oct 14:30 |
| `今夜` | today 20:00 |

## Korean 한국어

| You type | Wisp reads |
| --- | --- |
| `내일 오전 9시` | tomorrow 09:00 |
| `모레 오전 10시` | Saturday 10:00 |
| `금요일 17:00` | Friday 17:00 |
| `다음 주 월요일` | Monday 12 Oct 09:00 |
| `2시간 후`, `30분 후` | 12:16, 10:46 today |
| `10월 20일 오후 2시 30분` | 20 Oct 14:30 |
| `오늘 밤` | today 20:00 |

## Russian Русский

| You type | Wisp reads |
| --- | --- |
| `завтра в 9:00` | tomorrow 09:00 |
| `послезавтра в 10` | Saturday 10:00 |
| `в пятницу в 17:00` | Friday 17:00 |
| `в следующий понедельник` | Monday 12 Oct 09:00 |
| `через 2 часа`, `через 30 минут` | 12:16, 10:46 today |
| `через неделю` | Thursday 15 Oct 10:16 |
| `20.10 14:00`, `20 октября` | 20 Oct 14:00, 20 Oct 09:00 |
| `сегодня вечером` | today 20:00 |
| `в 3 часа дня` | today 15:00 |

## French Français

| You type | Wisp reads |
| --- | --- |
| `demain 9h` | tomorrow 09:00 |
| `après-demain à 10h` | Saturday 10:00 |
| `demain à 3h de l'après-midi` | tomorrow 15:00 |
| `vendredi 17h` | Friday 17:00 |
| `lundi prochain` | Monday 12 Oct 09:00 |
| `dans 2 heures`, `dans 2h` | 12:16 today |
| `20/10 14h30` | 20 Oct 14:30 |
| `le 20 octobre à 14 heures` | 20 Oct 14:00 |
| `le 1er` | 1 Nov 09:00 |
| `ce soir` | today 20:00 |

## German Deutsch

| You type | Wisp reads |
| --- | --- |
| `morgen 9 Uhr` | tomorrow 09:00 |
| `übermorgen um 10` | Saturday 10:00 |
| `Fr 17:00` | Friday 17:00 |
| `nächsten Montag` | Monday 12 Oct 09:00 |
| `in 2 Stunden`, `in einer Stunde` | 12:16, 11:16 today |
| `20.10. 14:30`, `am 20. Oktober` | 20 Oct 14:30, 20 Oct 09:00 |
| `14.30` | today 14:30 |
| `heute Abend` | today 20:00 |

## Spanish Español

| You type | Wisp reads |
| --- | --- |
| `mañana a las 9` | tomorrow 09:00 |
| `mañana a las 3 de la tarde` | tomorrow 15:00 |
| `pasado mañana a las 10` | Saturday 10:00 |
| `viernes 17:00` | Friday 17:00 |
| `el próximo lunes`, `el lunes que viene` | Monday 12 Oct 09:00 |
| `en 2 horas` | 12:16 today |
| `20/10 14:00`, `el 20 de octubre` | 20 Oct 14:00, 20 Oct 09:00 |
| `esta noche` | today 20:00 |

## Portuguese (Brazil) Português

| You type | Wisp reads |
| --- | --- |
| `amanhã às 9h` | tomorrow 09:00 |
| `depois de amanhã às 10h` | Saturday 10:00 |
| `sexta 17h` | Friday 17:00 |
| `próxima segunda`, `segunda que vem` | Monday 12 Oct 09:00 |
| `em 2 horas`, `daqui a 30 minutos` | 12:16, 10:46 today |
| `20/10 14h30`, `20 de outubro` | 20 Oct 14:30, 20 Oct 09:00 |

## Italian Italiano

| You type | Wisp reads |
| --- | --- |
| `domani alle 9` | tomorrow 09:00 |
| `dopodomani alle 10` | Saturday 10:00 |
| `venerdì 17:00` | Friday 17:00 |
| `lunedì prossimo` | Monday 12 Oct 09:00 |
| `tra 2 ore`, `fra un'ora` | 12:16, 11:16 today |
| `20/10 14:00`, `20 ottobre` | 20 Oct 14:00, 20 Oct 09:00 |
| `stasera` | today 20:00 |

## Esperanto

Letters with hats can be typed in the x-system: `morgaux` for `morgaŭ`.

| You type | Wisp reads |
| --- | --- |
| `morgaŭ je la 9a`, `morgaux je 9:00` | tomorrow 09:00 |
| `postmorgaŭ je la 10a` | Saturday 10:00 |
| `vendredon 17:00` | Friday 17:00 |
| `venontan lundon` | Monday 12 Oct 09:00 |
| `post 2 horoj` | 12:16 today |
| `20/10 14:00`, `la 20-an de oktobro` | 20 Oct 14:00, 20 Oct 09:00 |
| `ĉi-vespere` | today 20:00 |

## Reminder times

The amount of a "before start", "after start" or "before due" reminder is
written like `15m`, `2h`, `1h30m`, `1d` or just a number of minutes (`90`). Words
work too, in English and in the language Wisp is shown in: `15 min`, `2 hours`,
`1 day`, `15 Min`, `2 Stunden`, `1 jour`, `2 часа`, `2時間`, `15분`, `2小时`.

An "every day" reminder rings at the start's time of day, or at a time you
type: `09:00`, `9am`, `下午3点`, `午後3時`, `오후 3시`, `15h` in French or
`15 Uhr` in German.

## When Wisp can't read a date

The text next to the field turns red and says which word it couldn't read. Fix
it, or close the note with `Esc` twice: Wisp then keeps the date the note had
before.
