//! The options one evaluation is run with, built per call.

use crate::{DateTime, Value};
use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

/// What a `load` of a variable the context does not bind does.
///
/// ```
/// use predicator_rs::OnUnbound;
///
/// assert_eq!(OnUnbound::default(), OnUnbound::Undefined);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum OnUnbound {
    /// The load pushes undefined, and the evaluation fails with an
    /// [`UndefinedVariableError`](crate::UndefinedVariableError) only when
    /// undefined is its answer, or when an instruction refuses that
    /// undefined as an operand. The default.
    #[default]
    Undefined,
    /// The load itself fails the evaluation with an
    /// [`UndefinedVariableError`](crate::UndefinedVariableError).
    Error,
}

/// The clock a run reads the current time from. It is `Send` and `Sync`,
/// so options may be shared between threads whether or not they name one.
type Clock<'a> = Box<dyn Fn() -> Option<DateTime> + Send + Sync + 'a>;

/// A function the host passes in by name: it takes the call's arguments
/// and answers a value, or refuses with a message.
type HostFunction<'a> = Box<dyn Fn(&[Value]) -> Result<Value, String> + Send + Sync + 'a>;

/// The random source `Math.random` draws from: each call answers 64 random
/// bits.
type Random<'a> = Box<dyn Fn() -> u64 + Send + Sync + 'a>;

/// The options an evaluation is run with, built per call: nothing is
/// configured globally.
///
/// The clock is the one a program that reads the current time (a
/// `relative_date` instruction) reads it from: a callable answering the
/// current instant, or `None` when it has none to give. A run reads it at
/// most once, the first time the program reads the time, and never when the
/// program does not; every later read in the same run answers the same
/// instant. A clock that answers `None` fails the run with an
/// [`EvaluationError`](crate::EvaluationError), reason `clock_unavailable`.
/// Without one, the run reads the system clock, to the microsecond. A clock
/// is `Send` and `Sync` and may borrow from its caller, so `Options` is
/// `Send` and `Sync` whether or not it names one.
///
/// A host function is passed by name ([`Options::function`]) and is called
/// by the `call` instruction with the call's arguments and nothing else. It
/// answers a value, which is checked against the value domain, or refuses
/// with a message, which fails the run with an
/// [`EvaluationError`](crate::EvaluationError) whose reason is that
/// message. A name passed this way shadows the library function of that
/// name for the runs these options are given to, and no other.
///
/// The random source is the one `Math.random` draws from
/// ([`Options::random`]). Without one, a run draws from a source seeded
/// from the standard library's per-process hash keys, which is not
/// cryptographic and must not be used where a guess at the next draw would
/// matter.
///
/// The loop budget ([`Options::loop_budget`]) is how many back edges (the
/// `jump_backward` instruction) one run may take, 10,000 unless the options
/// say otherwise; a run that has spent it fails at its next back edge with
/// an [`EvaluationError`](crate::EvaluationError), reason
/// `loop_budget_exceeded`. The protected roots
/// ([`Options::protected_roots`]) are the context roots a statement program
/// may not store into; [`evaluate`](crate::evaluate) does not read them.
///
/// ```
/// use predicator_rs::{Date, DateTime, OnUnbound, Options};
///
/// let strict = Options::new().on_unbound(OnUnbound::Error);
/// # let _ = strict;
/// let noon = DateTime::new(Date::new(2030, 1, 15).unwrap(), 12, 0, 0, 0);
/// let fixed = Options::new().clock(move || noon);
/// # let _ = fixed;
/// let hosted = Options::new()
///     .function("double", |arguments| match arguments {
///         [predicator_rs::Value::Integer(x)] => Ok(predicator_rs::Value::from(x * 2)),
///         _ => Err("double expects one integer".to_owned()),
///     })
///     .random(|| 0);
/// # let _ = hosted;
/// ```
pub struct Options<'a> {
    pub(crate) on_unbound: OnUnbound,
    pub(crate) clock: Option<Clock<'a>>,
    pub(crate) functions: BTreeMap<String, HostFunction<'a>>,
    pub(crate) random: Option<Random<'a>>,
    pub(crate) loop_budget: u64,
    pub(crate) protected_roots: BTreeSet<String>,
}

/// How many back edges a run may take when the options do not say.
pub(crate) const DEFAULT_LOOP_BUDGET: u64 = 10_000;

impl Default for Options<'_> {
    fn default() -> Self {
        Options {
            on_unbound: OnUnbound::default(),
            clock: None,
            functions: BTreeMap::new(),
            random: None,
            loop_budget: DEFAULT_LOOP_BUDGET,
            protected_roots: BTreeSet::new(),
        }
    }
}

impl<'a> Options<'a> {
    /// The default options.
    pub fn new() -> Options<'a> {
        Options::default()
    }

    /// The same options with `policy` as what a load of an unbound
    /// variable does.
    #[must_use]
    pub fn on_unbound(mut self, policy: OnUnbound) -> Options<'a> {
        self.on_unbound = policy;
        self
    }

    /// The same options with `clock` as the clock a run reads the current
    /// time from, in place of the system clock.
    #[must_use]
    pub fn clock(mut self, clock: impl Fn() -> Option<DateTime> + Send + Sync + 'a) -> Options<'a> {
        self.clock = Some(Box::new(clock));
        self
    }

    /// The same options with `function` passed in under `name`, replacing a
    /// function these options already pass under that name. The `call`
    /// instruction calls it with the call's arguments, deepest first, in
    /// place of any library function of that name. It answers the call's
    /// value or refuses with a message; a refusal fails the run with an
    /// [`EvaluationError`](crate::EvaluationError) whose reason and message
    /// are that message, and an answer outside the value domain (a float
    /// that is not finite, lists and maps nested past 100 levels) fails it
    /// with the reason the domain check gives. No arity is checked: the
    /// function sees however many arguments the call passes.
    ///
    /// ```
    /// use predicator_rs::{Context, Instructions, Options, Value, evaluate};
    ///
    /// let options = Options::new().function("len", |arguments| Ok(Value::from(arguments.len() as i64)));
    /// let program = Instructions::from_json(r#"[["lit","x"],["lit","y"],["call","len",2]]"#)?;
    /// assert_eq!(evaluate(&program, &Context::new(), &options)?, Value::from(2));
    /// # Ok::<(), predicator_rs::Error>(())
    /// ```
    #[must_use]
    pub fn function(
        mut self,
        name: impl Into<String>,
        function: impl Fn(&[Value]) -> Result<Value, String> + Send + Sync + 'a,
    ) -> Options<'a> {
        self.functions.insert(name.into(), Box::new(function));
        self
    }

    /// The same options with `random` as the source `Math.random` draws
    /// from, in place of the default. Each draw calls it once and turns the
    /// 64 bits it answers into a float in `[0, 1)`: the top 53 bits, over
    /// 2^53. A source that must change state between draws keeps that state
    /// behind a shared reference (an atomic, a mutex).
    ///
    /// ```
    /// use predicator_rs::{Context, Instructions, Options, Value, evaluate};
    ///
    /// let program = Instructions::from_json(r#"[["call","Math.random",0]]"#)?;
    /// let half = Options::new().random(|| 1 << 63);
    /// assert_eq!(evaluate(&program, &Context::new(), &half)?, Value::from(0.5));
    /// # Ok::<(), predicator_rs::Error>(())
    /// ```
    #[must_use]
    pub fn random(mut self, random: impl Fn() -> u64 + Send + Sync + 'a) -> Options<'a> {
        self.random = Some(Box::new(random));
        self
    }

    /// The same options with `budget` as how many back edges (the
    /// `jump_backward` instruction) one run may take, in place of the
    /// default 10,000. Every back edge a run takes spends one; a run that
    /// reaches a back edge with none left fails there with an
    /// [`EvaluationError`](crate::EvaluationError), reason
    /// `loop_budget_exceeded`. A budget of 0 lets a run take no back edge.
    /// A program with no back edge never spends any.
    ///
    /// ```
    /// use predicator_rs::{Context, Instructions, Options, evaluate};
    ///
    /// let forever = Instructions::from_json(r#"[["lit",true],["pop_jump_if_falsy",2],["jump_backward",2]]"#)?;
    /// let spent = evaluate(&forever, &Context::new(), &Options::new().loop_budget(3)).unwrap_err();
    /// assert_eq!(spent.reason(), "loop_budget_exceeded");
    /// # Ok::<(), predicator_rs::Error>(())
    /// ```
    #[must_use]
    pub fn loop_budget(mut self, budget: u64) -> Options<'a> {
        self.loop_budget = budget;
        self
    }

    /// The same options with each of `roots` protected: a `store` whose
    /// path's root is one of them is refused when it is reached, with an
    /// [`EvaluationError`](crate::EvaluationError), reason `protected_root`,
    /// that names the root ([`EvaluationError::root`](crate::EvaluationError::root)).
    /// A root is protected whole: there is no protecting `a.b` while
    /// leaving `a.c` writable. The check comes after the path's segments
    /// are checked, so a malformed path still fails on its segment first.
    /// The roots add to any these options already protect.
    /// [`execute`](crate::execute) and
    /// [`execute_value`](crate::execute_value) read them;
    /// [`evaluate`](crate::evaluate) does not.
    ///
    /// ```
    /// use predicator_rs::{Context, Error, Instructions, Options, execute};
    ///
    /// let program = Instructions::from_json(r#"[["lit","x"],["lit",1],["store",1],["lit","locked"],["lit",2],["store",1]]"#)?;
    /// let stopped = execute(&program, &Context::new(), &Options::new().protected_roots(["locked"])).unwrap_err();
    /// let Error::Evaluation(refusal) = stopped.error() else { unreachable!() };
    /// assert_eq!((refusal.reason(), refusal.root()), ("protected_root", Some("locked")));
    /// # Ok::<(), predicator_rs::Error>(())
    /// ```
    #[must_use]
    pub fn protected_roots<I>(mut self, roots: I) -> Options<'a>
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        self.protected_roots
            .extend(roots.into_iter().map(Into::into));
        self
    }

    /// Whether these options protect the root `name`.
    pub(crate) fn protects(&self, name: &str) -> bool {
        self.protected_roots.contains(name)
    }

    /// The host function these options pass under `name`, if any.
    pub(crate) fn host_function(&self, name: &str) -> Option<&HostFunction<'a>> {
        self.functions.get(name)
    }

    /// The random source these options name, if any.
    pub(crate) fn random_source(&self) -> Option<&Random<'a>> {
        self.random.as_ref()
    }

    /// The current time from the clock these options name, or the system
    /// clock when they name none.
    pub(crate) fn now(&self) -> Option<DateTime> {
        match &self.clock {
            Some(clock) => clock(),
            None => system_now(),
        }
    }
}

impl fmt::Debug for Options<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let clock = if self.clock.is_some() {
            "given"
        } else {
            "system"
        };
        let random = if self.random.is_some() {
            "given"
        } else {
            "default"
        };
        let functions: Vec<&str> = self.functions.keys().map(String::as_str).collect();
        f.debug_struct("Options")
            .field("on_unbound", &self.on_unbound)
            .field("clock", &clock)
            .field("functions", &functions)
            .field("random", &random)
            .field("loop_budget", &self.loop_budget)
            .field("protected_roots", &self.protected_roots)
            .finish()
    }
}

/// The system clock's current time, to the microsecond, or `None` when it
/// names an instant this crate cannot hold.
fn system_now() -> Option<DateTime> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let (sign, since) = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(since) => (1, since),
        Err(before) => (-1, before.duration()),
    };
    let micros = i128::from(sign) * i128::try_from(since.as_micros()).ok()?;
    let seconds = i64::try_from(micros.div_euclid(1_000_000)).ok()?;
    let microsecond = u32::try_from(micros.rem_euclid(1_000_000)).ok()?;
    DateTime::from_unix(seconds, microsecond)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread_safe<T: Send + Sync>(_: &T) {}

    // Sabotage: with the Send + Sync bound removed from Clock and from
    // Options::clock (or from HostFunction and Options::function, or from
    // Random and Options::random), this test fails to compile.
    #[test]
    fn options_and_a_context_may_be_shared_between_threads() {
        let reads = std::sync::atomic::AtomicU32::new(0);
        let draws = std::sync::atomic::AtomicU64::new(0);
        thread_safe(&Options::new());
        thread_safe(&Options::new().clock(|| {
            reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            None
        }));
        thread_safe(
            &Options::new()
                .function("f", |_| {
                    reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(Value::Null)
                })
                .random(|| draws.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
                .loop_budget(1)
                .protected_roots(["p"]),
        );
        thread_safe(&crate::Context::new());
    }

    // Sabotage: with now always reading the system clock, this test fails.
    #[test]
    fn a_given_clock_replaces_the_system_clock() {
        let epoch = DateTime::from_unix(0, 0);
        assert_eq!(Options::new().clock(move || epoch).now(), epoch);
        assert_eq!(Options::new().clock(|| None).now(), None);
        let system = Options::new().now().unwrap();
        // The system clock reads a time after this test was written.
        assert!(system.unix_seconds() > 1_790_000_000);
        assert_eq!(
            format!("{:?}", Options::new()),
            "Options { on_unbound: Undefined, clock: \"system\", functions: [], random: \"default\", loop_budget: 10000, protected_roots: {} }"
        );
        assert_eq!(
            format!(
                "{:?}",
                Options::new()
                    .clock(|| None)
                    .function("b", |_| Ok(Value::Null))
                    .function("a", |_| Ok(Value::Null))
                    .random(|| 0)
                    .loop_budget(5)
                    .protected_roots(["q", "p"])
            ),
            "Options { on_unbound: Undefined, clock: \"given\", functions: [\"a\", \"b\"], random: \"given\", loop_budget: 5, protected_roots: {\"p\", \"q\"} }"
        );
    }

    // Sabotage: with Options::function keeping the first function passed
    // under a name rather than the last, this test fails.
    #[test]
    fn a_function_passed_twice_under_one_name_is_the_last() {
        let options = Options::new()
            .function("f", |_| Ok(Value::from(1)))
            .function("f", |_| Ok(Value::from(2)));
        let answer = options.host_function("f").map(|function| function(&[]));
        assert_eq!(answer, Some(Ok(Value::from(2))));
        assert!(options.host_function("g").is_none());
        assert_eq!(
            Options::new()
                .random(|| 7)
                .random_source()
                .map(|random| random()),
            Some(7)
        );
        assert!(Options::new().random_source().is_none());
    }

    // Sabotage: with Default setting loop_budget to 0, this test fails.
    #[test]
    fn the_loop_budget_defaults_to_ten_thousand_and_protected_roots_add_up() {
        assert_eq!(Options::new().loop_budget, 10_000);
        assert_eq!(Options::new().loop_budget(0).loop_budget, 0);
        let options = Options::new()
            .protected_roots(["a"])
            .protected_roots(vec![String::from("b")]);
        assert!(options.protects("a") && options.protects("b"));
        assert!(!options.protects("c"));
        assert!(!Options::new().protects("a"));
    }
}
