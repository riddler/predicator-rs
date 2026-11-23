//! The compiler's second stage: the token stream the lexer answers in, a
//! private tree out ([`tree`]), or the one refusal that stopped it. It reads
//! the reference's grammar with the reference's precedence, lowest first:
//!
//! ```text
//! program        -> statement ( ";" statement )* ";"?
//! statement      -> if | while | location "=" expression | expression
//! expression     -> logical_or
//! logical_or     -> logical_and ( ( "OR" | "||" ) logical_and )*
//! logical_and    -> logical_not ( ( "AND" | "&&" ) logical_not )*
//! logical_not    -> ( "NOT" | "!" ) logical_not | comparison
//! comparison     -> addition ( relation addition | presence )?
//! relation       -> ">" | "<" | ">=" | "<=" | "==" | "!=" | "===" | "!=="
//!                 | "in" | "contains" | "intersects"
//! presence       -> "is" "not"? ( "blank" | "present" | "defined" )
//! addition       -> multiplication ( ( "+" | "-" ) multiplication )*
//! multiplication -> unary ( ( "*" | "/" | "%" ) unary )*
//! unary          -> ( "-" | "!" ) unary | postfix
//! postfix        -> primary ( "[" expression "]" | "." name | "::" type )*
//! ```
//!
//! The binary operators associate to the left. A comparison takes one
//! operator at most, so a second is a token after a finished expression:
//! `a < b < c` is refused at the second `<`. `NOT` and a `!` at the head of a
//! `logical_not` bind looser than a comparison (`!a == b` negates the
//! comparison); a `!` anywhere else is the prefix operator of `unary`.
//!
//! # The newer operators
//!
//! `intersects` and the presence test `is [not] blank`, `is [not] present`
//! and `is [not] defined` are read by position: each only directly after a
//! complete `addition`, where a relation may stand, and so at the precedence
//! of `in` and `contains`. Each word is spelled in lower case or in capitals
//! (`IS NOT BLANK`, `INTERSECTS`), and `not` is the lexer's `NOT` in either
//! spelling. A presence test is the comparison, so it does not chain with
//! another relation, and none of the words is reserved: anywhere else each
//! is an ordinary name (`is is blank` tests the variable `is`), and a word
//! after an operand that starts no such operator is left for the caller.
//!
//! # Statements
//!
//! A statement is an assignment where an `addition` read from its start is
//! followed by `=`; that read is a probe, and when it is refused or no `=`
//! follows, the statement is read again as an expression. A left side that
//! is not an identifier under property and bracket accesses is refused at
//! the `=`. A statement ending in a closing brace needs no `;` before the
//! next. A program holds one statement at least.
//!
//! # Refusals
//!
//! A refusal is a [`ParseError`] carrying the reference's message, at the
//! position and over the span of the token it names, and a reason, which is
//! this crate's token for the refusal. The end of input is the lexer's end
//! token, a zero-width span where the source ends.
//!
//! # Depth
//!
//! One refusal is not the reference's, which follows a source as deep as it
//! nests: a source nesting more than [`DEPTH_LIMIT`] levels is refused with
//! the reason `nesting_depth_exceeded`, at the token the parse stopped on. A
//! level is counted each time the parse enters a whole expression (the
//! source's, and one inside a parenthesis, a list, an object, an index, a
//! call's arguments, a statement's condition or value), for each prefix
//! operator, a relative date's operand, a block's statements, an `else if`
//! and a statement's assignment probe. A chain of left-associative
//! operators or of postfix steps is read in a loop and costs none.
//!
//! The parse does not recurse: what is open is held on explicit stacks, so
//! the stack a parse runs on does not grow with the source, and the bound
//! exists for the walks over the tree that follow. The tree drops without
//! recursing too (see [`tree`]).

mod durations;
mod expression;
mod statements;
#[cfg(test)]
mod tests;
mod text;
pub(crate) mod tree;

use crate::lexer::{Kind, Refusal, Span, Token};
use crate::{ParseError, Position};
use tree::{Node, Program};

/// How many levels a source may nest, the whole expression counting as the
/// first. A source at exactly this depth parses. It is the bound the Ruby
/// sibling chose, and above the 300 parentheses the reference's compile
/// transcript compiles.
pub(crate) const DEPTH_LIMIT: usize = 400;

/// Parses one expression from a token stream the lexer answered.
pub(crate) fn parse(tokens: &[Token]) -> Result<Node, Refusal> {
    let mut descent = Descent::new(tokens);
    let node = descent.expression()?;
    let trailing = descent.peek();
    if trailing.kind != Kind::Eof {
        return Err(descent.unexpected(
            "trailing_token",
            |found| format!("Unexpected token {found} after expression"),
            trailing,
        ));
    }
    Ok(node)
}

/// Parses a program, one statement or more, from a token stream the lexer
/// answered.
pub(crate) fn parse_program(tokens: &[Token]) -> Result<Program, Refusal> {
    Descent::new(tokens).program()
}

/// The token a stream with no tokens at all reads as: the end of input at
/// the start of the source. The lexer always ends a stream with its own.
static END: Token = Token {
    kind: Kind::Eof,
    line: 1,
    column: 1,
    length: 0,
};

/// One parse of one token stream: the tokens, the cursor and how many
/// levels are open.
struct Descent<'a> {
    tokens: &'a [Token],
    index: usize,
    depth: usize,
}

impl<'a> Descent<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Descent {
            tokens,
            index: 0,
            depth: 0,
        }
    }

    /// The token at the cursor; past the end, the stream's last token.
    fn peek(&self) -> &'a Token {
        self.token_at(self.index)
    }

    fn token_at(&self, index: usize) -> &'a Token {
        self.tokens
            .get(index)
            .or_else(|| self.tokens.last())
            .unwrap_or(&END)
    }

    /// The token after the one at the cursor.
    fn ahead(&self) -> &'a Token {
        self.token_at(self.index + 1)
    }

    /// The token just consumed.
    fn previous(&self) -> &'a Token {
        self.token_at(self.index.saturating_sub(1))
    }

    fn at(&self, kind: &Kind) -> bool {
        &self.peek().kind == kind
    }

    fn advance(&mut self) -> &'a Token {
        let token = self.peek();
        self.index += 1;
        token
    }

    /// The token, consumed, when it is the kind given; the refusal naming
    /// it otherwise.
    fn expect(&mut self, kind: &Kind, reason: &str, message: &str) -> Result<&'a Token, Refusal> {
        let token = self.peek();
        if &token.kind != kind {
            return Err(self.unexpected(reason, |found| format!("{message} {found}"), token));
        }
        Ok(self.advance())
    }

    /// Opens one more level, or refuses at the token the parse stopped on
    /// when the source is already at the limit. Whoever opens a level closes
    /// it with [`Descent::leave`].
    fn enter(&mut self) -> Result<(), Refusal> {
        self.depth += 1;
        if self.depth <= DEPTH_LIMIT {
            return Ok(());
        }
        Err(refuse(
            "nesting_depth_exceeded",
            format!(
                "Expression nests past the depth limit of {DEPTH_LIMIT} levels, \
                 the whole expression counting as the first"
            ),
            span_of(self.peek()),
        ))
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    /// A refusal naming `token`, at it and over it.
    fn unexpected(
        &self,
        reason: &str,
        message: impl FnOnce(&str) -> String,
        token: &Token,
    ) -> Refusal {
        refuse(
            reason,
            message(&text::describe(&token.kind)),
            span_of(token),
        )
    }
}

/// A refusal at the start of `span` and over it.
fn refuse(reason: &str, message: String, span: Span) -> Refusal {
    Refusal {
        error: Box::new(ParseError::new(reason, message).with_position(span.start)),
        span,
    }
}

fn start_of(token: &Token) -> Position {
    Position {
        line: token.line,
        column: token.column,
    }
}

/// One past the token's last character; a string token, which may hold a
/// line feed, carries its own.
fn end_of(token: &Token) -> Position {
    match &token.kind {
        Kind::String { end, .. } => *end,
        _ => Position {
            line: token.line,
            column: token.column + token.length,
        },
    }
}

fn span_of(token: &Token) -> Span {
    Span {
        start: start_of(token),
        end: end_of(token),
    }
}

fn between(start: Position, end: Position) -> Span {
    Span { start, end }
}
