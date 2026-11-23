//! An instruction list: the program the evaluator runs.

use crate::{Error, ParseError, Value, tagged};

/// An instruction list: a flat list of instructions, each a list whose first
/// member names the opcode and whose other members are its operands, as the
/// reference's instruction-set text defines it.
///
/// An instruction is read when the evaluator reaches it, as the reference
/// reads it: an instruction that is not a list, names no opcode the build
/// runs or carries operands of the wrong shape is refused then, with the
/// reason `unknown_instruction`, and an opcode the instruction set has
/// retired with the reason `retired_opcode`. An instruction a jump skips is
/// never read.
///
/// ```
/// use predicator_rs::{Context, Instructions, Options, Value, evaluate};
///
/// let instructions = Instructions::from_json(r#"[["lit",3],["lit",5],["compare","LT"]]"#)?;
/// assert_eq!(evaluate(&instructions, &Context::new(), &Options::new())?, Value::from(true));
/// # Ok::<(), predicator_rs::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Instructions {
    pub(crate) list: Vec<Value>,
}

impl Instructions {
    /// Reads an instruction list from JSON text in the conformance corpus's
    /// tagged-value encoding (the [`tagged`] module), which carries an
    /// undefined, a date, an instant or a duration operand as a tagged
    /// object.
    ///
    /// Every refusal is an [`Error::Parse`]: the reasons
    /// [`tagged::decode`] gives for a text that does not decode, and
    /// `not_an_instruction_list` for a text that decodes to anything but a
    /// list. The instructions themselves are read when they run.
    ///
    /// ```
    /// use predicator_rs::Instructions;
    ///
    /// assert!(Instructions::from_json(r#"[["lit",{"$type":"undefined"}]]"#).is_ok());
    /// let refused = Instructions::from_json(r#"{"lit":1}"#).unwrap_err();
    /// assert_eq!(refused.reason(), "not_an_instruction_list");
    /// ```
    pub fn from_json(text: &str) -> Result<Instructions, Error> {
        // Value implements Drop, so the list is taken out rather than moved.
        match &mut tagged::decode(text)? {
            Value::List(list) => Ok(Instructions {
                list: core::mem::take(list),
            }),
            _ => Err(ParseError::new(
                "not_an_instruction_list",
                "an instruction list is a JSON array",
            )
            .into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Sabotage: with from_json accepting a map as an empty list, this test
    // fails.
    #[test]
    fn only_a_list_is_an_instruction_list() {
        let read = Instructions::from_json(r#"[["load","x"],5,[]]"#).unwrap();
        assert_eq!(read.list.len(), 3);
        for text in [r#"{"a":1}"#, "5", "null", r#""x""#] {
            let refused = Instructions::from_json(text)
                .err()
                .map(|error| (error.type_name().to_owned(), error.reason().to_owned()));
            assert_eq!(
                refused,
                Some((
                    "ParseError".to_owned(),
                    "not_an_instruction_list".to_owned()
                )),
                "{text}"
            );
        }
        assert_eq!(
            Instructions::from_json("[").unwrap_err().reason(),
            "malformed_json"
        );
    }
}
