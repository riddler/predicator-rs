//! The conformance corpus's tagged-value encoding, as text.
//!
//! JSON carries most predicator values as themselves: null, booleans,
//! integers, floats, strings, lists and maps (objects). Four do not fit, and
//! the encoding carries each as an object with a `$type` key:
//!
//! ```text
//! {"$type":"undefined"}
//! {"$type":"date","value":"2026-08-06"}
//! {"$type":"datetime","value":"2026-08-06T12:00:00Z"}
//! {"$type":"duration","value":{"days":3,"hours":0,"minutes":0,"months":0,"seconds":0,"weeks":0,"years":0}}
//! ```
//!
//! The encoding is the corpus's apparatus, specified by the reference's
//! `conformance/README.md` at the tag this crate's vendored corpus names. It
//! is offered for a host that keeps values or instruction lists as JSON.
//!
//! [`encode`] writes the text the reference writes: no whitespace, the keys
//! of every object in codepoint order, floats in the reference's spelling, a
//! datetime's fraction left out when it is zero and six digits when it is
//! not, and a duration's `milliseconds` written only when it is not zero.
//! [`decode`] reads any JSON text, which may also name an instant with an
//! offset or a fraction of any length; an integer stays an integer and a
//! float a float.
//!
//! ```
//! use predicator_rs::{tagged, Value};
//!
//! let value = tagged::decode(r#"{"x":{"$type":"undefined"},"y":[1,1.0]}"#).unwrap();
//! let Value::Map(map) = &value else { panic!() };
//! assert_eq!(map.get(&"x".into()), Some(&Value::Undefined));
//! assert_eq!(map.get(&"y".into()), Some(&Value::List(vec![1.into(), 1.0.into()])));
//! assert_eq!(tagged::encode(&value).unwrap(), r#"{"x":{"$type":"undefined"},"y":[1,1.0]}"#);
//! ```

use crate::boundary;
use crate::json::{self, Extension, Refusal};
use crate::{
    Date, DateTime, Duration, Error, EvaluationError, Key, Map, ParseError, Position, Value,
};

/// The units a duration's tag always carries; `milliseconds` is carried
/// only when it is not zero.
const REQUIRED_UNITS: [&str; 7] = [
    "years", "months", "weeks", "days", "hours", "minutes", "seconds",
];

/// Reads a text in the tagged encoding into a value.
///
/// Every refusal is an [`Error::Parse`] at the position of the fault, with
/// one of these reasons:
///
/// - `malformed_json`: the text is not JSON;
/// - `depth_limit_exceeded`: lists and objects nest past 100 levels, a
///   tag's own braces counted;
/// - `integer_out_of_range`: an integer (a number with neither a fraction
///   nor an exponent) outside the 64-bit signed range;
/// - `non_finite_number`: a number too large for a finite float;
/// - `invalid_tagged_value`: an object with a `$type` key that is not one of
///   the four tags, or a tag whose value is not what the tag carries.
///
/// ```
/// use predicator_rs::{tagged, Value};
///
/// assert_eq!(tagged::decode("[1,null]").unwrap(), Value::from(vec![1.into(), Value::Null]));
/// let refused = tagged::decode("[1, 99999999999999999999]").unwrap_err();
/// assert_eq!(refused.type_name(), "ParseError");
/// assert_eq!(refused.reason(), "integer_out_of_range");
/// assert_eq!(refused.position().map(|at| at.column), Some(5));
/// ```
pub fn decode(text: &str) -> Result<Value, Error> {
    json::read(text, &untag).map_err(|fault| {
        ParseError::new(fault.refusal.reason, fault.refusal.message)
            .with_position(position_of(text, fault.offset))
            .into()
    })
}

/// Writes a value as text in the tagged encoding.
///
/// Every refusal is an [`Error::Evaluation`], with one of these reasons:
///
/// - `non_finite_number`: a float that is not finite;
/// - `depth_limit_exceeded`: lists and maps nested past 100 levels, or a
///   text that would, a tag's own braces counted;
/// - `reserved_map_key`: a map with a `$type` key, which would read back as
///   a tag;
/// - `non_string_map_key`: a map key that is not a string (an integer, a
///   boolean or undefined), for which a JSON object has no form;
/// - `invalid_tagged_value`: a date or an instant outside the years 0 to
///   9999 the encoding carries.
///
/// ```
/// use predicator_rs::{tagged, Date, Map, Value};
///
/// let date = Date::new(2026, 8, 6).unwrap();
/// let value = Value::from(vec![Value::from(1), Value::from(1.0), Value::from(date)]);
/// assert_eq!(
///     tagged::encode(&value).unwrap(),
///     r#"[1,1.0,{"$type":"date","value":"2026-08-06"}]"#
/// );
/// let reserved: Map = [("$type", Value::from("x"))].into_iter().collect();
/// assert_eq!(tagged::encode(&reserved.into()).unwrap_err().reason(), "reserved_map_key");
/// ```
pub fn encode(value: &Value) -> Result<String, Error> {
    boundary::check(value)?;
    json::write(value, &Tagged)
        .map_err(|refusal| EvaluationError::new(refusal.reason, refusal.message).into())
}

/// The line and column of a byte offset into `text`, both from 1.
fn position_of(text: &str, offset: usize) -> Position {
    let before = text.get(..offset).unwrap_or(text);
    let line_start = before.rfind('\n').map_or(0, |at| at + 1);
    Position {
        line: before.matches('\n').count() + 1,
        column: before.get(line_start..).unwrap_or_default().chars().count() + 1,
    }
}

fn invalid(message: String) -> Refusal {
    Refusal::new("invalid_tagged_value", message)
}

/// The value an object stands for: the tag it carries, or the map itself
/// when it has no `$type` key.
pub(crate) fn untag(map: Map) -> Result<Value, Refusal> {
    let Some(tag) = map.get_str("$type") else {
        return Ok(Value::Map(map));
    };
    let Value::String(tag) = tag else {
        return Err(invalid(
            "a $type that is not a string is not a tag".to_owned(),
        ));
    };
    let refused = || {
        let keys: Vec<&Key> = map.keys().collect();
        invalid(format!(
            "a {tag:?} tag with the keys {keys:?} is not a tagged value"
        ))
    };
    let value = map.get_str("value");
    let tagged = match (tag.as_str(), map.len(), value) {
        ("undefined", 1, _) => Some(Value::Undefined),
        ("date", 2, Some(Value::String(text))) => Date::parse(text).map(Value::Date),
        ("datetime", 2, Some(Value::String(text))) => DateTime::parse(text).map(Value::DateTime),
        ("duration", 2, Some(Value::Map(units))) => duration(units).map(Value::Duration),
        _ => None,
    };
    tagged.ok_or_else(refused)
}

/// The duration a tag's value map carries: the seven units always written,
/// `milliseconds` when it is there, each an integer, and nothing else.
pub(crate) fn duration(units: &Map) -> Option<Duration> {
    let mut duration = Duration::default();
    for (name, amount) in units.iter() {
        let (Key::String(name), Value::Integer(amount)) = (name, amount) else {
            return None;
        };
        *duration.unit_mut(name)? = *amount;
    }
    REQUIRED_UNITS
        .iter()
        .all(|unit| units.get_str(unit).is_some())
        .then_some(duration)
}

/// The text a date or an instant's year allows the encoding to carry.
pub(crate) fn carried(year: i32, text: String) -> Result<String, Refusal> {
    if (0..=9999).contains(&year) {
        Ok(text)
    } else {
        Err(invalid(format!(
            "{text} is outside the years 0 to 9999 the encoding carries"
        )))
    }
}

/// The refusal of a map with a `$type` key.
pub(crate) fn check_map(map: &Map) -> Result<(), Refusal> {
    if map.get_str("$type").is_some() {
        return Err(Refusal::new(
            "reserved_map_key",
            "a map with a \"$type\" key would read back as a tag",
        ));
    }
    Ok(())
}

/// A duration's tag value, its units in key order, `milliseconds` only when
/// it is not zero.
fn duration_text(duration: &Duration, out: &mut String) {
    let mut units: Vec<(&str, i64)> = duration
        .units()
        .into_iter()
        .filter(|(name, _, amount)| *name != "milliseconds" || *amount != 0)
        .map(|(name, _, amount)| (name, amount))
        .collect();
    units.sort_unstable_by_key(|(name, _)| *name);
    out.push('{');
    for (index, (name, amount)) in units.into_iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&format!("\"{name}\":{amount}"));
    }
    out.push('}');
}

/// The tagged encoding's writing of the four values JSON has no form for.
struct Tagged;

impl Extension for Tagged {
    fn write_other(&self, value: &Value, depth: usize, out: &mut String) -> Result<(), Refusal> {
        json::depth_check(depth)?;
        match value {
            Value::Date(date) => {
                let text = carried(date.year(), date.to_string())?;
                out.push_str(&format!("{{\"$type\":\"date\",\"value\":\"{text}\"}}"));
            }
            Value::DateTime(instant) => {
                let text = carried(instant.date().year(), instant.to_string())?;
                out.push_str(&format!("{{\"$type\":\"datetime\",\"value\":\"{text}\"}}"));
            }
            Value::Duration(duration) => {
                json::depth_check(depth + 1)?;
                out.push_str("{\"$type\":\"duration\",\"value\":");
                duration_text(duration, out);
                out.push('}');
            }
            _ => out.push_str("{\"$type\":\"undefined\"}"),
        }
        Ok(())
    }

    fn check_map(&self, map: &Map) -> Result<(), Refusal> {
        check_map(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reason(text: &str) -> String {
        decode(text).unwrap_err().reason().to_owned()
    }

    fn date(year: i32, month: u32, day: u32) -> Date {
        Date::new(year, month, day).unwrap()
    }

    // Sabotage: with untag answering Null for an undefined tag, this test
    // fails.
    #[test]
    fn each_kind_round_trips_through_its_text() {
        let instant = DateTime::new(date(2026, 8, 9), 10, 30, 0, 500_000).unwrap();
        let pairs = [
            (Value::Null, "null"),
            (Value::Undefined, r#"{"$type":"undefined"}"#),
            (Value::from(true), "true"),
            (Value::from(-7), "-7"),
            (Value::from(3.0), "3.0"),
            (Value::from("x"), r#""x""#),
            (Value::from(vec![1.into(), 1.0.into()]), "[1,1.0]"),
            (
                Value::from(
                    [("b", Value::Null), ("a", Value::from(1))]
                        .into_iter()
                        .collect::<Map>(),
                ),
                r#"{"a":1,"b":null}"#,
            ),
            (
                Value::from(date(2026, 8, 9)),
                r#"{"$type":"date","value":"2026-08-09"}"#,
            ),
            (
                Value::from(instant),
                r#"{"$type":"datetime","value":"2026-08-09T10:30:00.500000Z"}"#,
            ),
            (
                Value::from(DateTime::from_unix(0, 0).unwrap()),
                r#"{"$type":"datetime","value":"1970-01-01T00:00:00Z"}"#,
            ),
            (
                Value::from(Duration {
                    weeks: 2,
                    days: 3,
                    ..Duration::default()
                }),
                r#"{"$type":"duration","value":{"days":3,"hours":0,"minutes":0,"months":0,"seconds":0,"weeks":2,"years":0}}"#,
            ),
            (
                Value::from(Duration {
                    seconds: 1,
                    milliseconds: 500,
                    ..Duration::default()
                }),
                r#"{"$type":"duration","value":{"days":0,"hours":0,"milliseconds":500,"minutes":0,"months":0,"seconds":1,"weeks":0,"years":0}}"#,
            ),
            (
                Value::from(Duration {
                    days: -3,
                    ..Duration::default()
                }),
                r#"{"$type":"duration","value":{"days":-3,"hours":0,"minutes":0,"months":0,"seconds":0,"weeks":0,"years":0}}"#,
            ),
        ];
        for (value, text) in pairs {
            assert_eq!(encode(&value).unwrap(), text);
            assert_eq!(decode(text).unwrap(), value, "{text}");
        }
    }

    // Sabotage: with duration accepting a value map that lacks a required
    // unit, this test fails.
    #[test]
    fn a_tag_that_does_not_carry_its_value_is_refused() {
        let units = r#""days":0,"hours":0,"minutes":0,"months":0,"seconds":0,"weeks":0"#;
        for text in [
            r#"{"$type":"nope"}"#,
            r#"{"$type":1}"#,
            r#"{"$type":"undefined","value":1}"#,
            r#"{"$type":"date","value":"2026-02-30"}"#,
            r#"{"$type":"date","value":20260806}"#,
            r#"{"$type":"date"}"#,
            r#"{"$type":"datetime","value":"2026-08-06"}"#,
            r#"{"$type":"datetime","value":"2026-08-06T12:00:00Z","x":1}"#,
            &format!(r#"{{"$type":"duration","value":{{{units}}}}}"#),
            &format!(r#"{{"$type":"duration","value":{{{units},"years":0.5}}}}"#),
            &format!(r#"{{"$type":"duration","value":{{{units},"years":0,"decades":0}}}}"#),
            r#"{"$type":"duration","value":[]}"#,
        ] {
            assert_eq!(reason(text), "invalid_tagged_value", "{text}");
        }
        let decoded = decode(&format!(
            r#"{{"$type":"duration","value":{{{units},"years":0}}}}"#
        ));
        assert_eq!(decoded.unwrap(), Value::from(Duration::default()));
        let offset = decode(r#"{"$type":"datetime","value":"2026-08-06T12:00:00+01:30"}"#);
        assert_eq!(
            encode(&offset.unwrap()).unwrap(),
            r#"{"$type":"datetime","value":"2026-08-06T10:30:00Z"}"#
        );
    }

    // Sabotage: with decode's error left at no position, this test fails.
    #[test]
    fn a_decode_refusal_is_a_parse_error_at_its_position() {
        let refused = decode("[1,\n  {\"$type\":\"nope\"}]").unwrap_err();
        assert_eq!(refused.type_name(), "ParseError");
        assert_eq!(refused.position(), Some(Position { line: 2, column: 3 }));
        assert_eq!(reason("[1e400]"), "non_finite_number");
        assert_eq!(reason("[9223372036854775808]"), "integer_out_of_range");
        assert_eq!(reason("{"), "malformed_json");
        let refused = decode("\"é\" x").unwrap_err();
        assert_eq!(refused.position(), Some(Position { line: 1, column: 5 }));
    }

    // Sabotage: with write_other's depth check removed, this test fails.
    #[test]
    fn a_tag_counts_toward_the_depth_limit() {
        let wrap = |mut value: Value, levels: usize| {
            for _ in 0..levels {
                value = Value::from(vec![value]);
            }
            value
        };
        assert!(encode(&wrap(Value::Undefined, 99)).is_ok());
        assert_eq!(
            encode(&wrap(Value::Undefined, 100)).unwrap_err().reason(),
            "depth_limit_exceeded"
        );
        let duration = Value::from(Duration::default());
        assert!(encode(&wrap(duration.clone(), 98)).is_ok());
        assert_eq!(
            encode(&wrap(duration, 99)).unwrap_err().reason(),
            "depth_limit_exceeded"
        );
        let text = encode(&wrap(Value::from(Duration::default()), 98)).unwrap();
        assert!(decode(&text).is_ok());
        assert_eq!(reason(&format!("[{text}]")), "depth_limit_exceeded");
    }

    // Sabotage: with the writer writing a non-string key as a string key,
    // this test fails.
    #[test]
    fn a_map_key_that_is_not_a_string_has_no_tagged_form() {
        for key in [Key::from(1), Key::from(true), Key::Undefined] {
            let map: Map = [(key, Value::Null)].into_iter().collect();
            let refused = encode(&Value::from(vec![map.into()]))
                .map_err(|error| (error.type_name(), error.reason().to_owned()));
            assert_eq!(
                refused,
                Err(("EvaluationError", "non_string_map_key".to_owned()))
            );
        }
        let decoded = decode(r#"{"1":1,"true":2,"undefined":3}"#).unwrap();
        let Value::Map(map) = &decoded else {
            panic!("{decoded:?}")
        };
        assert_eq!(map.get(&"1".into()), Some(&Value::from(1)));
        assert_eq!(map.get(&Key::from(1)), None);
        assert_eq!(map.get(&Key::from(true)), None);
        assert_eq!(map.get(&Key::Undefined), None);
    }

    // Sabotage: with carried admitting the year 10000, this test fails.
    #[test]
    fn an_encode_refusal_is_an_evaluation_error() {
        let refused = |value: Value| {
            let error = encode(&value).unwrap_err();
            assert_eq!(error.type_name(), "EvaluationError");
            error.reason().to_owned()
        };
        assert_eq!(refused(Value::from(f64::NAN)), "non_finite_number");
        assert_eq!(
            refused(Value::from(date(10_000, 1, 1))),
            "invalid_tagged_value"
        );
        assert_eq!(
            refused(Value::from(date(-1, 12, 31))),
            "invalid_tagged_value"
        );
        let far = DateTime::new(date(10_000, 1, 1), 0, 0, 0, 0).unwrap();
        assert_eq!(refused(Value::from(far)), "invalid_tagged_value");
        let inner: Map = [("$type", Value::from("date"))].into_iter().collect();
        assert_eq!(refused(Value::from(vec![inner.into()])), "reserved_map_key");
        assert!(encode(&Value::from(date(0, 1, 1))).is_ok());
        assert!(encode(&Value::from(date(9999, 12, 31))).is_ok());
    }
}
