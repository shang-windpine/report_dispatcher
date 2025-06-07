//! Filter的词法分析器

use crate::token::{Span, Token, TokenKind};

pub struct Lexer<'a> {
    input: &'a str,
    /// 输入字符串中的当前位置（字节索引）
    position: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Lexer { input, position: 0 }
    }

    /// 返回当前位置的字符，不推进位置
    fn peek(&self) -> Option<char> {
        self.input[self.position..].chars().next()
    }

    /// 返回下一个位置的字符，不推进位置
    fn peek_next(&self) -> Option<char> {
        self.input[self.position..].chars().nth(1)
    }

    /// 推进位置一个字符并返回该字符
    fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        if let Some(c) = c {
            self.position += c.len_utf8();
        }
        c
    }

    /// 跳过空白字符
    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.bump();
            } else {
                break;
            }
        }
    }
    
    /// 读取数字字面量
    fn read_number(&mut self, start: usize) -> Token<'a> {
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.bump();
            } else {
                break;
            }
        }
        let value_str = &self.input[start..self.position];
        let value = value_str.parse::<i64>().unwrap_or(0); // 理论上不应该失败
        Token {
            kind: TokenKind::Number(value),
            span: Span::new(start, self.position),
        }
    }
    
    /// 读取双引号包围的字符串字面量
    /// 注意：开始的引号已经被调用者消费
    fn read_string(&mut self, start: usize) -> Token<'a> {
        let content_start = self.position;
        while let Some(c) = self.peek() {
            if c == '"' {
                break;
            }
            self.bump();
        }
        let content_end = self.position;
        self.bump(); // 消费结束引号
        
        let content = &self.input[content_start..content_end];
        Token {
            kind: TokenKind::String(content),
            span: Span::new(start, self.position),
        }
    }

    /// 读取标识符或关键字
    /// 标识符可以包含字母、数字、连字符和下划线
    fn read_identifier(&mut self, start: usize) -> Token<'a> {
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                self.bump();
            } else {
                break;
            }
        }
        let literal = &self.input[start..self.position];
        
        // 检查特殊关键字 "Filter:" 和 "CrossFilter:"
        if self.peek() == Some(':') {
             if literal.eq_ignore_ascii_case("Filter") {
                self.bump(); // 消费 ':'
                return Token { kind: TokenKind::Filter, span: Span::new(start, self.position) };
             }
             if literal.eq_ignore_ascii_case("CrossFilter") {
                self.bump(); // 消费 ':'
                return Token { kind: TokenKind::CrossFilter, span: Span::new(start, self.position) };
             }
        }

        let kind = match_keyword(literal);
        Token { kind, span: Span::new(start, self.position) }
    }
}

fn match_keyword(s: &str) -> TokenKind {
    match s.to_ascii_lowercase().as_str() {
        "and" => TokenKind::And,
        "or" => TokenKind::Or,
        "not" => TokenKind::Not,
        "in" => TokenKind::In,
        "is" => TokenKind::Is,
        "null" => TokenKind::Null,
        "today" => TokenKind::Today,
        "yesterday" => TokenKind::Yesterday,
        "tomorrow" => TokenKind::Tomorrow,
        "current_user" => TokenKind::CurrentUser,
        _ => TokenKind::Identifier(s),
    }
}

impl<'a> Iterator for Lexer<'a> {
    type Item = Token<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.skip_whitespace();
        let start = self.position;

        let Some(c) = self.bump() else {
            return None; // 到达输入末尾
        };

        let token = match c {
            '=' => Token { kind: TokenKind::Eq, span: Span::new(start, self.position) },
            '(' => Token { kind: TokenKind::LParen, span: Span::new(start, self.position) },
            ')' => Token { kind: TokenKind::RParen, span: Span::new(start, self.position) },
            '[' => Token { kind: TokenKind::LBracket, span: Span::new(start, self.position) },
            ']' => Token { kind: TokenKind::RBracket, span: Span::new(start, self.position) },
            '<' => {
                if self.peek() == Some('=') {
                    self.bump();
                    Token { kind: TokenKind::Lte, span: Span::new(start, self.position) }
                } else {
                    Token { kind: TokenKind::Lt, span: Span::new(start, self.position) }
                }
            }
            '>' => {
                if self.peek() == Some('=') {
                    self.bump();
                    Token { kind: TokenKind::Gte, span: Span::new(start, self.position) }
                } else {
                    Token { kind: TokenKind::Gt, span: Span::new(start, self.position) }
                }
            }
            '!' => {
                if self.peek() == Some('=') {
                    self.bump();
                    Token { kind: TokenKind::NotEq, span: Span::new(start, self.position) }
                } else {
                    Token { kind: TokenKind::Illegal, span: Span::new(start, self.position) }
                }
            }
            ';' => Token { kind: TokenKind::Semicolon, span: Span::new(start, self.position) },
            '-' => Token { kind: TokenKind::Dash, span: Span::new(start, self.position) },
            '"' => self.read_string(start),
            c if c.is_ascii_digit() => self.read_number(start),
            c if c.is_alphabetic() => self.read_identifier(start),
            _ => Token { kind: TokenKind::Illegal, span: Span::new(start, self.position) },
        };
        Some(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_filter() {
        let input = r#"Filter: status["Open"]"#;
        let mut lexer = Lexer::new(input);
        
        assert_eq!(lexer.next().unwrap().kind, TokenKind::Filter);
        assert_eq!(lexer.next().unwrap().kind, TokenKind::Identifier("status"));
        assert_eq!(lexer.next().unwrap().kind, TokenKind::LBracket);
        assert_eq!(lexer.next().unwrap().kind, TokenKind::String("Open"));
        assert_eq!(lexer.next().unwrap().kind, TokenKind::RBracket);
        assert_eq!(lexer.next(), None);
    }
    
    #[test]
    fn test_all_operators_and_punctuation() {
        let input = "!= = > < >= <= ( ) [ ] ; -";
        let kinds: Vec<_> = Lexer::new(input).map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::NotEq, TokenKind::Eq, TokenKind::Gt, TokenKind::Lt,
                TokenKind::Gte, TokenKind::Lte, TokenKind::LParen, TokenKind::RParen,
                TokenKind::LBracket, TokenKind::RBracket, TokenKind::Semicolon,
                TokenKind::Dash,
            ]
        );
    }

    #[test]
    fn test_keywords_and_identifiers() {
        let input = "AND or nOt is IN NULL today current_user My-Identifier";
        let kinds: Vec<_> = Lexer::new(input).map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::And, TokenKind::Or, TokenKind::Not, TokenKind::Is, TokenKind::In,
                TokenKind::Null, TokenKind::Today, TokenKind::CurrentUser,
                TokenKind::Identifier("My-Identifier"),
            ]
        );
    }
    
    #[test]
    fn test_numbers_and_strings() {
        let input = r#"12345 "hello world""#;
         let kinds: Vec<_> = Lexer::new(input).map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Number(12345),
                TokenKind::String("hello world"),
            ]
        );
    }
    
    #[test]
    fn test_complex_query() {
        let input = r#"Filter: title["Plan" AND (v1 OR v2)];CrossFilter: <Test-Run> dueDate[>today]"#;
        let kinds: Vec<_> = Lexer::new(input).map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Filter,
                TokenKind::Identifier("title"),
                TokenKind::LBracket,
                TokenKind::String("Plan"),
                TokenKind::And,
                TokenKind::LParen,
                TokenKind::Identifier("v1"),
                TokenKind::Or,
                TokenKind::Identifier("v2"),
                TokenKind::RParen,
                TokenKind::RBracket,
                TokenKind::Semicolon,
                TokenKind::CrossFilter,
                TokenKind::Lt,
                TokenKind::Identifier("Test-Run"),
                TokenKind::Gt,
                TokenKind::Identifier("dueDate"),
                TokenKind::LBracket,
                TokenKind::Gt,
                TokenKind::Today,
                TokenKind::RBracket
            ]
        );
    }
    
    #[test]
    fn test_cross_filter_with_brackets() {
        let input = r#"CrossFilter: <Test-Run>"#;
        let kinds: Vec<_> = Lexer::new(input).map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::CrossFilter,
                TokenKind::Lt,
                TokenKind::Identifier("Test-Run"),
                TokenKind::Gt,
            ]
        );
    }

    #[test]
    fn test_greater_than_operator() {
        let input = "field[>5]";
        let kinds: Vec<_> = Lexer::new(input).map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Identifier("field"),
                TokenKind::LBracket,
                TokenKind::Gt,
                TokenKind::Number(5),
                TokenKind::RBracket,
            ]
        );
    }
} 