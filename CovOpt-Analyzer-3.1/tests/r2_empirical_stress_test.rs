#![allow(dead_code)]

#[path = "../src/auto_fixer.rs"]
mod auto_fixer;

use auto_fixer::run_async_starvation_shield;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn compile_snippet(crate_name: &str, content: &str) -> (bool, String) {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_dir = temp_dir.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let cargo_toml = format!(
        r#"
[package]
name = "{crate_name}"
version = "0.1.0"
edition = "2024"

[dependencies]
tokio = {{ version = "1.43", features = ["full"] }}
"#
    );

    fs::write(temp_dir.path().join("Cargo.toml"), cargo_toml).unwrap();
    fs::write(src_dir.join("lib.rs"), content).unwrap();

    let output = Command::new("cargo")
        .arg("check")
        .arg("--manifest-path")
        .arg(temp_dir.path().join("Cargo.toml"))
        .output()
        .expect("Failed to execute cargo check");

    let is_success = output.status.success();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (is_success, stderr)
}

#[test]
fn test_empirical_compilation_verification() {
    let temp_dir = tempfile::tempdir().unwrap();

    // 1. File A: Standard Async constructs (async fn, async block, async closure, thread::spawn, spawn_blocking, ?)
    let valid_scenarios = r#"
use std::fs;
use std::thread;
use std::time::Duration;

pub fn sync_function() {
    thread::sleep(Duration::from_millis(1));
    let _ = fs::read_to_string("test.txt");
}

pub async fn async_function() {
    thread::sleep(Duration::from_millis(1));
}

pub fn function_with_async_block() {
    let _ = async {
        thread::sleep(Duration::from_millis(1));
    };
}

pub fn function_with_async_closure() {
    let closure = async || {
        thread::sleep(Duration::from_millis(1));
    };
    let _ = closure;
}

pub async fn async_fn_with_thread_spawn() {
    thread::spawn(|| {
        thread::sleep(Duration::from_millis(1));
    });
}

pub async fn async_fn_with_spawn_blocking() {
    tokio::task::spawn_blocking(move || {
        thread::sleep(Duration::from_millis(1));
    });
}

pub async fn async_fn_with_question_mark() -> Result<String, std::io::Error> {
    let content = fs::read_to_string("dummy.txt")?;
    Ok(content)
}
"#;

    let file_a = temp_dir.path().join("valid_taint.rs");
    fs::write(&file_a, valid_scenarios).unwrap();
    let count_a = run_async_starvation_shield(&file_a).unwrap();
    let code_a = fs::read_to_string(&file_a).unwrap();
    let (ok_a, err_a) = compile_snippet("test_a", &code_a);
    println!("=== FILE A (Standard Async Rewrites) ===");
    println!("Rewrites: {}", count_a);
    println!("Cargo Check Success: {}", ok_a);
    if !ok_a {
        println!("Compiler Output:\n{}", err_a);
    }

    // 2. File B1: Sync closure inside async fn
    let sync_closure_bug = r#"
use std::thread;
use std::time::Duration;

pub async fn async_fn_with_sync_closure() {
    let vec = vec![1, 2, 3];
    let _res: Vec<_> = vec.into_iter().map(|_| {
        thread::sleep(Duration::from_millis(1));
        1
    }).collect();
}
"#;
    let file_b1 = temp_dir.path().join("sync_closure_bug.rs");
    fs::write(&file_b1, sync_closure_bug).unwrap();
    let count_b1 = run_async_starvation_shield(&file_b1).unwrap();
    let code_b1 = fs::read_to_string(&file_b1).unwrap();
    let (ok_b1, err_b1) = compile_snippet("test_b1", &code_b1);
    println!("\n=== FILE B1 (Sync closure inside async fn) ===");
    println!("Rewrites: {}", count_b1);
    println!("Cargo Check Success: {}", ok_b1);
    if !ok_b1 {
        println!("Compiler Output:\n{}", err_b1);
    }

    // 3. File B2: Mutex lock returning MutexGuard
    let mutex_bug = r#"
use std::sync::{Arc, Mutex};

pub async fn async_fn_with_mutex_lock(mtx: Arc<Mutex<i32>>) -> i32 {
    let val = *mtx.lock().unwrap();
    val
}
"#;
    let file_b2 = temp_dir.path().join("mutex_bug.rs");
    fs::write(&file_b2, mutex_bug).unwrap();
    let count_b2 = run_async_starvation_shield(&file_b2).unwrap();
    let code_b2 = fs::read_to_string(&file_b2).unwrap();
    let (ok_b2, err_b2) = compile_snippet("test_b2", &code_b2);
    println!("\n=== FILE B2 (Mutex lock returning MutexGuard) ===");
    println!("Rewrites: {}", count_b2);
    println!("Cargo Check Success: {}", ok_b2);
    if !ok_b2 {
        println!("Compiler Output:\n{}", err_b2);
    }

    // 4. File C: dummy_async_shield.rs fixture
    let mut dummy_path = PathBuf::from("tests/dummy_async_shield.rs");
    if !dummy_path.exists() {
        dummy_path = PathBuf::from("CovOpt-Analyzer/tests/dummy_async_shield.rs");
    }
    let file_c = temp_dir.path().join("dummy_async_shield_test.rs");
    fs::copy(&dummy_path, &file_c).unwrap();
    let count_c = run_async_starvation_shield(&file_c).unwrap();
    let code_c = fs::read_to_string(&file_c).unwrap();
    let (ok_c, err_c) = compile_snippet("test_c", &code_c);
    println!("\n=== FILE C (dummy_async_shield.rs fixture) ===");
    println!("Rewrites: {}", count_c);
    println!("Cargo Check Success: {}", ok_c);
    if !ok_c {
        println!("Compiler Output:\n{}", err_c);
    }

    assert!(ok_a, "FILE A compilation failed:\n{}", err_a);
    assert!(ok_b1, "FILE B1 compilation failed:\n{}", err_b1);
    assert!(ok_b2, "FILE B2 compilation failed:\n{}", err_b2);
    assert!(ok_c, "FILE C compilation failed:\n{}", err_c);
}
