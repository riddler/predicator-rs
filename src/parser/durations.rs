//! Duration literals and the words that make one a relative date.
//!
//! The lexer writes `3d8h` as a number and a unit per component. A literal
//! written in whole numbers keeps its components as written, a unit written
//! twice included. A component written with a point is expanded as the
//! reference expands it: the whole part keeps its unit when it is not zero,
//! and the fraction's worth, which must be a whole number of milliseconds (a
//! month 30 days, a year 365), is spent largest first through days, hours,
//! minutes, seconds and milliseconds, the pair `(0, unit)` standing for a
//! component worth nothing. An inexact fraction is refused over its
//! component; a unit named twice after an expansion is refused over the
//! whole literal, the alphabetically first such unit named.

use super::tree::{Direction, Expression, Node};
use super::{Descent, between, end_of, refuse, start_of};
use crate::lexer::{Kind, Refusal, Span, Token};

/// The units a fraction's worth is spent into, largest first, and their
/// worth in milliseconds.
const LADDER: [(&str, i128); 5] = [
    ("d", 86_400_000),
    ("h", 3_600_000),
    ("m", 60_000),
    ("s", 1_000),
    ("ms", 1),
];

/// One component as written: its whole part, the digits of its fraction
/// (none for a whole number), its unit, and what it covers.
struct Component<'a> {
    whole: i64,
    fraction: Option<&'a str>,
    unit: &'static str,
    span: Span,
}

impl<'a> Descent<'a> {
    /// The duration (or relative date) the number just consumed starts,
    /// when a unit follows it; `None` when none does, and nothing more is
    /// consumed.
    pub(super) fn duration(&mut self, number: &'a Token) -> Result<Option<Node>, Refusal> {
        let Some(first) = self.component(number) else {
            return Ok(None);
        };
        let mut components = vec![first];
        while let Some(component) = self.next_component() {
            components.push(component);
        }
        let start = start_of(number);
        let span = between(start, end_of(self.previous()));
        let node = Node {
            kind: Expression::Duration(whole_units(&components, span)?),
            position: start,
            span,
        };
        self.direction(node).map(Some)
    }

    /// A number token, already consumed, and the unit at the cursor, which
    /// is consumed when it is one.
    fn component(&mut self, number: &'a Token) -> Option<Component<'a>> {
        let unit_token = self.peek();
        let Kind::DurationUnit(unit) = unit_token.kind else {
            return None;
        };
        let (whole, fraction) = match &number.kind {
            Kind::Integer(whole) => (*whole, None),
            Kind::FractionalNumber { whole, fraction } => (*whole, Some(fraction.as_str())),
            _ => return None,
        };
        self.advance();
        Some(Component {
            whole,
            fraction,
            unit,
            span: between(start_of(number), end_of(unit_token)),
        })
    }

    /// The next component, when a number and a unit follow.
    fn next_component(&mut self) -> Option<Component<'a>> {
        let number = self.peek();
        if !matches!(
            number.kind,
            Kind::Integer(_) | Kind::FractionalNumber { .. }
        ) || !matches!(self.ahead().kind, Kind::DurationUnit(_))
        {
            return None;
        }
        self.advance();
        self.component(number)
    }

    /// `ago`, or `from now`, after a duration; the relative date is at the
    /// word.
    fn direction(&mut self, duration: Node) -> Result<Node, Refusal> {
        let word = self.peek();
        let direction = match word.kind {
            Kind::AgoOp => Direction::Ago,
            Kind::FromOp => Direction::Future,
            _ => return Ok(duration),
        };
        self.advance();
        let last = if direction == Direction::Ago {
            word
        } else {
            self.expect(
                &Kind::NowOp,
                "expected_now",
                "Expected 'now' after 'from' but found",
            )?
        };
        let span = between(duration.span.start, end_of(last));
        Ok(Node {
            kind: Expression::RelativeDate {
                duration: Box::new(duration),
                direction,
            },
            position: start_of(word),
            span,
        })
    }
}

/// The literal's units: as written when no component has a point, and
/// expanded otherwise; `span` is the whole literal's.
fn whole_units(
    components: &[Component<'_>],
    span: Span,
) -> Result<Vec<(i64, &'static str)>, Refusal> {
    if components
        .iter()
        .all(|component| component.fraction.is_none())
    {
        return Ok(components
            .iter()
            .map(|component| (component.whole, component.unit))
            .collect());
    }
    let mut units = Vec::new();
    for component in components {
        units.extend(expanded(component)?);
    }
    let twice = units
        .iter()
        .enumerate()
        .filter(|(at, (_, unit))| units.iter().skip(at + 1).any(|(_, other)| other == unit))
        .map(|(_, (_, unit))| *unit)
        .min();
    if let Some(unit) = twice {
        return Err(refuse(
            "duration_unit_twice",
            format!("Duration literal names the '{unit}' unit twice after expanding a fraction"),
            span,
        ));
    }
    Ok(units)
}

/// One component in whole units.
fn expanded(component: &Component<'_>) -> Result<Vec<(i64, &'static str)>, Refusal> {
    let Some(fraction) = component.fraction else {
        return Ok(vec![(component.whole, component.unit)]);
    };
    let inexact = || {
        refuse(
            "duration_fraction",
            format!(
                "Duration fraction is not a whole number of milliseconds: {}.{fraction}{}",
                component.whole, component.unit
            ),
            component.span,
        )
    };
    // Trailing zeros change nothing. A fraction ending in another digit is
    // not divisible by both 2 and 5, so its denominator is cleared only by
    // a unit's powers of 2 (2^11 at most) or of 5 (5^6 at most): past
    // eleven digits it is never exact.
    let digits = fraction.trim_end_matches('0');
    if digits.len() > 11 {
        return Err(inexact());
    }
    let numerator = digits
        .bytes()
        .fold(0_i128, |sum, digit| sum * 10 + i128::from(digit - b'0'));
    let denominator = digits.bytes().fold(1_i128, |power, _| power * 10);
    let total = numerator * milliseconds(component.unit);
    if total % denominator != 0 {
        return Err(inexact());
    }
    let mut left = total / denominator;
    let mut pairs = Vec::new();
    if component.whole > 0 {
        pairs.push((component.whole, component.unit));
    }
    for (unit, size) in LADDER {
        let count = left / size;
        left %= size;
        if count > 0 {
            // The fraction is under one unit, so no count passes i64.
            pairs.push((i64::try_from(count).unwrap_or(i64::MAX), unit));
        }
    }
    if pairs.is_empty() {
        pairs.push((0, component.unit));
    }
    Ok(pairs)
}

/// A unit's worth in milliseconds.
fn milliseconds(unit: &str) -> i128 {
    match unit {
        "y" => 31_536_000_000,
        "mo" => 2_592_000_000,
        "w" => 604_800_000,
        "d" => 86_400_000,
        "h" => 3_600_000,
        "m" => 60_000,
        "s" => 1_000,
        _ => 1,
    }
}
