### Added

- `Value`, the predicator value: null, undefined, a boolean, an `i64`
  integer, an `f64` float, a string, a list, a `Map`, a `Date`, a
  `DateTime` (an instant in UTC to the microsecond) and a `Duration`,
  compared, ordered, cloned and dropped without recursion.
- `Key`, a map's key: a string, an integer, a boolean or undefined, in a
  stated total order.
- `Error`, every entry point's error, over `ParseError`, `EvaluationError`,
  `TypeMismatchError` and `UndefinedVariableError`, each with its reason, a
  message and a `Position` where one is known.
- `tagged::encode` and `tagged::decode` for the conformance corpus's
  tagged-value encoding, refusing an integer outside the `i64` range, a
  float that is not finite, a value nested past 100 levels and a map key
  that is not a string by name.
- The `serde` feature implements `Serialize` and `Deserialize` for `Value`
  (in the tagged encoding), `Date`, `DateTime` and `Duration`.
