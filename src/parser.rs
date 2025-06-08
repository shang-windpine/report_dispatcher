//! Filter的语法分析器
//!
//! ## 解析流程图
//!
//! ```text
//! parse()
//!   ├─ 检查token类型
//!   │   ├─ "Filter:" → parse_field_filters_until_semicolon_or_crossfilter()
//!   │   │                └─ parse_field_filter()
//!   │   │                     ├─ 解析字段名 (Identifier)
//!   │   │                     ├─ 期望 '['
//!   │   │                     ├─ parse_condition()
//!   │   │                     └─ 期望 ']'
//!   │   │
//!   │   └─ "CrossFilter:" → parse_cross_filter()
//!   │                        ├─ 期望 '<'
//!   │                        ├─ 解析实体名 Source-Target
//!   │                        ├─ 期望 '>'
//!   │                        └─ parse_field_filters_until_semicolon_or_crossfilter()
//!   │
//!   └─ parse_condition() (递归下降解析)
//!        └─ parse_or_expression()
//!             ├─ parse_and_expression()
//!             │    ├─ parse_not_expression()
//!             │    │    └─ parse_primary_expression()
//!             │    │         ├─ "(" → 分组表达式 (递归调用parse_condition)
//!             │    │         ├─ "IS" → IS NULL / IS NOT NULL
//!             │    │         ├─ "IN" → IN (值列表)
//!             │    │         ├─ 比较运算符 → 比较操作 + 字面值
//!             │    │         └─ 其他 → 默认相等比较 + 字面值
//!             │    │
//!             │    └─ 遇到AND时，继续解析右侧NOT表达式
//!             │
//!             └─ 遇到OR时，继续解析右侧AND表达式
//! ```
//!
//! ## 语法优先级（从高到低）
//!
//! 1. **括号分组** `(expression)`
//! 2. **NOT操作** `NOT expression`
//! 3. **比较操作** `field[>value]`, `field[=value]`, `IS NULL`, `IN (...)`
//! 4. **AND操作** `expr1 AND expr2`
//! 5. **OR操作** `expr1 OR expr2`
//!
//! ## 支持的语法结构
//!
//! ### 基础过滤器
//! ```text
//! Filter: field_name[condition]
//! ```
//!
//! ### 交叉过滤器
//! ```text
//! CrossFilter: <Source-Target> field_name[condition]
//! ```
//!
//! ### 条件表达式
//! - **比较操作**: `=`, `!=`, `>`, `<`, `>=`, `<=`
//! - **空值检查**: `IS NULL`, `IS NOT NULL`
//! - **列表包含**: `IN (value1, value2, ...)`
//! - **逻辑操作**: `AND`, `OR`, `NOT`
//! - **分组**: `(expression)`
//!
//! ### 字面值类型
//! - **字符串**: `"quoted string"` 或 `unquoted_identifier`
//! - **数字**: `123`, `-456`
//! - **日期关键字**: `today`, `yesterday`, `tomorrow`
//! - **用户关键字**: `current_user`
//! - **空值**: `null`
//!
//! ## 解析示例
//!
//! ```text
//! // 简单过滤
//! Filter: status["Open"]
//!
//! // 复杂条件
//! Filter: priority[>2 AND <=5]; status["Open" OR "Pending"]
//!
//! // 交叉过滤
//! CrossFilter: <Test-Run> result["PASS"]
//!
//! // 混合查询
//! Filter: assignee[current_user]; CrossFilter: <Bug-Fix> priority[>=3]
//! ```

use crate::ast::{Query, FieldFilter, CrossFilter, Condition, Identifier, CompOp, Literal};
use crate::token::{Token, TokenKind, Span};

pub struct Parser<'a> {
    tokens: &'a [Token<'a>],
    position: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub span: Option<Span>,
}

impl ParseError {
    fn new(message: String, span: Option<Span>) -> Self {
        Self { message, span }
    }
    
    fn at_position(message: String, span: Span) -> Self {
        Self { message, span: Some(span) }
    }
}

impl<'a> Parser<'a> {
    pub fn new(tokens: &'a [Token<'a>]) -> Self {
        Self {
            tokens,
            position: 0,
        }
    }

    /// 返回当前 token，不推进位置
    fn peek(&self) -> Option<&Token<'a>> {
        self.tokens.get(self.position)
    }

    /// 返回当前 token 并推进位置
    fn advance(&mut self) -> Option<&Token<'a>> {
        if self.position < self.tokens.len() {
            let token = &self.tokens[self.position];
            self.position += 1;
            Some(token)
        } else {
            None
        }
    }

    /// 期望特定类型的 token 并推进，否则返回错误
    fn expect(&mut self, expected: TokenKind) -> Result<&Token<'a>, ParseError> {
        if let Some(token) = self.peek() {
            if std::mem::discriminant(&token.kind) == std::mem::discriminant(&expected) {
                Ok(self.advance().unwrap())
            } else {
                Err(ParseError::at_position(
                    format!("Expected {:?}, found {:?}", expected, token.kind),
                    token.span,
                ))
            }
        } else {
            Err(ParseError::new(
                format!("Expected {:?}, but reached end of input", expected),
                None,
            ))
        }
    }

    /// 检查当前 token 是否匹配给定类型
    fn match_token(&self, kind: &TokenKind) -> bool {
        if let Some(token) = self.peek() {
            std::mem::discriminant(&token.kind) == std::mem::discriminant(kind)
        } else {
            false
        }
    }

    /// 检查当前 token 是否为比较运算符
    fn is_comparison_operator(&self) -> bool {
        if let Some(token) = self.peek() {
            matches!(token.kind, 
                TokenKind::Eq | TokenKind::NotEq | TokenKind::Gt | 
                TokenKind::Lt | TokenKind::Gte | TokenKind::Lte)
        } else {
            false
        }
    }

    pub fn parse(&mut self) -> Result<Query, ParseError> {
        let mut base_filters = Vec::new();
        let mut cross_filters = Vec::new();

        while let Some(token) = self.peek() {
            match &token.kind {
                TokenKind::Filter => {
                    self.advance(); // 消费 "Filter:"
                    let filters = self.parse_field_filters_until_semicolon_or_crossfilter()?;
                    base_filters.extend(filters);
                }
                TokenKind::CrossFilter => {
                    self.advance(); // 消费 "CrossFilter:"
                    let cross_filter = self.parse_cross_filter()?;
                    cross_filters.push(cross_filter);
                }
                _ => {
                    return Err(ParseError::at_position(
                        format!("Unexpected token: {:?}", token.kind),
                        token.span,
                    ));
                }
            }
        }

        Ok(Query {
            base_filters,
            cross_filters,
        })
    }

    /// 解析字段Filter，直到遇到分号、CrossFilter 或输入结束
    fn parse_field_filters_until_semicolon_or_crossfilter(&mut self) -> Result<Vec<FieldFilter>, ParseError> {
        let mut filters = Vec::new();

        loop {
            // 解析一个字段Filter
            let filter = self.parse_field_filter()?;
            filters.push(filter);

            // 检查是否需要继续
            if let Some(token) = self.peek() {
                match &token.kind {
                    TokenKind::Semicolon => {
                        self.advance(); // 消费分号
                        // 检查下一个 token 是否为 CrossFilter 或输入结束
                        if let Some(next_token) = self.peek() {
                            if matches!(next_token.kind, TokenKind::CrossFilter) {
                                break; // 基础Filter结束
                            }
                            // 否则继续解析更多字段Filter
                        } else {
                            break; // 输入结束
                        }
                    }
                    TokenKind::CrossFilter => {
                        break; // 基础Filter结束
                    }
                    _ => {
                        return Err(ParseError::at_position(
                            format!("Expected semicolon or CrossFilter, found {:?}", token.kind),
                            token.span,
                        ));
                    }
                }
            } else {
                break; // 输入结束
            }
        }

        Ok(filters)
    }

    fn parse_cross_filter(&mut self) -> Result<CrossFilter, ParseError> {
        // 期望 <Source-Target>
        self.expect(TokenKind::Lt)?;
        
        let entity_token = self.expect(TokenKind::Identifier(""))?;
        let entity_name = if let TokenKind::Identifier(name) = &entity_token.kind {
            name
        } else {
            return Err(ParseError::at_position(
                "Expected entity identifier".to_string(),
                entity_token.span,
            ));
        };

        // 按连字符分割实体名称，获取源和目标
        let parts: Vec<&str> = entity_name.split('-').collect();
        if parts.len() != 2 {
            return Err(ParseError::at_position(
                format!("Entity identifier '{}' must be in format 'Source-Target'", entity_name),
                entity_token.span,
            ));
        }

        let source_entity = Identifier(parts[0].to_string());
        let target_entity = Identifier(parts[1].to_string());

        self.expect(TokenKind::Gt)?;

        // 解析关联Filter的字段Filter
        let filters = self.parse_field_filters_until_semicolon_or_crossfilter()?;

        Ok(CrossFilter {
            source_entity,
            target_entity,
            filters,
        })
    }

    fn parse_field_filter(&mut self) -> Result<FieldFilter, ParseError> {
        let field_token = self.expect(TokenKind::Identifier(""))?;
        let field = if let TokenKind::Identifier(name) = &field_token.kind {
            Identifier(name.to_string())
        } else {
            return Err(ParseError::at_position(
                "Expected field identifier".to_string(),
                field_token.span,
            ));
        };

        self.expect(TokenKind::LBracket)?;
        let condition = self.parse_condition()?;
        self.expect(TokenKind::RBracket)?;

        Ok(FieldFilter { field, condition })
    }

    /// 解析条件表达式的入口点
    /// 
    /// 条件解析采用递归下降方式，按照优先级从低到高依次处理：
    /// OR → AND → NOT → PRIMARY
    fn parse_condition(&mut self) -> Result<Condition, ParseError> {
        self.parse_or_expression()
    }

    /// 解析OR表达式 (最低优先级)
    /// 
    /// 语法: `and_expr (OR and_expr)*`
    /// 示例: `"Open" OR "Pending" OR "Closed"`
    fn parse_or_expression(&mut self) -> Result<Condition, ParseError> {
        let mut left = self.parse_and_expression()?;

        while self.match_token(&TokenKind::Or) {
            self.advance(); // 消费 OR
            let right = self.parse_and_expression()?;
            left = Condition::Or(Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    /// 解析AND表达式 (中等优先级)
    /// 
    /// 语法: `not_expr (AND not_expr)*`
    /// 示例: `>5 AND <=10`
    fn parse_and_expression(&mut self) -> Result<Condition, ParseError> {
        let mut left = self.parse_not_expression()?;

        while self.match_token(&TokenKind::And) {
            self.advance(); // 消费 AND
            let right = self.parse_not_expression()?;
            left = Condition::And(Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    /// 解析NOT表达式 (较高优先级)
    /// 
    /// 语法: `NOT* primary_expr`
    /// 示例: `NOT "Closed"`, `NOT NOT "Open"`
    fn parse_not_expression(&mut self) -> Result<Condition, ParseError> {
        if self.match_token(&TokenKind::Not) {
            self.advance(); // 消费 NOT
            let expr = self.parse_not_expression()?; // 允许 NOT 链式调用
            Ok(Condition::Not(Box::new(expr)))
        } else {
            self.parse_primary_expression()
        }
    }

    /// 解析基础表达式 (最高优先级)
    /// 
    /// 支持的表达式类型:
    /// - `(condition)` - 分组表达式
    /// - `IS [NOT] NULL` - 空值检查
    /// - `IN (value1, value2, ...)` - 列表包含
    /// - `op value` - 带运算符的比较 (如 `>5`, `="test"`)
    /// - `value` - 默认相等比较 (如 `"Open"` 等价于 `="Open"`)
    fn parse_primary_expression(&mut self) -> Result<Condition, ParseError> {
        if let Some(token) = self.peek() {
            match &token.kind {
                TokenKind::LParen => {
                    self.advance(); // 消费 (
                    let expr = self.parse_condition()?;
                    self.expect(TokenKind::RParen)?;
                    Ok(Condition::Grouped(Box::new(expr)))
                }
                TokenKind::Is => {
                    self.advance(); // 消费 IS
                    if self.match_token(&TokenKind::Not) {
                        self.advance(); // 消费 NOT
                        self.expect(TokenKind::Null)?;
                        Ok(Condition::IsNotNull)
                    } else {
                        self.expect(TokenKind::Null)?;
                        Ok(Condition::IsNull)
                    }
                }
                TokenKind::In => {
                    self.advance(); // 消费 IN
                    self.expect(TokenKind::LParen)?;
                    let mut values = Vec::new();

                    // 解析逗号分隔的值列表
                    if !self.match_token(&TokenKind::RParen) {
                        loop {
                            values.push(self.parse_literal()?);
                            if self.match_token(&TokenKind::RParen) {
                                break;
                            }
                            self.expect(TokenKind::Comma)?;
                        }
                    }

                    self.expect(TokenKind::RParen)?;
                    Ok(Condition::In(values))
                }
                _ => {
                    // 检查是否以比较运算符开始
                    if self.is_comparison_operator() {
                        let op = self.parse_comparison_operator()?;
                        let value = self.parse_literal()?;
                        Ok(Condition::Comparison { op, value })
                    } else {
                        // 如果没有指定运算符，默认为相等比较
                        let value = self.parse_literal()?;
                        Ok(Condition::Comparison { op: CompOp::Eq, value })
                    }
                }
            }
        } else {
            Err(ParseError::new("Unexpected end of input".to_string(), None))
        }
    }

    fn parse_comparison_operator(&mut self) -> Result<CompOp, ParseError> {
        if let Some(token) = self.advance() {
            match &token.kind {
                TokenKind::Eq => Ok(CompOp::Eq),
                TokenKind::NotEq => Ok(CompOp::NotEq),
                TokenKind::Gt => Ok(CompOp::Gt),
                TokenKind::Lt => Ok(CompOp::Lt),
                TokenKind::Gte => Ok(CompOp::Gte),
                TokenKind::Lte => Ok(CompOp::Lte),
                _ => Err(ParseError::at_position(
                    format!("Expected comparison operator, found {:?}", token.kind),
                    token.span,
                )),
            }
        } else {
            Err(ParseError::new("Expected comparison operator".to_string(), None))
        }
    }

    fn parse_literal(&mut self) -> Result<Literal, ParseError> {
        if let Some(token) = self.advance() {
            match &token.kind {
                TokenKind::String(s) => Ok(Literal::String(s.to_string())),
                TokenKind::Number(n) => Ok(Literal::Number(*n)),
                TokenKind::Today => Ok(Literal::Date("today".to_string())),
                TokenKind::Yesterday => Ok(Literal::Date("yesterday".to_string())),
                TokenKind::Tomorrow => Ok(Literal::Date("tomorrow".to_string())),
                TokenKind::CurrentUser => Ok(Literal::CurrentUser),
                TokenKind::Identifier(s) => {
                    // 不带引号的字符串
                    Ok(Literal::String(s.to_string()))
                }
                _ => Err(ParseError::at_position(
                    format!("Expected literal value, found {:?}", token.kind),
                    token.span,
                )),
            }
        } else {
            Err(ParseError::new("Expected literal value".to_string(), None))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse_string(input: &str) -> Result<Query, ParseError> {
        let tokens: Vec<_> = Lexer::new(input).collect();
        Parser::new(&tokens).parse()
    }

    #[test]
    fn test_simple_filter() {
        let input = r#"Filter: status["Open"]"#;
        let result = parse_string(input).unwrap();
        
        assert_eq!(result.base_filters.len(), 1);
        assert_eq!(result.cross_filters.len(), 0);
        
        let filter = &result.base_filters[0];
        assert_eq!(filter.field.0, "status");
        
        if let Condition::Comparison { op, value } = &filter.condition {
            assert_eq!(*op, CompOp::Eq);
            assert_eq!(*value, Literal::String("Open".to_string()));
        } else {
            panic!("Expected comparison condition");
        }
    }

    #[test]
    fn test_multiple_filters() {
        let input = r#"Filter: status["Open"]; priority[>2]"#;
        let result = parse_string(input).unwrap();
        
        assert_eq!(result.base_filters.len(), 2);
        assert_eq!(result.cross_filters.len(), 0);
        
        let filter1 = &result.base_filters[0];
        assert_eq!(filter1.field.0, "status");
        
        let filter2 = &result.base_filters[1];
        assert_eq!(filter2.field.0, "priority");
        
        if let Condition::Comparison { op, value } = &filter2.condition {
            assert_eq!(*op, CompOp::Gt);
            assert_eq!(*value, Literal::Number(2));
        } else {
            panic!("Expected comparison condition");
        }
    }

    #[test]
    fn test_cross_filter() {
        let input = r#"CrossFilter: <Test-Run> status["PASS"]"#;
        let result = parse_string(input).unwrap();
        
        assert_eq!(result.base_filters.len(), 0);
        assert_eq!(result.cross_filters.len(), 1);
        
        let cross_filter = &result.cross_filters[0];
        assert_eq!(cross_filter.source_entity.0, "Test");
        assert_eq!(cross_filter.target_entity.0, "Run");
        assert_eq!(cross_filter.filters.len(), 1);
        
        let filter = &cross_filter.filters[0];
        assert_eq!(filter.field.0, "status");
    }

    #[test]
    fn test_logical_operations() {
        let input = r#"Filter: status["Open" OR "Pending"]"#;
        let result = parse_string(input).unwrap();
        
        let filter = &result.base_filters[0];
        if let Condition::Or(left, right) = &filter.condition {
            if let (Condition::Comparison { value: left_val, .. }, 
                    Condition::Comparison { value: right_val, .. }) = (left.as_ref(), right.as_ref()) {
                assert_eq!(*left_val, Literal::String("Open".to_string()));
                assert_eq!(*right_val, Literal::String("Pending".to_string()));
            } else {
                panic!("Expected comparison conditions in OR");
            }
        } else {
            panic!("Expected OR condition");
        }
    }

    #[test]
    fn test_not_condition() {
        let input = r#"Filter: status[NOT "Closed"]"#;
        let result = parse_string(input).unwrap();
        
        let filter = &result.base_filters[0];
        if let Condition::Not(inner) = &filter.condition {
            if let Condition::Comparison { op, value } = inner.as_ref() {
                assert_eq!(*op, CompOp::Eq);
                assert_eq!(*value, Literal::String("Closed".to_string()));
            } else {
                panic!("Expected comparison condition inside NOT");
            }
        } else {
            panic!("Expected NOT condition");
        }
    }

    #[test]
    fn test_grouped_condition() {
        let input = r#"Filter: status[("Open" OR "Pending")]"#;
        let result = parse_string(input).unwrap();
        
        let filter = &result.base_filters[0];
        if let Condition::Grouped(inner) = &filter.condition {
            if let Condition::Or(_, _) = inner.as_ref() {
                // Success - we have a grouped OR condition
            } else {
                panic!("Expected OR condition inside group");
            }
        } else {
            panic!("Expected grouped condition");
        }
    }

    #[test]
    fn test_date_keywords() {
        let input = r#"Filter: created[>today]; modified[<=yesterday]"#;
        let result = parse_string(input).unwrap();
        
        assert_eq!(result.base_filters.len(), 2);
        
        let filter1 = &result.base_filters[0];
        if let Condition::Comparison { op, value } = &filter1.condition {
            assert_eq!(*op, CompOp::Gt);
            assert_eq!(*value, Literal::Date("today".to_string()));
        } else {
            panic!("Expected comparison with today");
        }
        
        let filter2 = &result.base_filters[1];
        if let Condition::Comparison { op, value } = &filter2.condition {
            assert_eq!(*op, CompOp::Lte);
            assert_eq!(*value, Literal::Date("yesterday".to_string()));
        } else {
            panic!("Expected comparison with yesterday");
        }
    }

    #[test]
    fn test_current_user() {
        let input = r#"Filter: assignee[!=current_user]"#;
        let result = parse_string(input).unwrap();
        
        let filter = &result.base_filters[0];
        if let Condition::Comparison { op, value } = &filter.condition {
            assert_eq!(*op, CompOp::NotEq);
            assert_eq!(*value, Literal::CurrentUser);
        } else {
            panic!("Expected comparison with current_user");
        }
    }

    #[test]
    fn test_in_clause() {
        let input = r#"Filter: status[IN ("Open", "Pending")]"#;
        let result = parse_string(input).unwrap();

        let filter = &result.base_filters[0];
        assert_eq!(filter.field.0, "status");

        if let Condition::In(values) = &filter.condition {
            assert_eq!(values.len(), 2);
            assert_eq!(values[0], Literal::String("Open".to_string()));
            assert_eq!(values[1], Literal::String("Pending".to_string()));
        } else {
            panic!("Expected IN condition");
        }
    }

    #[test]
    fn test_in_clause_empty() {
        let input = r#"Filter: status[IN ()]"#;
        let result = parse_string(input).unwrap();
        let filter = &result.base_filters[0];
        if let Condition::In(values) = &filter.condition {
            assert!(values.is_empty());
        } else {
            panic!("Expected IN condition");
        }
    }

    #[test]
    fn test_in_clause_single_item() {
        let input = r#"Filter: status[IN ("Open")]"#;
        let result = parse_string(input).unwrap();
        let filter = &result.base_filters[0];
        if let Condition::In(values) = &filter.condition {
            assert_eq!(values.len(), 1);
            assert_eq!(values[0], Literal::String("Open".to_string()));
        } else {
            panic!("Expected IN condition");
        }
    }

    #[test]
    fn test_in_clause_trailing_comma_is_error() {
        let input = r#"Filter: status[IN ("Open",)]"#;
        assert!(parse_string(input).is_err());
    }

    #[test]
    fn test_complex_query() {
        let input = r#"Filter: title["Plan" AND ("v1" OR "v2")]; priority[>2]; CrossFilter: <Test-Run> status["PASS"]"#;
        let result = parse_string(input).unwrap();
        
        assert_eq!(result.base_filters.len(), 2);
        assert_eq!(result.cross_filters.len(), 1);
        
        // Verify the complex title condition
        let title_filter = &result.base_filters[0];
        assert_eq!(title_filter.field.0, "title");
        
        if let Condition::And(left, right) = &title_filter.condition {
            // Left should be "Plan"
            if let Condition::Comparison { value, .. } = left.as_ref() {
                assert_eq!(*value, Literal::String("Plan".to_string()));
            } else {
                panic!("Expected comparison on left side of AND");
            }
            
            // Right should be grouped OR
            if let Condition::Grouped(inner) = right.as_ref() {
                if let Condition::Or(_, _) = inner.as_ref() {
                    // Success
                } else {
                    panic!("Expected OR inside group");
                }
            } else {
                panic!("Expected grouped condition on right side of AND");
            }
        } else {
            panic!("Expected AND condition for title");
        }
    }
} 