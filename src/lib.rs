//! A Rust port of the predicator condition language, which compiles a
//! condition to a small instruction set and evaluates it on a stack machine.
//!
//! The crate holds its value model so far: [`Value`] and the types it is
//! built from ([`Map`], [`Date`], [`DateTime`], [`Duration`]), the
//! [`Error`] every entry point answers, and the [`tagged`] codec for the
//! conformance corpus's tagged-value encoding. The compiler and the
//! evaluator are to come.
//!
//! With the `serde` feature (off by default), [`Value`] implements
//! `Serialize` and `Deserialize` in the tagged encoding, and [`Date`],
//! [`DateTime`] and [`Duration`] in the text and the unit map that encoding
//! carries them in.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod boundary;
mod civil;
mod error;
mod json;
mod map;
#[cfg(feature = "serde")]
mod serde_impl;
pub mod tagged;
mod value;

pub use civil::{Date, DateTime, Duration};
pub use error::{
    Error, EvaluationError, ParseError, Position, TypeMismatchError, UndefinedVariableError,
};
pub use map::Map;
pub use value::Value;

#[cfg(test)]
mod tests {
    use unicode_segmentation::UnicodeSegmentation;

    // The crate's one required dependency resolves and links.
    // Sabotage: with the expected count changed from 2 to 3, this test fails.
    #[test]
    fn links_the_segmentation_dependency() {
        assert_eq!("e\u{301}x".graphemes(true).count(), 2);
    }
}
