//! The expression grammar, read without recursion.
//!
//! What a recursive descent would hold on the call stack is held here on
//! two explicit stacks. A frame is one whole expression being read: what it
//! sits inside (the source itself, a parenthesis, a list, an object, a call,
//! an index) and the operators still waiting for their right operand. A new
//! frame opens where the grammar enters a whole expression and closes at its
//! closing token. Inside a frame an operator is folded into its left operand
//! once a looser operator, or the end of the frame, follows it, which gives
//! the precedence and the left associativity the grammar's levels give.

use core::mem;

use super::tree::{
    Arithmetic, Comparison, Direction, Entry, Expression, KeyStyle, Literal, Membership, Node,
    Presence, Unary,
};
use super::{Descent, between, end_of, span_of, start_of};
use crate::Position;
use crate::lexer::{Kind, Refusal, Span};

/// The seven scalar type names a cast may name, in the reference's order.
const CAST_TYPE_NAMES: [&str; 7] = [
    "integer", "float", "string", "boolean", "date", "datetime", "duration",
];

/// A bare `=` where an expression goes on.
const EQUALS: &str = "'=' is not an equality operator - use '==' for equality. \
                      Assignment is only valid at the start of a statement.";

/// What a primary may be, in the refusal of anything else.
const EXPECTED: &str = "Expected number, string, boolean, date, datetime, identifier, \
                        function call, list, object, or '(' but found";

/// What the outermost frame reads: a whole expression, or an addition alone
/// (a statement's assignment probe).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Base {
    Expression,
    Addition,
}

/// Where an operand starts, which decides what a prefix token is there.
#[derive(Clone, Copy)]
enum Slot {
    /// The head of a `logical_not`: `NOT` and `!` negate what follows.
    Logical,
    /// Any other operand: `-` and `!` are prefix operators, `NOT` is not.
    Operand,
    /// A relative date's operand: a primary and nothing else.
    Primary,
}

/// What a frame sits inside.
enum Barrier {
    Root(Base),
    /// A parenthesis, opened at the position given.
    Paren(Position),
    List {
        open: Position,
        elements: Vec<Node>,
    },
    Object {
        open: Position,
        entries: Vec<Entry>,
        key: Key,
    },
    Call {
        name: String,
        position: Position,
        arguments: Vec<Node>,
    },
    /// An index into `object`, whose key starts at `key`.
    Index {
        object: Box<Node>,
        key: Position,
    },
}

/// An object key read before its value: the name, how it is written, where
/// it is and what it covers.
struct Key {
    name: String,
    style: KeyStyle,
    position: Position,
    span: Span,
}

/// An operator waiting in a frame.
enum Op {
    /// `NOT` or `!` at the head of a `logical_not`.
    Not(Position),
    /// A prefix `-` or `!`.
    Unary(Unary, Position),
    /// `next` or `last`, waiting for its duration; `start` is the index of
    /// the token after the word.
    RelativeDate {
        direction: Direction,
        position: Position,
        start: usize,
    },
    /// An infix operator with its left operand.
    Binary {
        operator: Binary,
        left: Box<Node>,
        position: Position,
    },
}

/// An infix operator.
#[derive(Clone, Copy)]
enum Binary {
    Or,
    And,
    Comparison(Comparison),
    Membership(Membership),
    Arithmetic(Arithmetic),
}

impl Binary {
    /// How tightly it binds: a fold takes every waiting operator at least
    /// as tight as the one that follows.
    fn level(self) -> u8 {
        match self {
            Binary::Or => 1,
            Binary::And => 2,
            Binary::Comparison(_) | Binary::Membership(_) => 3,
            Binary::Arithmetic(Arithmetic::Add | Arithmetic::Subtract) => 4,
            Binary::Arithmetic(_) => 5,
        }
    }
}

struct Frame {
    barrier: Barrier,
    ops: Vec<Op>,
}

impl Frame {
    fn new(barrier: Barrier) -> Self {
        Frame {
            barrier,
            ops: Vec::new(),
        }
    }
}

/// The frame being read and the frames it sits inside.
struct Frames {
    current: Frame,
    outer: Vec<Frame>,
}

impl Frames {
    fn open(&mut self, barrier: Barrier) {
        let inner = Frame::new(barrier);
        self.outer.push(mem::replace(&mut self.current, inner));
    }

    /// Closes the current frame and answers what it sat inside, or `None`
    /// when it is the outermost.
    fn close(&mut self) -> Option<Barrier> {
        let parent = self.outer.pop()?;
        Some(mem::replace(&mut self.current, parent).barrier)
    }
}

/// What to read next.
enum Step {
    /// An operand, starting at the cursor.
    Operand(Slot),
    /// A primary has been read.
    Primary(Node),
    /// An operand's postfix steps, after its primary.
    Postfix(Node),
    /// An operand is complete; what follows decides what it belongs to.
    Fold(Node),
    /// The outermost frame is complete.
    Done(Node),
}

impl Descent<'_> {
    /// One whole expression, one level deeper.
    pub(super) fn expression(&mut self) -> Result<Node, Refusal> {
        self.read(Base::Expression)
    }

    /// Reads from the cursor; the depth is as it was afterwards, a refusal
    /// included, so a refused probe leaves nothing open.
    pub(super) fn read(&mut self, base: Base) -> Result<Node, Refusal> {
        let depth = self.depth;
        let answer = self.machine(base);
        self.depth = depth;
        answer
    }

    fn machine(&mut self, base: Base) -> Result<Node, Refusal> {
        self.enter()?;
        let mut frames = Frames {
            current: Frame::new(Barrier::Root(base)),
            outer: Vec::new(),
        };
        let mut step = Step::Operand(match base {
            Base::Expression => Slot::Logical,
            Base::Addition => Slot::Operand,
        });
        loop {
            step = match step {
                Step::Operand(slot) => self.operand(slot, &mut frames)?,
                Step::Primary(node) => self.after_primary(node, &mut frames)?,
                Step::Postfix(node) => self.postfix(node, &mut frames)?,
                Step::Fold(node) => self.fold(node, &mut frames)?,
                Step::Done(node) => return Ok(node),
            };
        }
    }

    /// Prefix operators, or the primary they lead to.
    fn operand(&mut self, slot: Slot, frames: &mut Frames) -> Result<Step, Refusal> {
        let token = self.peek();
        let op = match (&token.kind, slot) {
            (Kind::NotOp(_) | Kind::Bang, Slot::Logical) => Op::Not(start_of(token)),
            (Kind::Minus, Slot::Logical | Slot::Operand) => {
                Op::Unary(Unary::Minus, start_of(token))
            }
            (Kind::Bang, Slot::Operand) => Op::Unary(Unary::Bang, start_of(token)),
            _ => return self.primary(frames),
        };
        self.advance();
        self.enter()?;
        let next = match op {
            Op::Not(_) => Slot::Logical,
            _ => Slot::Operand,
        };
        frames.current.ops.push(op);
        Ok(Step::Operand(next))
    }

    fn primary(&mut self, frames: &mut Frames) -> Result<Step, Refusal> {
        let token = self.advance();
        let at = start_of(token);
        let leaf = |kind| Node {
            kind,
            position: at,
            span: span_of(token),
        };
        let literal = |value| leaf(Expression::Literal(value));
        let node = match &token.kind {
            Kind::Integer(value) => match self.duration(token)? {
                Some(node) => node,
                None => literal(Literal::Integer(*value)),
            },
            Kind::FractionalNumber { .. } => match self.duration(token)? {
                Some(node) => node,
                None => {
                    return Err(self.unexpected(
                        "expected_duration_unit",
                        |found| format!("Expected a duration unit after {found}"),
                        token,
                    ));
                }
            },
            Kind::Float(value) => literal(Literal::Float(*value)),
            Kind::Boolean(value) => literal(Literal::Boolean(*value)),
            Kind::Undefined => literal(Literal::Undefined),
            Kind::Null => literal(Literal::Null),
            Kind::Date(date) => literal(Literal::Date(*date)),
            Kind::DateTime(instant, precision) => literal(Literal::DateTime(*instant, *precision)),
            Kind::String { value, quote, .. } => leaf(Expression::String {
                value: value.clone(),
                quote: *quote,
            }),
            Kind::Identifier(name) => leaf(Expression::Identifier(name.clone())),
            Kind::FunctionName(name) | Kind::QualifiedFunctionName(name) => {
                return self.call(name, at, frames);
            }
            Kind::Lparen => {
                self.enter()?;
                frames.open(Barrier::Paren(at));
                return Ok(Step::Operand(Slot::Logical));
            }
            Kind::Lbracket => {
                if self.at(&Kind::Rbracket) {
                    let close = self.advance();
                    Node {
                        kind: Expression::List(Vec::new()),
                        position: at,
                        span: between(at, end_of(close)),
                    }
                } else {
                    self.enter()?;
                    frames.open(Barrier::List {
                        open: at,
                        elements: Vec::new(),
                    });
                    return Ok(Step::Operand(Slot::Logical));
                }
            }
            Kind::Lbrace => {
                if self.at(&Kind::Rbrace) {
                    let close = self.advance();
                    Node {
                        kind: Expression::Object(Vec::new()),
                        position: at,
                        span: between(at, end_of(close)),
                    }
                } else {
                    let key = self.object_key()?;
                    self.enter()?;
                    frames.open(Barrier::Object {
                        open: at,
                        entries: Vec::new(),
                        key,
                    });
                    return Ok(Step::Operand(Slot::Logical));
                }
            }
            Kind::NextOp | Kind::LastOp => {
                let direction = if token.kind == Kind::NextOp {
                    Direction::Next
                } else {
                    Direction::Last
                };
                let start = self.index;
                self.enter()?;
                frames.current.ops.push(Op::RelativeDate {
                    direction,
                    position: at,
                    start,
                });
                return Ok(Step::Operand(Slot::Primary));
            }
            Kind::IfKw | Kind::ElseKw | Kind::WhileKw => {
                let word = match token.kind {
                    Kind::IfKw => "if",
                    Kind::ElseKw => "else",
                    _ => "while",
                };
                return Err(super::refuse(
                    "statement_keyword",
                    format!(
                        "'{word}' is a statement keyword, not an expression - control flow is \
                         only valid in a program (Predicator.parse_program/2)."
                    ),
                    span_of(token),
                ));
            }
            _ => {
                return Err(self.unexpected(
                    "expected_primary",
                    |found| format!("{EXPECTED} {found}"),
                    token,
                ));
            }
        };
        Ok(Step::Primary(node))
    }

    /// A call, its name consumed: an empty argument list, or a frame for
    /// the first argument.
    fn call(
        &mut self,
        name: &str,
        position: Position,
        frames: &mut Frames,
    ) -> Result<Step, Refusal> {
        self.expect(
            &Kind::Lparen,
            "expected_open_paren",
            "Expected '(' after function name but found",
        )?;
        if self.at(&Kind::Rparen) {
            let close = self.advance();
            return Ok(Step::Primary(Node {
                kind: Expression::Call {
                    name: name.to_owned(),
                    arguments: Vec::new(),
                },
                position,
                span: between(position, end_of(close)),
            }));
        }
        self.enter()?;
        frames.open(Barrier::Call {
            name: name.to_owned(),
            position,
            arguments: Vec::new(),
        });
        Ok(Step::Operand(Slot::Logical))
    }

    /// An object key, bare or quoted, and the colon after it.
    fn object_key(&mut self) -> Result<Key, Refusal> {
        let token = self.peek();
        let (name, style) = match &token.kind {
            Kind::Identifier(name) => (name.clone(), KeyStyle::Bare),
            Kind::String { value, quote, .. } => (value.clone(), KeyStyle::Quoted(*quote)),
            _ => {
                return Err(self.unexpected(
                    "expected_object_key",
                    |found| {
                        format!("Expected identifier or string for object key but found {found}")
                    },
                    token,
                ));
            }
        };
        self.advance();
        self.expect(
            &Kind::Colon,
            "expected_object_colon",
            "Expected ':' after object key but found",
        )?;
        Ok(Key {
            name,
            style,
            position: start_of(token),
            span: span_of(token),
        })
    }

    /// A primary read: a relative date waiting for it takes it, and
    /// otherwise its postfix steps follow.
    fn after_primary(&mut self, node: Node, frames: &mut Frames) -> Result<Step, Refusal> {
        let (direction, position, start) = match frames.current.ops.pop() {
            Some(Op::RelativeDate {
                direction,
                position,
                start,
            }) => (direction, position, start),
            other => {
                frames.current.ops.extend(other);
                return Ok(Step::Postfix(node));
            }
        };
        self.leave();
        Ok(Step::Primary(
            self.relative_date(direction, position, start, node)?,
        ))
    }

    /// `next` or `last` and the primary after it, which must be a duration.
    fn relative_date(
        &self,
        direction: Direction,
        position: Position,
        start: usize,
        duration: Node,
    ) -> Result<Node, Refusal> {
        if !matches!(duration.kind, Expression::Duration(_)) {
            let word = if direction == Direction::Next {
                "next"
            } else {
                "last"
            };
            return Err(self.unexpected(
                "expected_duration",
                |found| format!("Expected duration after '{word}' but found {found}"),
                self.token_at(start),
            ));
        }
        let span = between(position, duration.span.end);
        Ok(Node {
            kind: Expression::RelativeDate {
                duration: Box::new(duration),
                direction,
            },
            position,
            span,
        })
    }

    /// An index, a property or a cast after an operand, or none.
    fn postfix(&mut self, object: Node, frames: &mut Frames) -> Result<Step, Refusal> {
        let token = self.peek();
        let step = match token.kind {
            Kind::Lbracket => {
                self.advance();
                let key = start_of(self.peek());
                self.enter()?;
                frames.open(Barrier::Index {
                    object: Box::new(object),
                    key,
                });
                return Ok(Step::Operand(Slot::Logical));
            }
            Kind::Dot => {
                self.advance();
                let name = self.peek();
                let property = match &name.kind {
                    Kind::Identifier(property) => property.as_str(),
                    Kind::LastOp => "last",
                    Kind::NextOp => "next",
                    Kind::AgoOp => "ago",
                    Kind::FromOp => "from",
                    Kind::NowOp => "now",
                    _ => {
                        return Err(self.unexpected(
                            "expected_property_name",
                            |found| format!("Expected property name after '.' but found {found}"),
                            name,
                        ));
                    }
                };
                self.advance();
                let span = between(object.span.start, end_of(name));
                Node {
                    kind: Expression::Property {
                        object: Box::new(object),
                        name: property.to_owned(),
                    },
                    position: start_of(name),
                    span,
                }
            }
            Kind::DoubleColon => {
                self.advance();
                let name = self.peek();
                let Kind::Identifier(type_name) = &name.kind else {
                    return Err(self.unexpected(
                        "expected_type_name",
                        |found| format!("Expected a type name after '::' but found {found}"),
                        name,
                    ));
                };
                self.advance();
                let Some(type_name) = CAST_TYPE_NAMES.iter().find(|known| *known == type_name)
                else {
                    return Err(super::refuse(
                        "unknown_cast_type",
                        format!(
                            "Unknown cast type '{type_name}' - expected one of: {}",
                            CAST_TYPE_NAMES.join(", ")
                        ),
                        span_of(name),
                    ));
                };
                let span = between(object.span.start, end_of(name));
                Node {
                    kind: Expression::Cast {
                        operand: Box::new(object),
                        type_name,
                    },
                    position: start_of(name),
                    span,
                }
            }
            _ => return Ok(Step::Fold(object)),
        };
        Ok(Step::Postfix(step))
    }

    /// An operand is complete. Folds the operators that bind tighter than
    /// what follows it, then takes what follows as the next operator, or
    /// closes the frame when nothing that follows belongs to it.
    fn fold(&mut self, node: Node, frames: &mut Frames) -> Result<Step, Refusal> {
        let ops = &mut frames.current.ops;
        let mut node = self.fold_while(ops, node, |op| matches!(op, Op::Unary(..)))?;
        let token = self.peek();
        if let Some(operator) = arithmetic(&token.kind) {
            let level = Binary::Arithmetic(operator).level();
            node = self.fold_while(ops, node, |op| binary_at_least(op, level))?;
            return Ok(self.push(ops, Binary::Arithmetic(operator), node));
        }
        node = self.fold_while(ops, node, |op| binary_at_least(op, 4))?;
        if matches!(frames.current.barrier, Barrier::Root(Base::Addition)) {
            return self.close(node, frames);
        }
        let ops = &mut frames.current.ops;
        let compared = matches!(
            ops.last(),
            Some(Op::Binary {
                operator: Binary::Comparison(_) | Binary::Membership(_),
                ..
            })
        );
        if compared {
            node = self.fold_while(ops, node, |op| binary_at_least(op, 3))?;
        } else {
            if token.kind == Kind::Eq {
                return Err(super::refuse(
                    "assignment_in_expression",
                    EQUALS.to_owned(),
                    span_of(token),
                ));
            }
            if let Some(operator) = relation(&token.kind) {
                return Ok(self.push(ops, operator, node));
            }
            node = self.presence(node)?;
        }
        node = self.fold_while(ops, node, |op| matches!(op, Op::Not(_)))?;
        let token = self.peek();
        match token.kind {
            Kind::AndOp(_) | Kind::AndAnd => {
                node = self.fold_while(ops, node, |op| binary_at_least(op, 2))?;
                Ok(self.push(ops, Binary::And, node))
            }
            Kind::OrOp(_) | Kind::OrOr => {
                node = self.fold_while(ops, node, |op| binary_at_least(op, 1))?;
                Ok(self.push(ops, Binary::Or, node))
            }
            _ => {
                node = self.fold_while(ops, node, |_| true)?;
                self.close(node, frames)
            }
        }
    }

    /// Consumes an infix operator and waits for its right operand.
    fn push(&mut self, ops: &mut Vec<Op>, operator: Binary, left: Node) -> Step {
        let token = self.advance();
        ops.push(Op::Binary {
            operator,
            left: Box::new(left),
            position: start_of(token),
        });
        Step::Operand(match operator {
            Binary::Or | Binary::And => Slot::Logical,
            _ => Slot::Operand,
        })
    }

    /// The presence suffix after `operand`, when one follows it.
    fn presence(&mut self, operand: Node) -> Result<Node, Refusal> {
        let is = self.peek();
        let Kind::Identifier(word) = &is.kind else {
            return Ok(operand);
        };
        if word != "is" && word != "IS" {
            return Ok(operand);
        }
        let negated = matches!(self.ahead().kind, Kind::NotOp(_));
        let last = self.token_at(self.index + 1 + usize::from(negated));
        let kind = match &last.kind {
            Kind::Identifier(word) => match word.as_str() {
                "blank" | "BLANK" => Presence::Blank,
                "present" | "PRESENT" => Presence::Present,
                "defined" | "DEFINED" => Presence::Defined,
                _ => return Ok(operand),
            },
            _ => return Ok(operand),
        };
        self.index += 2 + usize::from(negated);
        let span = between(operand.span.start, end_of(last));
        Ok(Node {
            kind: Expression::Presence {
                kind,
                negated,
                operand: Box::new(operand),
            },
            position: start_of(is),
            span,
        })
    }

    /// Folds the waiting operators `test` takes, the innermost first.
    fn fold_while(
        &self,
        ops: &mut Vec<Op>,
        mut node: Node,
        test: impl Fn(&Op) -> bool,
    ) -> Result<Node, Refusal> {
        while ops.last().is_some_and(&test) {
            let Some(op) = ops.pop() else { break };
            node = self.apply(op, node)?;
        }
        Ok(node)
    }

    /// One waiting operator over its right operand.
    fn apply(&self, op: Op, operand: Node) -> Result<Node, Refusal> {
        let (kind, position, span) = match op {
            Op::Not(position) => {
                let span = between(position, operand.span.end);
                let operand = Box::new(operand);
                (Expression::Not { operand }, position, span)
            }
            Op::Unary(operator, position) => {
                let span = between(position, operand.span.end);
                let operand = Box::new(operand);
                (Expression::Unary { operator, operand }, position, span)
            }
            Op::RelativeDate {
                direction,
                position,
                start,
            } => return self.relative_date(direction, position, start, operand),
            Op::Binary {
                operator,
                left,
                position,
            } => {
                let span = between(left.span.start, operand.span.end);
                let right = Box::new(operand);
                let kind = match operator {
                    Binary::Or => Expression::Or { left, right },
                    Binary::And => Expression::And { left, right },
                    Binary::Comparison(operator) => Expression::Comparison {
                        operator,
                        left,
                        right,
                    },
                    Binary::Membership(operator) => Expression::Membership {
                        operator,
                        left,
                        right,
                    },
                    Binary::Arithmetic(operator) => Expression::Arithmetic {
                        operator,
                        left,
                        right,
                    },
                };
                (kind, position, span)
            }
        };
        Ok(Node {
            kind,
            position,
            span,
        })
    }

    /// The current frame's expression is complete: what it sits inside
    /// takes it.
    fn close(&mut self, node: Node, frames: &mut Frames) -> Result<Step, Refusal> {
        let Some(barrier) = frames.close() else {
            self.leave();
            return Ok(Step::Done(node));
        };
        match barrier {
            Barrier::Root(_) => {
                self.leave();
                Ok(Step::Done(node))
            }
            Barrier::Paren(open) => {
                let close = self.expect(
                    &Kind::Rparen,
                    "expected_close_paren",
                    "Expected ')' but found",
                )?;
                self.leave();
                let mut node = node;
                node.span = between(open, end_of(close));
                Ok(Step::Primary(node))
            }
            Barrier::List { open, mut elements } => {
                elements.push(node);
                if self.at(&Kind::Comma) {
                    self.advance();
                    frames.open(Barrier::List { open, elements });
                    return Ok(Step::Operand(Slot::Logical));
                }
                let close = self.expect(
                    &Kind::Rbracket,
                    "expected_close_bracket",
                    "Expected ']' but found",
                )?;
                self.leave();
                Ok(Step::Primary(Node {
                    kind: Expression::List(elements),
                    position: open,
                    span: between(open, end_of(close)),
                }))
            }
            Barrier::Object {
                open,
                mut entries,
                key,
            } => {
                entries.push(Entry {
                    key: key.name,
                    style: key.style,
                    position: key.position,
                    span: key.span,
                    value: node,
                });
                if self.at(&Kind::Comma) {
                    self.advance();
                    let key = self.object_key()?;
                    frames.open(Barrier::Object { open, entries, key });
                    return Ok(Step::Operand(Slot::Logical));
                }
                let close = self.expect(
                    &Kind::Rbrace,
                    "expected_close_brace",
                    "Expected '}' but found",
                )?;
                self.leave();
                Ok(Step::Primary(Node {
                    kind: Expression::Object(entries),
                    position: open,
                    span: between(open, end_of(close)),
                }))
            }
            Barrier::Call {
                name,
                position,
                mut arguments,
            } => {
                arguments.push(node);
                if self.at(&Kind::Comma) {
                    self.advance();
                    frames.open(Barrier::Call {
                        name,
                        position,
                        arguments,
                    });
                    return Ok(Step::Operand(Slot::Logical));
                }
                let close = self.expect(
                    &Kind::Rparen,
                    "expected_close_paren",
                    "Expected ')' but found",
                )?;
                self.leave();
                Ok(Step::Primary(Node {
                    kind: Expression::Call { name, arguments },
                    position,
                    span: between(position, end_of(close)),
                }))
            }
            Barrier::Index { object, key } => {
                let close = self.expect(
                    &Kind::Rbracket,
                    "expected_close_bracket",
                    "Expected ']' but found",
                )?;
                self.leave();
                let span = between(object.span.start, end_of(close));
                Ok(Step::Postfix(Node {
                    kind: Expression::Index {
                        object,
                        key: Box::new(node),
                    },
                    position: key,
                    span,
                }))
            }
        }
    }
}

/// Whether `op` is an infix operator binding at `level` or tighter.
fn binary_at_least(op: &Op, level: u8) -> bool {
    matches!(op, Op::Binary { operator, .. } if operator.level() >= level)
}

fn arithmetic(kind: &Kind) -> Option<Arithmetic> {
    Some(match kind {
        Kind::Plus => Arithmetic::Add,
        Kind::Minus => Arithmetic::Subtract,
        Kind::Multiply => Arithmetic::Multiply,
        Kind::Divide => Arithmetic::Divide,
        Kind::Modulo => Arithmetic::Modulo,
        _ => return None,
    })
}

/// The relation a token starts after a complete addition: a comparison,
/// `in`, `contains`, or `intersects` in either spelling (an identifier, or
/// a function's name where a parenthesis follows it).
fn relation(kind: &Kind) -> Option<Binary> {
    Some(match kind {
        Kind::Gt => Binary::Comparison(Comparison::Gt),
        Kind::Lt => Binary::Comparison(Comparison::Lt),
        Kind::Gte => Binary::Comparison(Comparison::Gte),
        Kind::Lte => Binary::Comparison(Comparison::Lte),
        Kind::EqualEqual => Binary::Comparison(Comparison::EqualEqual),
        Kind::Ne => Binary::Comparison(Comparison::Ne),
        Kind::StrictEqual => Binary::Comparison(Comparison::StrictEq),
        Kind::StrictNe => Binary::Comparison(Comparison::StrictNe),
        Kind::InOp(_) => Binary::Membership(Membership::In),
        Kind::ContainsOp(_) => Binary::Membership(Membership::Contains),
        Kind::Identifier(word) | Kind::FunctionName(word)
            if word == "intersects" || word == "INTERSECTS" =>
        {
            Binary::Membership(Membership::Intersects)
        }
        _ => return None,
    })
}
