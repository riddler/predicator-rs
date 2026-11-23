//! The options one evaluation is run with, built per call.

use crate::DateTime;
use core::fmt;

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

/// The clock a run reads the current time from.
type Clock<'a> = Box<dyn Fn() -> Option<DateTime> + 'a>;

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
/// Without one, the run reads the system clock, to the microsecond.
///
/// ```
/// use predicator_rs::{Date, DateTime, OnUnbound, Options};
///
/// let strict = Options::new().on_unbound(OnUnbound::Error);
/// # let _ = strict;
/// let noon = DateTime::new(Date::new(2030, 1, 15).unwrap(), 12, 0, 0, 0);
/// let fixed = Options::new().clock(move || noon);
/// # let _ = fixed;
/// ```
#[derive(Default)]
pub struct Options<'a> {
    pub(crate) on_unbound: OnUnbound,
    pub(crate) clock: Option<Clock<'a>>,
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
    pub fn clock(mut self, clock: impl Fn() -> Option<DateTime> + 'a) -> Options<'a> {
        self.clock = Some(Box::new(clock));
        self
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
        f.debug_struct("Options")
            .field("on_unbound", &self.on_unbound)
            .field("clock", &clock)
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
            "Options { on_unbound: Undefined, clock: \"system\" }"
        );
        assert_eq!(
            format!("{:?}", Options::new().clock(|| None)),
            "Options { on_unbound: Undefined, clock: \"given\" }"
        );
    }
}
