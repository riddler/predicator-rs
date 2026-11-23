//! The string and date literals, and the ISO 8601 text a date literal's
//! body is read as (the lexer's module documentation states the rules).

use super::{Kind, Quote, Refusal, Scan, Token};
use crate::{Date, DateTime, Position};

/// The refusal of the numeric escape, in either spelling of its letter.
const NUMERIC_ESCAPE: &str = "Unsupported escape sequence \\u in string literal: predicator has \
                              no numeric escape; write the character itself (string literals \
                              are UTF-8)";

impl Scan<'_> {
    /// A string literal, quoted with `quote`. A refusal inside it spans its
    /// opening quote.
    pub(super) fn string(&mut self, quote: Quote) -> Result<(), Refusal> {
        let closing = match quote {
            Quote::Double => '"',
            Quote::Single => '\'',
        };
        let opening = Position {
            line: self.line,
            column: self.column + 1,
        };
        let mut chars = self.rest().char_indices().skip(1);
        let mut value = String::new();
        let mut line = self.line;
        let mut column = self.column + 1;
        let mut length = 1;
        loop {
            let Some((offset, char)) = chars.next() else {
                return Err(self.unterminated(quote, opening));
            };
            if char == closing {
                self.tokens.push(Token {
                    kind: Kind::String {
                        value,
                        quote,
                        end: Position {
                            line,
                            column: column + 1,
                        },
                    },
                    line: self.line,
                    column: self.column,
                    length: length + 1,
                });
                self.at += offset + 1;
                self.line = line;
                self.column = column + 1;
                return Ok(());
            }
            match char {
                '\\' => {
                    let Some((_, escaped)) = chars.next() else {
                        return Err(self.unterminated(quote, opening));
                    };
                    if matches!(escaped, 'u' | 'U') {
                        return Err(self.refuse(
                            "unsupported_escape",
                            NUMERIC_ESCAPE.to_owned(),
                            opening,
                        ));
                    }
                    value.push(match escaped {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        other => other,
                    });
                    length += 2;
                    column += 2;
                }
                '\n' => {
                    value.push('\n');
                    length += 1;
                    line += 1;
                    column = 1;
                }
                other => {
                    value.push(other);
                    length += 1;
                    column += 1;
                }
            }
        }
    }

    fn unterminated(&self, quote: Quote, opening: Position) -> Refusal {
        let name = match quote {
            Quote::Double => "double",
            Quote::Single => "single",
        };
        self.refuse(
            "unterminated_string",
            format!("Unterminated {name}-quoted string literal"),
            opening,
        )
    }

    /// A date or datetime literal, `#...#`. A refusal spans the whole
    /// literal: through the closing `#`, or through the end of the source
    /// when there is none.
    pub(super) fn date(&mut self) -> Result<(), Refusal> {
        let body_start = self.at + 1;
        let mut line = self.line;
        let mut column = self.column + 1;
        for (length, (offset, char)) in (1..).zip(self.from(body_start).char_indices()) {
            match char {
                '#' => {
                    let body = self.from(body_start).get(..offset).unwrap_or_default();
                    let end = Position {
                        line,
                        column: column + 1,
                    };
                    let (kind, name) = if body.contains('T') {
                        (
                            read_datetime(body)
                                .map(|(instant, precision)| Kind::DateTime(instant, precision)),
                            "datetime",
                        )
                    } else {
                        (read_date(body).map(Kind::Date), "date")
                    };
                    let Some(kind) = kind else {
                        return Err(self.refuse(
                            &format!("invalid_{name}"),
                            format!("Invalid {name} format: {body}"),
                            end,
                        ));
                    };
                    self.emit(kind, length + 1);
                    self.at = body_start + offset + 1;
                    self.line = end.line;
                    self.column = end.column;
                    return Ok(());
                }
                '\n' => {
                    line += 1;
                    column = 1;
                }
                _ => column += 1,
            }
        }
        Err(self.refuse(
            "unterminated_date",
            "Unterminated date literal".to_owned(),
            Position { line, column },
        ))
    }
}

/// A reader over ASCII text, one fixed field at a time; every step answers
/// `None` on text it does not expect.
struct Fields<'a> {
    bytes: &'a [u8],
}

impl<'a> Fields<'a> {
    /// The text after an optional sign, and the sign as a multiplier.
    fn signed(text: &'a str) -> (Fields<'a>, i32) {
        let bytes = text.as_bytes();
        match bytes.split_first() {
            Some((b'-', rest)) => (Fields { bytes: rest }, -1),
            Some((b'+', rest)) => (Fields { bytes: rest }, 1),
            _ => (Fields { bytes }, 1),
        }
    }

    fn next(&mut self) -> Option<u8> {
        let (first, rest) = self.bytes.split_first()?;
        self.bytes = rest;
        Some(*first)
    }

    /// Exactly `count` digits, as a number.
    fn number(&mut self, count: usize) -> Option<u32> {
        let mut value = 0;
        for _ in 0..count {
            let byte = self.next().filter(u8::is_ascii_digit)?;
            value = value * 10 + u32::from(byte - b'0');
        }
        Some(value)
    }

    fn literal(&mut self, byte: u8) -> Option<()> {
        (self.next()? == byte).then_some(())
    }

    /// `YYYY-MM-DD`, the year multiplied by `sign`.
    fn date(&mut self, sign: i32) -> Option<Date> {
        let year = i32::try_from(self.number(4)?).ok()? * sign;
        self.literal(b'-')?;
        let month = self.number(2)?;
        self.literal(b'-')?;
        let day = self.number(2)?;
        Date::new(year, month, day)
    }
}

/// A date literal's body, as the reference's runtime reads ISO 8601 date
/// text: `YYYY-MM-DD` with an optional sign before the year.
fn read_date(body: &str) -> Option<Date> {
    let (mut fields, sign) = Fields::signed(body);
    let date = fields.date(sign)?;
    fields.bytes.is_empty().then_some(date)
}

/// A datetime literal's body, as the reference's runtime reads ISO 8601
/// datetime text, as the instant it names; one outside the years 0 to 9999
/// is refused; with the count of fraction digits it writes, up to six.
fn read_datetime(body: &str) -> Option<(DateTime, u8)> {
    let (mut fields, sign) = Fields::signed(body);
    let date = fields.date(sign)?;
    if !matches!(fields.next()?, b'T' | b' ') {
        return None;
    }
    let hour = fields.number(2)?;
    fields.literal(b':')?;
    let minute = fields.number(2)?;
    fields.literal(b':')?;
    let second = fields.number(2)?;
    let (microsecond, precision, rest) = microsecond(fields.bytes)?;
    let offset = offset(rest)?;
    let local = DateTime::new(date, hour, minute, second, microsecond)?;
    let instant = DateTime::from_unix(local.unix_seconds().checked_sub(offset)?, microsecond)?;
    (0..=9999)
        .contains(&instant.date().year())
        .then_some((instant, precision))
}

/// The fraction of a second after a full stop or a comma, as microseconds
/// (digits past the sixth dropped; at least one digit), how many digits
/// it writes up to six, and the text after it; 0, 0 and the text as it is
/// when no fraction is written.
fn microsecond(bytes: &[u8]) -> Option<(u32, u8, &[u8])> {
    let Some((b'.' | b',', rest)) = bytes.split_first() else {
        return Some((0, 0, bytes));
    };
    let count = rest.iter().take_while(|byte| byte.is_ascii_digit()).count();
    if count == 0 {
        return None;
    }
    let mut value = 0;
    for place in 0..6 {
        let digit = rest
            .get(place)
            .filter(|byte| place < count && byte.is_ascii_digit())
            .map_or(0, |byte| u32::from(byte - b'0'));
        value = value * 10 + digit;
    }
    let precision = u8::try_from(count.min(6)).unwrap_or(6);
    Some((value, precision, rest.get(count..).unwrap_or_default()))
}

/// The offset from UTC in seconds that the whole of `bytes` names, as the
/// reference's runtime reads it: `Z`; `+HH:MM`, `+HHMM` or `+HH` with
/// either sign, where each two-character field is read as an integer that
/// may carry a sign of its own and must be under 24 (hours) or 60
/// (minutes); `-00:00` is refused, and so is no offset at all.
fn offset(bytes: &[u8]) -> Option<i64> {
    if bytes == b"Z" {
        return Some(0);
    }
    if bytes == b"-00:00" {
        return None;
    }
    let (sign, rest) = match bytes.split_first()? {
        (b'+', rest) => (1, rest),
        (b'-', rest) => (-1, rest),
        _ => return None,
    };
    let (hour, minute, tail) = match rest {
        [h1, h2, b':', m1, m2, tail @ ..] => ([*h1, *h2], [*m1, *m2], tail),
        [h1, h2, m1, m2, tail @ ..] => ([*h1, *h2], [*m1, *m2], tail),
        [h1, h2, tail @ ..] => ([*h1, *h2], *b"00", tail),
        _ => return None,
    };
    let hour = field(hour).filter(|hour| *hour < 24)?;
    let minute = field(minute).filter(|minute| *minute < 60)?;
    tail.is_empty().then_some((hour * 60 + minute) * 60 * sign)
}

/// A two-character integer: two digits, or a sign and one digit.
fn field(bytes: [u8; 2]) -> Option<i64> {
    let digit = |byte: u8| byte.is_ascii_digit().then(|| i64::from(byte - b'0'));
    match bytes {
        [b'+', units] => digit(units),
        [b'-', units] => digit(units).map(|units| -units),
        [tens, units] => Some(digit(tens)? * 10 + digit(units)?),
    }
}
