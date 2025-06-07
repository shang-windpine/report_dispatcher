pub mod ast;
pub mod token;
pub mod parser;
pub mod lexer;
pub mod sql_compiler;
pub mod config;

use lexer::Lexer;
use parser::Parser;
use sql_compiler::{
    SqlCompiler, BatchConfig, CompilerConfig, OptimizationConfig
};
use config::TableMappingConfig;

/// 创建SQL编译器实例，优先使用JSON配置，失败时使用默认配置
fn create_compiler_with_config() -> SqlCompiler {
    match TableMappingConfig::from_json_file("table_mapping.json") {
        Ok(table_config) => {
            println!("✅ 成功从JSON配置文件加载表映射");
            let config = CompilerConfig {
                table_mapping: table_config.mappings,
                ..Default::default()
            };
            SqlCompiler::from_config(config)
        }
        Err(e) => {
            println!("⚠️ 无法加载JSON配置文件 ({}), 使用默认配置", e);
            SqlCompiler::new()
        }
    }
}

/// 创建SQL编译器实例（静默版本，不打印加载信息）
fn create_compiler_with_config_silent() -> SqlCompiler {
    match TableMappingConfig::from_json_file("table_mapping.json") {
        Ok(table_config) => {
            let config = CompilerConfig {
                table_mapping: table_config.mappings,
                ..Default::default()
            };
            SqlCompiler::from_config(config)
        }
        Err(_) => SqlCompiler::new(),
    }
}

fn main() {
    println!("--- Report Dispatcher: Filter到 SQL 编译器 ---");
    
    // 显示当前使用的表映射配置
    println!("\n[配置信息]:");
    match TableMappingConfig::from_json_file("table_mapping.json") {
        Ok(config) => {
            println!("✅ 使用JSON配置文件: table_mapping.json");
            println!("✅ 加载了 {} 个表映射配置", config.get_mappings().len());
            println!("配置详情:");
            for (entity, table) in config.get_mappings() {
                println!("  {} -> {}", entity, table);
            }
        }
        Err(e) => {
            println!("❌ JSON配置文件加载失败: {}", e);
            println!("⚠️ 将使用默认配置");
        }
    }

    // 1. 示例Filter
    let filter_string = r#"Filter: status["Open"]; priority[>2]; CrossFilter: <Test-Run> status["PASS"]"#;
    println!("\n[输入 DSL]:\n{}\n", filter_string);

    // 2. 词法分析器 - 对 DSL 进行分词
    println!("[步骤 1]: 对 DSL 进行分词...");
    let tokens: Vec<_> = Lexer::new(filter_string).collect();
    println!("生成了 {} 个 token", tokens.len());
    
    // 3. 语法分析器 - 从 token 构建 AST
    println!("\n[步骤 2]: 将 token 解析为 AST...");
    let mut parser = Parser::new(&tokens);
    match parser.parse() {
        Ok(ast) => {
            println!("✓ 成功将 DSL 解析为 AST");
            println!("AST 结构: {:#?}", ast);

            // 4. SQL 编译器 - 生成优化的 SQL
            println!("\n[步骤 3]: 将 AST 编译为 SQL...");
            let mut compiler = create_compiler_with_config();

            // 使用编译和优化方法，指定实体名为"Issue"
            match compiler.compile_optimized(ast.clone(), "Issue") {
                Ok(result) => {
                    println!("✅ 成功编译为 SQL");
                    println!("\n[生成的 SQL]:");
                    println!("{}", result.sql);
                    
                    if !result.optimizations.is_empty() {
                        println!("\n[应用的优化]:");
                        for opt in &result.optimizations {
                            println!("• {:?}", opt);
                        }
                    }

                    // 5. 演示批量查询编译
                    println!("\n[步骤 4]: 演示批量查询编译...");
                    
                    match compiler.compile_batch_query(ast, "Issue") {
                        Ok(batch_result) => {
                            println!("✓ 批量编译完成");
                            println!("生成了 {} 个 SQL 查询", batch_result.queries.len());
                            
                            if let Some(estimated_rows) = batch_result.total_estimated_rows {
                                println!("预计处理的总行数: {}", estimated_rows);
                            }
                            
                            if batch_result.queries.len() > 1 {
                                println!("\n[批量查询]:");
                                for (i, query) in batch_result.queries.iter().enumerate() {
                                    println!("批次 {}: {}", i + 1, query);
                                }
                            }
                        }
                        Err(e) => {
                            println!("✗ 批量编译失败: {}", e.message);
                        }
                    }
                }
                Err(e) => {
                    println!("✗ SQL 编译失败: {}", e.message);
                }
            }
        }
        Err(e) => {
            println!("✗ 解析失败: {}", e.message);
            if let Some(span) = e.span {
                println!("  位置 {}-{}", span.start, span.end);
            }
        }
    }
    
    // 演示大量 ID 处理场景
    demonstrate_large_id_scenarios();
}

fn demonstrate_large_id_scenarios() {
    println!("\n--- 大量ID处理场景演示 ---");
    
    // 模拟原Java项目中的"先查ID，再用ID IN查询"场景
    println!("\n[场景1]: 模拟大量ID的IN查询优化");
    let large_id_dsl = r#"Filter: id[IN (1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20)]"#;
    
    // 使用较小的阈值来触发优化
    let config = CompilerConfig {
        optimization_config: OptimizationConfig {
            max_in_values: 5,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut compiler = SqlCompiler::from_config(config);
    
    println!("DSL: {}", large_id_dsl);
    
    let tokens: Vec<_> = Lexer::new(large_id_dsl).collect();
    let mut parser = Parser::new(&tokens);
    
    if let Ok(ast) = parser.parse() {
        match compiler.compile_optimized(ast.clone(), "Issue") {
            Ok(result) => {
                println!("生成的SQL: {}", result.sql);
                println!("应用的优化:");
                for opt in &result.optimizations {
                    match opt {
                        sql_compiler::Optimization::InToUnion { field, total_values, union_count } => {
                            println!("  • 将{}字段的{}个值拆分为{}个UNION查询", field, total_values, union_count);
                        }
                        _ => println!("  • {:?}", opt),
                    }
                }
            }
            Err(e) => println!("编译失败: {}", e.message),
        }
        
        // 演示批量查询方案
        println!("\n[场景2]: 批量查询方案演示");
        
        match compiler.compile_batch_query(ast, "Issue") {
            Ok(batch_result) => {
                println!("生成了{}个批量查询:", batch_result.queries.len());
                for (i, query) in batch_result.queries.iter().enumerate() {
                    println!("  批次{}: {}", i + 1, query);
                }
            }
            Err(e) => println!("批量编译失败: {}", e.message),
        }
    }
}

/// 测试完整的spec示例
fn test_full_specification_example() {
    println!("\n--- 完整DSL示例测试 ---");
    
    let complex_dsl = r#"Filter: status["Open", "InProgress"]; assignee[CurrentUser]; priority[>=3]; created_date[>today]; CrossFilter: <Issue-Comment> content[CONTAINS "bug"]; created_date[>yesterday]"#;
    
    println!("复杂DSL: {}", complex_dsl);
    
    let tokens: Vec<_> = Lexer::new(complex_dsl).collect();
    let mut parser = Parser::new(&tokens);
    
    match parser.parse() {
        Ok(ast) => {
            println!("成功解析复杂DSL: {:#?}", ast);
            
            let mut compiler = create_compiler_with_config_silent();
            match compiler.compile_optimized(ast, "Issue") {
                Ok(result) => {
                    println!("\n复杂查询的SQL:");
                    println!("{}", result.sql);
                    
                    if !result.optimizations.is_empty() {
                        println!("\n优化信息:");
                        for opt in &result.optimizations {
                            println!("• {:?}", opt);
                        }
                    }
                }
                Err(e) => println!("复杂查询编译失败: {}", e.message),
            }
        }
        Err(e) => println!("复杂DSL解析失败: {}", e.message),
    }
}
