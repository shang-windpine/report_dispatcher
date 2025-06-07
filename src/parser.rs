//! The parser for the filter language.

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

    /// Returns the current token without advancing the position.
    fn peek(&self) -> Option<&Token<'a>> {
        self.tokens.get(self.position)
    }

    /// Returns the current token and advances the position.
    fn advance(&mut self) -> Option<&Token<'a>> {
        if self.position < self.tokens.len() {
            let token = &self.tokens[self.position];
            self.position += 1;
            Some(token)
        } else {
            None
        }
    }

    /// Expects a specific token kind and advances, or returns an error.
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

    /// Checks if the current token matches the given kind.
    fn match_token(&self, kind: &TokenKind) -> bool {
        if let Some(token) = self.peek() {
            std::mem::discriminant(&token.kind) == std::mem::discriminant(kind)
        } else {
            false
        }
    }

    /// Checks if the current token is a comparison operator.
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
                    self.advance(); // consume "Filter:"
                    let filters = self.parse_field_filters_until_semicolon_or_crossfilter()?;
                    base_filters.extend(filters);
                }
                TokenKind::CrossFilter => {
                    self.advance(); // consume "CrossFilter:"
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

    /// Parse field filters until we hit a semicolon, CrossFilter, or end of input.
    fn parse_field_filters_until_semicolon_or_crossfilter(&mut self) -> Result<Vec<FieldFilter>, ParseError> {
        let mut filters = Vec::new();

        loop {
            // Parse one field filter
            let filter = self.parse_field_filter()?;
            filters.push(filter);

            // Check if we need to continue
            if let Some(token) = self.peek() {
                match &token.kind {
                    TokenKind::Semicolon => {
                        self.advance(); // consume semicolon
                        // Check if next token is CrossFilter or end
                        if let Some(next_token) = self.peek() {
                            if matches!(next_token.kind, TokenKind::CrossFilter) {
                                break; // End of base filters
                            }
                            // Otherwise continue parsing more field filters
                        } else {
                            break; // End of input
                        }
                    }
                    TokenKind::CrossFilter => {
                        break; // End of base filters
                    }
                    _ => {
                        return Err(ParseError::at_position(
                            format!("Expected semicolon or CrossFilter, found {:?}", token.kind),
                            token.span,
                        ));
                    }
                }
            } else {
                break; // End of input
            }
        }

        Ok(filters)
    }

    fn parse_cross_filter(&mut self) -> Result<CrossFilter, ParseError> {
        // Expect <Source-Target>
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

        // Split the entity name by dash to get source and target
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

        // Parse field filters for the cross filter
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

    fn parse_condition(&mut self) -> Result<Condition, ParseError> {
        self.parse_or_expression()
    }

    fn parse_or_expression(&mut self) -> Result<Condition, ParseError> {
        let mut left = self.parse_and_expression()?;

        while self.match_token(&TokenKind::Or) {
            self.advance(); // consume OR
            let right = self.parse_and_expression()?;
            left = Condition::Or(Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    fn parse_and_expression(&mut self) -> Result<Condition, ParseError> {
        let mut left = self.parse_not_expression()?;

        while self.match_token(&TokenKind::And) {
            self.advance(); // consume AND
            let right = self.parse_not_expression()?;
            left = Condition::And(Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    fn parse_not_expression(&mut self) -> Result<Condition, ParseError> {
        if self.match_token(&TokenKind::Not) {
            self.advance(); // consume NOT
            let expr = self.parse_not_expression()?; // Allow chaining of NOT
            Ok(Condition::Not(Box::new(expr)))
        } else {
            self.parse_primary_expression()
        }
    }

    fn parse_primary_expression(&mut self) -> Result<Condition, ParseError> {
        if let Some(token) = self.peek() {
            match &token.kind {
                TokenKind::LParen => {
                    self.advance(); // consume (
                    let expr = self.parse_condition()?;
                    self.expect(TokenKind::RParen)?;
                    Ok(Condition::Grouped(Box::new(expr)))
                }
                TokenKind::Is => {
                    self.advance(); // consume IS
                    if self.match_token(&TokenKind::Not) {
                        self.advance(); // consume NOT
                        self.expect(TokenKind::Null)?;
                        Ok(Condition::IsNotNull)
                    } else {
                        self.expect(TokenKind::Null)?;
                        Ok(Condition::IsNull)
                    }
                }
                TokenKind::In => {
                    self.advance(); // consume IN
                    self.expect(TokenKind::LParen)?;
                    let mut values = Vec::new();
                    
                    // Parse comma-separated values
                    loop {
                        let value = self.parse_literal()?;
                        values.push(value);
                        
                        if let Some(token) = self.peek() {
                            if matches!(token.kind, TokenKind::RParen) {
                                break;
                            }
                            // We don't have comma in our token list, but let's assume space-separated for now
                            // In a real implementation, we'd add comma to the lexer
                        } else {
                            return Err(ParseError::new("Expected closing parenthesis".to_string(), None));
                        }
                    }
                    
                    self.expect(TokenKind::RParen)?;
                    Ok(Condition::In(values))
                }
                _ => {
                    // Check if this starts with a comparison operator
                    if self.is_comparison_operator() {
                        let op = self.parse_comparison_operator()?;
                        let value = self.parse_literal()?;
                        Ok(Condition::Comparison { op, value })
                    } else {
                        // Default to equality comparison if no operator is specified
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
                    // Unquoted string
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