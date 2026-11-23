//! The compiler's scanner: source text in, a token stream out, or the one
//! refusal that stopped it. It answers the tokens the reference's lexer
//! answers, at the positions it gives them; there is no partial stream and
//! no recovery.
//!
//! # Positions
//!
//! A position counts lines and columns from 1. A column counts characters
//! (Unicode scalar values), as the reference counts them, never bytes: a
//! space and a tab advance it by one, a line feed starts the next line, and
//! a carriage return outside a literal advances nothing. A token's length
//! counts the characters of source it covers, a raw line feed inside a
//! string or date literal included. A string token also carries the
//! exclusive end of its literal, which is on a later line than its start
//! when the literal holds a raw line feed.
//!
//! One count is the reference's own and is not a column count: the stretch
//! a run of duration units consumes is counted in grapheme clusters (below).
//! The clusters are the extended grapheme clusters of the Unicode version
//! `unicode-segmentation` implements, which can be newer than the one the
//! reference's runtime implements, so a character a newer version assigns
//! may join a cluster here that it does not join there.
//!
//! # Numbers and duration units
//!
//! A number is decimal digits with at most one decimal point, which a digit
//! must follow: `1.` is the number 1 and then a dot. There is no sign and no
//! exponent. An integer outside `i64` is refused with the reason
//! `integer_out_of_range`, and a decimal too large for a finite `f64` with
//! the reason `non_finite_number`; the reference reads an integer of any
//! size and raises on such a decimal.
//!
//! A number followed by a duration unit is an integer token, or a
//! fractional number token where it has a point, then one token per unit.
//! The units (`ms`, `mo`, `y`, `d`, `h`, `m`, `s`, `w`) are read one after
//! another, the two-letter spellings before the one-letter ones, and the
//! read stops at the first character that is not a unit: `5days` is 5, the
//! unit `d` and the identifier `ays`. A number followed by anything else is
//! an integer or a float token, so `1.5x` is a float and then an identifier.
//!
//! After the FIRST unit of a run, the reference's lexer goes on over a copy
//! of the source that starts just after that unit and ends at the first
//! line feed past the copy's first character. Any further units are read
//! inside that copy, and the source from that line feed on is never read:
//! it yields no token, and the end of input is where the copy ends. The
//! column then jumps by the grapheme clusters, not the characters, between
//! the end of the number and the start of what the copy holds after the
//! units, the dropped text included. This scanner reproduces both:
//!
//! - `5d x`, a line feed and `y` scans as 5, `d`, the identifier `x` at
//!   column 6, then the end of input; `y` is never read.
//! - `5dh`, a line feed and `x` scans as 5, `d`, `h` and the end of input
//!   at column 6 of line 1.
//! - `5d`, a line feed, `x`, a line feed and `y` keeps the first line feed
//!   (the copy's first character) and so scans `x` on line 2, then ends.
//!
//! # Literals
//!
//! A string literal is quoted with `"` or `'`. A raw line feed inside it is
//! content and moves the line. A backslash escapes the next character:
//! `\n`, `\t` and `\r` stand for a line feed, a tab and a carriage return,
//! and any other character stands for itself; the escape advances the
//! column by two whatever it stands for, a backslash before a raw line feed
//! included. `\u` and `\U` are refused.
//!
//! A date literal is `#...#`. A body holding a `T` is read as a datetime and
//! any other as a date, each as the reference's runtime reads ISO 8601
//! text: a four-digit year with an optional sign, a date with hyphens, a
//! time with colons after a `T` or a space, an optional fraction after a
//! full stop or a comma (any count of digits, read to the microsecond) and
//! an offset of `Z`, `+HH:MM`, `+HHMM` or `+HH` (either sign; `-00:00` is
//! refused, and a datetime with no offset is refused). A datetime is the
//! instant it names, which must fall in the years 0 to 9999; a date may
//! have any year its four digits and sign can write.
//!
//! # Refusals
//!
//! A refusal is a [`ParseError`] at the refusal's start and a [`Span`]. An
//! unexpected character spans that character; an unterminated string or a
//! refused escape spans the string's opening quote; a malformed or
//! unterminated date literal spans the whole literal, through the closing
//! `#` or the end of the source; a number refused for its size spans the
//! number.
//!
//! # Words
//!
//! A word is `[A-Za-z_][A-Za-z0-9_]*`. The reserved words become their own
//! tokens; every other word is an identifier, or a function's name where an
//! opening parenthesis follows it past any whitespace. The words the
//! instruction set's newer operators read (`is`, `blank`, `present`,
//! `defined`, `intersects`) are ordinary identifiers here: the parser reads
//! them by position. A dotted name (`a.b.c`) is one token only where a call
//! follows it; anywhere else its first part stands alone and the dot is the
//! next token.
//!
//! The scanner holds no state between calls and never recurses, so no
//! source makes it panic or overflow the stack.

mod literals;
mod numbers;
#[cfg(test)]
mod tests;

use crate::{Date, DateTime, ParseError, Position};

/// What a token is, with what it stands for where it stands for something.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Kind {
    /// A name that is not a reserved word.
    Identifier(String),
    /// A whole number.
    Integer(i64),
    /// A decimal number not followed by a duration unit.
    Float(f64),
    /// A string literal: its content, its quote, and the exclusive end of
    /// the literal.
    String {
        value: String,
        quote: Quote,
        end: Position,
    },
    /// `true` or `false`.
    Boolean(bool),
    /// `undefined`.
    Undefined,
    /// `null`.
    Null,
    /// A date literal.
    Date(Date),
    /// A datetime literal, as the instant it names.
    DateTime(DateTime),
    /// `>`.
    Gt,
    /// `<`.
    Lt,
    /// `>=`.
    Gte,
    /// `<=`.
    Lte,
    /// `=`.
    Eq,
    /// `!=`.
    Ne,
    /// `==`.
    EqualEqual,
    /// `===`.
    StrictEqual,
    /// `!==`.
    StrictNe,
    /// `+`.
    Plus,
    /// `-`.
    Minus,
    /// `*`.
    Multiply,
    /// `/`.
    Divide,
    /// `%`.
    Modulo,
    /// `&&`.
    AndAnd,
    /// `||`.
    OrOr,
    /// `!`.
    Bang,
    /// `AND` or `and`, as written.
    AndOp(&'static str),
    /// `OR` or `or`, as written.
    OrOp(&'static str),
    /// `NOT` or `not`, as written.
    NotOp(&'static str),
    /// `(`.
    Lparen,
    /// `)`.
    Rparen,
    /// `[`.
    Lbracket,
    /// `]`.
    Rbracket,
    /// `{`.
    Lbrace,
    /// `}`.
    Rbrace,
    /// `:`.
    Colon,
    /// `::`.
    DoubleColon,
    /// `,`.
    Comma,
    /// `;`.
    Semicolon,
    /// `.`.
    Dot,
    /// `IN` or `in`, as written.
    InOp(&'static str),
    /// `CONTAINS` or `contains`, as written.
    ContainsOp(&'static str),
    /// `if`.
    IfKw,
    /// `else`.
    ElseKw,
    /// `while`.
    WhileKw,
    /// A name an opening parenthesis follows.
    FunctionName(String),
    /// A dotted name an opening parenthesis follows.
    QualifiedFunctionName(String),
    /// A duration unit's spelling.
    DurationUnit(&'static str),
    /// A decimal number a duration unit follows: its whole part and the
    /// digits of its fraction as written, so a duration is expanded from
    /// exact digits rather than from a float.
    FractionalNumber { whole: i64, fraction: String },
    /// `ago`.
    AgoOp,
    /// `from`.
    FromOp,
    /// `now`.
    NowOp,
    /// `next`.
    NextOp,
    /// `last`.
    LastOp,
    /// The end of input.
    Eof,
}

impl Kind {
    /// The token's type as the reference names it.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Kind::Identifier(_) => "identifier",
            Kind::Integer(_) => "integer",
            Kind::Float(_) => "float",
            Kind::String { .. } => "string",
            Kind::Boolean(_) => "boolean",
            Kind::Undefined => "undefined",
            Kind::Null => "null",
            Kind::Date(_) => "date",
            Kind::DateTime(_) => "datetime",
            Kind::Gt => "gt",
            Kind::Lt => "lt",
            Kind::Gte => "gte",
            Kind::Lte => "lte",
            Kind::Eq => "eq",
            Kind::Ne => "ne",
            Kind::EqualEqual => "equal_equal",
            Kind::StrictEqual => "strict_equal",
            Kind::StrictNe => "strict_ne",
            Kind::Plus => "plus",
            Kind::Minus => "minus",
            Kind::Multiply => "multiply",
            Kind::Divide => "divide",
            Kind::Modulo => "modulo",
            Kind::AndAnd => "and_and",
            Kind::OrOr => "or_or",
            Kind::Bang => "bang",
            Kind::AndOp(_) => "and_op",
            Kind::OrOp(_) => "or_op",
            Kind::NotOp(_) => "not_op",
            Kind::Lparen => "lparen",
            Kind::Rparen => "rparen",
            Kind::Lbracket => "lbracket",
            Kind::Rbracket => "rbracket",
            Kind::Lbrace => "lbrace",
            Kind::Rbrace => "rbrace",
            Kind::Colon => "colon",
            Kind::DoubleColon => "double_colon",
            Kind::Comma => "comma",
            Kind::Semicolon => "semicolon",
            Kind::Dot => "dot",
            Kind::InOp(_) => "in_op",
            Kind::ContainsOp(_) => "contains_op",
            Kind::IfKw => "if_kw",
            Kind::ElseKw => "else_kw",
            Kind::WhileKw => "while_kw",
            Kind::FunctionName(_) => "function_name",
            Kind::QualifiedFunctionName(_) => "qualified_function_name",
            Kind::DurationUnit(_) => "duration_unit",
            Kind::FractionalNumber { .. } => "fractional_number",
            Kind::AgoOp => "ago_op",
            Kind::FromOp => "from_op",
            Kind::NowOp => "now_op",
            Kind::NextOp => "next_op",
            Kind::LastOp => "last_op",
            Kind::Eof => "eof",
        }
    }
}

/// The quote a string literal is written with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Quote {
    /// `"`.
    Double,
    /// `'`.
    Single,
}

/// One token: what it is, where it starts, and how many characters of
/// source it covers.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Token {
    pub(crate) kind: Kind,
    pub(crate) line: usize,
    pub(crate) column: usize,
    pub(crate) length: usize,
}

/// The extent of a refusal: from its start, inclusive, to its end,
/// exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Span {
    pub(crate) start: Position,
    pub(crate) end: Position,
}

/// Why the source does not scan: the error, whose position is the span's
/// start, and the span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Refusal {
    pub(crate) error: Box<ParseError>,
    pub(crate) span: Span,
}

/// Scans source text: every token, the last of kind [`Kind::Eof`], or the
/// refusal that stopped the scan.
pub(crate) fn tokenize(source: &str) -> Result<Vec<Token>, Refusal> {
    let mut scan = Scan {
        source,
        at: 0,
        line: 1,
        column: 1,
        tokens: Vec::new(),
        chain_end: 0,
        kept: false,
    };
    scan.run()?;
    Ok(scan.tokens)
}

/// One scan of one source: the text still in reach (a duration unit can
/// cut it short), the byte offset into it, the running line and column, the
/// tokens so far, where the last dotted name no call followed ends, and
/// whether a duration unit has kept the source to one line yet.
struct Scan<'a> {
    source: &'a str,
    at: usize,
    line: usize,
    column: usize,
    tokens: Vec<Token>,
    chain_end: usize,
    kept: bool,
}

/// The reserved words, by their exact spelling, and the token each becomes.
fn reserved(word: &str) -> Option<Kind> {
    Some(match word {
        "true" => Kind::Boolean(true),
        "false" => Kind::Boolean(false),
        "undefined" => Kind::Undefined,
        "null" => Kind::Null,
        "AND" => Kind::AndOp("AND"),
        "and" => Kind::AndOp("and"),
        "OR" => Kind::OrOp("OR"),
        "or" => Kind::OrOp("or"),
        "NOT" => Kind::NotOp("NOT"),
        "not" => Kind::NotOp("not"),
        "IN" => Kind::InOp("IN"),
        "in" => Kind::InOp("in"),
        "CONTAINS" => Kind::ContainsOp("CONTAINS"),
        "contains" => Kind::ContainsOp("contains"),
        "if" => Kind::IfKw,
        "else" => Kind::ElseKw,
        "while" => Kind::WhileKw,
        "ago" => Kind::AgoOp,
        "from" => Kind::FromOp,
        "now" => Kind::NowOp,
        "next" => Kind::NextOp,
        "last" => Kind::LastOp,
        _ => return None,
    })
}

/// The punctuation at the start of `text`, the longest spelling first, and
/// its length.
fn punctuation(text: &str) -> Option<(Kind, usize)> {
    /// A spelling and the token it makes.
    type Spelling = (&'static str, fn() -> Kind);
    const SPELLINGS: [Spelling; 28] = [
        ("===", || Kind::StrictEqual),
        ("!==", || Kind::StrictNe),
        (">=", || Kind::Gte),
        ("<=", || Kind::Lte),
        ("==", || Kind::EqualEqual),
        ("!=", || Kind::Ne),
        ("&&", || Kind::AndAnd),
        ("||", || Kind::OrOr),
        ("::", || Kind::DoubleColon),
        (">", || Kind::Gt),
        ("<", || Kind::Lt),
        ("=", || Kind::Eq),
        ("!", || Kind::Bang),
        ("+", || Kind::Plus),
        ("-", || Kind::Minus),
        ("*", || Kind::Multiply),
        ("/", || Kind::Divide),
        ("%", || Kind::Modulo),
        ("(", || Kind::Lparen),
        (")", || Kind::Rparen),
        ("[", || Kind::Lbracket),
        ("]", || Kind::Rbracket),
        ("{", || Kind::Lbrace),
        ("}", || Kind::Rbrace),
        (":", || Kind::Colon),
        (",", || Kind::Comma),
        (";", || Kind::Semicolon),
        (".", || Kind::Dot),
    ];
    SPELLINGS
        .iter()
        .find(|(spelling, _)| text.starts_with(spelling))
        .map(|(spelling, kind)| (kind(), spelling.len()))
}

fn is_word_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// The length in bytes of the word at the start of `text`.
fn word_length(text: &str) -> usize {
    text.bytes().take_while(|byte| is_word_byte(*byte)).count()
}

impl<'a> Scan<'a> {
    fn run(&mut self) -> Result<(), Refusal> {
        while let Some(char) = self.rest().chars().next() {
            match char {
                ' ' | '\t' => {
                    self.at += 1;
                    self.column += 1;
                }
                '\n' => {
                    self.at += 1;
                    self.line += 1;
                    self.column = 1;
                }
                '\r' => self.at += 1,
                '0'..='9' => self.number()?,
                'a'..='z' | 'A'..='Z' | '_' => self.word(),
                '"' => self.string(Quote::Double)?,
                '\'' => self.string(Quote::Single)?,
                '#' => self.date()?,
                _ => self.operator(char)?,
            }
        }
        self.tokens.push(Token {
            kind: Kind::Eof,
            line: self.line,
            column: self.column,
            length: 0,
        });
        Ok(())
    }

    /// The source from the scan's offset on.
    fn rest(&self) -> &'a str {
        self.source.get(self.at..).unwrap_or_default()
    }

    /// The source from `offset` on.
    fn from(&self, offset: usize) -> &'a str {
        self.source.get(offset..).unwrap_or_default()
    }

    /// The source from `start` to the scan's offset.
    fn since(&self, start: usize) -> &'a str {
        self.source.get(start..self.at).unwrap_or_default()
    }

    /// Adds a token at the running position and moves the column past it.
    fn emit(&mut self, kind: Kind, length: usize) {
        self.tokens.push(Token {
            kind,
            line: self.line,
            column: self.column,
            length,
        });
        self.column += length;
    }

    /// A refusal at the running position, spanning to the end given.
    fn refuse(&self, reason: &str, message: String, end: Position) -> Refusal {
        let start = self.position();
        Refusal {
            error: Box::new(ParseError::new(reason, message).with_position(start)),
            span: Span { start, end },
        }
    }

    fn position(&self) -> Position {
        Position {
            line: self.line,
            column: self.column,
        }
    }

    /// Whether an opening parenthesis follows the byte offset, past any
    /// whitespace.
    fn call_follows(&self, offset: usize) -> bool {
        self.from(offset)
            .trim_start_matches([' ', '\t', '\n', '\r'])
            .starts_with('(')
    }

    /// An identifier, a reserved word, or a function's name. Every later
    /// part of a dotted name no call followed ends where the first did, so
    /// no call follows it either, and the name is not read again from each
    /// part.
    fn word(&mut self) {
        let start = self.at;
        let length = word_length(self.rest());
        self.at += length;
        let name = self.since(start);
        let mut after_dot = self.rest().bytes();
        let dotted = after_dot.next() == Some(b'.') && after_dot.next().is_some_and(is_word_start);
        if start >= self.chain_end && dotted {
            return self.dotted(name);
        }
        let kind = match reserved(name) {
            Some(kind) => kind,
            None if self.call_follows(self.at) => Kind::FunctionName(name.to_owned()),
            None => Kind::Identifier(name.to_owned()),
        };
        self.emit(kind, length);
    }

    /// The first part of a dotted name, `name`, has been read: the whole
    /// name is one token where a call follows it, and otherwise the first
    /// part stands alone.
    fn dotted(&mut self, name: &str) {
        let mut end = self.at;
        loop {
            let mut ahead = self.from(end).bytes();
            if ahead.next() != Some(b'.') || !ahead.next().is_some_and(is_word_start) {
                break;
            }
            end += 1 + word_length(self.from(end + 1));
        }
        if self.call_follows(end) {
            let length = name.len() + (end - self.at);
            let whole = self
                .source
                .get(self.at - name.len()..end)
                .unwrap_or_default();
            self.at = end;
            return self.emit(Kind::QualifiedFunctionName(whole.to_owned()), length);
        }
        self.chain_end = end;
        let kind = reserved(name).unwrap_or_else(|| Kind::Identifier(name.to_owned()));
        self.emit(kind, name.len());
    }

    /// Punctuation, or the refusal of a character that starts no token.
    fn operator(&mut self, char: char) -> Result<(), Refusal> {
        match punctuation(self.rest()) {
            Some((kind, length)) => {
                self.at += length;
                self.emit(kind, length);
                Ok(())
            }
            None => Err(self.refuse(
                "unexpected_character",
                format!("Unexpected character '{char}'"),
                Position {
                    line: self.line,
                    column: self.column + 1,
                },
            )),
        }
    }
}
