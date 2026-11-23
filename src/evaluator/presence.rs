//! The presence and overlap instructions, `presence` and `intersects`, as
//! the reference's instruction-set text states them. Neither ever fails on
//! a value: each answers a boolean, or undefined, for every operand.

use crate::Value;
use crate::compare::member_equal;

/// What a `presence` instruction asks of its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Presence {
    /// Whether the value is blank.
    Blank,
    /// Whether the value is anything but undefined.
    Defined,
}

impl Presence {
    /// The question `name` names, or `None` when it names none.
    pub(super) fn named(name: &str) -> Option<Presence> {
        match name {
            "blank" => Some(Presence::Blank),
            "defined" => Some(Presence::Defined),
            _ => None,
        }
    }
}

/// The code points a blank string may hold: the Unicode White_Space set,
/// as the reference's instruction-set text lists it code point by code
/// point: U+0009 through U+000D, U+0020, U+0085, U+00A0, U+1680, U+2000
/// through U+200A, U+2028, U+2029, U+202F, U+205F and U+3000. U+200B and
/// U+FEFF are not in it. A string is matched against this list, never
/// through the language's own trim, so the answer does not move with the
/// Unicode tables of the toolchain.
const WHITE_SPACE: &[char] = &[
    '\u{0009}', '\u{000A}', '\u{000B}', '\u{000C}', '\u{000D}', '\u{0020}', '\u{0085}', '\u{00A0}',
    '\u{1680}', '\u{2000}', '\u{2001}', '\u{2002}', '\u{2003}', '\u{2004}', '\u{2005}', '\u{2006}',
    '\u{2007}', '\u{2008}', '\u{2009}', '\u{200A}', '\u{2028}', '\u{2029}', '\u{202F}', '\u{205F}',
    '\u{3000}',
];

/// `presence`: the answer to `question` about `value`.
///
/// Blank is undefined, null, a string empty or holding only [`WHITE_SPACE`]
/// code points, an empty list and a map with no keys. Every other value is
/// not blank: every number, both booleans, a date, an instant, a duration
/// (a zero one too), and a list or a map with anything in it. Blank does
/// not look inside a member.
pub(super) fn presence(value: &Value, question: Presence) -> Value {
    Value::Bool(match question {
        Presence::Defined => !matches!(value, Value::Undefined),
        Presence::Blank => match value {
            Value::Undefined | Value::Null => true,
            Value::String(text) => text.chars().all(|point| WHITE_SPACE.contains(&point)),
            Value::List(members) => members.is_empty(),
            Value::Map(map) => map.is_empty(),
            _ => false,
        },
    })
}

/// `left intersects right`: whether some member of the left list equals
/// some member of the right one, under the equality `in` uses (an undefined
/// member equals nothing); false when either list is empty. Any operand
/// that is not a list, undefined included, answers undefined.
pub(super) fn intersects(left: &Value, right: &Value) -> Value {
    match (left, right) {
        (Value::List(left), Value::List(right)) => Value::Bool(
            left.iter()
                .any(|member| right.iter().any(|other| member_equal(member, other))),
        ),
        _ => Value::Undefined,
    }
}

#[cfg(test)]
mod tests {
    use super::WHITE_SPACE;
    use crate::{Context, Duration, Error, Instructions, Map, Options, Value, evaluate};

    fn run(text: &str, value: Value) -> Result<Value, Error> {
        let mut context = Context::new();
        context.insert("x", value);
        evaluate(
            &Instructions::from_json(text).unwrap(),
            &context,
            &Options::new(),
        )
    }

    /// `x is blank`, `x` bound to `value`.
    fn blank(value: Value) -> Value {
        run(r#"[["load","x"],["presence","blank"]]"#, value).unwrap()
    }

    fn reason(text: &str) -> String {
        run(text, Value::Null).unwrap_err().reason().to_owned()
    }

    // Sabotage: with U+3000 left out of WHITE_SPACE, this test fails.
    #[test]
    fn a_blank_string_holds_only_the_listed_code_points() {
        assert_eq!(blank(Value::from("\u{00A0}")), Value::from(true));
        assert_eq!(blank(Value::from("\u{3000}")), Value::from(true));
        assert_eq!(blank(Value::from("\u{200B}")), Value::from(false));
        assert_eq!(blank(Value::from("\u{FEFF}")), Value::from(false));
        assert_eq!(blank(Value::from("\u{180E}")), Value::from(false));
        assert_eq!(blank(Value::from(" \u{200B} ")), Value::from(false));
        assert_eq!(blank(Value::from(" x ")), Value::from(false));
        assert_eq!(WHITE_SPACE.len(), 25);
        let every: String = WHITE_SPACE.iter().collect();
        assert_eq!(blank(Value::from(every)), Value::from(true));
        for point in WHITE_SPACE {
            assert_eq!(
                blank(Value::from(point.to_string())),
                Value::from(true),
                "{point:?}"
            );
        }
    }

    // Sabotage: with a zero duration read as blank, this test fails.
    #[test]
    fn blank_and_defined_answer_every_kind_without_looking_inside() {
        let mut map = Map::new();
        map.insert("a", Value::Undefined);
        assert_eq!(blank(Value::Map(map)), Value::from(false));
        assert_eq!(blank(Value::Map(Map::new())), Value::from(true));
        assert_eq!(
            blank(Value::from(vec![Value::Undefined])),
            Value::from(false)
        );
        assert_eq!(
            blank(Value::Duration(Duration::default())),
            Value::from(false)
        );
        assert_eq!(blank(Value::from(0)), Value::from(false));
        let defined =
            |value: Value| run(r#"[["load","x"],["presence","defined"]]"#, value).unwrap();
        assert_eq!(defined(Value::Null), Value::from(true));
        assert_eq!(defined(Value::from("")), Value::from(true));
        assert_eq!(defined(Value::Undefined), Value::from(false));
    }

    // Sabotage: with intersects matching members by plain equality, under
    // which an integer never equals a float, this test fails.
    #[test]
    fn intersects_matches_members_as_in_does_and_takes_two_lists() {
        assert_eq!(
            run(
                r#"[["lit",[1,2]],["lit",[2.0]],["intersects"]]"#,
                Value::Null
            )
            .unwrap(),
            Value::from(true)
        );
        assert_eq!(
            run(r#"[["lit",[1]],["lit","a"],["intersects"]]"#, Value::Null).unwrap(),
            Value::Undefined
        );
        assert_eq!(
            run(r#"[["lit",[1]],["load","x"],["intersects"]]"#, Value::Null).unwrap(),
            Value::Undefined
        );
        assert_eq!(
            run(
                r#"[["lit",[[1]]],["lit",1],["make_list",1],["make_list",1],["intersects"]]"#,
                Value::Null
            )
            .unwrap(),
            Value::from(true)
        );
        assert_eq!(
            run(r#"[["lit",[[1]]],["lit",[1]],["intersects"]]"#, Value::Null).unwrap(),
            Value::from(false)
        );
    }

    // Sabotage: with presence reading any operand as blank, this test fails.
    #[test]
    fn presence_takes_blank_or_defined_and_one_value() {
        for operands in [
            r#""present""#,
            r#""Blank""#,
            "1",
            "",
            r#""blank","defined""#,
        ] {
            let comma = if operands.is_empty() { "" } else { "," };
            assert_eq!(
                reason(&format!(r#"[["lit",1],["presence"{comma}{operands}]]"#)),
                "unknown_instruction",
                "{operands}"
            );
        }
        assert_eq!(
            reason(r#"[["lit",1],["intersects",1]]"#),
            "unknown_instruction"
        );
        assert_eq!(reason(r#"[["presence","blank"]]"#), "insufficient_operands");
        assert_eq!(
            reason(r#"[["lit",[1]],["intersects"]]"#),
            "insufficient_operands"
        );
    }
}
