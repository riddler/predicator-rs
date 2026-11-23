//! Civil dates, instants in UTC and durations: small value types with
//! hand-written arithmetic on the proleptic Gregorian calendar.

use core::fmt;

/// Seconds in a civil day; an instant here has no leap seconds.
const SECONDS_PER_DAY: i64 = 86_400;

/// Microseconds in a second.
const MICROS_PER_SECOND: u32 = 1_000_000;

/// The days from 1970-01-01 to the given civil date, negative before it.
/// The year is any `i32`, so no step can overflow an `i64`.
fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = i64::from(year) - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let month_from_march = (i64::from(month) + 9) % 12;
    let day_of_year = (153 * month_from_march + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The civil date `days` after 1970-01-01, or `None` when its year is
/// outside `i32`.
fn civil_from_days(days: i64) -> Option<Date> {
    let shifted = days.checked_add(719_468)?;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let year = era
        .checked_mul(400)?
        .checked_add(year_of_era + i64::from(month <= 2))?;
    Some(Date {
        year: i32::try_from(year).ok()?,
        month: u8::try_from(month).ok()?,
        day: u8::try_from(day).ok()?,
    })
}

fn is_leap_year(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        2 if is_leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// A civil date: a year, a month and a day on the proleptic Gregorian
/// calendar, with no time of day and no zone. Dates order chronologically.
///
/// ```
/// use predicator_rs::Date;
///
/// let date = Date::new(2026, 8, 6).unwrap();
/// assert_eq!((date.year(), date.month(), date.day()), (2026, 8, 6));
/// assert_eq!(date.to_string(), "2026-08-06");
/// assert!(Date::new(2026, 2, 29).is_none());
/// assert!(date < Date::new(2026, 8, 7).unwrap());
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    year: i32,
    month: u8,
    day: u8,
}

impl Date {
    /// The date with these parts, or `None` when they name no calendar
    /// date (a month outside 1 to 12, a day past the month's end).
    pub fn new(year: i32, month: u32, day: u32) -> Option<Date> {
        if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
            return None;
        }
        Some(Date {
            year,
            month: u8::try_from(month).ok()?,
            day: u8::try_from(day).ok()?,
        })
    }

    /// The year; negative before year 0 (1 BC).
    pub fn year(&self) -> i32 {
        self.year
    }

    /// The month, 1 to 12.
    pub fn month(&self) -> u32 {
        u32::from(self.month)
    }

    /// The day of the month, from 1.
    pub fn day(&self) -> u32 {
        u32::from(self.day)
    }

    /// The days since 1970-01-01, negative before it.
    pub(crate) fn days(self) -> i64 {
        days_from_civil(self.year, self.month(), self.day())
    }

    /// The date `days` after 1970-01-01 (before it when negative), or
    /// `None` when its year is outside `i32`.
    pub(crate) fn from_days(days: i64) -> Option<Date> {
        civil_from_days(days)
    }

    /// Reads `YYYY-MM-DD` with a four-digit year, the form the tagged
    /// encoding carries.
    pub(crate) fn parse(text: &str) -> Option<Date> {
        let mut parts = Digits::new(text);
        let year = parts.number(4)?;
        parts.literal(b'-')?;
        let month = parts.number(2)?;
        parts.literal(b'-')?;
        let day = parts.number(2)?;
        parts.end()?;
        Date::new(i32::try_from(year).ok()?, month, day)
    }
}

impl fmt::Display for Date {
    /// ISO 8601, `YYYY-MM-DD`. A year outside 0 to 9999 is written with as
    /// many digits as it takes, and a minus sign below 0.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.year < 0 { "-" } else { "" };
        write!(
            f,
            "{sign}{:04}-{:02}-{:02}",
            self.year.unsigned_abs(),
            self.month,
            self.day
        )
    }
}

impl fmt::Debug for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Date({self})")
    }
}

/// An instant in UTC to the microsecond. Its date may be any [`Date`], so
/// years past 9999 (and before 0) are held without overflow. Instants order
/// chronologically, and two values at the same instant are equal however
/// they were made.
///
/// ```
/// use predicator_rs::{Date, DateTime};
///
/// let date = Date::new(2026, 8, 6).unwrap();
/// let instant = DateTime::new(date, 12, 0, 0, 500_000).unwrap();
/// assert_eq!(instant.to_string(), "2026-08-06T12:00:00.500000Z");
/// assert_eq!(instant.date(), date);
/// assert_eq!(
///     DateTime::from_unix(instant.unix_seconds(), 500_000),
///     Some(instant)
/// );
/// assert_eq!(DateTime::from_unix(0, 0).unwrap().to_string(), "1970-01-01T00:00:00Z");
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DateTime {
    date: Date,
    second_of_day: u32,
    microsecond: u32,
}

impl DateTime {
    /// The instant at a time of day on `date`, or `None` when the hour is
    /// past 23, the minute or second past 59 or the microsecond past
    /// 999999.
    pub fn new(
        date: Date,
        hour: u32,
        minute: u32,
        second: u32,
        microsecond: u32,
    ) -> Option<DateTime> {
        if hour > 23 || minute > 59 || second > 59 || microsecond >= MICROS_PER_SECOND {
            return None;
        }
        Some(DateTime {
            date,
            second_of_day: hour * 3600 + minute * 60 + second,
            microsecond,
        })
    }

    /// The instant `seconds` after 1970-01-01T00:00:00Z (before it when
    /// negative) and `microsecond` into that second, or `None` when the
    /// microsecond is past 999999 or the year is outside `i32`.
    pub fn from_unix(seconds: i64, microsecond: u32) -> Option<DateTime> {
        if microsecond >= MICROS_PER_SECOND {
            return None;
        }
        Some(DateTime {
            date: civil_from_days(seconds.div_euclid(SECONDS_PER_DAY))?,
            second_of_day: u32::try_from(seconds.rem_euclid(SECONDS_PER_DAY)).ok()?,
            microsecond,
        })
    }

    /// The whole seconds since 1970-01-01T00:00:00Z, negative before it.
    pub fn unix_seconds(&self) -> i64 {
        self.date.days() * SECONDS_PER_DAY + i64::from(self.second_of_day)
    }

    /// The microsecond within the second, 0 to 999999.
    pub fn microsecond(&self) -> u32 {
        self.microsecond
    }

    /// The instant's calendar date in UTC.
    pub fn date(&self) -> Date {
        self.date
    }

    /// The hour, 0 to 23.
    pub fn hour(&self) -> u32 {
        self.second_of_day / 3600
    }

    /// The minute, 0 to 59.
    pub fn minute(&self) -> u32 {
        self.second_of_day % 3600 / 60
    }

    /// The second, 0 to 59.
    pub fn second(&self) -> u32 {
        self.second_of_day % 60
    }

    /// Reads ISO 8601 text as the reference's codec reads it: a four-digit
    /// year, a `T` or a space before the time, an optional fraction after a
    /// full stop or a comma (any count of digits, read to the microsecond),
    /// and an offset of `Z`, `+HH:MM`, `+HHMM` or `+HH` (either sign) read
    /// as the instant it names. `-00:00` is refused, and so is an instant
    /// outside the years 0 to 9999 the encoding carries.
    pub(crate) fn parse(text: &str) -> Option<DateTime> {
        let mut parts = Digits::new(text);
        let year = parts.number(4)?;
        parts.literal(b'-')?;
        let month = parts.number(2)?;
        parts.literal(b'-')?;
        let day = parts.number(2)?;
        if parts.literal(b'T').is_none() {
            parts.literal(b' ')?;
        }
        let hour = parts.number(2)?;
        parts.literal(b':')?;
        let minute = parts.number(2)?;
        parts.literal(b':')?;
        let second = parts.number(2)?;
        let microsecond = match parts.peek() {
            Some(b'.' | b',') => {
                parts.skip();
                parts.fraction()?
            }
            _ => 0,
        };
        let offset = parts.offset()?;
        parts.end()?;
        let date = Date::new(i32::try_from(year).ok()?, month, day)?;
        let local = DateTime::new(date, hour, minute, second, microsecond)?;
        let instant = DateTime::from_unix(local.unix_seconds() - offset, microsecond)?;
        (0..=9999).contains(&instant.date.year).then_some(instant)
    }
}

impl fmt::Display for DateTime {
    /// ISO 8601 in UTC: the fraction left out when the microsecond is zero
    /// and exactly six digits when it is not.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}T{:02}:{:02}:{:02}",
            self.date,
            self.hour(),
            self.minute(),
            self.second()
        )?;
        if self.microsecond != 0 {
            write!(f, ".{:06}", self.microsecond)?;
        }
        f.write_str("Z")
    }
}

impl fmt::Debug for DateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DateTime({self})")
    }
}

/// A duration: the eight units the reference's duration carries, every one
/// present and zero unless set. A unit may be negative (the difference of
/// two dates can be). Two durations are equal when every unit is.
///
/// Its text, the form `Display` writes, is the duration-literal grammar:
/// the units largest first, each followed by its suffix (`y`, `mo`, `w`,
/// `d`, `h`, `m`, `s`, `ms`), a zero unit left out, and `0s` when every
/// unit is zero. A negative unit is written with its minus sign.
///
/// ```
/// use predicator_rs::Duration;
///
/// let duration = Duration { weeks: 2, days: 3, ..Duration::default() };
/// assert_eq!(duration.to_string(), "2w3d");
/// assert_eq!(Duration::default().to_string(), "0s");
/// assert_eq!(Duration { days: -3, ..Duration::default() }.to_string(), "-3d");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Duration {
    /// Years.
    pub years: i64,
    /// Months.
    pub months: i64,
    /// Weeks.
    pub weeks: i64,
    /// Days.
    pub days: i64,
    /// Hours.
    pub hours: i64,
    /// Minutes.
    pub minutes: i64,
    /// Seconds.
    pub seconds: i64,
    /// Milliseconds.
    pub milliseconds: i64,
}

impl Duration {
    /// The units with their names in the tagged encoding, largest first.
    pub(crate) fn units(&self) -> [(&'static str, &'static str, i64); 8] {
        [
            ("years", "y", self.years),
            ("months", "mo", self.months),
            ("weeks", "w", self.weeks),
            ("days", "d", self.days),
            ("hours", "h", self.hours),
            ("minutes", "m", self.minutes),
            ("seconds", "s", self.seconds),
            ("milliseconds", "ms", self.milliseconds),
        ]
    }

    /// The unit named `name` in the tagged encoding, for writing.
    pub(crate) fn unit_mut(&mut self, name: &str) -> Option<&mut i64> {
        match name {
            "years" => Some(&mut self.years),
            "months" => Some(&mut self.months),
            "weeks" => Some(&mut self.weeks),
            "days" => Some(&mut self.days),
            "hours" => Some(&mut self.hours),
            "minutes" => Some(&mut self.minutes),
            "seconds" => Some(&mut self.seconds),
            "milliseconds" => Some(&mut self.milliseconds),
            _ => None,
        }
    }

    /// Reads the duration-literal grammar as the reference's duration
    /// parser reads it: the whole text is one or more pairs of an amount
    /// and a unit, the amount `[0-9]+` with an optional fraction
    /// `\.[0-9]+`, the unit `mo`, `ms`, `y`, `w`, `d`, `h`, `m` or `s`
    /// (the two-letter units tried first). No sign, no space, no empty
    /// text. A repeated unit adds to the amount before it. A fraction must
    /// come to a whole count of milliseconds: the whole part keeps its own
    /// unit and the rest is spread largest first over days, hours,
    /// minutes, seconds and milliseconds (a month 30 days, a year 365), so
    /// `0.5mo` is 15 days. A unit whose amount is outside `i64` is
    /// [`DurationText::Overflow`].
    pub(crate) fn parse(text: &str) -> Result<Duration, DurationText> {
        let mut duration = Duration::default();
        let mut rest = text.as_bytes();
        if rest.is_empty() {
            return Err(DurationText::Malformed);
        }
        while !rest.is_empty() {
            let (whole, after) = digit_run(rest);
            let (fraction, after) = match after.split_first() {
                Some((b'.', after)) => match digit_run(after) {
                    ([], _) => return Err(DurationText::Malformed),
                    run => run,
                },
                _ => (&[][..], after),
            };
            if whole.is_empty() {
                return Err(DurationText::Malformed);
            }
            let (unit, after) = duration_unit(after).ok_or(DurationText::Malformed)?;
            duration.add_component(whole, fraction, unit)?;
            rest = after;
        }
        Ok(duration)
    }

    /// Adds one `whole.fraction` amount of `unit` (its suffix).
    fn add_component(
        &mut self,
        whole: &[u8],
        fraction: &[u8],
        unit: &str,
    ) -> Result<(), DurationText> {
        let amount = whole.iter().try_fold(0_i64, |amount, digit| {
            amount.checked_mul(10)?.checked_add(i64::from(digit - b'0'))
        });
        self.add_unit(unit, amount.ok_or(DurationText::Overflow)?)?;
        // Trailing zeros of a fraction change nothing; past eleven more
        // digits no unit's milliseconds can clear the denominator.
        let mut fraction = fraction;
        while let Some((b'0', head)) = fraction.split_last() {
            fraction = head;
        }
        if fraction.is_empty() {
            return Ok(());
        }
        if fraction.len() > 11 {
            return Err(DurationText::Malformed);
        }
        let numerator = fraction
            .iter()
            .fold(0_i128, |sum, digit| sum * 10 + i128::from(digit - b'0'));
        let denominator = fraction.iter().fold(1_i128, |power, _| power * 10);
        let total = numerator * unit_milliseconds(unit);
        if total % denominator != 0 {
            return Err(DurationText::Malformed);
        }
        let mut left = total / denominator;
        for (suffix, size) in [
            ("d", 86_400_000),
            ("h", 3_600_000),
            ("m", 60_000),
            ("s", 1_000),
            ("ms", 1),
        ] {
            let count = left / size;
            left %= size;
            if count > 0 {
                self.add_unit(
                    suffix,
                    i64::try_from(count).map_err(|_| DurationText::Overflow)?,
                )?;
            }
        }
        Ok(())
    }

    /// Adds `amount` to the unit whose suffix is `suffix`.
    fn add_unit(&mut self, suffix: &str, amount: i64) -> Result<(), DurationText> {
        let slot = match suffix {
            "y" => &mut self.years,
            "mo" => &mut self.months,
            "w" => &mut self.weeks,
            "d" => &mut self.days,
            "h" => &mut self.hours,
            "m" => &mut self.minutes,
            "s" => &mut self.seconds,
            _ => &mut self.milliseconds,
        };
        *slot = slot.checked_add(amount).ok_or(DurationText::Overflow)?;
        Ok(())
    }
}

/// Why a text is not a duration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DurationText {
    /// The text is not the duration-literal grammar, or a fraction leaves
    /// less than a millisecond over.
    Malformed,
    /// A unit's amount is outside `i64`.
    Overflow,
}

/// The leading ASCII digits of `bytes`, and what follows them.
fn digit_run(bytes: &[u8]) -> (&[u8], &[u8]) {
    let count = bytes
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    bytes.split_at_checked(count).unwrap_or((bytes, &[]))
}

/// The duration unit `bytes` starts with, two-letter units first, and what
/// follows it.
fn duration_unit(bytes: &[u8]) -> Option<(&'static str, &[u8])> {
    for unit in ["mo", "ms", "y", "w", "d", "h", "m", "s"] {
        if let Some(rest) = bytes.strip_prefix(unit.as_bytes()) {
            return Some((unit, rest));
        }
    }
    None
}

/// The milliseconds in one of `unit`, a month 30 days and a year 365.
fn unit_milliseconds(unit: &str) -> i128 {
    match unit {
        "y" => 31_536_000_000,
        "mo" => 2_592_000_000,
        "w" => 604_800_000,
        "d" => 86_400_000,
        "h" => 3_600_000,
        "m" => 60_000,
        "s" => 1_000,
        _ => 1,
    }
}

impl fmt::Display for Duration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut written = false;
        for (_, suffix, amount) in self.units() {
            if amount != 0 {
                write!(f, "{amount}{suffix}")?;
                written = true;
            }
        }
        if written { Ok(()) } else { f.write_str("0s") }
    }
}

/// A reader over ASCII text, one fixed field at a time; every step answers
/// `None` on text it does not expect.
struct Digits<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Digits<'a> {
    fn new(text: &'a str) -> Self {
        Digits {
            bytes: text.as_bytes(),
            at: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn skip(&mut self) {
        self.at += 1;
    }

    fn digit(&mut self) -> Option<u32> {
        let byte = self.peek().filter(u8::is_ascii_digit)?;
        self.skip();
        Some(u32::from(byte - b'0'))
    }

    /// Exactly `count` digits, as a number.
    fn number(&mut self, count: usize) -> Option<u32> {
        let mut value = 0;
        for _ in 0..count {
            value = value * 10 + self.digit()?;
        }
        Some(value)
    }

    fn literal(&mut self, byte: u8) -> Option<()> {
        (self.peek()? == byte).then(|| self.skip())
    }

    /// One or more digits of a fraction of a second, as microseconds:
    /// digits past the sixth are dropped.
    fn fraction(&mut self) -> Option<u32> {
        let mut micros = 0;
        let mut count = 0;
        while let Some(digit) = self.digit() {
            if count < 6 {
                micros = micros * 10 + digit;
            }
            count += 1;
        }
        if count == 0 {
            return None;
        }
        for _ in count..6 {
            micros *= 10;
        }
        Some(micros)
    }

    /// An offset from UTC in seconds: `Z`, or a sign, two digits of hours
    /// and optionally two of minutes, with or without a colon.
    fn offset(&mut self) -> Option<i64> {
        let sign = match self.peek()? {
            b'Z' => {
                self.skip();
                return Some(0);
            }
            b'+' => 1,
            b'-' => -1,
            _ => return None,
        };
        self.skip();
        let hours = self.number(2)?;
        let colon = self.literal(b':').is_some();
        let minutes = match self.peek() {
            Some(byte) if byte.is_ascii_digit() => self.number(2)?,
            _ if colon => return None,
            _ => 0,
        };
        if hours > 23 || minutes > 59 || (sign < 0 && colon && hours == 0 && minutes == 0) {
            return None;
        }
        Some(sign * i64::from(hours * 3600 + minutes * 60))
    }

    fn end(&self) -> Option<()> {
        (self.at == self.bytes.len()).then_some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u32, day: u32) -> Date {
        Date::new(year, month, day).unwrap()
    }

    // Sabotage: with days_in_month giving February 29 days in every year,
    // this test fails.
    #[test]
    fn a_date_is_a_calendar_date() {
        assert!(Date::new(2024, 2, 29).is_some());
        assert!(Date::new(2000, 2, 29).is_some());
        assert!(Date::new(1900, 2, 29).is_none());
        assert!(Date::new(2023, 2, 29).is_none());
        assert!(Date::new(2026, 4, 31).is_none());
        assert!(Date::new(2026, 13, 1).is_none());
        assert!(Date::new(2026, 0, 1).is_none());
        assert!(Date::new(2026, 1, 0).is_none());
        assert_eq!(date(2026, 1, 31).day(), 31);
    }

    // Sabotage: with civil_from_days's month taken as month_from_march + 2,
    // this test fails.
    #[test]
    fn day_numbers_round_trip_across_eras() {
        assert_eq!(date(1970, 1, 1).days(), 0);
        assert_eq!(date(2000, 3, 1).days(), 11_017);
        assert_eq!(date(1969, 12, 31).days(), -1);
        for days in [
            -1_000_000, -719_528, -1, 0, 59, 60, 11_016, 2_932_896, 9_000_000,
        ] {
            let back = civil_from_days(days).unwrap();
            assert_eq!(back.days(), days, "{back:?}");
        }
        assert_eq!(civil_from_days(-719_528), Some(date(0, 1, 1)));
        assert_eq!(civil_from_days(2_932_896), Some(date(9999, 12, 31)));
        assert_eq!(civil_from_days(i64::MAX), None);
        let last = date(i32::MAX, 12, 31);
        assert_eq!(civil_from_days(last.days()), Some(last));
        assert_eq!(civil_from_days(last.days() + 1), None);
        let first = date(i32::MIN, 1, 1);
        assert_eq!(civil_from_days(first.days()), Some(first));
        assert_eq!(civil_from_days(first.days() - 1), None);
    }

    // Sabotage: with Date's Display writing the year without zero padding,
    // this test fails.
    #[test]
    fn a_date_writes_and_reads_iso_text() {
        assert_eq!(date(2026, 8, 6).to_string(), "2026-08-06");
        assert_eq!(date(7, 1, 2).to_string(), "0007-01-02");
        assert_eq!(date(12_026, 1, 2).to_string(), "12026-01-02");
        assert_eq!(date(-1, 1, 2).to_string(), "-0001-01-02");
        assert_eq!(format!("{:?}", date(2026, 8, 6)), "Date(2026-08-06)");
        assert_eq!(Date::parse("2026-08-06"), Some(date(2026, 8, 6)));
        for text in [
            "2026-8-06",
            "20260806",
            "+2026-08-06",
            "2026-02-30",
            "2026-08-06 ",
            "12026-01-01",
        ] {
            assert_eq!(Date::parse(text), None, "{text}");
        }
    }

    // Sabotage: with DateTime::new accepting an hour of 24, this test fails.
    #[test]
    fn an_instant_holds_the_microsecond_and_far_years() {
        let instant = DateTime::new(date(2026, 8, 9), 10, 30, 0, 500_000).unwrap();
        assert_eq!(instant.to_string(), "2026-08-09T10:30:00.500000Z");
        assert_eq!(
            (instant.hour(), instant.minute(), instant.second()),
            (10, 30, 0)
        );
        assert_eq!(instant.microsecond(), 500_000);
        assert_eq!(
            format!("{instant:?}"),
            "DateTime(2026-08-09T10:30:00.500000Z)"
        );
        assert!(DateTime::new(date(2026, 8, 9), 24, 0, 0, 0).is_none());
        assert!(DateTime::new(date(2026, 8, 9), 0, 60, 0, 0).is_none());
        assert!(DateTime::new(date(2026, 8, 9), 0, 0, 60, 0).is_none());
        assert!(DateTime::new(date(2026, 8, 9), 0, 0, 0, 1_000_000).is_none());
        assert!(DateTime::from_unix(0, 1_000_000).is_none());
        let before = DateTime::from_unix(-1, 999_999).unwrap();
        assert_eq!(before.to_string(), "1969-12-31T23:59:59.999999Z");
        let far = DateTime::new(date(i32::MAX, 12, 31), 23, 59, 59, 999_999).unwrap();
        assert_eq!(DateTime::from_unix(far.unix_seconds(), 999_999), Some(far));
        assert!(DateTime::from_unix(far.unix_seconds() + 1, 0).is_none());
        assert!(DateTime::from_unix(i64::MIN, 0).is_none());
        assert!(before < DateTime::from_unix(0, 0).unwrap());
    }

    // Sabotage: with the offset added to the local instant rather than
    // subtracted, this test fails.
    #[test]
    fn an_instant_reads_the_forms_the_reference_reads() {
        let read = |text: &str| DateTime::parse(text).map(|instant| instant.to_string());
        assert_eq!(
            read("2026-08-06T12:00:00Z").as_deref(),
            Some("2026-08-06T12:00:00Z")
        );
        assert_eq!(
            read("2026-08-06 12:00:00Z").as_deref(),
            Some("2026-08-06T12:00:00Z")
        );
        assert_eq!(
            read("2026-08-06T12:00:00,5Z").as_deref(),
            Some("2026-08-06T12:00:00.500000Z")
        );
        assert_eq!(
            read("2026-08-06T12:00:00.1234567Z").as_deref(),
            Some("2026-08-06T12:00:00.123456Z")
        );
        assert_eq!(
            read("2026-08-06T12:00:00+0130").as_deref(),
            Some("2026-08-06T10:30:00Z")
        );
        assert_eq!(
            read("2026-08-06T12:00:00+01").as_deref(),
            Some("2026-08-06T11:00:00Z")
        );
        assert_eq!(
            read("2026-08-06T12:00:00-01:00").as_deref(),
            Some("2026-08-06T13:00:00Z")
        );
        assert_eq!(
            read("2026-08-06T12:00:00-0000").as_deref(),
            Some("2026-08-06T12:00:00Z")
        );
        for text in [
            "2026-08-06t12:00:00z",
            "2026-08-06T12:00:00-00:00",
            "2026-08-06T12:00:00",
            "2026-08-06T24:00:00Z",
            "2026-08-06T12:00:60Z",
            "2026-08-06T12:00Z",
            "2026-08-06T12:00:00.Z",
            "2026-08-06T12:00:00+01:",
            "2026-08-06T12:00:00+24:00",
            "2026-08-06T12:00:00Z ",
            "20260806T120000Z",
            "9999-12-31T23:30:00-01:00",
            "0000-01-01T00:30:00+01:00",
        ] {
            assert_eq!(read(text), None, "{text}");
        }
    }

    // Sabotage: with Duration's Display skipping negative units as the
    // reference's code does, this test fails.
    #[test]
    fn a_duration_writes_its_units_largest_first() {
        let every = Duration {
            years: 1,
            months: 2,
            weeks: 3,
            days: 4,
            hours: 5,
            minutes: 6,
            seconds: 7,
            milliseconds: 8,
        };
        assert_eq!(every.to_string(), "1y2mo3w4d5h6m7s8ms");
        assert_eq!(
            Duration {
                seconds: -90,
                ..Duration::default()
            }
            .to_string(),
            "-90s"
        );
        assert_eq!(Duration::default().to_string(), "0s");
        let mut written = Duration::default();
        for (name, _, _) in every.units() {
            *written.unit_mut(name).unwrap() = 1;
        }
        assert!(written.units().iter().all(|(_, _, amount)| *amount == 1));
        assert!(written.unit_mut("fortnights").is_none());
    }

    /// The duration `text` reads as, its text, or why it is refused.
    fn read(text: &str) -> String {
        match Duration::parse(text) {
            Ok(duration) => format!("{duration:?}"),
            Err(why) => format!("{why:?}"),
        }
    }

    fn of(duration: Duration) -> String {
        format!("{duration:?}")
    }

    // Sabotage: with a fraction's remainder truncated to the millisecond
    // rather than refused, this test fails.
    #[test]
    fn a_duration_reads_the_literal_grammar_with_its_fractions() {
        assert_eq!(
            read("3d8h30m"),
            of(Duration {
                days: 3,
                hours: 8,
                minutes: 30,
                ..Duration::default()
            })
        );
        assert_eq!(
            read("1mo1ms1m"),
            of(Duration {
                months: 1,
                milliseconds: 1,
                minutes: 1,
                ..Duration::default()
            })
        );
        assert_eq!(
            read("1d2d"),
            of(Duration {
                days: 3,
                ..Duration::default()
            })
        );
        assert_eq!(read("0s"), of(Duration::default()));
        assert_eq!(
            read("1.5s"),
            of(Duration {
                seconds: 1,
                milliseconds: 500,
                ..Duration::default()
            })
        );
        assert_eq!(
            read("0.5mo"),
            of(Duration {
                days: 15,
                ..Duration::default()
            })
        );
        assert_eq!(
            read("0.5y"),
            of(Duration {
                days: 182,
                hours: 12,
                ..Duration::default()
            })
        );
        assert_eq!(
            read("1.25d0.5d"),
            of(Duration {
                days: 1,
                hours: 18,
                ..Duration::default()
            })
        );
        assert_eq!(
            read(&format!("2.{}s", "0".repeat(40))),
            of(Duration {
                seconds: 2,
                ..Duration::default()
            })
        );
        assert_eq!(
            read("0.00000000001mo"),
            "Malformed",
            "eleven digits leave less than a millisecond"
        );
        assert_eq!(
            read("0.0009765625mo"),
            of(Duration {
                minutes: 42,
                seconds: 11,
                milliseconds: 250,
                ..Duration::default()
            })
        );
        for refused in [
            "", "0.5ms", "1.s", ".5s", "1", "d", "1d ", " 1d", "-1d", "1x", "1mos", "1d\n", "1D",
        ] {
            assert_eq!(read(refused), "Malformed", "{refused:?}");
        }
        assert_eq!(read("9223372036854775808s"), "Overflow");
        assert_eq!(read("9223372036854775807d1d"), "Overflow");
        assert_eq!(read("9223372036854775807h0.5d"), "Overflow");
        assert_eq!(
            read("9223372036854775807ms"),
            of(Duration {
                milliseconds: i64::MAX,
                ..Duration::default()
            })
        );
    }
}
