### Added

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
