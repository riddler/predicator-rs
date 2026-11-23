//! The value domain: what a context holds, an instruction pushes and an
//! evaluation answers.

use crate::{Date, DateTime, Duration, Key, Map};
use core::cmp::Ordering;
use core::fmt;

/// A predicator value.
///
/// `Null` and `Undefined` are both values and neither is the other:
/// undefined is an absence (no value was ever supplied), null is a value
/// that is present and empty. An integer is an `i64`; a float an `f64`.
///
/// Equality (`==`) is strict: two values are equal when they are the same
/// kind and the same value, so `Integer(1)` and `Float(1.0)` are not equal,
/// two instants are equal when they are the same instant, and lists and
/// maps are equal member by member. Ordering (`partial_cmp`) agrees with
/// it: integers and floats order by number (an integer and a float at the
/// same number are unordered, being unequal), strings by codepoint, `false`
/// before `true`, dates and instants chronologically, lists member by
/// member with a proper prefix first; two maps, two durations or two values
/// of other kinds are unordered unless equal. The `compare` instruction's
/// own rules are the evaluator's, not these.
///
/// Equality, ordering, cloning, `Debug` and dropping walk a value without
/// recursion, so a value nested deeper than the stack is compared, copied,
/// printed and dropped without exhausting it. Because `Value` implements
/// `Drop`, a member is taken out of a value with [`core::mem::replace`]
/// rather than moved out of a pattern.
///
/// ```
/// use predicator_rs::Value;
///
/// let list = Value::from(vec![Value::from(1), Value::from("x"), Value::Null]);
/// assert_eq!(list, Value::List(vec![1.into(), "x".into(), Value::Null]));
/// assert_ne!(Value::from(1), Value::from(1.0));
/// assert!(Value::from(1) < Value::from(1.5));
/// assert_ne!(Value::Null, Value::Undefined);
/// ```
#[non_exhaustive]
pub enum Value {
    /// A value that is present and empty.
    Null,
    /// An absence: no value was ever supplied.
    Undefined,
    /// `true` or `false`.
    Bool(bool),
    /// An integer.
    Integer(i64),
    /// A float.
    Float(f64),
    /// A string.
    String(String),
    /// A list.
    List(Vec<Value>),
    /// A map from [`Key`]s.
    Map(Map),
    /// A civil date.
    Date(Date),
    /// An instant in UTC.
    DateTime(DateTime),
    /// A duration.
    Duration(Duration),
}

/// Moves the members of a list or a map out of `value` onto `into`.
fn take_members(value: &mut Value, into: &mut Vec<Value>) {
    match value {
        Value::List(items) => into.append(items),
        Value::Map(map) => into.extend(core::mem::take(&mut map.entries).into_values()),
        _ => {}
    }
}

impl Drop for Value {
    /// Drops the members of a list or a map from a work list rather than by
    /// recursion: each member is emptied of its own members before it is
    /// dropped.
    fn drop(&mut self) {
        let mut pending = Vec::new();
        take_members(self, &mut pending);
        while let Some(mut member) = pending.pop() {
            take_members(&mut member, &mut pending);
        }
    }
}

/// A copy of a value that is neither a list nor a map; a list or a map
/// comes back empty, its members built by the caller.
fn shallow_copy(value: &Value) -> Value {
    match value {
        Value::Null => Value::Null,
        Value::Undefined => Value::Undefined,
        Value::Bool(flag) => Value::Bool(*flag),
        Value::Integer(number) => Value::Integer(*number),
        Value::Float(number) => Value::Float(*number),
        Value::String(text) => Value::String(text.clone()),
        Value::List(_) => Value::List(Vec::new()),
        Value::Map(_) => Value::Map(Map::new()),
        Value::Date(date) => Value::Date(*date),
        Value::DateTime(instant) => Value::DateTime(*instant),
        Value::Duration(duration) => Value::Duration(*duration),
    }
}

/// One step of an iterative copy.
enum Copy<'a> {
    /// Copy this value onto the built stack.
    Visit(&'a Value),
    /// Gather the last `count` built values into a list.
    List(usize),
    /// Gather the last built values, one per key, into a map.
    Map(Vec<&'a Key>),
}

impl Clone for Value {
    fn clone(&self) -> Value {
        let mut work = vec![Copy::Visit(self)];
        let mut built: Vec<Value> = Vec::new();
        while let Some(step) = work.pop() {
            match step {
                Copy::Visit(Value::List(items)) => {
                    work.push(Copy::List(items.len()));
                    work.extend(items.iter().rev().map(Copy::Visit));
                }
                Copy::Visit(Value::Map(map)) => {
                    work.push(Copy::Map(map.entries.keys().collect()));
                    work.extend(map.entries.values().rev().map(Copy::Visit));
                }
                Copy::Visit(value) => built.push(shallow_copy(value)),
                Copy::List(count) => {
                    let items = built.split_off(built.len().saturating_sub(count));
                    built.push(Value::List(items));
                }
                Copy::Map(keys) => {
                    let values = built.split_off(built.len().saturating_sub(keys.len()));
                    let entries = keys.into_iter().cloned().zip(values).collect();
                    built.push(Value::Map(Map { entries }));
                }
            }
        }
        built.pop().unwrap_or(Value::Null)
    }
}

/// Whether two values that are not both lists nor both maps are equal.
fn scalar_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Null, Value::Null) | (Value::Undefined, Value::Undefined) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Integer(a), Value::Integer(b)) => a == b,
        (Value::Float(a), Value::Float(b)) => a == b,
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Date(a), Value::Date(b)) => a == b,
        (Value::DateTime(a), Value::DateTime(b)) => a == b,
        (Value::Duration(a), Value::Duration(b)) => a == b,
        _ => false,
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Value) -> bool {
        let mut pending = vec![(self, other)];
        while let Some(pair) = pending.pop() {
            match pair {
                (Value::List(left), Value::List(right)) => {
                    if left.len() != right.len() {
                        return false;
                    }
                    pending.extend(left.iter().zip(right));
                }
                (Value::Map(left), Value::Map(right)) => {
                    if left.len() != right.len() {
                        return false;
                    }
                    for ((left_key, left), (right_key, right)) in
                        left.entries.iter().zip(&right.entries)
                    {
                        if left_key != right_key {
                            return false;
                        }
                        pending.push((left, right));
                    }
                }
                (left, right) => {
                    if !scalar_equal(left, right) {
                        return false;
                    }
                }
            }
        }
        true
    }
}

/// How an integer stands to a finite float, by number: exactly, with no
/// rounding of the integer to a float.
fn integer_against_float(integer: i64, float: f64) -> Option<Ordering> {
    // 2^63, exactly representable as a float.
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    if float.is_nan() {
        return None;
    }
    if float >= LIMIT {
        return Some(Ordering::Less);
    }
    if float < -LIMIT {
        return Some(Ordering::Greater);
    }
    let whole = float.trunc();
    // In range and whole, so the cast is exact; at the same whole number
    // the fraction decides.
    match integer.cmp(&(whole as i64)) {
        Ordering::Equal => 0.0_f64.partial_cmp(&(float - whole)),
        decided => Some(decided),
    }
}

/// How two values that are not both lists stand to each other; `None` when
/// they are unordered.
fn scalar_order(left: &Value, right: &Value) -> Option<Ordering> {
    let unequal = |ordering: Option<Ordering>| ordering.filter(|&o| o != Ordering::Equal);
    match (left, right) {
        (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(b)),
        (Value::Integer(a), Value::Integer(b)) => Some(a.cmp(b)),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
        (Value::Integer(a), Value::Float(b)) => unequal(integer_against_float(*a, *b)),
        (Value::Float(a), Value::Integer(b)) => {
            unequal(integer_against_float(*b, *a)).map(Ordering::reverse)
        }
        (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
        (Value::Date(a), Value::Date(b)) => Some(a.cmp(b)),
        (Value::DateTime(a), Value::DateTime(b)) => Some(a.cmp(b)),
        _ => (left == right).then_some(Ordering::Equal),
    }
}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Value) -> Option<Ordering> {
        // Each frame is a pair of lists and the index of the next member
        // pair to compare.
        let mut frames: Vec<(&[Value], &[Value], usize)> = Vec::new();
        let mut current = (self, other);
        loop {
            match current {
                (Value::List(left), Value::List(right)) => frames.push((left, right, 0)),
                (left, right) => match scalar_order(left, right) {
                    Some(Ordering::Equal) => {}
                    decided => return decided,
                },
            }
            loop {
                let Some((left, right, index)) = frames.last_mut() else {
                    return Some(Ordering::Equal);
                };
                match (left.get(*index), right.get(*index)) {
                    (Some(left), Some(right)) => {
                        *index += 1;
                        current = (left, right);
                        break;
                    }
                    (None, Some(_)) => return Some(Ordering::Less),
                    (Some(_), None) => return Some(Ordering::Greater),
                    (None, None) => {
                        frames.pop();
                    }
                }
            }
        }
    }
}

/// One piece of an iterative `Debug` print.
enum Piece<'a> {
    Value(&'a Value),
    Text(&'static str),
    Key(&'a Key),
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut work = vec![Piece::Value(self)];
        while let Some(piece) = work.pop() {
            match piece {
                Piece::Text(text) => f.write_str(text)?,
                Piece::Key(key) => write!(f, "{key:?}: ")?,
                Piece::Value(Value::List(items)) => {
                    f.write_str("List([")?;
                    work.push(Piece::Text("])"));
                    for (index, item) in items.iter().enumerate().rev() {
                        work.push(Piece::Value(item));
                        if index > 0 {
                            work.push(Piece::Text(", "));
                        }
                    }
                }
                Piece::Value(Value::Map(map)) => {
                    f.write_str("Map({")?;
                    work.push(Piece::Text("})"));
                    for (index, (key, item)) in map.entries.iter().enumerate().rev() {
                        work.push(Piece::Value(item));
                        work.push(Piece::Key(key));
                        if index > 0 {
                            work.push(Piece::Text(", "));
                        }
                    }
                }
                Piece::Value(Value::Null) => f.write_str("Null")?,
                Piece::Value(Value::Undefined) => f.write_str("Undefined")?,
                Piece::Value(Value::Bool(flag)) => write!(f, "Bool({flag})")?,
                Piece::Value(Value::Integer(number)) => write!(f, "Integer({number})")?,
                Piece::Value(Value::Float(number)) => write!(f, "Float({number:?})")?,
                Piece::Value(Value::String(text)) => write!(f, "String({text:?})")?,
                Piece::Value(Value::Date(date)) => write!(f, "{date:?}")?,
                Piece::Value(Value::DateTime(instant)) => write!(f, "{instant:?}")?,
                Piece::Value(Value::Duration(duration)) => write!(f, "{duration:?}")?,
            }
        }
        Ok(())
    }
}

impl From<bool> for Value {
    fn from(flag: bool) -> Value {
        Value::Bool(flag)
    }
}

impl From<i64> for Value {
    fn from(number: i64) -> Value {
        Value::Integer(number)
    }
}

impl From<f64> for Value {
    fn from(number: f64) -> Value {
        Value::Float(number)
    }
}

impl From<&str> for Value {
    fn from(text: &str) -> Value {
        Value::String(text.to_owned())
    }
}

impl From<String> for Value {
    fn from(text: String) -> Value {
        Value::String(text)
    }
}

impl From<Vec<Value>> for Value {
    fn from(items: Vec<Value>) -> Value {
        Value::List(items)
    }
}

impl From<Map> for Value {
    fn from(map: Map) -> Value {
        Value::Map(map)
    }
}

impl From<Date> for Value {
    fn from(date: Date) -> Value {
        Value::Date(date)
    }
}

impl From<DateTime> for Value {
    fn from(instant: DateTime) -> Value {
        Value::DateTime(instant)
    }
}

impl From<Duration> for Value {
    fn from(duration: Duration) -> Value {
        Value::Duration(duration)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u32, day: u32) -> Date {
        Date::new(year, month, day).unwrap()
    }

    fn every_kind() -> Vec<Value> {
        let instant = DateTime::new(date(2026, 8, 9), 10, 30, 0, 500_000).unwrap();
        vec![
            Value::Null,
            Value::Undefined,
            Value::from(true),
            Value::from(-7),
            Value::from(2.5),
            Value::from("x"),
            Value::from(vec![Value::from(1), Value::Null]),
            Value::from([("a", Value::from(1))].into_iter().collect::<Map>()),
            Value::from(date(2026, 8, 9)),
            Value::from(instant),
            Value::from(Duration {
                days: 3,
                ..Duration::default()
            }),
        ]
    }

    // Sabotage: with scalar_equal's Duration arm answering true for any two
    // durations, this test fails.
    #[test]
    fn each_kind_equals_itself_and_no_other_kind() {
        let values = every_kind();
        for (i, left) in values.iter().enumerate() {
            for (j, right) in values.iter().enumerate() {
                assert_eq!(left == right, i == j, "{left:?} against {right:?}");
            }
            assert_eq!(&left.clone(), left);
        }
        assert_ne!(Value::from(1), Value::from(1.0));
        assert_ne!(Value::from("x"), Value::from("y"));
        assert_ne!(
            Value::from(Duration {
                days: 3,
                ..Duration::default()
            }),
            Value::from(Duration {
                days: 4,
                ..Duration::default()
            })
        );
        assert_ne!(Value::from(vec![Value::from(1)]), Value::from(vec![]));
        let map = |key: &str| Value::from([(key, Value::Null)].into_iter().collect::<Map>());
        assert_ne!(map("a"), map("b"));
        assert_ne!(map("a"), Value::from(Map::new()));
        let keyed = |key: Key| Value::from([(key, Value::Null)].into_iter().collect::<Map>());
        assert_ne!(keyed(Key::from(1)), keyed(Key::from("1")));
        assert_ne!(keyed(Key::from(true)), keyed(Key::from("true")));
        assert_eq!(keyed(Key::Undefined), keyed(Key::Undefined));
        assert_ne!(
            Value::from([("a", Value::from(1))].into_iter().collect::<Map>()),
            Value::from([("a", Value::from(2))].into_iter().collect::<Map>())
        );
    }

    // Sabotage: with the reverse dropped from the Float-Integer arm of
    // scalar_order, this test fails.
    #[test]
    fn values_order_within_their_kind_and_numbers_across_two() {
        let order = |a: Value, b: Value| a.partial_cmp(&b);
        assert_eq!(order(1.into(), 2.into()), Some(Ordering::Less));
        assert_eq!(order(2.5.into(), 1.5.into()), Some(Ordering::Greater));
        assert_eq!(order(1.into(), 1.5.into()), Some(Ordering::Less));
        assert_eq!(order(1.5.into(), 1.into()), Some(Ordering::Greater));
        assert_eq!(order(2.into(), 1.5.into()), Some(Ordering::Greater));
        assert_eq!(order((-2).into(), (-1.5).into()), Some(Ordering::Less));
        assert_eq!(order(1.into(), 1.0.into()), None);
        assert_eq!(order(i64::MAX.into(), 9.3e18.into()), Some(Ordering::Less));
        assert_eq!(
            order(i64::MAX.into(), (i64::MAX as f64).into()),
            Some(Ordering::Less)
        );
        assert_eq!(
            order(i64::MIN.into(), (-9.3e18).into()),
            Some(Ordering::Greater)
        );
        assert_eq!(order(1.into(), f64::NAN.into()), None);
        assert_eq!(order("b".into(), "a".into()), Some(Ordering::Greater));
        assert_eq!(order("Z".into(), "a".into()), Some(Ordering::Less));
        assert_eq!(order(false.into(), true.into()), Some(Ordering::Less));
        assert_eq!(
            order(date(2026, 1, 1).into(), date(2025, 12, 31).into()),
            Some(Ordering::Greater)
        );
        let instant = |seconds| Value::from(DateTime::from_unix(seconds, 0).unwrap());
        assert_eq!(order(instant(0), instant(1)), Some(Ordering::Less));
        assert_eq!(order(Value::Null, Value::Null), Some(Ordering::Equal));
        assert_eq!(order(Value::Null, Value::Undefined), None);
        assert_eq!(order(1.into(), "1".into()), None);
        let days = |days| {
            Value::from(Duration {
                days,
                ..Duration::default()
            })
        };
        assert_eq!(order(days(1), days(2)), None);
        assert_eq!(order(days(1), days(1)), Some(Ordering::Equal));
        let map = |n: i64| Value::from([("a", Value::from(n))].into_iter().collect::<Map>());
        assert_eq!(order(map(1), map(2)), None);
        assert_eq!(order(map(1), map(1)), Some(Ordering::Equal));
    }

    // Sabotage: with a proper prefix ordered after the longer list, this
    // test fails.
    #[test]
    fn lists_order_member_by_member() {
        let list = |items: Vec<Value>| Value::from(items);
        let order = |a: Value, b: Value| a.partial_cmp(&b);
        assert_eq!(
            order(
                list(vec![1.into(), 2.into()]),
                list(vec![1.into(), 3.into()])
            ),
            Some(Ordering::Less)
        );
        assert_eq!(
            order(list(vec![1.into()]), list(vec![1.into(), 0.into()])),
            Some(Ordering::Less)
        );
        assert_eq!(
            order(list(vec![1.into(), 0.into()]), list(vec![1.into()])),
            Some(Ordering::Greater)
        );
        assert_eq!(
            order(
                list(vec![list(vec![]), 2.into()]),
                list(vec![list(vec![]), 2.into()])
            ),
            Some(Ordering::Equal)
        );
        assert_eq!(
            order(
                list(vec![list(vec![1.into()]), 0.into()]),
                list(vec![list(vec![2.into()])])
            ),
            Some(Ordering::Less)
        );
        assert_eq!(order(list(vec![1.into()]), list(vec!["a".into()])), None);
        assert_eq!(order(list(vec![]), 1.into()), None);
    }

    // Sabotage: with the separator pushed before every list member, the
    // first included, this test fails.
    #[test]
    fn debug_prints_every_kind() {
        let printed = format!("{:?}", Value::from(every_kind()));
        assert_eq!(
            printed,
            "List([Null, Undefined, Bool(true), Integer(-7), Float(2.5), String(\"x\"), \
             List([Integer(1), Null]), Map({\"a\": Integer(1)}), Date(2026-08-09), \
             DateTime(2026-08-09T10:30:00.500000Z), Duration { years: 0, months: 0, weeks: 0, \
             days: 3, hours: 0, minutes: 0, seconds: 0, milliseconds: 0 }])"
        );
        let map: Map = [("a", Value::Null), ("b", Value::Null)]
            .into_iter()
            .collect();
        assert_eq!(
            format!("{:?}", Value::from(map)),
            "Map({\"a\": Null, \"b\": Null})"
        );
    }

    // Sabotage: with Clone's list step gathering one member fewer, this
    // test fails.
    #[test]
    fn a_clone_is_equal_and_separate() {
        let original = Value::from(vec![
            Value::from(every_kind()),
            Value::from(
                [("k", Value::from(every_kind()))]
                    .into_iter()
                    .collect::<Map>(),
            ),
        ]);
        let mut copy = original.clone();
        assert!(copy == original);
        if let Value::List(items) = &mut copy {
            items.push(Value::Null);
        }
        assert!(copy != original);
    }
}
