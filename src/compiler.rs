//! The compiler: a source in, an instruction list out. The lexer scans the
//! source, the parser reads the tokens into a tree, and the emitter
//! ([`emitter`]) walks the tree into the instructions the evaluator runs,
//! the same list the reference's compiler emits for the same source.

mod emitter;

use crate::lexer::{self, Refusal};
use crate::{Error, Instructions, parser};

/// Compiles an expression's source to the instruction list it runs as.
///
/// The instructions are the reference's for the same source: what
/// [`evaluate`](crate::evaluate) runs, and what
/// [`Instructions::to_json`] writes in the conformance corpus's tagged
/// encoding.
///
/// A source that does not scan or parse is refused with an
/// [`Error::Parse`] carrying the reference's message, the position of the
/// token it names, and a reason naming the refusal (`unexpected_character`,
/// `trailing_token`, `nesting_depth_exceeded` and others). A statement is
/// not an expression: `x = 1` is refused here and compiled by
/// [`compile_program`].
///
/// ```
/// use predicator_rs::{Context, Options, Position, Value, compile, evaluate};
///
/// let instructions = compile("score > 85")?;
/// assert_eq!(
///     instructions.to_json()?,
///     r#"[["load","score"],["lit",85],["compare","GT"]]"#
/// );
/// let mut context = Context::new();
/// context.insert("score", Value::from(90));
/// assert_eq!(evaluate(&instructions, &context, &Options::new())?, Value::from(true));
///
/// let refused = compile("score >").unwrap_err();
/// assert_eq!(refused.type_name(), "ParseError");
/// assert_eq!(refused.position(), Some(Position { line: 1, column: 8 }));
/// # Ok::<(), predicator_rs::Error>(())
/// ```
pub fn compile(source: &str) -> Result<Instructions, Error> {
    let tokens = lexer::tokenize(source).map_err(refused)?;
    let node = parser::parse(&tokens).map_err(refused)?;
    Ok(Instructions {
        list: emitter::expression(&node),
    })
}

/// Compiles a statement program's source, one statement or more, to the
/// instruction list it runs as.
///
/// A statement is an assignment (`location = value`, the location a
/// variable under any run of `.name` and `[key]` steps), an `if` with an
/// optional `else` or `else if`, a `while`, or an expression. Statements
/// are separated by `;`, and a statement ending in a closing brace needs
/// none before the next. Each compiles as the reference compiles it: an
/// assignment to its location's segments, its value and a `store`; an
/// expression to its instructions and a `pop`; an `if` and a `while` to
/// their condition, a jump over their blocks, and a back edge for the
/// `while`. [`execute`](crate::execute) and
/// [`execute_value`](crate::execute_value) run the list.
///
/// A source that does not scan or parse is refused as [`compile`] refuses
/// one, with an [`Error::Parse`].
///
/// ```
/// use predicator_rs::{Context, Key, Options, Value, compile_program, execute};
///
/// let program = compile_program("x = 1; if x > 0 { y = x + 1 }")?;
/// let execution = execute(&program, &Context::new(), &Options::new())?;
/// assert_eq!(execution.context().get(&Key::from("y")), Some(&Value::from(2)));
///
/// let refused = compile_program("x =").unwrap_err();
/// assert_eq!(refused.type_name(), "ParseError");
/// # Ok::<(), predicator_rs::Error>(())
/// ```
pub fn compile_program(source: &str) -> Result<Instructions, Error> {
    let tokens = lexer::tokenize(source).map_err(refused)?;
    let program = parser::parse_program(&tokens).map_err(refused)?;
    Ok(Instructions {
        list: emitter::program(&program),
    })
}

/// The error a refusal answers: its parse error, at the start of its span.
fn refused(refusal: Refusal) -> Error {
    Error::Parse(*refusal.error)
}
