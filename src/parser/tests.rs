use super::tree::{
    Arithmetic, Block, Comparison, Direction, Entry, Expression, KeyStyle, Literal, Membership,
    Node, Presence, Program, Statement, Unary,
};
use super::*;
use crate::json::float_text;
use crate::lexer::{Quote, tokenize};
use serde_json::{Map as JsonMap, Value as Json, json};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn at(position: Position) -> Json {
    json!([position.line, position.column])
}

fn over(span: Span) -> Json {
    json!([at(span.start), at(span.end)])
}

/// A node in the fixture's shape: its tag, position and span, and the
/// fields the tag carries.
fn shaped(tag: &str, position: Position, span: Span, fields: Json) -> Json {
    let mut object = JsonMap::new();
    object.insert("node".to_owned(), json!(tag));
    object.insert("at".to_owned(), at(position));
    object.insert("span".to_owned(), over(span));
    if let Json::Object(fields) = fields {
        object.extend(fields);
    }
    Json::Object(object)
}

fn literal(value: &Literal) -> String {
    match value {
        Literal::Integer(value) => format!("i:{value}"),
        Literal::Float(value) => format!("f:{}", float_text(*value)),
        Literal::Boolean(value) => format!("b:{value}"),
        Literal::Undefined => "undefined".to_owned(),
        Literal::Null => "null".to_owned(),
        Literal::Date(date) => format!("d:{date}"),
        Literal::DateTime(instant, precision) => format!(
            "dt:{}:{precision}",
            i128::from(instant.unix_seconds()) * 1_000_000 + i128::from(instant.microsecond())
        ),
    }
}

fn quote(quote: Quote) -> &'static str {
    match quote {
        Quote::Double => "double",
        Quote::Single => "single",
    }
}

fn comparison(operator: Comparison) -> &'static str {
    match operator {
        Comparison::Gt => "gt",
        Comparison::Lt => "lt",
        Comparison::Gte => "gte",
        Comparison::Lte => "lte",
        Comparison::EqualEqual => "equal_equal",
        Comparison::Ne => "ne",
        Comparison::StrictEq => "strict_eq",
        Comparison::StrictNe => "strict_ne",
    }
}

fn arithmetic(operator: Arithmetic) -> &'static str {
    match operator {
        Arithmetic::Add => "add",
        Arithmetic::Subtract => "subtract",
        Arithmetic::Multiply => "multiply",
        Arithmetic::Divide => "divide",
        Arithmetic::Modulo => "modulo",
    }
}

fn membership(operator: Membership) -> &'static str {
    match operator {
        Membership::In => "in",
        Membership::Contains => "contains",
        Membership::Intersects => "intersects",
    }
}

fn direction(direction: Direction) -> &'static str {
    match direction {
        Direction::Ago => "ago",
        Direction::Future => "future",
        Direction::Next => "next",
        Direction::Last => "last",
    }
}

fn entry(entry: &Entry) -> Json {
    let style = match entry.style {
        KeyStyle::Bare => "identifier",
        KeyStyle::Quoted(quoted) => quote(quoted),
    };
    json!({
        "key": entry.key,
        "style": style,
        "at": at(entry.position),
        "span": over(entry.span),
        "value": node(&entry.value),
    })
}

/// An expression node in the fixture's shape.
fn node(node: &Node) -> Json {
    let (tag, fields) = match &node.kind {
        Expression::Literal(value) => ("literal", json!({"value": literal(value)})),
        Expression::String { value, quote: q } => {
            ("string", json!({"value": value, "quote": quote(*q)}))
        }
        Expression::Identifier(name) => ("identifier", json!({"name": name})),
        Expression::Comparison {
            operator,
            left,
            right,
        } => (
            "comparison",
            json!({"op": comparison(*operator), "left": self::node(left), "right": self::node(right)}),
        ),
        Expression::Arithmetic {
            operator,
            left,
            right,
        } => (
            "arithmetic",
            json!({"op": arithmetic(*operator), "left": self::node(left), "right": self::node(right)}),
        ),
        Expression::Membership {
            operator,
            left,
            right,
        } => (
            "membership",
            json!({"op": membership(*operator), "left": self::node(left), "right": self::node(right)}),
        ),
        Expression::Presence {
            kind,
            negated,
            operand,
        } => {
            let kind = match kind {
                Presence::Blank => "blank",
                Presence::Present => "present",
                Presence::Defined => "defined",
            };
            (
                "presence",
                json!({"kind": kind, "negated": negated, "operand": self::node(operand)}),
            )
        }
        Expression::Unary { operator, operand } => {
            let operator = match operator {
                Unary::Minus => "minus",
                Unary::Bang => "bang",
            };
            (
                "unary",
                json!({"op": operator, "operand": self::node(operand)}),
            )
        }
        Expression::And { left, right } => (
            "and",
            json!({"left": self::node(left), "right": self::node(right)}),
        ),
        Expression::Or { left, right } => (
            "or",
            json!({"left": self::node(left), "right": self::node(right)}),
        ),
        Expression::Not { operand } => ("not", json!({"operand": self::node(operand)})),
        Expression::List(elements) => (
            "list",
            json!({"elements": elements.iter().map(self::node).collect::<Vec<_>>()}),
        ),
        Expression::Object(entries) => (
            "object",
            json!({"entries": entries.iter().map(entry).collect::<Vec<_>>()}),
        ),
        Expression::Call { name, arguments } => (
            "call",
            json!({"name": name, "arguments": arguments.iter().map(self::node).collect::<Vec<_>>()}),
        ),
        Expression::Index { object, key } => (
            "index",
            json!({"object": self::node(object), "key": self::node(key)}),
        ),
        Expression::Property { object, name } => (
            "property",
            json!({"object": self::node(object), "name": name}),
        ),
        Expression::Cast { operand, type_name } => (
            "cast",
            json!({"operand": self::node(operand), "type": type_name}),
        ),
        Expression::Duration(units) => ("duration", json!({"units": units})),
        Expression::RelativeDate {
            duration,
            direction: way,
        } => (
            "relative_date",
            json!({"duration": self::node(duration), "direction": direction(*way)}),
        ),
    };
    shaped(tag, node.position, node.span, fields)
}

fn block(block: &Block) -> Json {
    shaped(
        "block",
        block.position,
        block.span,
        json!({"statements": block.statements.iter().map(statement).collect::<Vec<_>>()}),
    )
}

fn statement(statement: &Statement) -> Json {
    match statement {
        Statement::Assignment {
            target,
            value,
            position,
            span,
        } => shaped(
            "assignment",
            *position,
            *span,
            json!({"target": node(target), "value": node(value)}),
        ),
        Statement::If {
            condition,
            then_block,
            else_block,
            position,
            span,
        } => shaped(
            "if",
            *position,
            *span,
            json!({
                "condition": node(condition),
                "then": block(then_block),
                "else": else_block.as_ref().map(block),
            }),
        ),
        Statement::While {
            condition,
            body,
            position,
            span,
        } => shaped(
            "while",
            *position,
            *span,
            json!({"condition": node(condition), "body": block(body)}),
        ),
        Statement::Expression(expression) => node(expression),
    }
}

fn program(program: &Program) -> Json {
    shaped(
        "program",
        program.position,
        program.span,
        json!({"statements": program.statements.iter().map(statement).collect::<Vec<_>>()}),
    )
}

fn refusal(refusal: &Refusal) -> Json {
    json!({
        "error": refusal.error.message(),
        "at": at(refusal.error.position().unwrap()),
        "span": over(refusal.span),
    })
}

/// What the parser answers for a source, in the fixture's shape: the
/// expression parse and the program parse, or the lexer's refusal.
fn answer(source: &str) -> Json {
    match tokenize(source) {
        Ok(tokens) => json!({
            "source": source,
            "expression": parse(&tokens).as_ref().map_or_else(refusal, node),
            "program": parse_program(&tokens).as_ref().map_or_else(refusal, program),
        }),
        Err(error) => json!({"source": source, "lex": refusal(&error)}),
    }
}

/// The reason, message, position and span of the refusal a source parses
/// to, as an expression or as a program.
fn refused(source: &str, as_program: bool) -> (String, String, Position, Span) {
    let tokens = tokenize(source).unwrap();
    let error = if as_program {
        parse_program(&tokens).unwrap_err()
    } else {
        parse(&tokens).unwrap_err()
    };
    (
        error.error.reason().to_owned(),
        error.error.message().to_owned(),
        error.error.position().unwrap(),
        error.span,
    )
}

fn reason(source: &str, as_program: bool) -> String {
    refused(source, as_program).0
}

fn expression(source: &str) -> Node {
    parse(&tokenize(source).unwrap()).unwrap()
}

fn point(line: usize, column: usize) -> Position {
    Position { line, column }
}

fn manifest_path(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn lines(path: &str) -> Vec<Json> {
    std::fs::read_to_string(manifest_path(path))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

// Every source of tests/fixtures/parser-reference.jsonl parses as the
// reference's parser at the vendored tag parses it, run read-only over those
// sources: the tree with each node's position and span, or the refusal's
// message, position and span, as an expression and as a program. The
// sources cover every production, every refusal and the newer operators.
// Sabotage: with `+` and `-` given the level of `*`, this test fails on
// `1 + 2 * 3`.
#[test]
fn parses_as_the_reference_does() {
    let rows = lines("tests/fixtures/parser-reference.jsonl");
    let mut differing = Vec::new();
    for expected in &rows {
        let actual = answer(expected["source"].as_str().unwrap());
        if &actual != expected {
            differing.push(format!("expected {expected}\n  actual {actual}"));
        }
    }
    assert!(differing.is_empty(), "{}", differing.join("\n"));
    assert_eq!(rows.len(), 397);
}

// Every row of the vendored compile transcript that is a parse refusal
// answers the row's message, position and span (and its reason, where the
// row names one), or the answer SUPERSEDED.json lists for it; every row
// that compiles a source parses it.
// Sabotage: with the trailing-token refusal after a statement given the
// expression's wording, this test fails on the missing-separator row.
#[test]
fn every_compile_transcript_refusal_matches() {
    let superseded: Json = serde_json::from_str(
        &std::fs::read_to_string(manifest_path("conformance/transcript/SUPERSEDED.json")).unwrap(),
    )
    .unwrap();
    let mut answers = BTreeMap::new();
    for row in superseded["rows"].as_array().unwrap() {
        if row["file"] == "compile.json" {
            answers.insert(row["index"].as_u64().unwrap(), row["answer"].clone());
        }
    }
    let rows = lines("conformance/transcript/compile.json");
    let mut counts = BTreeMap::new();
    for (index, row) in (0..).zip(&rows) {
        let kind = row["kind"].as_str().unwrap();
        let source = row["source"].as_str().unwrap();
        *counts.entry(kind).or_insert(0) += 1;
        let as_program = kind.starts_with("program");
        if kind == "refusal" || kind == "program_refusal" {
            let expected = answers.remove(&index).unwrap_or_else(|| row.clone());
            let actual = match tokenize(source) {
                Ok(tokens) if as_program => parse_program(&tokens).unwrap_err(),
                Ok(tokens) => parse(&tokens).unwrap_err(),
                Err(error) => error,
            };
            let mut answer = json!({
                "message": actual.error.message(),
                "position": {"line": actual.span.start.line, "column": actual.span.start.column},
                "span": {
                    "start": {"line": actual.span.start.line, "column": actual.span.start.column},
                    "end": {"line": actual.span.end.line, "column": actual.span.end.column},
                },
            });
            assert_eq!(
                actual.error.position(),
                Some(actual.span.start),
                "row {index}"
            );
            if expected.get("reason").is_some() {
                answer["reason"] = json!(actual.error.reason());
            }
            for key in ["message", "position", "span", "reason"] {
                assert_eq!(answer.get(key), expected.get(key), "row {index} {key}");
            }
        } else {
            let tokens = tokenize(source).unwrap();
            if as_program {
                assert!(parse_program(&tokens).is_ok(), "row {index}");
            } else {
                assert!(parse(&tokens).is_ok(), "row {index}");
            }
        }
    }
    assert!(answers.is_empty(), "superseded rows not found: {answers:?}");
    assert_eq!(
        counts,
        BTreeMap::from([
            ("compile", 63),
            ("decompile", 45),
            ("program", 35),
            ("program_refusal", 29),
            ("refusal", 27),
        ])
    );
}

// Every source in the vendored corpus, every tier, parses as an expression.
// Sabotage: with the lower-case `intersects` left out of the relations,
// this test fails on the tier 10 overlap cases.
#[test]
fn every_corpus_source_parses() {
    let mut parsed = 0;
    for tier in 1..=10 {
        for case in lines(&format!("conformance/corpus/tier-{tier}.json")) {
            let Some(source) = case["source"].as_str() else {
                continue;
            };
            let tokens = tokenize(source).unwrap();
            if let Err(error) = parse(&tokens) {
                panic!("{}: {}", case["id"], error.error.message());
            }
            parsed += 1;
        }
    }
    assert!(parsed > 300, "{parsed}");
}

// The newer operators sit at the precedence of `in`: `NOT`, `AND` and `OR`
// bind looser, arithmetic tighter, and each word stays a name elsewhere.
// Sabotage: with the capital `IS` left out of the presence words, this
// test fails on `x IS NOT BLANK`.
#[test]
fn the_newer_operators_take_the_precedence_of_in() {
    let tag = |source: &str| node(&expression(source))["node"].clone();
    assert_eq!(tag("not x is blank"), "not");
    assert_eq!(tag("x is blank AND y is present"), "and");
    assert_eq!(tag("x is not blank OR y is not defined"), "or");
    assert_eq!(tag("(x + 1) is defined"), "presence");
    assert_eq!(tag("x + 1 intersects y"), "membership");
    let tree = node(&expression("x IS NOT BLANK"));
    assert_eq!(
        (tree["kind"].clone(), tree["negated"].clone()),
        (json!("blank"), json!(true))
    );
    let tree = node(&expression("is is blank AND blank is present"));
    assert_eq!(tree["left"]["operand"]["name"], "is");
    assert_eq!(tree["right"]["operand"]["name"], "blank");
    assert_eq!(reason("x is blank is blank", false), "trailing_token");
    assert_eq!(
        reason("x intersects y intersects z", false),
        "trailing_token"
    );
    assert_eq!(reason("x == y is blank", false), "trailing_token");
}

// Each refusal carries its reason.
// Sabotage: with the unassignable-location refusal given the reason
// `assignment_in_expression`, this test fails.
#[test]
fn each_refusal_carries_its_reason() {
    let cases = [
        ("()", false, "expected_primary"),
        ("x y", false, "trailing_token"),
        ("x = 1", false, "assignment_in_expression"),
        ("if", false, "statement_keyword"),
        ("(x]", false, "expected_close_paren"),
        ("f(x]", false, "expected_close_paren"),
        ("[x)", false, "expected_close_bracket"),
        ("x[0)", false, "expected_close_bracket"),
        ("{a: 1)", false, "expected_close_brace"),
        ("{1: 2}", false, "expected_object_key"),
        ("{a 1}", false, "expected_object_colon"),
        ("a.1", false, "expected_property_name"),
        ("a::1", false, "expected_type_name"),
        ("a::list", false, "unknown_cast_type"),
        ("1d from x", false, "expected_now"),
        ("next x", false, "expected_duration"),
        ("0.5ms", false, "duration_fraction"),
        ("0.000000000001s", false, "duration_fraction"),
        ("1.5d2h", false, "duration_unit_twice"),
        ("x = 1 y = 2", true, "trailing_token"),
        ("else { }", true, "unexpected_else"),
        ("x = 1 else { }", true, "unexpected_else"),
        ("1 = x", true, "unassignable_location"),
        ("if x y", true, "expected_open_brace"),
        ("if x { y", true, "expected_close_brace"),
    ];
    for (source, as_program, expected) in cases {
        assert_eq!(reason(source, as_program), expected, "{source}");
    }
}

// A fraction is expanded into whole units as the reference expands it.
// Sabotage: with the whole part kept even when it is zero, this test fails
// on `0.5d`.
#[test]
fn a_fraction_expands_into_whole_units() {
    let units = |source: &str| node(&expression(source))["units"].clone();
    assert_eq!(units("1.5h"), json!([[1, "h"], [30, "m"]]));
    assert_eq!(units("0.5d"), json!([[12, "h"]]));
    assert_eq!(units("0.0d"), json!([[0, "d"]]));
    assert_eq!(units("2.50mo"), json!([[2, "mo"], [15, "d"]]));
    assert_eq!(units("0.001s"), json!([[1, "ms"]]));
    assert_eq!(units("1.25y"), json!([[1, "y"], [91, "d"], [6, "h"]]));
    assert_eq!(units("0.5s1s"), json!([[500, "ms"], [1, "s"]]));
    assert_eq!(units("3d3d"), json!([[3, "d"], [3, "d"]]));
}

/// A comparison whose right side nests a prefix minus and a parenthesis
/// per repetition, two levels each, behind a `!` that stays open over it
/// when the count needs one more level.
fn mixed(n: usize) -> String {
    let (bang, repeats) = if (n - 1) % 2 == 0 {
        ("", (n - 1) / 2)
    } else {
        ("!", (n - 2) / 2)
    };
    format!(
        "{bang}x == {}x{}",
        "-(".repeat(repeats),
        ")".repeat(repeats)
    )
}

/// Runs `check` on a thread whose stack is 256 KiB.
fn on_a_small_stack(check: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(check)
        .unwrap()
        .join()
        .unwrap();
}

/// The nesting shapes, each built `n` levels deep (the whole expression or
/// statement counting as the first), with whether it is a program.
fn shapes(n: usize) -> Vec<(String, bool)> {
    let inner = n - 1;
    let wrap = |open: &str, close: &str| format!("{}x{}", open.repeat(inner), close.repeat(inner));
    vec![
        (wrap("(", ")"), false),
        (wrap("[", "]"), false),
        (wrap("f(", ")"), false),
        (wrap("a[", "]"), false),
        (wrap("{k: ", "}"), false),
        (wrap("-", ""), false),
        (wrap("!", ""), false),
        (wrap("NOT ", ""), false),
        (mixed(n), false),
        (
            format!("{}next 1d{}", "(".repeat(n - 2), ")".repeat(n - 2)),
            false,
        ),
        (
            format!("{}y{}", "if x { ".repeat(n - 1), " }".repeat(n - 1)),
            true,
        ),
        (
            format!("{}if x {{ }}", "if x { } else ".repeat(n - 1)),
            true,
        ),
        (format!("y = {}", wrap("(", ")")), true),
    ]
}

// A source nesting exactly DEPTH_LIMIT levels parses, in every nesting
// shape, on a 256 KiB stack, and one level deeper is refused with the
// reason nesting_depth_exceeded at the token the parse stopped on.
// Sabotage: with `enter` checking `<` rather than `<=` against the limit,
// this test fails on the deepest accepted parenthesis.
#[test]
fn the_deepest_accepted_source_parses_on_a_256_kib_stack() {
    on_a_small_stack(|| {
        for (source, as_program) in shapes(DEPTH_LIMIT) {
            let tokens = tokenize(&source).unwrap();
            let parsed = if as_program {
                parse_program(&tokens).map(drop)
            } else {
                parse(&tokens).map(drop)
            };
            assert!(parsed.is_ok(), "{source:.40}");
        }
        for (source, as_program) in shapes(DEPTH_LIMIT + 1) {
            let (reason, message, ..) = refused(&source, as_program);
            assert_eq!(reason, "nesting_depth_exceeded", "{source:.40}");
            assert_eq!(
                message,
                "Expression nests past the depth limit of 400 levels, \
                 the whole expression counting as the first"
            );
        }
    });
    let (_, _, position, span) = refused(&format!("{}x", "(".repeat(DEPTH_LIMIT)), false);
    assert_eq!(position, point(1, DEPTH_LIMIT + 1));
    assert_eq!(span.end, point(1, DEPTH_LIMIT + 2));
}

// No source overflows the stack: a chain as long as the source costs no
// depth, parses and drops on a 256 KiB stack, and a source nested far past
// the limit is refused there.
// Sabotage: with `enter` never refusing, this test fails on 200 000 open
// parentheses.
#[test]
fn no_source_overflows_the_stack() {
    on_a_small_stack(|| {
        let chain = format!("x{}", " + x".repeat(100_000));
        assert!(parse(&tokenize(&chain).unwrap()).is_ok());
        let postfix = format!("x{}", ".y[0]::string".repeat(30_000));
        assert!(parse(&tokenize(&postfix).unwrap()).is_ok());
        let statements = "x = x + 1;".repeat(20_000);
        assert!(parse_program(&tokenize(&statements).unwrap()).is_ok());
        let deep = "(".repeat(200_000);
        assert_eq!(reason(&deep, false), "nesting_depth_exceeded");
        assert_eq!(reason(&deep, true), "nesting_depth_exceeded");
        let deep = "if x { ".repeat(200_000);
        assert_eq!(reason(&deep, true), "nesting_depth_exceeded");
    });
}

// No prefix of any fixture source makes the parser panic: each parses or
// is refused, as an expression and as a program.
// Sabotage: with the presence word read by an unchecked index two past
// `is`, the prefix `x is` panics and this test fails on its assertion.
#[test]
fn no_prefix_of_any_source_panics() {
    let mut panicked = Vec::new();
    for row in lines("tests/fixtures/parser-reference.jsonl") {
        let source = row["source"].as_str().unwrap().to_owned();
        for (end, _) in source.char_indices().chain([(source.len(), ' ')]) {
            let prefix = source[..end].to_owned();
            let Ok(tokens) = tokenize(&prefix) else {
                continue;
            };
            let outcome = std::panic::catch_unwind(|| {
                let _ = parse(&tokens);
                let _ = parse_program(&tokens);
            });
            if outcome.is_err() {
                panicked.push(prefix);
            }
        }
    }
    assert_eq!(panicked, Vec::<String>::new());
}

// Token streams the lexer never writes are still answered with a refusal:
// no tokens at all, a function's name with no parenthesis after it, and a
// number with a point and no unit after it.
// Sabotage: with the empty stream read from a token made up at line 0,
// this test fails on the position.
#[test]
fn streams_the_lexer_never_writes_are_refused() {
    let token = |kind, column| crate::lexer::Token {
        kind,
        line: 1,
        column,
        length: 1,
    };
    let refusal = parse(&[]).unwrap_err();
    assert_eq!(refusal.error.reason(), "expected_primary");
    assert_eq!(refusal.error.position(), Some(point(1, 1)));
    assert!(parse_program(&[]).is_err());
    let stream = [
        token(Kind::FunctionName("f".to_owned()), 1),
        token(Kind::Eof, 2),
    ];
    let refusal = parse(&stream).unwrap_err();
    assert_eq!(refusal.error.reason(), "expected_open_paren");
    assert_eq!(
        refusal.error.message(),
        "Expected '(' after function name but found end of input"
    );
    let stream = [
        token(
            Kind::FractionalNumber {
                whole: 1,
                fraction: "5".to_owned(),
            },
            1,
        ),
        token(Kind::Eof, 4),
    ];
    let refusal = parse(&stream).unwrap_err();
    assert_eq!(refusal.error.reason(), "expected_duration_unit");
    assert_eq!(
        refusal.error.message(),
        "Expected a duration unit after number '1.5'"
    );
}
