//! The context an evaluation reads its variables from.

use crate::{EvaluationError, Key, Map, Value, boundary};

/// The variables an evaluation reads: each root name bound to a value. A
/// `load` reads a root by its string name; a name the context does not bind
/// is unbound, which is not the same as a name bound to
/// [`Value::Undefined`].
///
/// Every bound value is checked against the value domain when an
/// evaluation starts: a float that is not finite (`non_finite_number`) and
/// lists and maps nested past 100 levels (`depth_limit_exceeded`) are
/// refused with an [`EvaluationError`].
///
/// ```
/// use predicator_rs::{Context, Map, Value};
///
/// let mut context = Context::new();
/// context.insert("score", Value::from(90));
/// let bindings: Map = [("score", Value::from(90))].into_iter().collect();
/// assert_eq!(context, Context::from(bindings));
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Context {
    bindings: Map,
}

impl Context {
    /// An empty context.
    pub fn new() -> Context {
        Context::default()
    }

    /// Binds `name` to `value`, answering the value it replaces.
    pub fn insert(&mut self, name: impl Into<Key>, value: Value) -> Option<Value> {
        self.bindings.insert(name, value)
    }

    /// The value bound to the root `name`, if the context binds it.
    pub(crate) fn root(&self, name: &str) -> Option<&Value> {
        self.bindings.get_str(name)
    }

    /// The first bound value outside the value domain, refused.
    pub(crate) fn check(&self) -> Result<(), EvaluationError> {
        self.bindings.values().try_for_each(boundary::check)
    }
}

impl From<Map> for Context {
    /// The context binding each key of `bindings` to its value.
    fn from(bindings: Map) -> Context {
        Context { bindings }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Sabotage: with root looking the name up as Key::Integer(0), this test
    // fails.
    #[test]
    fn a_context_binds_roots_by_name_and_tells_unbound_from_undefined() {
        let mut context = Context::new();
        assert_eq!(context.insert("x", Value::from(1)), None);
        assert_eq!(context.insert("x", Value::from(2)), Some(Value::from(1)));
        context.insert("u", Value::Undefined);
        assert_eq!(context.root("x"), Some(&Value::from(2)));
        assert_eq!(context.root("u"), Some(&Value::Undefined));
        assert_eq!(context.root("y"), None);
        assert!(context.check().is_ok());
    }

    // Sabotage: with check answering Ok(()) without a walk, this test fails.
    #[test]
    fn a_bound_value_outside_the_domain_is_refused() {
        let mut context = Context::new();
        context.insert("ok", Value::from(1));
        context.insert("x", Value::List(vec![Value::Float(f64::NAN)]));
        assert_eq!(
            context.check().map_err(|error| error.reason().to_owned()),
            Err("non_finite_number".to_owned())
        );
    }
}
