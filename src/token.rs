//! The token definition for the filter language.

/// A token is a single unit of the language, with a specific kind and location.
#[derive(Debug, Clone, PartialEq)]
pub struct Token<'a> {
    pub kind: TokenKind<'a>,
    pub span: Span,
}

/// The kind of a token.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind<'a> {
    // Keywords
    Filter,      // "Filter:"
    CrossFilter, // "CrossFilter:"
    And,         // "AND"
    Or,          // "OR"
    Not,         // "NOT"
    In,          // "IN"
    Is,          // "IS"
    Null,        // "NULL"

    // Literals
    Identifier(&'a str),
    String(&'a str), // The raw string, including quotes
    Number(i64),

    // Special Value Keywords
    Today,
    Yesterday,
    Tomorrow,
    CurrentUser,

    // Punctuation
    LParen,    // (
    RParen,    // )
    LBracket,  // [
    RBracket,  // ]
    Semicolon, // ;
    Dash,      // -

    // Operators
    Eq,    // =
    NotEq, // !=
    Gt,    // >
    Lt,    // <
    Gte,   // >=
    Lte,   // <=

    // Special
    Illegal, // An illegal/unknown character
    Eof,     // End of file
}

/// Represents a span in the source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    /// The starting byte offset.
    pub start: usize,
    /// The ending byte offset.
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
} 