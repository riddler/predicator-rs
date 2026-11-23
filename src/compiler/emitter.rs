//! The compiler's last stage: the parser's tree in, the instruction list
//! out, as the reference's instructions visitor emits it.
//!
//! Every instruction is a list whose first member names the opcode and
//! whose other members are its operands, each a [`Value`], so the list is
//! the one [`Instructions::to_json`](crate::Instructions::to_json) writes in
//! the tagged encoding: an undefined, a date or an instant operand is
//! carried as its tag.
//!
//! The opcodes the emitter writes are one table ([`Opcode`], declared by
//! `opcodes!`), each row the opcode and its name; an opcode the
//! instruction set adds is one row there and the arm that emits it.
//!
//! A node is emitted after its operands (post-order), a short-circuit or a
//! statement's jump before the instructions it jumps over, its offset
//! counted once they are written. The walk does not recurse: what is still
//! to be written is held on an explicit stack of tasks, so a chain of
//! operators as long as the source (which the parser's depth bound does not
//! cover) is emitted on a stack that does not grow with it.

use crate::Value;
use crate::parser::tree::{
    Arithmetic, Block, Comparison, Direction, Expression, Literal, Membership, Node, Presence,
    Program, Statement, Unary,
};

/// Declares the opcode table: one row per opcode the emitter writes, the
/// variant and the name the instruction set gives it.
macro_rules! opcodes {
    ($($variant:ident => $name:literal,)*) => {
        /// An opcode the emitter writes.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum Opcode {
            $($variant,)*
        }

        impl Opcode {
            /// Every row of the table, in the order it is declared.
            #[cfg(test)]
            const ALL: &'static [Opcode] = &[$(Opcode::$variant,)*];

            /// The opcode's name in an instruction.
            fn name(self) -> &'static str {
                match self {
                    $(Opcode::$variant => $name,)*
                }
            }
        }
    };
}

opcodes! {
    Lit => "lit",
    Load => "load",
    Compare => "compare",
    Not => "not",
    UnaryBang => "unary_bang",
    UnaryMinus => "unary_minus",
    JumpIfFalsyOrPop => "jump_if_falsy_or_pop",
    JumpIfTrueOrPop => "jump_if_true_or_pop",
    Add => "add",
    Subtract => "subtract",
    Multiply => "multiply",
    Divide => "divide",
    Modulo => "modulo",
    Access => "access",
    BracketAccess => "bracket_access",
    In => "in",
    Contains => "contains",
    Intersects => "intersects",
    MakeList => "make_list",
    ObjectNew => "object_new",
    ObjectSet => "object_set",
    Duration => "duration",
    RelativeDate => "relative_date",
    Call => "call",
    Store => "store",
    Pop => "pop",
    Jump => "jump",
    PopJumpIfFalsy => "pop_jump_if_falsy",
    JumpBackward => "jump_backward",
    Cast => "cast",
    Presence => "presence",
}

/// What is still to be written, in the order a stack pops it.
enum Task<'a> {
    /// An expression node, its operands first.
    Node(&'a Node),
    /// A statement.
    Statement(&'a Statement),
    /// Statements, in order.
    Statements(&'a [Statement]),
    /// An instruction, finished.
    Emit(Value),
    /// A jump whose offset is counted at its [`Task::Close`]: it is written
    /// with a zero offset and its index kept.
    Open(Opcode),
    /// Counts the offset of the jump opened last: from it to the next
    /// instruction, plus `extra` instructions still to be written past it.
    Close(usize),
    /// Keeps the index of the next instruction, where a loop's back edge
    /// returns to.
    Mark,
    /// A loop's back edge, to the index kept last.
    BackEdge,
}

/// The instructions an expression runs as.
pub(super) fn expression(node: &Node) -> Vec<Value> {
    run(Task::Node(node))
}

/// The instructions a program runs as.
pub(super) fn program(program: &Program) -> Vec<Value> {
    run(Task::Statements(&program.statements))
}

/// Writes everything `first` stands for.
fn run(first: Task<'_>) -> Vec<Value> {
    let mut out = Vec::new();
    let mut kept: Vec<usize> = Vec::new();
    let mut tasks = vec![first];
    while let Some(task) = tasks.pop() {
        match task {
            Task::Node(node) => node_tasks(node, &mut tasks),
            Task::Statement(statement) => statement_tasks(statement, &mut tasks),
            Task::Statements(statements) => {
                tasks.extend(statements.iter().rev().map(Task::Statement));
            }
            Task::Emit(instruction) => out.push(instruction),
            Task::Open(opcode) => {
                kept.push(out.len());
                out.push(instruction(opcode, vec![Value::Integer(0)]));
            }
            Task::Close(extra) => {
                if let Some(at) = kept.pop() {
                    let offset = count(out.len() - at + extra);
                    if let Some(Value::List(parts)) = out.get_mut(at) {
                        if let Some(slot) = parts.get_mut(1) {
                            *slot = offset;
                        }
                    }
                }
            }
            Task::Mark => kept.push(out.len()),
            Task::BackEdge => {
                let back = kept.pop().map_or(0, |at| out.len() - at);
                out.push(instruction(Opcode::JumpBackward, vec![count(back)]));
            }
        }
    }
    out
}

/// Pushes what writes `node`, its operands first.
fn node_tasks<'a>(node: &'a Node, tasks: &mut Vec<Task<'a>>) {
    let emit = |opcode: Opcode, operands: Vec<Value>| Task::Emit(instruction(opcode, operands));
    match &node.kind {
        Expression::Literal(value) => tasks.push(emit(Opcode::Lit, vec![literal(value)])),
        Expression::String { value, .. } => {
            tasks.push(emit(Opcode::Lit, vec![Value::from(value.as_str())]));
        }
        Expression::Identifier(name) => {
            tasks.push(emit(Opcode::Load, vec![Value::from(name.as_str())]));
        }
        Expression::Comparison {
            operator,
            left,
            right,
        } => {
            tasks.push(emit(
                Opcode::Compare,
                vec![Value::from(comparison(*operator))],
            ));
            tasks.push(Task::Node(right));
            tasks.push(Task::Node(left));
        }
        Expression::Arithmetic {
            operator,
            left,
            right,
        } => {
            tasks.push(emit(arithmetic(*operator), Vec::new()));
            tasks.push(Task::Node(right));
            tasks.push(Task::Node(left));
        }
        Expression::Membership {
            operator,
            left,
            right,
        } => {
            tasks.push(emit(membership(*operator), Vec::new()));
            tasks.push(Task::Node(right));
            tasks.push(Task::Node(left));
        }
        Expression::Presence {
            kind,
            negated,
            operand,
        } => {
            // `is present` is `is not blank`: one presence opcode, and a
            // `not` after it for a negation.
            let (word, negate) = match kind {
                Presence::Blank => ("blank", *negated),
                Presence::Present => ("blank", !*negated),
                Presence::Defined => ("defined", *negated),
            };
            if negate {
                tasks.push(emit(Opcode::Not, Vec::new()));
            }
            tasks.push(emit(Opcode::Presence, vec![Value::from(word)]));
            tasks.push(Task::Node(operand));
        }
        Expression::Unary { operator, operand } => {
            let opcode = match operator {
                Unary::Minus => Opcode::UnaryMinus,
                Unary::Bang => Opcode::UnaryBang,
            };
            tasks.push(emit(opcode, Vec::new()));
            tasks.push(Task::Node(operand));
        }
        Expression::And { left, right } => {
            short_circuit(Opcode::JumpIfFalsyOrPop, left, right, tasks);
        }
        Expression::Or { left, right } => {
            short_circuit(Opcode::JumpIfTrueOrPop, left, right, tasks);
        }
        Expression::Not { operand } => {
            tasks.push(emit(Opcode::Not, Vec::new()));
            tasks.push(Task::Node(operand));
        }
        Expression::List(elements) => match literal_list(elements) {
            // A list of literals is one `lit` of the list.
            Some(values) => tasks.push(emit(Opcode::Lit, vec![Value::List(values)])),
            None => {
                tasks.push(emit(Opcode::MakeList, vec![count(elements.len())]));
                tasks.extend(elements.iter().rev().map(Task::Node));
            }
        },
        Expression::Object(entries) => {
            for entry in entries.iter().rev() {
                tasks.push(emit(
                    Opcode::ObjectSet,
                    vec![Value::from(entry.key.as_str())],
                ));
                tasks.push(Task::Node(&entry.value));
            }
            tasks.push(emit(Opcode::ObjectNew, Vec::new()));
        }
        Expression::Call { name, arguments } => {
            tasks.push(emit(
                Opcode::Call,
                vec![Value::from(name.as_str()), count(arguments.len())],
            ));
            tasks.extend(arguments.iter().rev().map(Task::Node));
        }
        Expression::Index { object, key } => {
            tasks.push(emit(Opcode::BracketAccess, Vec::new()));
            tasks.push(Task::Node(key));
            tasks.push(Task::Node(object));
        }
        Expression::Property { object, name } => {
            tasks.push(emit(Opcode::Access, vec![Value::from(name.as_str())]));
            tasks.push(Task::Node(object));
        }
        Expression::Cast { operand, type_name } => {
            tasks.push(emit(Opcode::Cast, vec![Value::from(*type_name)]));
            tasks.push(Task::Node(operand));
        }
        Expression::Duration(units) => {
            let units = units
                .iter()
                .map(|&(amount, unit)| Value::List(vec![Value::Integer(amount), Value::from(unit)]))
                .collect();
            tasks.push(emit(Opcode::Duration, vec![Value::List(units)]));
        }
        Expression::RelativeDate {
            duration,
            direction,
        } => {
            tasks.push(emit(
                Opcode::RelativeDate,
                vec![Value::from(relative(*direction))],
            ));
            tasks.push(Task::Node(duration));
        }
    }
}

/// `left`, a jump past `right` that keeps `left` when it decides the
/// answer, then `right`.
fn short_circuit<'a>(opcode: Opcode, left: &'a Node, right: &'a Node, tasks: &mut Vec<Task<'a>>) {
    tasks.push(Task::Close(0));
    tasks.push(Task::Node(right));
    tasks.push(Task::Open(opcode));
    tasks.push(Task::Node(left));
}

/// Pushes what writes `statement`. Every statement leaves the stack as it
/// found it: an assignment ends in its `store`, an expression in a `pop`,
/// and an `if` or a `while` consumes its condition.
fn statement_tasks<'a>(statement: &'a Statement, tasks: &mut Vec<Task<'a>>) {
    match statement {
        Statement::Expression(node) => {
            tasks.push(Task::Emit(instruction(Opcode::Pop, Vec::new())));
            tasks.push(Task::Node(node));
        }
        Statement::Assignment { target, value, .. } => assignment(target, value, tasks),
        // c; pop_jump_if_falsy past A; A  or, with an else block,
        // c; pop_jump_if_falsy past A and the jump; A; jump past B; B
        Statement::If {
            condition,
            then_block,
            else_block,
            ..
        } => match else_block {
            None => {
                tasks.push(Task::Close(0));
                tasks.push(block(then_block));
                tasks.push(Task::Open(Opcode::PopJumpIfFalsy));
                tasks.push(Task::Node(condition));
            }
            Some(otherwise) => {
                tasks.push(Task::Close(0));
                tasks.push(block(otherwise));
                tasks.push(Task::Open(Opcode::Jump));
                tasks.push(Task::Close(1));
                tasks.push(block(then_block));
                tasks.push(Task::Open(Opcode::PopJumpIfFalsy));
                tasks.push(Task::Node(condition));
            }
        },
        // c; pop_jump_if_falsy past A and the back edge; A; back to c
        Statement::While {
            condition, body, ..
        } => {
            tasks.push(Task::BackEdge);
            tasks.push(Task::Close(1));
            tasks.push(block(body));
            tasks.push(Task::Open(Opcode::PopJumpIfFalsy));
            tasks.push(Task::Node(condition));
            tasks.push(Task::Mark);
        }
    }
}

fn block(block: &Block) -> Task<'_> {
    Task::Statements(&block.statements)
}

/// `target = value`: one segment per step of the location, root first (a
/// name as a `lit`, a bracket's key as its own instructions), the value,
/// then a `store` of that many segments. The location is walked in a loop,
/// so a path as long as the source costs no stack.
fn assignment<'a>(target: &'a Node, value: &'a Node, tasks: &mut Vec<Task<'a>>) {
    let lit = |name: &str| Task::Emit(instruction(Opcode::Lit, vec![Value::from(name)]));
    let mut steps = Vec::new();
    let mut at = target;
    let root = loop {
        match &at.kind {
            Expression::Property { object, name } => {
                steps.push(lit(name));
                at = object;
            }
            Expression::Index { object, key } => {
                steps.push(Task::Node(key));
                at = object;
            }
            Expression::Identifier(name) => break lit(name),
            // The parser refuses any other location; were one here, its
            // instructions stand for the root.
            _ => break Task::Node(at),
        }
    };
    tasks.push(Task::Emit(instruction(
        Opcode::Store,
        vec![count(steps.len() + 1)],
    )));
    tasks.push(Task::Node(value));
    tasks.extend(steps);
    tasks.push(root);
}

/// An instruction: the opcode's name, then its operands.
fn instruction(opcode: Opcode, operands: Vec<Value>) -> Value {
    let mut parts = Vec::with_capacity(operands.len() + 1);
    parts.push(Value::from(opcode.name()));
    parts.extend(operands);
    Value::List(parts)
}

/// A count or an offset as an integer operand.
fn count(n: usize) -> Value {
    Value::Integer(i64::try_from(n).unwrap_or(i64::MAX))
}

fn literal(literal: &Literal) -> Value {
    match literal {
        Literal::Integer(n) => Value::Integer(*n),
        Literal::Float(x) => Value::Float(*x),
        Literal::Boolean(b) => Value::Bool(*b),
        Literal::Undefined => Value::Undefined,
        Literal::Null => Value::Null,
        Literal::Date(date) => Value::Date(*date),
        Literal::DateTime(instant, _) => Value::DateTime(*instant),
    }
}

/// The values of a list whose every element is a literal or a string, or
/// `None` when one is anything else.
fn literal_list(elements: &[Node]) -> Option<Vec<Value>> {
    elements
        .iter()
        .map(|element| match &element.kind {
            Expression::Literal(value) => Some(literal(value)),
            Expression::String { value, .. } => Some(Value::from(value.as_str())),
            _ => None,
        })
        .collect()
}

fn comparison(operator: Comparison) -> &'static str {
    match operator {
        Comparison::Gt => "GT",
        Comparison::Lt => "LT",
        Comparison::Gte => "GTE",
        Comparison::Lte => "LTE",
        Comparison::EqualEqual => "EQ",
        Comparison::Ne => "NE",
        Comparison::StrictEq => "STRICT_EQ",
        Comparison::StrictNe => "STRICT_NE",
    }
}

fn arithmetic(operator: Arithmetic) -> Opcode {
    match operator {
        Arithmetic::Add => Opcode::Add,
        Arithmetic::Subtract => Opcode::Subtract,
        Arithmetic::Multiply => Opcode::Multiply,
        Arithmetic::Divide => Opcode::Divide,
        Arithmetic::Modulo => Opcode::Modulo,
    }
}

fn membership(operator: Membership) -> Opcode {
    match operator {
        Membership::In => Opcode::In,
        Membership::Contains => Opcode::Contains,
        Membership::Intersects => Opcode::Intersects,
    }
}

fn relative(direction: Direction) -> &'static str {
    match direction {
        Direction::Ago => "ago",
        Direction::Future => "future",
        Direction::Next => "next",
        Direction::Last => "last",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Instructions, compile, compile_program, tagged};

    fn json(instructions: &Instructions) -> String {
        tagged::encode(&Value::List(instructions.list.clone())).unwrap()
    }

    /// Runs `check` on a thread whose stack is 256 KiB.
    fn on_a_small_stack(check: impl FnOnce() + Send + 'static) {
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(check)
            .unwrap()
            .join()
            .unwrap();
    }

    // Every opcode in the table has a name of its own, and each is one the
    // evaluator runs, so nothing the compiler writes is refused as unknown.
    // Sabotage: with the Intersects row named "intersect", this test fails.
    #[test]
    fn every_opcode_the_emitter_writes_is_one_the_evaluator_runs() {
        let names: Vec<&str> = Opcode::ALL.iter().map(|opcode| opcode.name()).collect();
        let mut distinct = names.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), names.len());
        for name in names {
            assert!(
                crate::evaluator::OPCODES.iter().any(|row| row.name == name),
                "{name}"
            );
        }
    }

    // The presence forms: one presence opcode, blank or defined, and a not
    // after it for a negation; `is present` is `is not blank`.
    // Sabotage: with Presence::Present mapped to ("blank", negated), this
    // test fails on `x is present`.
    #[test]
    fn a_presence_negation_is_the_opcode_and_then_not() {
        let cases = [
            ("x is blank", r#"[["load","x"],["presence","blank"]]"#),
            (
                "x is not blank",
                r#"[["load","x"],["presence","blank"],["not"]]"#,
            ),
            (
                "x is present",
                r#"[["load","x"],["presence","blank"],["not"]]"#,
            ),
            ("x is not present", r#"[["load","x"],["presence","blank"]]"#),
            ("x is defined", r#"[["load","x"],["presence","defined"]]"#),
            (
                "x IS NOT DEFINED",
                r#"[["load","x"],["presence","defined"],["not"]]"#,
            ),
        ];
        for (source, want) in cases {
            assert_eq!(json(&compile(source).unwrap()), want, "{source}");
        }
    }

    // A list of literals is one lit, and a list holding anything else is
    // built by make_list after its elements; an operand that JSON cannot
    // carry is written as its tag.
    // Sabotage: with literal_list answering None for a string element, this
    // test fails on the first source.
    #[test]
    fn a_list_of_literals_is_one_lit() {
        let cases = [
            (
                r#"[1, 'a', null, undefined]"#,
                r#"[["lit",[1,"a",null,{"$type":"undefined"}]]]"#,
            ),
            ("[x, 1]", r#"[["load","x"],["lit",1],["make_list",2]]"#),
            ("[[]]", r#"[["lit",[]],["make_list",1]]"#),
            ("[1d]", r#"[["duration",[[1,"d"]]],["make_list",1]]"#),
        ];
        for (source, want) in cases {
            assert_eq!(json(&compile(source).unwrap()), want, "{source}");
        }
    }

    // A jump's offset counts the instructions it passes over, and a back
    // edge returns to the loop's condition, nested loops included.
    // Sabotage: with BackEdge counting one fewer, this test fails on the
    // while loop.
    #[test]
    fn jumps_count_what_they_pass_over() {
        let cases = [
            (
                "a AND b OR c",
                r#"[["load","a"],["jump_if_falsy_or_pop",2],["load","b"],["jump_if_true_or_pop",2],["load","c"],["pop"]]"#,
            ),
            (
                "while a { b = 1 }",
                r#"[["load","a"],["pop_jump_if_falsy",5],["lit","b"],["lit",1],["store",1],["jump_backward",5]]"#,
            ),
            (
                "if a { b } else { c }",
                r#"[["load","a"],["pop_jump_if_falsy",4],["load","b"],["pop"],["jump",3],["load","c"],["pop"]]"#,
            ),
            (
                "a[k + 1].b = 2",
                r#"[["lit","a"],["load","k"],["lit",1],["add"],["lit","b"],["lit",2],["store",3]]"#,
            ),
        ];
        for (source, want) in cases {
            assert_eq!(json(&compile_program(source).unwrap()), want, "{source}");
        }
    }

    // A chain as long as the source, of infix operators, postfix steps,
    // short circuits and an assignment's location, emits on a 256 KiB
    // stack, and so does a program of many statements.
    // Sabotage: with assignment leaving out a property step's segment, this
    // test fails on the location's length.
    #[test]
    fn a_chain_as_long_as_the_source_emits_on_a_small_stack() {
        on_a_small_stack(|| {
            let sum = compile(&format!("x{}", " + x".repeat(50_000))).unwrap();
            assert_eq!(sum.list.len(), 100_001);
            let postfix = compile(&format!("x{}", ".y[0]::string".repeat(20_000))).unwrap();
            assert_eq!(postfix.list.len(), 80_001);
            let ands = compile(&format!("x{}", " AND x".repeat(50_000))).unwrap();
            assert_eq!(ands.list.len(), 100_001);
            let compared = compile(&format!("x{} == 1", " - 1".repeat(50_000))).unwrap();
            assert_eq!(compared.list.len(), 100_003);
            let location = compile_program(&format!("x{} = 1", ".y".repeat(50_000))).unwrap();
            assert_eq!(location.list.len(), 50_003);
            let statements = compile_program(&"x = x + 1;".repeat(20_000)).unwrap();
            assert_eq!(statements.list.len(), 100_000);
        });
    }
}
