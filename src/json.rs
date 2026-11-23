//! A private JSON reader and writer that read and write the text the
//! reference does: RFC 8259 JSON, objects written with their keys in
//! codepoint order and no whitespace, strings escaped as the reference's
//! encoder escapes them, and floats in the reference's own spelling.
//!
//! Both walk without recursion and hold to the depth limit, so no text and
//! no value can exhaust the stack.

use crate::boundary::DEPTH_LIMIT;
use crate::{Key, Map, Value};

/// Why a text could not be read or a value written: a reason token and a
/// sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Refusal {
    pub(crate) reason: &'static str,
    pub(crate) message: String,
}

impl Refusal {
    pub(crate) fn new(reason: &'static str, message: impl Into<String>) -> Refusal {
        Refusal {
            reason,
            message: message.into(),
        }
    }
}

/// A refusal of a text, at a byte offset into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fault {
    pub(crate) refusal: Refusal,
    pub(crate) offset: usize,
}

/// A list or an object the reader is inside.
enum Frame {
    List(Vec<Value>),
    Object { map: Map, key: String, start: usize },
}

struct Reader<'a> {
    text: &'a str,
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn fault(&self, reason: &'static str, message: String, offset: usize) -> Fault {
        Fault {
            refusal: Refusal::new(reason, message),
            offset,
        }
    }

    /// The refusal of whatever is at the current offset.
    fn unexpected(&self) -> Fault {
        let message = match self.peek() {
            Some(byte) => format!("unexpected byte 0x{byte:02X} at position {}", self.at),
            None => format!("unexpected end of text at position {}", self.at),
        };
        self.fault("malformed_json", message, self.at)
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), Fault> {
        if self.peek() == Some(byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(self.unexpected())
        }
    }

    /// The text from `start` to the current offset.
    fn since(&self, start: usize) -> &str {
        self.text.get(start..self.at).unwrap_or_default()
    }

    fn literal(&mut self, word: &str, value: Value) -> Result<Value, Fault> {
        for expected in word.bytes() {
            self.expect(expected)?;
        }
        Ok(value)
    }

    fn digits(&mut self) -> Result<(), Fault> {
        if !self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            return Err(self.unexpected());
        }
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.at += 1;
        }
        Ok(())
    }

    /// A number: an integer when it has neither a fraction nor an exponent,
    /// refused outside `i64`; otherwise a float, refused when not finite.
    fn number(&mut self) -> Result<Value, Fault> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        if self.peek() == Some(b'0') {
            self.at += 1;
        } else {
            self.digits()?;
        }
        let mut float = false;
        if self.peek() == Some(b'.') {
            self.at += 1;
            self.digits()?;
            float = true;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            self.digits()?;
            float = true;
        }
        let text = self.since(start);
        if !float {
            return text.parse().map(Value::Integer).map_err(|_| {
                self.fault(
                    "integer_out_of_range",
                    format!("the integer {text} is outside the 64-bit signed range"),
                    start,
                )
            });
        }
        match text.parse::<f64>() {
            Ok(number) if number.is_finite() => Ok(Value::Float(number)),
            _ => Err(self.fault(
                "non_finite_number",
                format!("the number {text} is too large for a finite float"),
                start,
            )),
        }
    }

    fn hex4(&mut self) -> Result<u32, Fault> {
        let mut value = 0;
        for _ in 0..4 {
            let digit = self
                .peek()
                .and_then(|byte| char::from(byte).to_digit(16))
                .ok_or_else(|| self.unexpected())?;
            value = value * 16 + digit;
            self.at += 1;
        }
        Ok(value)
    }

    /// The character a `\u` escape names, a surrogate pair read whole.
    fn unicode_escape(&mut self, start: usize) -> Result<char, Fault> {
        let first = self.hex4()?;
        let code = if (0xD800..0xDC00).contains(&first) {
            if self.bytes.get(self.at..self.at + 2) != Some(br"\u".as_slice()) {
                return Err(self.lone_surrogate(start));
            }
            self.at += 2;
            let second = self.hex4()?;
            if !(0xDC00..0xE000).contains(&second) {
                return Err(self.lone_surrogate(start));
            }
            0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
        } else {
            first
        };
        char::from_u32(code).ok_or_else(|| self.lone_surrogate(start))
    }

    fn lone_surrogate(&self, start: usize) -> Fault {
        self.fault(
            "malformed_json",
            format!("a lone surrogate escape at position {start}"),
            start,
        )
    }

    fn string(&mut self) -> Result<String, Fault> {
        self.expect(b'"')?;
        let mut text = String::new();
        let mut run = self.at;
        loop {
            match self.peek() {
                Some(b'"') => {
                    text.push_str(self.since(run));
                    self.at += 1;
                    return Ok(text);
                }
                Some(b'\\') => {
                    text.push_str(self.since(run));
                    let start = self.at;
                    self.at += 1;
                    let escaped = match self.peek() {
                        Some(b'"') => '"',
                        Some(b'\\') => '\\',
                        Some(b'/') => '/',
                        Some(b'b') => '\u{8}',
                        Some(b'f') => '\u{c}',
                        Some(b'n') => '\n',
                        Some(b'r') => '\r',
                        Some(b't') => '\t',
                        Some(b'u') => {
                            self.at += 1;
                            text.push(self.unicode_escape(start)?);
                            run = self.at;
                            continue;
                        }
                        _ => return Err(self.unexpected()),
                    };
                    self.at += 1;
                    text.push(escaped);
                    run = self.at;
                }
                Some(byte) if byte >= 0x20 => self.at += 1,
                _ => return Err(self.unexpected()),
            }
        }
    }

    /// An object's key and the colon after it.
    fn key(&mut self) -> Result<String, Fault> {
        let key = self.string()?;
        self.skip_whitespace();
        self.expect(b':')?;
        Ok(key)
    }

    fn too_deep(&self) -> Fault {
        self.fault(
            "depth_limit_exceeded",
            format!("the text nests past {DEPTH_LIMIT} levels"),
            self.at,
        )
    }
}

/// Reads a whole JSON text into a value. Each object is handed to `finish`,
/// which answers the value it stands for (a map, or what a tag carries) or
/// refuses it; a refusal is placed at the object's opening brace. A key
/// given twice keeps its first value, as the reference's reader does.
pub(crate) fn read(
    text: &str,
    finish: &dyn Fn(Map) -> Result<Value, Refusal>,
) -> Result<Value, Fault> {
    let mut reader = Reader {
        text,
        bytes: text.as_bytes(),
        at: 0,
    };
    let mut frames: Vec<Frame> = Vec::new();
    'value: loop {
        reader.skip_whitespace();
        let start = reader.at;
        let mut value = match reader.peek() {
            Some(b'[' | b'{') if frames.len() >= DEPTH_LIMIT => {
                return Err(reader.too_deep());
            }
            Some(b'[') => {
                reader.at += 1;
                reader.skip_whitespace();
                if reader.peek() == Some(b']') {
                    reader.at += 1;
                    Value::List(Vec::new())
                } else {
                    frames.push(Frame::List(Vec::new()));
                    continue 'value;
                }
            }
            Some(b'{') => {
                reader.at += 1;
                reader.skip_whitespace();
                if reader.peek() == Some(b'}') {
                    reader.at += 1;
                    finish(Map::new()).map_err(|refusal| Fault {
                        refusal,
                        offset: start,
                    })?
                } else {
                    let key = reader.key()?;
                    frames.push(Frame::Object {
                        map: Map::new(),
                        key,
                        start,
                    });
                    continue 'value;
                }
            }
            Some(b'"') => Value::String(reader.string()?),
            Some(b't') => reader.literal("true", Value::Bool(true))?,
            Some(b'f') => reader.literal("false", Value::Bool(false))?,
            Some(b'n') => reader.literal("null", Value::Null)?,
            Some(b'-' | b'0'..=b'9') => reader.number()?,
            _ => return Err(reader.unexpected()),
        };
        loop {
            reader.skip_whitespace();
            match frames.pop() {
                None if reader.peek().is_none() => return Ok(value),
                None => return Err(reader.unexpected()),
                Some(Frame::List(mut items)) => {
                    items.push(value);
                    match reader.peek() {
                        Some(b',') => {
                            reader.at += 1;
                            frames.push(Frame::List(items));
                            continue 'value;
                        }
                        Some(b']') => {
                            reader.at += 1;
                            value = Value::List(items);
                        }
                        _ => return Err(reader.unexpected()),
                    }
                }
                Some(Frame::Object {
                    mut map,
                    key,
                    start,
                }) => {
                    let key = Key::String(key);
                    if !map.contains_key(&key) {
                        map.insert(key, value);
                    }
                    match reader.peek() {
                        Some(b',') => {
                            reader.at += 1;
                            reader.skip_whitespace();
                            let key = reader.key()?;
                            frames.push(Frame::Object { map, key, start });
                            continue 'value;
                        }
                        Some(b'}') => {
                            reader.at += 1;
                            value = finish(map).map_err(|refusal| Fault {
                                refusal,
                                offset: start,
                            })?;
                        }
                        _ => return Err(reader.unexpected()),
                    }
                }
            }
        }
    }
}

/// What a writer does with the values JSON has no form for (undefined, a
/// date, an instant, a duration) and with a map before it is written.
pub(crate) trait Extension {
    /// Writes `value`, which sits at text depth `depth`, onto `out`.
    fn write_other(&self, value: &Value, depth: usize, out: &mut String) -> Result<(), Refusal>;

    /// Refuses a map that cannot be written.
    fn check_map(&self, map: &Map) -> Result<(), Refusal>;
}

/// The refusal of a list, map or tag at a depth past the limit.
pub(crate) fn depth_check(depth: usize) -> Result<(), Refusal> {
    if depth > DEPTH_LIMIT {
        return Err(Refusal::new(
            "depth_limit_exceeded",
            format!("the value nests past {DEPTH_LIMIT} levels"),
        ));
    }
    Ok(())
}

/// Writes `text` as a JSON string, escaped as the reference's encoder
/// escapes it: the quote, the backslash and the control characters, the
/// five with short forms short and the rest as `\u` and four uppercase hex
/// digits; nothing else.
pub(crate) fn write_string(text: &str, out: &mut String) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            control if u32::from(control) < 0x20 => {
                out.push_str(&format!("\\u{:04X}", u32::from(control)));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

/// The magnitude from which a float is always written in exponent form.
const EXPONENT_FROM: f64 = 9_007_199_254_740_992.0;

/// A finite float in the reference's spelling, its host language's shortest
/// form: the fewest significant digits that read back as the same float,
/// placed in one of two forms,
///
/// - plain, with a point and a `.0` when the number is whole: `1230.0`,
///   `0.001`;
/// - exponent, one digit, a point, at least one more digit, `e` and the
///   exponent, signed only when negative: `1.0e16`, `1.5e-7`.
///
/// At a magnitude of 2^53 or more the exponent form is written; below it,
/// whichever is shorter, and the plain form when both are the same length.
/// A negative number carries a minus; zero is `0.0`, negative zero `-0.0`.
pub(crate) fn float_text(number: f64) -> String {
    let sign = if number.is_sign_negative() { "-" } else { "" };
    if number == 0.0 {
        return format!("{sign}0.0");
    }
    // Rust's exponent form writes the shortest round-trip digits.
    let (digits, point) = scientific(&format!("{:e}", number.abs()));
    let digits = even_on_tie(number.abs(), digits, point);
    let digits = digits.as_str();
    let exponent_form = exponent_form(digits, point);
    if number.abs() >= EXPONENT_FROM {
        return format!("{sign}{exponent_form}");
    }
    let plain_form = plain_form(digits, point);
    if plain_form.len() <= exponent_form.len() {
        format!("{sign}{plain_form}")
    } else {
        format!("{sign}{exponent_form}")
    }
}

/// The significant digits of a number written in Rust's exponent form, with
/// no trailing zeros, and where the point falls: the number is `0.DIGITS`
/// times ten to the power of the second value.
fn scientific(text: &str) -> (String, i64) {
    let (mantissa, exponent) = text.split_once('e').unwrap_or((text, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let point = exponent.parse::<i64>().unwrap_or(0) + 1;
    (digits.trim_end_matches('0').to_owned(), point)
}

/// The shortest digits the reference writes. When the float lies exactly
/// halfway between two shortest candidates that both read back as it, the
/// reference takes the one whose last digit is even, where Rust's own
/// formatting may take the other: `2242476775408840.25` is written
/// `2242476775408840.2`.
fn even_on_tie(magnitude: f64, digits: String, point: i64) -> String {
    let Some((head, last)) = digits
        .char_indices()
        .last()
        .and_then(|(at, last)| Some((digits.get(..at)?, last.to_digit(10)?)))
    else {
        return digits;
    };
    // The float's exact decimal expansion; 800 digits hold any f64.
    let (exact, exact_point) = scientific(&format!("{magnitude:.800e}"));
    if exact_point != point || last % 2 == 0 {
        return digits;
    }
    let other = if exact == format!("{head}{}5", last.saturating_sub(1)) {
        format!("{head}{}", last.saturating_sub(1))
    } else if last < 9 && exact == format!("{digits}5") {
        format!("{head}{}", last + 1)
    } else {
        return digits;
    };
    let other = other.trim_end_matches('0');
    let reads_back = format!("0.{other}e{point}").parse::<f64>() == Ok(magnitude);
    if reads_back { other.to_owned() } else { digits }
}

/// `0.DIGITS` times ten to the power `point`, written plain.
fn plain_form(digits: &str, point: i64) -> String {
    let zeros = |count: i64| "0".repeat(usize::try_from(count).unwrap_or(0));
    let length = i64::try_from(digits.len()).unwrap_or(i64::MAX);
    if point <= 0 {
        return format!("0.{}{digits}", zeros(-point));
    }
    if point >= length {
        return format!("{digits}{}.0", zeros(point - length));
    }
    let (whole, fraction) = digits
        .split_at_checked(usize::try_from(point).unwrap_or(0))
        .unwrap_or((digits, ""));
    format!("{whole}.{fraction}")
}

/// `0.DIGITS` times ten to the power `point`, in exponent form.
fn exponent_form(digits: &str, point: i64) -> String {
    let mut characters = digits.chars();
    let first = characters.next().unwrap_or('0');
    let rest = characters.as_str();
    let rest = if rest.is_empty() { "0" } else { rest };
    format!("{first}.{rest}e{}", point - 1)
}

/// The refusal of a map key JSON has no form for: an object's keys are
/// strings.
pub(crate) fn non_string_key(key: &Key) -> Refusal {
    Refusal::new(
        "non_string_map_key",
        format!("the map key {key:?} is not a string, and a JSON object's keys are"),
    )
}

/// One step of an iterative write.
enum Task<'a> {
    Value(&'a Value, usize),
    Text(&'static str),
    Key(&'a str),
}

/// Writes `value` as JSON text with no whitespace and the keys of each map
/// in order, handing what JSON has no form for to `extension`. A list or a
/// map past the depth limit, and a float that is not finite, are refused.
pub(crate) fn write(value: &Value, extension: &dyn Extension) -> Result<String, Refusal> {
    let mut out = String::new();
    let mut work = vec![Task::Value(value, 1)];
    while let Some(task) = work.pop() {
        match task {
            Task::Text(text) => out.push_str(text),
            Task::Key(key) => {
                write_string(key, &mut out);
                out.push(':');
            }
            Task::Value(value, depth) => match value {
                Value::Null => out.push_str("null"),
                Value::Bool(true) => out.push_str("true"),
                Value::Bool(false) => out.push_str("false"),
                Value::Integer(number) => out.push_str(&number.to_string()),
                Value::Float(number) if number.is_finite() => out.push_str(&float_text(*number)),
                Value::Float(number) => {
                    return Err(Refusal::new(
                        "non_finite_number",
                        format!("the float {number} is not finite"),
                    ));
                }
                Value::String(text) => write_string(text, &mut out),
                Value::List(items) => {
                    depth_check(depth)?;
                    out.push('[');
                    work.push(Task::Text("]"));
                    for (index, item) in items.iter().enumerate().rev() {
                        work.push(Task::Value(item, depth + 1));
                        if index > 0 {
                            work.push(Task::Text(","));
                        }
                    }
                }
                Value::Map(map) => {
                    depth_check(depth)?;
                    extension.check_map(map)?;
                    out.push('{');
                    work.push(Task::Text("}"));
                    for (index, (key, item)) in map.entries.iter().enumerate().rev() {
                        let key = match key {
                            Key::String(key) => key.as_str(),
                            _ => return Err(non_string_key(key)),
                        };
                        work.push(Task::Value(item, depth + 1));
                        work.push(Task::Key(key));
                        if index > 0 {
                            work.push(Task::Text(","));
                        }
                    }
                }
                other => extension.write_other(other, depth, &mut out)?,
            },
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plain JSON: every object is a map, and nothing else is written.
    struct Plain;

    impl Extension for Plain {
        fn write_other(&self, _: &Value, _: usize, out: &mut String) -> Result<(), Refusal> {
            out.push_str("\"other\"");
            Ok(())
        }

        fn check_map(&self, _: &Map) -> Result<(), Refusal> {
            Ok(())
        }
    }

    fn plain(text: &str) -> Result<Value, Fault> {
        read(text, &|map| Ok(Value::Map(map)))
    }

    fn refused(text: &str) -> (&'static str, usize) {
        let fault = plain(text).unwrap_err();
        (fault.refusal.reason, fault.offset)
    }

    // Sabotage: with EXPONENT_FROM raised to 1e17, or with even_on_tie
    // answering Rust's digits unchanged, this test fails.
    #[test]
    fn floats_are_written_as_the_reference_writes_them() {
        // Each pair was read from the reference's own float text.
        let pinned = [
            (1.0e16, "1.0e16"),
            (9_007_199_254_740_992.0, "9.007199254740992e15"),
            (9_007_199_254_740_991.0, "9007199254740991.0"),
            (123_456_789_012_345.0, "123456789012345.0"),
            (1.0e15, "1.0e15"),
            (1.0e-5, "1.0e-5"),
            (0.001, "0.001"),
            (0.0001, "0.0001"),
            (1.5e-7, "1.5e-7"),
            (1.0e22, "1.0e22"),
            (0.1, "0.1"),
            (-0.0, "-0.0"),
            (0.0, "0.0"),
            (5.0e-324, "5.0e-324"),
            (1.797_693_134_862_315_7e308, "1.7976931348623157e308"),
            (1234.5, "1234.5"),
            (100.0, "100.0"),
            (-2.5, "-2.5"),
            (1.0e10, "1.0e10"),
            (0.00012, "1.2e-4"),
            (std::f64::consts::SQRT_2, "1.4142135623730951"),
            // Each exactly halfway between two shortest candidates, the
            // last digits 2 and 3: the reference takes the even one.
            (2_242_476_775_408_840.0 + 0.25, "2242476775408840.2"),
            (2_216_607_000_000_000.0 + 0.25, "2216607000000000.2"),
            (2_240_000_000_000_000.0 + 0.25, "2240000000000000.2"),
        ];
        for (number, text) in pinned {
            assert_eq!(float_text(number), text, "{number:e}");
            assert_eq!(plain(text).unwrap(), Value::Float(number));
        }
    }

    // Sabotage: with write_string's control-character escape written in
    // lowercase hex, this test fails.
    #[test]
    fn strings_are_escaped_as_the_reference_escapes_them() {
        let mut out = String::new();
        write_string(
            "a/b\u{1}\t\n\"\\é\u{2028}\u{7f}\u{1b}\u{8}\u{c}\r\u{1f}",
            &mut out,
        );
        assert_eq!(
            out,
            "\"a/b\\u0001\\t\\n\\\"\\\\é\u{2028}\u{7f}\\u001B\\b\\f\\r\\u001F\""
        );
        assert_eq!(
            plain(&out).unwrap(),
            Value::from("a/b\u{1}\t\n\"\\é\u{2028}\u{7f}\u{1b}\u{8}\u{c}\r\u{1f}")
        );
        assert_eq!(
            plain(r#""\/\u00e9\ud83d\ude00""#).unwrap(),
            Value::from("/é\u{1f600}")
        );
    }

    // Sabotage: with a key given twice keeping its last value, this test
    // fails.
    #[test]
    fn the_reader_takes_rfc_8259_json() {
        assert_eq!(
            plain(" [1 , 2.0,-0, -0.0, 1E2,true,false,null] ").unwrap(),
            Value::from(vec![
                1.into(),
                2.0.into(),
                0.into(),
                (-0.0).into(),
                100.0.into(),
                true.into(),
                false.into(),
                Value::Null,
            ])
        );
        let object = plain(r#"{"b":{},"a":[],"a":1}"#).unwrap();
        let Value::Map(map) = &object else {
            panic!("{object:?}")
        };
        assert_eq!(map.get(&"a".into()), Some(&Value::List(vec![])));
        assert_eq!(map.get(&"b".into()), Some(&Value::Map(Map::new())));
        assert_eq!(
            plain("9223372036854775807").unwrap(),
            Value::Integer(i64::MAX)
        );
        assert_eq!(
            plain("-9223372036854775808").unwrap(),
            Value::Integer(i64::MIN)
        );
        assert_eq!(plain("1e-400").unwrap(), Value::Float(0.0));
    }

    // Sabotage: with the integer arm of Reader::number reading an integer
    // that overflows as 0, this test fails.
    #[test]
    fn the_reader_refuses_by_name_and_offset() {
        assert_eq!(refused("9223372036854775808"), ("integer_out_of_range", 0));
        assert_eq!(
            refused("[-9223372036854775809]"),
            ("integer_out_of_range", 1)
        );
        assert_eq!(refused("1e400"), ("non_finite_number", 0));
        assert_eq!(refused("01"), ("malformed_json", 1));
        assert_eq!(refused("[1,]"), ("malformed_json", 3));
        assert_eq!(refused("[1 2]"), ("malformed_json", 3));
        assert_eq!(refused("{\"a\" 1}"), ("malformed_json", 5));
        assert_eq!(refused("{\"a\":1 \"b\":2}"), ("malformed_json", 7));
        assert_eq!(refused("{1:2}"), ("malformed_json", 1));
        assert_eq!(refused("\"\\ud800\""), ("malformed_json", 1));
        assert_eq!(refused("\"\\ud800\\u0041\""), ("malformed_json", 1));
        assert_eq!(refused("\"\\udc00\""), ("malformed_json", 1));
        assert_eq!(refused("\"\\x\""), ("malformed_json", 2));
        assert_eq!(refused("\"\\u12g4\""), ("malformed_json", 5));
        assert_eq!(refused("\"a\u{1}\""), ("malformed_json", 2));
        assert_eq!(refused("\"open"), ("malformed_json", 5));
        assert_eq!(refused("tru"), ("malformed_json", 3));
        assert_eq!(refused("1."), ("malformed_json", 2));
        assert_eq!(refused("-"), ("malformed_json", 1));
        assert_eq!(refused("1e+"), ("malformed_json", 3));
        assert_eq!(refused(""), ("malformed_json", 0));
        assert_eq!(refused("1 1"), ("malformed_json", 2));
        assert_eq!(refused("[1"), ("malformed_json", 2));
        assert_eq!(refused("}"), ("malformed_json", 0));
        let message = plain("[o]").unwrap_err().refusal.message;
        assert_eq!(message, "unexpected byte 0x6F at position 1");
        let message = plain("[").unwrap_err().refusal.message;
        assert_eq!(message, "unexpected end of text at position 1");
    }

    // Sabotage: with the reader's depth test frames.len() > DEPTH_LIMIT,
    // this test fails.
    #[test]
    fn the_reader_holds_one_hundred_levels() {
        let text = |levels: usize| format!("{}{}", "[".repeat(levels), "]".repeat(levels));
        assert!(plain(&text(100)).is_ok());
        assert_eq!(refused(&text(101)), ("depth_limit_exceeded", 100));
        assert_eq!(
            refused(&"{\"a\":".repeat(101)),
            ("depth_limit_exceeded", 500)
        );
        assert_eq!(
            refused(&"[".repeat(1_000_000)),
            ("depth_limit_exceeded", 100)
        );
    }

    // Sabotage: with the map arm of write pushing a separator before its
    // first entry, this test fails.
    #[test]
    fn the_writer_writes_compact_ordered_json() {
        let value =
            plain(r#" { "z" : [ 1 , 2.5 , "x" ] , "a" : { } , "m": [null,true,false] } "#).unwrap();
        assert_eq!(
            write(&value, &Plain).unwrap(),
            r#"{"a":{},"m":[null,true,false],"z":[1,2.5,"x"]}"#
        );
        assert_eq!(write(&Value::Undefined, &Plain).unwrap(), "\"other\"");
        let infinite = Value::from(vec![Value::Float(f64::INFINITY)]);
        assert_eq!(
            write(&infinite, &Plain).unwrap_err().reason,
            "non_finite_number"
        );
        let mut deep = Value::Null;
        for _ in 0..101 {
            deep = Value::from(vec![deep]);
        }
        assert_eq!(
            write(&deep, &Plain).unwrap_err().reason,
            "depth_limit_exceeded"
        );
    }
}
