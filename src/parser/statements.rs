//! The statement grammar over the expression grammar, read without
//! recursion: what a statement, a block or an `if` waits for is held on an
//! explicit stack of tasks, and each expression inside is read whole by the
//! expression grammar.

use super::expression::Base;
use super::tree::{Block, Expression, Node, Program, Statement};
use super::{Descent, between, end_of, refuse, span_of, start_of};
use crate::Position;
use crate::lexer::{Kind, Refusal, Token};

/// What waits for the next statement, block or statement list to be read.
enum Task {
    /// A statement list ending at `terminator`, with the statements so far.
    Sequence {
        terminator: Kind,
        statements: Vec<Statement>,
    },
    /// A block's statements, the block opened at `open`.
    Block { open: Position },
    /// An `if`'s then block.
    Then { position: Position, condition: Node },
    /// An `if`'s else block.
    Else {
        position: Position,
        condition: Node,
        then_block: Block,
    },
    /// The `if` an `else if` holds.
    ElseIf,
    /// A `while`'s body.
    Body { position: Position, condition: Node },
}

/// What has just been read.
enum Read {
    Statement(Box<Statement>),
    Block(Block),
    Statements(Vec<Statement>),
}

/// What to read next.
enum Next {
    Statement,
    Block,
    /// An `if`, its keyword at the cursor.
    If,
    Done(Read),
}

impl Descent<'_> {
    pub(super) fn program(&mut self) -> Result<Program, Refusal> {
        let position = start_of(self.peek());
        let mut tasks = vec![Task::Sequence {
            terminator: Kind::Eof,
            statements: Vec::new(),
        }];
        let mut next = Next::Statement;
        loop {
            let read = match next {
                Next::Statement => match self.statement(&mut tasks)? {
                    Some(statement) => Read::Statement(Box::new(statement)),
                    None => {
                        next = Next::Block;
                        continue;
                    }
                },
                Next::Block => match self.open_block(&mut tasks)? {
                    Some(block) => Read::Block(block),
                    None => {
                        next = Next::Statement;
                        continue;
                    }
                },
                Next::If => {
                    self.if_statement(&mut tasks)?;
                    next = Next::Block;
                    continue;
                }
                Next::Done(read) => read,
            };
            let Some(task) = tasks.pop() else {
                return self.finish(position, read);
            };
            next = self.deliver(task, read, &mut tasks)?;
        }
    }

    /// The program's statements are read: the end of input must follow.
    fn finish(&self, position: Position, read: Read) -> Result<Program, Refusal> {
        let statements = match read {
            Read::Statements(statements) => statements,
            Read::Statement(statement) => vec![*statement],
            Read::Block(mut block) => core::mem::take(&mut block.statements),
        };
        let token = self.peek();
        match token.kind {
            Kind::Eof => {}
            Kind::ElseKw => return Err(stray_else(token)),
            _ => {
                return Err(self.unexpected(
                    "trailing_token",
                    |found| format!("Unexpected token {found} after statement"),
                    token,
                ));
            }
        }
        let start = statements
            .first()
            .map_or(position, |first| first.span().start);
        let end = statements.last().map_or(position, |last| last.span().end);
        Ok(Program {
            statements,
            position,
            span: between(start, end),
        })
    }

    /// Hands what was read to the task waiting for it and answers what to
    /// read next.
    fn deliver(&mut self, task: Task, read: Read, tasks: &mut Vec<Task>) -> Result<Next, Refusal> {
        Ok(match (task, read) {
            (
                Task::Sequence {
                    terminator,
                    mut statements,
                },
                Read::Statement(statement),
            ) => {
                let braced = matches!(*statement, Statement::If { .. } | Statement::While { .. });
                statements.push(*statement);
                let goes_on = if self.at(&Kind::Semicolon) {
                    self.advance();
                    !self.at(&terminator)
                } else {
                    braced && !self.at(&terminator)
                };
                if goes_on {
                    tasks.push(Task::Sequence {
                        terminator,
                        statements,
                    });
                    Next::Statement
                } else {
                    Next::Done(Read::Statements(statements))
                }
            }
            (Task::Block { open }, Read::Statements(statements)) => {
                let close = self.expect(
                    &Kind::Rbrace,
                    "expected_close_brace",
                    "Expected '}' to close the block but found",
                )?;
                self.leave();
                Next::Done(Read::Block(Block {
                    statements,
                    position: open,
                    span: between(open, end_of(close)),
                }))
            }
            (
                Task::Then {
                    position,
                    condition,
                },
                Read::Block(then_block),
            ) => {
                if !self.at(&Kind::ElseKw) {
                    let span = between(position, then_block.span.end);
                    return Ok(Next::Done(Read::Statement(Box::new(Statement::If {
                        condition,
                        then_block,
                        else_block: None,
                        position,
                        span,
                    }))));
                }
                self.advance();
                tasks.push(Task::Else {
                    position,
                    condition,
                    then_block,
                });
                if self.at(&Kind::IfKw) {
                    self.enter()?;
                    tasks.push(Task::ElseIf);
                    Next::If
                } else {
                    Next::Block
                }
            }
            (
                Task::Else {
                    position,
                    condition,
                    then_block,
                },
                Read::Block(else_block),
            ) => {
                let span = between(position, else_block.span.end);
                Next::Done(Read::Statement(Box::new(Statement::If {
                    condition,
                    then_block,
                    else_block: Some(else_block),
                    position,
                    span,
                })))
            }
            (Task::ElseIf, Read::Statement(nested)) => {
                self.leave();
                let (position, span) = (nested.position(), nested.span());
                Next::Done(Read::Block(Block {
                    statements: vec![*nested],
                    position,
                    span,
                }))
            }
            (
                Task::Body {
                    position,
                    condition,
                },
                Read::Block(body),
            ) => {
                let span = between(position, body.span.end);
                Next::Done(Read::Statement(Box::new(Statement::While {
                    condition,
                    body,
                    position,
                    span,
                })))
            }
            // Each task is pushed just before what it waits for is read,
            // and what is read is what that task waits for; a pairing that
            // does not hold is handed on as it is.
            (_, read) => Next::Done(read),
        })
    }

    /// One statement: an assignment or an expression is read whole, and an
    /// `if` or a `while` leaves its task and answers `None`, its block to
    /// be read next.
    fn statement(&mut self, tasks: &mut Vec<Task>) -> Result<Option<Statement>, Refusal> {
        let token = self.peek();
        match token.kind {
            Kind::IfKw => {
                self.if_statement(tasks)?;
                Ok(None)
            }
            Kind::WhileKw => {
                self.advance();
                let condition = self.expression()?;
                tasks.push(Task::Body {
                    position: start_of(token),
                    condition,
                });
                Ok(None)
            }
            Kind::ElseKw => Err(stray_else(token)),
            _ => self.assignment_or_expression().map(Some),
        }
    }

    /// An `if` and its condition, its keyword at the cursor; its then block
    /// is read next.
    fn if_statement(&mut self, tasks: &mut Vec<Task>) -> Result<(), Refusal> {
        let keyword = self.advance();
        let condition = self.expression()?;
        tasks.push(Task::Then {
            position: start_of(keyword),
            condition,
        });
        Ok(())
    }

    /// A block's opening brace: an empty block is read whole, and otherwise
    /// its statements are read next, one level deeper, and `None` answered.
    fn open_block(&mut self, tasks: &mut Vec<Task>) -> Result<Option<Block>, Refusal> {
        let open = start_of(self.expect(
            &Kind::Lbrace,
            "expected_open_brace",
            "Expected '{' to open a block but found",
        )?);
        if self.at(&Kind::Rbrace) {
            let close = self.advance();
            return Ok(Some(Block {
                statements: Vec::new(),
                position: open,
                span: between(open, end_of(close)),
            }));
        }
        self.enter()?;
        tasks.push(Task::Block { open });
        tasks.push(Task::Sequence {
            terminator: Kind::Rbrace,
            statements: Vec::new(),
        });
        Ok(None)
    }

    /// An assignment when an addition read from the cursor is followed by
    /// `=`, and an expression read again from the start otherwise: a probe
    /// that was refused, or no `=` after it, is discarded.
    fn assignment_or_expression(&mut self) -> Result<Statement, Refusal> {
        let start = self.index;
        let target = match self.read(Base::Addition) {
            Ok(target) if self.at(&Kind::Eq) => target,
            _ => {
                self.index = start;
                return self.expression().map(Statement::Expression);
            }
        };
        let equals = self.advance();
        if !is_location(&target) {
            return Err(refuse(
                "unassignable_location",
                "Left side of '=' must be an assignable location - an identifier, a property \
                 access, or a bracket access."
                    .to_owned(),
                span_of(equals),
            ));
        }
        let value = self.expression()?;
        let span = between(target.span.start, value.span.end);
        Ok(Statement::Assignment {
            target,
            value,
            position: start_of(equals),
            span,
        })
    }
}

/// Whether `node` is an identifier under any run of property and bracket
/// accesses.
fn is_location(node: &Node) -> bool {
    let mut node = node;
    loop {
        match &node.kind {
            Expression::Identifier(_) => return true,
            Expression::Property { object, .. } | Expression::Index { object, .. } => {
                node = object;
            }
            _ => return false,
        }
    }
}

fn stray_else(token: &Token) -> Refusal {
    refuse(
        "unexpected_else",
        "Unexpected 'else' - an 'else' block must follow an 'if' block.".to_owned(),
        span_of(token),
    )
}
