pub mod ast;
pub mod token;
pub mod parser;
pub mod lexer;
pub mod sql_compiler;
pub mod config;

use lexer::Lexer;
use parser::Parser;
use sql_compiler::{
    SqlCompiler, CompilerConfig
};
use config::TableMappingConfig;
use anyhow::Result;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

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

/// 处理单个Filter字符串的核心逻辑
fn process_filter_string(compiler: &mut SqlCompiler, filter_string: &str) {
    println!("\n[输入 DSL]:\n{}\n", filter_string);

    println!("[步骤 1]: 对 DSL 进行分词...");
    let tokens: Vec<_> = Lexer::new(filter_string).collect();
    println!("生成了 {} 个 token", tokens.len());
    
    println!("\n[步骤 2]: 将 token 解析为 AST...");
    let mut parser = Parser::new(&tokens);
    match parser.parse() {
        Ok(ast) => {
            println!("✓ 成功将 DSL 解析为 AST");

            println!("\n[步骤 3]: 将 AST 编译为 SQL...");
            
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
}

fn main() -> Result<()> {
    println!("--- Report Dispatcher: 交互式 Filter-to-SQL 编译器 ---");
    println!("输入 'exit' 或 'quit' 退出程序。");
    
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
    
    let mut compiler = create_compiler_with_config_silent();
    let mut rl = DefaultEditor::new()?;

    loop {
        match rl.readline(">> ") {
            Ok(line) => {
                let input = line.trim();
                if input.eq_ignore_ascii_case("exit") || input.eq_ignore_ascii_case("quit") {
                    break;
                }
                
                if input.is_empty() {
                    continue;
                }

                rl.add_history_entry(input)?;
                
                process_filter_string(&mut compiler, input);
            }
            Err(ReadlineError::Interrupted) => {
                println!("接收到 Ctrl-C，正在退出...");
                break;
            }
            Err(ReadlineError::Eof) => {
                println!("接收到 Ctrl-D，正在退出...");
                break;
            }
            Err(err) => {
                println!("发生错误: {:?}", err);
                break;
            }
        }
    }

    Ok(())
}