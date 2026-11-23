# predicator-rs

A Rust port of the [predicator](https://github.com/riddler/predicator-ex)
condition language. A condition is a short expression such as
`score > 10 and status in ["open", "held"]`. The crate will compile it to a
small instruction set and evaluate it on a stack machine, with the same
answers as the reference implementation on its shared conformance corpus.

The published 0.0.0 is a placeholder that reserves the crate name; the first
usable version is 0.1.0. What the source holds so far is the value model
(`Value` and its types), the errors, the codec for the conformance corpus's
tagged-value encoding (`tagged`), and the evaluator:
`evaluate` runs an instruction list (`Instructions`, read from JSON in the
tagged encoding) over a `Context`, with `Options` passed per call. It runs
every tier of the instruction set: `lit`, `load`, `compare`, `not`,
`unary_bang`, `unary_minus`, `jump_if_falsy_or_pop` and
`jump_if_true_or_pop`; the arithmetic `add`, `subtract`, `multiply`,
`divide` and `modulo`; `access`, `bracket_access`, `in`, `contains` and
`make_list`; `object_new`, `object_set`, `duration` and
`relative_date`; `call`; `store` and `pop`; `cast`; `jump` and
`pop_jump_if_falsy`; `jump_backward`; and `presence` and `intersects`. It
refuses every other opcode as `unknown_instruction` (the retired `and` and
`or` as `retired_opcode`). A variable the context does not bind
reads as undefined, and the evaluation fails with an `UndefinedVariableError`
when undefined is its answer or an instruction refuses it; with
`Options::new().on_unbound(OnUnbound::Error)` the load itself fails.

`relative_date` reads the current time from the clock the options name
(`Options::new().clock(|| ...)`, a callable answering an
`Option<DateTime>`), or from the system clock when they name none. A run
reads the clock at most once, the first time the program reads the time, and
never when it does not; a clock that answers `None` fails the run with an
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

## Casts, presence and overlap

`cast` converts a value to one of the seven type names (`integer`, `float`,
`string`, `boolean`, `date`, `datetime`, `duration`) as the reference's
conversion matrix states it: undefined casts to undefined, and a value that
cannot become one of the target type, `null`, a list and a map included,
casts to undefined rather than failing. A string is read whole, with no
trimming: an integer as `-?[0-9]+`, a float as `-?[0-9]+(\.[0-9]+)?`, a
boolean as exactly `true` or `false`, a date as `YYYY-MM-DD`, an instant as
an ISO 8601 datetime (a `T` before the time, a fraction after a full stop
or a comma, read to the microsecond, and an offset of `Z`,
`+HH:MM`, `+HHMM` or `+HH`, either sign), and a duration as the
duration-literal grammar, fractions included (`"1.5s"` is a second and 500
milliseconds). A float is written as the reference writes one (`1.0e16`,
`0.0001`), and an instant in UTC with its fraction left out when it is zero
and six digits when it is not.

`presence` answers `x is blank` (`["presence","blank"]`) and `x is defined`
(`["presence","defined"]`) for every value and never fails. Blank is
undefined, `null`, an empty list, a map with no keys, and a string that is
empty or holds only the Unicode White_Space code points the reference lists
(U+0009 to U+000D, U+0020, U+0085, U+00A0, U+1680, U+2000 to U+200A,
U+2028, U+2029, U+202F, U+205F and U+3000), matched code point by code point
rather than through Rust's own trim; a zero-width space (U+200B) is not one
of them. Every other value, `0`, `false` and a zero duration included, is
not blank. `intersects` answers whether two lists share a member under the
equality `in` uses, and undefined when either operand is not a list.

## Statement programs

`execute` runs an instruction list as a statement program: its `store`s
write the context, and its answer is the context it leaves rather than a
value. The context passed in is never changed; a run writes a copy. A run
that completes answers an `Execution`; one that stops answers a `Stopped`,
which holds the error it stopped on, the context as of the last statement
it completed (every earlier write kept, the failing write not made) and the
roots it had stored into. Keeping that context or the one passed in is the
caller's choice. A `Stopped` converts into its `Error`, so `?` takes it
where only the error matters.

```rust
use predicator_rs::{Context, Error, Instructions, Key, Options, Value, execute};

fn main() -> Result<(), Error> {
    // x = 1; y = x + 2
    let program = Instructions::from_json(
        r#"[["lit","x"],["lit",1],["store",1],["lit","y"],["load","x"],["lit",2],["add"],["store",1]]"#,
    )?;
    let execution = execute(&program, &Context::new(), &Options::new())?;
    assert_eq!(execution.context().get(&Key::from("y")), Some(&Value::from(3)));
    assert_eq!(execution.stored_roots(), [Key::from("x"), Key::from("y")]);

    // a = 1; a.b = 2 stops at the second statement, keeping the first.
    let program = Instructions::from_json(
        r#"[["lit","a"],["lit",1],["store",1],["lit","a"],["lit","b"],["lit",2],["store",2]]"#,
    )?;
    let stopped = execute(&program, &Context::new(), &Options::new()).unwrap_err();
    assert_eq!(stopped.error().reason(), "not_a_container");
    assert_eq!(stopped.context().get(&Key::from("a")), Some(&Value::from(1)));
    Ok(())
}
```

`execute_value` does the same and also answers the value of the program's
last expression statement (`Execution::value`): what its last `pop`
discarded, or undefined when it popped nothing.

`store` writes as the reference's instruction-set text states it: an
interior slot that is missing, `null` or undefined becomes a list when the
segment after it is an integer and a map otherwise; an index past a list's
end pads it with undefined; the leaf is overwritten. An integer segment
into a map is an integer key. A path through a scalar, a date or an
instant is refused as `not_a_container`, a negative index as
`invalid_index`, and an empty path as `not_assignable`; a segment that is
neither a string nor an integer is a `TypeMismatchError`. A write that is
refused leaves nothing of itself behind.

`Options::new().protected_roots(["locked"])` names roots a statement
program may not store into: a `store` whose path's root is one of them is
refused when it is reached, with an `EvaluationError`, reason
`protected_root`, whose `root()` names the root. A root is protected whole,
and the check comes after the path's segments are checked, as the
reference makes it.

A run may take as many back edges (`jump_backward`) as its loop budget
allows, 10,000 unless `Options::new().loop_budget(..)` says otherwise; a
run that reaches a back edge with none left fails there with an
`EvaluationError`, reason `loop_budget_exceeded`. The budget bounds
`evaluate` and `execute` alike.

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
- **A cast whose answer is an integer outside `i64` is refused.** A string
  or a float cast to `integer`, and a string cast to `duration` with a unit
  past that range, fail with an `EvaluationError`, reason
  `integer_overflow`; the reference's code answers an integer of any size
  there.
- **A string too large for a finite float casts to a refusal.** `::float`
  on such a string fails with an `EvaluationError`, reason
  `non_finite_number`; the reference's code raises there.
- **An instant outside the years 0 to 9999 casts to no text.** `::string`
  on such an instant (one date arithmetic can make) fails with an
  `EvaluationError`, reason `date_out_of_range`, and `::datetime` on a date
  outside those years is undefined, as the reference's instruction-set text
  states that nothing outside them is an instant. The reference's code makes
  the instant, raises when it writes one past 9999 or before -9999 as text,
  and writes one between as text with a minus sign.
- **`::date` and `::datetime` read a four-digit year only**, the calendar
  date and the ISO 8601 datetime the reference's instruction-set text
  names; the reference's code also reads a year with a minus sign
  (`-0005-01-01`) or a plus sign (`+2026-08-09`,
  `+2026-08-09T10:00:00Z`).
- **`::datetime` refuses a space before the time.** The reference's
  instruction-set text names an ISO 8601 datetime, which separates the date
  from the time with a `T`; the reference's code also reads a space there
  (`2026-08-09 10:00:00Z`), and this crate's tagged codec still does.
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
  hold one.
- **`index_of` counts grapheme clusters**, as `len` and `substring` do: it
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
- **Values are bounded at 100 levels.** Lists and maps nested past 100 levels
  are refused where a value crosses into the crate and by the tagged codec,
  with the reason `depth_limit_exceeded`; the reference has no bound. A
  deeper value the host builds is still compared, copied and dropped without
  recursion.
- **`execute` answers the roots a run stored into** (`stored_roots`), each
  once in the order of its first store, whether or not a store changed the
  value there. The reference has no such answer; its callers compare the
  context before and after. `Execution::value` is `None` from `execute`
  and the last expression statement's value from `execute_value`.
- **`evaluate` does not read the protected roots.** A `store` in an
  instruction list `evaluate` runs writes whatever its root; the
  reference's documentation says the option matters to its statement entry
  point alone, while its code reads it in an instruction list its
  expression entry point runs too.
- **A store through a date or an instant is refused** with the reason
  `not_a_container`, as the reference's instruction-set text states it for
  an interior slot that holds a scalar; the reference's code holds those
  two as maps and writes the key into them.
- **A store through a duration writes into the plain map of its eight
  units**, so the slot holds a map afterwards, whatever the key. The
  reference's code writes the key into the duration's own map.
- **A store makes a list at most 1,048,576 (2^20) items long.** An index
  past a list's end at or beyond that is refused with an `EvaluationError`,
  reason `list_too_long`; a list already longer may be written inside its
  end. The reference pads to any index.
