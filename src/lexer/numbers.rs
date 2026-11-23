//! Numbers and the duration units written after them (the module
//! documentation of the lexer states the rules, the line-feed cut after the
//! first unit included).

use super::{Kind, Refusal, Scan, Token};
use crate::Position;
use unicode_segmentation::UnicodeSegmentation;

/// The duration unit at the start of `text`, the two-letter spellings
/// first.
fn unit_at(text: &str) -> Option<&'static str> {
    const UNITS: [&str; 8] = ["ms", "mo", "y", "d", "h", "m", "s", "w"];
    UNITS.into_iter().find(|unit| text.starts_with(unit))
}

/// The count of grapheme clusters in `text`.
fn clusters(text: &str) -> usize {
    text.graphemes(true).count()
}

impl<'a> Scan<'a> {
    /// A number, and the duration units after it.
    pub(super) fn number(&mut self) -> Result<(), Refusal> {
        let start = self.at;
        let whole = self.rest().bytes().take_while(u8::is_ascii_digit).count();
        self.at += whole;
        let mut ahead = self.rest().bytes();
        let fraction =
            if ahead.next() == Some(b'.') && ahead.next().is_some_and(|b| b.is_ascii_digit()) {
                1 + self
                    .from(self.at + 1)
                    .bytes()
                    .take_while(u8::is_ascii_digit)
                    .count()
            } else {
                0
            };
        self.at += fraction;
        let text = self.since(start);
        let length = text.len();
        let digits = text.get(..whole).unwrap_or_default();
        let unit_follows = unit_at(self.rest()).is_some();
        if fraction > 0 {
            match text.parse::<f64>() {
                Ok(value) if value.is_finite() => {
                    if !unit_follows {
                        self.emit(Kind::Float(value), length);
                        return Ok(());
                    }
                }
                _ => {
                    return Err(self.refuse_number(
                        "non_finite_number",
                        format!("Number literal {text} is too large for a finite float"),
                        length,
                    ));
                }
            }
        }
        let Ok(integer) = digits.parse::<i64>() else {
            return Err(self.refuse_number(
                "integer_out_of_range",
                format!("Integer literal {digits} is outside the 64-bit integer range"),
                length,
            ));
        };
        let leading = if fraction > 0 {
            Kind::FractionalNumber {
                whole: integer,
                fraction: text.get(whole + 1..).unwrap_or_default().to_owned(),
            }
        } else {
            Kind::Integer(integer)
        };
        self.emit(leading, length);
        if unit_follows {
            self.units();
        }
        Ok(())
    }

    /// A refusal of the number of `length` characters at the running
    /// position, spanning it.
    fn refuse_number(&self, reason: &str, message: String, length: usize) -> Refusal {
        let end = Position {
            line: self.line,
            column: self.column + length,
        };
        self.refuse(reason, message, end)
    }

    /// The run of duration units at the scan's offset, at least one. After
    /// the first, the source in reach is cut at the first line feed past
    /// the character after that unit, and the column jumps by the grapheme
    /// clusters the units and the dropped text span, as the reference
    /// counts them.
    fn units(&mut self) {
        let start = self.at;
        let mut column = self.column;
        let mut dropped = "";
        while let Some(unit) = unit_at(self.rest()) {
            self.tokens.push(Token {
                kind: Kind::DurationUnit(unit),
                line: self.line,
                column,
                length: unit.len(),
            });
            let first = self.at == start;
            self.at += unit.len();
            column += unit.len();
            if first {
                dropped = self.keep_line();
            }
        }
        self.column += self.consumed(self.since(start), dropped);
    }

    /// Cuts the source in reach at the first line feed past the character
    /// at the scan's offset, and answers the text cut off ("" when none is).
    /// Once a unit has kept the source to one line, no line feed is left
    /// past the character after any later unit, so the cut is taken once.
    fn keep_line(&mut self) -> &'a str {
        if self.kept {
            return "";
        }
        self.kept = true;
        let rest = self.rest();
        let first = rest.chars().next().map_or(0, char::len_utf8);
        let Some(feed) = rest.get(first..).and_then(|after| after.find('\n')) else {
            return "";
        };
        let cut = self.at + first + feed;
        let dropped = self.from(cut);
        self.source = self.source.get(..cut).unwrap_or_default();
        dropped
    }

    /// The grapheme clusters of `units` plus what follows them up to the
    /// text still in reach, plus the `dropped` text: the reference counts
    /// the whole source after the number and takes away what is left after
    /// the units. The units are ASCII letters, so only the first cluster of
    /// what is left can join the last unit; a carriage return ending what is
    /// left joins the dropped line feed.
    fn consumed(&self, units: &str, dropped: &str) -> usize {
        let left = self.rest();
        let Some(first) = left.graphemes(true).next() else {
            return clusters(units) + clusters(dropped);
        };
        let joined = clusters(&format!("{units}{first}")) - 1;
        if dropped.is_empty() {
            return joined;
        }
        joined + clusters(dropped) - usize::from(self.source.ends_with('\r'))
    }
}
