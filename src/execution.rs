//! What a statement program answers: the run it completed, or the run that
//! stopped, each with the context it left and the roots it stored into.

use crate::{Context, Error, Key, Value};
use core::fmt;

/// A statement program that ran to its end ([`execute`](crate::execute),
/// [`execute_value`](crate::execute_value)): the context it left, the roots
/// it stored into and, from [`execute_value`](crate::execute_value), the
/// value of its last expression statement.
///
/// ```
/// use predicator_rs::{Context, Instructions, Key, Options, Value, execute};
///
/// let program = Instructions::from_json(r#"[["lit","x"],["lit",1],["store",1]]"#)?;
/// let execution = execute(&program, &Context::new(), &Options::new())?;
/// assert_eq!(execution.context().get(&Key::from("x")), Some(&Value::from(1)));
/// assert_eq!(execution.stored_roots(), [Key::from("x")]);
/// assert_eq!(execution.value(), None);
/// # Ok::<(), predicator_rs::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Execution {
    pub(crate) context: Context,
    pub(crate) value: Option<Value>,
    pub(crate) stored_roots: Vec<Key>,
}

impl Execution {
    /// The context the program left: the one it was given with every write
    /// it made. The context passed in is never changed.
    pub fn context(&self) -> &Context {
        &self.context
    }

    /// The context the program left, taken.
    pub fn into_context(self) -> Context {
        self.context
    }

    /// The value of the program's last expression statement (what its last
    /// `pop` discarded, undefined when it popped nothing), when
    /// [`execute_value`](crate::execute_value) made the run; `None` when
    /// [`execute`](crate::execute) made it.
    pub fn value(&self) -> Option<&Value> {
        self.value.as_ref()
    }

    /// The roots the program stored into, each once, in the order of its
    /// first store, whether or not the store changed the value there.
    pub fn stored_roots(&self) -> &[Key] {
        &self.stored_roots
    }
}

/// A statement program that stopped before its end: the [`Error`] it
/// stopped on, the context as of the last statement it completed (every
/// earlier write kept, the failing write not made) and the roots it had
/// stored into by then. The context passed in is never changed; a caller
/// that wants all or nothing keeps the one it has.
///
/// It converts into its [`Error`], so `?` takes a stopped run where only the
/// error matters.
///
/// ```
/// use predicator_rs::{Context, Instructions, Key, Options, Value, execute};
///
/// let program = Instructions::from_json(
///     r#"[["lit","a"],["lit",1],["store",1],["lit","a"],["lit","b"],["lit",2],["store",2]]"#,
/// )?;
/// let stopped = execute(&program, &Context::new(), &Options::new()).unwrap_err();
/// assert_eq!(stopped.error().reason(), "not_a_container");
/// assert_eq!(stopped.context().get(&Key::from("a")), Some(&Value::from(1)));
/// assert_eq!(stopped.stored_roots(), [Key::from("a")]);
/// # Ok::<(), predicator_rs::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Stopped {
    pub(crate) error: Box<Error>,
    pub(crate) context: Context,
    pub(crate) stored_roots: Vec<Key>,
}

impl Stopped {
    /// The error the run stopped on.
    pub fn error(&self) -> &Error {
        &self.error
    }

    /// The error the run stopped on, taken.
    pub fn into_error(self) -> Error {
        *self.error
    }

    /// The context as of the last statement the run completed.
    pub fn context(&self) -> &Context {
        &self.context
    }

    /// The context as of the last statement the run completed, taken.
    pub fn into_context(self) -> Context {
        self.context
    }

    /// The roots the run stored into before it stopped, each once, in the
    /// order of its first store.
    pub fn stored_roots(&self) -> &[Key] {
        &self.stored_roots
    }
}

impl From<Stopped> for Error {
    /// The error the run stopped on.
    fn from(stopped: Stopped) -> Error {
        *stopped.error
    }
}

impl fmt::Display for Stopped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.error, f)
    }
}

impl std::error::Error for Stopped {}
