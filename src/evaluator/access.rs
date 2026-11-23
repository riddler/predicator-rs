//! Reading into a value: `access` and `bracket_access`, and the
//! containment test `in` and `contains` share, as the reference's
//! instruction-set text states them.
//!
//! A duration reads as the map of its eight units (`years` through
//! `milliseconds`), as that text makes it. A date and an instant are
//! neither a map nor a list, so reading into one answers undefined, whatever
//! the key.

use super::{Fault, mismatch};
use crate::compare::member_equal;
use crate::{Key, Value};
use core::mem;

/// The unit of `duration` named `name`, as an integer; undefined for any
/// other name.
fn unit(duration: &crate::Duration, name: &str) -> Value {
    duration
        .units()
        .into_iter()
        .find(|(unit, _, _)| *unit == name)
        .map_or(Value::Undefined, |(_, _, amount)| Value::Integer(amount))
}

/// `access`: `target`'s member named `property`. A missing key, a list, and
/// a target that is neither a map nor a duration all answer undefined; it
/// never fails.
pub(super) fn access(mut target: Value, property: &str) -> Value {
    match &mut target {
        Value::Map(map) => map.remove(&Key::from(property)).unwrap_or(Value::Undefined),
        Value::Duration(duration) => unit(duration, property),
        _ => Value::Undefined,
    }
}

/// How a value indexes a map: `Ok(Some(key))` for the four key kinds,
/// `Ok(None)` for null (a miss), and `Err` for any other value.
fn map_key(key: &Value) -> Result<Option<Key>, ()> {
    match key {
        Value::String(text) => Ok(Some(Key::String(text.clone()))),
        Value::Integer(number) => Ok(Some(Key::Integer(*number))),
        Value::Bool(flag) => Ok(Some(Key::Bool(*flag))),
        Value::Undefined => Ok(Some(Key::Undefined)),
        Value::Null => Ok(None),
        _ => Err(()),
    }
}

/// `bracket_access`: `target[key]`.
///
/// - A map takes a string, an integer, a boolean or undefined as its key;
///   a missing key, and a `null` key, answer undefined. Any other key
///   (a float, a list, a map, a date, an instant, a duration) is a
///   `TypeMismatchError`, operation `bracket_access`. A duration takes keys
///   as a map does, its unit names the keys it holds.
/// - A list takes an integer: a negative or out-of-range index answers
///   undefined, and any other key is a `TypeMismatchError`.
/// - Any other target answers undefined, whatever the key.
pub(super) fn bracket(mut target: Value, key: &Value) -> Result<Value, Fault> {
    let refused = |expected| mismatch("bracket_access", expected, key);
    Ok(match &mut target {
        Value::Map(map) => match map_key(key) {
            Ok(Some(key)) => map.remove(&key).unwrap_or(Value::Undefined),
            Ok(None) => Value::Undefined,
            Err(()) => return Err(refused("string")),
        },
        Value::Duration(duration) => match map_key(key) {
            Ok(Some(Key::String(name))) => unit(duration, &name),
            Ok(_) => Value::Undefined,
            Err(()) => return Err(refused("string")),
        },
        Value::List(items) => match key {
            Value::Integer(index) => usize::try_from(*index)
                .ok()
                .and_then(|index| items.get_mut(index))
                .map_or(Value::Undefined, |item| {
                    mem::replace(item, Value::Undefined)
                }),
            _ => return Err(refused("integer")),
        },
        _ => Value::Undefined,
    })
}

/// Whether `container` contains `needle`, for `in` (`needle in container`)
/// and `contains` (`container contains needle`) alike. It never fails: every
/// pairing answers a value, as instruction-set version 7 makes it.
///
/// | container | needle | answer |
/// |---|---|---|
/// | any | undefined | undefined |
/// | undefined | any | undefined |
/// | a list | any other | `true` when a member equals the needle, else `false` |
/// | a string | a string | `true` when the needle occurs in it, else `false` |
/// | a string | any other | undefined |
/// | null, a boolean, a number, a date, an instant, a duration or a map | any | undefined |
///
/// A list's member equals the needle under membership equality
/// ([`member_equal`]): null equals null, numbers match across integer and
/// float, and a string member is matched whole. The string test is by code
/// point and case-sensitive, with no normalization, and the empty string
/// occurs in every string.
pub(super) fn containment(container: &Value, needle: &Value) -> Value {
    match (container, needle) {
        (Value::Undefined, _) | (_, Value::Undefined) => Value::Undefined,
        (Value::List(members), needle) => {
            Value::Bool(members.iter().any(|member| member_equal(member, needle)))
        }
        (Value::String(text), Value::String(needle)) => Value::Bool(text.contains(needle.as_str())),
        _ => Value::Undefined,
    }
}

#[cfg(test)]
mod tests {
    use crate::{Context, Duration, Error, Instructions, Key, Map, Options, Value, evaluate};

    /// A context binding `x` to a map with one key of each kind, `items`
    /// to a list, `span` to a duration and `when` to a date.
    fn context() -> Context {
        let mut map = Map::new();
        map.insert("a", Value::from(1));
        map.insert(2, Value::from("two"));
        map.insert(true, Value::from("yes"));
        map.insert(Key::Undefined, Value::from("absent"));
        let mut context = Context::new();
        context.insert("x", Value::Map(map));
        context.insert(
            "items",
            Value::from(vec![Value::from(10), Value::from(20), Value::from(30)]),
        );
        context.insert(
            "span",
            Value::Duration(Duration {
                days: 21,
                milliseconds: 5,
                ..Duration::default()
            }),
        );
        context.insert("when", Value::Date(crate::Date::new(2026, 3, 1).unwrap()));
        context.insert("flag", Value::from(true));
        context
    }

    fn run(text: &str) -> Result<Value, Error> {
        evaluate(
            &Instructions::from_json(text).unwrap(),
            &context(),
            &Options::new(),
        )
    }

    /// `target[key]`, the target a root of the context and the key a
    /// literal in the tagged encoding.
    fn bracket(target: &str, key: &str) -> Result<Value, Error> {
        run(&format!(
            r#"[["load","{target}"],["lit",{key}],["bracket_access"]]"#
        ))
    }

    /// A refusal as a string naming it, so that an unexpected refusal
    /// fails the assertion it meets.
    fn refused(error: Error) -> Value {
        Value::from(format!("refused: {}", error.reason()))
    }

    fn reason(answer: Result<Value, Error>) -> (String, String) {
        match answer {
            Ok(value) => ("no error".to_owned(), format!("{value:?}")),
            Err(error) => (error.type_name().to_owned(), error.reason().to_owned()),
        }
    }

    fn mismatched() -> (String, String) {
        ("TypeMismatchError".to_owned(), "bracket_access".to_owned())
    }

    // Sabotage: with access answering undefined for a duration, this test
    // fails.
    #[test]
    fn access_reads_a_map_key_or_a_duration_unit_and_never_fails() {
        let access = |target: &str, property: &str| {
            run(&format!(r#"[["load","{target}"],["access","{property}"]]"#))
                .unwrap_or_else(refused)
        };
        assert_eq!(access("x", "a"), Value::from(1));
        assert_eq!(access("x", "missing"), Value::Undefined);
        assert_eq!(access("span", "days"), Value::from(21));
        assert_eq!(access("span", "milliseconds"), Value::from(5));
        assert_eq!(access("span", "years"), Value::from(0));
        assert_eq!(access("span", "fortnights"), Value::Undefined);
        assert_eq!(access("items", "0"), Value::Undefined);
        assert_eq!(access("when", "year"), Value::Undefined);
        assert_eq!(access("flag", "a"), Value::Undefined);
        assert_eq!(
            run(r#"[["lit",null],["access","a"]]"#).unwrap_or_else(refused),
            Value::Undefined
        );
        assert_eq!(
            reason(run(r#"[["access","a"]]"#)).1,
            "insufficient_operands"
        );
    }

    // Sabotage: with map_key reading an integer key as its text, this test
    // fails.
    #[test]
    fn a_map_takes_the_four_key_kinds_misses_on_null_and_refuses_any_other() {
        assert_eq!(
            bracket("x", r#""a""#).unwrap_or_else(refused),
            Value::from(1)
        );
        assert_eq!(
            bracket("x", "2").unwrap_or_else(refused),
            Value::from("two")
        );
        assert_eq!(
            bracket("x", "true").unwrap_or_else(refused),
            Value::from("yes")
        );
        assert_eq!(
            bracket("x", "false").unwrap_or_else(refused),
            Value::Undefined
        );
        assert_eq!(
            bracket("x", r#"{"$type":"undefined"}"#).unwrap_or_else(refused),
            Value::from("absent")
        );
        assert_eq!(
            bracket("x", r#""2""#).unwrap_or_else(refused),
            Value::Undefined
        );
        assert_eq!(
            bracket("x", "null").unwrap_or_else(refused),
            Value::Undefined
        );
        for key in [
            "1.5",
            "[]",
            r#"{"a":1}"#,
            r#"{"$type":"date","value":"2026-03-01"}"#,
            r#"{"$type":"datetime","value":"2026-03-01T00:00:00Z"}"#,
        ] {
            assert_eq!(reason(bracket("x", key)), mismatched(), "{key}");
        }
    }

    // Sabotage: with the duration arm reading every key as a miss, this
    // test fails.
    #[test]
    fn a_duration_takes_keys_as_a_map_of_its_units() {
        assert_eq!(
            bracket("span", r#""days""#).unwrap_or_else(refused),
            Value::from(21)
        );
        assert_eq!(
            bracket("span", r#""weeks""#).unwrap_or_else(refused),
            Value::from(0)
        );
        assert_eq!(
            bracket("span", r#""day""#).unwrap_or_else(refused),
            Value::Undefined
        );
        assert_eq!(
            bracket("span", "1").unwrap_or_else(refused),
            Value::Undefined
        );
        assert_eq!(
            bracket("span", "null").unwrap_or_else(refused),
            Value::Undefined
        );
        assert_eq!(reason(bracket("span", "1.5")), mismatched());
    }

    // Sabotage: with a negative index read as counting from the end, this
    // test fails.
    #[test]
    fn a_list_takes_a_non_negative_integer_and_refuses_any_other_key() {
        assert_eq!(
            bracket("items", "0").unwrap_or_else(refused),
            Value::from(10)
        );
        assert_eq!(
            bracket("items", "2").unwrap_or_else(refused),
            Value::from(30)
        );
        assert_eq!(
            bracket("items", "3").unwrap_or_else(refused),
            Value::Undefined
        );
        assert_eq!(
            bracket("items", "-1").unwrap_or_else(refused),
            Value::Undefined
        );
        assert_eq!(
            bracket("items", "9223372036854775807").unwrap_or_else(refused),
            Value::Undefined
        );
        for key in ["1.0", r#""0""#, "true", "null", r#"{"$type":"undefined"}"#] {
            assert_eq!(reason(bracket("items", key)), mismatched(), "{key}");
        }
        let unbound = run(r#"[["load","items"],["load","missing"],["bracket_access"]]"#);
        assert_eq!(
            reason(unbound),
            (
                "UndefinedVariableError".to_owned(),
                "unbound_variable".to_owned()
            )
        );
    }

    // Sabotage: with the fallback arm of bracket refusing a float key, this
    // test fails.
    #[test]
    fn any_other_target_answers_undefined_whatever_the_key() {
        for key in ["1.5", r#""year""#, "0", "[]", "null"] {
            assert_eq!(
                bracket("when", key).unwrap_or_else(refused),
                Value::Undefined,
                "{key}"
            );
            assert_eq!(
                bracket("flag", key).unwrap_or_else(refused),
                Value::Undefined,
                "{key}"
            );
        }
        assert_eq!(
            run(r#"[["lit","abc"],["lit",0],["bracket_access"]]"#).unwrap_or_else(refused),
            Value::Undefined
        );
        assert_eq!(
            reason(run(r#"[["lit",1],["bracket_access"]]"#)).1,
            "insufficient_operands"
        );
    }

    /// `needle in container` and `container contains needle`, which must
    /// agree; the answer.
    fn contained(container: &str, needle: &str) -> Value {
        let inside = run(&format!(r#"[["lit",{needle}],["lit",{container}],["in"]]"#))
            .unwrap_or_else(refused);
        let contains = run(&format!(
            r#"[["lit",{container}],["lit",{needle}],["contains"]]"#
        ))
        .unwrap();
        assert_eq!(inside, contains, "{needle} in {container}");
        inside
    }

    // Sabotage: with containment answering false for a string container and
    // a needle that is not a string, this test fails.
    #[test]
    fn containment_answers_every_row_of_its_table() {
        let undefined = r#"{"$type":"undefined"}"#;
        assert_eq!(contained("[1,2]", undefined), Value::Undefined);
        assert_eq!(contained(undefined, "1"), Value::Undefined);
        assert_eq!(
            contained(&format!("[{undefined}]"), undefined),
            Value::Undefined
        );
        assert_eq!(contained("[1,2]", "2.0"), Value::from(true));
        assert_eq!(contained("[1,2]", "3"), Value::from(false));
        assert_eq!(contained("[null]", "null"), Value::from(true));
        assert_eq!(contained("[1]", "null"), Value::from(false));
        assert_eq!(
            contained(&format!("[{undefined}]"), "1"),
            Value::from(false)
        );
        assert_eq!(contained(r#"["ab"]"#, r#""a""#), Value::from(false));
        assert_eq!(contained("[[1,2]]", "[1,2.0]"), Value::from(true));
        assert_eq!(
            contained(
                r#"[{"$type":"datetime","value":"2026-03-01T00:00:00Z"}]"#,
                r#"{"$type":"date","value":"2026-03-01"}"#
            ),
            Value::from(true)
        );
        assert_eq!(contained(r#""abc""#, r#""bc""#), Value::from(true));
        assert_eq!(contained(r#""abc""#, r#""BC""#), Value::from(false));
        assert_eq!(contained(r#""abc""#, r#""""#), Value::from(true));
        assert_eq!(contained(r#""""#, r#""a""#), Value::from(false));
        assert_eq!(contained(r#""a1""#, "1"), Value::Undefined);
        assert_eq!(contained(r#""a""#, "null"), Value::Undefined);
        for container in [
            "null",
            "true",
            "12",
            "1.5",
            r#"{"a":1}"#,
            r#"{"$type":"date","value":"2026-03-01"}"#,
            r#"{"$type":"datetime","value":"2026-03-01T00:00:00Z"}"#,
        ] {
            assert_eq!(contained(container, "1"), Value::Undefined, "{container}");
            assert_eq!(
                contained(container, r#""a""#),
                Value::Undefined,
                "{container}"
            );
        }
    }

    // Sabotage: with contains taking the top of the stack as the container,
    // this test fails.
    #[test]
    fn in_takes_its_container_from_the_top_and_contains_from_beneath() {
        assert_eq!(
            run(r#"[["lit",[1]],["lit",[[1]]],["in"]]"#).unwrap_or_else(refused),
            Value::from(true)
        );
        assert_eq!(
            run(r#"[["lit",[[1]]],["lit",[1]],["contains"]]"#).unwrap_or_else(refused),
            Value::from(true)
        );
        assert_eq!(
            run(r#"[["lit",[[1]]],["lit",[1]],["in"]]"#).unwrap_or_else(refused),
            Value::from(false)
        );
        for opcode in ["in", "contains"] {
            assert_eq!(
                reason(run(&format!(r#"[["lit",[1]],["{opcode}"]]"#))).1,
                "insufficient_operands"
            );
        }
    }

    // Sabotage: with make_list reversing the values it takes, this test
    // fails.
    #[test]
    fn make_list_gathers_its_count_in_source_order() {
        assert_eq!(
            run(r#"[["lit",0],["lit",1],["lit",2],["make_list",2]]"#).unwrap_or_else(refused),
            Value::from(vec![Value::from(1), Value::from(2)])
        );
        assert_eq!(
            run(r#"[["lit",0],["make_list",0]]"#).unwrap_or_else(refused),
            Value::from(Vec::new())
        );
        assert_eq!(
            run(r#"[["lit",0],["lit",1],["make_list",2],["make_list",1]]"#).unwrap_or_else(refused),
            Value::from(vec![Value::from(vec![Value::from(0), Value::from(1)])])
        );
        assert_eq!(
            reason(run(r#"[["lit",0],["make_list",2]]"#)).1,
            "insufficient_operands"
        );
        assert_eq!(
            reason(run(r#"[["make_list",9223372036854775807]]"#)).1,
            "insufficient_operands"
        );
    }
}
