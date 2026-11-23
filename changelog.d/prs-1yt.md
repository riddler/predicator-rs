### Added

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
