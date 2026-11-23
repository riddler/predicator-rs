//! The stack machine: it runs an instruction list over a context in
//! expression mode, as the reference's instruction-set text states it.
//!
//! Dispatch is table driven. [`OPCODES`] holds one row per opcode this
//! build runs, each reading its own operands into an [`Op`]; an opcode is
//! looked up by name before any operand is read, so an opcode the build
//! does not know, and one the instruction set has retired ([`RETIRED`]), is
//! refused by its name alone. An opcode a later tier adds is a row added to
//! the table and an arm added to [`Machine::step`].
//!
//! The arithmetic instructions live in `arithmetic`, reading into a value
//! (`access`, `bracket_access`) and the containment `in` and `contains`
//! share in `access`, and the rich types (`object_new`, `object_set`,
//! `duration`, `relative_date`) in `rich`.

mod access;
mod arithmetic;
mod rich;

use crate::compare::{self, Operator};
use crate::options::OnUnbound;
use crate::{
    Context, Error, EvaluationError, Instructions, Options, TypeMismatchError,
    UndefinedVariableError, Value,
};
use arithmetic::Arithmetic;

/// The program an entry point is given: what can be run as an instruction
/// list. The trait is sealed; it is implemented for `&Instructions`.
pub trait Program: sealed::Sealed {}

mod sealed {
    use crate::{Error, Instructions};
    use std::borrow::Cow;

    /// The one method a program has: the instruction list it runs.
    pub trait Sealed {
        /// The instruction list, or the error that stops it being one.
        fn resolve(&self) -> Result<Cow<'_, Instructions>, Error>;
    }

    impl Sealed for &Instructions {
        fn resolve(&self) -> Result<Cow<'_, Instructions>, Error> {
            Ok(Cow::Borrowed(*self))
        }
    }
}

impl Program for &Instructions {}

/// One instruction, its operands read.
#[derive(Debug)]
enum Op<'a> {
    Lit(&'a Value),
    Load(&'a str),
    Compare(Operator),
    Not,
    UnaryBang,
    UnaryMinus,
    JumpIfFalsyOrPop(usize),
    JumpIfTrueOrPop(usize),
    Arithmetic(Arithmetic),
    Access(&'a str),
    BracketAccess,
    In,
    Contains,
    MakeList(usize),
    ObjectNew,
    ObjectSet(&'a str),
    Duration(&'a [Value]),
    RelativeDate(&'a str),
}

/// One row of the dispatch table: an opcode's name and the reader of its
/// operands, which answers `None` for operands of the wrong shape.
struct Row {
    name: &'static str,
    read: for<'a> fn(&'a [Value]) -> Option<Op<'a>>,
}

/// Every opcode this build runs.
const OPCODES: &[Row] = &[
    Row {
        name: "lit",
        read: read_lit,
    },
    Row {
        name: "load",
        read: read_load,
    },
    Row {
        name: "compare",
        read: read_compare,
    },
    Row {
        name: "not",
        read: |operands| bare(operands, Op::Not),
    },
    Row {
        name: "unary_bang",
        read: |operands| bare(operands, Op::UnaryBang),
    },
    Row {
        name: "unary_minus",
        read: |operands| bare(operands, Op::UnaryMinus),
    },
    Row {
        name: "jump_if_falsy_or_pop",
        read: |operands| offset(operands).map(Op::JumpIfFalsyOrPop),
    },
    Row {
        name: "jump_if_true_or_pop",
        read: |operands| offset(operands).map(Op::JumpIfTrueOrPop),
    },
    Row {
        name: "add",
        read: |operands| bare(operands, Op::Arithmetic(Arithmetic::Add)),
    },
    Row {
        name: "subtract",
        read: |operands| bare(operands, Op::Arithmetic(Arithmetic::Subtract)),
    },
    Row {
        name: "multiply",
        read: |operands| bare(operands, Op::Arithmetic(Arithmetic::Multiply)),
    },
    Row {
        name: "divide",
        read: |operands| bare(operands, Op::Arithmetic(Arithmetic::Divide)),
    },
    Row {
        name: "modulo",
        read: |operands| bare(operands, Op::Arithmetic(Arithmetic::Modulo)),
    },
    Row {
        name: "access",
        read: read_access,
    },
    Row {
        name: "bracket_access",
        read: |operands| bare(operands, Op::BracketAccess),
    },
    Row {
        name: "in",
        read: |operands| bare(operands, Op::In),
    },
    Row {
        name: "contains",
        read: |operands| bare(operands, Op::Contains),
    },
    Row {
        name: "make_list",
        read: read_make_list,
    },
    Row {
        name: "object_new",
        read: |operands| bare(operands, Op::ObjectNew),
    },
    Row {
        name: "object_set",
        read: |operands| string(operands).map(Op::ObjectSet),
    },
    Row {
        name: "duration",
        read: read_duration,
    },
    Row {
        name: "relative_date",
        read: |operands| string(operands).map(Op::RelativeDate),
    },
];

/// The opcodes the instruction set has retired, each with the version that
/// removed it.
const RETIRED: &[(&str, u32)] = &[("and", 3), ("or", 3)];

fn read_lit(operands: &[Value]) -> Option<Op<'_>> {
    match operands {
        [value] => Some(Op::Lit(value)),
        _ => None,
    }
}

fn read_load(operands: &[Value]) -> Option<Op<'_>> {
    match operands {
        [Value::String(name)] => Some(Op::Load(name)),
        _ => None,
    }
}

fn read_access(operands: &[Value]) -> Option<Op<'_>> {
    match operands {
        [Value::String(property)] => Some(Op::Access(property)),
        _ => None,
    }
}

/// `make_list`'s count: one non-negative integer. A count past the address
/// space asks for more values than any stack holds.
fn read_make_list(operands: &[Value]) -> Option<Op<'_>> {
    match operands {
        [Value::Integer(count)] if *count >= 0 => {
            Some(Op::MakeList(usize::try_from(*count).unwrap_or(usize::MAX)))
        }
        _ => None,
    }
}

fn read_compare(operands: &[Value]) -> Option<Op<'_>> {
    match operands {
        [Value::String(name)] => Operator::named(name).map(Op::Compare),
        _ => None,
    }
}

/// `duration`'s operand: one list of unit pairs, each read when the
/// instruction runs.
fn read_duration(operands: &[Value]) -> Option<Op<'_>> {
    match operands {
        [Value::List(pairs)] => Some(Op::Duration(pairs)),
        _ => None,
    }
}

/// An opcode whose one operand is a string.
fn string(operands: &[Value]) -> Option<&str> {
    match operands {
        [Value::String(text)] => Some(text),
        _ => None,
    }
}

/// An opcode that takes no operand.
fn bare<'a>(operands: &[Value], op: Op<'a>) -> Option<Op<'a>> {
    operands.is_empty().then_some(op)
}

/// A jump's offset: one positive integer.
fn offset(operands: &[Value]) -> Option<usize> {
    match operands {
        // An offset past the address space jumps past every instruction.
        [Value::Integer(offset)] if *offset > 0 => {
            Some(usize::try_from(*offset).unwrap_or(usize::MAX))
        }
        _ => None,
    }
}

/// Reads `instruction`: its opcode by name first, then its operands.
fn decode(instruction: &Value) -> Result<Op<'_>, EvaluationError> {
    let unknown = |name: &str| {
        EvaluationError::new(
            "unknown_instruction",
            format!("unknown instruction: {name}"),
        )
    };
    let Value::List(parts) = instruction else {
        return Err(unknown("an instruction that is not a list"));
    };
    let Some((Value::String(name), operands)) = parts.split_first() else {
        return Err(unknown("an instruction that names no opcode"));
    };
    if let Some(row) = OPCODES.iter().find(|row| row.name == name) {
        return (row.read)(operands)
            .ok_or_else(|| unknown(&format!("{name} with operands it does not take")));
    }
    match RETIRED.iter().find(|(retired, _)| retired == name) {
        Some((_, version)) => Err(EvaluationError::new(
            "retired_opcode",
            format!("the instruction {name} was retired at instruction-set version {version}"),
        )),
        None => Err(unknown(name)),
    }
}

/// Why a run stopped.
enum Fault {
    /// An error, as it is.
    Error(Error),
    /// A type mismatch on an undefined operand, which the entry point
    /// reports as an unbound variable when the run loaded one.
    RejectedUndefined(TypeMismatchError),
}

impl From<EvaluationError> for Fault {
    fn from(error: EvaluationError) -> Fault {
        Fault::Error(error.into())
    }
}

/// The machine's state over one run.
struct Machine<'a> {
    context: &'a Context,
    options: &'a Options<'a>,
    on_unbound: OnUnbound,
    /// The current time, once this run has read the clock.
    now: Option<crate::DateTime>,
    stack: Vec<Value>,
    /// The first root this run loaded and did not find bound.
    unbound: Option<&'a str>,
}

fn insufficient(operation: &str, wanted: usize) -> EvaluationError {
    EvaluationError::new(
        "insufficient_operands",
        format!("{operation} takes {wanted} value(s) from the stack"),
    )
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Undefined => "undefined",
        Value::Bool(_) => "boolean",
        Value::Integer(_) => "integer",
        Value::Float(_) => "float",
        Value::String(_) => "string",
        Value::List(_) => "list",
        Value::Map(_) => "map",
        Value::Date(_) => "date",
        Value::DateTime(_) => "datetime",
        Value::Duration(_) => "duration",
    }
}

/// The refusal of `operand` by `operation`, which wanted `expected`.
fn mismatch(operation: &str, expected: &str, operand: &Value) -> Fault {
    let error = TypeMismatchError::new(
        operation,
        format!(
            "{operation} takes a {expected}, not a {}",
            type_name(operand)
        ),
    );
    match operand {
        Value::Undefined => Fault::RejectedUndefined(error),
        _ => Fault::Error(error.into()),
    }
}

/// The refusal of a pair by `operation`, which wanted a `wanted`. It names
/// an undefined operand when either is one, so that an unbound load is
/// reported as such.
fn mismatch_pair(operation: &str, wanted: &str, left: &Value, right: &Value) -> Fault {
    let error = TypeMismatchError::new(
        operation,
        format!(
            "{operation} takes a {wanted}, not a {} and a {}",
            type_name(left),
            type_name(right)
        ),
    );
    match (left, right) {
        (Value::Undefined, _) | (_, Value::Undefined) => Fault::RejectedUndefined(error),
        _ => Fault::Error(error.into()),
    }
}

impl<'a> Machine<'a> {
    /// Runs `list` from its first instruction to its end and answers the
    /// top of the stack.
    fn run(&mut self, list: &'a [Value]) -> Result<Value, Fault> {
        let mut at = 0;
        while let Some(instruction) = list.get(at) {
            at = self.step(decode(instruction)?, at)?;
        }
        self.stack.pop().ok_or_else(|| {
            EvaluationError::new("empty_stack", "the run ended with nothing on the stack").into()
        })
    }

    /// Runs one instruction at `at` and answers where the next one is.
    fn step(&mut self, op: Op<'a>, at: usize) -> Result<usize, Fault> {
        let next = at.saturating_add(1);
        match op {
            Op::Lit(value) => self.stack.push(value.clone()),
            Op::Load(name) => self.load(name)?,
            Op::Compare(operator) => {
                let (Some(right), Some(left)) = (self.stack.pop(), self.stack.pop()) else {
                    return Err(insufficient("compare", 2).into());
                };
                self.stack.push(compare::compare(operator, &left, &right));
            }
            Op::Not => self.negate("logical_not")?,
            Op::UnaryBang => self.negate("unary_bang")?,
            Op::UnaryMinus => {
                let top = self
                    .stack
                    .last_mut()
                    .ok_or_else(|| insufficient("unary_minus", 1))?;
                *top = match &*top {
                    Value::Integer(number) => {
                        Value::Integer(number.checked_neg().ok_or_else(|| {
                            EvaluationError::new(
                                "integer_overflow",
                                format!("-({number}) is outside the 64-bit integer range"),
                            )
                        })?)
                    }
                    Value::Float(number) => Value::Float(-number),
                    other => return Err(mismatch("unary_minus", "number", other)),
                };
            }
            Op::Arithmetic(operation) => {
                let (right, left) = self.pair(operation.name())?;
                self.stack.push(arithmetic::apply(operation, left, right)?);
            }
            Op::Access(property) => {
                let target = self.stack.pop().ok_or_else(|| insufficient("access", 1))?;
                self.stack.push(access::access(target, property));
            }
            Op::BracketAccess => {
                let (key, target) = self.pair("bracket_access")?;
                self.stack.push(access::bracket(target, &key)?);
            }
            Op::In => {
                let (container, needle) = self.pair("in")?;
                self.stack.push(access::containment(&container, &needle));
            }
            Op::Contains => {
                let (needle, container) = self.pair("contains")?;
                self.stack.push(access::containment(&container, &needle));
            }
            Op::MakeList(count) => {
                let at = self
                    .stack
                    .len()
                    .checked_sub(count)
                    .ok_or_else(|| insufficient("make_list", count))?;
                let items = self.stack.split_off(at);
                self.stack.push(Value::List(items));
            }
            Op::ObjectNew => self.stack.push(Value::Map(crate::Map::new())),
            Op::ObjectSet(key) => {
                let (value, target) = self.pair("object_set")?;
                self.stack.push(rich::object_set(target, key, value)?);
            }
            Op::Duration(pairs) => self.stack.push(Value::Duration(rich::duration(pairs)?)),
            Op::RelativeDate(direction) => {
                let top = self
                    .stack
                    .pop()
                    .ok_or_else(|| insufficient("relative_date", 1))?;
                let (duration, sign) = rich::relative(&top, direction)?;
                let now = self.now()?;
                self.stack.push(rich::shift(now, &duration, sign)?);
            }
            Op::JumpIfFalsyOrPop(offset) => {
                if !self.condition("jump_if_falsy_or_pop")? {
                    return Ok(at.saturating_add(offset));
                }
                self.stack.pop();
            }
            Op::JumpIfTrueOrPop(offset) => {
                if self.condition("jump_if_true_or_pop")? {
                    return Ok(at.saturating_add(offset));
                }
                self.stack.pop();
            }
        }
        Ok(next)
    }

    /// The top two values, popped: the top first, then the one beneath it.
    fn pair(&mut self, operation: &str) -> Result<(Value, Value), Fault> {
        let (Some(top), Some(beneath)) = (self.stack.pop(), self.stack.pop()) else {
            return Err(insufficient(operation, 2).into());
        };
        Ok((top, beneath))
    }

    /// The current time: the clock read the first time a run asks, and the
    /// same instant every later time. A clock with no time to give fails
    /// the run with the reason `clock_unavailable`, the one refusal every
    /// instruction that reads the time answers.
    fn now(&mut self) -> Result<crate::DateTime, EvaluationError> {
        if let Some(now) = self.now {
            return Ok(now);
        }
        let now = self.options.now().ok_or_else(|| {
            EvaluationError::new("clock_unavailable", "the clock gave no current time")
        })?;
        self.now = Some(now);
        Ok(now)
    }

    /// Pushes the value bound to the root `name`, or undefined when it is
    /// unbound (failing at once under [`OnUnbound::Error`]).
    fn load(&mut self, name: &'a str) -> Result<(), Fault> {
        match self.context.root(name) {
            Some(value) => self.stack.push(value.clone()),
            None if self.on_unbound == OnUnbound::Error => {
                return Err(Fault::Error(unbound(name).into()));
            }
            None => {
                self.unbound.get_or_insert(name);
                self.stack.push(Value::Undefined);
            }
        }
        Ok(())
    }

    /// `not` and `unary_bang`: the top of the stack, which must be a
    /// boolean, negated.
    fn negate(&mut self, operation: &str) -> Result<(), Fault> {
        let top = self
            .stack
            .last_mut()
            .ok_or_else(|| insufficient(operation, 1))?;
        match top {
            Value::Bool(flag) => *flag = !*flag,
            other => return Err(mismatch(operation, "boolean", other)),
        }
        Ok(())
    }

    /// A jump's condition, the top of the stack: `true` when it is exactly
    /// true, `false` when it is falsy (false, null or undefined).
    fn condition(&self, operation: &str) -> Result<bool, Fault> {
        match self.stack.last() {
            Some(Value::Bool(flag)) => Ok(*flag),
            Some(Value::Null | Value::Undefined) => Ok(false),
            Some(other) => Err(mismatch(operation, "boolean", other)),
            None => Err(insufficient(operation, 1).into()),
        }
    }
}

fn unbound(name: &str) -> UndefinedVariableError {
    UndefinedVariableError::new(
        "unbound_variable",
        format!("the variable {name} is not bound"),
    )
}

/// Evaluates `program` over `context` and answers the value it leaves on
/// top of the stack.
///
/// The program is an `&`[`Instructions`]. It runs in expression mode, as the
/// reference's instruction-set text states it: from its first instruction
/// to its end, and its answer is the top of the stack (a run that ends with
/// nothing on it fails with the reason `empty_stack`).
///
/// A variable the context does not bind is read as `options` says
/// ([`OnUnbound`]). Under the default, a run whose answer is undefined, or
/// that fails because an instruction refused an undefined operand, fails
/// instead with an [`UndefinedVariableError`] (reason `unbound_variable`)
/// naming the first unbound variable it loaded, when it loaded one.
///
/// Before anything runs, every value the context binds is checked against
/// the value domain ([`Context`]).
///
/// ```
/// use predicator_rs::{Context, Instructions, OnUnbound, Options, Value, evaluate};
///
/// let program = Instructions::from_json(r#"[["load","score"],["lit",85],["compare","GT"]]"#)?;
/// let mut context = Context::new();
/// context.insert("score", Value::from(90));
/// assert_eq!(evaluate(&program, &context, &Options::new())?, Value::from(true));
///
/// let unbound = evaluate(&program, &Context::new(), &Options::new()).unwrap_err();
/// assert_eq!(unbound.type_name(), "UndefinedVariableError");
/// assert_eq!(unbound.reason(), "unbound_variable");
///
/// let strict = Options::new().on_unbound(OnUnbound::Error);
/// assert_eq!(evaluate(&program, &Context::new(), &strict).unwrap_err().reason(), "unbound_variable");
/// # Ok::<(), predicator_rs::Error>(())
/// ```
pub fn evaluate<P: Program>(
    program: P,
    context: &Context,
    options: &Options,
) -> Result<Value, Error> {
    let instructions = program.resolve()?;
    context.check()?;
    let mut machine = Machine {
        context,
        options,
        on_unbound: options.on_unbound,
        now: None,
        stack: Vec::new(),
        unbound: None,
    };
    let outcome = machine.run(&instructions.list);
    let unbound_error = |first: Option<&str>| first.map(|name| Error::from(unbound(name)));
    match outcome {
        Ok(Value::Undefined) => match unbound_error(machine.unbound) {
            Some(error) => Err(error),
            None => Ok(Value::Undefined),
        },
        Ok(value) => Ok(value),
        Err(Fault::RejectedUndefined(error)) => {
            Err(unbound_error(machine.unbound).unwrap_or_else(|| error.into()))
        }
        Err(Fault::Error(error)) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_with(text: &str, context: &Context, options: &Options) -> Result<Value, Error> {
        evaluate(&Instructions::from_json(text).unwrap(), context, options)
    }

    fn run(text: &str) -> Result<Value, Error> {
        run_with(text, &Context::new(), &Options::new())
    }

    /// The type and reason of the error `text` fails with; an answer
    /// instead is reported in the same shape, so the assertion names it.
    fn refusal(text: &str) -> (String, String) {
        match run(text) {
            Ok(value) => ("no error".to_owned(), format!("{value:?}")),
            Err(error) => (error.type_name().to_owned(), error.reason().to_owned()),
        }
    }

    fn evaluation(reason: &str) -> (String, String) {
        ("EvaluationError".to_owned(), reason.to_owned())
    }

    fn mismatched(reason: &str) -> (String, String) {
        ("TypeMismatchError".to_owned(), reason.to_owned())
    }

    // Sabotage: with the retired lookup in decode skipped (every unknown
    // name answering unknown_instruction), this test fails.
    #[test]
    fn an_unknown_or_retired_opcode_is_refused_by_name_before_its_operands() {
        assert_eq!(refusal(r#"[["and"]]"#), evaluation("retired_opcode"));
        assert_eq!(
            refusal(r#"[["or",1,2,{"x":[]}]]"#),
            evaluation("retired_opcode")
        );
        for text in [
            r#"[["intersects"]]"#,
            r#"[["bogus",1]]"#,
            r#"[[]]"#,
            r#"[[5]]"#,
            r#"[5]"#,
            r#"[["lit",1],"lit"]"#,
        ] {
            assert_eq!(refusal(text), evaluation("unknown_instruction"), "{text}");
        }
        assert!(
            RETIRED
                .iter()
                .all(|(name, _)| OPCODES.iter().all(|row| row.name != *name))
        );
    }

    // Sabotage: with read_load accepting any operand as an empty name, this
    // test fails.
    #[test]
    fn operands_of_the_wrong_shape_are_an_unknown_instruction() {
        for text in [
            r#"[["lit"]]"#,
            r#"[["lit",1,2]]"#,
            r#"[["load",5]]"#,
            r#"[["load"]]"#,
            r#"[["lit",1],["lit",1],["compare","FOO"]]"#,
            r#"[["lit",1],["lit",1],["compare",1]]"#,
            r#"[["lit",true],["not",1]]"#,
            r#"[["lit",true],["unary_bang",null]]"#,
            r#"[["lit",1],["unary_minus",1]]"#,
            r#"[["lit",true],["jump_if_falsy_or_pop",0]]"#,
            r#"[["lit",true],["jump_if_true_or_pop",-1]]"#,
            r#"[["lit",true],["jump_if_true_or_pop",1.0]]"#,
            r#"[["lit",true],["jump_if_true_or_pop"]]"#,
            r#"[["lit",1],["lit",2],["add",1]]"#,
            r#"[["lit",{}],["access"]]"#,
            r#"[["lit",{}],["access",1]]"#,
            r#"[["lit",{}],["lit","a"],["bracket_access","a"]]"#,
            r#"[["lit",1],["lit",[1]],["in",1]]"#,
            r#"[["lit",[1]],["lit",1],["contains",null]]"#,
            r#"[["make_list",-1]]"#,
            r#"[["make_list",1.0]]"#,
            r#"[["make_list"]]"#,
        ] {
            assert_eq!(refusal(text), evaluation("unknown_instruction"), "{text}");
        }
    }

    // Sabotage: with run answering Value::Null for an empty stack, this test
    // fails.
    #[test]
    fn the_answer_is_the_top_of_the_stack() {
        assert_eq!(run(r#"[["lit",42]]"#).unwrap(), Value::from(42));
        assert_eq!(run(r#"[["lit",1],["lit",2]]"#).unwrap(), Value::from(2));
        assert_eq!(
            run(r#"[["lit",{"$type":"undefined"}]]"#).unwrap(),
            Value::Undefined
        );
        assert_eq!(refusal("[]"), evaluation("empty_stack"));
    }

    // Sabotage: with compare popping the left operand first, this test
    // fails.
    #[test]
    fn compare_pops_the_right_operand_then_the_left() {
        assert_eq!(
            run(r#"[["lit",3],["lit",5],["compare","LT"]]"#).unwrap(),
            Value::from(true)
        );
        assert_eq!(
            run(r#"[["lit",3],["lit",5],["lit",4],["compare","GT"]]"#).unwrap(),
            Value::from(true)
        );
        assert_eq!(
            refusal(r#"[["lit",1],["compare","EQ"]]"#),
            evaluation("insufficient_operands")
        );
        assert_eq!(
            refusal(r#"[["compare","EQ"]]"#),
            evaluation("insufficient_operands")
        );
    }

    // Sabotage: with negate's operation fixed to "logical_not", this test
    // fails.
    #[test]
    fn not_and_unary_bang_negate_a_boolean_and_refuse_anything_else() {
        assert_eq!(
            run(r#"[["lit",true],["not"]]"#).unwrap(),
            Value::from(false)
        );
        assert_eq!(
            run(r#"[["lit",false],["unary_bang"]]"#).unwrap(),
            Value::from(true)
        );
        assert_eq!(refusal(r#"[["lit",5],["not"]]"#), mismatched("logical_not"));
        assert_eq!(
            refusal(r#"[["lit",null],["not"]]"#),
            mismatched("logical_not")
        );
        assert_eq!(
            refusal(r#"[["lit",{"$type":"undefined"}],["unary_bang"]]"#),
            mismatched("unary_bang")
        );
        assert_eq!(refusal(r#"[["not"]]"#), evaluation("insufficient_operands"));
        assert_eq!(
            refusal(r#"[["unary_bang"]]"#),
            evaluation("insufficient_operands")
        );
    }

    // Sabotage: with checked_neg replaced by wrapping_neg, this test fails.
    #[test]
    fn unary_minus_negates_a_number_and_refuses_an_overflow() {
        assert_eq!(
            run(r#"[["lit",5],["unary_minus"]]"#).unwrap(),
            Value::from(-5)
        );
        assert_eq!(
            run(r#"[["lit",2.5],["unary_minus"]]"#).unwrap(),
            Value::from(-2.5)
        );
        assert_eq!(
            run(r#"[["lit",-9223372036854775807],["unary_minus"]]"#).unwrap(),
            Value::from(i64::MAX)
        );
        assert_eq!(
            refusal(r#"[["lit",-9223372036854775808],["unary_minus"]]"#),
            evaluation("integer_overflow")
        );
        assert_eq!(
            refusal(r#"[["lit","x"],["unary_minus"]]"#),
            mismatched("unary_minus")
        );
        assert_eq!(
            refusal(r#"[["unary_minus"]]"#),
            evaluation("insufficient_operands")
        );
    }

    // Sabotage: with condition reading null as true, this test fails.
    #[test]
    fn the_short_circuit_jumps_keep_the_value_they_jump_on() {
        let and =
            |left: &str| format!(r#"[["lit",{left}],["jump_if_falsy_or_pop",2],["lit","b"]]"#);
        let or = |left: &str| format!(r#"[["lit",{left}],["jump_if_true_or_pop",2],["lit","b"]]"#);
        assert_eq!(run(&and("true")).unwrap(), Value::from("b"));
        assert_eq!(run(&and("false")).unwrap(), Value::from(false));
        assert_eq!(run(&and("null")).unwrap(), Value::Null);
        assert_eq!(
            run(&and(r#"{"$type":"undefined"}"#)).unwrap(),
            Value::Undefined
        );
        assert_eq!(run(&or("true")).unwrap(), Value::from(true));
        assert_eq!(run(&or("false")).unwrap(), Value::from("b"));
        assert_eq!(run(&or("null")).unwrap(), Value::from("b"));
        assert_eq!(
            run(&or(r#"{"$type":"undefined"}"#)).unwrap(),
            Value::from("b")
        );
        assert_eq!(refusal(&and("1")), mismatched("jump_if_falsy_or_pop"));
        assert_eq!(refusal(&or("\"x\"")), mismatched("jump_if_true_or_pop"));
        assert_eq!(
            refusal(r#"[["jump_if_falsy_or_pop",1]]"#),
            evaluation("insufficient_operands")
        );
        assert_eq!(
            refusal(r#"[["jump_if_true_or_pop",1]]"#),
            evaluation("insufficient_operands")
        );
    }

    // Sabotage: with a taken jump answering at + offset + 1, this test
    // fails.
    #[test]
    fn a_jump_lands_at_its_offset_and_a_skipped_instruction_is_never_read() {
        assert_eq!(
            run(
                r#"[["lit",false],["jump_if_falsy_or_pop",2],["bogus"],["lit",1],["compare","EQ"]]"#
            )
            .ok(),
            Some(Value::Undefined)
        );
        assert_eq!(
            run(r#"[["lit",true],["jump_if_true_or_pop",2],["and"]]"#).unwrap(),
            Value::from(true)
        );
        assert_eq!(
            run(r#"[["lit",true],["jump_if_true_or_pop",9223372036854775807],["and"]]"#).unwrap(),
            Value::from(true)
        );
    }

    // Sabotage: with load recording the last unbound root instead of the
    // first, this test fails.
    #[test]
    fn an_unbound_root_is_reported_where_its_undefined_is_the_answer_or_refused() {
        let options = Options::new();
        let mut context = Context::new();
        context.insert("bound", Value::Undefined);
        context.insert("flag", Value::from(true));
        let at = |text: &str| run_with(text, &context, &options);
        let unbound_error = |text: &str| {
            let error = at(text).unwrap_err();
            (
                error.type_name().to_owned(),
                error.reason().to_owned(),
                error.message().to_owned(),
            )
        };
        let named = |name: &str| {
            (
                "UndefinedVariableError".to_owned(),
                "unbound_variable".to_owned(),
                format!("the variable {name} is not bound"),
            )
        };
        assert_eq!(unbound_error(r#"[["load","missing"]]"#), named("missing"));
        assert_eq!(
            unbound_error(r#"[["load","a"],["load","b"],["compare","EQ"]]"#),
            named("a")
        );
        assert_eq!(unbound_error(r#"[["load","a"],["not"]]"#), named("a"));
        assert_eq!(
            unbound_error(r#"[["load","a"],["pop_missing_opcode"]]"#).1,
            "unknown_instruction"
        );
        assert_eq!(at(r#"[["load","bound"]]"#).unwrap(), Value::Undefined);
        assert_eq!(
            at(r#"[["load","bound"],["not"]]"#).unwrap_err().type_name(),
            "TypeMismatchError"
        );
        assert_eq!(
            at(r#"[["load","flag"],["jump_if_true_or_pop",2],["load","skipped"]]"#).unwrap(),
            Value::from(true)
        );
        assert_eq!(
            at(r#"[["load","missing"],["jump_if_falsy_or_pop",2],["lit",1],["lit",true]]"#)
                .unwrap(),
            Value::from(true)
        );
        assert_eq!(
            at(r#"[["load","missing"],["lit",1],["unary_minus"],["not"]]"#)
                .unwrap_err()
                .type_name(),
            "TypeMismatchError"
        );
    }

    // Sabotage: with load ignoring on_unbound, this test fails.
    #[test]
    fn under_the_error_policy_a_load_of_an_unbound_root_fails_at_once() {
        let options = Options::new().on_unbound(OnUnbound::Error);
        let mut context = Context::new();
        context.insert("x", Value::Undefined);
        // Under the default this program answers true, the unbound load
        // skipped by the jump it feeds.
        let error = run_with(
            r#"[["load","missing"],["jump_if_falsy_or_pop",2],["lit",1],["lit",true]]"#,
            &context,
            &options,
        )
        .err()
        .map(|error| (error.type_name().to_owned(), error.reason().to_owned()));
        assert_eq!(
            error,
            Some((
                "UndefinedVariableError".to_owned(),
                "unbound_variable".to_owned()
            ))
        );
        assert_eq!(
            run_with(r#"[["load","x"]]"#, &context, &options).unwrap(),
            Value::Undefined
        );
    }

    // Sabotage: with evaluate skipping context.check, this test fails.
    #[test]
    fn a_context_outside_the_value_domain_is_refused_before_anything_runs() {
        let mut context = Context::new();
        context.insert("x", Value::Float(f64::INFINITY));
        let error = run_with(r#"[["lit",1]]"#, &context, &Options::new())
            .err()
            .map(|error| (error.type_name().to_owned(), error.reason().to_owned()));
        assert_eq!(
            error,
            Some(("EvaluationError".to_owned(), "non_finite_number".to_owned()))
        );
    }
}
