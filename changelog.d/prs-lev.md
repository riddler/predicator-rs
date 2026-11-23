### Added

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
