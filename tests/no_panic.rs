//! No input makes the crate panic or overflow its stack: sources,
//! instruction lists and contexts generated from a fixed seed, by a small
//! generator written here (no added dependency), go through every entry
//! point, and each must answer, `Ok` or `Err`.
//!
//! The work runs twice: on the test's own thread, where a panic fails the
//! test, and on a thread with a small stack in a child process (this test
//! binary, run again for one test with an environment variable set), since
//! a stack overflow aborts the whole process rather than failing a test.

#[cfg(test)]
mod no_panic {
    use predicator_rs::{
        Context, Date, DateTime, Duration, Instructions, Map, OnUnbound, Options, Value, compile,
        compile_program, evaluate, execute, execute_value, parse_duration, tagged,
    };
    use std::process::Command;

    const SEED: u64 = 0x5eed_1234_abcd_0042;
    const ROUNDS: usize = 3_000;
    const SMALL_STACK: usize = 256 * 1024;
    const CHILD: &str = "PREDICATOR_NO_PANIC_CHILD";

    /// A xorshift64 generator: the same draws from the same seed, every run.
    struct Draw(u64);

    impl Draw {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }

        /// A number below `bound`.
        fn below(&mut self, bound: usize) -> usize {
            (self.next() % bound as u64) as usize
        }

        fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
            &items[self.below(items.len())]
        }

        fn chance(&mut self, one_in: usize) -> bool {
            self.below(one_in) == 0
        }
    }

    /// Words and marks a source is made of: the language's own, and some it
    /// does not have.
    const SOURCE_PARTS: &[&str] = &[
        "x",
        "score",
        "a.b",
        "items",
        "a",
        "b",
        "0",
        "1",
        "-1",
        "9223372036854775807",
        "9223372036854775808",
        "1.5",
        "1e309",
        "'s'",
        "\"t\"",
        "\"\\u12\"",
        "true",
        "false",
        "null",
        "undefined",
        "AND",
        "OR",
        "NOT",
        "and",
        "or",
        "not",
        "in",
        "contains",
        "is",
        "blank",
        "defined",
        "between",
        "+",
        "-",
        "*",
        "/",
        "%",
        "==",
        "!=",
        "===",
        "!==",
        "<",
        ">",
        "<=",
        ">=",
        "!",
        "(",
        ")",
        "[",
        "]",
        "{",
        "}",
        ",",
        ":",
        ";",
        "=",
        ".",
        "::",
        "integer",
        "string",
        "duration",
        "date",
        "if",
        "else",
        "while",
        "3d",
        "1h30m",
        "2w",
        "ago",
        "from",
        "now",
        "next",
        "last",
        "len(",
        "upper(",
        "Math.pow(",
        "Math.random(",
        "Date.now(",
        "JSON.parse(",
        "JSON.stringify(",
        "#2030-01-15#",
        "#2030-01-15T12:00:00Z#",
        "#bad#",
        "\u{301}",
        "\u{1F600}",
        "\n",
        " ",
        "\t",
        "\"",
        "'",
        "#",
        "@",
        "\\",
    ];

    fn source(draw: &mut Draw) -> String {
        let count = draw.below(14);
        let mut text = String::new();
        for _ in 0..count {
            let part: &&str = draw.pick(SOURCE_PARTS);
            text.push_str(part);
            if draw.chance(3) {
                text.push(' ');
            }
        }
        text
    }

    /// A source nested `levels` deep through one of the forms that open a
    /// level.
    fn nested(levels: usize, form: usize) -> String {
        let (open, close) = match form {
            0 => ("(", ")"),
            1 => ("[", "]"),
            2 => ("NOT ", ""),
            3 => ("-", ""),
            4 => ("{a: ", "}"),
            5 => ("len(", ")"),
            _ => ("if x { ", " }"),
        };
        format!("{}x{}", open.repeat(levels), close.repeat(levels))
    }

    const OPCODES: &[&str] = &[
        "lit",
        "load",
        "compare",
        "not",
        "unary_bang",
        "unary_minus",
        "jump_if_falsy_or_pop",
        "jump_if_true_or_pop",
        "add",
        "subtract",
        "multiply",
        "divide",
        "modulo",
        "access",
        "bracket_access",
        "in",
        "contains",
        "make_list",
        "object_new",
        "object_set",
        "duration",
        "relative_date",
        "call",
        "store",
        "pop",
        "jump",
        "pop_jump_if_falsy",
        "jump_backward",
        "cast",
        "presence",
        "intersects",
        "and",
        "or",
        "nope",
    ];

    const WORDS: &[&str] = &[
        "x",
        "a",
        "EQ",
        "NE",
        "GT",
        "LT",
        "GTE",
        "LTE",
        "STRICT_EQ",
        "IN",
        "ago",
        "next",
        "sideways",
        "d",
        "mo",
        "years",
        "integer",
        "float",
        "string",
        "boolean",
        "date",
        "datetime",
        "duration",
        "blank",
        "defined",
        "len",
        "Math.pow",
        "Math.random",
        "Date.now",
        "JSON.parse",
        "JSON.stringify",
        "substring",
        "index_of",
        "f",
        "",
    ];

    /// One operand, as JSON text.
    fn operand(draw: &mut Draw, depth: usize) -> String {
        match draw.below(if depth > 3 { 6 } else { 9 }) {
            0 => format!("{}", draw.next() as i64 % 7),
            1 => draw
                .pick(&[
                    "0",
                    "-1",
                    "9223372036854775807",
                    "-9223372036854775808",
                    "1e400",
                    "2.5",
                ])
                .to_string(),
            2 => format!("{:?}", draw.pick(WORDS)),
            3 => draw.pick(&["true", "false", "null"]).to_string(),
            4 => draw
                .pick(&[
                    r#"{"$type":"undefined"}"#,
                    r#"{"$type":"date","value":"2030-01-15"}"#,
                    r#"{"$type":"datetime","value":"2030-01-15T12:00:00Z"}"#,
                    r#"{"$type":"duration","value":{"days":3}}"#,
                    r#"{"$type":"nope"}"#,
                ])
                .to_string(),
            5 => format!("{}", draw.below(12)),
            6 => {
                let items: Vec<String> = (0..draw.below(4))
                    .map(|_| operand(draw, depth + 1))
                    .collect();
                format!("[{}]", items.join(","))
            }
            7 => {
                let pairs: Vec<String> = (0..draw.below(4))
                    .map(|_| format!("{:?}:{}", draw.pick(WORDS), operand(draw, depth + 1)))
                    .collect();
                format!("{{{}}}", pairs.join(","))
            }
            _ => format!("[[{},{:?}]]", draw.next() as i64 % 5, draw.pick(WORDS)),
        }
    }

    /// The operands an opcode takes, mostly of the shape it reads, sometimes
    /// of any shape, so a list both runs and is refused.
    fn operands(name: &str, draw: &mut Draw) -> Vec<String> {
        if draw.chance(8) {
            return (0..draw.below(3)).map(|_| operand(draw, 0)).collect();
        }
        let word = |draw: &mut Draw, words: &[&str]| format!("{:?}", draw.pick(words));
        let small = |draw: &mut Draw| format!("{}", draw.below(6));
        match name {
            "lit" => vec![operand(draw, 0)],
            "load" | "access" | "object_set" => {
                vec![word(draw, &["x", "a", "score", "items", "days"])]
            }
            "compare" => vec![word(
                draw,
                &[
                    "EQ",
                    "NE",
                    "GT",
                    "LT",
                    "GTE",
                    "LTE",
                    "STRICT_EQ",
                    "STRICT_NE",
                    "IN",
                ],
            )],
            "jump_if_falsy_or_pop"
            | "jump_if_true_or_pop"
            | "jump"
            | "pop_jump_if_falsy"
            | "jump_backward"
            | "make_list"
            | "store" => vec![small(draw)],
            "duration" => vec![operand(draw, 0)],
            "relative_date" => vec![word(draw, &["ago", "last", "future", "next", "sideways"])],
            "call" => vec![
                word(
                    draw,
                    &[
                        "len",
                        "upper",
                        "substring",
                        "index_of",
                        "concat",
                        "Math.pow",
                        "Math.abs",
                        "Math.round",
                        "Math.min",
                        "Math.random",
                        "Date.now",
                        "Date.year",
                        "JSON.parse",
                        "JSON.stringify",
                        "f",
                        "nope",
                    ],
                ),
                small(draw),
            ],
            "cast" => vec![word(
                draw,
                &[
                    "integer", "float", "string", "boolean", "date", "datetime", "duration", "x",
                ],
            )],
            "presence" => vec![word(draw, &["blank", "defined", "x"])],
            _ => Vec::new(),
        }
    }

    fn instructions(draw: &mut Draw) -> String {
        let count = draw.below(12);
        let rows: Vec<String> = (0..count)
            .map(|_| {
                // Half the rows push a value, so most operators find operands.
                let name = match draw.chance(2) {
                    true => *draw.pick(&["lit", "lit", "load"]),
                    false => *draw.pick(OPCODES),
                };
                let mut row = vec![format!("{name:?}")];
                row.extend(operands(name, draw));
                format!("[{}]", row.join(","))
            })
            .collect();
        format!("[{}]", rows.join(","))
    }

    fn value(draw: &mut Draw, depth: usize) -> Value {
        match draw.below(if depth > 3 { 9 } else { 11 }) {
            0 => Value::Undefined,
            1 => Value::Null,
            2 => Value::from(draw.chance(2)),
            3 => Value::Integer(*draw.pick(&[0, 1, -1, 7, i64::MAX, i64::MIN])),
            4 => Value::Float(*draw.pick(&[0.0, -0.0, 1.5, 1e308, f64::NAN, f64::INFINITY])),
            5 => Value::from(*draw.pick(&["", " ", "x", "3d", "2030-01-15", "\u{301}e"])),
            6 => Value::Date(Date::new(2030, 1, 15).unwrap()),
            7 => {
                Value::DateTime(DateTime::from_unix(draw.next() as i64 % 4_000_000_000, 0).unwrap())
            }
            8 => Value::Duration(Duration {
                days: draw.next() as i64 % 5,
                milliseconds: draw.next() as i64,
                ..Duration::default()
            }),
            9 => Value::List((0..draw.below(4)).map(|_| value(draw, depth + 1)).collect()),
            _ => {
                let mut map = Map::new();
                for _ in 0..draw.below(4) {
                    map.insert(*draw.pick(WORDS), value(draw, depth + 1));
                }
                Value::Map(map)
            }
        }
    }

    fn context(draw: &mut Draw) -> Context {
        let mut context = Context::new();
        for _ in 0..draw.below(4) {
            context.insert(
                *draw.pick(&["x", "a", "score", "items", "f"]),
                value(draw, 0),
            );
        }
        context
    }

    /// How far the generated inputs reach: programs read, and runs answered
    /// with a value rather than refused.
    #[derive(Default)]
    struct Reach {
        programs: usize,
        values: usize,
    }

    /// Every entry point, given one program both ways where it takes one.
    fn every_entry_point(
        text: &str,
        list: Option<&Instructions>,
        context: &Context,
        options: &Options,
        reach: &mut Reach,
    ) {
        let _ = compile(text);
        let _ = compile_program(text);
        let _ = parse_duration(text);
        let _ = tagged::decode(text);
        let _ = evaluate(text, context, options);
        let _ = execute(text, context, options);
        let _ = execute_value(text, context, options);
        if let Some(list) = list {
            let _ = list.to_json();
            reach.programs += 1;
            reach.values += usize::from(evaluate(list, context, options).is_ok());
            let _ = execute(list, context, options);
            let _ = execute_value(list, context, options);
        }
    }

    fn work() -> Reach {
        let mut reach = Reach::default();
        let mut draw = Draw(SEED);
        let noon = DateTime::from_unix(1_894_708_800, 0);
        let plain = Options::new();
        let hosted = Options::new()
            .on_unbound(OnUnbound::Error)
            .clock(move || noon)
            .function("f", |arguments| {
                arguments.first().cloned().ok_or_else(|| "no".to_owned())
            })
            .random(|| u64::MAX)
            .loop_budget(50)
            .protected_roots(["a"]);
        let no_clock = Options::new().clock(|| None).loop_budget(0);
        for round in 0..ROUNDS {
            let options = match round % 3 {
                0 => &plain,
                1 => &hosted,
                _ => &no_clock,
            };
            let context = context(&mut draw);
            let _ = tagged::encode(&value(&mut draw, 0));
            let text = source(&mut draw);
            let compiled = compile(&text).ok();
            every_entry_point(&text, compiled.as_ref(), &context, options, &mut reach);
            let json = instructions(&mut draw);
            let list = Instructions::from_json(&json).ok();
            every_entry_point(&json, list.as_ref(), &context, options, &mut reach);
        }
        // Sources nested past and up to the parser's bound, through every form
        // that opens a level.
        for form in 0..7 {
            for levels in [399, 400, 401, 5_000] {
                let text = nested(levels, form);
                let compiled = compile(&text).ok();
                every_entry_point(
                    &text,
                    compiled.as_ref(),
                    &Context::new(),
                    &plain,
                    &mut reach,
                );
            }
        }
        // A context binding a value nested far past the value bound, built
        // without recursion: refused by every entry point, and dropped.
        let mut deep = Value::Integer(1);
        for _ in 0..100_000 {
            deep = Value::List(vec![deep]);
        }
        let mut deep_context = Context::new();
        deep_context.insert("x", deep);
        every_entry_point(
            "x",
            compile("x").ok().as_ref(),
            &deep_context,
            &plain,
            &mut reach,
        );
        drop(deep_context);
        // Instruction lists and tagged texts nested far past the value bound.
        let deep_list = format!("[[\"lit\",{}1{}]]", "[".repeat(5_000), "]".repeat(5_000));
        every_entry_point(&deep_list, None, &Context::new(), &plain, &mut reach);
        let deep_map = format!("{}1{}", "{\"a\":".repeat(5_000), "}".repeat(5_000));
        every_entry_point(&deep_map, None, &Context::new(), &plain, &mut reach);
        reach
    }

    // Sabotage: with relative_date asserting its direction is a known one,
    // the generated "sideways" panics and this test fails.
    #[test]
    fn no_generated_input_panics() {
        let reach = work();
        // The generator reaches the evaluator: most programs are read, and
        // some runs answer a value rather than a refusal.
        assert!(
            reach.programs > ROUNDS / 2,
            "{} programs read",
            reach.programs
        );
        assert!(
            reach.values > ROUNDS / 20,
            "{} runs answered a value",
            reach.values
        );
    }

    // The work itself on a small stack, run only in the child process the
    // next test starts.
    #[test]
    fn no_panic_child() {
        if std::env::var_os(CHILD).is_none() {
            return;
        }
        std::thread::Builder::new()
            .stack_size(SMALL_STACK)
            .spawn(|| {
                work();
            })
            .unwrap()
            .join()
            .unwrap();
    }

    // Sabotage: with the body of Value's Drop emptied (so a value drops its
    // members by recursion), the deep context overflows the small stack and
    // this test fails.
    #[test]
    fn no_generated_input_overflows_a_small_stack() {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "no_panic::no_panic_child",
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
        // The child ran the work, not a filter that matched nothing.
        let report = String::from_utf8_lossy(&output.stdout);
        assert!(
            report.contains("1 passed"),
            "the child ran no test: {report}"
        );
    }
}
