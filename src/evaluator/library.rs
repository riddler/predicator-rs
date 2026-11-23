//! The function library `call` runs: the reference's string, list, math,
//! date and JSON functions, by the name a program calls each one by.
//!
//! A function's arity is checked before its body runs (`call`). A body
//! that refuses its arguments answers the reference's own message, which
//! is the [`EvaluationError`]'s reason as well as its message, as the
//! reference gives it. Where the reference's code raises, or answers a
//! value this crate cannot hold, the body answers a named refusal instead:
//! `integer_overflow` for an integer outside `i64`, `non_finite_number` for
//! a float that is not finite, and the JSON reader's and writer's own
//! reasons.
//!
//! Strings are counted in grapheme clusters, as the reference counts them
//! for `len` and `substring`; `index_of` answers in the same unit
//! ([`cluster_index`]).

use crate::json::{self, Extension, ReferenceFault, Refusal};
use crate::value::integer_against_float;
use crate::{DateTime, Duration, EvaluationError, Key, Map, Value};
use core::cmp::Ordering;
use std::borrow::Cow;
use unicode_segmentation::UnicodeSegmentation;

/// What a library function may read from the run it is called in.
pub(super) trait Run {
    /// The run's current time, the clock read once per run.
    fn now(&mut self) -> Result<DateTime, EvaluationError>;

    /// 64 random bits from the run's random source.
    fn draw(&mut self) -> u64;
}

/// How many arguments a function takes.
#[derive(Debug, Clone, Copy)]
pub(super) enum Arity {
    /// Exactly this many.
    Exactly(usize),
    /// Either of these two.
    Either(usize, usize),
}

impl Arity {
    /// Whether `count` arguments are accepted.
    pub(super) fn admits(self, count: usize) -> bool {
        match self {
            Arity::Exactly(arity) => count == arity,
            Arity::Either(one, other) => count == one || count == other,
        }
    }

    /// The arity as the reference's refusal writes it: `1`, `2 or 3`.
    pub(super) fn describe(self) -> String {
        match self {
            Arity::Exactly(arity) => arity.to_string(),
            Arity::Either(one, other) => format!("{one} or {other}"),
        }
    }
}

/// A function's body: its arguments, already counted, and the run.
type Body = fn(&[Value], &mut dyn Run) -> Result<Value, EvaluationError>;

/// One library function.
pub(super) struct Function {
    pub(super) name: &'static str,
    pub(super) arity: Arity,
    pub(super) body: Body,
}

const fn function(name: &'static str, arity: Arity, body: Body) -> Function {
    Function { name, arity, body }
}

/// Every library function, by name.
const LIBRARY: &[Function] = &[
    function("len", Arity::Exactly(1), len),
    function("upper", Arity::Exactly(1), upper),
    function("lower", Arity::Exactly(1), lower),
    function("trim", Arity::Exactly(1), trim),
    function("starts_with", Arity::Exactly(2), starts_with),
    function("ends_with", Arity::Exactly(2), ends_with),
    function("substring", Arity::Either(2, 3), substring),
    function("index_of", Arity::Exactly(2), index_of),
    function("concat", Arity::Exactly(2), concat),
    function("Math.pow", Arity::Exactly(2), pow),
    function("Math.sqrt", Arity::Exactly(1), sqrt),
    function("Math.abs", Arity::Exactly(1), abs),
    function("Math.floor", Arity::Exactly(1), floor),
    function("Math.ceil", Arity::Exactly(1), ceil),
    function("Math.round", Arity::Exactly(1), round),
    function("Math.min", Arity::Exactly(2), min),
    function("Math.max", Arity::Exactly(2), max),
    function("Math.random", Arity::Exactly(0), random),
    function("Date.year", Arity::Exactly(1), year),
    function("Date.month", Arity::Exactly(1), month),
    function("Date.day", Arity::Exactly(1), day),
    function("Date.now", Arity::Exactly(0), now),
    function("JSON.stringify", Arity::Exactly(1), stringify),
    function("JSON.parse", Arity::Exactly(1), parse),
];

/// The library function called `name`, if there is one.
pub(super) fn find(name: &str) -> Option<&'static Function> {
    LIBRARY.iter().find(|function| function.name == name)
}

/// A refusal whose reason is its message, as the reference's library
/// refuses.
pub(super) fn refused(message: impl Into<String>) -> EvaluationError {
    let message = message.into();
    EvaluationError::new(message.clone(), message)
}

fn overflow(call: String) -> EvaluationError {
    EvaluationError::new(
        "integer_overflow",
        format!("{call} is outside the 64-bit integer range"),
    )
}

fn integer(count: usize) -> Value {
    Value::Integer(i64::try_from(count).unwrap_or(i64::MAX))
}

fn len(arguments: &[Value], _: &mut dyn Run) -> Result<Value, EvaluationError> {
    match arguments {
        [Value::String(text)] => Ok(integer(text.graphemes(true).count())),
        _ => Err(refused("len() expects a string argument")),
    }
}

/// Each character mapped on its own, with no context: a capital sigma
/// lowers to `σ` wherever it stands, as the reference lowers it.
fn upper(arguments: &[Value], _: &mut dyn Run) -> Result<Value, EvaluationError> {
    match arguments {
        [Value::String(text)] => Ok(Value::String(
            text.chars().flat_map(char::to_uppercase).collect(),
        )),
        _ => Err(refused("upper() expects a string argument")),
    }
}

fn lower(arguments: &[Value], _: &mut dyn Run) -> Result<Value, EvaluationError> {
    match arguments {
        [Value::String(text)] => Ok(Value::String(
            text.chars().flat_map(char::to_lowercase).collect(),
        )),
        _ => Err(refused("lower() expects a string argument")),
    }
}

/// Unicode whitespace, the set the reference trims.
fn trim(arguments: &[Value], _: &mut dyn Run) -> Result<Value, EvaluationError> {
    match arguments {
        [Value::String(text)] => Ok(Value::String(text.trim().to_owned())),
        _ => Err(refused("trim() expects a string argument")),
    }
}

fn starts_with(arguments: &[Value], _: &mut dyn Run) -> Result<Value, EvaluationError> {
    match arguments {
        [Value::String(text), Value::String(prefix)] => {
            Ok(Value::Bool(text.starts_with(prefix.as_str())))
        }
        _ => Err(refused("starts_with() expects two string arguments")),
    }
}

fn ends_with(arguments: &[Value], _: &mut dyn Run) -> Result<Value, EvaluationError> {
    match arguments {
        [Value::String(text), Value::String(suffix)] => {
            Ok(Value::Bool(text.ends_with(suffix.as_str())))
        }
        _ => Err(refused("ends_with() expects two string arguments")),
    }
}

/// The `length` grapheme clusters of `text` from the one at `start`, as
/// many as there are.
fn clusters(text: &str, start: i64, length: Option<i64>) -> Value {
    let start = usize::try_from(start).unwrap_or(usize::MAX);
    let length = length.map_or(usize::MAX, |length| {
        usize::try_from(length).unwrap_or(usize::MAX)
    });
    Value::String(text.graphemes(true).skip(start).take(length).collect())
}

fn substring(arguments: &[Value], _: &mut dyn Run) -> Result<Value, EvaluationError> {
    match arguments {
        [Value::String(text), Value::Integer(start)] if *start >= 0 => {
            Ok(clusters(text, *start, None))
        }
        [
            Value::String(text),
            Value::Integer(start),
            Value::Integer(length),
        ] if *start >= 0 && *length >= 0 => Ok(clusters(text, *start, Some(*length))),
        [Value::String(_), Value::Integer(_)] => {
            Err(refused("substring() expects a non-negative start index"))
        }
        [Value::String(_), Value::Integer(_), Value::Integer(_)] => Err(refused(
            "substring() expects a non-negative start index and length",
        )),
        [_, _] => Err(refused(
            "substring() expects a string and an integer start index",
        )),
        _ => Err(refused(
            "substring() expects a string, an integer start index, and an integer length",
        )),
    }
}

/// Where `part` first occurs in `text`, counted in grapheme clusters: the
/// index of the first cluster of the first occurrence that starts and ends
/// on a cluster boundary, so that it is a run of whole clusters (the one
/// `substring` gives back at that index), or `None` when there is none. The
/// empty `part` occurs at `0`.
///
/// This is the one place `index_of`'s unit is decided. The reference's code
/// answers a byte offset here while counting `len` and `substring` in
/// clusters; this crate counts all three in clusters.
fn cluster_index(text: &str, part: &str) -> Option<usize> {
    if part.is_empty() {
        return Some(0);
    }
    let boundaries: Vec<usize> = text
        .grapheme_indices(true)
        .map(|(offset, _)| offset)
        .chain([text.len()])
        .collect();
    let mut from = 0;
    while let Some(found) = text.get(from..).and_then(|rest| rest.find(part)) {
        let start = from.saturating_add(found);
        let end = start.saturating_add(part.len());
        if let (Ok(index), Ok(_)) = (
            boundaries.binary_search(&start),
            boundaries.binary_search(&end),
        ) {
            return Some(index);
        }
        // Past the first character of this occurrence, to the next.
        let step = text
            .get(start..)
            .and_then(|rest| rest.chars().next())
            .map_or(1, char::len_utf8);
        from = start.saturating_add(step);
    }
    None
}

fn index_of(arguments: &[Value], _: &mut dyn Run) -> Result<Value, EvaluationError> {
    match arguments {
        [Value::String(text), Value::String(part)] => Ok(match cluster_index(text, part) {
            Some(index) => integer(index),
            None => Value::Integer(-1),
        }),
        _ => Err(refused("index_of() expects two string arguments")),
    }
}

fn concat(arguments: &[Value], _: &mut dyn Run) -> Result<Value, EvaluationError> {
    match arguments {
        [Value::List(first), Value::List(second)] => {
            let mut joined = first.clone();
            joined.extend(second.iter().cloned());
            Ok(Value::List(joined))
        }
        _ => Err(refused("concat() expects two list arguments")),
    }
}

/// A number's value as a float, an integer rounded to the nearest float.
fn float(value: &Value) -> Option<f64> {
    match value {
        Value::Integer(number) => Some(*number as f64),
        Value::Float(number) => Some(*number),
        _ => None,
    }
}

/// A float answer, refused when it is not finite (where the reference's
/// code raises).
fn finite(call: &str, answer: f64) -> Result<Value, EvaluationError> {
    if answer.is_finite() {
        Ok(Value::Float(answer))
    } else {
        Err(EvaluationError::new(
            "non_finite_number",
            format!("{call} answers no finite float"),
        ))
    }
}

/// A whole float as an integer, refused outside `i64`.
fn whole(call: &str, number: f64) -> Result<Value, EvaluationError> {
    // 2^63, exactly representable as a float.
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    if (-LIMIT..LIMIT).contains(&number) {
        // In range and whole, so the cast is exact.
        Ok(Value::Integer(number as i64))
    } else {
        Err(overflow(call.to_owned()))
    }
}

/// `base` to the power `exponent`, exactly, or `None` outside `i64`.
fn integer_power(base: i64, exponent: i64) -> Option<i64> {
    match u32::try_from(exponent) {
        Ok(exponent) => base.checked_pow(exponent),
        // Only these bases stay in range past a `u32` exponent.
        Err(_) => match base {
            0 | 1 => Some(base),
            -1 => Some(if exponent % 2 == 0 { 1 } else { -1 }),
            _ => None,
        },
    }
}

fn pow(arguments: &[Value], _: &mut dyn Run) -> Result<Value, EvaluationError> {
    match arguments {
        [Value::Integer(base), Value::Integer(exponent)] if *exponent >= 0 => {
            integer_power(*base, *exponent)
                .map(Value::Integer)
                .ok_or_else(|| overflow(format!("Math.pow({base}, {exponent})")))
        }
        [base, exponent] => match (float(base), float(exponent)) {
            (Some(base), Some(exponent)) => finite("Math.pow", base.powf(exponent)),
            _ => Err(refused("Math.pow expects two numeric arguments")),
        },
        _ => Err(refused("Math.pow expects two numeric arguments")),
    }
}

fn sqrt(arguments: &[Value], _: &mut dyn Run) -> Result<Value, EvaluationError> {
    match arguments {
        [Value::Integer(number)] if *number >= 0 => {
            let unsigned = number.unsigned_abs();
            let root = unsigned.isqrt();
            if root.checked_mul(root) == Some(unsigned) {
                Ok(Value::Integer(i64::try_from(root).unwrap_or(i64::MAX)))
            } else {
                Ok(Value::Float((*number as f64).sqrt()))
            }
        }
        // Negative zero is not below zero.
        [Value::Float(number)] if *number >= 0.0 => Ok(Value::Float(number.sqrt())),
        [Value::Integer(_) | Value::Float(_)] => {
            Err(refused("Math.sqrt expects a non-negative number"))
        }
        _ => Err(refused("Math.sqrt expects a numeric argument")),
    }
}

fn abs(arguments: &[Value], _: &mut dyn Run) -> Result<Value, EvaluationError> {
    match arguments {
        [Value::Integer(number)] => number
            .checked_abs()
            .map(Value::Integer)
            .ok_or_else(|| overflow(format!("Math.abs({number})"))),
        [Value::Float(number)] => Ok(Value::Float(number.abs())),
        _ => Err(refused("Math.abs expects a numeric argument")),
    }
}

/// `Math.floor` and `Math.ceil`: the argument read as a float, as the
/// reference's code reads it, then rounded by `rounding` to an integer.
fn rounded(
    arguments: &[Value],
    name: &str,
    rounding: fn(f64) -> f64,
) -> Result<Value, EvaluationError> {
    match arguments {
        [value] => match float(value) {
            Some(number) => whole(name, rounding(number)),
            None => Err(refused(format!("{name} expects a numeric argument"))),
        },
        _ => Err(refused(format!("{name} expects a numeric argument"))),
    }
}

fn floor(arguments: &[Value], _: &mut dyn Run) -> Result<Value, EvaluationError> {
    rounded(arguments, "Math.floor", f64::floor)
}

fn ceil(arguments: &[Value], _: &mut dyn Run) -> Result<Value, EvaluationError> {
    rounded(arguments, "Math.ceil", f64::ceil)
}

/// Half away from zero; an integer is its own rounding.
fn round(arguments: &[Value], _: &mut dyn Run) -> Result<Value, EvaluationError> {
    match arguments {
        [Value::Integer(number)] => Ok(Value::Integer(*number)),
        [Value::Float(number)] => whole("Math.round", number.round()),
        _ => Err(refused("Math.round expects a numeric argument")),
    }
}

/// How two numbers stand, an integer against a float exactly; `None` when
/// either is not a number.
fn numeric_order(first: &Value, second: &Value) -> Option<Ordering> {
    match (first, second) {
        (Value::Integer(a), Value::Integer(b)) => Some(a.cmp(b)),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
        (Value::Integer(a), Value::Float(b)) => integer_against_float(*a, *b),
        (Value::Float(a), Value::Integer(b)) => {
            integer_against_float(*b, *a).map(Ordering::reverse)
        }
        _ => None,
    }
}

/// `Math.min` and `Math.max`: the second argument when it is strictly
/// `beyond` the first, else the first, so a tie answers the first argument,
/// its type unchanged.
fn pick(arguments: &[Value], name: &str, beyond: Ordering) -> Result<Value, EvaluationError> {
    match arguments {
        [first, second] => match numeric_order(second, first) {
            Some(order) if order == beyond => Ok(second.clone()),
            Some(_) => Ok(first.clone()),
            None => Err(refused(format!("{name} expects two numeric arguments"))),
        },
        _ => Err(refused(format!("{name} expects two numeric arguments"))),
    }
}

fn min(arguments: &[Value], _: &mut dyn Run) -> Result<Value, EvaluationError> {
    pick(arguments, "Math.min", Ordering::Less)
}

fn max(arguments: &[Value], _: &mut dyn Run) -> Result<Value, EvaluationError> {
    pick(arguments, "Math.max", Ordering::Greater)
}

/// A float in `[0, 1)` from 64 random bits: the top 53, over 2^53.
pub(super) fn unit_interval(bits: u64) -> f64 {
    const SCALE: f64 = 1.0 / 9_007_199_254_740_992.0;
    (bits >> 11) as f64 * SCALE
}

fn random(_: &[Value], run: &mut dyn Run) -> Result<Value, EvaluationError> {
    Ok(Value::Float(unit_interval(run.draw())))
}

/// `Date.year`, `Date.month` and `Date.day`: the part `part` reads from a
/// date or from an instant's date in UTC.
fn date_part(
    arguments: &[Value],
    name: &str,
    part: fn(&crate::Date) -> i64,
) -> Result<Value, EvaluationError> {
    match arguments {
        [Value::Date(date)] => Ok(Value::Integer(part(date))),
        [Value::DateTime(instant)] => Ok(Value::Integer(part(&instant.date()))),
        _ => Err(refused(format!(
            "{name}() expects a date or datetime argument"
        ))),
    }
}

fn year(arguments: &[Value], _: &mut dyn Run) -> Result<Value, EvaluationError> {
    date_part(arguments, "Date.year", |date| i64::from(date.year()))
}

fn month(arguments: &[Value], _: &mut dyn Run) -> Result<Value, EvaluationError> {
    date_part(arguments, "Date.month", |date| i64::from(date.month()))
}

fn day(arguments: &[Value], _: &mut dyn Run) -> Result<Value, EvaluationError> {
    date_part(arguments, "Date.day", |date| i64::from(date.day()))
}

fn now(_: &[Value], run: &mut dyn Run) -> Result<Value, EvaluationError> {
    run.now().map(Value::DateTime)
}

/// How `JSON.stringify` writes what plain JSON has no form for: undefined
/// as the string `"undefined"` and a date and an instant as their text, as
/// the reference's encoder writes them, and a duration as the map of its
/// eight units by name. A map's keys are written as text, sorted in the
/// order the reference's encoder writes a map of up to 32 keys in
/// (integers, then `false`, `true` and undefined, then strings); this crate
/// writes every map in that order, and a duration's units by name, where
/// the reference's order for a larger map and for a duration's units is
/// its runtime's own.
struct Stringify;

/// A key's place in the reference's order of map keys.
fn key_rank(key: &Key) -> (u8, i64) {
    match key {
        Key::Integer(number) => (0, *number),
        Key::Bool(false) => (1, 0),
        Key::Bool(true) => (2, 0),
        Key::Undefined => (3, 0),
        Key::String(_) => (4, 0),
    }
}

impl Extension for Stringify {
    fn write_other(&self, value: &Value, depth: usize, out: &mut String) -> Result<(), Refusal> {
        match value {
            Value::Date(date) => json::write_string(&date.to_string(), out),
            Value::DateTime(instant) => json::write_string(&instant.to_string(), out),
            Value::Duration(duration) => {
                json::depth_check(depth)?;
                out.push_str(&duration_text(duration));
            }
            _ => json::write_string("undefined", out),
        }
        Ok(())
    }

    fn check_map(&self, _: &Map) -> Result<(), Refusal> {
        Ok(())
    }

    fn entries<'m>(&self, map: &'m Map) -> Result<Vec<(Cow<'m, str>, &'m Value)>, Refusal> {
        let mut entries: Vec<(&Key, &Value)> = map.iter().collect();
        // Stable, so strings keep the map's own codepoint order.
        entries.sort_by_key(|(key, _)| key_rank(key));
        Ok(entries
            .into_iter()
            .map(|(key, value)| {
                let text = match key {
                    Key::String(text) => Cow::Borrowed(text.as_str()),
                    Key::Integer(number) => Cow::Owned(number.to_string()),
                    Key::Bool(flag) => Cow::Owned(flag.to_string()),
                    Key::Undefined => Cow::Borrowed("undefined"),
                };
                (text, value)
            })
            .collect())
    }
}

/// A duration as the map of its eight units, sorted by name.
fn duration_text(duration: &Duration) -> String {
    let mut units: Vec<(&str, i64)> = duration
        .units()
        .into_iter()
        .map(|(name, _, amount)| (name, amount))
        .collect();
    units.sort_unstable_by_key(|(name, _)| *name);
    let units: Vec<String> = units
        .into_iter()
        .map(|(name, amount)| format!("\"{name}\":{amount}"))
        .collect();
    format!("{{{}}}", units.join(","))
}

/// Never refuses what the reference writes; a value nested past the depth
/// limit is refused with the writer's reason, `depth_limit_exceeded`.
fn stringify(arguments: &[Value], _: &mut dyn Run) -> Result<Value, EvaluationError> {
    let [value] = arguments else {
        return Err(refused("JSON.stringify expects exactly 1 argument"));
    };
    json::write(value, &Stringify)
        .map(Value::String)
        .map_err(|refusal| EvaluationError::new(refusal.reason, refusal.message))
}

/// Plain JSON: every object a map, a `$type` key included. Where the
/// reference's reader names a byte or an early end, the text fails with its
/// message (`Invalid JSON: unexpected byte 0x6F at position 1`) as the
/// reason; every other fault fails with a named reason
/// ([`json::reference_fault`]).
fn parse(arguments: &[Value], _: &mut dyn Run) -> Result<Value, EvaluationError> {
    let [Value::String(text)] = arguments else {
        return Err(refused("JSON.parse expects a string argument"));
    };
    json::read(text, &|map| Ok(Value::Map(map))).map_err(|fault| {
        match json::reference_fault(text, &fault) {
            ReferenceFault::Text(fault) => refused(format!("Invalid JSON: {fault}")),
            ReferenceFault::Named { reason, message } => {
                EvaluationError::new(reason, format!("Invalid JSON: {message}"))
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Date;

    /// A run with a fixed time and a fixed draw.
    struct Fixed;

    impl Run for Fixed {
        fn now(&mut self) -> Result<DateTime, EvaluationError> {
            DateTime::from_unix(0, 0).ok_or_else(|| EvaluationError::new("x", "x"))
        }

        fn draw(&mut self) -> u64 {
            u64::MAX
        }
    }

    fn call(name: &str, arguments: &[Value]) -> Result<Value, EvaluationError> {
        let function = find(name).unwrap();
        assert!(function.arity.admits(arguments.len()), "{name}");
        (function.body)(arguments, &mut Fixed)
    }

    fn answer(name: &str, arguments: &[Value]) -> Value {
        call(name, arguments).unwrap()
    }

    fn reason(name: &str, arguments: &[Value]) -> String {
        call(name, arguments).unwrap_err().reason().to_owned()
    }

    fn text(text: &str) -> Value {
        Value::from(text)
    }

    // Sabotage: with len counting chars rather than grapheme clusters, this
    // test fails.
    #[test]
    fn strings_are_counted_and_cut_in_grapheme_clusters() {
        let accented = "e\u{301}xy";
        assert_eq!(answer("len", &[text(accented)]), Value::from(3));
        assert_eq!(answer("len", &[text("")]), Value::from(0));
        assert_eq!(
            answer("substring", &[text(accented), 1.into(), 2.into()]),
            text("xy")
        );
        assert_eq!(
            answer("substring", &[text(accented), 0.into(), 1.into()]),
            text("e\u{301}")
        );
        assert_eq!(
            answer("substring", &[text("hello world"), 6.into()]),
            text("world")
        );
        assert_eq!(
            answer("substring", &[text("hello"), 10.into(), 5.into()]),
            text("")
        );
        assert_eq!(
            answer(
                "substring",
                &[text("hello"), i64::MAX.into(), i64::MAX.into()]
            ),
            text("")
        );
    }

    // Sabotage: with cluster_index answering the byte offset of the first
    // occurrence, this test fails.
    #[test]
    fn index_of_counts_in_grapheme_clusters() {
        let at = |haystack: &str, needle: &str| answer("index_of", &[text(haystack), text(needle)]);
        assert_eq!(at("hello world", "world"), Value::from(6));
        assert_eq!(at("hello world", "nope"), Value::from(-1));
        assert_eq!(at("e\u{301}xy", "x"), Value::from(1));
        assert_eq!(at("", ""), Value::from(0));
        assert_eq!(at("abc", ""), Value::from(0));
        // An occurrence that starts or ends inside a cluster is not one.
        assert_eq!(at("e\u{301}", "e"), Value::from(-1));
        assert_eq!(at("e\u{301}", "\u{301}"), Value::from(-1));
        assert_eq!(at("e\u{301}xe", "e"), Value::from(2));
        assert_eq!(at("\u{1F468}\u{200D}\u{1F469}x", "x"), Value::from(1));
    }

    // Sabotage: with lower mapping the whole string with str::to_lowercase
    // (which lowers a final sigma to its final form), this test fails.
    #[test]
    fn case_maps_character_by_character() {
        assert_eq!(
            answer("upper", &[text("stra\u{df}e \u{fb01}")]),
            text("STRASSE FI")
        );
        assert_eq!(
            answer("lower", &[text("\u{3a3}\u{391}\u{3a3}")]),
            text("\u{3c3}\u{3b1}\u{3c3}")
        );
        assert_eq!(answer("lower", &[text("\u{130}")]), text("i\u{307}"));
    }

    // Sabotage: with trim removing only ASCII spaces, this test fails.
    #[test]
    fn trim_removes_unicode_whitespace_at_both_ends() {
        assert_eq!(
            answer("trim", &[text("\u{3000}\t a b\u{85}\u{a0}\n")]),
            text("a b")
        );
        assert_eq!(answer("trim", &[text("\u{200b}x")]), text("\u{200b}x"));
    }

    // Sabotage: with starts_with testing ends_with, this test fails.
    #[test]
    fn prefix_suffix_and_concat() {
        assert_eq!(
            answer("starts_with", &[text("abc"), text("ab")]),
            Value::from(true)
        );
        assert_eq!(
            answer("starts_with", &[text("abc"), text("bc")]),
            Value::from(false)
        );
        assert_eq!(
            answer("ends_with", &[text("abc"), text("bc")]),
            Value::from(true)
        );
        assert_eq!(
            answer("ends_with", &[text("abc"), text("ab")]),
            Value::from(false)
        );
        assert_eq!(
            answer(
                "concat",
                &[Value::from(vec![1.into()]), Value::from(vec![2.into()])]
            ),
            Value::from(vec![1.into(), 2.into()])
        );
    }

    // Sabotage: with the negative-start arm of substring answering the
    // type message, this test fails.
    #[test]
    fn string_and_list_functions_refuse_with_the_references_messages() {
        let cases: [(&str, Vec<Value>, &str); 12] = [
            ("len", vec![1.into()], "len() expects a string argument"),
            (
                "upper",
                vec![Value::Null],
                "upper() expects a string argument",
            ),
            (
                "lower",
                vec![Value::Undefined],
                "lower() expects a string argument",
            ),
            ("trim", vec![1.into()], "trim() expects a string argument"),
            (
                "starts_with",
                vec![text("a"), 1.into()],
                "starts_with() expects two string arguments",
            ),
            (
                "ends_with",
                vec![1.into(), text("a")],
                "ends_with() expects two string arguments",
            ),
            (
                "substring",
                vec![text("a"), (-1).into()],
                "substring() expects a non-negative start index",
            ),
            (
                "substring",
                vec![text("a"), 0.into(), (-1).into()],
                "substring() expects a non-negative start index and length",
            ),
            (
                "substring",
                vec![text("a"), 1.5.into()],
                "substring() expects a string and an integer start index",
            ),
            (
                "substring",
                vec![1.into(), 0.into(), 1.into()],
                "substring() expects a string, an integer start index, and an integer length",
            ),
            (
                "index_of",
                vec![1.into(), text("")],
                "index_of() expects two string arguments",
            ),
            (
                "concat",
                vec![Value::from(vec![]), text("a")],
                "concat() expects two list arguments",
            ),
        ];
        for (name, arguments, message) in cases {
            let error = call(name, &arguments).unwrap_err();
            assert_eq!(
                (error.reason(), error.message()),
                (message, message),
                "{name}"
            );
        }
    }

    // Sabotage: with integer_power computing through a float, this test
    // fails.
    #[test]
    fn pow_is_exact_for_integers_and_a_float_otherwise() {
        assert_eq!(
            answer("Math.pow", &[3.into(), 39.into()]),
            Value::from(4_052_555_153_018_976_267_i64)
        );
        assert_eq!(answer("Math.pow", &[0.into(), 0.into()]), Value::from(1));
        assert_eq!(
            answer("Math.pow", &[(-1).into(), 5_000_000_001_i64.into()]),
            Value::from(-1)
        );
        assert_eq!(
            answer("Math.pow", &[(-1).into(), 5_000_000_000_i64.into()]),
            Value::from(1)
        );
        assert_eq!(
            answer("Math.pow", &[1.into(), i64::MAX.into()]),
            Value::from(1)
        );
        assert_eq!(
            answer("Math.pow", &[0.into(), i64::MAX.into()]),
            Value::from(0)
        );
        assert_eq!(
            answer("Math.pow", &[2.into(), 0.5.into()]),
            Value::from(2.0_f64.sqrt())
        );
        assert_eq!(
            answer("Math.pow", &[4.0.into(), 2.into()]),
            Value::from(16.0)
        );
        assert_eq!(
            reason("Math.pow", &[2.into(), 63.into()]),
            "integer_overflow"
        );
        assert_eq!(
            reason("Math.pow", &[2.into(), 5_000_000_000_i64.into()]),
            "integer_overflow"
        );
        assert_eq!(
            reason("Math.pow", &[0.into(), (-1).into()]),
            "non_finite_number"
        );
        assert_eq!(
            reason("Math.pow", &[(-8).into(), 0.5.into()]),
            "non_finite_number"
        );
        assert_eq!(
            reason("Math.pow", &[10.0.into(), 400.into()]),
            "non_finite_number"
        );
        assert_eq!(
            reason("Math.pow", &[text("2"), 1.into()]),
            "Math.pow expects two numeric arguments"
        );
    }

    // Sabotage: with the exact-root test in sqrt dropped (always a float),
    // this test fails.
    #[test]
    fn sqrt_is_an_integer_for_an_exact_root_and_refuses_a_negative() {
        assert_eq!(answer("Math.sqrt", &[16.into()]), Value::from(4));
        assert_eq!(answer("Math.sqrt", &[0.into()]), Value::from(0));
        assert_eq!(
            answer("Math.sqrt", &[(3_037_000_499_i64 * 3_037_000_499).into()]),
            Value::from(3_037_000_499_i64)
        );
        assert_eq!(
            answer("Math.sqrt", &[i64::MAX.into()]),
            Value::from((i64::MAX as f64).sqrt())
        );
        assert_eq!(answer("Math.sqrt", &[16.0.into()]), Value::from(4.0));
        assert_eq!(answer("Math.sqrt", &[(-0.0).into()]), Value::from(-0.0));
        assert_eq!(
            reason("Math.sqrt", &[(-1).into()]),
            "Math.sqrt expects a non-negative number"
        );
        assert_eq!(
            reason("Math.sqrt", &[(-0.5).into()]),
            "Math.sqrt expects a non-negative number"
        );
        assert_eq!(
            reason("Math.sqrt", &[text("4")]),
            "Math.sqrt expects a numeric argument"
        );
    }

    // Sabotage: with round rounding half to even, this test fails.
    #[test]
    fn abs_floor_ceil_and_round() {
        assert_eq!(answer("Math.abs", &[(-7).into()]), Value::from(7));
        assert_eq!(answer("Math.abs", &[(-7.5).into()]), Value::from(7.5));
        assert_eq!(reason("Math.abs", &[i64::MIN.into()]), "integer_overflow");
        assert_eq!(answer("Math.floor", &[(-2.5).into()]), Value::from(-3));
        assert_eq!(answer("Math.floor", &[7.into()]), Value::from(7));
        assert_eq!(answer("Math.ceil", &[2.1.into()]), Value::from(3));
        assert_eq!(answer("Math.ceil", &[(-2.9).into()]), Value::from(-2));
        // An integer is read as a float first, as the reference reads it.
        assert_eq!(
            answer("Math.floor", &[9_007_199_254_740_993_i64.into()]),
            Value::from(9_007_199_254_740_992_i64)
        );
        assert_eq!(reason("Math.floor", &[i64::MAX.into()]), "integer_overflow");
        assert_eq!(reason("Math.ceil", &[1e19.into()]), "integer_overflow");
        assert_eq!(
            answer("Math.floor", &[(i64::MIN as f64).into()]),
            Value::from(i64::MIN)
        );
        assert_eq!(answer("Math.round", &[2.5.into()]), Value::from(3));
        assert_eq!(answer("Math.round", &[(-2.5).into()]), Value::from(-3));
        assert_eq!(answer("Math.round", &[0.49.into()]), Value::from(0));
        assert_eq!(
            answer("Math.round", &[i64::MAX.into()]),
            Value::from(i64::MAX)
        );
        assert_eq!(reason("Math.round", &[1e300.into()]), "integer_overflow");
        for name in ["Math.abs", "Math.floor", "Math.ceil", "Math.round"] {
            assert_eq!(
                reason(name, &[Value::Null]),
                format!("{name} expects a numeric argument")
            );
        }
    }

    // Sabotage: with pick answering the second argument on a tie, this test
    // fails.
    #[test]
    fn min_and_max_answer_the_winning_argument_and_the_first_on_a_tie() {
        assert_eq!(answer("Math.min", &[3.into(), 7.into()]), Value::from(3));
        assert_eq!(
            answer("Math.min", &[7.into(), 3.5.into()]),
            Value::from(3.5)
        );
        assert_eq!(answer("Math.max", &[2.into(), 3.into()]), Value::from(3));
        assert_eq!(
            answer("Math.max", &[3.5.into(), 3.into()]),
            Value::from(3.5)
        );
        assert_eq!(answer("Math.min", &[1.into(), 1.0.into()]), Value::from(1));
        assert_eq!(
            answer("Math.min", &[1.0.into(), 1.into()]),
            Value::from(1.0)
        );
        assert_eq!(answer("Math.max", &[1.into(), 1.0.into()]), Value::from(1));
        assert_eq!(
            answer("Math.max", &[1.0.into(), 1.into()]),
            Value::from(1.0)
        );
        assert_eq!(
            reason("Math.min", &[1.into(), text("2")]),
            "Math.min expects two numeric arguments"
        );
        assert_eq!(
            reason("Math.max", &[Value::Null, 2.into()]),
            "Math.max expects two numeric arguments"
        );
    }

    // Sabotage: with unit_interval keeping the low 53 bits instead of the
    // top 53, this test fails.
    #[test]
    fn a_draw_is_a_float_in_the_unit_interval() {
        assert_eq!(unit_interval(0), 0.0);
        assert_eq!(unit_interval(1 << 63), 0.5);
        assert_eq!(unit_interval(u64::MAX), 1.0 - 1.0 / 9_007_199_254_740_992.0);
        assert!(unit_interval(u64::MAX) < 1.0);
        assert_eq!(
            answer("Math.random", &[]),
            Value::from(unit_interval(u64::MAX))
        );
    }

    // Sabotage: with date_part reading a date's month for Date.day, this
    // test fails.
    #[test]
    fn date_parts_read_a_date_or_an_instants_date() {
        let date = Value::from(Date::new(2024, 3, 15).unwrap());
        let instant =
            Value::from(DateTime::new(Date::new(1999, 12, 31).unwrap(), 23, 0, 0, 0).unwrap());
        assert_eq!(
            answer("Date.year", std::slice::from_ref(&date)),
            Value::from(2024)
        );
        assert_eq!(
            answer("Date.month", std::slice::from_ref(&date)),
            Value::from(3)
        );
        assert_eq!(answer("Date.day", &[date]), Value::from(15));
        assert_eq!(
            answer("Date.year", std::slice::from_ref(&instant)),
            Value::from(1999)
        );
        assert_eq!(
            answer("Date.month", std::slice::from_ref(&instant)),
            Value::from(12)
        );
        assert_eq!(answer("Date.day", &[instant]), Value::from(31));
        for name in ["Date.year", "Date.month", "Date.day"] {
            assert_eq!(
                reason(name, &[text("2024-03-15")]),
                format!("{name}() expects a date or datetime argument")
            );
        }
        assert_eq!(
            answer("Date.now", &[]),
            Value::from(DateTime::from_unix(0, 0).unwrap())
        );
    }

    // Sabotage: with Stringify::entries keeping the map's own key order,
    // this test fails.
    #[test]
    fn stringify_writes_what_the_references_encoder_writes() {
        let stringify = |value: Value| answer("JSON.stringify", &[value]);
        let mut map = Map::new();
        map.insert("b", Value::Undefined);
        map.insert("a", Value::from(Date::new(2024, 3, 15).unwrap()));
        map.insert(true, 1.into());
        map.insert(false, 2.into());
        map.insert(Key::Undefined, 3.into());
        map.insert(10, 4.into());
        map.insert(-2, 5.into());
        assert_eq!(
            stringify(Value::from(map)),
            text(
                r#"{"-2":5,"10":4,"false":2,"true":1,"undefined":3,"a":"2024-03-15","b":"undefined"}"#
            )
        );
        let instant = DateTime::new(Date::new(2024, 3, 15).unwrap(), 12, 0, 0, 500_000).unwrap();
        assert_eq!(
            stringify(Value::from(instant)),
            text(r#""2024-03-15T12:00:00.500000Z""#)
        );
        let duration = Duration {
            days: 1,
            ..Duration::default()
        };
        assert_eq!(
            stringify(Value::from(duration)),
            text(
                r#"{"days":1,"hours":0,"milliseconds":0,"minutes":0,"months":0,"seconds":0,"weeks":0,"years":0}"#
            )
        );
        assert_eq!(stringify(Value::from(1.0e20)), text("1.0e20"));
        assert_eq!(stringify(text("\u{e9}\u{0}")), text("\"\u{e9}\\u0000\""));
        let mut tagged = Map::new();
        tagged.insert("$type", text("date"));
        assert_eq!(stringify(Value::from(tagged)), text(r#"{"$type":"date"}"#));
        let mut deep = Value::Null;
        for _ in 0..101 {
            deep = Value::from(vec![deep]);
        }
        assert_eq!(reason("JSON.stringify", &[deep]), "depth_limit_exceeded");
    }

    // Sabotage: with reference_fault naming a literal's fault where this
    // reader stopped, this test fails.
    #[test]
    fn parse_reports_the_references_fault_text() {
        assert_eq!(
            answer("JSON.parse", &[text("[1,2,3]")]),
            Value::from(vec![1.into(), 2.into(), 3.into()])
        );
        let mut tagged = Map::new();
        tagged.insert("$type", text("undefined"));
        assert_eq!(
            answer("JSON.parse", &[text(r#"{"$type":"undefined"}"#)]),
            Value::from(tagged)
        );
        // Each answer is the reference's own, read from its reader.
        let pinned = [
            ("not json", "unexpected byte 0x6F at position 1"),
            ("", "unexpected end of input at position 0"),
            (" ", "unexpected end of input at position 1"),
            ("[1,", "unexpected end of input at position 3"),
            ("{\"a\"", "unexpected end of input at position 4"),
            ("[1] x", "unexpected byte 0x20 at position 3"),
            ("[1]  x", "unexpected byte 0x20 at position 3"),
            ("1 x", "unexpected byte 0x20 at position 1"),
            ("{\"a\":1}  }", "unexpected byte 0x20 at position 7"),
            ("[1 x]", "unexpected byte 0x78 at position 3"),
            ("[1  x]", "unexpected byte 0x78 at position 4"),
            ("{\"a\" x}", "unexpected byte 0x78 at position 5"),
            ("{\"a\":1 x}", "unexpected byte 0x78 at position 7"),
            ("[1,  x]", "unexpected byte 0x78 at position 5"),
            (" x", "unexpected byte 0x78 at position 1"),
            ("  ]", "unexpected byte 0x5D at position 2"),
            ("nul", "unexpected end of input at position 3"),
            ("nulx", "unexpected end of input at position 4"),
            ("nuxl", "unexpected end of input at position 4"),
            ("nuxl ", "unexpected byte 0x75 at position 1"),
            ("trux1", "unexpected byte 0x72 at position 1"),
            ("[nul]", "unexpected end of input at position 5"),
            ("[tx]", "unexpected end of input at position 4"),
            ("falsy", "unexpected end of input at position 5"),
            ("\"abc", "unexpected end of input at position 4"),
            ("\"a\\u12", "unexpected end of input at position 6"),
            ("{1:2}", "unexpected byte 0x31 at position 1"),
            ("{\"a\":}", "unexpected byte 0x7D at position 5"),
            ("[,]", "unexpected byte 0x2C at position 1"),
            ("[1,]", "unexpected byte 0x5D at position 3"),
            ("{,}", "unexpected byte 0x2C at position 1"),
            ("-x", "unexpected byte 0x78 at position 1"),
            ("1.x", "unexpected byte 0x78 at position 2"),
            ("1e", "unexpected end of input at position 2"),
            ("1ex", "unexpected byte 0x78 at position 2"),
            ("1e+", "unexpected end of input at position 3"),
            ("\t\n x", "unexpected byte 0x78 at position 3"),
            ("-", "unexpected end of input at position 1"),
            ("01", "unexpected byte 0x31 at position 1"),
            ("1.", "unexpected end of input at position 2"),
            ("\"\\x\"", "unexpected byte 0x78 at position 2"),
            ("\"a\u{1}\"", "unexpected byte 0x01 at position 2"),
            ("[]]", "unexpected byte 0x5D at position 2"),
            ("{}x", "unexpected byte 0x78 at position 2"),
            ("x", "unexpected byte 0x78 at position 0"),
            ("\"\\ud800\"", "unexpected end of input at position 8"),
            ("\"\\ud800x\"", "unexpected end of input at position 9"),
            ("\"\\ud800xxxxxxxx\"", "unexpected byte 0x78 at position 7"),
            ("\"\\ud800abcdefgh\"", "unexpected byte 0x61 at position 7"),
            ("[\"\\ud800\", 1]", "unexpected end of input at position 13"),
            ("\"ab\\ud800\"", "unexpected end of input at position 10"),
            ("\"\\ud800\\u\"", "unexpected end of input at position 10"),
            ("\"\\ud800\\n\"", "unexpected end of input at position 10"),
            ("\"\\u00\"", "unexpected end of input at position 6"),
        ];
        for (input, fault) in pinned {
            let message = format!("Invalid JSON: {fault}");
            let error = call("JSON.parse", &[text(input)]).unwrap_err();
            assert_eq!(
                (error.reason(), error.message()),
                (message.as_str(), message.as_str()),
                "{input:?}"
            );
        }
        assert_eq!(
            reason("JSON.parse", &[text("9223372036854775808")]),
            "integer_out_of_range"
        );
        assert_eq!(reason("JSON.parse", &[text("1e400")]), "non_finite_number");
        assert_eq!(
            reason("JSON.parse", &[text(&"[".repeat(101))]),
            "depth_limit_exceeded"
        );
        // Where the reference answers its runtime's own error term, the
        // fault is refused by name.
        for input in [
            "\"\\u12G4\"",
            "\"a\\u12G4\"",
            "\"\\udc00\"",
            "\"\\ud800\\u0041\"",
            "\"\\ud800\\uZZZZ\"",
            "\"\\ud800\\ud800\"",
        ] {
            assert_eq!(
                reason("JSON.parse", &[text(input)]),
                "invalid_unicode_escape",
                "{input:?}"
            );
        }
        assert_eq!(
            call("JSON.parse", &[text("\"a\\u12G4\"")])
                .unwrap_err()
                .message(),
            "Invalid JSON: the escape at position 2 names no character"
        );
        assert_eq!(
            answer("JSON.parse", &[text("\"\\ud800\\udc00\"")]),
            text("\u{10000}")
        );
        assert_eq!(
            reason("JSON.parse", &[1.into()]),
            "JSON.parse expects a string argument"
        );
    }

    // Sabotage: with Arity::Either admitting only its first count, this
    // test fails.
    #[test]
    fn arities_admit_and_describe_their_counts() {
        assert!(Arity::Either(2, 3).admits(2));
        assert!(Arity::Either(2, 3).admits(3));
        assert!(!Arity::Either(2, 3).admits(1));
        assert!(Arity::Exactly(0).admits(0));
        assert!(!Arity::Exactly(0).admits(1));
        assert_eq!(Arity::Either(2, 3).describe(), "2 or 3");
        assert_eq!(Arity::Exactly(1).describe(), "1");
        assert!(find("nope").is_none());
        // Each name is the library's once.
        for function in LIBRARY {
            let named = LIBRARY.iter().filter(|other| other.name == function.name);
            assert_eq!(named.count(), 1, "{}", function.name);
        }
    }
}
