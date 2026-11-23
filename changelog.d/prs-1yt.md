### Added

- `parse_duration` reads a duration from its literal spelling (`"1h30m"`),
  answering the `Duration` or an `EvaluationError`, reason
  `invalid_duration_format`.
- `ISA_VERSION` names the version of the instruction set the crate runs.

### Changed

- `relative_date` reads a plain map as the duration its unit names spell, a
  unit it leaves out as 0, where it refused every map before.
- `object_set` on a duration answers the plain map of its eight units with
  the key written, where it refused a duration before.
- A units map (one carrying `years` through `seconds` under their string
  names, each an integer) casts as a duration: it passes through
  `::duration` unchanged and `::string` writes it in the duration-literal
  grammar, where both answered undefined before.
- Breaking: a clock that answers `None` fails the run with the reason
  `clock_failed`, renamed from `clock_unavailable`; a caller matching the
  reason matches the new name.
- Breaking: `OnUnbound` is `non_exhaustive`, so a `match` on it needs a
  wildcard arm.
- `EvaluationError` alone carries the root a refusal names; the other three
  error types no longer hold an empty root field in their `Debug` output.
