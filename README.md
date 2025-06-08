# Filter DSL 到 SQL 编译器

一个将自定义Filter DSL语言编译为优化SQL查询的Rust库。(目前仅作为工作项目的摘取)

## 功能特性

- **DSL解析**: 支持复杂的Filter表达式和CrossFilter关联查询
- **SQL优化**: 自动进行OR到IN转换、大量IN值的UNION拆分等优化
- **JSON配置**: 支持从JSON文件加载实体到数据库表的映射配置
- **批量查询**: 支持大数据集的批量查询处理
- **类型安全**: 使用Rust的类型系统确保编译时安全

## JSON配置文件

### 配置文件格式

在项目根目录创建 `table_mapping.json` 文件：

```json
{
  "Test": "tests",
  "Run": "test_runs", 
  "Project": "projects",
  "Task": "tasks",
  "User": "users",
  "Issue": "issues",
  "Milestone": "milestones",
  "Repository": "repositories"
}
```

### 配置说明

- **键**: DSL中使用的实体名称（如 `Test`, `Project`）
- **值**: 对应的数据库表名（如 `tests`, `projects`）
- 如果实体名称在配置中不存在，将自动使用小写的实体名作为表名

### 使用方式

#### 1. 从JSON配置创建编译器

```rust
use report_dispatcher::sql_compiler::SqlCompiler;

// 从JSON配置文件创建编译器
let compiler = SqlCompiler::from_json_config("table_mapping.json")?;

// 带优化配置创建编译器
let optimization_config = OptimizationConfig {
    max_or_conditions_for_in: 3,
    max_in_values: 500,
};
let compiler = SqlCompiler::from_json_config_with_optimization(
    "table_mapping.json", 
    optimization_config
)?;
```

#### 2. 手动设置配置

```rust
use report_dispatcher::config::TableMappingConfig;

// 加载配置
let config = TableMappingConfig::from_json_file("table_mapping.json")?;

// 应用到编译器
let mut compiler = SqlCompiler::new();
compiler.set_table_mapping_from_config(&config);
```

#### 3. 错误处理

```rust
match SqlCompiler::from_json_config("table_mapping.json") {
    Ok(compiler) => {
        println!("✓ 成功加载JSON配置");
        // 使用编译器...
    }
    Err(e) => {
        println!("⚠ 配置加载失败: {}", e);
        // 使用默认配置
        let mut compiler = SqlCompiler::new();
        let default_config = TableMappingConfig::default();
        compiler.set_table_mapping_from_config(&default_config);
    }
}
```

## DSL语法示例

```rust
// 基础Filter
let dsl = r#"Filter: status["Open"]; priority[>2]"#;

// 带CrossFilter的复杂查询
let dsl = r#"Filter: title["Release Plan"]; CrossFilter: <Test-Run> status["PASS"]"#;

// 编译为SQL
let result = compiler.compile(ast)?;
println!("生成的SQL: {}", result.sql);
```

## 运行示例

```bash
cargo run
```

程序将：
1. 显示当前的JSON配置信息
2. 演示DSL解析和SQL编译过程
3. 展示各种优化场景
4. 演示批量查询处理

## 配置文件位置

- 默认位置: 项目根目录的 `table_mapping.json`
- 可以通过API指定其他路径
- 如果配置文件不存在，将使用内置的默认配置

## 默认配置

如果没有JSON配置文件，系统将使用以下默认映射：

```json
{
  "Test": "tests",
  "Run": "test_runs",
  "Project": "projects", 
  "Task": "tasks",
  "User": "users",
  "Issue": "issues"
}
```

## 依赖项

- `serde`: JSON序列化/反序列化
- `serde_json`: JSON处理
- `sea-query`: SQL查询构建器
