//! A Rust port of the predicator condition language, which compiles a
//! condition to a small instruction set and evaluates it on a stack machine.
//!
//! The crate holds its value model: [`Value`] and the types it is
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
//! and `or` as `retired_opcode`.
//!
//! The compiler turns a source into the same instructions the reference's
//! compiler does: [`compile`] reads one expression and [`compile_program`]
//! a statement program, each answering [`Instructions`] or the
//! [`Error::Parse`] that refused the source, and [`Instructions::to_json`]
//! writes the list back in the tagged encoding. [`evaluate`] also takes a
//! source as its program (a `&str`), compiled as an expression, and
//! [`execute`] and [`execute_value`] one compiled as a statement program.
//!
//! [`execute`] and [`execute_value`] run the same instructions as a
//! statement program: the answer is an [`Execution`], the context the run
//! leaves (a new value; the one passed in is never changed) with the roots
//! it stored into, or a [`Stopped`], the error a run stopped on with the
//! context as of its last completed statement. A store into a root the
//! [`Options`] protect ([`Options::protected_roots`]) is refused.
//!
//! [`parse_duration`] reads a duration from its literal spelling (`"1h30m"`),
//! and [`ISA_VERSION`] is the version of the instruction set the crate runs.
//!
//! # The public surface
//!
//! The crate root holds the functions [`compile`], [`compile_program`],
//! [`evaluate`], [`execute`], [`execute_value`] and [`parse_duration`]; the
//! constant [`ISA_VERSION`]; the module [`tagged`]; and the types [`Value`]
//! (with its parts [`Map`] and [`Key`]), [`Context`], [`Instructions`],
//! [`Duration`], [`Date`], [`DateTime`], [`Options`] (with its part
//! [`OnUnbound`]), [`Execution`], [`Stopped`], [`Position`], and [`Error`]
//! with the four error types it holds. Every other module is private.
//! Every entry point answers a `Result`, and nothing is configured
//! globally: host functions, the clock and the random source are passed per
//! call in an [`Options`], and may borrow from the caller for the length of
//! the call.
//!
//! No source, instruction list or context makes the crate panic or
//! overflow its stack. A panic inside a function the caller passes in (a
//! host function, a clock or a random source) is not caught: it unwinds
//! through the run to the caller, as any panic in the caller's own code
//! does.
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
mod compiler;
mod context;
mod error;
mod evaluator;
mod execution;
mod instructions;
mod json;
mod lexer;
mod map;
mod options;
mod parser;
#[cfg(feature = "serde")]
mod serde_impl;
pub mod tagged;
mod value;

pub use civil::{Date, DateTime, Duration, parse_duration};
pub use compiler::{compile, compile_program};
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

/// The version of the reference's instruction set this crate runs: every
/// opcode of every tier up to it, the version the vendored conformance
/// corpus names.
///
/// ```
/// assert_eq!(predicator_rs::ISA_VERSION, 7);
/// ```
pub const ISA_VERSION: u32 = 7;

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
