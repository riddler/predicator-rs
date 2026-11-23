//! The errors the crate answers: one enum over four error types (a parse
//! error and the three evaluation error types the conformance corpus names),
//! each carrying its reason, a message and, where one is known, a position
//! in the source.

use core::fmt;

/// A place in source text: a line and a column, both counted from 1.
///
/// The column counts characters (Unicode scalar values) from the start of
/// the line.
///
/// ```
/// let position = predicator_rs::Position { line: 1, column: 7 };
/// assert_eq!(position.line, 1);
/// assert_eq!(position.column, 7);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Position {
    /// The line, counted from 1.
    pub line: usize,
    /// The column, counted from 1.
    pub column: usize,
}

/// Declares one of the four error types: a reason, a message and an optional
/// position, with the same methods on each, and any further optional fields
/// one type alone carries (each `None` until that type's own method sets it).
macro_rules! error_type {
    ($(#[$doc:meta])* $name:ident, $variant:ident $(, $field:ident: $type:ty)*) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $name {
            reason: String,
            message: String,
            position: Option<Position>,
            $($field: Option<$type>,)*
        }

        impl $name {
            /// An error with a reason (the corpus's token) and a message (a
            /// sentence for a person), at no position.
            ///
            /// ```
            #[doc = concat!("use predicator_rs::{", stringify!($name), ", Position};")]
            ///
            #[doc = concat!("let error = ", stringify!($name), "::new(\"some_reason\", \"a sentence\");")]
            /// assert_eq!(error.reason(), "some_reason");
            /// assert_eq!(error.position(), None);
            /// ```
            pub fn new(reason: impl Into<String>, message: impl Into<String>) -> Self {
                Self {
                    reason: reason.into(),
                    message: message.into(),
                    position: None,
                    $($field: None,)*
                }
            }

            /// The same error at `position`.
            ///
            /// ```
            #[doc = concat!("use predicator_rs::{", stringify!($name), ", Position};")]
            ///
            #[doc = concat!("let error = ", stringify!($name), "::new(\"some_reason\", \"a sentence\");")]
            /// let at = Position { line: 2, column: 3 };
            /// assert_eq!(error.with_position(at).position(), Some(at));
            /// ```
            #[must_use]
            pub fn with_position(mut self, position: Position) -> Self {
                self.position = Some(position);
                self
            }

            /// The reason: the structured token the corpus names, which is
            /// normative.
            ///
            /// ```
            #[doc = concat!("use predicator_rs::{", stringify!($name), ", Position};")]
            ///
            #[doc = concat!("let error = ", stringify!($name), "::new(\"some_reason\", \"a sentence\");")]
            /// assert_eq!(error.reason(), "some_reason");
            /// ```
            pub fn reason(&self) -> &str {
                &self.reason
            }

            /// The message: a sentence for a person, which is not normative.
            ///
            /// ```
            #[doc = concat!("use predicator_rs::{", stringify!($name), ", Position};")]
            ///
            #[doc = concat!("let error = ", stringify!($name), "::new(\"some_reason\", \"a sentence\");")]
            /// assert_eq!(error.message(), "a sentence");
            /// assert_eq!(error.to_string(), "a sentence");
            /// ```
            pub fn message(&self) -> &str {
                &self.message
            }

            /// Where in the source the failure is, when that is known.
            ///
            /// ```
            #[doc = concat!("use predicator_rs::{", stringify!($name), ", Position};")]
            ///
            #[doc = concat!("let error = ", stringify!($name), "::new(\"some_reason\", \"a sentence\");")]
            /// assert_eq!(error.position(), None);
            /// let at = Position { line: 1, column: 1 };
            /// assert_eq!(error.with_position(at).position(), Some(at));
            /// ```
            pub fn position(&self) -> Option<Position> {
                self.position
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.message)
            }
        }

        impl std::error::Error for $name {}

        impl From<$name> for Error {
            fn from(error: $name) -> Error {
                Error::$variant(error)
            }
        }
    };
}

error_type! {
    /// A text that does not parse: a source, or a text handed to
    /// [`tagged::decode`](crate::tagged::decode). It knows its position.
    ///
    /// ```
    /// use predicator_rs::{ParseError, Position};
    ///
    /// let error = ParseError::new("malformed_json", "the text ends inside a list")
    ///     .with_position(Position { line: 1, column: 3 });
    /// assert_eq!(error.reason(), "malformed_json");
    /// assert_eq!(error.position(), Some(Position { line: 1, column: 3 }));
    /// ```
    ParseError, Parse
}

error_type! {
    /// An evaluation failure that is neither a type mismatch nor an unbound
    /// variable, and a value refused where it crosses into the crate (a
    /// float that is not finite, a value nested too deep).
    ///
    /// ```
    /// use predicator_rs::EvaluationError;
    ///
    /// let error = EvaluationError::new("division_by_zero", "the divisor is zero");
    /// assert_eq!(error.reason(), "division_by_zero");
    /// assert_eq!(error.to_string(), "the divisor is zero");
    /// assert_eq!(error.position(), None);
    /// ```
    EvaluationError, Evaluation, root: String
}

impl EvaluationError {
    /// The context root a refusal names, when it names one: the root a
    /// `store` was refused because the options protect it (reason
    /// `protected_root`; [`Options::protected_roots`](crate::Options::protected_roots)).
    ///
    /// ```
    /// use predicator_rs::EvaluationError;
    ///
    /// assert_eq!(EvaluationError::new("division_by_zero", "the divisor is zero").root(), None);
    /// ```
    pub fn root(&self) -> Option<&str> {
        self.root.as_deref()
    }

    /// The same error naming `root` as the root it refuses.
    pub(crate) fn with_root(mut self, root: impl Into<String>) -> Self {
        self.root = Some(root.into());
        self
    }
}

error_type! {
    /// An operand an operation refused on its type. The reason names the
    /// operation.
    ///
    /// ```
    /// use predicator_rs::TypeMismatchError;
    ///
    /// let error = TypeMismatchError::new("add", "add takes two numbers");
    /// assert_eq!(error.reason(), "add");
    /// ```
    TypeMismatchError, TypeMismatch
}

error_type! {
    /// A variable the context does not bind, reported as an error rather
    /// than as undefined.
    ///
    /// ```
    /// use predicator_rs::UndefinedVariableError;
    ///
    /// let error = UndefinedVariableError::new("unbound_variable", "x is not bound");
    /// assert_eq!(error.reason(), "unbound_variable");
    /// ```
    UndefinedVariableError, UndefinedVariable
}

/// Every error the crate answers. The variant is the error's type, which
/// with the reason is normative where the conformance corpus expects an
/// error (it names the three evaluation types); the message is not.
///
/// ```
/// use predicator_rs::{Error, EvaluationError};
///
/// let error = Error::from(EvaluationError::new("empty_stack", "nothing is left"));
/// assert_eq!(error.type_name(), "EvaluationError");
/// assert_eq!(error.reason(), "empty_stack");
/// assert_eq!(error.message(), "nothing is left");
/// assert_eq!(error.position(), None);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A text that does not parse.
    Parse(ParseError),
    /// An evaluation failure, or a value refused where it crosses in.
    Evaluation(EvaluationError),
    /// An operand refused on its type.
    TypeMismatch(TypeMismatchError),
    /// A variable the context does not bind.
    UndefinedVariable(UndefinedVariableError),
}

impl Error {
    /// The error's type as the corpus spells it: `"ParseError"`,
    /// `"EvaluationError"`, `"TypeMismatchError"` or
    /// `"UndefinedVariableError"`.
    ///
    /// ```
    /// use predicator_rs::compile;
    ///
    /// assert_eq!(compile("x >").unwrap_err().type_name(), "ParseError");
    /// ```
    pub fn type_name(&self) -> &'static str {
        match self {
            Error::Parse(_) => "ParseError",
            Error::Evaluation(_) => "EvaluationError",
            Error::TypeMismatch(_) => "TypeMismatchError",
            Error::UndefinedVariable(_) => "UndefinedVariableError",
        }
    }

    /// The reason: the structured token the corpus names, which is
    /// normative.
    ///
    /// ```
    /// use predicator_rs::{Context, Options, evaluate};
    ///
    /// let refused = evaluate("1 / 0", &Context::new(), &Options::new());
    /// assert_eq!(refused.unwrap_err().reason(), "division_by_zero");
    /// ```
    pub fn reason(&self) -> &str {
        match self {
            Error::Parse(error) => error.reason(),
            Error::Evaluation(error) => error.reason(),
            Error::TypeMismatch(error) => error.reason(),
            Error::UndefinedVariable(error) => error.reason(),
        }
    }

    /// The message: a sentence for a person, which is not normative.
    ///
    /// ```
    /// use predicator_rs::{Error, TypeMismatchError};
    ///
    /// let error = Error::from(TypeMismatchError::new("add", "add takes numbers"));
    /// assert_eq!(error.message(), "add takes numbers");
    /// assert_eq!(error.to_string(), "add takes numbers");
    /// ```
    pub fn message(&self) -> &str {
        match self {
            Error::Parse(error) => error.message(),
            Error::Evaluation(error) => error.message(),
            Error::TypeMismatch(error) => error.message(),
            Error::UndefinedVariable(error) => error.message(),
        }
    }

    /// Where in the source the failure is, when that is known.
    ///
    /// ```
    /// use predicator_rs::{Context, Options, Position, compile, evaluate};
    ///
    /// let parse = compile("x >").unwrap_err();
    /// assert_eq!(parse.position(), Some(Position { line: 1, column: 4 }));
    /// let run = evaluate("1 / 0", &Context::new(), &Options::new()).unwrap_err();
    /// assert_eq!(run.position(), None);
    /// ```
    pub fn position(&self) -> Option<Position> {
        match self {
            Error::Parse(error) => error.position(),
            Error::Evaluation(error) => error.position(),
            Error::TypeMismatch(error) => error.position(),
            Error::UndefinedVariable(error) => error.position(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    // Sabotage: with type_name's TypeMismatchError arm answering
    // "TypeMismatch", this test fails.
    #[test]
    fn each_variant_carries_its_corpus_type_reason_message_and_position() {
        let at = Position { line: 2, column: 5 };
        let errors = [
            (
                Error::from(ParseError::new("p", "parse").with_position(at)),
                "ParseError",
                "p",
                "parse",
                Some(at),
            ),
            (
                Error::from(EvaluationError::new("e", "evaluation")),
                "EvaluationError",
                "e",
                "evaluation",
                None,
            ),
            (
                Error::from(TypeMismatchError::new("t", "mismatch").with_position(at)),
                "TypeMismatchError",
                "t",
                "mismatch",
                Some(at),
            ),
            (
                Error::from(UndefinedVariableError::new("u", "unbound")),
                "UndefinedVariableError",
                "u",
                "unbound",
                None,
            ),
        ];
        for (error, type_name, reason, message, position) in errors {
            assert_eq!(error.type_name(), type_name);
            assert_eq!(error.reason(), reason);
            assert_eq!(error.message(), message);
            assert_eq!(error.to_string(), message);
            assert_eq!(error.position(), position);
            let source: &dyn std::error::Error = &error;
            assert!(source.source().is_none());
        }
    }
}
