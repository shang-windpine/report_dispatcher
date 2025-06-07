use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use std::hint::black_box;
use report_dispatcher::lexer::Lexer;
use report_dispatcher::parser::Parser;
use report_dispatcher::sql_compiler::{SqlCompiler, BatchConfig, QueryCompiler, BatchQueryCompiler, TableMappingProvider};
use std::collections::HashMap;

// 创建一个编译器实例并设置表映射
fn create_compiler() -> SqlCompiler {
    let mut compiler = SqlCompiler::new();
    let mut table_mapping = HashMap::new();
    table_mapping.insert("Test".to_string(), "tests".to_string());
    table_mapping.insert("Run".to_string(), "test_runs".to_string());
    table_mapping.insert("Project".to_string(), "projects".to_string());
    table_mapping.insert("Task".to_string(), "tasks".to_string());
    compiler.table_mapper_mut().set_table_mapping(table_mapping);
    compiler
}

// 基准测试：词法分析性能
fn benchmark_lexer(c: &mut Criterion) {
    let test_cases = vec![
        ("simple", r#"Filter: status["Open"]"#),
        ("medium", r#"Filter: status["Open"]; priority[>2]; assignee[!=current_user]"#),
        ("complex", r#"Filter: title["Release Plan" AND ("Version 1" OR "Version 2")];dueDate[>today];assignee[!=current_user];CrossFilter: <Test-Run>run-id[1]"#),
    ];

    let mut group = c.benchmark_group("lexer_performance");
    
    for (name, dsl) in test_cases {
        group.bench_with_input(BenchmarkId::new("tokenize", name), &dsl, |b, &dsl| {
            b.iter(|| {
                let tokens: Vec<_> = Lexer::new(black_box(dsl)).collect();
                black_box(tokens)
            })
        });
    }
    
    group.finish();
}

// 基准测试：语法分析性能
fn benchmark_parser(c: &mut Criterion) {
    let test_cases = vec![
        ("simple", r#"Filter: status["Open"]"#),
        ("medium", r#"Filter: status["Open"]; priority[>2]; assignee[!=current_user]"#),
        ("complex", r#"Filter: title["Release Plan" AND ("Version 1" OR "Version 2")];dueDate[>today];assignee[!=current_user];CrossFilter: <Test-Run>run-id[1]"#),
    ];

    let mut group = c.benchmark_group("parser_performance");
    
    for (name, dsl) in test_cases {
        // 预先词法分析
        let tokens: Vec<_> = Lexer::new(dsl).collect();
        
        group.bench_with_input(BenchmarkId::new("parse", name), &tokens, |b, tokens| {
            b.iter(|| {
                let mut parser = Parser::new(black_box(tokens));
                match parser.parse() {
                    Ok(ast) => black_box(ast),
                    Err(_) => panic!("解析失败"),
                }
            })
        });
    }
    
    group.finish();
}

// 基准测试：SQL编译性能
fn benchmark_sql_compiler(c: &mut Criterion) {
    let test_cases = vec![
        ("simple", r#"Filter: status["Open"]"#),
        ("medium", r#"Filter: status["Open"]; priority[>2]; assignee[!=current_user]"#),
        ("complex", r#"Filter: title["Release Plan" AND ("Version 1" OR "Version 2")];dueDate[>today];assignee[!=current_user];CrossFilter: <Test-Run>run-id[1]"#),
        ("or_optimization", r#"Filter: status["Open" OR "Pending" OR "Review" OR "Approved" OR "Testing"]"#),
    ];

    let mut group = c.benchmark_group("sql_compiler_performance");
    
    for (name, dsl) in test_cases {
        // 预处理：词法分析和语法分析
        let tokens: Vec<_> = Lexer::new(dsl).collect();
        let mut parser = Parser::new(&tokens);
        let ast = parser.parse().expect("解析应该成功");
        
        group.bench_with_input(BenchmarkId::new("compile", name), &ast, |b, ast| {
            b.iter(|| {
                let compiler = create_compiler();
                match compiler.compile(black_box(ast.clone()), "Task") {
                    Ok(result) => black_box(result),
                    Err(_) => panic!("编译失败"),
                }
            })
        });
    }
    
    group.finish();
}

// 基准测试：完整的端到端处理
fn benchmark_end_to_end(c: &mut Criterion) {
    let test_cases = vec![
        ("simple", r#"Filter: status["Open"]"#),
        ("medium", r#"Filter: status["Open"]; priority[>2]; assignee[!=current_user]"#),
        ("complex", r#"Filter: title["Release Plan" AND ("Version 1" OR "Version 2")];dueDate[>today];assignee[!=current_user];CrossFilter: <Test-Run>run-id[1]"#),
    ];

    let mut group = c.benchmark_group("end_to_end_performance");
    
    for (name, dsl) in test_cases {
        group.bench_with_input(BenchmarkId::new("full_pipeline", name), &dsl, |b, &dsl| {
            b.iter(|| {
                // 完整的处理流程
                let tokens: Vec<_> = Lexer::new(black_box(dsl)).collect();
                let mut parser = Parser::new(&tokens);
                let ast = parser.parse().expect("解析应该成功");
                let compiler = create_compiler();
                let result = compiler.compile(ast, "Task").expect("编译应该成功");
                black_box(result)
            })
        });
    }
    
    group.finish();
}

// 基准测试：批量查询编译
fn benchmark_batch_compilation(c: &mut Criterion) {
    let dsl = r#"Filter: status["Open"]; priority[>2]"#;
    
    // 预处理
    let tokens: Vec<_> = Lexer::new(dsl).collect();
    let mut parser = Parser::new(&tokens);
    let ast = parser.parse().expect("解析应该成功");
    
    let batch_configs = vec![
        ("small_batch", BatchConfig { max_batch_size: 100, enable_batch_processing: true }),
        ("medium_batch", BatchConfig { max_batch_size: 500, enable_batch_processing: true }),
        ("large_batch", BatchConfig { max_batch_size: 1000, enable_batch_processing: true }),
    ];

    let mut group = c.benchmark_group("batch_compilation");
    
    for (name, config) in batch_configs {
        group.bench_with_input(BenchmarkId::new("compile_batch", name), &config, |b, config| {
            b.iter(|| {
                let compiler = create_compiler();
                match compiler.batch_processor().compile_batch(black_box(ast.clone()), "Task", black_box(config)) {
                    Ok(result) => black_box(result),
                    Err(_) => panic!("批量编译失败"),
                }
            })
        });
    }
    
    group.finish();
}

criterion_group!(
    benches,
    benchmark_lexer,
    benchmark_parser,
    benchmark_sql_compiler,
    benchmark_end_to_end,
    benchmark_batch_compilation
);
criterion_main!(benches); 