//! SQL compiler that converts AST to optimized SQL queries using sea-query.

use crate::ast::{Query as AstQuery, FieldFilter, CrossFilter, Condition, CompOp, Literal};
use sea_query::{SelectStatement, Asterisk, Expr, SimpleExpr, PostgresQueryBuilder, JoinType, Iden, Value};
use std::collections::HashMap;

/// Configuration for SQL optimization
#[derive(Debug, Clone)]
pub struct OptimizationConfig {
    /// Maximum number of OR conditions before converting to IN clause
    pub max_or_conditions_for_in: usize,
    /// Maximum number of IN values before splitting into UNION
    pub max_in_values: usize,
}

impl Default for OptimizationConfig {
    fn default() -> Self {
        Self {
            max_or_conditions_for_in: 5,
            max_in_values: 1000,
        }
    }
}

/// Represents a table identifier for sea-query
#[derive(Debug, Clone, Copy)]
pub enum TableName {
    Base,
    Joined(usize), // For multiple joins, use index
}

impl Iden for TableName {
    fn unquoted(&self, s: &mut dyn std::fmt::Write) {
        match self {
            TableName::Base => write!(s, "base_table").unwrap(),
            TableName::Joined(idx) => write!(s, "joined_table_{}", idx).unwrap(),
        }
    }
}

/// Column identifier wrapper
#[derive(Debug, Clone)]
pub struct ColumnName(pub String);

impl Iden for ColumnName {
    fn unquoted(&self, s: &mut dyn std::fmt::Write) {
        write!(s, "{}", self.0).unwrap();
    }
}

/// SQL Compiler that converts AST to SQL queries
pub struct SqlCompiler {
    config: OptimizationConfig,
    /// Maps entity names to table names for schema resolution
    table_mapping: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompileError {
    pub message: String,
}

impl CompileError {
    fn new(message: String) -> Self {
        Self { message }
    }
}

/// Represents an optimization applied during compilation
#[derive(Debug, Clone, PartialEq)]
pub enum Optimization {
    OrToIn { field: String, value_count: usize },
    InToUnion { field: String, total_values: usize, union_count: usize },
    ConditionSimplification { original: String, simplified: String },
    RedundantConditionRemoval { removed_condition: String },
}

/// Result of SQL compilation with optimization information
#[derive(Debug)]
pub struct CompileResult {
    pub sql: String,
    pub optimizations: Vec<Optimization>,
}

impl SqlCompiler {
    pub fn new() -> Self {
        Self {
            config: OptimizationConfig::default(),
            table_mapping: HashMap::new(),
        }
    }

    pub fn with_config(config: OptimizationConfig) -> Self {
        Self {
            config,
            table_mapping: HashMap::new(),
        }
    }

    /// Set table mapping for entity names
    pub fn set_table_mapping(&mut self, mapping: HashMap<String, String>) {
        self.table_mapping = mapping;
    }

    /// Get the actual table name for an entity
    fn get_table_name(&self, entity: &str) -> String {
        self.table_mapping
            .get(entity)
            .cloned()
            .unwrap_or_else(|| entity.to_lowercase())
    }

        /// Compile a Query AST into optimized SQL  
    pub fn compile(&self, query: AstQuery) -> Result<CompileResult, CompileError> {
        let mut optimizations = Vec::new();
        
        // Start with a basic SELECT query
        let mut select = SelectStatement::new();
        select.from(TableName::Base);
        select.column(Asterisk);

        // Handle base filters
        if !query.base_filters.is_empty() {
            let (conditions, mut filter_opts) = self.compile_field_filters(&query.base_filters)?;
            optimizations.append(&mut filter_opts);
            
            select.and_where(conditions);
        }

        // Handle cross filters (JOINs)
        let mut join_index = 0;
        for cross_filter in query.cross_filters {
            let (join_conditions, mut cross_opts) = self.compile_cross_filter(&cross_filter, &mut join_index)?;
            optimizations.append(&mut cross_opts);
            
            // Add JOIN
            let _source_table = self.get_table_name(&cross_filter.source_entity.0);
            let _target_table = self.get_table_name(&cross_filter.target_entity.0);
            
            select.join(
                JoinType::InnerJoin,
                TableName::Joined(join_index),
                Expr::col((TableName::Base, ColumnName("id".to_string())))
                    .equals((TableName::Joined(join_index), ColumnName("id".to_string())))
            );

            select.and_where(join_conditions);
        }

        // Build the final SQL
        let sql = select.to_string(PostgresQueryBuilder);

        Ok(CompileResult {
            sql,
            optimizations,
        })
    }

    /// Compile field filters with optimizations
    fn compile_field_filters(&self, filters: &[FieldFilter]) -> Result<(SimpleExpr, Vec<Optimization>), CompileError> {
        let mut optimizations = Vec::new();
        let mut conditions = Vec::new();

        for filter in filters {
            let (condition, mut opts) = self.compile_condition(&filter.field.0, &filter.condition)?;
            optimizations.append(&mut opts);
            conditions.push(condition);
        }

        // Combine all conditions with AND
        let combined = self.combine_conditions_with_and(conditions);
        
        Ok((combined, optimizations))
    }

    /// Compile cross filter with optimizations
    fn compile_cross_filter(&self, cross_filter: &CrossFilter, join_index: &mut usize) -> Result<(SimpleExpr, Vec<Optimization>), CompileError> {
        *join_index += 1;
        
        let mut optimizations = Vec::new();
        let mut conditions = Vec::new();

        for filter in &cross_filter.filters {
            // Use the joined table for the field reference
            let qualified_field = format!("joined_table_{}.{}", join_index, filter.field.0);
            let (condition, mut opts) = self.compile_condition(&qualified_field, &filter.condition)?;
            optimizations.append(&mut opts);
            conditions.push(condition);
        }

        let combined = self.combine_conditions_with_and(conditions);
        Ok((combined, optimizations))
    }

    /// Compile a single condition with optimizations
    fn compile_condition(&self, field: &str, condition: &Condition) -> Result<(SimpleExpr, Vec<Optimization>), CompileError> {
        let mut optimizations = Vec::new();
        
        let expr = match condition {
            Condition::Comparison { op, value } => {
                self.compile_comparison(field, op, value)?
            }
            Condition::And(left, right) => {
                let (left_expr, mut left_opts) = self.compile_condition(field, left)?;
                let (right_expr, mut right_opts) = self.compile_condition(field, right)?;
                optimizations.append(&mut left_opts);
                optimizations.append(&mut right_opts);
                left_expr.and(right_expr)
            }
            Condition::Or(left, right) => {
                // Check for OR optimization opportunities
                if let Some((in_expr, opt)) = self.try_optimize_or_to_in(field, condition)? {
                    optimizations.push(opt);
                    in_expr
                } else {
                    let (left_expr, mut left_opts) = self.compile_condition(field, left)?;
                    let (right_expr, mut right_opts) = self.compile_condition(field, right)?;
                    optimizations.append(&mut left_opts);
                    optimizations.append(&mut right_opts);
                    left_expr.or(right_expr)
                }
            }
            Condition::Not(inner) => {
                let (inner_expr, mut inner_opts) = self.compile_condition(field, inner)?;
                optimizations.append(&mut inner_opts);
                inner_expr.not()
            }
            Condition::Grouped(inner) => {
                self.compile_condition(field, inner)?.0
            }
            Condition::In(values) => {
                let in_values: Vec<Value> = values.iter()
                    .map(|v| self.literal_to_value(v))
                    .collect::<Result<Vec<_>, _>>()?;
                
                // Check if we need to split large IN clauses into UNION
                if in_values.len() > self.config.max_in_values {
                    optimizations.push(Optimization::InToUnion {
                        field: field.to_string(),
                        total_values: in_values.len(),
                        union_count: (in_values.len() + self.config.max_in_values - 1) / self.config.max_in_values,
                    });
                    // For now, we'll keep the IN clause but log the optimization opportunity
                }
                
                Expr::col(ColumnName(field.to_string())).is_in(in_values)
            }
            Condition::IsNull => {
                Expr::col(ColumnName(field.to_string())).is_null()
            }
            Condition::IsNotNull => {
                Expr::col(ColumnName(field.to_string())).is_not_null()
            }
        };

        Ok((expr, optimizations))
    }

    /// Try to optimize OR conditions to IN clauses
    fn try_optimize_or_to_in(&self, field: &str, condition: &Condition) -> Result<Option<(SimpleExpr, Optimization)>, CompileError> {
        let equality_values = self.extract_equality_values_from_or(field, condition);
        
        if equality_values.len() >= self.config.max_or_conditions_for_in {
            let in_values: Vec<Value> = equality_values.iter()
                .map(|v| self.literal_to_value(v))
                .collect::<Result<Vec<_>, _>>()?;
            
            let in_expr = Expr::col(ColumnName(field.to_string())).is_in(in_values);
            let optimization = Optimization::OrToIn {
                field: field.to_string(),
                value_count: equality_values.len(),
            };
            
            return Ok(Some((in_expr, optimization)));
        }
        
        Ok(None)
    }

    /// Extract equality values from OR conditions for the same field
    fn extract_equality_values_from_or<'a>(&self, _target_field: &str, condition: &'a Condition) -> Vec<&'a Literal> {
        let mut values = Vec::new();
        self.collect_equality_values(condition, &mut values);
        values
    }

    /// Recursively collect equality values from OR conditions
    fn collect_equality_values<'a>(&self, condition: &'a Condition, values: &mut Vec<&'a Literal>) {
        match condition {
            Condition::Comparison { op: CompOp::Eq, value } => {
                values.push(value);
            }
            Condition::Or(left, right) => {
                self.collect_equality_values(left, values);
                self.collect_equality_values(right, values);
            }
            Condition::Grouped(inner) => {
                self.collect_equality_values(inner, values);
            }
            _ => {} // Other condition types break the equality pattern
        }
    }

    /// Combine multiple conditions with AND
    fn combine_conditions_with_and(&self, conditions: Vec<SimpleExpr>) -> SimpleExpr {
        if conditions.is_empty() {
            return Expr::val(true).into();
        }
        
        conditions.into_iter().reduce(|acc, expr| acc.and(expr)).unwrap()
    }

    /// Compile a comparison operation
    fn compile_comparison(&self, field: &str, op: &CompOp, value: &Literal) -> Result<SimpleExpr, CompileError> {
        let col = Expr::col(ColumnName(field.to_string()));
        let val = self.literal_to_value(value)?;

        let expr = match op {
            CompOp::Eq => col.eq(val),
            CompOp::NotEq => col.ne(val),
            CompOp::Gt => col.gt(val),
            CompOp::Lt => col.lt(val),
            CompOp::Gte => col.gte(val),
            CompOp::Lte => col.lte(val),
        };

        Ok(expr)
    }

    /// Convert AST Literal to sea-query Value
    fn literal_to_value(&self, literal: &Literal) -> Result<Value, CompileError> {
        match literal {
            Literal::String(s) => Ok(Value::String(Some(Box::new(s.clone())))),
            Literal::Number(n) => Ok(Value::BigInt(Some(*n))),
            Literal::Date(d) => {
                // Handle special date keywords
                match d.as_str() {
                    "today" => Ok(Value::String(Some(Box::new("CURRENT_DATE".to_string())))),
                    "yesterday" => Ok(Value::String(Some(Box::new("CURRENT_DATE - INTERVAL '1 day'".to_string())))),
                    "tomorrow" => Ok(Value::String(Some(Box::new("CURRENT_DATE + INTERVAL '1 day'".to_string())))),
                    _ => Ok(Value::String(Some(Box::new(d.clone())))),
                }
            }
            Literal::CurrentUser => Ok(Value::String(Some(Box::new("CURRENT_USER".to_string())))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;

    fn create_test_compiler() -> SqlCompiler {
        let mut compiler = SqlCompiler::new();
        let mut mapping = HashMap::new();
        mapping.insert("Test".to_string(), "tests".to_string());
        mapping.insert("Run".to_string(), "test_runs".to_string());
        compiler.set_table_mapping(mapping);
        compiler
    }

    #[test]
    fn test_simple_filter_compilation() {
        let compiler = create_test_compiler();
        
        let query = Query {
            base_filters: vec![
                FieldFilter {
                    field: Identifier("status".to_string()),
                    condition: Condition::Comparison {
                        op: CompOp::Eq,
                        value: Literal::String("Open".to_string()),
                    },
                }
            ],
            cross_filters: vec![],
        };

        let result = compiler.compile(query).unwrap();
        assert!(result.sql.contains("status"));
        assert!(result.sql.contains("Open"));
    }

    #[test]
    fn test_or_to_in_optimization() {
        let mut config = OptimizationConfig::default();
        config.max_or_conditions_for_in = 2; // Lower threshold for testing
        let compiler = SqlCompiler::with_config(config);

        // Create an OR condition with multiple equality checks
        let condition = Condition::Or(
            Box::new(Condition::Comparison {
                op: CompOp::Eq,
                value: Literal::String("Open".to_string()),
            }),
            Box::new(Condition::Or(
                Box::new(Condition::Comparison {
                    op: CompOp::Eq,
                    value: Literal::String("Pending".to_string()),
                }),
                Box::new(Condition::Comparison {
                    op: CompOp::Eq,
                    value: Literal::String("Review".to_string()),
                }),
            )),
        );

        let query = Query {
            base_filters: vec![
                FieldFilter {
                    field: Identifier("status".to_string()),
                    condition,
                }
            ],
            cross_filters: vec![],
        };

        let result = compiler.compile(query).unwrap();
        
        // Should have applied OR to IN optimization
        assert_eq!(result.optimizations.len(), 1);
        match &result.optimizations[0] {
            Optimization::OrToIn { field, value_count } => {
                assert_eq!(field, "status");
                assert_eq!(*value_count, 3);
            }
            _ => panic!("Expected OrToIn optimization"),
        }
    }

    #[test]
    fn test_cross_filter_compilation() {
        let compiler = create_test_compiler();
        
        let query = Query {
            base_filters: vec![],
            cross_filters: vec![
                CrossFilter {
                    source_entity: Identifier("Test".to_string()),
                    target_entity: Identifier("Run".to_string()),
                    filters: vec![
                        FieldFilter {
                            field: Identifier("status".to_string()),
                            condition: Condition::Comparison {
                                op: CompOp::Eq,
                                value: Literal::String("PASS".to_string()),
                            },
                        }
                    ],
                }
            ],
        };

        let result = compiler.compile(query).unwrap();
        assert!(result.sql.contains("JOIN"));
        assert!(result.sql.contains("joined_table_1"));
    }

    #[test]
    fn test_complex_query_with_optimizations() {
        let compiler = create_test_compiler();
        
        // Create a complex condition that should trigger optimizations
        let or_condition = Condition::Or(
            Box::new(Condition::Comparison {
                op: CompOp::Eq,
                value: Literal::String("High".to_string()),
            }),
            Box::new(Condition::Or(
                Box::new(Condition::Comparison {
                    op: CompOp::Eq,
                    value: Literal::String("Critical".to_string()),
                }),
                Box::new(Condition::Or(
                    Box::new(Condition::Comparison {
                        op: CompOp::Eq,
                        value: Literal::String("Blocker".to_string()),
                    }),
                    Box::new(Condition::Or(
                        Box::new(Condition::Comparison {
                            op: CompOp::Eq,
                            value: Literal::String("Urgent".to_string()),
                        }),
                        Box::new(Condition::Comparison {
                            op: CompOp::Eq,
                            value: Literal::String("Emergency".to_string()),
                        }),
                    )),
                )),
            )),
        );

        let query = Query {
            base_filters: vec![
                FieldFilter {
                    field: Identifier("priority".to_string()),
                    condition: or_condition,
                },
                FieldFilter {
                    field: Identifier("assignee".to_string()),
                    condition: Condition::Comparison {
                        op: CompOp::NotEq,
                        value: Literal::CurrentUser,
                    },
                }
            ],
            cross_filters: vec![],
        };

        let result = compiler.compile(query).unwrap();
        
        // Should have optimized the OR condition to IN
        assert!(!result.optimizations.is_empty());
        assert!(result.sql.contains("priority"));
        assert!(result.sql.contains("assignee"));
    }

    #[test]
    fn test_date_keyword_handling() {
        let compiler = create_test_compiler();
        
        let query = Query {
            base_filters: vec![
                FieldFilter {
                    field: Identifier("created_date".to_string()),
                    condition: Condition::Comparison {
                        op: CompOp::Gt,
                        value: Literal::Date("today".to_string()),
                    },
                }
            ],
            cross_filters: vec![],
        };

        let result = compiler.compile(query).unwrap();
        assert!(result.sql.contains("created_date"));
        assert!(result.sql.contains("CURRENT_DATE"));
    }
} 