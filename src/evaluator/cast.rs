//! The `cast` instruction, as the reference's instruction-set text states
//! its conversion matrix, with the reference's code answering where the
//! text is silent.
//!
//! Two rules make every cell: undefined casts to undefined, and a value
//! that cannot become one of the target type casts to undefined too, never
//! to an error. Null casts to undefined for every target, by the second
//! rule.
//!
//! | from \ to | integer | float | string | boolean | date | datetime | duration |
//! |---|---|---|---|---|---|---|---|
//! | integer | = | widen | text | - | - | - | - |
//! | float | truncate | = | text | - | - | - | - |
//! | string | read | read | = | read | read | read | read |
//! | boolean | - | - | text | = | - | - | - |
//! | date | - | - | text | - | = | midnight UTC | - |
//! | datetime | - | - | text | - | its date | = | - |
//! | duration | - | - | text | - | - | - | = |
//!
//! `=` passes the value through and `-` is undefined; a list, a map, null
//! and undefined are `-` for every target. A string is read whole or not at
//! all, with no trimming: an integer as `-?[0-9]+`, a float as
//! `-?[0-9]+(\.[0-9]+)?`, a boolean as exactly `true` or `false`, a date as
//! `YYYY-MM-DD`, an instant as the text the tagged codec reads (a date, a
//! time, an offset; [`DateTime::parse`]), and a duration as the
//! duration-literal grammar ([`Duration::parse`]). A float is written as
//! the reference writes one (`json::float_text`), an instant in UTC with
//! the fraction left out when it is zero and six digits when it is not.
//!
//! Where the answer is a value this crate cannot hold, or one the reference
//! raises on, the cast fails the run; [`refusal`] is the one place each
//! such refusal is made.

use crate::civil::DurationText;
use crate::{Date, DateTime, Duration, EvaluationError, Value};

/// The seven type names a cast converts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    Integer,
    Float,
    String,
    Boolean,
    Date,
    DateTime,
    Duration,
}

impl Target {
    /// The target `name` names, or `None` when it names none.
    pub(super) fn named(name: &str) -> Option<Target> {
        Some(match name {
            "integer" => Target::Integer,
            "float" => Target::Float,
            "string" => Target::String,
            "boolean" => Target::Boolean,
            "date" => Target::Date,
            "datetime" => Target::DateTime,
            "duration" => Target::Duration,
            _ => return None,
        })
    }
}

/// Why a cast fails the run rather than answer a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refused {
    /// The integer the cast answers is outside `i64`; the reference's
    /// integers have no bound.
    IntegerOverflow,
    /// The string names a number too large for a finite float; the
    /// reference raises.
    NonFinite,
    /// The instant is outside the years 0 to 9999 an instant's text
    /// carries; the reference raises past 9999.
    OutOfRange,
}

/// The refusal of a cast to `target`: the one function that turns a cast
/// the reference raises on, or answers with a value this crate cannot hold,
/// into an error.
///
/// | cast | reason |
/// |---|---|
/// | a string or a float to an integer outside `i64` | `integer_overflow` |
/// | a string to a duration with a unit outside `i64` | `integer_overflow` |
/// | a string to a float too large for a finite float | `non_finite_number` |
/// | an instant outside the years 0 to 9999 to a string | `date_out_of_range` |
fn refusal(refused: Refused, target: Target) -> EvaluationError {
    let target = format!("{target:?}").to_lowercase();
    match refused {
        Refused::IntegerOverflow => EvaluationError::new(
            "integer_overflow",
            format!("the cast to {target} answers an integer outside the 64-bit range"),
        ),
        Refused::NonFinite => EvaluationError::new(
            "non_finite_number",
            format!("the cast to {target} answers a number too large for a finite float"),
        ),
        Refused::OutOfRange => EvaluationError::new(
            "date_out_of_range",
            format!("the cast to {target} takes an instant outside the years 0 to 9999"),
        ),
    }
}

/// `cast`: `value` converted to `target`, undefined when it cannot be one,
/// or the refusal of a cast this crate cannot answer.
pub(super) fn cast(value: Value, target: Target) -> Result<Value, EvaluationError> {
    convert(value, target).map_err(|refused| refusal(refused, target))
}

fn convert(value: Value, target: Target) -> Result<Value, Refused> {
    Ok(match (value, target) {
        (value @ Value::Integer(_), Target::Integer)
        | (value @ Value::Float(_), Target::Float)
        | (value @ Value::String(_), Target::String)
        | (value @ Value::Bool(_), Target::Boolean)
        | (value @ Value::Date(_), Target::Date)
        | (value @ Value::DateTime(_), Target::DateTime)
        | (value @ Value::Duration(_), Target::Duration) => value,
        // Widened to the nearest float, ties to even: exact up to 2^53.
        (Value::Integer(number), Target::Float) => Value::Float(number as f64),
        (Value::Float(number), Target::Integer) => Value::Integer(truncate(number)?),
        (Value::Integer(number), Target::String) => Value::String(number.to_string()),
        (Value::Float(number), Target::String) => Value::String(crate::json::float_text(number)),
        (Value::Bool(flag), Target::String) => Value::String(flag.to_string()),
        (Value::Date(date), Target::String) => Value::String(date.to_string()),
        (Value::DateTime(instant), Target::String) => Value::String(instant_text(instant)?),
        (Value::Duration(duration), Target::String) => Value::String(duration.to_string()),
        (Value::Date(date), Target::DateTime) => midnight(date),
        (Value::DateTime(instant), Target::Date) => Value::Date(instant.date()),
        (Value::String(ref text), target) => read(text, target)?,
        _ => Value::Undefined,
    })
}

/// A string read as `target`, whole or not at all.
fn read(text: &str, target: Target) -> Result<Value, Refused> {
    Ok(match target {
        Target::Integer if is_decimal(text, false) => text
            .parse::<i64>()
            .map(Value::Integer)
            .map_err(|_| Refused::IntegerOverflow)?,
        Target::Float if is_decimal(text, true) => {
            let number = text.parse::<f64>().map_err(|_| Refused::NonFinite)?;
            if !number.is_finite() {
                return Err(Refused::NonFinite);
            }
            Value::Float(number)
        }
        Target::Boolean => match text {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => Value::Undefined,
        },
        Target::Date => Date::parse(text).map_or(Value::Undefined, Value::Date),
        Target::DateTime => DateTime::parse(text).map_or(Value::Undefined, Value::DateTime),
        Target::Duration => match Duration::parse(text) {
            Ok(duration) => Value::Duration(duration),
            Err(DurationText::Malformed) => Value::Undefined,
            Err(DurationText::Overflow) => return Err(Refused::IntegerOverflow),
        },
        Target::Integer | Target::Float | Target::String => Value::Undefined,
    })
}

/// Whether `text` is `-?[0-9]+`, and with `fraction` also
/// `-?[0-9]+\.[0-9]+`.
fn is_decimal(text: &str, fraction: bool) -> bool {
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    let (whole, part) = match unsigned.split_once('.') {
        Some((whole, part)) if fraction => (whole, part),
        Some(_) => return false,
        None => (unsigned, "0"),
    };
    let digits = |run: &str| !run.is_empty() && run.bytes().all(|byte| byte.is_ascii_digit());
    digits(whole) && digits(part)
}

/// A float truncated toward zero, when the integer fits in `i64`.
fn truncate(number: f64) -> Result<i64, Refused> {
    /// 2^63, the first float past `i64`.
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    let whole = number.trunc();
    if (-LIMIT..LIMIT).contains(&whole) {
        // In range and whole, so the conversion is exact.
        Ok(whole as i64)
    } else {
        Err(Refused::IntegerOverflow)
    }
}

/// `date` at midnight UTC, or undefined when its year is outside the 0 to
/// 9999 an instant holds.
fn midnight(date: Date) -> Value {
    if !(0..=9999).contains(&date.year()) {
        return Value::Undefined;
    }
    DateTime::new(date, 0, 0, 0, 0).map_or(Value::Undefined, Value::DateTime)
}

/// An instant's text, when its year is within 0 to 9999.
fn instant_text(instant: DateTime) -> Result<String, Refused> {
    if (0..=9999).contains(&instant.date().year()) {
        Ok(instant.to_string())
    } else {
        Err(Refused::OutOfRange)
    }
}

#[cfg(test)]
mod tests {
    use crate::{Context, Date, DateTime, Duration, Error, Instructions, Options, Value, evaluate};

    /// `x::target`, `x` bound to `value`.
    fn cast(value: Value, target: &str) -> Result<Value, Error> {
        let mut context = Context::new();
        context.insert("x", value);
        evaluate(
            &Instructions::from_json(&format!(r#"[["load","x"],["cast","{target}"]]"#)).unwrap(),
            &context,
            &Options::new(),
        )
    }

    fn answer(value: Value, target: &str) -> Value {
        cast(value, target).unwrap()
    }

    fn reason(value: Value, target: &str) -> String {
        match cast(value, target) {
            Ok(value) => format!("no error: {value:?}"),
            Err(error) => error.reason().to_owned(),
        }
    }

    fn text(value: &str) -> Value {
        Value::from(value)
    }

    // Sabotage: with null cast to a string answering "null", this test
    // fails.
    #[test]
    fn a_value_with_no_conversion_casts_to_undefined() {
        let mut map = crate::Map::new();
        map.insert("a", Value::from(1));
        for target in [
            "integer", "float", "string", "boolean", "date", "datetime", "duration",
        ] {
            for value in [
                Value::Null,
                Value::Undefined,
                Value::from(vec![Value::from(1)]),
                Value::Map(map.clone()),
            ] {
                assert_eq!(answer(value.clone(), target), Value::Undefined, "{value:?}");
            }
        }
        assert_eq!(answer(Value::from(true), "float"), Value::Undefined);
        assert_eq!(answer(Value::from(1.0), "boolean"), Value::Undefined);
        assert_eq!(
            answer(Value::Duration(Duration::default()), "date"),
            Value::Undefined
        );
    }

    // Sabotage: with a float cast to an integer by rounding, this test
    // fails.
    #[test]
    fn numbers_widen_truncate_and_refuse_past_the_integer_range() {
        assert_eq!(
            answer(Value::from(9_007_199_254_740_993), "float"),
            Value::from(9_007_199_254_740_992.0)
        );
        assert_eq!(answer(Value::from(-2.7), "integer"), Value::from(-2));
        assert_eq!(answer(Value::from(2.7), "integer"), Value::from(2));
        assert_eq!(
            answer(Value::from(-9_223_372_036_854_775_808.0), "integer"),
            Value::from(i64::MIN)
        );
        assert_eq!(
            reason(Value::from(9_223_372_036_854_775_808.0), "integer"),
            "integer_overflow"
        );
        assert_eq!(reason(Value::from(-1e19), "integer"), "integer_overflow");
    }

    // Sabotage: with a float written by Rust's own Display, this test fails.
    #[test]
    fn a_float_is_written_as_the_reference_writes_one() {
        assert_eq!(answer(Value::from(1e16), "string"), text("1.0e16"));
        assert_eq!(answer(Value::from(42.0), "string"), text("42.0"));
        assert_eq!(answer(Value::from(0.0001), "string"), text("0.0001"));
        assert_eq!(answer(Value::from(-0.0), "string"), text("-0.0"));
        assert_eq!(answer(Value::from(-7), "string"), text("-7"));
    }

    // Sabotage: with the integer read through str::parse alone (which
    // takes a leading plus), this test fails.
    #[test]
    fn a_string_is_read_whole_as_a_number() {
        assert_eq!(answer(text("+5"), "integer"), Value::Undefined);
        assert_eq!(answer(text("-0"), "integer"), Value::from(0));
        assert_eq!(answer(text("00012"), "integer"), Value::from(12));
        assert_eq!(answer(text("-"), "integer"), Value::Undefined);
        assert_eq!(answer(text("1.5"), "integer"), Value::Undefined);
        assert_eq!(
            answer(text("\u{FF14}\u{FF12}"), "integer"),
            Value::Undefined
        );
        assert_eq!(
            reason(text("9223372036854775808"), "integer"),
            "integer_overflow"
        );
        assert_eq!(
            answer(text("-9223372036854775808"), "integer"),
            Value::from(i64::MIN)
        );
        assert_eq!(answer(text("1."), "float"), Value::Undefined);
        assert_eq!(answer(text(".5"), "float"), Value::Undefined);
        assert_eq!(answer(text("1.2.3"), "float"), Value::Undefined);
        assert_eq!(answer(text("inf"), "float"), Value::Undefined);
        assert_eq!(answer(text("-0"), "float"), Value::from(-0.0));
        let huge = format!("1{}.0", "0".repeat(400));
        assert_eq!(reason(text(&huge), "float"), "non_finite_number");
        let tiny = format!("0.{}1", "0".repeat(400));
        assert_eq!(answer(text(&tiny), "float"), Value::from(0.0));
    }

    // Sabotage: with midnight taking a date of any year, this test fails.
    #[test]
    fn dates_and_instants_bridge_inside_the_years_an_instant_holds() {
        let far = Date::new(10_000, 1, 1).unwrap();
        let early = Date::new(-1, 12, 31).unwrap();
        assert_eq!(answer(Value::Date(far), "datetime"), Value::Undefined);
        assert_eq!(answer(Value::Date(early), "datetime"), Value::Undefined);
        assert_eq!(answer(Value::Date(far), "string"), text("10000-01-01"));
        assert_eq!(answer(Value::Date(early), "string"), text("-0001-12-31"));
        let past = DateTime::new(far, 0, 0, 0, 0).unwrap();
        let before = DateTime::new(early, 23, 59, 59, 999_999).unwrap();
        assert_eq!(reason(Value::DateTime(past), "string"), "date_out_of_range");
        assert_eq!(
            reason(Value::DateTime(before), "string"),
            "date_out_of_range"
        );
        assert_eq!(answer(Value::DateTime(past), "date"), Value::Date(far));
        let last = DateTime::new(Date::new(9999, 12, 31).unwrap(), 23, 59, 59, 999_999).unwrap();
        assert_eq!(
            answer(Value::DateTime(last), "string"),
            text("9999-12-31T23:59:59.999999Z")
        );
    }

    // Sabotage: with the cast reading only an instant written with Z, this
    // test fails.
    #[test]
    fn a_string_is_read_as_a_date_or_an_instant_in_the_reference_forms() {
        let instant = |value: &str| match answer(text(value), "datetime") {
            Value::DateTime(instant) => instant.to_string(),
            other => format!("{other:?}"),
        };
        assert_eq!(instant("2026-08-09T10:00:00+05"), "2026-08-09T05:00:00Z");
        assert_eq!(instant("2026-08-09T10:00:00+0530"), "2026-08-09T04:30:00Z");
        assert_eq!(instant("2026-08-09 10:00:00Z"), "2026-08-09T10:00:00Z");
        assert_eq!(
            instant("2026-08-09T10:00:00,5Z"),
            "2026-08-09T10:00:00.500000Z"
        );
        assert_eq!(
            instant("2026-08-09T10:00:00.1234567Z"),
            "2026-08-09T10:00:00.123456Z"
        );
        for refused in [
            "2026-08-09T10:00:00",
            "2026-08-09T10:00:00-00:00",
            "2026-08-09t10:00:00Z",
            "2026-08-09T10:00Z",
            "2026-08-09T24:00:00Z",
            "20260809T100000Z",
            "-0005-08-09T10:00:00Z",
        ] {
            assert_eq!(instant(refused), "Undefined", "{refused}");
        }
        assert_eq!(
            answer(text("2024-02-29"), "date"),
            Value::Date(Date::new(2024, 2, 29).unwrap())
        );
        for refused in [
            "2026-02-29",
            "-0005-01-01",
            "10000-01-01",
            "20260809",
            "2026-8-9",
        ] {
            assert_eq!(answer(text(refused), "date"), Value::Undefined, "{refused}");
        }
    }

    // Sabotage: with a duration unit past i64 read as undefined, this test
    // fails.
    #[test]
    fn a_string_is_read_as_a_duration_or_refused_past_the_integer_range() {
        assert_eq!(
            answer(text("1d2h"), "duration"),
            Value::Duration(Duration {
                days: 1,
                hours: 2,
                ..Duration::default()
            })
        );
        assert_eq!(
            reason(text("9223372036854775808s"), "duration"),
            "integer_overflow"
        );
        assert_eq!(
            reason(text("9223372036854775807s1s"), "duration"),
            "integer_overflow"
        );
        assert_eq!(answer(text("1d 2h"), "duration"), Value::Undefined);
        assert_eq!(
            answer(
                Value::Duration(Duration {
                    days: 3,
                    ..Duration::default()
                }),
                "string"
            ),
            text("3d")
        );
    }

    // Sabotage: with an unknown type name read as a cast to integer, this
    // test fails.
    #[test]
    fn cast_takes_one_of_the_seven_type_names_and_one_value() {
        let run = |text: &str| {
            evaluate(
                &Instructions::from_json(text).unwrap(),
                &Context::new(),
                &Options::new(),
            )
            .unwrap_err()
            .reason()
            .to_owned()
        };
        for operands in [r#""widget""#, "5", r#""Integer""#, r#""integer","float""#] {
            assert_eq!(
                run(&format!(r#"[["lit",1],["cast",{operands}]]"#)),
                "unknown_instruction",
                "{operands}"
            );
        }
        assert_eq!(run(r#"[["lit",1],["cast"]]"#), "unknown_instruction");
        assert_eq!(run(r#"[["cast","string"]]"#), "insufficient_operands");
    }
}
