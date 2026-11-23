# predicator-rs

[![MIT License][badge_license_url]](LICENSE)
[![CI][badge_ci_url]][ci_url]

A safe, non-evaluating condition language for end users, for Rust. A
condition is written as text, such as `score > 85 and 2 in items`, compiled
to a flat list of stack-machine instructions and run against a context of
variables. No code is ever evaluated: the text is read by a lexer and a
parser of this crate, and the instructions are run by an evaluator that can
do only what the instruction set says. The language, the instruction set and
the answers are wire-compatible with the Elixir reference implementation,
[predicator-ex](https://github.com/riddler/predicator-ex), and the crate is
checked against that implementation's conformance corpus.

## Why predicator

A rule an end user writes has to be decided by code the user did not write,
and often in more than one place. Handing the text to a general-purpose
interpreter hands the user the whole interpreter; a hand-rolled parser per
place lets the copies drift apart. predicator gives one small language with
a defined set of operations, a defined answer for every operand a rule can
meet (an error value, never a crash), and a shared corpus that every
implementation of it must pass. A condition compiled by one implementation
can be run by another, because the instruction list is plain data.

## Install

Add the crate to your `Cargo.toml`:

```toml
[dependencies]
predicator-rs = "0.1"
```

The crate is `predicator-rs` on crates.io (the name `predicator` there is
another project's) and the library is imported as `predicator_rs`:

```rust
use predicator_rs::{Context, Options, Value, evaluate};
```

A `0.0.0` release of `predicator-rs` on crates.io is a placeholder that
reserves the name; it has no library API.

## Basic usage

```rust
use predicator_rs::{Context, Error, Key, Options, Value};
use predicator_rs::{compile, evaluate, execute, execute_value};

fn main() -> Result<(), Error> {
    // Compile once: the instruction list is plain data to store or send.
    let instructions = compile("score > 85 and 2 in items")?;
    assert_eq!(
        instructions.to_json()?,
        concat!(
            r#"[["load","score"],["lit",85],["compare","GT"],"#,
            r#"["jump_if_falsy_or_pop",4],["lit",2],["load","items"],["in"]]"#,
        )
    );

    // Run it against a context of variables.
    let mut context = Context::new();
    context.insert("score", Value::from(90));
    let items = vec![Value::from(1), Value::from(2), Value::from(3)];
    context.insert("items", Value::from(items));
    let options = Options::new();
    assert_eq!(evaluate(&instructions, &context, &options)?, Value::from(true));

    // Or hand evaluate the source and let it compile.
    assert_eq!(evaluate("score + 1", &context, &options)?, Value::from(91));

    // A statement program answers the context it leaves.
    let execution = execute("x = 1; y = x + 2", &Context::new(), &options)?;
    let y = execution.context().get(&Key::from("y"));
    assert_eq!(y, Some(&Value::from(3)));
    assert_eq!(execution.stored_roots(), [Key::from("x"), Key::from("y")]);

    // execute_value also answers the last expression statement's value.
    let execution = execute_value("x = 2; x * 10", &Context::new(), &options)?;
    assert_eq!(execution.value(), Some(&Value::from(20)));

    // A host function is passed per call, by name.
    let options = Options::new().function("double", |arguments| {
        match arguments {
            [Value::Integer(x)] => x
                .checked_mul(2)
                .map(Value::from)
                .ok_or_else(|| "too large".to_owned()),
            _ => Err("double takes one integer".to_owned()),
        }
    });
    let doubled = evaluate("double(score)", &context, &options)?;
    assert_eq!(doubled, Value::from(180));
    Ok(())
}
```

## Results and errors

Every entry point answers a `Result`. Nothing about a source, an instruction
list or a context makes the crate panic: a source that does not compile, a
run that stops and a value outside the language's domain all answer an
`Err`.

- `compile(source)` answers the `Instructions` of an expression, and
  `compile_program(source)` those of statements separated by `;`.
- `evaluate(program, &context, &options)` answers an expression's `Value`.
- `execute(program, &context, &options)` answers an `Execution`: the
  context the run left and the roots it stored into.
- `execute_value(program, &context, &options)` answers an `Execution` that
  also holds the last expression statement's value.
- `parse_duration(text)` answers a `Duration` read from text such as
  `"1h30m"`.
- `ISA_VERSION` is the instruction-set version the crate emits and runs,
  7.

A program is an `&Instructions` or a `&str` source: `evaluate` compiles a
source as an expression and `execute` and `execute_value` as a statement
program, and a source that does not compile is refused before anything
runs. `Instructions::from_json` reads an instruction list from JSON in the
conformance corpus's tagged encoding and `Instructions::to_json` writes one;
neither needs a feature.

An `Err` is an `Error`, which holds one of four error types, each with a
`reason`, a `message` and a `position` (an `Option<Position>`, a line and a
column counted from 1):

- `Error::Parse(ParseError)`: a source or a JSON text that does not parse.
  A `ParseError` from a source carries the position of the token it names.
- `Error::Evaluation(EvaluationError)`: a run that stopped, such as a
  division by zero, an unknown instruction or a host function that refused.
- `Error::TypeMismatch(TypeMismatchError)`: an operand an operation refused
  on its type. Its reason is the operation.
- `Error::UndefinedVariable(UndefinedVariableError)`: a variable the context
  does not bind, when the run's answer depends on it.

`Error::type_name` answers the type as the corpus spells it. The type and
the reason are what the conformance corpus matches; the message is for a
person and may change.

```rust
use predicator_rs::{Context, Options, Position, Value, compile, evaluate};

let mut context = Context::new();
context.insert("x", Value::from(1));
let refused = evaluate("x / 0", &context, &Options::new()).unwrap_err();
assert_eq!(refused.type_name(), "EvaluationError");
assert_eq!(refused.reason(), "division_by_zero");

let refused = compile("score >").unwrap_err();
assert_eq!(refused.type_name(), "ParseError");
assert_eq!(refused.position(), Some(Position { line: 1, column: 8 }));
```

A statement program writes a copy of the context; the one passed in is never
changed. A run that completes answers an `Execution`. One that stops answers
a `Stopped`, which holds the error it stopped on, the context as of the last
statement it completed (every earlier write kept, the failing write not
made) and the roots it had stored into; keeping that context or the one
passed in is the caller's choice. A `Stopped` converts into its `Error`, so
`?` takes it where only the error matters.

```rust
use predicator_rs::{Context, Key, Options, Value, execute};

// a = 1; a.b = 2 stops at the second statement, keeping the first.
let stopped = execute("a = 1; a.b = 2", &Context::new(), &Options::new())
    .unwrap_err();
assert_eq!(stopped.error().reason(), "not_a_container");
let a = stopped.context().get(&Key::from("a"));
assert_eq!(a, Some(&Value::from(1)));
assert_eq!(stopped.stored_roots(), [Key::from("a")]);
```

`store` writes as the reference's instruction-set text states it: an
interior slot that is missing, `null` or undefined becomes a list when the
segment after it is an integer and a map otherwise; an index past a list's
end pads it with undefined; the leaf is overwritten. An integer segment
into a map is an integer key. A path through a scalar, a date or an
instant is refused as `not_a_container`, a negative index as
`invalid_index`, and an empty path as `not_assignable`; a segment that is
neither a string nor an integer is a `TypeMismatchError`. A write that is
refused leaves nothing of itself behind.

## Options

The options of a run are an `Options` value built per call and passed by
reference. Nothing is configured globally.

- `on_unbound(OnUnbound)`: what a load of a variable the context does not
  bind does. `OnUnbound::Undefined` (the default) reads it as undefined, and
  the run fails with an `UndefinedVariableError` only when undefined is its
  answer or an instruction refuses that undefined; `OnUnbound::Error` fails
  the run at the load.
- `clock(f)`: a callable answering the current time as an
  `Option<DateTime>`, in place of the system clock. A run reads it at most
  once, the first time the program reads the time (`relative_date`,
  `Date.now`), and never when it does not; every later read in that run sees
  the same instant. A clock that answers `None` fails the run with an
  `EvaluationError`, reason `clock_failed`.
- `function(name, f)`: a host function, called by name with the call's
  arguments and nothing else, answering a `Value` or refusing with a
  `String`. A refusal fails the run with an `EvaluationError` whose reason
  and message are that text, and an answer is checked against the value
  domain. A name passed this way shadows the library function of that name
  for the runs those options are given to.
- `random(f)`: the source `Math.random` draws from, a callable answering 64
  random bits. Without one, a run draws from a source seeded from the
  standard library's per-process hash keys, which needs no dependency and is
  not cryptographic.
- `loop_budget(n)`: how many back edges (`jump_backward`) one run may take,
  10,000 unless set. A run that reaches a back edge with none left fails
  there with an `EvaluationError`, reason `loop_budget_exceeded`. The budget
  bounds `evaluate` and `execute` alike.
- `protected_roots(roots)`: roots a statement program may not store into. A
  `store` whose path's root is one of them is refused when it is reached,
  with an `EvaluationError`, reason `protected_root`, whose `root()` names
  the root. A root is protected whole, and the check comes after the path's
  segments are checked, as the reference makes it. `evaluate` does not read
  them.

A function passed in (a host function, the clock, the random source) is
called on the caller's thread and may borrow from the caller for the length
of the call. A panic inside one is not caught: it unwinds out of the entry
point to the caller, as a panic in the caller's own code would.

```rust
use predicator_rs::{Context, Date, DateTime, OnUnbound, Options, Value};
use predicator_rs::evaluate;

let noon = DateTime::new(Date::new(2030, 1, 15).unwrap(), 12, 0, 0, 0);
let options = Options::new()
    .on_unbound(OnUnbound::Error)
    .clock(move || noon)
    .random(|| 1 << 63)
    .loop_budget(100);
let year = evaluate("Date.year(Date.now())", &Context::new(), &options);
assert_eq!(year, Ok(Value::from(2030)));
let draw = evaluate("Math.random()", &Context::new(), &options);
assert_eq!(draw, Ok(Value::from(0.5)));
let unbound = evaluate("x", &Context::new(), &options).unwrap_err();
assert_eq!(unbound.reason(), "unbound_variable");
```

`call` runs the reference's function library when no host function shadows
the name: `len`, `upper`, `lower`, `trim`, `starts_with`, `ends_with`,
`substring`, `index_of` and `concat`; `Math.pow`, `Math.sqrt`, `Math.abs`,
`Math.floor`, `Math.ceil`, `Math.round`, `Math.min`, `Math.max` and
`Math.random`; `Date.year`, `Date.month`, `Date.day` and `Date.now`; and
`JSON.stringify` and `JSON.parse`. A function that refuses its arguments,
an unknown name and a wrong argument count fail the run with an
`EvaluationError` whose reason is the reference's own message
(`Unknown function: x`).

## The value domain

A `Value` is one of: `Null` (present and empty), `Undefined` (an absence,
distinct from null), `Bool`, `Integer` (an `i64`), `Float` (an `f64`; `1`
and `1.0` differ), `String`, `List` (a `Vec<Value>`), `Map` (a `Map` keyed
by `Key`), `Date` (a civil date), `DateTime` (an instant in UTC to the
microsecond) and `Duration` (eight integer units, `years` through
`milliseconds`). A `Key` is a string, an integer, a boolean or undefined,
the four kinds the reference's instruction-set text lets index a map. A
`Context` binds root names to values.

Every value a context binds is checked before anything runs, and so is a
host function's answer: a float that is not finite (`non_finite_number`)
and lists and maps nested past 100 levels (`depth_limit_exceeded`) are
refused with an `EvaluationError`. Equality, ordering, cloning and dropping
walk a value without recursion, so a deeper value the host builds is still
compared, copied and dropped without exhausting the stack.

`cast` converts a value to one of the seven type names (`integer`, `float`,
`string`, `boolean`, `date`, `datetime`, `duration`) as the reference's
conversion matrix states it: undefined casts to undefined, and a value that
cannot become one of the target type, `null`, a list and a map included,
casts to undefined rather than failing. A string is read whole, with no
trimming: an integer as `-?[0-9]+`, a float as `-?[0-9]+(\.[0-9]+)?`, a
boolean as exactly `true` or `false`, a date as `YYYY-MM-DD`, an instant as
an ISO 8601 datetime (a `T` before the time, a fraction after a full stop
or a comma, read to the microsecond, and an offset of `Z`, `+HH:MM`,
`+HHMM` or `+HH`, either sign), and a duration as the duration-literal
grammar, fractions included (`"1.5s"` is a second and 500 milliseconds).
`parse_duration` reads that grammar outside a run, answering the `Duration`
or an `EvaluationError`, reason `invalid_duration_format`. A float is
written as the reference writes one (`1.0e16`, `0.0001`), and an instant in
UTC with its fraction left out when it is zero and six digits when it is
not.

`presence` answers `x is blank` and `x is defined` for every value and
never fails. Blank is undefined, `null`, an empty list, a map with no keys,
and a string that is empty or holds only the Unicode White_Space code
points the reference lists (U+0009 to U+000D, U+0020, U+0085, U+00A0,
U+1680, U+2000 to U+200A, U+2028, U+2029, U+202F, U+205F and U+3000),
matched code point by code point rather than through Rust's own trim; a
zero-width space (U+200B) is not one of them. Every other value, `0`,
`false` and a zero duration included, is not blank. `intersects` answers
whether two lists share a member under the equality `in` uses, and
undefined when either operand is not a list.

The module `tagged` is a codec for the conformance corpus's tagged JSON
encoding, which keeps what a JSON round trip loses (undefined, dates,
instants, durations): `tagged::decode` reads a text into a `Value` and
`tagged::encode` writes one. With the `serde` feature (off by default),
`Value` implements `Serialize` and `Deserialize` in that encoding, and
`Date`, `DateTime` and `Duration` in the text and the unit map it carries
them in.

```rust
use predicator_rs::{Value, tagged};

let value = tagged::decode(r#"[1,1.0,{"$type":"undefined"}]"#)?;
let expected = vec![Value::from(1), Value::from(1.0), Value::Undefined];
assert_eq!(value, Value::from(expected));
assert_eq!(tagged::encode(&value)?, r#"[1,1.0,{"$type":"undefined"}]"#);
# Ok::<(), predicator_rs::Error>(())
```

## Compatibility

- **Rust 1.85 or later** (`rust-version` in `Cargo.toml`, edition 2024). CI
  runs the test suite with every feature on Rust 1.85, and the full quality
  gate on the toolchain `mise.toml` pins.
- **One required dependency**, `unicode-segmentation`, for the grapheme
  clusters the reference counts. `serde` is optional, behind the `serde`
  feature, off by default. The lexer, the parser, JSON reading and writing,
  date arithmetic and float text are written here.
- **The standard library is required.** The crate is not `no_std`.
- **wasm32-unknown-unknown.** CI builds the library for that target, with
  the default features and with every feature. The standard library has no
  system clock there, and reading it panics, so on that target a host passes
  a clock (`Options::clock`) to every run whose program reads the time; a
  program that does not read the time never reads the clock. The default
  random source works there, seeded more weakly; pass one (`Options::random`)
  where `Math.random` matters.
- **No shared state.** There is no global configuration, no static mutable
  state and no thread-local; host functions, the clock and the random source
  are passed per call, and `Options` is `Send` and `Sync`.
- **No panic on any input.** No source, instruction list or context makes
  the crate panic or overflow its stack, and the crate forbids `unsafe`
  code. A test feeds every entry point generated sources, instruction lists
  and contexts, on a small stack as well as the test's own. A panic inside a
  function the host passes in is the host's own and is not caught (ruled by
  the operator, 2026-11-20).

## Conformance

The language is defined by the reference implementation's conformance
corpus, vendored under `conformance/` at a release tag of
[predicator-ex](https://github.com/riddler/predicator-ex).
`conformance/SOURCE.json` names the tag, the commit and the corpus hash; at
this head they are the tag `v9.6.0`, the commit
`8d5081e33269fa1be8e8560a183bba7e72ff8930`, the instruction-set version 7
and the hash
`sha256:2eebecf5b5493aaed4ddfd5ac7926ef05211c4d57b6c27eda7724320486b7686`.

The corpus has two surfaces: the **evaluator** runs an instruction list
against a context, and the **compiler** compiles a source to the
instruction list the case expects. Cases are grouped in ten cumulative
tiers, and a claim of tier N says every case of that surface in tiers 1
through N passes. `conformance/registry.json` records the claims and one
entry per passing case. At this head:

- The evaluator claims tier 10, with 470 entries. The corpus holds 475
  cases; the other 5 carry the `retired` feature tag (they use an opcode
  the instruction set has retired), and a run that claims version 7 does
  not attempt them.
- The compiler claims tier 10, with 428 entries: every case in the corpus
  that has a source. A case with no source is not on the compiler surface.

No case is skipped. A case this crate cannot pass is a defect to bring to
the reference, never an entry on a skip list. The full gate re-runs both
surfaces and fails when the registry and a run made now disagree.

The crate also vendors two transcripts of the reference's own answers over
sources no corpus case reaches, its compiler's and its lexer's, and checks
its compiler and its lexer against them (`conformance/README.md` says what
is copied and from where). The compile transcript's decompile rows render
a tree back to source text; decompiling is out of scope until the crate has
a function for it.

A claim is made against one corpus. A sibling implementation that vendors
an older tag claims against that older corpus, which has fewer tiers and
fewer cases, and so claims less: the Ruby gem `predicator` 2.0.0 vendors
`v9.5.0`, instruction-set version 6, and claims tier 9 on the evaluator and
tier 7 on the compiler.

## Differences from the reference

Where the conformance corpus is silent, the reference at the tag the
vendored corpus names answers. These are the places where this crate's
answer differs from the reference's code, follows the reference's normative
text where its code says otherwise, or has no counterpart in the reference.

The differences the design starts from:

- **Integers are 64-bit, and an overflow is refused.** An integer is an
  `i64`; the reference's integers have no bound (ruled by the operator,
  2026-11-20). An unbounded integer would take a dependency the crate does
  not have. The tagged codec refuses an integer outside that range with the
  reason `integer_out_of_range`, rather than reading it as a float, and so
  does the lexer an integer literal in a source (a `ParseError` at the
  literal), where the reference's code reads an integer of any size. Every
  integer step is checked: `add`, `subtract`, `multiply` or `divide` on two
  integers whose result is outside that range, and `unary_minus` on the
  least `i64`, fail with an `EvaluationError`, reason `integer_overflow`,
  never a wrapped or rounded number. (`modulo` cannot leave the range: the
  least `i64` modulo `-1` is `0`.)
- **Where the reference raises, this crate answers a named refusal.** An
  entry point answers a `Result` and never panics, so a raise has no
  counterpart here: a string too large for a finite float cast to `float`,
  a decimal literal too large for one, a float result or a `Math.pow`
  answer that is not finite, an instant written as text past the year 9999
  and an instruction list that is not a list are each refused by name, and
  so are the `JSON.parse` faults the reference answers with its runtime's
  own error term, as the entries below state.
- **A source nests 400 levels at most** (ruled by the operator,
  2026-11-20), so that no source overflows the stack. The parser refuses a
  source that nests deeper with a `ParseError`, reason
  `nesting_depth_exceeded`, at the token where the parse stopped; the
  reference's parser follows a source as deep as it nests. The whole
  expression is the first level, and each parenthesis, list, object, index,
  call's arguments, prefix operator (`-`, `!`, `NOT`), relative date's
  operand, block's statements, `else if`, and statement's condition or value
  opens one more; a chain of infix operators or of postfix steps
  (`a + b + c`, `a.b[0]::string`) opens none, however long. The bound is
  above the 300 parentheses the reference's compile transcript compiles.
- **A host function takes its arguments only**, with no arity of its own
  (ruled by the operator, 2026-11-20): it sees the call's arguments as a
  slice and checks their count itself, and it cannot read or change the
  run's context. The reference's functions also receive the run's context,
  and a host's function there declares an arity the evaluator checks.
- **`execute` answers the roots a run stored into** (`stored_roots`), each
  once in the order of its first store, whether or not a store changed the
  value there, so a caller learns what a run wrote without comparing
  contexts. The reference has no such answer; its callers compare the
  context before and after. `Execution::value` is `None` from `execute` and
  the last expression statement's value from `execute_value`.
- **The clock is passed per call and read once per run**, as no state is
  shared between calls. The reference's code reads the system clock at each
  `relative_date`; here the clock is the one `Options` names (the system
  clock by default), read the first time a run reads the time, and every
  later read in that run sees the same instant. A clock with no time to
  give fails the run with the reason `clock_failed`, which the reference
  has no counterpart for.
- **`Math.random` draws from a source passed per call** (`Options::random`)
  and turns its 64 bits into a float in `[0, 1)`, the top 53 bits over
  2^53; the reference's code draws from its runtime's process-wide
  generator.

Refusals where the reference raises or has no bound:

- **Values are bounded at 100 levels.** Lists and maps nested past 100
  levels are refused where a value crosses into the crate and by the tagged
  codec, with the reason `depth_limit_exceeded`; the reference has no bound. A
  deeper value the host builds is still compared, copied and dropped
  without recursion.
- **A float that is not finite is refused** where a value crosses into the
  crate, with the reason `non_finite_number`; the reference's floats cannot
  hold one.
- **A store makes a list at most 1,048,576 (2^20) items long.** An index
  past a list's end at or beyond that is refused with an `EvaluationError`,
  reason `list_too_long`; a list already longer may be written inside its
  end. The bound is per list, not per run: a loop that stores padded lists
  under many roots can still grow a run's memory until its loop budget is
  spent. The reference pads to any index.
- **A cast whose answer is an integer outside `i64` is refused.** A string
  or a float cast to `integer`, and a string cast to `duration` with a unit
  past that range, fail with an `EvaluationError`, reason
  `integer_overflow`; the reference's code answers an integer of any size
  there.
- **A string too large for a finite float casts to a refusal.** `::float`
  on such a string fails with an `EvaluationError`, reason
  `non_finite_number`; the reference's code raises there.
- **A decimal literal too large for a finite float is refused.** The lexer
  refuses such a literal in a source with a `ParseError`, reason
  `non_finite_number`, at the literal; the reference's code raises there.
- **A float result that is not finite is refused.** `add`, `subtract`,
  `multiply` or `divide` whose float result overflows fails with an
  `EvaluationError`, reason `non_finite_number`; the reference's code
  raises there.
- **An instant outside the years 0 to 9999 casts to no text.** `::string`
  on such an instant (one date arithmetic can make) fails with an
  `EvaluationError`, reason `date_out_of_range`, and `::datetime` on a date
  outside those years is undefined, as the reference's instruction-set text
  states that nothing outside them is an instant. The reference's code
  makes the instant, raises when it writes one past 9999 or before -9999 as
  text, and writes one between as text with a minus sign.
- **Date arithmetic outside the years this crate holds is refused.** A date
  or an instant moves by a duration as the reference's code moves it (a
  week is 7 days, a month 30 and a year 365; a date moves by whole days,
  its hours, minutes and seconds truncated toward zero and its milliseconds
  left out; an instant moves to the second, and by its milliseconds only
  when they are positive). A result whose year falls outside `i32` fails
  with an `EvaluationError`, reason `date_out_of_range`; the reference's
  code answers a date at any year.
- **An instruction list read from JSON must be a list.**
  `Instructions::from_json` refuses any other JSON value with a
  `ParseError`, reason `not_an_instruction_list`; the reference's evaluator
  raises on an argument that is not a list.
- **A function result outside `i64` is refused.** `Math.pow` on two
  integers, `Math.abs` of the least `i64`, and `Math.floor`, `Math.ceil` or
  `Math.round` of a float past the `i64` range fail with an
  `EvaluationError`, reason `integer_overflow`; the reference's code
  answers an integer of any size. `Math.floor` and `Math.ceil` read an
  integer as a float first, as the reference's code does, so an integer
  past 2^53 may answer its nearest float's value, and `i64::MAX` overflows.
- **A function float that is not finite is refused.** `Math.pow` whose
  answer is not a finite float (`Math.pow(0, -1)`, `Math.pow(-8, 0.5)`, an
  overflow) fails with an `EvaluationError`, reason `non_finite_number`;
  the reference's code raises there.
- **`JSON.parse` reports the reference's fault text where the reference
  names a byte or an early end** (`Invalid JSON: unexpected byte 0x6F at
  position 1`, as the reason), a lone high surrogate escape included. An
  integer outside `i64` fails with the reason `integer_out_of_range` and a
  text nested past 100 levels with `depth_limit_exceeded`, where the
  reference answers a value. Where the reference answers with its
  runtime's own error term, this crate refuses by name: a number too large
  for a finite float with `non_finite_number`, and a `\u` escape whose
  digits are not hex, a lone low surrogate, or a high surrogate followed by
  an escape that is not a low one with `invalid_unicode_escape`.

Where this crate follows the reference's instruction-set text or its
documentation, and the reference's code says otherwise:

- **`::date` and `::datetime` read a four-digit year only**, the calendar
  date and the ISO 8601 datetime the reference's instruction-set text
  names; the reference's code also reads a year with a minus sign
  (`-0005-01-01`) or a plus sign (`+2026-08-09`, `+2026-08-09T10:00:00Z`).
- **`::datetime` refuses a space before the time.** The reference's
  instruction-set text names an ISO 8601 datetime, which separates the date
  from the time with a `T`; the reference's code also reads a space there
  (`2026-08-09 10:00:00Z`), and this crate's tagged codec still does.
- **Bracket access on a date or an instant answers undefined, whatever the
  key**, as the reference's instruction-set text states it for a target
  that is neither a map nor a list. The reference's code holds a date as a
  map, so there a key that is not a map key kind (a float, a list) is a
  `TypeMismatchError`.
- **A map key is a string, an integer, a boolean or undefined** (`Key`),
  the four kinds the reference's instruction-set text lets index a map;
  `null` is not one. The tagged encoding has no form for a key that is not
  a string (a JSON object's keys are strings), so the codec refuses one
  with the reason `non_string_map_key`. The reference's codec writes such a
  key as its text (`1` as `"1"`), which reads back as a string key.
- **A duration is one kind of value**: the map of its eight units, `years`
  through `milliseconds`, the same whether a literal, an instruction or the
  host made it, as the reference's instruction-set text states it. A unit
  may be negative. `access` and `bracket_access` read a unit by its name
  from any duration, and `add` and `subtract` move a date by any duration;
  the reference's code reads a unit only from a duration the host supplied,
  and moves a date only by one a literal or an instruction made.
- **`relative_date` takes a duration or a plain map, and moves by all of
  it.** It moves the current time by any duration exactly as `add` and
  `subtract` move an instant. A plain map is read as the duration its unit
  names spell (`years` through `milliseconds`, as strings): a unit it
  leaves out is 0 and a key that names no unit is passed over, as the
  reference's instruction-set text makes a duration the map of its units.
  A map holding a unit that is not an integer, a date, an instant and any
  other value fail with an `EvaluationError`, reason `invalid_stack_value`.
  The reference's code takes any map there, and moves by nothing for a
  duration the host supplied.
- **`object_set` writes into a map or a duration.** A duration beneath the
  value is the plain map of its eight units, as the reference's
  instruction-set text makes it, so the answer is that map with the key
  written; a unit name is a key like any other, so writing `days` replaces
  the unit's value. A date or an instant beneath the value fails with an
  `EvaluationError`, reason `invalid_stack_value`, as that text states it
  for a target that is not a map. The reference's code holds a date and an
  instant as maps and writes the key into them, and keeps a duration's own
  unit beside a written string key of the same name.
- **A units map casts as a duration.** A map carrying the seven units
  `years` through `seconds` under their string names, each an integer (and
  `milliseconds`, when it is there, an integer too), passes through
  `::duration` unchanged and is written by `::string` in the
  duration-literal grammar. The reference's code reads only its own
  duration's units that way, which it keys by atoms, so a string-keyed map
  from the context casts to undefined there. Where `object_set` has written
  a unit's name, this crate writes the unit written (`days` set to 5 on
  `3d` writes `5d`), and the reference writes its duration's own unit
  (`3d`). A units map whose units are not all integers casts to undefined.
- **A duration's text writes a negative unit with its sign** (`-3d`). The
  reference's instruction-set text leaves out only a zero unit; its code
  leaves out a negative one too.
- **A store through a date or an instant is refused** with the reason
  `not_a_container`, as the reference's instruction-set text states it for
  an interior slot that holds a scalar; the reference's code holds those
  two as maps and writes the key into them.
- **A store through a duration writes into the plain map of its eight
  units**, so the slot holds a map afterwards, whatever the key, and a key
  that is a unit's name replaces that unit. The reference's code writes the
  key into the duration's own map, whose units are keyed by atoms, so a
  written `days` there is a string key added beside the unit, which stays
  as it was.
- **`evaluate` does not read the protected roots.** A `store` in an
  instruction list `evaluate` runs writes whatever its root; the
  reference's documentation says the option matters to its statement entry
  point alone, while its code reads it in an instruction list its
  expression entry point runs too.

Where the reference's answer rests on its runtime, and this crate's on its
own:

- **`JSON.stringify` sorts every map's keys, and a duration's units by
  name.** It writes a date and an instant as their text and undefined as
  `"undefined"`, as the reference's encoder does, and a map's keys as text
  in the order that encoder writes a map of up to 32 keys in: integers
  first, then `false`, `true` and undefined, then strings. This crate
  writes every map in that order, and a duration as the map of its eight
  units sorted by name; the reference's encoder writes a map of more than
  32 keys, and a duration's units, in its runtime's own order. It refuses
  only a value nested past 100 levels, with the reason
  `depth_limit_exceeded`; the reference's code never refuses.
- **Two maps, or two durations, have no order.** `compare` under `GT`,
  `LT`, `GTE` or `LTE` answers undefined for two maps or two durations, and
  for a pair of list members that are two unequal maps, two unequal
  durations or two of the map-like values of different kinds (a map, a
  duration, a date or an instant, a date before an instant excepted); two
  equal members are level, and the walk steps on. Giving a map member
  against a date, an instant or a duration member no order is this crate's
  choice: the reference's term order orders that pair by the size of its
  runtime's structures. The reference's instruction-set text leaves the
  order of two maps unspecified for an implementation other than its own;
  its code orders them by its runtime's term order. Equality (`EQ`, `NE`,
  `STRICT_EQ`, `STRICT_NE`) on maps and durations is defined.
- **A duration and a map are different types to `compare`**, so `EQ`, `NE`
  and the ordering operators answer undefined for that pair, as for any
  pair of different types. The reference's code holds a duration as a map
  and answers `false` for `EQ` for a duration an instruction makes; a
  duration the host supplies is a map there, equal to a map of the same
  units.
- **Two instants are equal when they are the same instant**, at any depth
  of a list or a map. The reference's code compares its instant structs,
  which can record a different precision for the same instant.
- **`index_of` counts grapheme clusters**, as `len` and `substring` do: it
  answers the index of the first cluster of the first occurrence that is a
  run of whole clusters (an occurrence starting or ending inside a cluster
  is passed over), or `-1`. The reference's code answers a byte offset
  there while it counts `len` and `substring` in clusters. Clusters are the
  extended grapheme clusters of the Unicode version `unicode-segmentation`
  implements.
- **The column after a duration unit counts this crate's grapheme
  clusters.** The lexer reproduces the reference's column jump after a run
  of duration units, which counts grapheme clusters, using the extended
  grapheme clusters of the Unicode version `unicode-segmentation`
  implements; a character only a newer Unicode version assigns can join a
  cluster here that it does not join in the reference's runtime, moving
  the columns after it by one.
- **An error from running a compiled source carries no position.** The
  reference's code places a failure while running a source at the token of
  the instruction that failed, and its compile functions can answer that
  position, or that span, for each instruction; this crate keeps no such
  table, so an `EvaluationError`, a `TypeMismatchError` or an
  `UndefinedVariableError` has no `position()`. A `ParseError` from a
  source carries its position, as the reference's does.

## Documentation

- Look up
  - [The API reference on docs.rs](https://docs.rs/predicator-rs): every
    public type, function and method, each with an example that runs as a
    test.
  - [The instruction-set reference][isa_url]: the instructions, the value
    space and the refusals this crate implements, read at the corpus tag.
  - [The changelog][changelog_url]:
    what changed in each version.
  - [The conformance corpus][conformance_url]:
    what is vendored, from where, and what is this repository's own.
- Understand
  - [Why a predicate language of its own][why_url]: why not a
    general-purpose language.
  - [Embedding predicator][embedding_url] and
    [custom functions][functions_url], in the reference's terms (read the
    host function contract above for this crate's).

## Development

After checking out the repository, run `mise install` for the Rust toolchain
and the cargo tools `mise.toml` pins. Then `mise run gate:loop` runs the
inner loop (format, lints, tests), and `mise run gate` runs the full quality
gate: the inner loop, the corpus and registry checks, the docs with every
feature and with none, line coverage through `cargo llvm-cov` and
`cargo deny check`. The README's Rust examples run as doctests.

A release is published by CI, never from a checkout: pushing a tag `vX.Y.Z`
that names the version in `Cargo.toml` runs `.github/workflows/release.yml`,
which checks the tag and runs the full gate, then publishes the crate to
[crates.io](https://crates.io) by trusted publishing.

## Contributing

Bug reports and pull requests are welcome on GitHub at
https://github.com/riddler/predicator-rs.

## License

The crate is available as open source under the terms of the
[MIT License](https://opensource.org/licenses/MIT).

<!-- LINKS -->
[ci_url]: https://github.com/riddler/predicator-rs/actions/workflows/ci.yml
[changelog_url]: https://github.com/riddler/predicator-rs/blob/main/CHANGELOG.md
[conformance_url]: https://github.com/riddler/predicator-rs/tree/main/conformance
[isa_url]: https://github.com/riddler/predicator-ex/blob/v9.6.0/docs/isa.md
[why_url]: https://github.com/riddler/predicator-ex/blob/v9.6.0/docs/explanation/why-a-predicate-language-of-its-own.md
[embedding_url]: https://github.com/riddler/predicator-ex/blob/v9.6.0/docs/guides/embedding.md
[functions_url]: https://github.com/riddler/predicator-ex/blob/v9.6.0/docs/guides/custom-functions.md

<!-- BADGES -->
[badge_license_url]: https://img.shields.io/badge/license-MIT-brightgreen.svg
[badge_ci_url]: https://github.com/riddler/predicator-rs/actions/workflows/ci.yml/badge.svg
