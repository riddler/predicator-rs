### Added

- `Value`, the predicator value: null, undefined, a boolean, an `i64`
  integer, an `f64` float, a string, a list, a `Map` keyed by strings, a
  `Date`, a `DateTime` (an instant in UTC to the microsecond) and a
  `Duration`, compared, ordered, cloned and dropped without recursion.
- `Error`, every entry point's error, over `ParseError`, `EvaluationError`,
  `TypeMismatchError` and `UndefinedVariableError`, each with its reason, a
  message and a `Position` where one is known.
- `tagged::encode` and `tagged::decode` for the conformance corpus's
  tagged-value encoding, refusing an integer outside the `i64` range, a
  float that is not finite and a value nested past 100 levels by name.
- The `serde` feature implements `Serialize` and `Deserialize` for `Value`
  (in the tagged encoding), `Date`, `DateTime` and `Duration`.
