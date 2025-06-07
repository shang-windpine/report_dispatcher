//! SQL 编译器，将 AST 转换为使用 sea-query 的优化 SQL 查询

use crate::ast::{Query as AstQuery, FieldFilter, CrossFilter, Condition, CompOp, Literal};
use crate::config::{TableMappingConfig, ConfigError};
use sea_query::{SelectStatement, Asterisk, Expr, SimpleExpr, PostgresQueryBuilder, JoinType, Iden, Value};
use std::collections::HashMap;

// =============================================================================
// 核心 Trait 定义
// =============================================================================

/// 核心查询编译器 trait - 所有编译器必须实现的基本功能
pub trait QueryCompiler {
    /// 将查询 AST 编译为 SQL 字符串
    fn compile(&self, query: AstQuery, entity: &str) -> Result<CompileResult, CompileError>;
    
    /// 获取编译器名称（用于调试和日志）
    fn name(&self) -> &'static str;
    
    /// 获取支持的 SQL 方言
    fn supported_dialect(&self) -> SqlDialect;
}

/// 查询优化器 trait - 可选的优化功能
pub trait QueryOptimizer {
    /// 应用查询优化
    fn optimize(&self, query: &mut AstQuery) -> Vec<Optimization>;
    
    /// 获取优化配置
    fn optimization_config(&self) -> &OptimizationConfig;
    
    /// 设置优化配置
    fn set_optimization_config(&mut self, config: OptimizationConfig);
}

/// 批量查询编译器 trait - 可选的批量处理功能
pub trait BatchQueryCompiler {
    /// 编译批量查询
    fn compile_batch(&self, query: AstQuery, entity: &str, config: &BatchConfig) -> Result<BatchQueryResult, CompileError>;
    
    /// 估算查询复杂度
    fn estimate_query_complexity(&self, query: &AstQuery) -> QueryComplexity;
}

/// 表映射配置器 trait - 可选的表映射功能
pub trait TableMappingProvider {
    /// 获取实体的实际表名称
    fn get_table_name(&self, entity: &str) -> String;
    
    /// 设置表名映射
    fn set_table_mapping(&mut self, mapping: HashMap<String, String>);
    
    /// 从配置文件加载映射
    fn load_mapping_from_config(&mut self, config: &TableMappingConfig) -> Result<(), ConfigError>;
}

/// 编译器工厂 trait - 用于创建不同类型的编译器
pub trait CompilerFactory {
    type Compiler: QueryCompiler;
    
    /// 创建默认编译器
    fn create_default() -> Self::Compiler;
    
    /// 从配置创建编译器
    fn create_with_config(config: CompilerConfig) -> Result<Self::Compiler, CompileError>;
}

// =============================================================================
// 职责分离的具体实现结构体
// =============================================================================

/// 查询优化器的具体实现
#[derive(Debug, Clone)]
pub struct DefaultQueryOptimizer {
    config: OptimizationConfig,
}

impl DefaultQueryOptimizer {
    pub fn new() -> Self {
        Self {
            config: OptimizationConfig::default(),
        }
    }
    
    pub fn with_config(config: OptimizationConfig) -> Self {
        Self { config }
    }
}

impl QueryOptimizer for DefaultQueryOptimizer {
    fn optimize(&self, _query: &mut AstQuery) -> Vec<Optimization> {
        // 预处理优化逻辑可以在这里实现
        // 目前优化逻辑在 compile 过程中进行
        Vec::new()
    }
    
    fn optimization_config(&self) -> &OptimizationConfig {
        &self.config
    }
    
    fn set_optimization_config(&mut self, config: OptimizationConfig) {
        self.config = config;
    }
}

/// 批量查询处理器的具体实现
#[derive(Debug, Clone)]
pub struct DefaultBatchProcessor {
    config: BatchConfig,
}

impl DefaultBatchProcessor {
    pub fn new() -> Self {
        Self {
            config: BatchConfig::default(),
        }
    }
    
    pub fn with_config(config: BatchConfig) -> Self {
        Self { config }
    }
}

impl BatchQueryCompiler for DefaultBatchProcessor {
    fn compile_batch(&self, query: AstQuery, entity: &str, config: &BatchConfig) -> Result<BatchQueryResult, CompileError> {
        if !config.enable_batch_processing {
            // 如果不启用批量处理，需要有一个基础编译器来处理
            // 这里我们临时创建一个简单的编译器
            let basic_compiler = SqlCompiler::new();
            let result = basic_compiler.compile(query, entity)?;
            return Ok(BatchQueryResult {
                queries: vec![result.sql],
                optimizations: result.optimizations,
                total_estimated_rows: None,
            });
        }

        // 分析查询以查看是否包含大型 IN 子句
        let large_in_conditions = self.find_large_in_conditions(&query, config.max_batch_size);
        
        if large_in_conditions.is_empty() {
            // 没有大型 IN 条件，使用标准编译
            let basic_compiler = SqlCompiler::new();
            let result = basic_compiler.compile(query, entity)?;
            return Ok(BatchQueryResult {
                queries: vec![result.sql],
                optimizations: result.optimizations,
                total_estimated_rows: None,
            });
        }

        // 拆分为批量查询
        let mut all_queries = Vec::new();
        let mut all_optimizations = Vec::new();
        
        for (field, values) in large_in_conditions {
            let batches = self.create_batches(&values, config.max_batch_size);
            
            for batch in batches {
                let mut batch_query = query.clone();
                // 用批次替换大型 IN 条件
                self.replace_in_condition_with_batch(&mut batch_query, &field, batch);
                
                let basic_compiler = SqlCompiler::new();
                let result = basic_compiler.compile(batch_query, entity)?;
                all_queries.push(result.sql);
                all_optimizations.extend(result.optimizations);
            }
        }

        // 添加批量处理优化信息
        all_optimizations.push(Optimization::InToUnion {
            field: "batch_processing".to_string(),
            total_values: all_queries.len(),
            union_count: all_queries.len(),
        });

        let query_count = all_queries.len();
        Ok(BatchQueryResult {
            queries: all_queries,
            optimizations: all_optimizations,
            total_estimated_rows: Some(query_count * config.max_batch_size),
        })
    }
    
    fn estimate_query_complexity(&self, query: &AstQuery) -> QueryComplexity {
        let join_count = query.cross_filters.len();
        let condition_count = query.base_filters.len() + 
            query.cross_filters.iter().map(|f| f.filters.len()).sum::<usize>();
        
        // 简单的复杂度评估算法
        let complexity_score = (join_count as f64 * 2.0) + (condition_count as f64 * 1.0);
        
        QueryComplexity {
            estimated_rows: None, // 需要更复杂的统计信息来估算
            join_count,
            condition_count,
            complexity_score,
        }
    }
}

impl DefaultBatchProcessor {
    /// 查找超过批次大小阈值的 IN 条件
    fn find_large_in_conditions(&self, query: &AstQuery, max_batch_size: usize) -> Vec<(String, Vec<Literal>)> {
        let mut large_conditions = Vec::new();
        
        // 检查基础Filter
        for filter in &query.base_filters {
            if let Some((field, values)) = self.extract_large_in_from_condition(&filter.field.0, &filter.condition, max_batch_size) {
                large_conditions.push((field, values));
            }
        }
        
        // 检查关联Filter
        for cross_filter in &query.cross_filters {
            for filter in &cross_filter.filters {
                if let Some((field, values)) = self.extract_large_in_from_condition(&filter.field.0, &filter.condition, max_batch_size) {
                    large_conditions.push((field, values));
                }
            }
        }
        
        large_conditions
    }

    /// 从条件树中提取大型 IN 条件
    fn extract_large_in_from_condition(&self, field: &str, condition: &Condition, max_batch_size: usize) -> Option<(String, Vec<Literal>)> {
        match condition {
            Condition::In(values) if values.len() > max_batch_size => {
                Some((field.to_string(), values.clone()))
            }
            Condition::And(left, right) | Condition::Or(left, right) => {
                self.extract_large_in_from_condition(field, left, max_batch_size)
                    .or_else(|| self.extract_large_in_from_condition(field, right, max_batch_size))
            }
            Condition::Not(inner) | Condition::Grouped(inner) => {
                self.extract_large_in_from_condition(field, inner, max_batch_size)
            }
            _ => None,
        }
    }

    /// 从值列表创建批次
    fn create_batches(&self, values: &[Literal], batch_size: usize) -> Vec<Vec<Literal>> {
        values.chunks(batch_size)
            .map(|chunk| chunk.to_vec())
            .collect()
    }

    /// 用较小的批次替换大型 IN 条件
    fn replace_in_condition_with_batch(&self, _query: &mut AstQuery, _field: &str, _batch: Vec<Literal>) {
        // 这是一个简化的实现
        // 在实际实现中，需要遍历 AST 并替换特定的 IN 条件
    }
}

/// 表映射管理器的具体实现
#[derive(Debug, Clone)]
pub struct DefaultTableMapper {
    mappings: HashMap<String, String>,
}

impl DefaultTableMapper {
    pub fn new() -> Self {
        Self {
            mappings: HashMap::new(),
        }
    }
    
    pub fn with_mappings(mappings: HashMap<String, String>) -> Self {
        Self { mappings }
    }
}

impl TableMappingProvider for DefaultTableMapper {
    fn get_table_name(&self, entity: &str) -> String {
        self.mappings
            .get(entity)
            .cloned()
            .unwrap_or_else(|| entity.to_lowercase())
    }
    
    fn set_table_mapping(&mut self, mapping: HashMap<String, String>) {
        self.mappings = mapping;
    }
    
    fn load_mapping_from_config(&mut self, config: &TableMappingConfig) -> Result<(), ConfigError> {
        self.mappings = config.mappings.clone();
        Ok(())
    }
}

// =============================================================================
// 核心数据结构
// =============================================================================

/// SQL 方言枚举
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SqlDialect {
    PostgreSQL,
    MySQL,
    SQLite,
    MsSQL,
    Oracle,
}

/// 查询复杂度评估
#[derive(Debug, Clone, PartialEq)]
pub struct QueryComplexity {
    pub estimated_rows: Option<usize>,
    pub join_count: usize,
    pub condition_count: usize,
    pub complexity_score: f64,
}

/// 编译器配置
#[derive(Debug, Clone)]
pub struct CompilerConfig {
    pub optimization_config: OptimizationConfig,
    pub batch_config: BatchConfig,
    pub table_mapping: HashMap<String, String>,
    pub dialect: SqlDialect,
}

impl Default for CompilerConfig {
    fn default() -> Self {
        Self {
            optimization_config: OptimizationConfig::default(),
            batch_config: BatchConfig::default(),
            table_mapping: HashMap::new(),
            dialect: SqlDialect::PostgreSQL,
        }
    }
}

/// SQL 优化配置
#[derive(Debug, Clone)]
pub struct OptimizationConfig {
    /// 转换为 IN 子句前的最大 OR 条件数
    pub max_or_conditions_for_in: usize,
    /// 拆分为 UNION 前的最大 IN 值数量
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

/// 批量查询生成配置
#[derive(Debug, Clone)]
pub struct BatchConfig {
    /// 每批次的最大 IN 值数量
    pub max_batch_size: usize,
    /// 是否为大型 IN 子句启用批量处理
    pub enable_batch_processing: bool,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 500,
            enable_batch_processing: true,
        }
    }
}

/// 编译错误
#[derive(Debug, Clone, PartialEq)]
pub struct CompileError {
    pub message: String,
}

impl CompileError {
    pub fn new(message: String) -> Self {
        Self { message }
    }
}

/// 代表编译期间应用的优化
#[derive(Debug, Clone, PartialEq)]
pub enum Optimization {
    OrToIn { field: String, value_count: usize },
    InToUnion { field: String, total_values: usize, union_count: usize },
    ConditionSimplification { original: String, simplified: String },
    RedundantConditionRemoval { removed_condition: String },
}

/// SQL 编译结果，包含优化信息
#[derive(Debug)]
pub struct CompileResult {
    pub sql: String,
    pub optimizations: Vec<Optimization>,
}

/// 处理大型数据集的批量查询结果
#[derive(Debug)]
pub struct BatchQueryResult {
    pub queries: Vec<String>,
    pub optimizations: Vec<Optimization>,
    pub total_estimated_rows: Option<usize>,
}

// =============================================================================
// Sea-Query 相关结构
// =============================================================================

/// 代表 sea-query 的表标识符
#[derive(Debug, Clone)]
pub struct TableName(pub String);

impl Iden for TableName {
    fn unquoted(&self, s: &mut dyn std::fmt::Write) {
        write!(s, "{}", self.0).unwrap();
    }
}

/// 列标识符包装器
#[derive(Debug, Clone)]
pub struct ColumnName(pub String);

impl Iden for ColumnName {
    fn unquoted(&self, s: &mut dyn std::fmt::Write) {
        write!(s, "{}", self.0).unwrap();
    }
}

// =============================================================================
// 重构后的 SQL 编译器实现
// =============================================================================

/// 基于 sea-query 的 SQL 编译器实现 - 现在只负责核心编译功能
pub struct SqlCompiler {
    optimizer: DefaultQueryOptimizer,
    batch_processor: DefaultBatchProcessor,
    table_mapper: DefaultTableMapper,
}

impl SqlCompiler {
    /// 创建新的编译器实例
    pub fn new() -> Self {
        Self {
            optimizer: DefaultQueryOptimizer::new(),
            batch_processor: DefaultBatchProcessor::new(),
            table_mapper: DefaultTableMapper::new(),
        }
    }
    
    /// 从完整配置创建编译器
    pub fn from_config(config: CompilerConfig) -> Self {
        Self {
            optimizer: DefaultQueryOptimizer::with_config(config.optimization_config),
            batch_processor: DefaultBatchProcessor::with_config(config.batch_config),
            table_mapper: DefaultTableMapper::with_mappings(config.table_mapping),
        }
    }

    /// 获取优化器的引用
    pub fn optimizer(&self) -> &DefaultQueryOptimizer {
        &self.optimizer
    }

    /// 获取批量处理器的引用
    pub fn batch_processor(&self) -> &DefaultBatchProcessor {
        &self.batch_processor
    }

    /// 获取表映射器的引用
    pub fn table_mapper(&self) -> &DefaultTableMapper {
        &self.table_mapper
    }

    /// 获取优化器的可变引用
    pub fn optimizer_mut(&mut self) -> &mut DefaultQueryOptimizer {
        &mut self.optimizer
    }

    /// 获取批量处理器的可变引用
    pub fn batch_processor_mut(&mut self) -> &mut DefaultBatchProcessor {
        &mut self.batch_processor
    }

    /// 获取表映射器的可变引用
    pub fn table_mapper_mut(&mut self) -> &mut DefaultTableMapper {
        &mut self.table_mapper
    }

    /// 编译并优化查询的便捷方法
    pub fn compile_optimized(&mut self, mut query: AstQuery, entity: &str) -> Result<CompileResult, CompileError> {
        let optimizations = self.optimizer.optimize(&mut query);
        let mut result = self.compile(query, entity)?;
        result.optimizations.extend(optimizations);
        Ok(result)
    }

    /// 编译批量查询的便捷方法
    pub fn compile_batch_query(&self, query: AstQuery, entity: &str) -> Result<BatchQueryResult, CompileError> {
        let batch_config = &self.batch_processor.config;
        self.batch_processor.compile_batch(query, entity, batch_config)
    }
}

impl Default for SqlCompiler {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// SqlCompiler Trait 实现 - 现在只实现核心编译功能
// =============================================================================

impl QueryCompiler for SqlCompiler {
    fn compile(&self, query: AstQuery, entity: &str) -> Result<CompileResult, CompileError> {
        let mut optimizations = Vec::new();
        
        // 获取实际的表名
        let table_name = self.table_mapper.get_table_name(entity);
        
        // 从基本 SELECT 查询开始
        let mut select = SelectStatement::new();
        select.from(TableName(table_name));
        select.column(Asterisk);

        // 处理基础Filter
        if !query.base_filters.is_empty() {
            let (conditions, mut filter_opts) = self.compile_field_filters(&query.base_filters, entity)?;
            optimizations.append(&mut filter_opts);
            select.and_where(conditions);
        }

        // 处理关联Filter (JOINs)
        let mut join_index = 0;
        for cross_filter in query.cross_filters {
            let (join_conditions, mut cross_opts) = self.compile_cross_filter(&cross_filter, &mut join_index, &cross_filter.target_entity.0)?;
            optimizations.append(&mut cross_opts);
            
            // 获取关联表的实际名称
            let join_table_name = self.table_mapper.get_table_name(&cross_filter.target_entity.0);
            
            // 添加 JOIN
            select.join(
                JoinType::InnerJoin,
                TableName(format!("{} AS joined_table_{}", join_table_name, join_index)),
                Expr::col((TableName(self.table_mapper.get_table_name(entity)), ColumnName("id".to_string())))
                    .equals((TableName(format!("joined_table_{}", join_index)), ColumnName("id".to_string())))
            );

            select.and_where(join_conditions);
        }

        // 构建最终 SQL
        let sql = select.to_string(PostgresQueryBuilder);

        Ok(CompileResult {
            sql,
            optimizations,
        })
    }
    
    fn name(&self) -> &'static str {
        "SeaQuerySqlCompiler"
    }
    
    fn supported_dialect(&self) -> SqlDialect {
        SqlDialect::PostgreSQL
    }
}

// =============================================================================
// SqlCompiler 内部实现方法
// =============================================================================

impl SqlCompiler {
    /// 编译字段Filter并进行优化
    fn compile_field_filters(&self, filters: &[FieldFilter], entity: &str) -> Result<(SimpleExpr, Vec<Optimization>), CompileError> {
        let mut optimizations = Vec::new();
        let mut conditions = Vec::new();

        for filter in filters {
            // 使用实际的表名前缀
            let table_name = self.table_mapper.get_table_name(entity);
            let qualified_field = format!("{}.{}", table_name, filter.field.0);
            let (condition, mut opts) = self.compile_condition(&qualified_field, &filter.condition)?;
            optimizations.append(&mut opts);
            conditions.push(condition);
        }

        // 用 AND 组合所有条件
        let combined = self.combine_conditions_with_and(conditions);
        
        Ok((combined, optimizations))
    }

    /// 编译关联Filter并进行优化
    fn compile_cross_filter(&self, cross_filter: &CrossFilter, join_index: &mut usize, _join_entity: &str) -> Result<(SimpleExpr, Vec<Optimization>), CompileError> {
        *join_index += 1;
        
        let mut optimizations = Vec::new();
        let mut conditions = Vec::new();

        for filter in &cross_filter.filters {
            // 为字段引用使用连接表的实际名称
            let qualified_field = format!("joined_table_{}.{}", join_index, filter.field.0);
            let (condition, mut opts) = self.compile_condition(&qualified_field, &filter.condition)?;
            optimizations.append(&mut opts);
            conditions.push(condition);
        }

        let combined = self.combine_conditions_with_and(conditions);
        Ok((combined, optimizations))
    }

    /// 编译单个条件并进行优化
    fn compile_condition(&self, field: &str, condition: &Condition) -> Result<(SimpleExpr, Vec<Optimization>), CompileError> {
        let mut optimizations = Vec::new();
        let optimizer_config = self.optimizer.optimization_config();
        
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
                // 检查 OR 优化机会
                if let Some((in_expr, opt)) = self.try_optimize_or_to_in(field, condition, optimizer_config)? {
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
                
                // 检查是否需要将大型 IN 子句拆分为 UNION
                if in_values.len() > optimizer_config.max_in_values {
                    let (expr, opt) = self.split_large_in_to_union(field, &in_values, optimizer_config);
                    optimizations.push(opt);
                    expr
                } else {
                    Expr::col(ColumnName(field.to_string())).is_in(in_values)
                }
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

    /// 将大型 IN 子句拆分为 UNION 查询
    fn split_large_in_to_union(&self, field: &str, values: &[Value], config: &OptimizationConfig) -> (SimpleExpr, Optimization) {
        let chunk_size = config.max_in_values;
        let chunks: Vec<&[Value]> = values.chunks(chunk_size).collect();
        let union_count = chunks.len();
        
        // 为每个块创建单独的 IN 表达式
        let mut conditions = Vec::new();
        for chunk in chunks {
            let in_expr = Expr::col(ColumnName(field.to_string())).is_in(chunk.to_vec());
            conditions.push(in_expr);
        }
        
        // 用 OR 组合（在顶层有效地创建 UNION）
        let combined = conditions.into_iter().reduce(|acc, expr| acc.or(expr)).unwrap();
        
        let optimization = Optimization::InToUnion {
            field: field.to_string(),
            total_values: values.len(),
            union_count,
        };
        
        (combined, optimization)
    }

    /// 尝试将 OR 条件优化为 IN 子句
    fn try_optimize_or_to_in(&self, field: &str, condition: &Condition, config: &OptimizationConfig) -> Result<Option<(SimpleExpr, Optimization)>, CompileError> {
        let equality_values = self.extract_equality_values_from_or(field, condition);
        
        if equality_values.len() >= config.max_or_conditions_for_in {
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

    /// 从同一字段的 OR 条件中提取相等值
    fn extract_equality_values_from_or<'a>(&self, _target_field: &str, condition: &'a Condition) -> Vec<&'a Literal> {
        let mut values = Vec::new();
        self.collect_equality_values(condition, &mut values);
        values
    }

    /// 递归收集 OR 条件中的相等值
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
            _ => {} // 其他条件类型会破坏相等模式
        }
    }

    /// 用 AND 组合多个条件
    fn combine_conditions_with_and(&self, conditions: Vec<SimpleExpr>) -> SimpleExpr {
        if conditions.is_empty() {
            return Expr::val(true).into();
        }
        
        conditions.into_iter().reduce(|acc, expr| acc.and(expr)).unwrap()
    }

    /// 编译比较操作
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

    /// 将 AST 字面量转换为 sea-query 值
    fn literal_to_value(&self, literal: &Literal) -> Result<Value, CompileError> {
        match literal {
            Literal::String(s) => Ok(Value::String(Some(Box::new(s.clone())))),
            Literal::Number(n) => Ok(Value::BigInt(Some(*n))),
            Literal::Date(d) => {
                // 处理特殊日期关键字
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

// =============================================================================
// 编译器工厂实现
// =============================================================================

/// SqlCompiler 的工厂实现
pub struct SqlCompilerFactory;

impl CompilerFactory for SqlCompilerFactory {
    type Compiler = SqlCompiler;
    
    fn create_default() -> Self::Compiler {
        SqlCompiler::new()
    }
    
    fn create_with_config(config: CompilerConfig) -> Result<Self::Compiler, CompileError> {
        Ok(SqlCompiler::from_config(config))
    }
}

// =============================================================================
// 编译器注册表 - 支持动态选择编译器
// =============================================================================

/// 编译器注册表，用于管理不同的编译器实现
pub struct CompilerRegistry {
    compilers: HashMap<String, Box<dyn Fn() -> Box<dyn QueryCompiler>>>,
}

impl CompilerRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            compilers: HashMap::new(),
        };
        
        // 注册默认的 SqlCompiler
        registry.register("sql", || Box::new(SqlCompiler::new()));
        registry.register("default", || Box::new(SqlCompiler::new()));
        
        registry
    }
    
    /// 注册新的编译器
    pub fn register<F>(&mut self, name: &str, factory: F)
    where
        F: Fn() -> Box<dyn QueryCompiler> + 'static,
    {
        self.compilers.insert(name.to_string(), Box::new(factory));
    }
    
    /// 创建指定类型的编译器
    pub fn create(&self, name: &str) -> Option<Box<dyn QueryCompiler>> {
        self.compilers.get(name).map(|factory| factory())
    }
    
    /// 获取所有已注册的编译器名称
    pub fn available_compilers(&self) -> Vec<String> {
        self.compilers.keys().cloned().collect()
    }
}

impl Default for CompilerRegistry {
    fn default() -> Self {
        Self::new()
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
        compiler.table_mapper_mut().set_table_mapping(mapping);
        compiler
    }

    // =============================================================================
    // Trait 使用示例和测试
    // =============================================================================

    /// 示例：自定义编译器实现
    struct CustomCompiler {
        name: String,
        dialect: SqlDialect,
        config: OptimizationConfig,
    }

    impl CustomCompiler {
        fn new(name: String, dialect: SqlDialect) -> Self {
            Self { 
                name, 
                dialect,
                config: OptimizationConfig::default(),
            }
        }
    }

    impl QueryCompiler for CustomCompiler {
        fn compile(&self, _query: AstQuery, _entity: &str) -> Result<CompileResult, CompileError> {
            Ok(CompileResult {
                sql: format!("-- Generated by {} for {:?}\nSELECT * FROM custom_table;", self.name, self.dialect),
                optimizations: vec![],
            })
        }
        
        fn name(&self) -> &'static str {
            "CustomCompiler"
        }
        
        fn supported_dialect(&self) -> SqlDialect {
            self.dialect
        }
    }

    impl QueryOptimizer for CustomCompiler {
        fn optimize(&self, _query: &mut AstQuery) -> Vec<Optimization> {
            vec![Optimization::ConditionSimplification {
                original: "custom_original".to_string(),
                simplified: "custom_simplified".to_string(),
            }]
        }
        
        fn optimization_config(&self) -> &OptimizationConfig {
            &self.config
        }
        
        fn set_optimization_config(&mut self, _config: OptimizationConfig) {
            // Custom implementation
        }
    }

    impl BatchQueryCompiler for CustomCompiler {
        fn compile_batch(&self, query: AstQuery, entity: &str, _config: &BatchConfig) -> Result<BatchQueryResult, CompileError> {
            let result = self.compile(query, entity)?;
            Ok(BatchQueryResult {
                queries: vec![result.sql],
                optimizations: result.optimizations,
                total_estimated_rows: Some(100),
            })
        }
        
        fn estimate_query_complexity(&self, _query: &AstQuery) -> QueryComplexity {
            QueryComplexity {
                estimated_rows: Some(100),
                join_count: 0,
                condition_count: 1,
                complexity_score: 1.0,
            }
        }
    }

    impl TableMappingProvider for CustomCompiler {
        fn get_table_name(&self, entity: &str) -> String {
            format!("custom_{}", entity.to_lowercase())
        }
        
        fn set_table_mapping(&mut self, _mapping: HashMap<String, String>) {
            // Custom implementation
        }
        
        fn load_mapping_from_config(&mut self, _config: &TableMappingConfig) -> Result<(), ConfigError> {
            Ok(())
        }
    }

    #[test]
    fn test_trait_based_compilation() {
        let compiler: Box<dyn QueryCompiler> = Box::new(SqlCompiler::new());
        
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

        let result = compiler.compile(query, "Test").unwrap();
        assert_eq!(compiler.name(), "SeaQuerySqlCompiler");
        assert_eq!(compiler.supported_dialect(), SqlDialect::PostgreSQL);
        assert!(result.sql.contains("status"));
    }

    #[test]
    fn test_custom_compiler() {
        let compiler = CustomCompiler::new("TestCompiler".to_string(), SqlDialect::MySQL);
        
        let query = Query {
            base_filters: vec![],
            cross_filters: vec![],
        };

        let result = compiler.compile(query, "Test").unwrap();
        assert!(result.sql.contains("custom_table"));
        assert!(result.sql.contains("TestCompiler"));
        assert!(result.sql.contains("MySQL"));
        assert_eq!(compiler.name(), "CustomCompiler");
        assert_eq!(compiler.supported_dialect(), SqlDialect::MySQL);
    }

    #[test]
    fn test_compiler_interface() {
        let compiler = SqlCompiler::new();
        
        let query = Query {
            base_filters: vec![
                FieldFilter {
                    field: Identifier("priority".to_string()),
                    condition: Condition::Comparison {
                        op: CompOp::Eq,
                        value: Literal::String("High".to_string()),
                    },
                }
            ],
            cross_filters: vec![],
        };

        // 测试编译
        let result = compiler.compile(query.clone(), "Test").unwrap();
        assert!(result.sql.contains("priority"));
        
        // 测试复杂度评估
        let complexity = compiler.batch_processor().estimate_query_complexity(&query);
        assert_eq!(complexity.join_count, 0);
        assert_eq!(complexity.condition_count, 1);
        assert!(complexity.complexity_score > 0.0);
    }

    #[test]
    fn test_compiler_registry() {
        let mut registry = CompilerRegistry::new();
        
        // 注册自定义编译器
        registry.register("custom", || {
            Box::new(CustomCompiler::new("RegisteredCustom".to_string(), SqlDialect::SQLite))
        });
        
        // 测试默认编译器
        let default_compiler = registry.create("default").unwrap();
        assert_eq!(default_compiler.name(), "SeaQuerySqlCompiler");
        
        // 测试自定义编译器
        let custom_compiler = registry.create("custom").unwrap();
        assert_eq!(custom_compiler.name(), "CustomCompiler");
        
        // 测试可用编译器列表
        let available = registry.available_compilers();
        assert!(available.contains(&"default".to_string()));
        assert!(available.contains(&"custom".to_string()));
        assert!(available.contains(&"sql".to_string()));
    }

    #[test]
    fn test_compiler_factory() {
        // 测试默认工厂
        let compiler = SqlCompilerFactory::create_default();
        assert_eq!(compiler.name(), "SeaQuerySqlCompiler");
        
        // 测试配置工厂
        let config = CompilerConfig {
            optimization_config: OptimizationConfig {
                max_or_conditions_for_in: 10,
                max_in_values: 2000,
            },
            batch_config: BatchConfig::default(),
            table_mapping: {
                let mut map = HashMap::new();
                map.insert("Entity".to_string(), "entity_table".to_string());
                map
            },
            dialect: SqlDialect::PostgreSQL,
        };
        
        let compiler = SqlCompilerFactory::create_with_config(config.clone()).unwrap();
        assert_eq!(compiler.optimizer().optimization_config().max_or_conditions_for_in, 10);
        assert_eq!(compiler.optimizer().optimization_config().max_in_values, 2000);
        assert_eq!(compiler.table_mapper().get_table_name("Entity"), "entity_table");
    }

    #[test]
    fn test_different_sql_dialects() {
        let dialects = vec![
            SqlDialect::PostgreSQL,
            SqlDialect::MySQL,
            SqlDialect::SQLite,
            SqlDialect::MsSQL,
            SqlDialect::Oracle,
        ];
        
        for dialect in dialects {
            let compiler = CustomCompiler::new(format!("{:?}Compiler", dialect), dialect);
            assert_eq!(compiler.supported_dialect(), dialect);
            
            let query = Query {
                base_filters: vec![],
                cross_filters: vec![],
            };
            
            let result = compiler.compile(query, "Test").unwrap();
            assert!(result.sql.contains(&format!("{:?}", dialect)));
        }
    }
}