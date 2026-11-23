use super::*;
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// A scan's answer in the token transcript's own spelling: the tokens'
/// type, line, column and length (and a string token's quote and end), or
/// the refusal's message, position and span.
fn transcript(source: &str) -> Json {
    let at = |position: Position| json!({"line": position.line, "column": position.column});
    match tokenize(source) {
        Ok(tokens) => {
            let tokens: Vec<Json> = tokens
                .iter()
                .map(|token| {
                    let mut row = json!({
                        "type": token.kind.name(),
                        "line": token.line,
                        "column": token.column,
                        "length": token.length,
                    });
                    if let Kind::String { quote, end, .. } = &token.kind {
                        row["quote"] = json!(match quote {
                            Quote::Double => "double",
                            Quote::Single => "single",
                        });
                        row["end"] = at(*end);
                    }
                    row
                })
                .collect();
            json!({"kind": "tokens", "source": source, "tokens": tokens})
        }
        Err(refusal) => json!({
            "kind": "refusal",
            "source": source,
            "message": refusal.error.message(),
            "position": at(refusal.error.position().unwrap()),
            "span": {"start": at(refusal.span.start), "end": at(refusal.span.end)},
        }),
    }
}

fn kinds(source: &str) -> Vec<Kind> {
    tokenize(source)
        .unwrap()
        .into_iter()
        .map(|token| token.kind)
        .collect()
}

fn refusal(source: &str) -> (String, String, Position, Span) {
    let refusal = tokenize(source).unwrap_err();
    (
        refusal.error.reason().to_owned(),
        refusal.error.message().to_owned(),
        refusal.error.position().unwrap(),
        refusal.span,
    )
}

fn at(line: usize, column: usize) -> Position {
    Position { line, column }
}

fn conformance(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("conformance/transcript")
        .join(path)
}

// Every row of the vendored token transcript matches, or is listed in
// SUPERSEDED.json and matches the answer listed there (the reference's at
// the vendored tag).
// Sabotage: with read_datetime's year range check removed, this test fails
// on the superseded row (a datetime literal before year 0).
#[test]
fn every_transcript_row_matches_or_is_superseded() {
    let superseded: Json =
        serde_json::from_str(&std::fs::read_to_string(conformance("SUPERSEDED.json")).unwrap())
            .unwrap();
    let mut answers = BTreeMap::new();
    for row in superseded["rows"].as_array().unwrap() {
        if row["file"] == "tokens.json" {
            let mut answer = row["answer"].clone();
            answer["source"] = row["source"].clone();
            answers.insert(row["index"].as_u64().unwrap(), answer);
        }
    }
    let text = std::fs::read_to_string(conformance("tokens.json")).unwrap();
    let mut checked = 0;
    for (index, line) in (0..).zip(text.lines()) {
        let row: Json = serde_json::from_str(line).unwrap();
        let expected = answers.remove(&index).unwrap_or(row);
        let source = expected["source"].as_str().unwrap();
        assert_eq!(transcript(source), expected, "tokens.json row {index}");
        checked += 1;
    }
    assert_eq!(checked, 111);
    assert!(
        answers.is_empty(),
        "superseded rows past the end: {answers:?}"
    );
}

// Every source of the fixture below scans as the reference's lexer at the
// vendored tag scans it, run read-only over those sources: the duration
// cut and its cluster count, the date and datetime forms, the escapes, the
// dotted names, every punctuation spelling and every refusal shape. An
// integer outside i64, which the reference reads and this scanner refuses,
// is left to its own test below.
// Sabotage: with consumed counting the units alone, not the units joined
// to the cluster after them, this test fails on `1d`, a combining accent
// and `x`.
#[test]
fn scans_as_the_reference_does() {
    let mut count = 0;
    for line in REFERENCE.lines() {
        let expected: Json = serde_json::from_str(line).unwrap();
        let source = expected["source"].as_str().unwrap();
        assert_eq!(transcript(source), expected, "{source:?}");
        count += 1;
    }
    assert_eq!(count, 104);
}

// Sabotage: with the `\n` escape mapped to `n`, this test fails.
#[test]
fn a_string_holds_its_content_with_escapes_read() {
    assert_eq!(
        kinds(r#""a\nb\tc\rd\\e\"f\'g\qh" 'x"y'"#),
        vec![
            Kind::String {
                value: "a\nb\tc\rd\\e\"f'gqh".to_owned(),
                quote: Quote::Double,
                end: at(1, 25),
            },
            Kind::String {
                value: "x\"y".to_owned(),
                quote: Quote::Single,
                end: at(1, 31),
            },
            Kind::Eof,
        ]
    );
}

// Sabotage: with `Integer(integer)` emitted for a fractional number too,
// this test fails.
#[test]
fn numbers_carry_their_values() {
    assert_eq!(
        kinds("0 42 9223372036854775807 1.25 2.50h 007"),
        vec![
            Kind::Integer(0),
            Kind::Integer(42),
            Kind::Integer(i64::MAX),
            Kind::Float(1.25),
            Kind::FractionalNumber {
                whole: 2,
                fraction: "50".to_owned(),
            },
            Kind::DurationUnit("h"),
            Kind::Integer(7),
            Kind::Eof,
        ]
    );
    assert_eq!(
        kinds("1y2mo3w4d5h6m7s8ms"),
        vec![
            Kind::Integer(1),
            Kind::DurationUnit("y"),
            Kind::Integer(2),
            Kind::DurationUnit("mo"),
            Kind::Integer(3),
            Kind::DurationUnit("w"),
            Kind::Integer(4),
            Kind::DurationUnit("d"),
            Kind::Integer(5),
            Kind::DurationUnit("h"),
            Kind::Integer(6),
            Kind::DurationUnit("m"),
            Kind::Integer(7),
            Kind::DurationUnit("s"),
            Kind::Integer(8),
            Kind::DurationUnit("ms"),
            Kind::Eof,
        ]
    );
}

// An integer outside i64 is refused by name, spanning the number, where the
// reference reads an integer of any size.
// Sabotage: with the integer parsed as a u64 and cast to i64, this test
// fails.
#[test]
fn an_integer_outside_i64_is_refused_by_name() {
    assert_eq!(
        refusal("x > 9223372036854775808"),
        (
            "integer_out_of_range".to_owned(),
            "Integer literal 9223372036854775808 is outside the 64-bit integer range".to_owned(),
            at(1, 5),
            Span {
                start: at(1, 5),
                end: at(1, 24),
            },
        )
    );
    assert_eq!(refusal("99999999999999999999.5d").0, "integer_out_of_range");
    assert_eq!(refusal("99999999999999999999d").0, "integer_out_of_range");
}

// A decimal too large for a finite float is refused by name, where the
// reference raises.
// Sabotage: with the is_finite check dropped, this test fails.
#[test]
fn a_decimal_too_large_for_a_float_is_refused_by_name() {
    let digits = format!("1{}.5", "0".repeat(400));
    let (reason, message, position, span) = refusal(&format!("x {digits}"));
    assert_eq!(reason, "non_finite_number");
    assert_eq!(
        message,
        format!("Number literal {digits} is too large for a finite float")
    );
    assert_eq!(position, at(1, 3));
    assert_eq!(span.end, at(1, 3 + digits.len()));
    assert_eq!(refusal(&format!("{digits}d")).0, "non_finite_number");
}

// Sabotage: with the offset added rather than taken away, this test fails.
#[test]
fn dates_and_datetimes_carry_their_values() {
    let date = |year, month, day| Date::new(year, month, day).unwrap();
    let instant = |text: &str| DateTime::parse(text).unwrap();
    assert_eq!(
        kinds("#2024-01-15# #-0001-03-01# #+0042-12-31#"),
        vec![
            Kind::Date(date(2024, 1, 15)),
            Kind::Date(date(-1, 3, 1)),
            Kind::Date(date(42, 12, 31)),
            Kind::Eof,
        ]
    );
    assert_eq!(
        kinds(
            "#2024-01-15T10:00:00Z# #2024-01-15T10:00:00.1234567+05:30# \
             #2024-01-15T10:00:00-0200# #2024-01-15T10:00:00+-1:30# #2024-01-15T10:00:00-00#"
        ),
        vec![
            Kind::DateTime(instant("2024-01-15T10:00:00Z")),
            Kind::DateTime(instant("2024-01-15T04:30:00.123456Z")),
            Kind::DateTime(instant("2024-01-15T12:00:00Z")),
            Kind::DateTime(instant("2024-01-15T10:30:00Z")),
            Kind::DateTime(instant("2024-01-15T10:00:00Z")),
            Kind::Eof,
        ]
    );
}

// A datetime literal naming an instant outside the years 0 to 9999 is
// refused, as the reference at the vendored tag refuses it; a date literal
// keeps any year its digits write.
// Sabotage: with the range 0..=9999 widened to -9999..=9999, this test fails.
#[test]
fn a_datetime_outside_the_year_range_is_refused() {
    for body in [
        "-0001-01-01T00:00:00Z",
        "9999-12-31T23:00:00-01:00",
        "0000-01-01T00:30:00+01:00",
    ] {
        let source = format!("#{body}#");
        assert_eq!(
            refusal(&source),
            (
                "invalid_datetime".to_owned(),
                format!("Invalid datetime format: {body}"),
                at(1, 1),
                Span {
                    start: at(1, 1),
                    end: at(1, body.len() + 3),
                },
            )
        );
    }
    assert!(matches!(
        kinds("#0000-01-01T00:00:00Z# #9999-12-31T23:59:59.999999Z#").as_slice(),
        [Kind::DateTime(_), Kind::DateTime(_), Kind::Eof]
    ));
}

// Sabotage: with "is" added to the reserved words as an operator, this test
// fails.
#[test]
fn the_newer_operators_words_are_identifiers() {
    assert_eq!(
        kinds("x is not blank is present is defined a intersects b"),
        [
            "x",
            "is",
            "",
            "blank",
            "is",
            "present",
            "is",
            "defined",
            "a",
            "intersects",
            "b",
        ]
        .iter()
        .map(|word| match *word {
            "" => Kind::NotOp("not"),
            word => Kind::Identifier(word.to_owned()),
        })
        .chain([Kind::Eof])
        .collect::<Vec<_>>()
    );
}

// Sabotage: with the qualified name's text taken from one byte past its
// start, this test fails.
#[test]
fn names_and_calls_carry_their_text() {
    assert_eq!(
        kinds("f (x) Math.pow(a.b) not(x) AND y"),
        vec![
            Kind::FunctionName("f".to_owned()),
            Kind::Lparen,
            Kind::Identifier("x".to_owned()),
            Kind::Rparen,
            Kind::QualifiedFunctionName("Math.pow".to_owned()),
            Kind::Lparen,
            Kind::Identifier("a".to_owned()),
            Kind::Dot,
            Kind::Identifier("b".to_owned()),
            Kind::Rparen,
            Kind::NotOp("not"),
            Kind::Lparen,
            Kind::Identifier("x".to_owned()),
            Kind::Rparen,
            Kind::AndOp("AND"),
            Kind::Identifier("y".to_owned()),
            Kind::Eof,
        ]
    );
}

// Each refusal carries its reason.
// Sabotage: with the unterminated date's reason spelled "invalid_date",
// this test fails.
#[test]
fn each_refusal_carries_its_reason() {
    for (source, reason) in [
        ("a ~ b", "unexpected_character"),
        ("\"x", "unterminated_string"),
        ("'x\\", "unterminated_string"),
        ("\"\\u0041\"", "unsupported_escape"),
        ("#2024-01-15", "unterminated_date"),
        ("#2024-13-01#", "invalid_date"),
        ("#2024-01-01T25:00:00Z#", "invalid_datetime"),
    ] {
        assert_eq!(refusal(source).0, reason, "{source:?}");
    }
}

// A long line of duration units scans in time linear in its length (the
// cut and its count are taken once, not once per unit), and the first
// unit's column jump counts the dropped line: the end of input is two
// columns past the line's last character.
// Sabotage: with the dropped text left out of consumed, this test fails on
// the end of input's column.
#[test]
fn many_durations_on_one_long_line_scan() {
    let source = format!("{}0d\nx", "1d+".repeat(200_000));
    let tokens = tokenize(&source).unwrap();
    assert_eq!(tokens.len(), 600_003);
    let end = tokens.last().unwrap();
    assert_eq!(
        (end.kind.clone(), end.line, end.column),
        (Kind::Eof, 1, 600_005)
    );
}

// No input makes the scanner panic: every prefix (at a character boundary)
// of every source of the fixture below, which mixes every token shape and
// every refusal, scans to tokens or a refusal; and a long run of brackets
// scans without recursion.
// Sabotage: with the character after a backslash in a string read with
// `unwrap`, the prefix `"x\` panics and this test fails on its assertion.
#[test]
fn no_prefix_of_any_source_panics() {
    let mut panicked = Vec::new();
    for line in REFERENCE.lines() {
        let source = serde_json::from_str::<Json>(line).unwrap()["source"]
            .as_str()
            .unwrap()
            .to_owned();
        for (end, _) in source.char_indices().chain([(source.len(), ' ')]) {
            let prefix = source[..end].to_owned();
            if std::panic::catch_unwind(|| tokenize(&prefix)).is_err() {
                panicked.push(prefix);
            }
        }
    }
    assert_eq!(panicked, Vec::<String>::new());
    let deep = "(".repeat(100_000) + &"[".repeat(100_000);
    assert_eq!(tokenize(&deep).unwrap().len(), 200_001);
}

/// The reference lexer's answers at the vendored tag for the sources it
/// names, in the token transcript's spelling, one JSON object per line.
const REFERENCE: &str = r##"{"kind":"tokens","source":"5d x\ny","tokens":[{"column":1,"length":1,"line":1,"type":"integer"},{"column":2,"length":1,"line":1,"type":"duration_unit"},{"column":6,"length":1,"line":1,"type":"identifier"},{"column":7,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"5dh\nx","tokens":[{"column":1,"length":1,"line":1,"type":"integer"},{"column":2,"length":1,"line":1,"type":"duration_unit"},{"column":3,"length":1,"line":1,"type":"duration_unit"},{"column":6,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"3d4h\nfoo","tokens":[{"column":1,"length":1,"line":1,"type":"integer"},{"column":2,"length":1,"line":1,"type":"duration_unit"},{"column":7,"length":1,"line":1,"type":"integer"},{"column":8,"length":1,"line":1,"type":"duration_unit"},{"column":9,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"1d\nfoo\nbar","tokens":[{"column":1,"length":1,"line":1,"type":"integer"},{"column":2,"length":1,"line":1,"type":"duration_unit"},{"column":1,"length":3,"line":2,"type":"identifier"},{"column":4,"length":0,"line":2,"type":"eof"}]}
{"kind":"refusal","message":"Unexpected character '́'","position":{"column":2,"line":1},"source":"1d́x","span":{"end":{"column":3,"line":1},"start":{"column":2,"line":1}}}
{"kind":"tokens","source":"1d\r\nx","tokens":[{"column":1,"length":1,"line":1,"type":"integer"},{"column":2,"length":1,"line":1,"type":"duration_unit"},{"column":4,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"1d x\r\ny","tokens":[{"column":1,"length":1,"line":1,"type":"integer"},{"column":2,"length":1,"line":1,"type":"duration_unit"},{"column":5,"length":1,"line":1,"type":"identifier"},{"column":6,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"1d\r","tokens":[{"column":1,"length":1,"line":1,"type":"integer"},{"column":2,"length":1,"line":1,"type":"duration_unit"},{"column":3,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"1d\n","tokens":[{"column":1,"length":1,"line":1,"type":"integer"},{"column":2,"length":1,"line":1,"type":"duration_unit"},{"column":1,"length":0,"line":2,"type":"eof"}]}
{"kind":"tokens","source":"1day","tokens":[{"column":1,"length":1,"line":1,"type":"integer"},{"column":2,"length":1,"line":1,"type":"duration_unit"},{"column":3,"length":2,"line":1,"type":"identifier"},{"column":5,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"1dys","tokens":[{"column":1,"length":1,"line":1,"type":"integer"},{"column":2,"length":1,"line":1,"type":"duration_unit"},{"column":3,"length":1,"line":1,"type":"duration_unit"},{"column":4,"length":1,"line":1,"type":"duration_unit"},{"column":5,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"1mo","tokens":[{"column":1,"length":1,"line":1,"type":"integer"},{"column":2,"length":2,"line":1,"type":"duration_unit"},{"column":4,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"1mon","tokens":[{"column":1,"length":1,"line":1,"type":"integer"},{"column":2,"length":2,"line":1,"type":"duration_unit"},{"column":4,"length":1,"line":1,"type":"identifier"},{"column":5,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"1m5","tokens":[{"column":1,"length":1,"line":1,"type":"integer"},{"column":2,"length":1,"line":1,"type":"duration_unit"},{"column":3,"length":1,"line":1,"type":"integer"},{"column":4,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"1.5h","tokens":[{"column":1,"length":3,"line":1,"type":"fractional_number"},{"column":4,"length":1,"line":1,"type":"duration_unit"},{"column":5,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"1.5h\nx\ny","tokens":[{"column":1,"length":3,"line":1,"type":"fractional_number"},{"column":4,"length":1,"line":1,"type":"duration_unit"},{"column":1,"length":1,"line":2,"type":"identifier"},{"column":2,"length":0,"line":2,"type":"eof"}]}
{"kind":"tokens","source":"2.25mo3d","tokens":[{"column":1,"length":4,"line":1,"type":"fractional_number"},{"column":5,"length":2,"line":1,"type":"duration_unit"},{"column":7,"length":1,"line":1,"type":"integer"},{"column":8,"length":1,"line":1,"type":"duration_unit"},{"column":9,"length":0,"line":1,"type":"eof"}]}
{"kind":"refusal","message":"Unexpected character '😀'","position":{"column":7,"line":1},"source":"1d 😀\nzz","span":{"end":{"column":8,"line":1},"start":{"column":7,"line":1}}}
{"kind":"refusal","message":"Unexpected character '́'","position":{"column":8,"line":1},"source":"1h é\néé","span":{"end":{"column":9,"line":1},"start":{"column":8,"line":1}}}
{"kind":"tokens","source":"1d x\ny 2h z\nw","tokens":[{"column":1,"length":1,"line":1,"type":"integer"},{"column":2,"length":1,"line":1,"type":"duration_unit"},{"column":13,"length":1,"line":1,"type":"identifier"},{"column":14,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"#-0001-01-01#","tokens":[{"column":1,"length":13,"line":1,"type":"date"},{"column":14,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"#+2024-01-15#","tokens":[{"column":1,"length":13,"line":1,"type":"date"},{"column":14,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"#-0000-01-01#","tokens":[{"column":1,"length":13,"line":1,"type":"date"},{"column":14,"length":0,"line":1,"type":"eof"}]}
{"kind":"refusal","message":"Invalid date format: 20240115","position":{"column":1,"line":1},"source":"#20240115#","span":{"end":{"column":11,"line":1},"start":{"column":1,"line":1}}}
{"kind":"tokens","source":"#2024-01-15T10:00:00+-1:30#","tokens":[{"column":1,"length":27,"line":1,"type":"datetime"},{"column":28,"length":0,"line":1,"type":"eof"}]}
{"kind":"refusal","message":"Invalid datetime format: 2024-01-15T10:00:00-00:00","position":{"column":1,"line":1},"source":"#2024-01-15T10:00:00-00:00#","span":{"end":{"column":28,"line":1},"start":{"column":1,"line":1}}}
{"kind":"tokens","source":"#2024-01-15T10:00:00-0000#","tokens":[{"column":1,"length":26,"line":1,"type":"datetime"},{"column":27,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"#2024-01-15T10:00:00-00#","tokens":[{"column":1,"length":24,"line":1,"type":"datetime"},{"column":25,"length":0,"line":1,"type":"eof"}]}
{"kind":"refusal","message":"Invalid datetime format: 2024-01-15T10:00:00","position":{"column":1,"line":1},"source":"#2024-01-15T10:00:00#","span":{"end":{"column":22,"line":1},"start":{"column":1,"line":1}}}
{"kind":"refusal","message":"Invalid datetime format: 2024-01-15T10:00:00.Z","position":{"column":1,"line":1},"source":"#2024-01-15T10:00:00.Z#","span":{"end":{"column":24,"line":1},"start":{"column":1,"line":1}}}
{"kind":"tokens","source":"#2024-01-15T10:00:00,1234567Z#","tokens":[{"column":1,"length":30,"line":1,"type":"datetime"},{"column":31,"length":0,"line":1,"type":"eof"}]}
{"kind":"refusal","message":"Invalid datetime format: 9999-12-31T23:00:00-01:00","position":{"column":1,"line":1},"source":"#9999-12-31T23:00:00-01:00#","span":{"end":{"column":28,"line":1},"start":{"column":1,"line":1}}}
{"kind":"refusal","message":"Invalid datetime format: 0000-01-01T00:30:00+01:00","position":{"column":1,"line":1},"source":"#0000-01-01T00:30:00+01:00#","span":{"end":{"column":28,"line":1},"start":{"column":1,"line":1}}}
{"kind":"tokens","source":"#0000-01-01T00:00:00Z#","tokens":[{"column":1,"length":22,"line":1,"type":"datetime"},{"column":23,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"#9999-12-31T23:59:59.999999Z#","tokens":[{"column":1,"length":29,"line":1,"type":"datetime"},{"column":30,"length":0,"line":1,"type":"eof"}]}
{"kind":"refusal","message":"Invalid date format: 2024-01-15 10:00:00Z","position":{"column":1,"line":1},"source":"#2024-01-15 10:00:00Z#","span":{"end":{"column":23,"line":1},"start":{"column":1,"line":1}}}
{"kind":"refusal","message":"Invalid date format: 2024-02-30","position":{"column":1,"line":1},"source":"#2024-02-30#","span":{"end":{"column":13,"line":1},"start":{"column":1,"line":1}}}
{"kind":"refusal","message":"Invalid date format: 2023-02-29","position":{"column":1,"line":1},"source":"#2023-02-29#","span":{"end":{"column":13,"line":1},"start":{"column":1,"line":1}}}
{"kind":"refusal","message":"Invalid datetime format: 2024-01-15T24:00:00Z","position":{"column":1,"line":1},"source":"#2024-01-15T24:00:00Z#","span":{"end":{"column":23,"line":1},"start":{"column":1,"line":1}}}
{"kind":"refusal","message":"Invalid datetime format: 2024-01-15T10:00:60Z","position":{"column":1,"line":1},"source":"#2024-01-15T10:00:60Z#","span":{"end":{"column":23,"line":1},"start":{"column":1,"line":1}}}
{"kind":"refusal","message":"Invalid datetime format: 2024-01-15T10:00:00+24:00","position":{"column":1,"line":1},"source":"#2024-01-15T10:00:00+24:00#","span":{"end":{"column":28,"line":1},"start":{"column":1,"line":1}}}
{"kind":"refusal","message":"Invalid datetime format: 2024-01-15T10:00:00+05:3","position":{"column":1,"line":1},"source":"#2024-01-15T10:00:00+05:3#","span":{"end":{"column":27,"line":1},"start":{"column":1,"line":1}}}
{"kind":"refusal","message":"Invalid datetime format: 2024-01-15T10:00:00+05:30x","position":{"column":1,"line":1},"source":"#2024-01-15T10:00:00+05:30x#","span":{"end":{"column":29,"line":1},"start":{"column":1,"line":1}}}
{"kind":"tokens","source":"#2024-01-15T10:00:00+05#","tokens":[{"column":1,"length":24,"line":1,"type":"datetime"},{"column":25,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"#2024-01-15T10:00:00+0530#","tokens":[{"column":1,"length":26,"line":1,"type":"datetime"},{"column":27,"length":0,"line":1,"type":"eof"}]}
{"kind":"refusal","message":"Invalid datetime format: 2024-01-15 10:00:00ZT","position":{"column":1,"line":1},"source":"#2024-01-15 10:00:00ZT#","span":{"end":{"column":24,"line":1},"start":{"column":1,"line":1}}}
{"kind":"refusal","message":"Invalid date format:  2024-01-15","position":{"column":1,"line":1},"source":"# 2024-01-15#","span":{"end":{"column":14,"line":1},"start":{"column":1,"line":1}}}
{"kind":"refusal","message":"Invalid date format: 2024-01-1é","position":{"column":1,"line":1},"source":"#2024-01-1é#","span":{"end":{"column":13,"line":1},"start":{"column":1,"line":1}}}
{"kind":"tokens","source":"#2024-01-15T10:00:00Z# > x","tokens":[{"column":1,"length":22,"line":1,"type":"datetime"},{"column":24,"length":1,"line":1,"type":"gt"},{"column":26,"length":1,"line":1,"type":"identifier"},{"column":27,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"99999999999999999999.5","tokens":[{"column":1,"length":22,"line":1,"type":"float"},{"column":23,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"\"a\\\nb\" x","tokens":[{"column":1,"end":{"column":7,"line":1},"length":6,"line":1,"quote":"double","type":"string"},{"column":8,"length":1,"line":1,"type":"identifier"},{"column":9,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"\"a\\q\"","tokens":[{"column":1,"end":{"column":6,"line":1},"length":5,"line":1,"quote":"double","type":"string"},{"column":6,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"\"a\rb\" x","tokens":[{"column":1,"end":{"column":6,"line":1},"length":5,"line":1,"quote":"double","type":"string"},{"column":7,"length":1,"line":1,"type":"identifier"},{"column":8,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"\"é😀\" x","tokens":[{"column":1,"end":{"column":5,"line":1},"length":4,"line":1,"quote":"double","type":"string"},{"column":6,"length":1,"line":1,"type":"identifier"},{"column":7,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"'say \"x\"' y","tokens":[{"column":1,"end":{"column":10,"line":1},"length":9,"line":1,"quote":"single","type":"string"},{"column":11,"length":1,"line":1,"type":"identifier"},{"column":12,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"\"it's\"","tokens":[{"column":1,"end":{"column":7,"line":1},"length":6,"line":1,"quote":"double","type":"string"},{"column":7,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"a.b.c(1)","tokens":[{"column":1,"length":5,"line":1,"type":"qualified_function_name"},{"column":6,"length":1,"line":1,"type":"lparen"},{"column":7,"length":1,"line":1,"type":"integer"},{"column":8,"length":1,"line":1,"type":"rparen"},{"column":9,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"a.b.c + 1","tokens":[{"column":1,"length":1,"line":1,"type":"identifier"},{"column":2,"length":1,"line":1,"type":"dot"},{"column":3,"length":1,"line":1,"type":"identifier"},{"column":4,"length":1,"line":1,"type":"dot"},{"column":5,"length":1,"line":1,"type":"identifier"},{"column":7,"length":1,"line":1,"type":"plus"},{"column":9,"length":1,"line":1,"type":"integer"},{"column":10,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"a.b (1)","tokens":[{"column":1,"length":3,"line":1,"type":"qualified_function_name"},{"column":5,"length":1,"line":1,"type":"lparen"},{"column":6,"length":1,"line":1,"type":"integer"},{"column":7,"length":1,"line":1,"type":"rparen"},{"column":8,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"a .b(1)","tokens":[{"column":1,"length":1,"line":1,"type":"identifier"},{"column":3,"length":1,"line":1,"type":"dot"},{"column":4,"length":1,"line":1,"type":"function_name"},{"column":5,"length":1,"line":1,"type":"lparen"},{"column":6,"length":1,"line":1,"type":"integer"},{"column":7,"length":1,"line":1,"type":"rparen"},{"column":8,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"a.b.c.d(1)","tokens":[{"column":1,"length":7,"line":1,"type":"qualified_function_name"},{"column":8,"length":1,"line":1,"type":"lparen"},{"column":9,"length":1,"line":1,"type":"integer"},{"column":10,"length":1,"line":1,"type":"rparen"},{"column":11,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"a.b.1","tokens":[{"column":1,"length":1,"line":1,"type":"identifier"},{"column":2,"length":1,"line":1,"type":"dot"},{"column":3,"length":1,"line":1,"type":"identifier"},{"column":4,"length":1,"line":1,"type":"dot"},{"column":5,"length":1,"line":1,"type":"integer"},{"column":6,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"a.(","tokens":[{"column":1,"length":1,"line":1,"type":"identifier"},{"column":2,"length":1,"line":1,"type":"dot"},{"column":3,"length":1,"line":1,"type":"lparen"},{"column":4,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"x.y(","tokens":[{"column":1,"length":3,"line":1,"type":"qualified_function_name"},{"column":4,"length":1,"line":1,"type":"lparen"},{"column":5,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"true.x(1)","tokens":[{"column":1,"length":6,"line":1,"type":"qualified_function_name"},{"column":7,"length":1,"line":1,"type":"lparen"},{"column":8,"length":1,"line":1,"type":"integer"},{"column":9,"length":1,"line":1,"type":"rparen"},{"column":10,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"if (x)","tokens":[{"column":1,"length":2,"line":1,"type":"if_kw"},{"column":4,"length":1,"line":1,"type":"lparen"},{"column":5,"length":1,"line":1,"type":"identifier"},{"column":6,"length":1,"line":1,"type":"rparen"},{"column":7,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"not(x)","tokens":[{"column":1,"length":3,"line":1,"type":"not_op"},{"column":4,"length":1,"line":1,"type":"lparen"},{"column":5,"length":1,"line":1,"type":"identifier"},{"column":6,"length":1,"line":1,"type":"rparen"},{"column":7,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"f(x) g (y) h\n(z)","tokens":[{"column":1,"length":1,"line":1,"type":"function_name"},{"column":2,"length":1,"line":1,"type":"lparen"},{"column":3,"length":1,"line":1,"type":"identifier"},{"column":4,"length":1,"line":1,"type":"rparen"},{"column":6,"length":1,"line":1,"type":"function_name"},{"column":8,"length":1,"line":1,"type":"lparen"},{"column":9,"length":1,"line":1,"type":"identifier"},{"column":10,"length":1,"line":1,"type":"rparen"},{"column":12,"length":1,"line":1,"type":"function_name"},{"column":1,"length":1,"line":2,"type":"lparen"},{"column":2,"length":1,"line":2,"type":"identifier"},{"column":3,"length":1,"line":2,"type":"rparen"},{"column":4,"length":0,"line":2,"type":"eof"}]}
{"kind":"tokens","source":"is blank present defined intersects","tokens":[{"column":1,"length":2,"line":1,"type":"identifier"},{"column":4,"length":5,"line":1,"type":"identifier"},{"column":10,"length":7,"line":1,"type":"identifier"},{"column":18,"length":7,"line":1,"type":"identifier"},{"column":26,"length":10,"line":1,"type":"identifier"},{"column":36,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"x is not blank","tokens":[{"column":1,"length":1,"line":1,"type":"identifier"},{"column":3,"length":2,"line":1,"type":"identifier"},{"column":6,"length":3,"line":1,"type":"not_op"},{"column":10,"length":5,"line":1,"type":"identifier"},{"column":15,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"AND and OR or NOT not IN in CONTAINS contains","tokens":[{"column":1,"length":3,"line":1,"type":"and_op"},{"column":5,"length":3,"line":1,"type":"and_op"},{"column":9,"length":2,"line":1,"type":"or_op"},{"column":12,"length":2,"line":1,"type":"or_op"},{"column":15,"length":3,"line":1,"type":"not_op"},{"column":19,"length":3,"line":1,"type":"not_op"},{"column":23,"length":2,"line":1,"type":"in_op"},{"column":26,"length":2,"line":1,"type":"in_op"},{"column":29,"length":8,"line":1,"type":"contains_op"},{"column":38,"length":8,"line":1,"type":"contains_op"},{"column":46,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"if else while ago from now next last true false undefined null","tokens":[{"column":1,"length":2,"line":1,"type":"if_kw"},{"column":4,"length":4,"line":1,"type":"else_kw"},{"column":9,"length":5,"line":1,"type":"while_kw"},{"column":15,"length":3,"line":1,"type":"ago_op"},{"column":19,"length":4,"line":1,"type":"from_op"},{"column":24,"length":3,"line":1,"type":"now_op"},{"column":28,"length":4,"line":1,"type":"next_op"},{"column":33,"length":4,"line":1,"type":"last_op"},{"column":38,"length":4,"line":1,"type":"boolean"},{"column":43,"length":5,"line":1,"type":"boolean"},{"column":49,"length":9,"line":1,"type":"undefined"},{"column":59,"length":4,"line":1,"type":"null"},{"column":63,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"x\r\ny","tokens":[{"column":1,"length":1,"line":1,"type":"identifier"},{"column":1,"length":1,"line":2,"type":"identifier"},{"column":2,"length":0,"line":2,"type":"eof"}]}
{"kind":"refusal","message":"Unexpected character 'é'","position":{"column":1,"line":1},"source":"é","span":{"end":{"column":2,"line":1},"start":{"column":1,"line":1}}}
{"kind":"refusal","message":"Unexpected character ' '","position":{"column":1,"line":1},"source":" ","span":{"end":{"column":2,"line":1},"start":{"column":1,"line":1}}}
{"kind":"refusal","message":"Unexpected character '\f'","position":{"column":1,"line":1},"source":"\f","span":{"end":{"column":2,"line":1},"start":{"column":1,"line":1}}}
{"kind":"refusal","message":"Unexpected character '&'","position":{"column":3,"line":1},"source":"a & b","span":{"end":{"column":4,"line":1},"start":{"column":3,"line":1}}}
{"kind":"refusal","message":"Unexpected character '|'","position":{"column":3,"line":1},"source":"a | b","span":{"end":{"column":4,"line":1},"start":{"column":3,"line":1}}}
{"kind":"tokens","source":"1.","tokens":[{"column":1,"length":1,"line":1,"type":"integer"},{"column":2,"length":1,"line":1,"type":"dot"},{"column":3,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"1.2.3","tokens":[{"column":1,"length":3,"line":1,"type":"float"},{"column":4,"length":1,"line":1,"type":"dot"},{"column":5,"length":1,"line":1,"type":"integer"},{"column":6,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"x.1","tokens":[{"column":1,"length":1,"line":1,"type":"identifier"},{"column":2,"length":1,"line":1,"type":"dot"},{"column":3,"length":1,"line":1,"type":"integer"},{"column":4,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"f\n(1)","tokens":[{"column":1,"length":1,"line":1,"type":"function_name"},{"column":1,"length":1,"line":2,"type":"lparen"},{"column":2,"length":1,"line":2,"type":"integer"},{"column":3,"length":1,"line":2,"type":"rparen"},{"column":4,"length":0,"line":2,"type":"eof"}]}
{"kind":"tokens","source":"x::integer","tokens":[{"column":1,"length":1,"line":1,"type":"identifier"},{"column":2,"length":2,"line":1,"type":"double_colon"},{"column":4,"length":7,"line":1,"type":"identifier"},{"column":11,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"Foo.bar.baz","tokens":[{"column":1,"length":3,"line":1,"type":"identifier"},{"column":4,"length":1,"line":1,"type":"dot"},{"column":5,"length":3,"line":1,"type":"identifier"},{"column":8,"length":1,"line":1,"type":"dot"},{"column":9,"length":3,"line":1,"type":"identifier"},{"column":12,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"_a1.b_2 ( )","tokens":[{"column":1,"length":7,"line":1,"type":"qualified_function_name"},{"column":9,"length":1,"line":1,"type":"lparen"},{"column":11,"length":1,"line":1,"type":"rparen"},{"column":12,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"1msx","tokens":[{"column":1,"length":1,"line":1,"type":"integer"},{"column":2,"length":2,"line":1,"type":"duration_unit"},{"column":4,"length":1,"line":1,"type":"identifier"},{"column":5,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"1ms5","tokens":[{"column":1,"length":1,"line":1,"type":"integer"},{"column":2,"length":2,"line":1,"type":"duration_unit"},{"column":4,"length":1,"line":1,"type":"integer"},{"column":5,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":"1w","tokens":[{"column":1,"length":1,"line":1,"type":"integer"},{"column":2,"length":1,"line":1,"type":"duration_unit"},{"column":3,"length":0,"line":1,"type":"eof"}]}
{"kind":"refusal","message":"Unsupported escape sequence \\u in string literal: predicator has no numeric escape; write the character itself (string literals are UTF-8)","position":{"column":1,"line":1},"source":"\"\\u\"","span":{"end":{"column":2,"line":1},"start":{"column":1,"line":1}}}
{"kind":"refusal","message":"Unsupported escape sequence \\u in string literal: predicator has no numeric escape; write the character itself (string literals are UTF-8)","position":{"column":1,"line":1},"source":"\"\\U0041\"","span":{"end":{"column":2,"line":1},"start":{"column":1,"line":1}}}
{"kind":"refusal","message":"Unterminated double-quoted string literal","position":{"column":1,"line":1},"source":"\"x","span":{"end":{"column":2,"line":1},"start":{"column":1,"line":1}}}
{"kind":"refusal","message":"Unterminated single-quoted string literal","position":{"column":1,"line":1},"source":"'x","span":{"end":{"column":2,"line":1},"start":{"column":1,"line":1}}}
{"kind":"refusal","message":"Unterminated double-quoted string literal","position":{"column":1,"line":1},"source":"\"x\\","span":{"end":{"column":2,"line":1},"start":{"column":1,"line":1}}}
{"kind":"refusal","message":"Unterminated date literal","position":{"column":1,"line":1},"source":"#2024","span":{"end":{"column":6,"line":1},"start":{"column":1,"line":1}}}
{"kind":"refusal","message":"Unterminated date literal","position":{"column":1,"line":1},"source":"#2024\n-01","span":{"end":{"column":4,"line":2},"start":{"column":1,"line":1}}}
{"kind":"refusal","message":"Invalid date format: 2024-01-\n01","position":{"column":1,"line":1},"source":"#2024-01-\n01# > 5","span":{"end":{"column":4,"line":2},"start":{"column":1,"line":1}}}
{"kind":"tokens","source":"\"line1\nline2\" x","tokens":[{"column":1,"end":{"column":7,"line":2},"length":13,"line":1,"quote":"double","type":"string"},{"column":8,"length":1,"line":2,"type":"identifier"},{"column":9,"length":0,"line":2,"type":"eof"}]}
{"kind":"refusal","message":"Unexpected character '🇺'","position":{"column":6,"line":1},"source":"1d🇺🇸\nab","span":{"end":{"column":7,"line":1},"start":{"column":6,"line":1}}}
{"kind":"refusal","message":"Unexpected character 'क'","position":{"column":5,"line":1},"source":"1dक्ष\nx","span":{"end":{"column":6,"line":1},"start":{"column":5,"line":1}}}
{"kind":"refusal","message":"Unexpected character 'क'","position":{"column":3,"line":1},"source":"1dक्‍ष x","span":{"end":{"column":4,"line":1},"start":{"column":3,"line":1}}}
{"kind":"refusal","message":"Unexpected character '👨'","position":{"column":5,"line":1},"source":"1d👨‍👩\nq","span":{"end":{"column":6,"line":1},"start":{"column":5,"line":1}}}
{"kind":"tokens","source":"","tokens":[{"column":1,"length":0,"line":1,"type":"eof"}]}
{"kind":"tokens","source":" \t\n","tokens":[{"column":1,"length":0,"line":2,"type":"eof"}]}
{"kind":"tokens","source":"a===b!==c>=d<=e==f!=g&&h||i::j>k<l=m!n+o-p*q/r%s(t)[u]{v}w:x,y;z.","tokens":[{"column":1,"length":1,"line":1,"type":"identifier"},{"column":2,"length":3,"line":1,"type":"strict_equal"},{"column":5,"length":1,"line":1,"type":"identifier"},{"column":6,"length":3,"line":1,"type":"strict_ne"},{"column":9,"length":1,"line":1,"type":"identifier"},{"column":10,"length":2,"line":1,"type":"gte"},{"column":12,"length":1,"line":1,"type":"identifier"},{"column":13,"length":2,"line":1,"type":"lte"},{"column":15,"length":1,"line":1,"type":"identifier"},{"column":16,"length":2,"line":1,"type":"equal_equal"},{"column":18,"length":1,"line":1,"type":"identifier"},{"column":19,"length":2,"line":1,"type":"ne"},{"column":21,"length":1,"line":1,"type":"identifier"},{"column":22,"length":2,"line":1,"type":"and_and"},{"column":24,"length":1,"line":1,"type":"identifier"},{"column":25,"length":2,"line":1,"type":"or_or"},{"column":27,"length":1,"line":1,"type":"identifier"},{"column":28,"length":2,"line":1,"type":"double_colon"},{"column":30,"length":1,"line":1,"type":"identifier"},{"column":31,"length":1,"line":1,"type":"gt"},{"column":32,"length":1,"line":1,"type":"identifier"},{"column":33,"length":1,"line":1,"type":"lt"},{"column":34,"length":1,"line":1,"type":"identifier"},{"column":35,"length":1,"line":1,"type":"eq"},{"column":36,"length":1,"line":1,"type":"identifier"},{"column":37,"length":1,"line":1,"type":"bang"},{"column":38,"length":1,"line":1,"type":"identifier"},{"column":39,"length":1,"line":1,"type":"plus"},{"column":40,"length":1,"line":1,"type":"identifier"},{"column":41,"length":1,"line":1,"type":"minus"},{"column":42,"length":1,"line":1,"type":"identifier"},{"column":43,"length":1,"line":1,"type":"multiply"},{"column":44,"length":1,"line":1,"type":"identifier"},{"column":45,"length":1,"line":1,"type":"divide"},{"column":46,"length":1,"line":1,"type":"identifier"},{"column":47,"length":1,"line":1,"type":"modulo"},{"column":48,"length":1,"line":1,"type":"function_name"},{"column":49,"length":1,"line":1,"type":"lparen"},{"column":50,"length":1,"line":1,"type":"identifier"},{"column":51,"length":1,"line":1,"type":"rparen"},{"column":52,"length":1,"line":1,"type":"lbracket"},{"column":53,"length":1,"line":1,"type":"identifier"},{"column":54,"length":1,"line":1,"type":"rbracket"},{"column":55,"length":1,"line":1,"type":"lbrace"},{"column":56,"length":1,"line":1,"type":"identifier"},{"column":57,"length":1,"line":1,"type":"rbrace"},{"column":58,"length":1,"line":1,"type":"identifier"},{"column":59,"length":1,"line":1,"type":"colon"},{"column":60,"length":1,"line":1,"type":"identifier"},{"column":61,"length":1,"line":1,"type":"comma"},{"column":62,"length":1,"line":1,"type":"identifier"},{"column":63,"length":1,"line":1,"type":"semicolon"},{"column":64,"length":1,"line":1,"type":"identifier"},{"column":65,"length":1,"line":1,"type":"dot"},{"column":66,"length":0,"line":1,"type":"eof"}]}
"##;
