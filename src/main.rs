pub mod ast;
pub mod token;
pub mod parser;
pub mod lexer;
pub mod sql_compiler;

use lexer::Lexer;
use parser::Parser;
use sql_compiler::SqlCompiler;
use std::collections::HashMap;

fn main() {
    println!("--- Report Dispatcher: Filter to SQL Compiler ---");

    // 1. A sample filter string based on the DSL.
    let filter_string = r#"Filter: status["Open"]; priority[>2]; CrossFilter: <Test-Run> status["PASS"]"#;
    println!("\n[Input DSL]:\n{}\n", filter_string);

    // 2. Lexer - tokenize the DSL
    println!("[Step 1]: Tokenizing the DSL...");
    let tokens: Vec<_> = Lexer::new(filter_string).collect();
    println!("Generated {} tokens", tokens.len());
    
    // 3. Parser - build AST from tokens
    println!("\n[Step 2]: Parsing tokens into AST...");
    let mut parser = Parser::new(&tokens);
    match parser.parse() {
        Ok(ast) => {
            println!("Successfully parsed AST:");
            println!("  - Base filters: {}", ast.base_filters.len());
            println!("  - Cross filters: {}", ast.cross_filters.len());
            
            // 4. SQL Compiler - generate optimized SQL
            println!("\n[Step 3]: Compiling AST to SQL...");
            let mut compiler = SqlCompiler::new();
            
            // Set up table mapping for demonstration
            let mut table_mapping = HashMap::new();
            table_mapping.insert("Test".to_string(), "tests".to_string());
            table_mapping.insert("Run".to_string(), "test_runs".to_string());
            compiler.set_table_mapping(table_mapping);
            
            match compiler.compile(ast) {
                Ok(result) => {
                    println!("✅ Successfully generated SQL:");
                    println!("\n[Generated SQL]:");
                    println!("{}\n", result.sql);
                    
                    if !result.optimizations.is_empty() {
                        println!("[Applied Optimizations]:");
                        for (i, opt) in result.optimizations.iter().enumerate() {
                            println!("  {}. {:?}", i + 1, opt);
                        }
                    } else {
                        println!("[No optimizations applied]");
                    }
                    
                    println!("\n--- 🎉 Compilation Complete! ---");
                }
                Err(e) => {
                    println!("❌ SQL compilation failed: {}", e.message);
                }
            }
        }
        Err(e) => {
            println!("❌ Parsing failed: {}", e.message);
            if let Some(span) = e.span {
                println!("   Error location: characters {}..{}", span.start, span.end);
            }
        }
    }
    
    // Demonstrate different optimization scenarios
    demonstrate_optimizations();
    
    // Test the full DSL specification example
    test_full_specification_example();
}

fn test_full_specification_example() {
    println!("\n--- Testing Full DSL Specification Example ---");
    
    let spec_example = r#"Filter: title["Release Plan" AND ("Version 1" OR "Version 2")];dueDate[>today];assignee[!=current_user];CrossFilter: <Test-Run>run-id[1]"#;
    println!("\n[Specification Example]:");
    println!("{}", spec_example);
    
    println!("\n[Testing Lexer]:");
    let tokens: Vec<_> = Lexer::new(spec_example).collect();
    println!("Generated {} tokens", tokens.len());
    for (i, token) in tokens.iter().enumerate() {
        println!("  {}: {:?}", i, token);
    }
    
    println!("\n[Testing Parser]:");
    let mut parser = Parser::new(&tokens);
    match parser.parse() {
        Ok(ast) => {
            println!("✅ Parser succeeded!");
            println!("Base filters: {}", ast.base_filters.len());
            for (i, filter) in ast.base_filters.iter().enumerate() {
                println!("  Filter {}: {} = {:?}", i + 1, filter.field.0, filter.condition);
            }
            
            println!("Cross filters: {}", ast.cross_filters.len());
            for (i, cross_filter) in ast.cross_filters.iter().enumerate() {
                println!("  CrossFilter {}: {}-{}", i + 1, cross_filter.source_entity.0, cross_filter.target_entity.0);
                for (j, filter) in cross_filter.filters.iter().enumerate() {
                    println!("    Field {}: {} = {:?}", j + 1, filter.field.0, filter.condition);
                }
            }
            
            println!("\n[Testing SQL Compiler]:");
            let mut compiler = SqlCompiler::new();
            let mut table_mapping = HashMap::new();
            table_mapping.insert("Test".to_string(), "tests".to_string());
            table_mapping.insert("Run".to_string(), "test_runs".to_string());
            compiler.set_table_mapping(table_mapping);
            
            match compiler.compile(ast) {
                Ok(result) => {
                    println!("✅ SQL Compiler succeeded!");
                    println!("Generated SQL: {}", result.sql);
                    if !result.optimizations.is_empty() {
                        println!("Optimizations: {:?}", result.optimizations);
                    }
                }
                Err(e) => {
                    println!("❌ SQL Compiler failed: {}", e.message);
                }
            }
        }
        Err(e) => {
            println!("❌ Parser failed: {}", e.message);
            if let Some(span) = e.span {
                println!("Error location: characters {}..{}", span.start, span.end);
            }
        }
    }
}

fn demonstrate_optimizations() {
    println!("\n--- Optimization Demonstrations ---");
    
    // Example 1: OR to IN optimization
    println!("\n[Demo 1]: OR to IN optimization");
    let or_heavy_dsl = r#"Filter: status["Open" OR "Pending" OR "Review" OR "Approved" OR "Testing"]"#;
    compile_and_show_optimizations("OR-heavy query", or_heavy_dsl);
    
    // Example 2: Complex nested conditions
    println!("\n[Demo 2]: Complex nested conditions with Cross Filter");
    let complex_dsl = r#"Filter: priority[>3]; assignee[!=current_user]; CrossFilter: <Project-Task> status["Active"]"#;
    compile_and_show_optimizations("Complex query", complex_dsl);
    
    // Example 3: Date handling
    println!("\n[Demo 3]: Date keyword handling");
    let date_dsl = r#"Filter: created[>today]; updated[<=yesterday]"#;
    compile_and_show_optimizations("Date query", date_dsl);
}

fn compile_and_show_optimizations(_description: &str, dsl: &str) {
    println!("DSL: {}", dsl);
    
    let tokens: Vec<_> = Lexer::new(dsl).collect();
    let mut parser = Parser::new(&tokens);
    
    if let Ok(ast) = parser.parse() {
        let mut compiler = SqlCompiler::new();
        let mut table_mapping = HashMap::new();
        table_mapping.insert("Project".to_string(), "projects".to_string());
        table_mapping.insert("Task".to_string(), "tasks".to_string());
        table_mapping.insert("Test".to_string(), "tests".to_string());
        table_mapping.insert("Run".to_string(), "test_runs".to_string());
        compiler.set_table_mapping(table_mapping);
        
        if let Ok(result) = compiler.compile(ast) {
            println!("SQL: {}", result.sql);
            if !result.optimizations.is_empty() {
                println!("Optimizations: {:?}", result.optimizations);
            } else {
                println!("No optimizations applied");
            }
        } else {
            println!("Compilation failed");
        }
    } else {
        println!("Parsing failed");
    }
}
