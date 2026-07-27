use CovOpt_Analyzer::runner::check_workspace;
use std::fs;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn test_check_workspace_succeeds_on_valid_workspace() {
    let result = check_workspace();
    assert!(
        result.is_ok(),
        "Expected check_workspace() to succeed on healthy workspace, got: {:?}",
        result.err()
    );
}

#[test]
fn test_check_workspace_fails_on_compilation_error() {
    let temp_dir = tempdir().unwrap();
    let cargo_toml = temp_dir.path().join("Cargo.toml");
    let src_dir = temp_dir.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();

    fs::write(
        &cargo_toml,
        r#"[package]
name = "broken_crate"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();

    fs::write(src_dir.join("lib.rs"), "fn broken_syntax_error ( { ").unwrap();

    let output = Command::new("cargo")
        .current_dir(temp_dir.path())
        .args([
            "check",
            "--workspace",
            "--all-targets",
            "--message-format=json",
        ])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "Expected cargo check --workspace to fail on invalid syntax"
    );
}
