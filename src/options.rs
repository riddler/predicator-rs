//! The options one evaluation is run with, built per call.

/// What a `load` of a variable the context does not bind does.
///
/// ```
/// use predicator_rs::OnUnbound;
///
/// assert_eq!(OnUnbound::default(), OnUnbound::Undefined);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum OnUnbound {
    /// The load pushes undefined, and the evaluation fails with an
    /// [`UndefinedVariableError`](crate::UndefinedVariableError) only when
    /// undefined is its answer, or when an instruction refuses that
    /// undefined as an operand. The default.
    #[default]
    Undefined,
    /// The load itself fails the evaluation with an
    /// [`UndefinedVariableError`](crate::UndefinedVariableError).
    Error,
}

/// The options an evaluation is run with, built per call: nothing is
/// configured globally.
///
/// ```
/// use predicator_rs::{OnUnbound, Options};
///
/// let strict = Options::new().on_unbound(OnUnbound::Error);
/// # let _ = strict;
/// ```
#[derive(Debug, Default)]
pub struct Options {
    pub(crate) on_unbound: OnUnbound,
}

impl Options {
    /// The default options.
    pub fn new() -> Options {
        Options::default()
    }

    /// The same options with `policy` as what a load of an unbound
    /// variable does.
    #[must_use]
    pub fn on_unbound(mut self, policy: OnUnbound) -> Options {
        self.on_unbound = policy;
        self
    }
}
