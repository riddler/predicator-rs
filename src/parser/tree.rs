//! The tree the parser answers: expression nodes, statements, blocks and a
//! program, each with the position and the span the reference gives it.
//!
//! A node's position is the token that defines it: a leaf's own token, an
//! operator's token, the key, name or type an access or a cast names, the
//! opening token of something delimited. Its span runs from the start of
//! the source it covers to one past its end; a parenthesis widens the span
//! of the node inside it and leaves its position.
//!
//! A chain of left-associative operators or postfix steps leans left one
//! node per link and is not bounded by the parser's depth limit, so the tree
//! can be as deep as a source is long. Dropping a tree therefore never
//! recurses: [`Node`] and [`Block`] take their children apart on an explicit
//! stack. A walk over a tree must take a chain in a loop for the same
//! reason.

use core::mem;

use crate::lexer::{Quote, Span};
use crate::{Date, DateTime, Position};

/// One expression node: what it is, where it is, and what it covers.
#[derive(Debug, PartialEq)]
pub(crate) struct Node {
    pub(crate) kind: Expression,
    pub(crate) position: Position,
    pub(crate) span: Span,
}

/// What an expression node is.
#[derive(Debug, PartialEq)]
pub(crate) enum Expression {
    /// A number, a boolean, `undefined`, `null`, a date or a datetime.
    Literal(Literal),
    /// A string literal and the quote it is written with.
    String { value: String, quote: Quote },
    /// A variable.
    Identifier(String),
    /// `>`, `<`, `>=`, `<=`, `==`, `!=`, `===` or `!==`.
    Comparison {
        operator: Comparison,
        left: Box<Node>,
        right: Box<Node>,
    },
    /// `+`, `-`, `*`, `/` or `%`.
    Arithmetic {
        operator: Arithmetic,
        left: Box<Node>,
        right: Box<Node>,
    },
    /// `in`, `contains` or `intersects`.
    Membership {
        operator: Membership,
        left: Box<Node>,
        right: Box<Node>,
    },
    /// `is [not] blank`, `present` or `defined` after an operand.
    Presence {
        kind: Presence,
        negated: bool,
        operand: Box<Node>,
    },
    /// A prefix `-` or `!` below the comparison.
    Unary { operator: Unary, operand: Box<Node> },
    /// `AND`, `and` or `&&`.
    And { left: Box<Node>, right: Box<Node> },
    /// `OR`, `or` or `||`.
    Or { left: Box<Node>, right: Box<Node> },
    /// `NOT`, `not` or `!` above the comparison.
    Not { operand: Box<Node> },
    /// A list literal.
    List(Vec<Node>),
    /// An object literal's entries, in source order.
    Object(Vec<Entry>),
    /// A call by name, the name dotted where the source dots it.
    Call { name: String, arguments: Vec<Node> },
    /// `object[key]`.
    Index { object: Box<Node>, key: Box<Node> },
    /// `object.name`.
    Property { object: Box<Node>, name: String },
    /// `operand::type`, the type one of the seven scalar type names.
    Cast {
        operand: Box<Node>,
        type_name: &'static str,
    },
    /// A duration literal: its amounts and unit suffixes, a fraction
    /// already expanded into whole units.
    Duration(Vec<(i64, &'static str)>),
    /// A duration moved from now: `ago`, `from now`, `next` or `last`.
    RelativeDate {
        duration: Box<Node>,
        direction: Direction,
    },
}

/// A literal's value.
#[derive(Debug, PartialEq)]
pub(crate) enum Literal {
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Undefined,
    Null,
    Date(Date),
    /// A datetime and how many digits of a fraction of a second it writes.
    DateTime(DateTime, u8),
}

/// A comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Comparison {
    Gt,
    Lt,
    Gte,
    Lte,
    EqualEqual,
    Ne,
    StrictEq,
    StrictNe,
}

/// An arithmetic operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Arithmetic {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
}

/// A membership operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Membership {
    In,
    Contains,
    Intersects,
}

/// The word after `is` (and an optional `not`) in a presence test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Presence {
    Blank,
    Present,
    Defined,
}

/// A prefix operator below the comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unary {
    Minus,
    Bang,
}

/// Which way a relative date moves from now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Direction {
    /// `ago`.
    Ago,
    /// `from now`.
    Future,
    /// `next`.
    Next,
    /// `last`.
    Last,
}

/// One entry of an object literal: its key, how the key is written, where
/// the key is, and the value.
#[derive(Debug, PartialEq)]
pub(crate) struct Entry {
    pub(crate) key: String,
    pub(crate) style: KeyStyle,
    pub(crate) position: Position,
    pub(crate) span: Span,
    pub(crate) value: Node,
}

/// How an object key is written: bare, or quoted with the quote given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyStyle {
    Bare,
    Quoted(Quote),
}

/// One statement of a program or a block.
#[derive(Debug, PartialEq)]
pub(crate) enum Statement {
    /// `location = value`, at the `=` and over both sides. The target is an
    /// identifier under any run of property and bracket accesses.
    Assignment {
        target: Node,
        value: Node,
        position: Position,
        span: Span,
    },
    /// `if condition { ... }` with an optional `else` block; `else if` is an
    /// else block holding the one `if`, at its position and over its span.
    If {
        condition: Node,
        then_block: Block,
        else_block: Option<Block>,
        position: Position,
        span: Span,
    },
    /// `while condition { ... }`.
    While {
        condition: Node,
        body: Block,
        position: Position,
        span: Span,
    },
    /// An expression on its own.
    Expression(Node),
}

impl Statement {
    /// Where the statement is.
    pub(crate) fn position(&self) -> Position {
        match self {
            Statement::Assignment { position, .. }
            | Statement::If { position, .. }
            | Statement::While { position, .. } => *position,
            Statement::Expression(node) => node.position,
        }
    }

    /// What the statement covers.
    pub(crate) fn span(&self) -> Span {
        match self {
            Statement::Assignment { span, .. }
            | Statement::If { span, .. }
            | Statement::While { span, .. } => *span,
            Statement::Expression(node) => node.span,
        }
    }
}

/// Braces around zero statements or more.
#[derive(Debug, PartialEq)]
pub(crate) struct Block {
    pub(crate) statements: Vec<Statement>,
    pub(crate) position: Position,
    pub(crate) span: Span,
}

/// A whole program: one statement or more, at its first token and over its
/// statements (a trailing `;` left out).
#[derive(Debug, PartialEq)]
pub(crate) struct Program {
    pub(crate) statements: Vec<Statement>,
    pub(crate) position: Position,
    pub(crate) span: Span,
}

impl Drop for Node {
    /// Takes the children apart on an explicit stack, so a chain as long as
    /// the source drops without recursing.
    fn drop(&mut self) {
        let mut pending = Vec::new();
        detach(&mut self.kind, &mut pending);
        while let Some(mut node) = pending.pop() {
            detach(&mut node.kind, &mut pending);
        }
    }
}

/// Moves the children of `kind` onto `pending`, leaving a leaf in its place.
fn detach(kind: &mut Expression, pending: &mut Vec<Node>) {
    match mem::replace(kind, Expression::Literal(Literal::Null)) {
        Expression::Comparison { left, right, .. }
        | Expression::Arithmetic { left, right, .. }
        | Expression::Membership { left, right, .. }
        | Expression::And { left, right }
        | Expression::Or { left, right }
        | Expression::Index {
            object: left,
            key: right,
        } => {
            pending.push(*left);
            pending.push(*right);
        }
        Expression::Presence { operand, .. }
        | Expression::Unary { operand, .. }
        | Expression::Not { operand }
        | Expression::Property {
            object: operand, ..
        }
        | Expression::Cast { operand, .. }
        | Expression::RelativeDate {
            duration: operand, ..
        } => pending.push(*operand),
        Expression::List(nodes)
        | Expression::Call {
            arguments: nodes, ..
        } => pending.extend(nodes),
        Expression::Object(entries) => pending.extend(entries.into_iter().map(|entry| entry.value)),
        Expression::Literal(_)
        | Expression::String { .. }
        | Expression::Identifier(_)
        | Expression::Duration(_) => {}
    }
}

impl Drop for Block {
    /// Takes nested blocks apart on an explicit stack, so statements nested
    /// as deep as the parser allows drop without recursing.
    fn drop(&mut self) {
        let mut pending = vec![mem::take(&mut self.statements)];
        while let Some(statements) = pending.pop() {
            for mut statement in statements {
                match &mut statement {
                    Statement::If {
                        then_block,
                        else_block,
                        ..
                    } => {
                        pending.push(mem::take(&mut then_block.statements));
                        if let Some(block) = else_block {
                            pending.push(mem::take(&mut block.statements));
                        }
                    }
                    Statement::While { body, .. } => {
                        pending.push(mem::take(&mut body.statements));
                    }
                    Statement::Assignment { .. } | Statement::Expression(_) => {}
                }
            }
        }
    }
}
