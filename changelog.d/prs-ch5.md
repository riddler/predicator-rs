### Added

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
