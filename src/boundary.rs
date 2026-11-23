//! The value boundary: where a value crosses into the crate (a value a host
//! builds, a value about to be written as text) it is checked against the
//! value domain. Owned values cannot be cyclic, so two faults remain.

use crate::{EvaluationError, Value};

/// How deep lists and maps may nest in a value that crosses the boundary,
/// the outermost counting as the first level.
pub(crate) const DEPTH_LIMIT: usize = 100;

/// The first fault of `value`, walked without recursion, or `Ok` when it is
/// in the domain:
///
/// - `non_finite_number`: a float that is not finite;
/// - `depth_limit_exceeded`: lists and maps nested past
///   [`DEPTH_LIMIT`] levels.
pub(crate) fn check(value: &Value) -> Result<(), EvaluationError> {
    let mut pending = vec![(value, 1)];
    while let Some((value, depth)) = pending.pop() {
        match value {
            Value::Float(number) if !number.is_finite() => {
                return Err(EvaluationError::new(
                    "non_finite_number",
                    format!("the float {number} is not finite"),
                ));
            }
            Value::List(_) | Value::Map(_) if depth > DEPTH_LIMIT => {
                return Err(EvaluationError::new(
                    "depth_limit_exceeded",
                    format!("the value nests past {DEPTH_LIMIT} levels"),
                ));
            }
            Value::List(items) => pending.extend(items.iter().map(|item| (item, depth + 1))),
            Value::Map(map) => pending.extend(map.values().map(|item| (item, depth + 1))),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Map;

    fn nested(levels: usize) -> Value {
        let mut value = Value::Integer(0);
        for level in 0..levels {
            value = if level % 2 == 0 {
                Value::List(vec![value])
            } else {
                Value::Map([("k", value)].into_iter().collect::<Map>())
            };
        }
        value
    }

    // Sabotage: with the depth test written depth > DEPTH_LIMIT + 1, this
    // test fails.
    #[test]
    fn the_boundary_refuses_past_one_hundred_levels() {
        assert!(check(&nested(100)).is_ok());
        let refused = check(&nested(101)).unwrap_err();
        assert_eq!(refused.reason(), "depth_limit_exceeded");
        assert!(check(&nested(0)).is_ok());
    }

    // Sabotage: with the float arm matching only NaN, this test fails.
    #[test]
    fn the_boundary_refuses_a_float_that_is_not_finite() {
        for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let inside = Value::List(vec![Value::Map(
                [("k", Value::Float(number))].into_iter().collect(),
            )]);
            assert_eq!(check(&inside).unwrap_err().reason(), "non_finite_number");
        }
        assert!(check(&Value::Float(f64::MAX)).is_ok());
    }
}
