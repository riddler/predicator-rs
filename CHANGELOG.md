# Changelog

## 0.1.0 / 2026-11-23

The first release: the Rust port of the predicator condition language. It compiles a condition's source, an expression or a statement program, to the reference's instruction set and runs it on a stack machine against a `Context`, with host functions, the clock and the random source passed per call in `Options`; nothing is configured globally, no source, instruction list or context makes it panic, and it forbids `unsafe` code. The minimum supported Rust version is 1.85 (edition 2024). The one required runtime dependency is `unicode-segmentation`; `serde` is an optional feature, off by default. It is checked against the conformance corpus vendored from riddler/predicator-ex at the tag `v9.6.0`, instruction-set version 7 (`conformance/SOURCE.json` records the tag, its commit and the corpus hash), which is also the newest predicator-ex tag on this date. `conformance/registry.json` claims tier 10 on both surfaces, the evaluator and the compiler, and every case on them passes with no skip list; a run that claims version 7 does not attempt the evaluator cases tagged `retired`. Where the corpus is silent the crate follows the reference at that tag, and the README's "Differences from the reference" section states where it answers otherwise: integers are `i64` and an overflow is refused, a raise in the reference is a named refusal here, a source nests 400 levels at most, a host function takes its arguments only, `execute` answers the roots a run stored into, the clock and the random source are passed per call, and where the reference's code and its instruction-set text differ the text is followed. The 0.0.0 release on crates.io is a placeholder that reserved the name and holds no library.

**Added**

- `parse_duration` reads a duration from its literal spelling (`"1h30m"`),
  answering the `Duration` or an `EvaluationError`, reason
  `invalid_duration_format`.
- `ISA_VERSION` names the version of the instruction set the crate runs.
- `relative_date` takes a plain map as well as a duration, reading it as
  the duration its unit names spell, a unit it leaves out as 0.
- `object_set` on a duration answers the plain map of its eight units with
  the key written.
- A units map (one carrying `years` through `seconds` under their string
  names, each an integer) casts as a duration: it passes through
  `::duration` unchanged and `::string` writes it in the duration-literal
  grammar.
- `Value`, `Key`, `Error` and `OnUnbound` are `non_exhaustive`, so a
  `match` on one needs a wildcard arm.
- `evaluate` runs the arithmetic instructions `add`, `subtract`,
  `multiply`, `divide` and `modulo`: two integers answer an integer and a
  float on either side a float, `add` also joins strings, a number beside a
  string and two lists, and a date or an instant moves by a duration. An
  integer result outside `i64` fails with the reason `integer_overflow`, a
  float result that is not finite with `non_finite_number`, a zero divisor
  with `division_by_zero` or `modulo_by_zero`, and a date moved outside the
  years the crate holds with `date_out_of_range`.
- `evaluate` runs `access` and `bracket_access`, which read a map's key, a
  list's index or a duration's unit and answer undefined for a miss; a key
  `bracket_access` cannot index with is a `TypeMismatchError`.
- `evaluate` runs `in` and `contains`, which test a list for a member and a
  string for a substring, and answer undefined for any other pairing, and
  `make_list`, which gathers values from the stack into a list.
- `evaluate`, `execute` and `execute_value` run `cast`, converting a value
  to `integer`, `float`, `string`, `boolean`, `date`, `datetime` or
  `duration` as the reference's conversion matrix states, and answering
  undefined where no conversion exists.
- They run `presence`, which answers `x is blank` and `x is defined` for
  every value, a blank string decided by the Unicode White_Space code
  points the reference lists, and `intersects`, which answers whether two
  lists share a member.
- A cast whose answer is an integer outside `i64` fails with the reason
  `integer_overflow`, a string too large for a finite float cast to
  `float` with `non_finite_number`, and an instant outside the years 0 to
  9999 cast to `string` with `date_out_of_range`.
- `evaluate` runs `object_new` and `object_set`, which build a map key by
  key; a target that is not a map fails with the reason
  `invalid_stack_value`.
- `evaluate` runs `duration`, which builds a duration from unit pairs
  (`[[2, "w"], [3, "d"]]`), and `relative_date`, which moves the current
  time back (`ago`, `last`) or forward (`future`, `next`) by a duration.
- `Options::clock` names the clock a run reads the current time from, a
  callable answering an `Option<DateTime>`; without one the run reads the
  system clock. A run reads the clock at most once, and only when the
  program reads the time; a clock that answers `None` fails the run with
  the reason `clock_failed`.
- `compile` turns an expression's source into the instruction list the
  reference's compiler emits for it, and `compile_program` a statement
  program's (assignments, `if`, `while` and expressions separated by `;`);
  each answers `Instructions` or a `ParseError` with the reference's
  message and the position of the token it names.
- `Instructions::to_json` writes an instruction list in the tagged
  encoding that `Instructions::from_json` reads, with no feature enabled.
- `evaluate` takes a source in place of an instruction list and compiles
  it as an expression, and `execute` and `execute_value` take one and
  compile it as a statement program; a source that does not compile is
  refused before anything runs.
- `Value`, the predicator value: null, undefined, a boolean, an `i64`
  integer, an `f64` float, a string, a list, a `Map`, a `Date`, a
  `DateTime` (an instant in UTC to the microsecond) and a `Duration`,
  compared, ordered, cloned and dropped without recursion.
- `Key`, a map's key: a string, an integer, a boolean or undefined, in a
  stated total order.
- `Error`, the error every entry point answers (or, from `execute` and
  `execute_value`, the `Stopped` that holds it), over `ParseError`,
  `EvaluationError`, `TypeMismatchError` and `UndefinedVariableError`, each
  with its reason, a message and a `Position` where one is known.
- `tagged::encode` and `tagged::decode` for the conformance corpus's
  tagged-value encoding, refusing an integer outside the `i64` range, a
  float that is not finite, a value nested past 100 levels and a map key
  that is not a string by name.
- The `serde` feature implements `Serialize` and `Deserialize` for `Value`
  (in the tagged encoding), `Date`, `DateTime` and `Duration`.
- `evaluate` runs `call`, with the reference's function library: the
  string functions `len`, `upper`, `lower`, `trim`, `starts_with`,
  `ends_with`, `substring` and `index_of` (counted in grapheme clusters),
  `concat`, the `Math.` functions, `Date.year`, `Date.month`, `Date.day`,
  `Date.now`, `JSON.stringify` and `JSON.parse`. A function's refusal, an
  unknown name and a wrong argument count fail with an `EvaluationError`
  whose reason is the reference's message.
- `Options::function` passes a host function in by name for the runs those
  options are given to: it takes the call's arguments, answers a value
  (checked against the value domain) or refuses with a message, and shadows
  the library function of that name.
- `Options::random` names the source `Math.random` draws from, a callable
  answering 64 random bits; without one a run draws from a source that needs
  no dependency and is not cryptographic.
- `evaluate` runs `store`, `pop`, `jump`, `pop_jump_if_falsy` and
  `jump_backward`: a `store` writes a copy of the context that later
  instructions read, and the context passed in is never changed.
- `execute` runs an instruction list as a statement program and answers an
  `Execution`: the context it leaves and `stored_roots`, the roots it stored
  into in the order of its first store. `execute_value` also answers the
  value of the last expression statement (`Execution::value`).
- A statement program that stops answers a `Stopped`: the error, the
  context as of the last statement it completed and the roots stored so far.
  It converts into its `Error`, so `?` takes it.
- `Options::loop_budget` sets how many back edges a run may take, 10,000 by
  default; a run that has spent them fails at the next with the reason
  `loop_budget_exceeded`.
- `Options::protected_roots` names roots a statement program may not store
  into; such a store fails with the reason `protected_root`, and
  `EvaluationError::root` names the root.
- `Context::get` reads the value bound to a key, so a caller can read the
  context a statement program leaves.
- `evaluate` runs an instruction list over a `Context` with `Options` given
  per call and answers the `Value` it leaves on top of the stack, running the
  instruction set's first tier: `lit`, `load`, `compare`, `not`,
  `unary_bang`, `unary_minus`, `jump_if_falsy_or_pop` and
  `jump_if_true_or_pop`. An opcode the instruction set does not name, or
  one given operands it does not take, is refused as
  `unknown_instruction`, and the retired `and` and `or` as
  `retired_opcode`.
- `Instructions::from_json` reads an instruction list from JSON in the
  tagged encoding, refusing a value that is not a list as
  `not_an_instruction_list`.
- `Context` binds root variables to values; a bound value outside the value
  domain is refused when an evaluation starts.
- `Options` and `OnUnbound`: a variable the context does not bind reads as
  undefined and fails the evaluation with an `UndefinedVariableError` when
  undefined is its answer or an instruction refuses it (`OnUnbound::Undefined`,
  the default), or fails the evaluation at the load (`OnUnbound::Error`).
