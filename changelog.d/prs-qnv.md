### Added

- `evaluate` runs an instruction list over a `Context` with `Options` given
  per call and answers the `Value` it leaves on top of the stack, running the
  instruction set's first tier: `lit`, `load`, `compare`, `not`,
  `unary_bang`, `unary_minus`, `jump_if_falsy_or_pop` and
  `jump_if_true_or_pop`. Any other opcode is refused as
  `unknown_instruction`, and the retired `and` and `or` as `retired_opcode`.
- `Instructions::from_json` reads an instruction list from JSON in the
  tagged encoding, refusing a value that is not a list as
  `not_an_instruction_list`.
- `Context` binds root variables to values; a bound value outside the value
  domain is refused when an evaluation starts.
- `Options` and `OnUnbound`: a variable the context does not bind reads as
  undefined and fails the evaluation with an `UndefinedVariableError` when
  undefined is its answer or an instruction refuses it (`OnUnbound::Undefined`,
  the default), or fails the evaluation at the load (`OnUnbound::Error`).
