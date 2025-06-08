//! Filter语言的 token 定义

/// token 是语言的单个单元，具有特定的类型和位置
#[derive(Debug, Clone, PartialEq)]
pub struct Token<'a> {
    pub kind: TokenKind<'a>,
    pub span: Span,
}

/// token 的类型
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind<'a> {
    // 关键字
    Filter,      // "Filter:"
    CrossFilter, // "CrossFilter:"
    And,         // "AND"
    Or,          // "OR"
    Not,         // "NOT"
    In,          // "IN"
    Is,          // "IS"
    Null,        // "NULL"

    // 字面量
    Identifier(&'a str),
    String(&'a str), // 原始字符串，包括引号
    Number(i64),

    // 特殊值关键字
    Today,
    Yesterday,
    Tomorrow,
    CurrentUser,

    // 标点符号
    LParen,    // (
    RParen,    // )
    LBracket,  // [
    RBracket,  // ]
    Semicolon, // ;
    Comma,     // ,
    Dash,      // -

    // 运算符
    Eq,    // =
    NotEq, // !=
    Gt,    // >
    Lt,    // <
    Gte,   // >=
    Lte,   // <=

    // 特殊
    Illegal, // 非法/未知字符
    Eof,     // 文件结束
}

/// 表示源文本中的位置范围
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    /// 起始字节偏移量
    pub start: usize,
    /// 结束字节偏移量
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
} 