### Added

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
  the reason `clock_unavailable`.
