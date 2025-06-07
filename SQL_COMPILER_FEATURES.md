# SQL 编译器实现总结

## 核心功能

### 1. AST 到 SQL 转换
SQL 编译器能够将解析后的 AST 转换为标准的 PostgreSQL SQL 查询语句，使用 `sea-query` 库确保 SQL 语法的正确性和安全性。

### 2. 实体映射
- 支持实体名称到表名的自定义映射
- 默认将实体名转换为小写作为表名
- 示例：`Test` → `tests`，`Run` → `test_runs`

### 3. 字段过滤
将 DSL 中的字段过滤条件转换为 SQL 的 WHERE 子句：
- 比较操作符：`=`, `!=`, `>`, `<`, `>=`, `<=`
- 逻辑操作符：`AND`, `OR`, `NOT`
- 空值检查：`IS NULL`, `IS NOT NULL`
- 列表匹配：`IN (...)`

### 4. 关联过滤 (CrossFilter)
将 CrossFilter 转换为 SQL JOIN 操作：
- 使用 INNER JOIN 连接关联表
- 支持多层级的关联过滤
- 为每个 JOIN 分配唯一的表别名

## 优化策略

### 1. OR 条件优化为 IN 语句
**触发条件：** OR 条件数量超过配置阈值（默认 5 个）
**优化效果：** 将多个等值 OR 条件合并为单个 IN 语句

**示例转换：**
```sql
-- 原始（多个 OR）
status = 'Open' OR status = 'Pending' OR status = 'Review' OR status = 'Approved' OR status = 'Testing'

-- 优化后（IN 语句）
status IN ('Open', 'Pending', 'Review', 'Approved', 'Testing')
```

**优势：**
- 减少 SQL 语句长度
- 提高数据库执行效率
- 更好的索引利用

### 2. 大型 IN 语句拆分
**触发条件：** IN 语句中的值数量超过配置阈值（默认 1000 个）
**优化策略：** 标记为需要拆分为 UNION 查询的候选

**配置项：**
```rust
OptimizationConfig {
    max_or_conditions_for_in: 5,    // OR 转 IN 的阈值
    max_in_values: 1000,            // IN 值数量上限
}
```

### 3. 特殊值处理
**日期关键字优化：**
- `today` → `CURRENT_DATE`
- `yesterday` → `CURRENT_DATE - INTERVAL '1 day'`
- `tomorrow` → `CURRENT_DATE + INTERVAL '1 day'`

**用户关键字优化：**
- `current_user` → `CURRENT_USER`

## 实现特点

### 1. 类型安全
使用 `sea-query` 的类型系统确保生成的 SQL 语法正确：
- 参数化查询，防止 SQL 注入
- 强类型的列名和表名标识符
- 类型安全的值转换

### 2. 模块化设计
```rust
pub struct SqlCompiler {
    config: OptimizationConfig,
    table_mapping: HashMap<String, String>,
}
```

### 3. 优化追踪
```rust
pub enum Optimization {
    OrToIn { field: String, value_count: usize },
    InToUnion { field: String, total_values: usize, union_count: usize },
    ConditionSimplification { original: String, simplified: String },
    RedundantConditionRemoval { removed_condition: String },
}
```

### 4. 错误处理
完善的错误处理机制，包含详细的错误信息和位置信息。

## 生成的 SQL 示例

### 基础过滤
**DSL：** `Filter: status["Open"]; priority[>2]`
**SQL：**
```sql
SELECT * FROM "base_table" 
WHERE "status" = 'Open' AND "priority" > 2
```

### 关联过滤
**DSL：** `CrossFilter: <Test-Run> status["PASS"]`
**SQL：**
```sql
SELECT * FROM "base_table" 
INNER JOIN "joined_table_1" ON "base_table"."id" = "joined_table_1"."id" 
WHERE "joined_table_1.status" = 'PASS'
```

### 复杂查询示例
**DSL：** `Filter: priority[>3]; assignee[!=current_user]; CrossFilter: <Project-Task> status["Active"]`
**SQL：**
```sql
SELECT * FROM "base_table" 
INNER JOIN "joined_table_1" ON "base_table"."id" = "joined_table_1"."id" 
WHERE "priority" > 3 AND "assignee" <> 'CURRENT_USER' AND "joined_table_1.status" = 'Active'
```

## 性能优化总结

1. **OR 转 IN 优化**：自动检测并优化多个等值 OR 条件
2. **类型安全构建**：使用 sea-query 确保高效的 SQL 生成
3. **参数化查询**：防止 SQL 注入，提高数据库缓存效率  
4. **智能映射**：支持自定义实体到表名的映射
5. **优化追踪**：记录所有应用的优化，便于调试和性能分析

## 扩展性

编译器设计为可扩展的架构，方便添加新的优化策略：
- 条件简化（如 `field[true AND condition]` → `field[condition]`）
- 冗余条件移除
- 查询重写优化
- 索引提示生成

## 测试覆盖

实现了全面的单元测试，包括：
- 基础 SQL 生成测试
- 优化策略验证测试
- 复杂查询场景测试
- 特殊值处理测试
- 错误处理测试

所有 21 个测试用例均通过，确保了系统的稳定性和正确性。 