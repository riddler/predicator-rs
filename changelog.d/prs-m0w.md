### Added

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
