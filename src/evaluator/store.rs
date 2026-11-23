//! `store`, the one instruction that writes the context, as the reference's
//! instruction-set text states it: a path of segments, root first, each a
//! string (a map key) or an integer (a list index, or a map key in a map),
//! and the value written at its end.
//!
//! A write is checked whole before anything changes, so a refused write
//! leaves no part of itself behind. Each interior slot on the path that is
//! missing, `null` or undefined becomes a list when the segment after it is
//! an integer and a map otherwise; a duration is written through as the
//! plain map of its eight units; anything else in an interior slot (a
//! scalar, a date, an instant) is refused as `not_a_container`. A list
//! takes an integer index of at least 0 (a negative one is
//! `invalid_index`, a string `not_a_container`); an index past its end pads
//! the gap with undefined, up to [`LIST_LIMIT`] items (past that,
//! `list_too_long`). The leaf is overwritten whatever it holds.

use super::{Fault, mismatch};
use crate::{Duration, EvaluationError, Key, Map, Value};

/// The longest a store may make a list by writing past its end.
pub(super) const LIST_LIMIT: usize = 1 << 20;

/// One segment of a store's path.
#[derive(Debug, Clone, Copy)]
pub(super) enum Segment<'a> {
    /// A string: a map key.
    Key(&'a str),
    /// An integer: a list index, or a map key in a map.
    Index(i64),
}

impl Segment<'_> {
    /// The segment as a map key.
    pub(super) fn key(self) -> Key {
        match self {
            Segment::Key(text) => Key::from(text),
            Segment::Index(number) => Key::Integer(number),
        }
    }
}

/// The segments of `path`, root first, or the type mismatch of the first
/// that is neither a string nor an integer.
pub(super) fn segments(path: &[Value]) -> Result<Vec<Segment<'_>>, Fault> {
    path.iter()
        .map(|segment| match segment {
            Value::String(text) => Ok(Segment::Key(text)),
            Value::Integer(number) => Ok(Segment::Index(*number)),
            other => Err(mismatch("store", "string or an integer", other)),
        })
        .collect()
}

/// A container on the path, as the check sees it: `None` for one the write
/// would create.
#[derive(Clone, Copy)]
enum Container<'v> {
    Map(Option<&'v Map>),
    List(Option<&'v [Value]>),
    Duration,
}

/// What a slot on the path holds, as the check sees it.
enum Slot<'v> {
    Absent,
    Map(&'v Map),
    List(&'v [Value]),
    Duration,
    Scalar(&'static str),
}

fn slot(value: Option<&Value>) -> Slot<'_> {
    match value {
        None | Some(Value::Null | Value::Undefined) => Slot::Absent,
        Some(Value::Map(map)) => Slot::Map(map),
        Some(Value::List(items)) => Slot::List(items),
        Some(Value::Duration(_)) => Slot::Duration,
        Some(other) => Slot::Scalar(super::type_name(other)),
    }
}

/// The path written the way a source writes it: `a.b`, `xs[2]`.
fn written(path: &[Segment<'_>]) -> String {
    let mut text = String::new();
    for (at, segment) in path.iter().enumerate() {
        match segment {
            Segment::Index(number) => text.push_str(&format!("[{number}]")),
            Segment::Key(key) if at == 0 => text.push_str(key),
            Segment::Key(key) => text.push_str(&format!(".{key}")),
        }
    }
    text
}

/// The index `segment` names in a list of `length` items.
fn index(
    segment: Segment<'_>,
    length: usize,
    path: &[Segment<'_>],
) -> Result<usize, EvaluationError> {
    match segment {
        Segment::Key(_) => Err(EvaluationError::new(
            "not_a_container",
            format!("store cannot write {} into a list by a key", written(path)),
        )),
        Segment::Index(number) if number < 0 => Err(EvaluationError::new(
            "invalid_index",
            format!(
                "store cannot write {}: a list index is at least 0",
                written(path)
            ),
        )),
        Segment::Index(number) => {
            let at = usize::try_from(number).unwrap_or(usize::MAX);
            if at >= length && at >= LIST_LIMIT {
                return Err(EvaluationError::new(
                    "list_too_long",
                    format!(
                        "store cannot write {}: a store makes a list at most {LIST_LIMIT} items long",
                        written(path)
                    ),
                ));
            }
            Ok(at)
        }
    }
}

/// Whether writing at `path` in `bindings` succeeds, without writing: the
/// refusal the write would meet, or `Ok`.
pub(super) fn check(bindings: &Map, path: &[Segment<'_>]) -> Result<(), EvaluationError> {
    if path.is_empty() {
        return Err(EvaluationError::new(
            "not_assignable",
            "store has no location to write",
        ));
    }
    let mut container = Container::Map(Some(bindings));
    for (at, segment) in path.iter().enumerate() {
        let through = path.get(..=at).unwrap_or(path);
        let child = match container {
            Container::Map(map) => slot(map.and_then(|map| map.get(&segment.key()))),
            Container::List(items) => {
                let at = index(*segment, items.map_or(0, <[Value]>::len), through)?;
                slot(items.and_then(|items| items.get(at)))
            }
            Container::Duration => match segment {
                Segment::Key(name)
                    if Duration::default()
                        .units()
                        .iter()
                        .any(|unit| unit.0 == *name) =>
                {
                    Slot::Scalar("integer")
                }
                _ => Slot::Absent,
            },
        };
        let Some(next) = path.get(at.saturating_add(1)) else {
            return Ok(());
        };
        container = match child {
            Slot::Absent => match next {
                Segment::Index(_) => Container::List(None),
                Segment::Key(_) => Container::Map(None),
            },
            Slot::Map(map) => Container::Map(Some(map)),
            Slot::List(items) => Container::List(Some(items)),
            Slot::Duration => Container::Duration,
            Slot::Scalar(kind) => {
                return Err(EvaluationError::new(
                    "not_a_container",
                    format!("store cannot write into the {kind} at {}", written(through)),
                ));
            }
        };
    }
    Ok(())
}

/// The map of `duration`'s eight units.
fn units(duration: &Duration) -> Map {
    duration
        .units()
        .into_iter()
        .map(|(name, _, amount)| (name, Value::Integer(amount)))
        .collect()
}

/// `slot` made the container the segment `next` writes into, where the
/// write makes one: a list for an integer and a map otherwise in place of
/// nothing, and a duration's units as a map.
fn prepare(slot: &mut Value, next: Segment<'_>) {
    let replacement = match (&*slot, next) {
        (Value::Null | Value::Undefined, Segment::Index(_)) => Value::List(Vec::new()),
        (Value::Null | Value::Undefined, Segment::Key(_)) => Value::Map(Map::new()),
        (Value::Duration(duration), _) => Value::Map(units(duration)),
        _ => return,
    };
    *slot = replacement;
}

/// Writes `value` at `path` in `bindings`. The path is one [`check`] has
/// passed, so every step finds the container it expects.
pub(super) fn write(bindings: &mut Map, path: &[Segment<'_>], value: Value) {
    let Some((first, rest)) = path.split_first() else {
        return;
    };
    let mut slot = bindings
        .entries
        .entry(first.key())
        .or_insert(Value::Undefined);
    for segment in rest {
        prepare(slot, *segment);
        slot = match slot {
            Value::Map(map) => map.entries.entry(segment.key()).or_insert(Value::Undefined),
            Value::List(items) => {
                let Segment::Index(number) = segment else {
                    return;
                };
                let Ok(at) = usize::try_from(*number) else {
                    return;
                };
                if at >= items.len() {
                    items.resize_with(at.saturating_add(1), || Value::Undefined);
                }
                let Some(item) = items.get_mut(at) else {
                    return;
                };
                item
            }
            _ => return,
        };
    }
    *slot = value;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(values: &[Value]) -> Vec<Segment<'_>> {
        segments(values).ok().unwrap()
    }

    /// The bindings after writing `value` at `segments`, or the reason the
    /// write is refused, with the bindings left as they were.
    fn put(bindings: &Map, segments: &[Value], value: Value) -> Result<Map, String> {
        let path = path(segments);
        match check(bindings, &path) {
            Ok(()) => {
                let mut written = bindings.clone();
                write(&mut written, &path, value);
                Ok(written)
            }
            Err(error) => Err(error.reason().to_owned()),
        }
    }

    fn map(entries: &[(&str, Value)]) -> Map {
        entries
            .iter()
            .map(|(key, value)| (*key, value.clone()))
            .collect()
    }

    fn s(text: &str) -> Value {
        Value::from(text)
    }

    fn i(number: i64) -> Value {
        Value::from(number)
    }

    // Sabotage: with prepare vivifying a map whatever the next segment, this
    // test fails.
    #[test]
    fn a_missing_null_or_undefined_slot_becomes_the_container_the_next_segment_wants() {
        for absent in [None, Some(Value::Null), Some(Value::Undefined)] {
            let bindings = match &absent {
                Some(value) => map(&[("a", value.clone())]),
                None => Map::new(),
            };
            assert_eq!(
                put(&bindings, &[s("a"), i(1), s("b")], i(5)),
                Ok(map(&[(
                    "a",
                    Value::List(vec![Value::Undefined, Value::Map(map(&[("b", i(5))]))])
                )])),
                "{absent:?}"
            );
            assert_eq!(
                put(&bindings, &[s("a"), s("b")], i(5)),
                Ok(map(&[("a", Value::Map(map(&[("b", i(5))])))])),
                "{absent:?}"
            );
        }
    }

    // Sabotage: with an integer segment in a map written as its text, this
    // test fails.
    #[test]
    fn a_map_takes_an_integer_segment_as_an_integer_key() {
        let bindings = map(&[("m", Value::Map(Map::new()))]);
        let mut inner = Map::new();
        inner.insert(Key::Integer(0), s("x"));
        assert_eq!(
            put(&bindings, &[s("m"), i(0)], s("x")),
            Ok(map(&[("m", Value::Map(inner))]))
        );
        let mut root = Map::new();
        root.insert(Key::Integer(3), i(1));
        assert_eq!(put(&Map::new(), &[i(3)], i(1)), Ok(root));
    }

    // Sabotage: with the check skipping a scalar in an interior slot, this
    // test fails.
    #[test]
    fn a_scalar_a_date_or_an_instant_on_the_path_is_not_a_container() {
        let date = Value::Date(crate::Date::new(2030, 1, 15).unwrap());
        let instant = Value::DateTime(crate::DateTime::from_unix(0, 0).unwrap());
        for held in [
            i(1),
            s("x"),
            Value::from(true),
            Value::from(1.5),
            date,
            instant,
        ] {
            let bindings = map(&[("a", held.clone())]);
            assert_eq!(
                put(&bindings, &[s("a"), s("b")], i(2)),
                Err("not_a_container".to_owned()),
                "{held:?}"
            );
            assert_eq!(put(&bindings, &[s("a")], i(2)), Ok(map(&[("a", i(2))])));
        }
    }

    // Sabotage: with prepare leaving a duration as it is, this test fails.
    #[test]
    fn a_duration_is_written_through_as_the_map_of_its_units() {
        let duration = Duration {
            days: 2,
            ..Duration::default()
        };
        let bindings = map(&[("d", Value::Duration(duration))]);
        let mut expected = units(&duration);
        expected.insert("x", i(1));
        assert_eq!(
            put(&bindings, &[s("d"), s("x")], i(1)),
            Ok(map(&[("d", Value::Map(expected))]))
        );
        let mut days = units(&duration);
        days.insert("days", i(9));
        assert_eq!(
            put(&bindings, &[s("d"), s("days")], i(9)),
            Ok(map(&[("d", Value::Map(days))]))
        );
        assert_eq!(
            put(&bindings, &[s("d"), s("days"), s("x")], i(9)),
            Err("not_a_container".to_owned())
        );
        let mut deeper = units(&duration);
        deeper.insert("x", Value::Map(map(&[("y", i(3))])));
        assert_eq!(
            put(&bindings, &[s("d"), s("x"), s("y")], i(3)),
            Ok(map(&[("d", Value::Map(deeper))]))
        );
    }

    // Sabotage: with index accepting a negative index as 0, this test fails.
    #[test]
    fn a_list_takes_a_non_negative_index_and_pads_past_its_end() {
        let bindings = map(&[("xs", Value::List(vec![i(1)]))]);
        assert_eq!(
            put(&bindings, &[s("xs"), i(2)], s("x")),
            Ok(map(&[(
                "xs",
                Value::List(vec![i(1), Value::Undefined, s("x")])
            )]))
        );
        assert_eq!(
            put(&bindings, &[s("xs"), i(0)], s("x")),
            Ok(map(&[("xs", Value::List(vec![s("x")]))]))
        );
        assert_eq!(
            put(&bindings, &[s("xs"), i(-1)], s("x")),
            Err("invalid_index".to_owned())
        );
        assert_eq!(
            put(&bindings, &[s("xs"), s("k")], s("x")),
            Err("not_a_container".to_owned())
        );
        assert_eq!(
            put(&bindings, &[s("xs"), s("k"), s("j")], s("x")),
            Err("not_a_container".to_owned())
        );
        assert_eq!(
            put(&Map::new(), &[s("ys"), i(-2), i(0)], s("x")),
            Err("invalid_index".to_owned())
        );
    }

    // Sabotage: with the padding bound written at >= LIST_LIMIT + 1, this
    // test fails.
    #[test]
    fn a_store_makes_a_list_at_most_the_limit_long() {
        let last = i64::try_from(LIST_LIMIT).unwrap() - 1;
        let padded = put(&Map::new(), &[s("xs"), i(last)], i(1)).unwrap();
        let Some(Value::List(items)) = padded.get(&Key::from("xs")) else {
            panic!("no list at xs: {padded:?}");
        };
        assert_eq!(items.len(), LIST_LIMIT);
        assert_eq!(
            put(&Map::new(), &[s("xs"), i(last + 1)], i(1)),
            Err("list_too_long".to_owned())
        );
        assert_eq!(
            put(&Map::new(), &[s("xs"), i(i64::MAX)], i(1)),
            Err("list_too_long".to_owned())
        );
        // A list already longer than the limit may still be written inside
        // its end, and not past it.
        let long = map(&[("xs", Value::List(vec![Value::Null; LIST_LIMIT + 1]))]);
        assert!(put(&long, &[s("xs"), i(last + 1)], i(1)).is_ok());
        assert_eq!(
            put(&long, &[s("xs"), i(last + 2)], i(1)),
            Err("list_too_long".to_owned())
        );
    }

    // Sabotage: with check answering Ok(()) for an empty path, this test
    // fails.
    #[test]
    fn an_empty_path_is_not_assignable_and_a_refusal_writes_nothing() {
        assert_eq!(
            put(&Map::new(), &[], i(1)),
            Err("not_assignable".to_owned())
        );
        let bindings = map(&[("a", Value::Map(map(&[("b", i(1))])))]);
        assert_eq!(
            put(&bindings, &[s("a"), s("c"), s("d"), i(-1)], i(1)),
            Err("invalid_index".to_owned())
        );
        assert_eq!(
            check(&bindings, &path(&[s("a"), s("b"), s("c")]))
                .unwrap_err()
                .message(),
            "store cannot write into the integer at a.b"
        );
        assert_eq!(written(&path(&[s("xs"), i(2), s("k")])), "xs[2].k");
    }

    // Sabotage: with segments accepting a float as index 0, this test fails.
    #[test]
    fn a_segment_is_a_string_or_an_integer() {
        assert!(segments(&[s("a"), i(1)]).is_ok());
        for bad in [
            Value::from(1.0),
            Value::Null,
            Value::from(true),
            Value::List(vec![]),
        ] {
            assert!(segments(&[s("a"), bad.clone()]).is_err(), "{bad:?}");
        }
    }
}
