//! The five arithmetic instructions, `add`, `subtract`, `multiply`,
//! `divide` and `modulo`, as the reference's instruction-set text states
//! them, with its code answering where the text is silent.
//!
//! Two integers answer an integer and a float on either side answers a
//! float. Every integer step is checked: a result outside `i64` fails the
//! run with the reason `integer_overflow`, never a wrapped number, and a
//! float result that is not finite fails it with the reason
//! `non_finite_number`. `divide` and `modulo` refuse a zero divisor before
//! they look at the operands' types.
//!
//! A date or an instant moves by a duration as the reference's duration
//! code moves it: a week is 7 days, a month 30 and a year 365, with no
//! calendar rule. A date moves by its date units plus the whole days its
//! hours, minutes and seconds add up to (truncated toward zero), and stays
//! a date; an instant moves by every unit to the second, and by its
//! milliseconds too when they are positive. Two dates are apart by a
//! duration in days, and two instants (a date read at midnight UTC) by the
//! difference of their whole seconds, each fraction dropped. A date or an
//! instant moved past the years this crate holds fails the run with the
//! reason `date_out_of_range`.

use super::{Fault, mismatch_pair};
use crate::{Date, DateTime, Duration, EvaluationError, Value};
use core::mem;

/// One of the five arithmetic instructions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Arithmetic {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
}

impl Arithmetic {
    /// The instruction's name, which is also the operation a refusal names.
    pub(super) fn name(self) -> &'static str {
        match self {
            Arithmetic::Add => "add",
            Arithmetic::Subtract => "subtract",
            Arithmetic::Multiply => "multiply",
            Arithmetic::Divide => "divide",
            Arithmetic::Modulo => "modulo",
        }
    }

    /// What a refusal says the instruction wanted, as the reference's text
    /// names it.
    fn wanted(self) -> &'static str {
        match self {
            Arithmetic::Add => "number or a string",
            Arithmetic::Subtract => "number or a date",
            Arithmetic::Multiply | Arithmetic::Divide | Arithmetic::Modulo => "number",
        }
    }
}

/// Microseconds in a second.
const MICROS: i128 = 1_000_000;

/// Seconds in a day.
const DAY_SECONDS: i128 = 86_400;

/// What `operation` answers for `left` and `right` (the right operand was
/// the top of the stack).
pub(super) fn apply(
    operation: Arithmetic,
    mut left: Value,
    mut right: Value,
) -> Result<Value, Fault> {
    if zero_divisor(operation, &right) {
        let reason = match operation {
            Arithmetic::Modulo => "modulo_by_zero",
            _ => "division_by_zero",
        };
        return Err(EvaluationError::new(reason, format!("{} by zero", operation.name())).into());
    }
    let answer = match operation {
        Arithmetic::Add => add(&mut left, &mut right),
        Arithmetic::Subtract => subtract(&left, &right),
        _ => numbers(operation, &left, &right),
    };
    match answer {
        Some(answer) => Ok(answer?),
        None => Err(mismatch_pair(
            operation.name(),
            operation.wanted(),
            &left,
            &right,
        )),
    }
}

/// Whether `right` is a divisor `operation` refuses before any type check:
/// an integer 0 for `divide` and `modulo`, a float zero for `divide` alone.
fn zero_divisor(operation: Arithmetic, right: &Value) -> bool {
    match (operation, right) {
        (Arithmetic::Divide | Arithmetic::Modulo, Value::Integer(0)) => true,
        (Arithmetic::Divide, Value::Float(number)) => *number == 0.0,
        _ => false,
    }
}

/// `None` when no rule takes the pair.
type Answer = Option<Result<Value, EvaluationError>>;

fn add(left: &mut Value, right: &mut Value) -> Answer {
    if let (Value::String(text), Some(number)) = (&mut *left, number_text(right)) {
        let mut text = mem::take(text);
        text.push_str(&number);
        return Some(Ok(Value::String(text)));
    }
    if let (Some(number), Value::String(text)) = (number_text(left), &*right) {
        return Some(Ok(Value::String(number + text)));
    }
    let joined = match (&mut *left, &mut *right) {
        (Value::String(left), Value::String(right)) => {
            let mut text = mem::take(left);
            text.push_str(right);
            Value::String(text)
        }
        (Value::List(left), Value::List(right)) => {
            let mut items = mem::take(left);
            items.append(right);
            Value::List(items)
        }
        (Value::Date(date), Value::Duration(duration))
        | (Value::Duration(duration), Value::Date(date)) => {
            return Some(shift_date(*date, duration, 1));
        }
        (Value::DateTime(instant), Value::Duration(duration))
        | (Value::Duration(duration), Value::DateTime(instant)) => {
            return Some(shift_instant(*instant, duration, 1));
        }
        _ => return numbers(Arithmetic::Add, left, right),
    };
    Some(Ok(joined))
}

fn subtract(left: &Value, right: &Value) -> Answer {
    if let (Value::Date(left), Value::Date(right)) = (left, right) {
        return Some(Ok(Value::Duration(Duration {
            days: left.days() - right.days(),
            ..Duration::default()
        })));
    }
    if let (Some(left), Some(right)) = (whole_seconds(left), whole_seconds(right)) {
        // Two instants this crate holds are far less than 2^63 seconds
        // apart, so the conversion holds.
        let seconds = i64::try_from(left - right).map_err(|_| out_of_range());
        return Some(seconds.map(|seconds| {
            Value::Duration(Duration {
                seconds,
                ..Duration::default()
            })
        }));
    }
    match (left, right) {
        (Value::Date(date), Value::Duration(duration)) => Some(shift_date(*date, duration, -1)),
        (Value::DateTime(instant), Value::Duration(duration)) => {
            Some(shift_instant(*instant, duration, -1))
        }
        _ => numbers(Arithmetic::Subtract, left, right),
    }
}

/// Two numbers under `operation`; `None` when either is not a number, or
/// for `modulo` not an integer.
fn numbers(operation: Arithmetic, left: &Value, right: &Value) -> Answer {
    let (left, right) = match (left, right) {
        (Value::Integer(left), Value::Integer(right)) => {
            return Some(integers(operation, *left, *right));
        }
        _ if operation == Arithmetic::Modulo => return None,
        (Value::Integer(left), Value::Float(right)) => (*left as f64, *right),
        (Value::Float(left), Value::Integer(right)) => (*left, *right as f64),
        (Value::Float(left), Value::Float(right)) => (*left, *right),
        _ => return None,
    };
    Some(floats(operation, left, right))
}

/// Two integers: checked at every step. A quotient truncates toward zero
/// and a remainder takes the dividend's sign. The divisor is never zero
/// here, a zero having been refused before.
fn integers(operation: Arithmetic, left: i64, right: i64) -> Result<Value, EvaluationError> {
    let (answer, symbol) = match operation {
        Arithmetic::Add => (left.checked_add(right), "+"),
        Arithmetic::Subtract => (left.checked_sub(right), "-"),
        Arithmetic::Multiply => (left.checked_mul(right), "*"),
        Arithmetic::Divide => (left.checked_div(right), "/"),
        // Any integer over -1 leaves no remainder; `checked_rem` calls the
        // least i64 over -1 an overflow because its quotient is one.
        Arithmetic::Modulo if right == -1 => (Some(0), "%"),
        Arithmetic::Modulo => (left.checked_rem(right), "%"),
    };
    answer.map(Value::Integer).ok_or_else(|| {
        EvaluationError::new(
            "integer_overflow",
            format!("{left} {symbol} {right} is outside the 64-bit integer range"),
        )
    })
}

/// Two floats, or an integer read as a float beside a float. `modulo`
/// never reaches here.
fn floats(operation: Arithmetic, left: f64, right: f64) -> Result<Value, EvaluationError> {
    let answer = match operation {
        Arithmetic::Add => left + right,
        Arithmetic::Subtract => left - right,
        Arithmetic::Multiply => left * right,
        Arithmetic::Divide | Arithmetic::Modulo => left / right,
    };
    if answer.is_finite() {
        Ok(Value::Float(answer))
    } else {
        Err(EvaluationError::new(
            "non_finite_number",
            format!("{} answered a float that is not finite", operation.name()),
        ))
    }
}

/// A number as `add` writes it beside a string, an integer in decimal and a
/// float as the reference writes one; `None` for any other value.
fn number_text(value: &Value) -> Option<String> {
    match value {
        Value::Integer(number) => Some(number.to_string()),
        Value::Float(number) => Some(crate::json::float_text(*number)),
        _ => None,
    }
}

fn out_of_range() -> EvaluationError {
    EvaluationError::new(
        "date_out_of_range",
        "the date arithmetic answers a date outside the years this implementation holds",
    )
}

/// The days a duration's date units add up to. Every sum and product in
/// this module is an `i128` over `i64` units, far from its bounds.
fn date_days(duration: &Duration) -> i128 {
    i128::from(duration.days)
        + i128::from(duration.weeks) * 7
        + i128::from(duration.months) * 30
        + i128::from(duration.years) * 365
}

/// The seconds a duration's hours, minutes and seconds add up to.
fn time_seconds(duration: &Duration) -> i128 {
    i128::from(duration.seconds)
        + i128::from(duration.minutes) * 60
        + i128::from(duration.hours) * 3600
}

/// A date at midnight UTC, or an instant, as the whole seconds since the
/// epoch, its fraction of a second dropped (the reference takes two
/// instants' whole seconds before it subtracts them); `None` for any other
/// value.
fn whole_seconds(value: &Value) -> Option<i128> {
    match value {
        Value::Date(date) => Some(i128::from(date.days()) * DAY_SECONDS),
        Value::DateTime(instant) => Some(i128::from(instant.unix_seconds())),
        _ => None,
    }
}

/// `date` moved by `duration`, forward when `sign` is 1 and back when it is
/// -1: by its date units and the whole days its time units add up to,
/// truncated toward zero. Milliseconds do not move a date.
fn shift_date(date: Date, duration: &Duration, sign: i128) -> Result<Value, EvaluationError> {
    let days = date_days(duration) + time_seconds(duration) / DAY_SECONDS;
    i64::try_from(i128::from(date.days()) + sign * days)
        .ok()
        .and_then(Date::from_days)
        .map(Value::Date)
        .ok_or_else(out_of_range)
}

/// `instant` moved by `duration`, forward when `sign` is 1 and back when it
/// is -1: by every unit to the second, and by its milliseconds when they
/// are positive, as the reference's duration code splits the two.
pub(super) fn shift_instant(
    instant: DateTime,
    duration: &Duration,
    sign: i128,
) -> Result<Value, EvaluationError> {
    let mut step = (date_days(duration) * DAY_SECONDS + time_seconds(duration)) * MICROS;
    if duration.milliseconds > 0 {
        step += i128::from(duration.milliseconds) * 1000;
    }
    let moved = i128::from(instant.unix_seconds()) * MICROS
        + i128::from(instant.microsecond())
        + sign * step;
    let seconds = i64::try_from(moved.div_euclid(MICROS)).ok();
    let microsecond = u32::try_from(moved.rem_euclid(MICROS)).ok();
    seconds
        .zip(microsecond)
        .and_then(|(seconds, microsecond)| DateTime::from_unix(seconds, microsecond))
        .map(Value::DateTime)
        .ok_or_else(out_of_range)
}

#[cfg(test)]
mod tests {
    use crate::{Context, Error, Instructions, Options, Value, evaluate};

    fn run(text: &str) -> Result<Value, Error> {
        evaluate(
            &Instructions::from_json(text).unwrap(),
            &Context::new(),
            &Options::new(),
        )
    }

    /// `left OPCODE right`, each operand a literal in the tagged encoding.
    fn apply(left: &str, opcode: &str, right: &str) -> Result<Value, Error> {
        run(&format!(r#"[["lit",{left}],["lit",{right}],["{opcode}"]]"#))
    }

    /// The answer, or a string naming the refusal, so that an unexpected
    /// refusal fails the assertion it meets.
    fn answer(left: &str, opcode: &str, right: &str) -> Value {
        apply(left, opcode, right)
            .unwrap_or_else(|error| Value::from(format!("refused: {}", error.reason())))
    }

    /// The type and reason of the error, or the answer in the same shape.
    fn refusal(left: &str, opcode: &str, right: &str) -> (String, String) {
        match apply(left, opcode, right) {
            Ok(value) => ("no error".to_owned(), format!("{value:?}")),
            Err(error) => (error.type_name().to_owned(), error.reason().to_owned()),
        }
    }

    fn evaluation(reason: &str) -> (String, String) {
        ("EvaluationError".to_owned(), reason.to_owned())
    }

    fn mismatched(reason: &str) -> (String, String) {
        ("TypeMismatchError".to_owned(), reason.to_owned())
    }

    const MAX: &str = "9223372036854775807";
    const MIN: &str = "-9223372036854775808";

    // Sabotage: with checked_add replaced by wrapping_add, this test fails.
    #[test]
    fn add_refuses_an_integer_overflow_at_both_ends() {
        assert_eq!(answer(MAX, "add", "0"), Value::from(i64::MAX));
        assert_eq!(answer(MIN, "add", "0"), Value::from(i64::MIN));
        assert_eq!(answer(MAX, "add", MIN), Value::from(-1));
        assert_eq!(refusal(MAX, "add", "1"), evaluation("integer_overflow"));
        assert_eq!(refusal(MIN, "add", "-1"), evaluation("integer_overflow"));
    }

    // Sabotage: with checked_sub replaced by wrapping_sub, this test fails.
    #[test]
    fn subtract_refuses_an_integer_overflow_at_both_ends() {
        assert_eq!(answer("10", "subtract", "3"), Value::from(7));
        assert_eq!(answer(MAX, "subtract", MAX), Value::from(0));
        assert_eq!(answer(MIN, "subtract", MIN), Value::from(0));
        assert_eq!(
            refusal(MIN, "subtract", "1"),
            evaluation("integer_overflow")
        );
        assert_eq!(
            refusal(MAX, "subtract", "-1"),
            evaluation("integer_overflow")
        );
    }

    // Sabotage: with checked_mul replaced by wrapping_mul, this test fails.
    #[test]
    fn multiply_refuses_an_integer_overflow_at_both_ends() {
        assert_eq!(answer(MAX, "multiply", "1"), Value::from(i64::MAX));
        assert_eq!(answer(MIN, "multiply", "1"), Value::from(i64::MIN));
        assert_eq!(
            refusal(MAX, "multiply", "2"),
            evaluation("integer_overflow")
        );
        assert_eq!(
            refusal(MIN, "multiply", "2"),
            evaluation("integer_overflow")
        );
        assert_eq!(
            refusal(MIN, "multiply", "-1"),
            evaluation("integer_overflow")
        );
    }

    // Sabotage: with checked_div replaced by wrapping_div, this test fails.
    #[test]
    fn divide_truncates_and_refuses_an_integer_overflow_at_both_ends() {
        assert_eq!(answer("7", "divide", "2"), Value::from(3));
        assert_eq!(answer("-7", "divide", "2"), Value::from(-3));
        assert_eq!(answer(MAX, "divide", "-1"), Value::from(-i64::MAX));
        assert_eq!(answer(MIN, "divide", "1"), Value::from(i64::MIN));
        assert_eq!(refusal(MIN, "divide", "-1"), evaluation("integer_overflow"));
    }

    // Sabotage: with the -1 arm in integers removed (checked_rem then
    // refusing the least i64 over -1), this test fails.
    #[test]
    fn modulo_takes_the_dividends_sign_and_never_overflows_at_either_end() {
        assert_eq!(answer("7", "modulo", "2"), Value::from(1));
        assert_eq!(answer("-7", "modulo", "2"), Value::from(-1));
        assert_eq!(answer("7", "modulo", "-2"), Value::from(1));
        assert_eq!(answer(MIN, "modulo", "-1"), Value::from(0));
        assert_eq!(answer(MAX, "modulo", "-1"), Value::from(0));
        assert_eq!(answer(MAX, "modulo", MIN), Value::from(i64::MAX));
        assert_eq!(answer(MIN, "modulo", MAX), Value::from(-1));
    }

    // Sabotage: with floats answering a non-finite float as it is, this
    // test fails.
    #[test]
    fn a_float_on_either_side_answers_a_float_and_a_non_finite_one_is_refused() {
        assert_eq!(answer("7.0", "divide", "2"), Value::from(3.5));
        assert_eq!(answer("1", "add", "0.5"), Value::from(1.5));
        assert_eq!(answer("0.5", "subtract", "1"), Value::from(-0.5));
        assert_eq!(answer("2.5", "multiply", "2.0"), Value::from(5.0));
        assert_eq!(
            answer(MAX, "add", "1.0"),
            Value::from(9.223_372_036_854_776e18)
        );
        assert_eq!(
            refusal("1.0e308", "multiply", "10"),
            evaluation("non_finite_number")
        );
        assert_eq!(
            refusal("-1.0e308", "subtract", "1.0e308"),
            evaluation("non_finite_number")
        );
        assert_eq!(
            refusal("1.0e308", "divide", "1.0e-10"),
            evaluation("non_finite_number")
        );
    }

    // Sabotage: with zero_divisor reading only an integer 0, this test
    // fails.
    #[test]
    fn a_zero_divisor_is_refused_before_the_operands_types() {
        assert_eq!(refusal("1", "divide", "0"), evaluation("division_by_zero"));
        assert_eq!(
            refusal("1", "divide", "0.0"),
            evaluation("division_by_zero")
        );
        assert_eq!(
            refusal("1", "divide", "-0.0"),
            evaluation("division_by_zero")
        );
        assert_eq!(
            refusal("true", "divide", "0"),
            evaluation("division_by_zero")
        );
        assert_eq!(
            refusal(r#""x""#, "modulo", "0"),
            evaluation("modulo_by_zero")
        );
        assert_eq!(refusal("5", "modulo", "0.0"), mismatched("modulo"));
        assert_eq!(refusal("5.0", "modulo", "2"), mismatched("modulo"));
        assert_eq!(refusal("5", "modulo", "2.0"), mismatched("modulo"));
    }

    // Sabotage: with number_text writing a float with Rust's own Display,
    // this test fails.
    #[test]
    fn add_joins_strings_and_lists_and_writes_a_number_beside_a_string() {
        assert_eq!(answer(r#""a""#, "add", r#""b""#), Value::from("ab"));
        assert_eq!(answer(r#""n: ""#, "add", "5"), Value::from("n: 5"));
        assert_eq!(
            answer(r#""n: ""#, "add", "1.0e16"),
            Value::from("n: 1.0e16")
        );
        assert_eq!(answer("2.5", "add", r#""x""#), Value::from("2.5x"));
        assert_eq!(answer("-3", "add", r#""x""#), Value::from("-3x"));
        assert_eq!(
            answer("[1,[2]]", "add", r#"[null,"x"]"#),
            Value::from(vec![
                Value::from(1),
                Value::from(vec![Value::from(2)]),
                Value::Null,
                Value::from("x"),
            ])
        );
    }

    // Sabotage: with apply's fallback for add naming the operation
    // "subtract", this test fails.
    #[test]
    fn a_pair_no_rule_takes_is_a_type_mismatch_named_for_its_operation() {
        assert_eq!(refusal("null", "add", "1"), mismatched("add"));
        assert_eq!(refusal("[1]", "add", r#""x""#), mismatched("add"));
        assert_eq!(refusal("true", "add", "true"), mismatched("add"));
        assert_eq!(
            refusal(r#""ab""#, "subtract", r#""a""#),
            mismatched("subtract")
        );
        assert_eq!(refusal("[1]", "subtract", "[1]"), mismatched("subtract"));
        assert_eq!(refusal("5", "multiply", "true"), mismatched("multiply"));
        assert_eq!(refusal(r#""x""#, "divide", "2"), mismatched("divide"));
        let error = run(r#"[["lit",{"$type":"undefined"}],["lit",1],["add"]]"#).unwrap_err();
        assert_eq!(
            (error.type_name(), error.reason()),
            ("TypeMismatchError", "add")
        );
        for opcode in ["add", "subtract", "multiply", "divide", "modulo"] {
            let error = run(&format!(r#"[["lit",1],["{opcode}"]]"#)).unwrap_err();
            assert_eq!(error.reason(), "insufficient_operands", "{opcode}");
        }
    }

    // Sabotage: with mismatch_pair reporting only a left undefined operand
    // as RejectedUndefined, this test fails.
    #[test]
    fn an_undefined_operand_from_an_unbound_load_is_reported_as_unbound() {
        for text in [
            r#"[["load","missing"],["lit",1],["add"]]"#,
            r#"[["lit",1],["load","missing"],["multiply"]]"#,
        ] {
            let error = run(text).unwrap_err();
            assert_eq!(
                (error.type_name(), error.reason()),
                ("UndefinedVariableError", "unbound_variable"),
                "{text}"
            );
        }
    }

    fn date(text: &str) -> String {
        format!(r#"{{"$type":"date","value":"{text}"}}"#)
    }

    fn instant(text: &str) -> String {
        format!(r#"{{"$type":"datetime","value":"{text}"}}"#)
    }

    /// A duration literal carrying `units` (`"days":3,...`), every other
    /// unit the encoding requires written as zero.
    fn duration(units: &str) -> String {
        let mut all: Vec<(String, String)> = [
            "years", "months", "weeks", "days", "hours", "minutes", "seconds",
        ]
        .iter()
        .map(|unit| ((*unit).to_owned(), "0".to_owned()))
        .collect();
        for pair in units.split(',') {
            let (name, amount) = pair.split_once(':').unwrap();
            let name = name.trim_matches('"').to_owned();
            match all.iter_mut().find(|(unit, _)| *unit == name) {
                Some(slot) => slot.1 = amount.to_owned(),
                None => all.push((name, amount.to_owned())),
            }
        }
        let units: Vec<String> = all
            .iter()
            .map(|(name, amount)| format!(r#""{name}":{amount}"#))
            .collect();
        format!(r#"{{"$type":"duration","value":{{{}}}}}"#, units.join(","))
    }

    fn tagged(text: &str) -> Value {
        crate::tagged::decode(text).unwrap()
    }

    // Sabotage: with date_days counting a month as 31 days, this test fails.
    #[test]
    fn a_date_moves_by_whole_days_with_months_of_30_and_years_of_365() {
        let span =
            duration(r#""years":1,"months":1,"weeks":1,"days":3,"hours":25,"milliseconds":900"#);
        // 365 + 30 + 7 + 3 days, and the 25 hours make one whole day more.
        assert_eq!(
            answer(&date("2024-01-01"), "add", &span),
            tagged(&date("2025-02-10"))
        );
        assert_eq!(
            answer(&span, "add", &date("2024-01-01")),
            tagged(&date("2025-02-10"))
        );
        assert_eq!(
            answer(&date("2025-02-10"), "subtract", &span),
            tagged(&date("2024-01-01"))
        );
        // Time units short of a day leave a date where it is, either way.
        let hours = duration(r#""hours":-23"#);
        assert_eq!(
            answer(&date("2024-01-01"), "add", &hours),
            tagged(&date("2024-01-01"))
        );
        assert_eq!(
            refusal(&duration(r#""days":1"#), "subtract", &date("2024-01-01")),
            mismatched("subtract")
        );
        assert_eq!(
            refusal(&duration(r#""days":1"#), "add", &duration(r#""days":1"#)),
            mismatched("add")
        );
    }

    // Sabotage: with shift_instant counting milliseconds of either sign,
    // this test fails.
    #[test]
    fn an_instant_moves_to_the_second_and_by_positive_milliseconds_alone() {
        let start = instant("2024-01-15T10:00:00Z");
        assert_eq!(
            answer(
                &start,
                "add",
                &duration(r#""days":1,"hours":2,"seconds":-1"#)
            ),
            tagged(&instant("2024-01-16T11:59:59Z"))
        );
        assert_eq!(
            answer(&duration(r#""milliseconds":1500"#), "add", &start),
            tagged(&instant("2024-01-15T10:00:01.500000Z"))
        );
        assert_eq!(
            answer(&start, "subtract", &duration(r#""milliseconds":250"#)),
            tagged(&instant("2024-01-15T09:59:59.750000Z"))
        );
        assert_eq!(
            answer(
                &start,
                "add",
                &duration(r#""seconds":1,"milliseconds":-500"#)
            ),
            tagged(&instant("2024-01-15T10:00:01Z"))
        );
    }

    // Sabotage: with whole_seconds rounding an instant to its nearest
    // second, this test fails.
    #[test]
    fn two_dates_are_apart_in_days_and_two_instants_in_whole_seconds() {
        assert_eq!(
            answer(&date("2024-01-10"), "subtract", &date("2024-01-15")),
            Value::Duration(crate::Duration {
                days: -5,
                ..crate::Duration::default()
            })
        );
        let seconds = |seconds| {
            Value::Duration(crate::Duration {
                seconds,
                ..crate::Duration::default()
            })
        };
        assert_eq!(
            answer(
                &instant("2024-01-15T00:00:00.500000Z"),
                "subtract",
                &instant("2024-01-15T00:00:02Z")
            ),
            seconds(-2)
        );
        assert_eq!(
            answer(
                &instant("2024-01-15T00:00:02Z"),
                "subtract",
                &instant("2024-01-15T00:00:00.500000Z")
            ),
            seconds(2)
        );
        assert_eq!(
            answer(
                &instant("2024-01-15T01:00:00Z"),
                "subtract",
                &date("2024-01-15")
            ),
            seconds(3600)
        );
        assert_eq!(
            answer(
                &date("2024-01-15"),
                "subtract",
                &instant("2024-01-14T23:00:00Z")
            ),
            seconds(3600)
        );
    }

    // Sabotage: with shift_date answering the epoch for a day count it
    // cannot hold, this test fails.
    #[test]
    fn a_date_moved_past_the_years_this_crate_holds_is_refused() {
        let far = duration(r#""years":9223372036854775807"#);
        assert_eq!(
            refusal(&date("2024-01-01"), "add", &far),
            evaluation("date_out_of_range")
        );
        assert_eq!(
            refusal(&instant("2024-01-01T00:00:00Z"), "subtract", &far),
            evaluation("date_out_of_range")
        );
        let days = duration(r#""days":1000000000000"#);
        assert_eq!(
            refusal(&date("2024-01-01"), "add", &days),
            evaluation("date_out_of_range")
        );
        assert_eq!(
            refusal(&instant("2024-01-01T00:00:00Z"), "add", &days),
            evaluation("date_out_of_range")
        );
    }
}
