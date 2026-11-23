# predicator-rs

A Rust port of the [predicator](https://github.com/riddler/predicator-ex)
condition language. A condition is a short expression such as
`score > 10 and status in ["open", "held"]`. The crate will compile it to a
small instruction set and evaluate it on a stack machine, with the same
answers as the reference implementation on its shared conformance corpus.

The published 0.0.0 is a placeholder that reserves the crate name; the first
usable version is 0.1.0. What the source holds so far is the value model(`Value` and its types), the errors, the codec for the conformance corpus's
tagged-value encoding (`tagged`), and the evaluator:
`evaluate` runs an instruction list (`Instructions`, read from JSON in the
tagged encoding) over a `Context`, with `Options` passed per call. It runs
the instruction set's first five tiers: `lit`, `load`, `compare`, `not`,
`unary_bang`, `unary_minus`, `jump_if_falsy_or_pop` and
`jump_if_true_or_pop`; the arithmetic `add`, `subtract`, `multiply`,
`divide` and `modulo`; `access`, `bracket_access`, `in`, `contains` and`make_list`; `object_new`, `object_set`, `duration` and
`relative_date`; and `call`. It refuses every other opcode as `unknown_instruction` until a
later version adds it (the retired `and` and `or` as `retired_opcode`). A variable the context does not bind
reads as undefined, and the evaluation fails with an `UndefinedVariableError`
when undefined is its answer or an instruction refuses it; with
`Options::new().on_unbound(OnUnbound::Error)` the load itself fails.

`relative_date` reads the current time from the clock the options name
(`Options::new().clock(|| ...)`, a callable answering an
`Option<DateTime>`), or from the system clock when they name none. A run
reads the clock at most once, the first time the program reads the time, andnever when it does not; a clock that answers `None` fails the run with an
`EvaluationError`, reason `clock_unavailable`.

`call` runs the reference's function library: `len`, `upper`, `lower`,
`trim`, `starts_with`, `ends_with`, `substring`, `index_of` and `concat`;
`Math.pow`, `Math.sqrt`, `Math.abs`, `Math.floor`, `Math.ceil`,
`Math.round`, `Math.min`, `Math.max` and `Math.random`; `Date.year`,
`Date.month`, `Date.day` and `Date.now`; and `JSON.stringify` and
`JSON.parse`. A function that refuses its arguments, an unknown name and a
wrong argument count fail the run with an `EvaluationError` whose reason is
the reference's own message (`Unknown function: x`). A host passes a function
of its own per call, by name:

```rust
use predicator_rs::{Context, Error, Instructions, Options, Value, evaluate};

fn main() -> Result<(), Error> {
    let options = Options::new().function("double", |arguments| match arguments {
        [Value::Integer(x)] => x.checked_mul(2).map(Value::from).ok_or_else(|| "too large".to_owned()),
        _ => Err("double takes one integer".to_owned()),
    });
    let program = Instructions::from_json(r#"[["lit",21],["call","double",1]]"#)?;
    assert_eq!(evaluate(&program, &Context::new(), &options)?, Value::from(42));
    Ok(())
}
```

The function takes the call's arguments and nothing else. An `Err` fails
the run with an `EvaluationError` whose reason and message are the text it
gives; an answer is checked against the value domain. A name passed this
way shadows the library function of that name for the runs those options
are given to. `Math.random` draws from the source `Options::random` names, a
callable answering 64 random bits; without one it draws from a source seeded
from the standard library's per-process hash keys, which needs no
dependency and is not cryptographic. `Date.now` reads the run's clock, as
`relative_date` does.

## Differences from the reference

Where the conformance corpus is silent, the reference at the tag the vendored
corpus names answers. These are the places where this crate's answer differs
from the reference's code, follows the reference's normative text where its
code says otherwise, or has no counterpart in the reference:

- **Integers are 64-bit.** An integer is an `i64`; the reference's integers
  have no bound (ruled by the operator, 2026-11-20). The tagged codec refuses
  an integer outside that range with the reason `integer_out_of_range`,
  rather than reading it as a float. Every integer step is checked: `add`,
  `subtract`, `multiply` or `divide` on two integers whose result is outside
  that range, and `unary_minus` on the least `i64`, fail with an
  `EvaluationError`, reason `integer_overflow`, never a wrapped or rounded
  number. (`modulo` cannot leave the range: the least `i64` modulo `-1` is
  `0`.)
- **A float result that is not finite is refused.** `add`, `subtract`,
  `multiply` or `divide` whose float result overflows fails with an
  `EvaluationError`, reason `non_finite_number`; the reference's code raises
  there.
- **Bracket access on a date or an instant answers undefined, whatever the
  key**, as the reference's instruction-set text states it for a target that
  is neither a map nor a list. The reference's code holds a date as a map, so
  there a key that is not a map key kind (a float, a list) is a
  `TypeMismatchError`.
- **Date arithmetic outside the years this crate holds is refused.** A date
  or an instant moves by a duration as the reference's code moves it (a week
  is 7 days, a month 30 and a year 365; a date moves by whole days, its
  hours, minutes and seconds truncated toward zero and its milliseconds left
  out; an instant moves to the second, and by its milliseconds only when they
  are positive). A result whose year falls outside `i32` fails with an
  `EvaluationError`, reason `date_out_of_range`; the reference's code answers
  a date at any year.
- **Two maps, or two durations, have no order.** `compare` under `GT`, `LT`,
  `GTE` or `LTE` answers undefined for two maps or two durations, and for a
  pair of list members that are two maps, two durations or two of the
  map-like values of different kinds (a map, a duration, a date or an
  instant, a date before an instant excepted). The reference's instruction-set
  text leaves the order of two maps unspecified for an implementation other
  than its own; its code orders them by its runtime's term order. Equality
  (`EQ`, `NE`, `STRICT_EQ`, `STRICT_NE`) on maps and durations is defined.
- **A duration and a map are different types to `compare`**, so `EQ`, `NE`
  and the ordering operators answer undefined for that pair, as for any pair
  of different types. The reference's code holds a duration as a map and
  answers `false` for `EQ`.
- **An instruction list read from JSON must be a list.**
  `Instructions::from_json` refuses any other JSON value with a
  `ParseError`, reason `not_an_instruction_list`; the reference's evaluator
  raises on an argument that is not a list.
- **A map key is a string, an integer, a boolean or undefined** (`Key`), the
  four kinds the reference's instruction-set text lets index a map; `null` is
  not one. The tagged encoding has no form for a key that is not a string (a
  JSON object's keys are strings), so the codec refuses one with the reason
  `non_string_map_key`. The reference's codec writes such a key as its text
  (`1` as `"1"`), which reads back as a string key.
- **A duration is one kind of value**: the map of its eight units, `years`
  through `milliseconds`, the same whether a literal, an instruction or the
  host made it, as the reference's instruction-set text states it. A unit may
  be negative. `access` and `bracket_access` read a unit by its name from any
  duration, and `add` and `subtract` move a date by any duration; the
  reference's code reads a unit only from a duration the host supplied, and
  moves a date only by one a literal or an instruction made.
- **`relative_date` takes a duration and moves by all of it.** It moves the
  current time by any duration exactly as `add` and `subtract` move an
  instant, and a value on the stack that is not a duration (a map included)
  fails with an `EvaluationError`, reason `invalid_stack_value`, as the
  reference's instruction-set text states it. The reference's code takes any
  map there, and moves by nothing for a duration the host supplied.
- **The clock is passed per call and read once per run.** The reference's
  code reads the system clock at each `relative_date`; here the clock is the
  one `Options` names (the system clock by default), read the first time a
  run reads the time, and every later read in that run sees the same
  instant. A clock with no time to give fails the run with the reason
  `clock_unavailable`, which the reference has no counterpart for.
- **`object_set` writes into a map and nothing else.** A date, an instant or
  a duration beneath the value fails with an `EvaluationError`, reason
  `invalid_stack_value`, as the reference's instruction-set text states it
  for any target that is not a map. The reference's code holds those three
  as maps and writes the key into them.
- **A duration's text writes a negative unit with its sign** (`-3d`). The
  reference's instruction-set text leaves out only a zero unit; its code
  leaves out a negative one too.
- **Two instants are equal when they are the same instant**, at any depth of
  a list or a map. The reference's code compares its instant structs, which
  can record a different precision for the same instant.
- **A float that is not finite is refused** where a value crosses into the
  crate, with the reason `non_finite_number`; the reference's floats cannot
  hold one.- **`index_of` counts grapheme clusters**, as `len` and `substring` do: it
  answers the index of the first cluster of the first occurrence that is a
  run of whole clusters (an occurrence starting or ending inside a cluster
  is passed over), or `-1`. The reference's code answers a byte offset there
  while it counts `len` and `substring` in clusters. Clusters are the
  extended grapheme clusters of the Unicode version `unicode-segmentation`
  implements.
- **A function result outside `i64` is refused.** `Math.pow` on two
  integers, `Math.abs` of the least `i64`, and `Math.floor`, `Math.ceil` or
  `Math.round` of a float past the `i64` range fail with an
  `EvaluationError`, reason `integer_overflow`; the reference's code answers
  an integer of any size. `Math.floor` and `Math.ceil` read an integer as a
  float first, as the reference's code does, so an integer past 2^53 may
  answer its nearest float's value, and `i64::MAX` overflows.
- **A function float that is not finite is refused.** `Math.pow` whose
  answer is not a finite float (`Math.pow(0, -1)`, `Math.pow(-8, 0.5)`, an
  overflow) fails with an `EvaluationError`, reason `non_finite_number`;
  the reference's code raises there.
- **`Math.random` draws from a source passed per call** (`Options::random`)
  and turns its 64 bits into a float in `[0, 1)`, the top 53 bits over 2^53;
  the reference's code draws from its runtime's process-wide generator.
- **A host function takes its arguments only**, with no arity of its own
  (ruled by the operator, 2026-11-20). The reference's functions also
  receive the run's context, and a host's function there declares an arity
  the evaluator checks.
- **`JSON.stringify` writes what the reference's encoder writes** (keys in
  the reference's order, integers first, then `false`, `true` and
  undefined as text, then strings; a date and an instant as their text,
  undefined as `"undefined"`, a duration as the map of its eight units) and
  refuses only a value nested past 100 levels, with the reason
  `depth_limit_exceeded`; the reference's code never refuses.
- **`JSON.parse` reports the reference's fault text** for a byte its reader
  refuses and for an early end (`Invalid JSON: unexpected byte 0x6F at
  position 1`, as the reason). An integer outside `i64` fails with the
  reason `integer_out_of_range`, a number too large for a finite float with
  `non_finite_number`, and a text nested past 100 levels with
  `depth_limit_exceeded`, where the reference answers a value; a malformed
  `\u` escape is refused with this crate's own text, where the reference
  writes its runtime's error term.
- **Values are bounded at 100 levels.** Lists and maps nested past 100 levels
  are refused where a value crosses into the crate and by the tagged codec,
  with the reason `depth_limit_exceeded`; the reference has no bound. A
  deeper value the host builds is still compared, copied and dropped without
  recursion.
