//! The `compare` instruction: what one operator makes of one pair of
//! values, as the reference's instruction-set text states it.
//!
//! Strict equality (`STRICT_EQ`, `STRICT_NE`) is [`Value`]'s own `==`: the
//! same kind and the same value, over every value, undefined and null
//! included. Every other operator propagates: undefined on either side, or a
//! pair of different types, answers undefined, never an error. Integer and
//! float are one type here and compare by number; a date and an instant are
//! one type and compare by the instant each names (a date at midnight UTC).
//! Null has no type peer, so every non-strict comparison involving it
//! answers undefined.
//!
//! Two lists compare member by member. Their members follow the reference's
//! term order: members of different kinds are ordered by kind (a number,
//! then `false`, null, `true` and undefined, then the map-like values, then a
//! list, then a string), two dates or two instants order by instant, and a
//! date orders before an instant. Two maps, two durations, or two map-like
//! values of different kinds have no order, at the top level or as members,
//! because the reference's text leaves the order of maps unspecified: an
//! ordering operator on them answers undefined.
//!
//! Every walk over nested lists and maps is iterative, so a pair nested
//! deeper than the stack is compared without exhausting it.

use crate::{Date, DateTime, Value};
use core::cmp::Ordering;

/// An operand of the `compare` instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Operator {
    Gt,
    Lt,
    Eq,
    Gte,
    Lte,
    Ne,
    StrictEq,
    StrictNe,
}

impl Operator {
    /// The operator an instruction names, or `None` for any other text.
    pub(crate) fn named(name: &str) -> Option<Operator> {
        Some(match name {
            "GT" => Operator::Gt,
            "LT" => Operator::Lt,
            "EQ" => Operator::Eq,
            "GTE" => Operator::Gte,
            "LTE" => Operator::Lte,
            "NE" => Operator::Ne,
            "STRICT_EQ" => Operator::StrictEq,
            "STRICT_NE" => Operator::StrictNe,
            _ => return None,
        })
    }
}

/// What `operator` answers for `left` and `right`: `true`, `false` or
/// undefined.
pub(crate) fn compare(operator: Operator, left: &Value, right: &Value) -> Value {
    let answer = match operator {
        Operator::StrictEq => return Value::Bool(left == right),
        Operator::StrictNe => return Value::Bool(left != right),
        // Undefined has no type, so a pair with undefined on either side
        // answers undefined below, as every pair of different types does.
        Operator::Eq => equal(left, right),
        Operator::Ne => equal(left, right).map(|equal| !equal),
        Operator::Gt => order(left, right).map(Ordering::is_gt),
        Operator::Lt => order(left, right).map(Ordering::is_lt),
        Operator::Gte => order(left, right).map(Ordering::is_ge),
        Operator::Lte => order(left, right).map(Ordering::is_le),
    };
    answer.map_or(Value::Undefined, Value::Bool)
}

/// The type a non-strict comparison matches a pair on; null and undefined
/// have none.
#[derive(PartialEq)]
enum Type {
    Number,
    Boolean,
    String,
    List,
    Map,
    Duration,
    Chronological,
}

fn type_of(value: &Value) -> Option<Type> {
    Some(match value {
        Value::Integer(_) | Value::Float(_) => Type::Number,
        Value::Bool(_) => Type::Boolean,
        Value::String(_) => Type::String,
        Value::List(_) => Type::List,
        Value::Map(_) => Type::Map,
        Value::Duration(_) => Type::Duration,
        Value::Date(_) | Value::DateTime(_) => Type::Chronological,
        Value::Null | Value::Undefined => return None,
    })
}

/// Whether two values are the same type for a non-strict comparison.
fn peers(left: &Value, right: &Value) -> bool {
    type_of(left).is_some_and(|kind| type_of(right) == Some(kind))
}

/// A date or an instant as the instant it names, a date at midnight UTC.
fn instant(value: &Value) -> Option<DateTime> {
    match value {
        Value::DateTime(instant) => Some(*instant),
        Value::Date(date) => midnight(*date),
        _ => None,
    }
}

fn midnight(date: Date) -> Option<DateTime> {
    DateTime::new(date, 0, 0, 0, 0)
}

/// The order of two numbers, by number across integer and float.
fn number_order(left: &Value, right: &Value) -> Option<Ordering> {
    match (left, right) {
        (Value::Integer(a), Value::Integer(b)) => Some(a.cmp(b)),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
        (Value::Integer(a), Value::Float(b)) => crate::value::integer_against_float(*a, *b),
        (Value::Float(a), Value::Integer(b)) => {
            crate::value::integer_against_float(*b, *a).map(Ordering::reverse)
        }
        _ => None,
    }
}

/// `==` at the top level: `None` when the pair is not of one type.
fn equal(left: &Value, right: &Value) -> Option<bool> {
    if !peers(left, right) {
        return None;
    }
    if let (Some(left), Some(right)) = (instant(left), instant(right)) {
        return Some(left == right);
    }
    Some(members_equal(left, right))
}

/// Whether a list's `member` equals the `needle` membership looks for, as
/// the reference's membership equality finds them: undefined equals
/// nothing, null equals null (the one place null is a value rather than an
/// absence), and every other pair is equal when `EQ` answers `true` for it,
/// so numbers match across integer and float and a date matches an instant
/// at its midnight UTC.
pub(crate) fn member_equal(member: &Value, needle: &Value) -> bool {
    match (member, needle) {
        (Value::Undefined, _) | (_, Value::Undefined) => false,
        (Value::Null, Value::Null) => true,
        _ => equal(member, needle) == Some(true),
    }
}

/// Whether two values are equal as the reference's structural equality
/// finds them, walked without recursion: numbers by number across integer
/// and float, two dates or two instants by instant, two nulls equal and two
/// undefineds equal (as members, where the reference compares them as
/// terms), a date never equal to an instant, and lists and maps member by
/// member.
fn members_equal(left: &Value, right: &Value) -> bool {
    let mut pending = vec![(left, right)];
    while let Some(pair) = pending.pop() {
        let same = match pair {
            (Value::List(left), Value::List(right)) => {
                if left.len() != right.len() {
                    return false;
                }
                pending.extend(left.iter().zip(right));
                true
            }
            (Value::Map(left), Value::Map(right)) => {
                if left.len() != right.len() {
                    return false;
                }
                for ((left_key, left), (right_key, right)) in left.iter().zip(right.iter()) {
                    if left_key != right_key {
                        return false;
                    }
                    pending.push((left, right));
                }
                true
            }
            (Value::Integer(_) | Value::Float(_), Value::Integer(_) | Value::Float(_)) => {
                number_order(pair.0, pair.1) == Some(Ordering::Equal)
            }
            (left, right) => left == right,
        };
        if !same {
            return false;
        }
    }
    true
}

/// The order of a pair at the top level: `None` when the pair is not of one
/// type, or is of a type with no order (two maps, two durations).
fn order(left: &Value, right: &Value) -> Option<Ordering> {
    if !peers(left, right) {
        return None;
    }
    match (left, right) {
        (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(b)),
        (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
        (Value::List(a), Value::List(b)) => list_order(a, b),
        (Value::Map(_), Value::Map(_)) | (Value::Duration(_), Value::Duration(_)) => None,
        _ => match (instant(left), instant(right)) {
            (Some(left), Some(right)) => Some(left.cmp(&right)),
            _ => number_order(left, right),
        },
    }
}

/// Where a member sits in the reference's term order, for a member pair of
/// two different kinds: a number, then `false`, null, `true` and undefined
/// (the reference's atoms, in the order of their names), then the map-like
/// values (a map, a duration, a date, an instant), then a list, then a
/// string.
fn rank(member: &Value) -> u8 {
    match member {
        Value::Integer(_) | Value::Float(_) => 0,
        Value::Bool(false) => 1,
        Value::Null => 2,
        Value::Bool(true) => 3,
        Value::Undefined => 4,
        Value::Map(_) | Value::Duration(_) | Value::Date(_) | Value::DateTime(_) => 5,
        Value::List(_) => 6,
        Value::String(_) => 7,
    }
}

/// The order of one member pair that is not two lists: `Some(Equal)` when
/// the pair is level (the walk steps to the next position), `None` when it
/// has no order.
fn member_order(left: &Value, right: &Value) -> Option<Ordering> {
    let ranks = (rank(left), rank(right));
    if ranks.0 != ranks.1 {
        return Some(ranks.0.cmp(&ranks.1));
    }
    match (left, right) {
        (Value::Integer(_) | Value::Float(_), _) => number_order(left, right),
        (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
        (Value::Date(a), Value::Date(b)) => Some(a.cmp(b)),
        (Value::DateTime(a), Value::DateTime(b)) => Some(a.cmp(b)),
        (Value::Date(_), Value::DateTime(_)) => Some(Ordering::Less),
        (Value::DateTime(_), Value::Date(_)) => Some(Ordering::Greater),
        _ if ranks.0 == 5 => members_equal(left, right).then_some(Ordering::Equal),
        // Two nulls, two undefineds, two falses or two trues.
        _ => Some(Ordering::Equal),
    }
}

/// Two lists from the front: the first member pair that is not level
/// decides, and a proper prefix orders before the longer list. A member
/// pair of two lists is walked as a list of its own on an explicit stack of
/// frames, so lists nested deeper than the stack are ordered without
/// exhausting it.
fn list_order(left: &[Value], right: &[Value]) -> Option<Ordering> {
    let mut frames: Vec<(&[Value], &[Value], usize)> = vec![(left, right, 0)];
    while let Some((left, right, index)) = frames.last_mut() {
        match (left.get(*index), right.get(*index)) {
            (Some(Value::List(a)), Some(Value::List(b))) => {
                *index += 1;
                frames.push((a, b, 0));
            }
            (Some(a), Some(b)) => {
                *index += 1;
                match member_order(a, b)? {
                    Ordering::Equal => {}
                    decided => return Some(decided),
                }
            }
            (None, Some(_)) => return Some(Ordering::Less),
            (Some(_), None) => return Some(Ordering::Greater),
            (None, None) => {
                frames.pop();
            }
        }
    }
    Some(Ordering::Equal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Duration, Map};

    const OPERATORS: [&str; 8] = [
        "GT",
        "LT",
        "EQ",
        "GTE",
        "LTE",
        "NE",
        "STRICT_EQ",
        "STRICT_NE",
    ];

    fn run(operator: &str, left: Value, right: Value) -> Value {
        compare(Operator::named(operator).unwrap(), &left, &right)
    }

    fn date(year: i32, month: u32, day: u32) -> Date {
        Date::new(year, month, day).unwrap()
    }

    fn days(days: i64) -> Value {
        Value::from(Duration {
            days,
            ..Duration::default()
        })
    }

    fn map(value: Value) -> Value {
        Value::from([("a", value)].into_iter().collect::<Map>())
    }

    fn list(items: Vec<Value>) -> Value {
        Value::from(items)
    }

    // Sabotage: with "GTE" read as Operator::Gt, this test fails.
    #[test]
    fn each_operator_is_read_by_its_name_and_no_other_text_is() {
        let read: Vec<Operator> = OPERATORS
            .iter()
            .map(|name| Operator::named(name).unwrap())
            .collect();
        assert_eq!(
            read,
            [
                Operator::Gt,
                Operator::Lt,
                Operator::Eq,
                Operator::Gte,
                Operator::Lte,
                Operator::Ne,
                Operator::StrictEq,
                Operator::StrictNe
            ]
        );
        for other in ["gt", "FOO", "", "EQ "] {
            assert_eq!(Operator::named(other), None, "{other:?}");
        }
    }

    // Sabotage: with type_of answering Number for undefined, this test fails.
    #[test]
    fn undefined_propagates_except_under_strict_equality() {
        for operator in ["GT", "LT", "EQ", "GTE", "LTE", "NE"] {
            assert_eq!(run(operator, Value::Undefined, 1.into()), Value::Undefined);
            assert_eq!(run(operator, 1.into(), Value::Undefined), Value::Undefined);
            assert_eq!(
                run(operator, Value::Undefined, Value::Undefined),
                Value::Undefined
            );
        }
        assert_eq!(
            run("STRICT_EQ", Value::Undefined, Value::Undefined),
            Value::Bool(true)
        );
        assert_eq!(
            run("STRICT_NE", Value::Undefined, 1.into()),
            Value::Bool(true)
        );
        assert_eq!(
            run("STRICT_EQ", Value::Null, Value::Undefined),
            Value::Bool(false)
        );
    }

    // Sabotage: with type_of answering Number for null, this test fails.
    #[test]
    fn a_pair_of_different_types_answers_undefined_and_null_has_no_peer() {
        for operator in ["GT", "LT", "EQ", "GTE", "LTE", "NE"] {
            assert_eq!(run(operator, 1.into(), "a".into()), Value::Undefined);
            assert_eq!(run(operator, true.into(), 1.into()), Value::Undefined);
            assert_eq!(run(operator, Value::Null, Value::Null), Value::Undefined);
            assert_eq!(run(operator, days(1), map(1.into())), Value::Undefined);
            assert_eq!(run(operator, list(vec![]), map(1.into())), Value::Undefined);
        }
        assert_eq!(
            run("STRICT_EQ", Value::Null, Value::Null),
            Value::Bool(true)
        );
        assert_eq!(run("STRICT_EQ", 1.into(), "1".into()), Value::Bool(false));
    }

    // Sabotage: with number_order's Float-Integer arm left unreversed, this
    // test fails.
    #[test]
    fn numbers_compare_by_number_across_integer_and_float() {
        assert_eq!(run("EQ", 1.into(), 1.0.into()), Value::Bool(true));
        assert_eq!(run("NE", 1.into(), 1.0.into()), Value::Bool(false));
        assert_eq!(run("STRICT_EQ", 1.into(), 1.0.into()), Value::Bool(false));
        assert_eq!(run("STRICT_NE", 1.into(), 1.0.into()), Value::Bool(true));
        assert_eq!(run("LT", 1.into(), 2.0.into()), Value::Bool(true));
        assert_eq!(run("GT", 2.5.into(), 2.into()), Value::Bool(true));
        assert_eq!(run("LTE", 2.5.into(), 2.into()), Value::Bool(false));
        assert_eq!(run("GTE", 5.into(), 5.into()), Value::Bool(true));
        assert_eq!(run("GT", 5.into(), 5.into()), Value::Bool(false));
        assert_eq!(run("LT", 1.5.into(), 2.5.into()), Value::Bool(true));
    }

    // Sabotage: with the Bool arm of order comparing b against a, this test
    // fails.
    #[test]
    fn strings_and_booleans_order_within_their_type() {
        assert_eq!(run("LT", "abc".into(), "abd".into()), Value::Bool(true));
        assert_eq!(run("LT", "Z".into(), "a".into()), Value::Bool(true));
        assert_eq!(run("GT", "é".into(), "z".into()), Value::Bool(true));
        assert_eq!(run("LT", false.into(), true.into()), Value::Bool(true));
        assert_eq!(run("GT", false.into(), true.into()), Value::Bool(false));
        assert_eq!(run("EQ", "a".into(), "a".into()), Value::Bool(true));
    }

    // Sabotage: with instant answering None for a date, this test fails.
    #[test]
    fn dates_and_instants_compare_by_instant_but_are_never_strictly_equal() {
        let day = Value::from(date(2024, 1, 15));
        let at_midnight = Value::from(midnight(date(2024, 1, 15)).unwrap());
        let one_am = Value::from(DateTime::new(date(2024, 1, 15), 1, 0, 0, 0).unwrap());
        assert_eq!(
            run("EQ", day.clone(), at_midnight.clone()),
            Value::Bool(true)
        );
        assert_eq!(
            run("STRICT_EQ", day.clone(), at_midnight.clone()),
            Value::Bool(false)
        );
        assert_eq!(run("LT", day.clone(), one_am.clone()), Value::Bool(true));
        assert_eq!(run("GT", one_am.clone(), day.clone()), Value::Bool(true));
        assert_eq!(
            run("GT", day.clone(), Value::from(date(2024, 1, 10))),
            Value::Bool(true)
        );
        assert_eq!(run("NE", one_am, at_midnight), Value::Bool(true));
    }

    // Sabotage: with the Map and Duration arm of order answering
    // Some(Ordering::Equal), this test fails.
    #[test]
    fn maps_and_durations_are_equal_or_not_but_have_no_order() {
        assert_eq!(run("EQ", map(1.into()), map(1.0.into())), Value::Bool(true));
        assert_eq!(
            run("STRICT_EQ", map(1.into()), map(1.0.into())),
            Value::Bool(false)
        );
        assert_eq!(run("NE", map(1.into()), map(2.into())), Value::Bool(true));
        assert_eq!(run("EQ", days(1), days(1)), Value::Bool(true));
        assert_eq!(run("EQ", days(1), days(2)), Value::Bool(false));
        for operator in ["GT", "LT", "GTE", "LTE"] {
            assert_eq!(
                run(operator, map(1.into()), map(1.into())),
                Value::Undefined
            );
            assert_eq!(run(operator, days(1), days(2)), Value::Undefined);
        }
    }

    // Sabotage: with the integer-float arm of members_equal comparing
    // strictly, this test fails.
    #[test]
    fn lists_are_equal_member_by_member_by_number_and_by_instant() {
        let left = list(vec![1.into(), map(list(vec![2.into()])), Value::Null]);
        let right = list(vec![1.0.into(), map(list(vec![2.0.into()])), Value::Null]);
        assert_eq!(run("EQ", left.clone(), right.clone()), Value::Bool(true));
        assert_eq!(run("STRICT_EQ", left, right), Value::Bool(false));
        assert_eq!(
            run(
                "EQ",
                list(vec![Value::Undefined]),
                list(vec![Value::Undefined])
            ),
            Value::Bool(true)
        );
        assert_eq!(
            run("EQ", list(vec![Value::Null]), list(vec![Value::Undefined])),
            Value::Bool(false)
        );
        assert_eq!(
            run("EQ", list(vec![1.into()]), list(vec![1.into(), 2.into()])),
            Value::Bool(false)
        );
        assert_eq!(
            run("EQ", list(vec![1.into()]), list(vec!["1".into()])),
            Value::Bool(false)
        );
        let day = Value::from(date(2024, 1, 15));
        let at_midnight = Value::from(midnight(date(2024, 1, 15)).unwrap());
        assert_eq!(
            run("EQ", list(vec![day.clone()]), list(vec![day.clone()])),
            Value::Bool(true)
        );
        assert_eq!(
            run("EQ", list(vec![day]), list(vec![at_midnight])),
            Value::Bool(false)
        );
        let keyed = |key: &str| Value::from([(key, Value::Null)].into_iter().collect::<Map>());
        assert_eq!(run("EQ", keyed("a"), keyed("b")), Value::Bool(false));
        assert_eq!(
            run("EQ", keyed("a"), Value::from(Map::new())),
            Value::Bool(false)
        );
    }

    // Sabotage: with rank placing null before false, this test fails.
    #[test]
    fn members_of_different_kinds_order_as_the_reference_orders_terms() {
        let ascending = [
            Value::from(5),
            Value::from(false),
            Value::Null,
            Value::from(true),
            Value::Undefined,
            map(1.into()),
            list(vec![]),
            Value::from("a"),
        ];
        for (i, low) in ascending.iter().enumerate() {
            for high in ascending.iter().skip(i + 1) {
                let low = list(vec![low.clone()]);
                let high = list(vec![high.clone()]);
                assert_eq!(run("LT", low.clone(), high.clone()), Value::Bool(true));
                assert_eq!(run("GT", high, low), Value::Bool(true));
            }
        }
        let day = Value::from(date(2030, 1, 1));
        let earlier = Value::from(DateTime::from_unix(0, 0).unwrap());
        assert_eq!(
            run("LT", list(vec![day.clone()]), list(vec![earlier.clone()])),
            Value::Bool(true)
        );
        assert_eq!(
            run("GT", list(vec![earlier]), list(vec![day.clone()])),
            Value::Bool(true)
        );
        assert_eq!(
            run("LT", list(vec![days(9)]), list(vec!["a".into()])),
            Value::Bool(true)
        );
        assert_eq!(
            run("GT", list(vec![days(9)]), list(vec![9.into()])),
            Value::Bool(true)
        );
    }

    // Sabotage: with member_order answering Some(Ordering::Less) for two
    // map-like values that are not equal, this test fails.
    #[test]
    fn list_members_level_step_on_and_unordered_members_answer_undefined() {
        let level = |member: Value| {
            run(
                "LT",
                list(vec![member.clone(), 1.into()]),
                list(vec![member, 2.into()]),
            )
        };
        for member in [
            Value::Null,
            Value::Undefined,
            Value::from(true),
            Value::from(false),
            Value::from(1.0),
            Value::from("x"),
            map(1.into()),
            days(3),
            Value::from(date(2026, 1, 1)),
        ] {
            assert_eq!(level(member.clone()), Value::Bool(true), "{member:?}");
        }
        assert_eq!(
            run(
                "LT",
                list(vec![1.into(), 9.into()]),
                list(vec![1.0.into(), 10.into()])
            ),
            Value::Bool(true)
        );
        for (left, right) in [
            (map(1.into()), map(2.into())),
            (days(1), days(2)),
            (map(1.into()), days(1)),
            (Value::from(date(2026, 1, 1)), days(1)),
        ] {
            for operator in ["GT", "LT", "GTE", "LTE"] {
                assert_eq!(
                    run(
                        operator,
                        list(vec![left.clone()]),
                        list(vec![right.clone()])
                    ),
                    Value::Undefined
                );
            }
        }
    }

    // Sabotage: with list_order's (None, Some(_)) arm answering Greater, this
    // test fails.
    #[test]
    fn lists_order_from_the_front_with_a_proper_prefix_first() {
        assert_eq!(
            run(
                "LT",
                list(vec![1.into(), 2.into()]),
                list(vec![1.into(), 3.into()])
            ),
            Value::Bool(true)
        );
        assert_eq!(
            run("LT", list(vec![1.into()]), list(vec![1.into(), 2.into()])),
            Value::Bool(true)
        );
        assert_eq!(
            run("GT", list(vec![1.into(), 0.into()]), list(vec![1.into()])),
            Value::Bool(true)
        );
        assert_eq!(run("GTE", list(vec![]), list(vec![])), Value::Bool(true));
        let nested = |innermost: i64, tail: i64| {
            list(vec![list(vec![list(vec![innermost.into()])]), tail.into()])
        };
        assert_eq!(run("LT", nested(1, 9), nested(2, 0)), Value::Bool(true));
        assert_eq!(run("LT", nested(1, 0), nested(1, 9)), Value::Bool(true));
        assert_eq!(run("LTE", nested(1, 0), nested(1, 0)), Value::Bool(true));
        assert_eq!(
            run(
                "LT",
                list(vec![list(vec![1.into()]), 5.into()]),
                list(vec![list(vec![1.into(), 0.into()]), 0.into()])
            ),
            Value::Bool(true)
        );
    }

    const LEVELS: usize = 100_000;
    const CHILD: &str = "PREDICATOR_DEEP_COMPARE_CHILD";

    fn deep(innermost: i64) -> Value {
        let mut value = Value::Integer(innermost);
        for level in 0..LEVELS {
            value = if level % 2 == 0 {
                Value::List(vec![value])
            } else {
                Value::Map([("k", value)].into_iter().collect::<Map>())
            };
        }
        value
    }

    fn deep_lists(innermost: i64) -> Value {
        let mut value = Value::Integer(innermost);
        for _ in 0..LEVELS {
            value = Value::List(vec![value]);
        }
        value
    }

    fn deep_work() {
        assert_eq!(run("EQ", deep(1), deep(1)), Value::Bool(true));
        assert_eq!(run("NE", deep(1), deep(2)), Value::Bool(true));
        assert_eq!(run("LT", deep_lists(1), deep_lists(2)), Value::Bool(true));
        assert_eq!(run("GTE", deep_lists(1), deep_lists(1)), Value::Bool(true));
    }

    // The work itself, run only in the child process the next test starts.
    #[test]
    fn deep_compare_child() {
        if std::env::var_os(CHILD).is_none() {
            return;
        }
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(deep_work)
            .unwrap()
            .join()
            .unwrap();
    }

    // Sabotage: with list_order's two-list arm answering list_order(a, b)
    // by recursion, this test fails.
    #[test]
    fn a_pair_far_deeper_than_the_stack_is_compared_without_recursion() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "compare::tests::deep_compare_child",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "the child failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
