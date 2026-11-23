//! The rich-type instructions, `object_new`, `object_set`, `duration` and
//! `relative_date`, as the reference's instruction-set text states them.
//!
//! `object_set` writes into a map and nothing else: a date, an instant and
//! a duration are not maps to it. `duration` builds the one duration kind
//! the crate has, the same kind a context or a date subtraction gives, and
//! `relative_date` moves the current time by any duration exactly as `add`
//! and `subtract` move an instant (`arithmetic`), so a month is 30 days and
//! a year 365 wherever a duration is applied.

use super::Fault;
use super::arithmetic::shift_instant;
use crate::{DateTime, Duration, EvaluationError, Key, Value};

/// `object_set`: `target` with `key` set to `value`. A target that is not a
/// map fails with the reason `invalid_stack_value`.
pub(super) fn object_set(mut target: Value, key: &str, value: Value) -> Result<Value, Fault> {
    if let Value::Map(map) = &mut target {
        map.insert(Key::from(key), value);
        return Ok(target);
    }
    Err(invalid_stack_value(&format!(
        "object_set takes a map beneath the value, not a {}",
        super::type_name(&target)
    )))
}

fn invalid_stack_value(message: &str) -> Fault {
    EvaluationError::new("invalid_stack_value", message).into()
}

/// The unit a `duration` unit string names, by its name in the duration's
/// map; `None` for a string that names none.
fn unit_name(unit: &str) -> Option<&'static str> {
    Some(match unit {
        "y" | "year" | "years" => "years",
        "mo" | "month" | "months" => "months",
        "w" | "week" | "weeks" => "weeks",
        "d" | "day" | "days" => "days",
        "h" | "hour" | "hours" => "hours",
        "m" | "min" | "minute" | "minutes" => "minutes",
        "s" | "sec" | "second" | "seconds" => "seconds",
        "ms" | "millisecond" | "milliseconds" => "milliseconds",
        _ => return None,
    })
}

/// `duration`: the duration `pairs` builds, each `[amount, unit]` setting
/// its unit, a later pair overwriting an earlier one naming the same unit.
/// The first pair that is not an integer and a string fails with the reason
/// `invalid_duration_format`, and the first unit string that names no unit
/// with `invalid_duration_unit`, whichever comes first.
pub(super) fn duration(pairs: &[Value]) -> Result<Duration, Fault> {
    let mut duration = Duration::default();
    for pair in pairs {
        let (amount, unit) = match pair {
            Value::List(parts) => match parts.as_slice() {
                [Value::Integer(amount), Value::String(unit)] => (*amount, unit),
                _ => return Err(invalid_format()),
            },
            _ => return Err(invalid_format()),
        };
        let slot = unit_name(unit).and_then(|name| duration.unit_mut(name));
        match slot {
            Some(slot) => *slot = amount,
            None => {
                return Err(EvaluationError::new(
                    "invalid_duration_unit",
                    format!("duration has no unit {unit:?}"),
                )
                .into());
            }
        }
    }
    Ok(duration)
}

fn invalid_format() -> Fault {
    EvaluationError::new(
        "invalid_duration_format",
        "a duration unit pair is an integer and a unit string",
    )
    .into()
}

/// `relative_date`'s operands checked before the clock is read: the
/// duration on top of the stack and the sign `direction` gives it, `-1` for
/// `ago` and `last` and `1` for `future` and `next`. A top that is not a
/// duration fails with the reason `invalid_stack_value`, and any other
/// direction with `invalid_direction`.
pub(super) fn relative(top: &Value, direction: &str) -> Result<(Duration, i128), Fault> {
    let Value::Duration(duration) = top else {
        return Err(invalid_stack_value(&format!(
            "relative_date takes a duration, not a {}",
            super::type_name(top)
        )));
    };
    let sign = match direction {
        "ago" | "last" => -1,
        "future" | "next" => 1,
        _ => {
            return Err(EvaluationError::new(
                "invalid_direction",
                format!("relative_date has no direction {direction:?}"),
            )
            .into());
        }
    };
    Ok((*duration, sign))
}

/// `now` moved by `duration`, forward when `sign` is 1 and back when it is
/// -1, as `add` and `subtract` move an instant.
pub(super) fn shift(now: DateTime, duration: &Duration, sign: i128) -> Result<Value, Fault> {
    Ok(shift_instant(now, duration, sign)?)
}

#[cfg(test)]
mod tests {
    use crate::{Context, Date, DateTime, Duration, Error, Instructions, Options, Value, evaluate};
    use std::cell::Cell;

    fn run_with(text: &str, options: &Options) -> Result<Value, Error> {
        evaluate(
            &Instructions::from_json(text).unwrap(),
            &Context::new(),
            options,
        )
    }

    fn run(text: &str) -> Result<Value, Error> {
        run_with(text, &Options::new())
    }

    fn reason(text: &str) -> String {
        match run(text) {
            Ok(value) => format!("answered {value:?}"),
            Err(error) => format!("{} {}", error.type_name(), error.reason()),
        }
    }

    fn noon() -> DateTime {
        DateTime::new(Date::new(2030, 1, 15).unwrap(), 12, 0, 0, 0).unwrap()
    }

    // Sabotage: with object_set inserting under the key "key" whatever its
    // operand, this test fails.
    #[test]
    fn object_new_and_object_set_build_a_map() {
        let built = run(
            r#"[["object_new"],["lit",1],["object_set","a"],["object_new"],["lit",2],["object_set","b"],["object_set","c"]]"#,
        )
        .unwrap();
        let inner: crate::Map = [("b", Value::from(2))].into_iter().collect();
        let outer: crate::Map = [("a", Value::from(1)), ("c", Value::Map(inner))]
            .into_iter()
            .collect();
        assert_eq!(built, Value::Map(outer));
        assert_eq!(
            run(r#"[["object_new"]]"#).unwrap(),
            Value::Map(crate::Map::new())
        );
        let overwritten =
            run(r#"[["object_new"],["lit",1],["object_set","a"],["lit",2],["object_set","a"]]"#)
                .unwrap();
        assert_eq!(
            overwritten,
            Value::Map([("a", Value::from(2))].into_iter().collect())
        );
    }

    // Sabotage: with object_set answering a duration target unchanged, this
    // test fails.
    #[test]
    fn object_set_refuses_a_target_that_is_not_a_map() {
        for target in [
            "5",
            "null",
            "[1]",
            r#"{"$type":"undefined"}"#,
            r#"{"$type":"date","value":"2030-01-15"}"#,
            r#"{"$type":"datetime","value":"2030-01-15T12:00:00Z"}"#,
        ] {
            let text = format!(r#"[["lit",{target}],["lit",1],["object_set","a"]]"#);
            assert_eq!(
                reason(&text),
                "EvaluationError invalid_stack_value",
                "{text}"
            );
        }
        assert_eq!(
            reason(r#"[["duration",[[3,"d"]]],["lit",1],["object_set","days"]]"#),
            "EvaluationError invalid_stack_value"
        );
        assert_eq!(
            reason(r#"[["lit",1],["object_set","a"]]"#),
            "EvaluationError insufficient_operands"
        );
        for text in [
            r#"[["object_new",1]]"#,
            r#"[["object_new"],["lit",1],["object_set"]]"#,
            r#"[["object_new"],["lit",1],["object_set",1]]"#,
            r#"[["object_new"],["lit",1],["object_set","a","b"]]"#,
        ] {
            assert_eq!(
                reason(text),
                "EvaluationError unknown_instruction",
                "{text}"
            );
        }
    }

    // Sabotage: with "min" left out of unit_name's minutes arm, this test
    // fails.
    #[test]
    fn duration_reads_every_unit_spelling_and_lets_a_later_pair_win() {
        let spellings = [
            ("years", ["y", "year", "years"].as_slice()),
            ("months", &["mo", "month", "months"]),
            ("weeks", &["w", "week", "weeks"]),
            ("days", &["d", "day", "days"]),
            ("hours", &["h", "hour", "hours"]),
            ("minutes", &["m", "min", "minute", "minutes"]),
            ("seconds", &["s", "sec", "second", "seconds"]),
            ("milliseconds", &["ms", "millisecond", "milliseconds"]),
        ];
        for (name, units) in spellings {
            for unit in units {
                let text = format!(r#"[["duration",[[-4,"{unit}"]]],["access","{name}"]]"#);
                assert_eq!(run(&text).unwrap(), Value::from(-4), "{unit}");
            }
        }
        assert_eq!(
            run(r#"[["duration",[[1,"h"],[2,"hours"],[5,"d"]]]]"#).unwrap(),
            Value::Duration(Duration {
                hours: 2,
                days: 5,
                ..Duration::default()
            })
        );
        assert_eq!(
            run(r#"[["duration",[]]]"#).unwrap(),
            Value::Duration(Duration::default())
        );
    }

    // Sabotage: with duration skipping a pair that is not a list instead of
    // refusing it, this test fails.
    #[test]
    fn duration_refuses_a_malformed_pair_or_an_unknown_unit_in_order() {
        for pairs in [
            r#"[["bad"]]"#,
            r#"[[1]]"#,
            r#"[[1,"d",2]]"#,
            r#"[[1.5,"d"]]"#,
            r#"[["d",1]]"#,
            r#"[5]"#,
            r#"[[1,"d"],"x",[1,"xyz"]]"#,
        ] {
            let text = format!(r#"[["duration",{pairs}]]"#);
            assert_eq!(
                reason(&text),
                "EvaluationError invalid_duration_format",
                "{text}"
            );
        }
        for pairs in [r#"[[3,"xyz"]]"#, r#"[[3,"D"]]"#, r#"[[1,"d"],[3,""],"x"]"#] {
            let text = format!(r#"[["duration",{pairs}]]"#);
            assert_eq!(
                reason(&text),
                "EvaluationError invalid_duration_unit",
                "{text}"
            );
        }
        for text in [
            r#"[["duration"]]"#,
            r#"[["duration",5]]"#,
            r#"[["duration",{}]]"#,
        ] {
            assert_eq!(
                reason(text),
                "EvaluationError unknown_instruction",
                "{text}"
            );
        }
    }

    // Sabotage: with relative_date's sign for "last" made 1, this test
    // fails.
    #[test]
    fn relative_date_moves_the_clock_time_by_any_duration() {
        let options = Options::new().clock(|| Some(noon()));
        let at = |text: &str| run_with(text, &options).unwrap();
        let instant = |text: &str| Value::DateTime(DateTime::parse(text).unwrap());
        assert_eq!(
            at(r#"[["duration",[[3,"d"]]],["relative_date","ago"]]"#),
            instant("2030-01-12T12:00:00Z")
        );
        assert_eq!(
            at(r#"[["duration",[[1,"mo"]]],["relative_date","last"]]"#),
            instant("2029-12-16T12:00:00Z")
        );
        assert_eq!(
            at(r#"[["duration",[[2,"h"],[500,"ms"]]],["relative_date","future"]]"#),
            instant("2030-01-15T14:00:00.500000Z")
        );
        assert_eq!(
            at(r#"[["duration",[[1,"y"]]],["relative_date","next"]]"#),
            instant("2031-01-15T12:00:00Z")
        );
        // A duration from the context, or from subtracting two dates, is
        // the same kind as one the instruction builds.
        let mut context = Context::new();
        context.insert(
            "span",
            Value::Duration(Duration {
                weeks: 1,
                ..Duration::default()
            }),
        );
        let program = Instructions::from_json(
            r#"[["load","span"],["relative_date","ago"],["lit",{"$type":"date","value":"2030-01-02"}],["lit",{"$type":"date","value":"2030-01-01"}],["subtract"],["relative_date","next"],["compare","LT"]]"#,
        )
        .unwrap();
        assert_eq!(
            evaluate(&program, &context, &options).unwrap(),
            Value::from(true)
        );
    }

    // Sabotage: with relative_date reading the clock before the duration
    // check, this test fails.
    #[test]
    fn relative_date_refuses_before_it_reads_the_clock() {
        let reads = Cell::new(0);
        let options = Options::new().clock(|| {
            reads.set(reads.get() + 1);
            Some(noon())
        });
        let refused = |text: &str| {
            let error = run_with(text, &options).unwrap_err();
            format!("{} {}", error.type_name(), error.reason())
        };
        assert_eq!(
            refused(r#"[["relative_date","ago"]]"#),
            "EvaluationError insufficient_operands"
        );
        assert_eq!(
            refused(r#"[["object_new"],["relative_date","ago"]]"#),
            "EvaluationError invalid_stack_value"
        );
        assert_eq!(
            refused(r#"[["lit",3],["relative_date","ago"]]"#),
            "EvaluationError invalid_stack_value"
        );
        assert_eq!(
            refused(r#"[["duration",[[1,"d"]]],["relative_date","sideways"]]"#),
            "EvaluationError invalid_direction"
        );
        for text in [
            r#"[["duration",[[1,"d"]]],["relative_date"]]"#,
            r#"[["duration",[[1,"d"]]],["relative_date",1]]"#,
        ] {
            assert_eq!(
                refused(text),
                "EvaluationError unknown_instruction",
                "{text}"
            );
        }
        assert_eq!(reads.get(), 0);
    }

    // Sabotage: with Machine::now reading the clock on every call (the
    // cache never filled), this test fails.
    #[test]
    fn a_run_reads_the_clock_at_most_once_and_only_when_it_reads_the_time() {
        let reads = Cell::new(0);
        let ticking = Options::new().clock(|| {
            reads.set(reads.get() + 1);
            DateTime::from_unix(i64::from(reads.get()) * 3600, 0)
        });
        // Two reads of the time in one run see the same instant.
        assert_eq!(
            run_with(
                r#"[["duration",[]],["relative_date","ago"],["duration",[]],["relative_date","next"],["compare","STRICT_EQ"]]"#,
                &ticking
            )
            .unwrap(),
            Value::from(true)
        );
        assert_eq!(reads.get(), 1);
        // A run that never reads the time never reads the clock.
        run_with(
            r#"[["duration",[[1,"d"]]],["lit",1],["compare","EQ"]]"#,
            &ticking,
        )
        .unwrap();
        assert_eq!(reads.get(), 1);
        // Each run reads it afresh.
        run_with(r#"[["duration",[]],["relative_date","ago"]]"#, &ticking).unwrap();
        assert_eq!(reads.get(), 2);
    }

    // Sabotage: with Machine::now answering the epoch when the clock gives
    // none, this test fails.
    #[test]
    fn a_clock_with_no_time_fails_the_run_with_one_refusal() {
        let options = Options::new().clock(|| None);
        for direction in ["ago", "last", "future", "next"] {
            let text = format!(r#"[["duration",[[1,"d"]]],["relative_date","{direction}"]]"#);
            let error = run_with(&text, &options).unwrap_err();
            assert_eq!(
                (error.type_name(), error.reason()),
                ("EvaluationError", "clock_unavailable")
            );
        }
        // Without a clock the run reads the system clock.
        let before = Options::new().now().unwrap();
        let read = run(r#"[["duration",[]],["relative_date","next"]]"#).unwrap();
        let after = Options::new().now().unwrap();
        let Value::DateTime(read) = read else {
            panic!("relative_date answered {read:?}");
        };
        assert!(before <= read && read <= after);
    }

    // Sabotage: with shift answering now unmoved, this test fails.
    #[test]
    fn relative_date_past_the_years_held_is_date_out_of_range() {
        let options = Options::new().clock(|| Some(noon()));
        let error = run_with(
            r#"[["duration",[[9223372036854775807,"y"]]],["relative_date","next"]]"#,
            &options,
        )
        .unwrap_err();
        assert_eq!(error.reason(), "date_out_of_range");
    }
}
