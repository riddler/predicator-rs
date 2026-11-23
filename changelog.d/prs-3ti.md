### Added

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
