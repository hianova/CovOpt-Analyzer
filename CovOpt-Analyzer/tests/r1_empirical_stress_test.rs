#![allow(dead_code)]

#[path = "../src/concurrency_fuzzer.rs"]
mod concurrency_fuzzer;

use concurrency_fuzzer::instrument_test_file;
use std::fs;
use std::process::Command;

fn compile_and_test_snippet(crate_name: &str, content: &str) -> (bool, bool, String) {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_dir = temp_dir.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let covopt_macro_path =
        std::path::Path::new("/Users/kuangtalin/Documents/CovOpt-Analyzer/covopt-macro")
            .canonicalize()
            .unwrap();

    let cargo_toml = format!(
        r#"
[package]
name = "{crate_name}"
version = "0.1.0"
edition = "2021"

[dependencies]
covopt-macro = {{ path = "{}" }}
"#,
        covopt_macro_path.display()
    );

    fs::write(temp_dir.path().join("Cargo.toml"), cargo_toml).unwrap();
    fs::write(src_dir.join("lib.rs"), content).unwrap();

    let input_path = src_dir.join("lib.rs");
    let instrumented_path = src_dir.join("lib.rs");

    // Instrument in-place for testing
    let _points =
        instrument_test_file(&input_path, &instrumented_path).expect("Instrumentation failed");

    let check_output = Command::new("cargo")
        .arg("check")
        .arg("--manifest-path")
        .arg(temp_dir.path().join("Cargo.toml"))
        .output()
        .expect("Failed to execute cargo check");

    let check_ok = check_output.status.success();

    let test_output = Command::new("cargo")
        .arg("test")
        .arg("--manifest-path")
        .arg(temp_dir.path().join("Cargo.toml"))
        .arg("--")
        .arg("--nocapture")
        .output()
        .expect("Failed to execute cargo test");

    let test_ok = test_output.status.success();
    let combined_log = format!(
        "CHECK STDERR:\n{}\nTEST STDERR:\n{}",
        String::from_utf8_lossy(&check_output.stderr),
        String::from_utf8_lossy(&test_output.stderr)
    );

    (check_ok, test_ok, combined_log)
}

#[test]
fn test_r1_target_attributes_and_atomic_instrumentation() {
    let temp_dir = tempfile::tempdir().unwrap();
    let input_path = temp_dir.path().join("input.rs");
    let output_path = temp_dir.path().join("output.rs");

    let code = r#"
use std::sync::atomic::{AtomicUsize, Ordering};
use covopt_macro::covopt_bench;

pub struct Counter {
    val: AtomicUsize,
}

impl Counter {
    pub fn inc(&self) -> usize {
        self.val.fetch_add(1, Ordering::SeqCst)
    }

    pub fn get(&self) -> usize {
        self.val.load(Ordering::SeqCst)
    }
}

#[covopt_bench]
pub fn bench_target_1() {
    let c = Counter { val: AtomicUsize::new(0) };
    c.inc();
}

#[covopt_test]
pub fn bench_target_2() {
    let c = Counter { val: AtomicUsize::new(0) };
    c.get();
}

#[bench]
pub fn bench_target_3() {
    let c = Counter { val: AtomicUsize::new(0) };
    c.inc();
}

#[test]
pub fn bench_target_4() {
    let c = Counter { val: AtomicUsize::new(0) };
    c.get();
}

pub fn normal_helper_function() {
    let c = Counter { val: AtomicUsize::new(0) };
    c.inc();
}
"#;

    fs::write(&input_path, code).unwrap();
    let delay_points = instrument_test_file(&input_path, &output_path).unwrap();
    let instrumented = fs::read_to_string(&output_path).unwrap();

    // 1. Verify delay points counted
    assert!(delay_points > 0, "Delay points must be > 0");

    // 2. Verify all target attributes trigger run_fuzz_loop wrapper
    assert!(
        instrumented.contains("run_fuzz_loop"),
        "Target functions must be wrapped in run_fuzz_loop"
    );

    // 3. Verify delay injection around fetch_add and load
    assert!(
        instrumented.contains("covopt_fuzzer :: spin_delay"),
        "Atomic ops must be wrapped with spin_delay"
    );
}

#[test]
fn test_r1_boundary_conditions_nested_loops_expressions_matches() {
    let temp_dir = tempfile::tempdir().unwrap();
    let input_path = temp_dir.path().join("boundary_input.rs");
    let output_path = temp_dir.path().join("boundary_output.rs");

    let boundary_code = r#"
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::hint::black_box;
use covopt_macro::covopt_bench;

pub struct ComplexAtomic {
    c1: AtomicUsize,
    c2: AtomicUsize,
    flag: AtomicBool,
}

#[covopt_bench]
pub fn test_complex_boundary() {
    let state = ComplexAtomic {
        c1: AtomicUsize::new(0),
        c2: AtomicUsize::new(0),
        flag: AtomicBool::new(true),
    };

    // Boundary 1: Nested Loops with Atomic operations
    for i in 0..black_box(2) {
        for j in 0..black_box(2) {
            let _val = black_box(state.c1.fetch_add(i + j, Ordering::SeqCst));
        }
    }

    // Boundary 2: Multiple atomic ops in single expression statement
    let combined = state.c1.fetch_add(1, Ordering::SeqCst) + state.c2.load(Ordering::SeqCst);
    let _ = black_box(combined);

    // Boundary 3: Atomic ops inside match and if control flows
    if state.flag.load(Ordering::SeqCst) {
        match state.c1.load(Ordering::SeqCst) {
            0 => { state.c2.store(10, Ordering::SeqCst); },
            _ => { state.c2.store(20, Ordering::SeqCst); },
        }
    } else {
        let _ = state.flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed);
    }
}
"#;

    fs::write(&input_path, boundary_code).unwrap();
    let points = instrument_test_file(&input_path, &output_path).unwrap();
    let instrumented = fs::read_to_string(&output_path).unwrap();

    assert!(
        points >= 14,
        "Expected at least 14 delay points across complex boundary conditions, got {}",
        points
    );
    assert!(
        instrumented.contains("covopt_fuzzer :: spin_delay"),
        "Boundary code must contain spin_delay calls"
    );
}

#[test]
fn test_r1_empirical_compilation_and_execution() {
    let dummy_code = r#"
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use covopt_macro::covopt_bench;

pub struct Counter {
    val: AtomicUsize,
}

impl Counter {
    pub fn new() -> Self {
        Self { val: AtomicUsize::new(0) }
    }
    pub fn inc(&self) -> usize {
        self.val.fetch_add(1, Ordering::SeqCst)
    }
    pub fn get(&self) -> usize {
        self.val.load(Ordering::SeqCst)
    }
}

#[covopt_bench]
pub fn dummy_atomic_bench() {
    let counter = Arc::new(Counter::new());
    let mut handles = vec![];

    for i in 0..4 {
        let c = Arc::clone(&counter);
        let i_val = black_box(i);
        handles.push(thread::spawn(move || {
            c.inc();
            let _ = black_box(c.get());
            let _ = black_box(i_val);
        }));
    }

    for handle in handles {
        let _ = handle.join();
    }
}
"#;

    let (check_ok, test_ok, logs) = compile_and_test_snippet("r1_fuzz_test_crate", dummy_code);
    println!("Cargo Check Output: {}", check_ok);
    println!("Cargo Test Output: {}", test_ok);
    if !check_ok || !test_ok {
        println!("Logs:\n{}", logs);
    }

    assert!(
        check_ok,
        "Instrumented code must compile cleanly with cargo check"
    );
    assert!(
        test_ok,
        "Instrumented code must execute cargo test successfully"
    );
}
