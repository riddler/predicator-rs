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
//! share in `access`, the rich types (`object_new`, `object_set`,
//! `duration`, `relative_date`) in `rich`, the function library `call`
//! runs in `library`, the write `store` makes in `store`, the conversions
//! `cast` makes in `cast`, and `presence` and `intersects` in `presence`.
//!
//! One machine runs both of the instruction set's modes: [`evaluate`]
//! (expression mode) answers the top of the stack, and [`execute`] and
//! [`execute_value`] (statement mode) answer the context the run leaves.

mod access;
mod arithmetic;
mod cast;
mod library;
mod presence;
mod rich;
mod store;

use crate::compare::{self, Operator};
use crate::options::OnUnbound;
use crate::{
    Context, Error, EvaluationError, Execution, Instructions, Key, Options, Stopped,
    TypeMismatchError, UndefinedVariableError, Value,
};
use arithmetic::Arithmetic;
use cast::Target;
use presence::Presence;
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;

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
    Call(&'a str, usize),
    Store(usize),
    Pop,
    Jump(usize),
    PopJumpIfFalsy(usize),
    JumpBackward(usize),
    Cast(Target),
    Presence(Presence),
    Intersects,
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
    Row {
        name: "call",
        read: read_call,
    },
    Row {
        name: "store",
        read: read_store,
    },
    Row {
        name: "pop",
        read: |operands| bare(operands, Op::Pop),
    },
    Row {
        name: "jump",
        read: |operands| offset(operands).map(Op::Jump),
    },
    Row {
        name: "pop_jump_if_falsy",
        read: |operands| offset(operands).map(Op::PopJumpIfFalsy),
    },
    Row {
        name: "jump_backward",
        read: |operands| offset(operands).map(Op::JumpBackward),
    },
    Row {
        name: "cast",
        read: |operands| string(operands).and_then(Target::named).map(Op::Cast),
    },
    Row {
        name: "presence",
        read: |operands| string(operands).and_then(Presence::named).map(Op::Presence),
    },
    Row {
        name: "intersects",
        read: |operands| bare(operands, Op::Intersects),
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

/// `call`'s operands: the function's name and a non-negative argument
/// count. A count past the address space asks for more values than any
/// stack holds.
fn read_call(operands: &[Value]) -> Option<Op<'_>> {
    match operands {
        [Value::String(name), Value::Integer(count)] if *count >= 0 => Some(Op::Call(
            name,
            usize::try_from(*count).unwrap_or(usize::MAX),
        )),
        _ => None,
    }
}

/// `store`'s count of path segments: one non-negative integer. A count
/// past the address space asks for more values than any stack holds.
fn read_store(operands: &[Value]) -> Option<Op<'_>> {
    match operands {
        [Value::Integer(count)] if *count >= 0 => {
            Some(Op::Store(usize::try_from(*count).unwrap_or(usize::MAX)))
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

/// The refusal of an instruction this build cannot run.
fn unknown(name: &str) -> EvaluationError {
    EvaluationError::new(
        "unknown_instruction",
        format!("unknown instruction: {name}"),
    )
}

/// Reads `instruction`: its opcode by name first, then its operands.
fn decode(instruction: &Value) -> Result<Op<'_>, EvaluationError> {
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
    /// The context the run reads, copied the first time a `store` writes
    /// it.
    context: Cow<'a, Context>,
    options: &'a Options<'a>,
    /// Whether a `store` reads the protected roots: statement mode only.
    protect: bool,
    /// The back edges the run may still take.
    budget: u64,
    /// The roots the run stored into, in the order of the first store, and
    /// the same roots as a set.
    stored: Vec<Key>,
    stored_set: BTreeSet<Key>,
    /// What the last `pop` discarded.
    last: Value,
    on_unbound: OnUnbound,
    /// The current time, once this run has read the clock.
    now: Option<crate::DateTime>,
    stack: Vec<Value>,
    /// The first root this run loaded and did not find bound.
    unbound: Option<&'a str>,
    /// The default random source, once this run has drawn from it: hash
    /// keys from the standard library and a count of draws.
    random: Option<(RandomState, u64)>,
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
    /// A machine about to run over `context` with `options`; `protect` says
    /// whether a `store` reads the protected roots.
    fn new(context: &'a Context, options: &'a Options<'a>, protect: bool) -> Machine<'a> {
        Machine {
            context: Cow::Borrowed(context),
            options,
            protect,
            budget: options.loop_budget,
            stored: Vec::new(),
            stored_set: BTreeSet::new(),
            last: Value::Undefined,
            on_unbound: options.on_unbound,
            now: None,
            stack: Vec::new(),
            unbound: None,
            random: None,
        }
    }

    /// Runs `list` from its first instruction until the instruction pointer
    /// reaches or passes its end.
    fn run(&mut self, list: &'a [Value]) -> Result<(), Fault> {
        let mut at = 0;
        while let Some(instruction) = list.get(at) {
            at = self.step(decode(instruction)?, at)?;
        }
        Ok(())
    }

    /// `store`: pops the value, then `count` path segments beneath it (the
    /// root deepest), and writes the value at that path. A path whose root
    /// the options protect is refused, in statement mode, after its
    /// segments are checked and before anything is written.
    fn store(&mut self, count: usize) -> Result<(), Fault> {
        let wanted = count.saturating_add(1);
        let at = self
            .stack
            .len()
            .checked_sub(wanted)
            .ok_or_else(|| insufficient("store", wanted))?;
        let mut taken = self.stack.split_off(at);
        let value = taken.pop().unwrap_or(Value::Undefined);
        let path = store::segments(&taken)?;
        let root = path.first().copied();
        if let Some(store::Segment::Key(name)) = root {
            if self.protect && self.options.protects(name) {
                return Err(EvaluationError::new(
                    "protected_root",
                    format!("store cannot write the protected root {name}"),
                )
                .with_root(name)
                .into());
            }
        }
        store::check(self.context.bindings(), &path)?;
        store::write(self.context.to_mut().bindings_mut(), &path, value);
        if let Some(root) = root.map(store::Segment::key) {
            if self.stored_set.insert(root.clone()) {
                self.stored.push(root);
            }
        }
        Ok(())
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
            Op::Cast(target) => {
                let value = self.stack.pop().ok_or_else(|| insufficient("cast", 1))?;
                self.stack.push(cast::cast(value, target)?);
            }
            Op::Presence(question) => {
                let value = self
                    .stack
                    .pop()
                    .ok_or_else(|| insufficient("presence", 1))?;
                self.stack.push(presence::presence(&value, question));
            }
            Op::Intersects => {
                let (right, left) = self.pair("intersects")?;
                self.stack.push(presence::intersects(&left, &right));
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
            Op::Call(name, count) => {
                let depth = self.stack.len();
                let at = depth.checked_sub(count).ok_or_else(|| {
                    EvaluationError::new(
                        "insufficient_arguments",
                        format!(
                            "Function {name}() expects {count} arguments, but only {depth} values on stack"
                        ),
                    )
                })?;
                let arguments = self.stack.split_off(at);
                let answer = self.call(name, &arguments)?;
                self.stack.push(answer);
            }
            Op::Store(count) => self.store(count)?,
            Op::Pop => {
                self.last = self.stack.pop().ok_or_else(|| insufficient("pop", 1))?;
            }
            Op::Jump(offset) => return Ok(at.saturating_add(offset)),
            Op::PopJumpIfFalsy(offset) => {
                let top = self
                    .stack
                    .pop()
                    .ok_or_else(|| insufficient("pop_jump_if_falsy", 1))?;
                match top {
                    Value::Bool(true) => {}
                    Value::Bool(false) | Value::Null | Value::Undefined => {
                        return Ok(at.saturating_add(offset));
                    }
                    other => return Err(mismatch("pop_jump_if_falsy", "boolean", &other)),
                }
            }
            Op::JumpBackward(offset) => {
                let target = at
                    .checked_sub(offset)
                    .ok_or_else(|| unknown("jump_backward to before the first instruction"))?;
                self.budget = self.budget.checked_sub(1).ok_or_else(|| {
                    EvaluationError::new(
                        "loop_budget_exceeded",
                        "the loop budget is spent at a backward jump",
                    )
                })?;
                return Ok(target);
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

    /// Calls the function `name` with `arguments`: the host function the
    /// options pass under that name, else the library function, else none
    /// (`Unknown function: NAME`). A host function's answer is checked
    /// against the value domain; a library function's arity is checked
    /// before its body runs. Every refusal is an [`EvaluationError`].
    fn call(&mut self, name: &str, arguments: &[Value]) -> Result<Value, EvaluationError> {
        if let Some(function) = self.options.host_function(name) {
            let answer = function(arguments).map_err(library::refused)?;
            crate::boundary::check(&answer)?;
            return Ok(answer);
        }
        let Some(function) = library::find(name) else {
            return Err(library::refused(format!("Unknown function: {name}")));
        };
        if !function.arity.admits(arguments.len()) {
            return Err(library::refused(format!(
                "Function {name}() expects {} arguments, got {}",
                function.arity.describe(),
                arguments.len()
            )));
        }
        (function.body)(arguments, self)
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

impl library::Run for Machine<'_> {
    fn now(&mut self) -> Result<crate::DateTime, EvaluationError> {
        Machine::now(self)
    }

    /// A draw from the source the options name, or from the default: the
    /// run's own hash keys over a count of its draws.
    fn draw(&mut self) -> u64 {
        if let Some(random) = self.options.random_source() {
            return random();
        }
        let (keys, draws) = self.random.get_or_insert_with(|| (RandomState::new(), 0));
        *draws = draws.wrapping_add(1);
        keys.hash_one(*draws)
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
/// The run may take as many back edges (the `jump_backward` instruction)
/// as [`Options::loop_budget`] says, and fails with the reason
/// `loop_budget_exceeded` at one past that. A `store` writes a copy of
/// the context that later instructions in the run read; the context passed
/// in is never changed, and [`Options::protected_roots`] is not read.
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
    let mut machine = Machine::new(context, options, false);
    let outcome = machine.run(&instructions.list).and_then(|()| {
        machine.stack.pop().ok_or_else(|| {
            EvaluationError::new("empty_stack", "the run ended with nothing on the stack").into()
        })
    });
    match outcome {
        Ok(Value::Undefined) => match machine.unbound {
            Some(name) => Err(unbound(name).into()),
            None => Ok(Value::Undefined),
        },
        Ok(value) => Ok(value),
        Err(fault) => Err(fault_error(fault, machine.unbound)),
    }
}

/// The error a run that stopped on `fault` answers: a refused undefined
/// operand is reported as the first unbound variable the run loaded, when it
/// loaded one.
fn fault_error(fault: Fault, first_unbound: Option<&str>) -> Error {
    match fault {
        Fault::RejectedUndefined(error) => match first_unbound {
            Some(name) => unbound(name).into(),
            None => error.into(),
        },
        Fault::Error(error) => error,
    }
}

/// Runs `program` over `context` in statement mode, keeping what the last
/// `pop` discarded when `keep_value` says so.
fn statements<P: Program>(
    program: P,
    context: &Context,
    options: &Options,
    keep_value: bool,
) -> Result<Execution, Stopped> {
    let refused = |error: Error| Stopped {
        error: Box::new(error),
        context: context.clone(),
        stored_roots: Vec::new(),
    };
    let instructions = program.resolve().map_err(refused)?;
    context.check().map_err(|error| refused(error.into()))?;
    let mut machine = Machine::new(context, options, true);
    let outcome = machine.run(&instructions.list);
    let Machine {
        context: left,
        stored,
        last,
        unbound: first_unbound,
        ..
    } = machine;
    let context = left.into_owned();
    match outcome {
        Ok(()) => Ok(Execution {
            context,
            value: keep_value.then_some(last),
            stored_roots: stored,
        }),
        Err(fault) => Err(Stopped {
            error: Box::new(fault_error(fault, first_unbound)),
            context,
            stored_roots: stored,
        }),
    }
}

/// Runs `program` over `context` as a statement program and answers the
/// context it leaves, with the roots it stored into.
///
/// The program is an `&`[`Instructions`]. It runs in statement mode, as the
/// reference's instruction-set text states it: from its first instruction
/// to its end, its `store`s writing a copy of `context` (the one passed in
/// is never changed), and what is left on the stack at the end is
/// discarded. A run answers an [`Execution`]: the context it left and
/// [`Execution::stored_roots`], the roots it stored into in the order of
/// its first store.
///
/// A run that stops answers a [`Stopped`]: the [`Error`] it stopped on, the
/// context as of the last statement it completed (every earlier write kept,
/// the failing write not made) and the roots it had stored into. A
/// `Stopped` converts into its `Error`, so `?` takes it where only the
/// error matters.
///
/// The [`Options`] are read as [`evaluate`] reads them, and two more:
/// [`Options::loop_budget`] bounds the back edges a run takes, as it does
/// for [`evaluate`], and [`Options::protected_roots`] names the roots a
/// `store` is refused for (reason `protected_root`). A variable the context
/// does not bind is read as [`evaluate`] reads it, and a refused undefined
/// operand is reported as the first unbound variable the run loaded; the
/// run's answer is the context, so an undefined value is never one.
///
/// Before anything runs, every value the context binds is checked against
/// the value domain ([`Context`]).
///
/// ```
/// use predicator_rs::{Context, Instructions, Key, Options, Value, execute};
///
/// // x = 1; y = x + 2
/// let program = Instructions::from_json(
///     r#"[["lit","x"],["lit",1],["store",1],["lit","y"],["load","x"],["lit",2],["add"],["store",1]]"#,
/// )?;
/// let given = Context::new();
/// let execution = execute(&program, &given, &Options::new())?;
/// assert_eq!(execution.context().get(&Key::from("y")), Some(&Value::from(3)));
/// assert_eq!(execution.stored_roots(), [Key::from("x"), Key::from("y")]);
/// assert_eq!(given, Context::new());
/// # Ok::<(), predicator_rs::Error>(())
/// ```
pub fn execute<P: Program>(
    program: P,
    context: &Context,
    options: &Options,
) -> Result<Execution, Stopped> {
    statements(program, context, options, false)
}

/// Runs `program` over `context` as [`execute`] does, and also answers the
/// value of its last expression statement: what its last `pop` discarded,
/// or undefined when it popped nothing ([`Execution::value`]).
///
/// A run that stops answers a [`Stopped`], exactly as [`execute`]'s does,
/// with no value.
///
/// ```
/// use predicator_rs::{Context, Instructions, Options, Value, execute_value};
///
/// // x = 2; x * 10
/// let program = Instructions::from_json(
///     r#"[["lit","x"],["lit",2],["store",1],["load","x"],["lit",10],["multiply"],["pop"]]"#,
/// )?;
/// let execution = execute_value(&program, &Context::new(), &Options::new())?;
/// assert_eq!(execution.value(), Some(&Value::from(20)));
/// # Ok::<(), predicator_rs::Error>(())
/// ```
pub fn execute_value<P: Program>(
    program: P,
    context: &Context,
    options: &Options,
) -> Result<Execution, Stopped> {
    statements(program, context, options, true)
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
            r#"[["overlaps"]]"#,
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
            r#"[["lit",1],["store",-1]]"#,
            r#"[["lit",1],["store","1"]]"#,
            r#"[["lit",1],["store"]]"#,
            r#"[["lit",1],["pop",1]]"#,
            r#"[["jump",0]]"#,
            r#"[["jump",-2]]"#,
            r#"[["lit",true],["pop_jump_if_falsy",0]]"#,
            r#"[["lit",1],["jump_backward",-1]]"#,
            r#"[["lit",1],["jump_backward",1.0]]"#,
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

    /// The type, reason and message of the error `text` fails with under
    /// `options`; an answer instead is reported in the same shape, so the
    /// assertion names it.
    fn failure(text: &str, options: &Options) -> (String, String, String) {
        match run_with(text, &Context::new(), options) {
            Ok(value) => ("no error".to_owned(), format!("{value:?}"), String::new()),
            Err(error) => (
                error.type_name().to_owned(),
                error.reason().to_owned(),
                error.message().to_owned(),
            ),
        }
    }

    fn function_error(message: &str) -> (String, String, String) {
        (
            "EvaluationError".to_owned(),
            message.to_owned(),
            message.to_owned(),
        )
    }

    // Sabotage: with the call arm splitting off the arguments in stack order
    // reversed, this test fails.
    #[test]
    fn call_passes_its_arguments_deepest_first_and_leaves_the_rest() {
        assert_eq!(
            run(r#"[["lit",0],["lit","hello world"],["lit",6],["lit",3],["call","substring",3]]"#)
                .ok(),
            Some(Value::from("wor"))
        );
        assert_eq!(
            run(r#"[["lit",0],["lit","x"],["call","len",1],["make_list",2]]"#).unwrap(),
            Value::from(vec![0.into(), 1.into()])
        );
        assert_eq!(
            run(r#"[["call","Math.random",0],["lit",1],["compare","LT"]]"#).unwrap(),
            Value::from(true)
        );
    }

    // Sabotage: with the call arm skipping the stack-depth check (popping
    // what there is), this test fails.
    #[test]
    fn call_refuses_too_few_values_an_unknown_name_and_a_wrong_arity() {
        let options = Options::new();
        assert_eq!(
            failure(r#"[["lit",1],["call","Math.max",2]]"#, &options),
            (
                "EvaluationError".to_owned(),
                "insufficient_arguments".to_owned(),
                "Function Math.max() expects 2 arguments, but only 1 values on stack".to_owned()
            )
        );
        assert_eq!(
            failure(r#"[["call","nope",1]]"#, &options).1,
            "insufficient_arguments"
        );
        assert_eq!(
            failure(r#"[["lit",1],["call","nope",1]]"#, &options),
            function_error("Unknown function: nope")
        );
        assert_eq!(
            failure(r#"[["lit",1],["lit",2],["call","len",2]]"#, &options),
            function_error("Function len() expects 1 arguments, got 2")
        );
        assert_eq!(
            failure(r#"[["lit","a"],["call","substring",1]]"#, &options),
            function_error("Function substring() expects 2 or 3 arguments, got 1")
        );
        assert_eq!(
            failure(r#"[["lit",1],["call","Math.random",1]]"#, &options),
            function_error("Function Math.random() expects 0 arguments, got 1")
        );
        for text in [
            r#"[["call","len"]]"#,
            r#"[["call","len",-1]]"#,
            r#"[["call","len",1.0]]"#,
            r#"[["call",1,1]]"#,
            r#"[["call","len",1,1]]"#,
        ] {
            assert_eq!(refusal(text), evaluation("unknown_instruction"), "{text}");
        }
        assert_eq!(
            refusal(r#"[["call","len",9223372036854775807]]"#),
            evaluation("insufficient_arguments")
        );
    }

    // Sabotage: with call looking up the library before the host's
    // functions, this test fails.
    #[test]
    fn a_host_function_shadows_the_library_for_its_options_only() {
        let seen = std::sync::Mutex::new(Vec::new());
        let options = Options::new()
            .function("len", |arguments| {
                seen.lock().unwrap().push(arguments.to_vec());
                Ok(Value::from(-1))
            })
            .function("twice", |arguments| match arguments {
                [Value::Integer(x)] => Ok(Value::from(x * 2)),
                _ => Err("twice takes one integer".to_owned()),
            });
        let program = r#"[["lit","abc"],["lit",true],["call","len",2]]"#;
        assert_eq!(
            run_with(program, &Context::new(), &options).ok(),
            Some(Value::from(-1))
        );
        assert_eq!(
            *seen.lock().unwrap(),
            vec![vec![Value::from("abc"), Value::from(true)]]
        );
        assert_eq!(
            run(r#"[["lit","abc"],["call","len",1]]"#).unwrap(),
            Value::from(3)
        );
        assert_eq!(
            run_with(
                r#"[["lit",21],["call","twice",1]]"#,
                &Context::new(),
                &options
            )
            .unwrap(),
            Value::from(42)
        );
        assert_eq!(
            failure(r#"[["lit","x"],["call","twice",1]]"#, &options),
            function_error("twice takes one integer")
        );
        assert_eq!(
            failure(r#"[["lit",1],["call","twice",1]]"#, &Options::new()),
            function_error("Unknown function: twice")
        );
    }

    // Sabotage: with call skipping the domain check of a host function's
    // answer, this test fails.
    #[test]
    fn a_host_functions_answer_is_checked_against_the_value_domain() {
        let options = Options::new()
            .function("infinite", |_| Ok(Value::Float(f64::INFINITY)))
            .function("deep", |_| {
                let mut deep = Value::Null;
                for _ in 0..101 {
                    deep = Value::from(vec![deep]);
                }
                Ok(deep)
            })
            .function("nothing", |_| Ok(Value::Undefined));
        assert_eq!(
            failure(r#"[["call","infinite",0]]"#, &options).1,
            "non_finite_number"
        );
        assert_eq!(
            failure(r#"[["call","deep",0]]"#, &options).1,
            "depth_limit_exceeded"
        );
        assert_eq!(
            run_with(r#"[["call","nothing",0]]"#, &Context::new(), &options).unwrap(),
            Value::Undefined
        );
    }

    // Sabotage: with draw ignoring the options' random source, this test
    // fails.
    #[test]
    fn math_random_draws_from_the_source_passed_per_call() {
        let draws = std::sync::atomic::AtomicU64::new(0);
        let options =
            Options::new().random(|| draws.fetch_add(1, std::sync::atomic::Ordering::SeqCst) << 62);
        let program = r#"[["call","Math.random",0],["call","Math.random",0],["make_list",2]]"#;
        assert_eq!(
            run_with(program, &Context::new(), &options).unwrap(),
            Value::from(vec![0.0.into(), 0.25.into()])
        );
        // The default source draws differently each time, in [0, 1).
        let drawn = run(program).unwrap();
        let Value::List(drawn) = &drawn else {
            panic!("a list")
        };
        assert_ne!(drawn[0], drawn[1]);
        for value in drawn {
            let Value::Float(number) = *value else {
                panic!("a float")
            };
            assert!((0.0..1.0).contains(&number));
        }
    }

    // Sabotage: with Date.now reading the options' clock at each call
    // rather than once per run, this test fails.
    #[test]
    fn date_now_reads_the_runs_clock_once() {
        let reads = std::sync::atomic::AtomicU32::new(0);
        let options = Options::new().clock(|| {
            let read = reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            crate::DateTime::from_unix(i64::from(read) * 86_400, 0)
        });
        let program = r#"[["call","Date.now",0],["call","Date.now",0],["make_list",2]]"#;
        let epoch = Value::from(crate::DateTime::from_unix(0, 0).unwrap());
        assert_eq!(
            run_with(program, &Context::new(), &options).unwrap(),
            Value::from(vec![epoch.clone(), epoch])
        );
        assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            failure(r#"[["call","Date.now",0]]"#, &Options::new().clock(|| None)).1,
            "clock_unavailable"
        );
    }

    // Sabotage: with call answering an unbound argument's refusal as an
    // UndefinedVariableError, this test fails.
    #[test]
    fn a_function_refusing_an_unbound_argument_is_an_evaluation_error() {
        assert_eq!(
            failure(r#"[["load","missing"],["call","len",1]]"#, &Options::new()),
            function_error("len() expects a string argument")
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

    // Sabotage: with pop_jump_if_falsy reading its condition without
    // popping it, this test fails.
    #[test]
    fn pop_jump_if_falsy_pops_always_and_jumps_on_a_falsy_value() {
        let taken = |condition: &str| {
            run(&format!(
                r#"[["lit","kept"],["lit",{condition}],["pop_jump_if_falsy",2],["lit","fell"]]"#
            ))
        };
        for falsy in ["false", "null", r#"{"$type":"undefined"}"#] {
            assert_eq!(taken(falsy).unwrap(), Value::from("kept"), "{falsy}");
        }
        assert_eq!(taken("true").unwrap(), Value::from("fell"));
        assert_eq!(
            refusal(r#"[["lit",0],["pop_jump_if_falsy",1]]"#),
            mismatched("pop_jump_if_falsy")
        );
        assert_eq!(
            refusal(r#"[["pop_jump_if_falsy",1]]"#),
            evaluation("insufficient_operands")
        );
        assert_eq!(
            run(r#"[["lit",1],["jump",2],["bogus"],["lit",2]]"#).unwrap(),
            Value::from(2)
        );
        assert_eq!(
            run(r#"[["lit",1],["jump",9223372036854775807],["bogus"]]"#).unwrap(),
            Value::from(1)
        );
    }

    // Sabotage: with jump_backward's target check made after the budget is
    // spent, this test fails.
    #[test]
    fn jump_backward_before_the_first_instruction_is_unknown_before_the_budget_is_read() {
        let none = Options::new().loop_budget(0);
        let at = |text: &str| {
            run_with(text, &Context::new(), &none)
                .err()
                .map(|error| (error.type_name().to_owned(), error.reason().to_owned()))
        };
        assert_eq!(
            at(r#"[["lit",1],["jump_backward",2]]"#),
            Some(evaluation("unknown_instruction"))
        );
        assert_eq!(
            at(r#"[["lit",1],["jump_backward",1]]"#),
            Some(evaluation("loop_budget_exceeded"))
        );
        // A back edge to the first instruction is in range.
        let once = Options::new().loop_budget(1);
        assert_eq!(
            run_with(
                r#"[["lit",1],["pop_jump_if_falsy",3],["lit",false],["jump_backward",3],["lit",0]]"#,
                &Context::new(),
                &once
            )
            .unwrap_err()
            .reason(),
            "pop_jump_if_falsy"
        );
    }

    // Sabotage: with store reading its segments top first (leaf first),
    // this test fails.
    #[test]
    fn store_pops_the_value_then_its_segments_and_writes_root_first() {
        assert_eq!(
            run(r#"[["lit","a"],["lit","b"],["lit",1],["store",2],["load","a"]]"#).unwrap(),
            Value::Map([("b", Value::from(1))].into_iter().collect())
        );
        assert_eq!(
            run(r#"[["lit","rest"],["lit","x"],["lit",1],["store",1],["pop"],["load","x"]]"#)
                .unwrap(),
            Value::from(1)
        );
        assert_eq!(
            refusal(r#"[["lit",1],["store",0]]"#),
            evaluation("not_assignable")
        );
        assert_eq!(
            refusal(r#"[["lit","x"],["store",1]]"#),
            evaluation("insufficient_operands")
        );
        assert_eq!(
            refusal(r#"[["lit","x"],["store",9223372036854775807]]"#),
            evaluation("insufficient_operands")
        );
        assert_eq!(
            refusal(r#"[["lit",true],["lit",1],["store",1]]"#),
            mismatched("store")
        );
        // An undefined segment from an unbound load is reported as the
        // unbound variable.
        assert_eq!(
            refusal(r#"[["load","missing"],["lit",1],["store",1]]"#),
            (
                "UndefinedVariableError".to_owned(),
                "unbound_variable".to_owned()
            )
        );
        assert_eq!(refusal(r#"[["pop"]]"#), evaluation("insufficient_operands"));
    }
}
