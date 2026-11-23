//! A Rust port of the predicator condition language, which compiles a
//! condition to a small instruction set and evaluates it on a stack machine.
//!
//! The crate holds its value model so far: [`Value`] and the types it is
//! built from ([`Map`] and its [`Key`], [`Date`], [`DateTime`], [`Duration`]), the
//! [`Error`] every entry point answers, and the [`tagged`] codec for the
//! conformance corpus's tagged-value encoding.
//!
//! The evaluator runs every tier of the instruction set:
//! [`evaluate`] runs [`Instructions`] (read with
//! [`Instructions::from_json`]) over a [`Context`] with the [`Options`]
//! given per call, answering a [`Value`]. It runs `lit`, `load`, `compare`,
//! `not`, `unary_bang`, `unary_minus`, `jump_if_falsy_or_pop` and
//! `jump_if_true_or_pop`; `add`, `subtract`, `multiply`, `divide` and
//! `modulo`; `access`, `bracket_access`, `in`, `contains` and `make_list`;
//! `object_new`, `object_set`, `duration` and `relative_date`, which
//! reads the current time from the clock the [`Options`] name; and `call`,
//! which calls a function the [`Options`] pass in by name
//! ([`Options::function`]) or one of the reference's library functions
//! (the string, list, math, date and JSON functions); `store` and `pop`;
//! `cast`; `jump` and `pop_jump_if_falsy`; `jump_backward`, each back edge
//! spending one of the run's loop budget ([`Options::loop_budget`]); and
//! `presence` and `intersects`. Every other opcode is refused as `unknown_instruction`, and the retired `and`
//! and `or` as `retired_opcode`. The compiler is to come.
//!
//! [`execute`] and [`execute_value`] run the same instructions as a
//! statement program: the answer is an [`Execution`], the context the run
//! leaves (a new value; the one passed in is never changed) with the roots
//! it stored into, or a [`Stopped`], the error a run stopped on with the
//! context as of its last completed statement. A store into a root the
//! [`Options`] protect ([`Options::protected_roots`]) is refused.
//!
//! With the `serde` feature (off by default), [`Value`] implements
//! `Serialize` and `Deserialize` in the tagged encoding, and [`Date`],
//! [`DateTime`] and [`Duration`] in the text and the unit map that encoding
//! carries them in.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod boundary;
mod civil;
mod compare;
mod context;
mod error;
mod evaluator;
mod execution;
mod instructions;
mod json;
// The lexer has no caller in the library until the compiler uses it; the
// expectation fails the build once one does, so it comes off then.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "no caller until the compiler uses the lexer")
)]
mod lexer;
mod map;
mod options;
#[cfg(feature = "serde")]
mod serde_impl;
pub mod tagged;
mod value;

pub use civil::{Date, DateTime, Duration};
pub use context::Context;
pub use error::{
    Error, EvaluationError, ParseError, Position, TypeMismatchError, UndefinedVariableError,
};
pub use evaluator::{evaluate, execute, execute_value};
pub use execution::{Execution, Stopped};
pub use instructions::Instructions;
pub use map::{Key, Map};
pub use options::{OnUnbound, Options};
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
