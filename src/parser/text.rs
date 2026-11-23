//! How a refusal's message names a token, in the reference's words: the
//! word connectives in capitals whatever case the source wrote, the end of
//! input in words, a string in double quotes, and every other token that
//! carries a value as its kind and that value in single quotes. A datetime
//! is written as the reference writes it back: in UTC, with as many digits
//! of a fraction of a second as the literal wrote, up to six.

use core::fmt::Write as _;

use crate::DateTime;
use crate::json::float_text;
use crate::lexer::Kind;

/// The token as a refusal's message names it.
pub(super) fn describe(kind: &Kind) -> String {
    match kind {
        Kind::Integer(value) => format!("number '{value}'"),
        Kind::Float(value) => format!("number '{}'", float_text(*value)),
        Kind::FractionalNumber { whole, fraction } => format!("number '{whole}.{fraction}'"),
        Kind::String { value, .. } => format!("string \"{value}\""),
        Kind::Boolean(value) => format!("boolean '{value}'"),
        Kind::Date(date) => format!("date '{date}'"),
        Kind::DateTime(instant, precision) => {
            format!("datetime '{}'", datetime_text(*instant, *precision))
        }
        Kind::Identifier(name) => format!("identifier '{name}'"),
        Kind::FunctionName(name) | Kind::QualifiedFunctionName(name) => {
            format!("function '{name}'")
        }
        Kind::DurationUnit(unit) => format!("duration unit '{unit}'"),
        other => fixed(other).to_owned(),
    }
}

/// The tokens named without their value.
fn fixed(kind: &Kind) -> &'static str {
    match kind {
        Kind::Undefined => "'undefined'",
        Kind::Null => "'null'",
        Kind::Gt => "'>'",
        Kind::Lt => "'<'",
        Kind::Gte => "'>='",
        Kind::Lte => "'<='",
        Kind::Eq => "'='",
        Kind::Ne => "'!='",
        Kind::EqualEqual => "'=='",
        Kind::StrictEqual => "'==='",
        Kind::StrictNe => "'!=='",
        Kind::AndOp(_) => "'AND'",
        Kind::OrOp(_) => "'OR'",
        Kind::NotOp(_) => "'NOT'",
        Kind::InOp(_) => "'IN'",
        Kind::ContainsOp(_) => "'CONTAINS'",
        Kind::AgoOp => "'ago'",
        Kind::FromOp => "'from'",
        Kind::NowOp => "'now'",
        Kind::NextOp => "'next'",
        Kind::LastOp => "'last'",
        Kind::Lparen => "'('",
        Kind::Rparen => "')'",
        Kind::Lbracket => "'['",
        Kind::Rbracket => "']'",
        Kind::Lbrace => "'{'",
        Kind::Rbrace => "'}'",
        Kind::Colon => "':'",
        Kind::DoubleColon => "'::'",
        Kind::Comma => "','",
        Kind::Semicolon => "';'",
        Kind::Dot => "'.'",
        Kind::Plus => "'+'",
        Kind::Minus => "'-'",
        Kind::Multiply => "'*'",
        Kind::Divide => "'/'",
        Kind::Modulo => "'%'",
        Kind::AndAnd => "'&&'",
        Kind::OrOr => "'||'",
        Kind::Bang => "'!'",
        Kind::IfKw => "'if'",
        Kind::ElseKw => "'else'",
        Kind::WhileKw => "'while'",
        // Every token that carries a value is written by `describe`; the
        // end of input is the one left.
        _ => "end of input",
    }
}

/// ISO 8601 in UTC with `precision` digits of the fraction of a second.
fn datetime_text(instant: DateTime, precision: u8) -> String {
    let mut text = format!(
        "{}T{:02}:{:02}:{:02}",
        instant.date(),
        instant.hour(),
        instant.minute(),
        instant.second()
    );
    if precision > 0 {
        let digits = format!("{:06}", instant.microsecond());
        let written = digits.get(..usize::from(precision)).unwrap_or(&digits);
        let _ = write!(text, ".{written}");
    }
    text.push('Z');
    text
}
