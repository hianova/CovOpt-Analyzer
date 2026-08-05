use CovOpt_Analyzer::model::{SampleKey, ScopeId, TraceId};
use CovOpt_Analyzer::repair::{SourceEdit, apply_edits_transactionally};
use CovOpt_Analyzer::trace::{Trace, TraceEvent, TraceEventKind};
use std::process::Command;

#[test]
fn temporal_verify_consumes_runtime_trace_ir_end_to_end() {
    let temp = tempfile::tempdir().unwrap();
    let trace_path = temp.path().join("trace.json");
    let mut trace = Trace::new(
        TraceId::new("cli-runtime-trace"),
        SampleKey {
            seed: Some(7),
            ..Default::default()
        },
    );
    trace.push(TraceEvent {
        trace: trace.id.clone(),
        sequence: 0,
        thread: "main".to_string(),
        logical_time: 0,
        scope: Some(ScopeId::new("worker")),
        kind: TraceEventKind::Return,
        operation: "completed".to_string(),
        observed_value: None,
        ordering: None,
        source: None,
    });
    std::fs::write(&trace_path, trace.deterministic_bytes().unwrap()).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_covopt"))
        .args([
            "verify",
            "temporal",
            "--target",
            "synthetic-target",
            "--operator",
            "eventually",
            "--event",
            "completed",
            "--fairness",
            "bounded scheduler fairness",
            "--trace",
            trace_path.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(document["trace_origin"], "runtime-trace");
    assert_eq!(document["result"]["status"], "Observed");
}

#[test]
fn temporal_verify_rejects_a_runtime_counterexample() {
    let temp = tempfile::tempdir().unwrap();
    let trace_path = temp.path().join("trace.json");
    let trace = Trace::new(TraceId::new("missing-event"), SampleKey::default());
    std::fs::write(&trace_path, trace.deterministic_bytes().unwrap()).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_covopt"))
        .args([
            "verify",
            "temporal",
            "--target",
            "synthetic-target",
            "--operator",
            "eventually",
            "--event",
            "never-observed",
            "--fairness",
            "bounded scheduler fairness",
            "--trace",
            trace_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("violation"));
}

#[test]
fn fix_rollback_restores_a_committed_transaction_end_to_end() {
    let workspace = tempfile::tempdir().unwrap();
    let source_dir = workspace.path().join("src");
    std::fs::create_dir_all(&source_dir).unwrap();
    let source_path = source_dir.join("lib.rs");
    let original = "pub fn answer() -> usize { 1 }\n";
    std::fs::write(&source_path, original).unwrap();
    let column = original.find('1').unwrap();
    let edit =
        SourceEdit::from_source("src/lib.rs", original, 1, column, 1, column + 1, "2").unwrap();
    let transaction = apply_edits_transactionally(workspace.path(), &[edit]).unwrap();
    assert!(std::fs::read_to_string(&source_path).unwrap().contains("2"));

    let output = Command::new(env!("CARGO_BIN_EXE_covopt"))
        .args(["fix", "--rollback", &transaction.manifest_path, "--json"])
        .current_dir(workspace.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read_to_string(source_path).unwrap(), original);
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(document["status"], "rolled-back");
}

#[test]
fn init_only_persists_policy_and_preserves_project_files() {
    let workspace = tempfile::tempdir().unwrap();
    let cargo = "[package]\nname = \"init-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
    let ignore = "local-only\n";
    std::fs::write(workspace.path().join("Cargo.toml"), cargo).unwrap();
    std::fs::write(workspace.path().join(".gitignore"), ignore).unwrap();
    std::fs::create_dir_all(workspace.path().join(".agents")).unwrap();
    std::fs::write(workspace.path().join(".agents/keep.md"), "owned by user\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_covopt"))
        .args(["init", "--yes"])
        .current_dir(workspace.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let config = std::fs::read_to_string(workspace.path().join(".covopt.toml")).unwrap();
    assert!(config.contains("version = 3"));
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("Cargo.toml")).unwrap(),
        cargo
    );
    assert_eq!(
        std::fs::read_to_string(workspace.path().join(".gitignore")).unwrap(),
        ignore
    );
    assert_eq!(
        std::fs::read_to_string(workspace.path().join(".agents/keep.md")).unwrap(),
        "owned by user\n"
    );
    assert!(!workspace.path().join(".agents/rules").exists());

    std::fs::write(workspace.path().join(".covopt.toml"), "user policy\n").unwrap();
    let second = Command::new(env!("CARGO_BIN_EXE_covopt"))
        .args(["init", "--yes"])
        .current_dir(workspace.path())
        .output()
        .unwrap();
    assert!(second.status.success());
    assert_eq!(
        std::fs::read_to_string(workspace.path().join(".covopt.toml")).unwrap(),
        "user policy\n"
    );
}

#[test]
fn inspect_config_works_without_init() {
    let workspace = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_covopt"))
        .args(["inspect", "--config"])
        .current_dir(workspace.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(document["version"], 3);
}

#[test]
fn check_plan_discovers_annotations_without_init() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(workspace.path().join("tests")).unwrap();
    std::fs::write(
        workspace.path().join("Cargo.toml"),
        "[package]\nname = \"embedded-check\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::write(
        workspace.path().join("tests/synthetic.rs"),
        r#"
            #[covopt_macro::covopt_test(target_fn = "work", expected = "ON", n_values = "1,2")]
            fn synthetic(n: usize) { let _ = n; }
        "#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_covopt"))
        .args([
            "check",
            "--plan",
            "--target",
            "synthetic",
            "--format",
            "json",
            "--budget",
            "30s",
        ])
        .current_dir(workspace.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!workspace.path().join(".covopt.toml").exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("embedded policy"));
}

#[cfg(unix)]
#[test]
fn init_hook_updates_one_managed_block_and_preserves_existing_hook() {
    let workspace = tempfile::tempdir().unwrap();
    let hooks = workspace.path().join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let hook = hooks.join("pre-commit");
    std::fs::write(&hook, "#!/bin/sh\necho existing-hook\n").unwrap();

    for _ in 0..2 {
        let output = Command::new(env!("CARGO_BIN_EXE_covopt"))
            .args(["init", "--hook"])
            .current_dir(workspace.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let content = std::fs::read_to_string(&hook).unwrap();
    assert!(content.contains("echo existing-hook"));
    assert!(content.contains("covopt check --staged --fast"));
    assert_eq!(content.matches("# >>> covopt pre-commit >>>").count(), 1);
    assert!(
        Command::new("sh")
            .args(["-n", hook.to_str().unwrap()])
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn converge_applies_verified_layout_repairs_and_persists_one_decision_bundle() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(workspace.path().join("src")).unwrap();
    std::fs::write(
        workspace.path().join("Cargo.toml"),
        "[package]\nname = \"converge-layout\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::write(
        workspace.path().join("src/lib.rs"),
        "use std::sync::atomic::AtomicUsize;\nstruct Counters { cold: u8, count: AtomicUsize }\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_covopt"))
        .args(["converge", "--budget", "60s", "--format", "json"])
        .current_dir(workspace.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let bundle: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(bundle["goal"]["authority"], "apply");
    assert_eq!(bundle["status"], "converged");
    assert!(!bundle["selected"].as_array().unwrap().is_empty());
    assert!(
        bundle["transactions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|transaction| transaction["status"] == "committed")
    );
    let updated = std::fs::read_to_string(workspace.path().join("src/lib.rs")).unwrap();
    assert!(updated.contains("repr (align (64))") || updated.contains("repr(align(64))"));
    assert!(
        workspace
            .path()
            .join("target/covopt/decision-bundle.json")
            .is_file()
    );
    assert!(!workspace.path().join(".covopt.toml").exists());
    assert!(!workspace.path().join(".agents").exists());
}

#[test]
fn converge_unknown_evaluator_fails_closed_without_touching_source() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(workspace.path().join("src")).unwrap();
    std::fs::write(
        workspace.path().join("Cargo.toml"),
        "[package]\nname = \"converge-unknown\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    let original = "pub fn value() -> usize { 1 }\n";
    std::fs::write(workspace.path().join("src/lib.rs"), original).unwrap();
    std::fs::write(
        workspace.path().join("goal.json"),
        r#"{
          "objectives": [{
            "id": "future-objective",
            "metric": { "id": "unknown.metric.v9" }
          }]
        }"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_covopt"))
        .args([
            "converge",
            "--spec",
            "goal.json",
            "--budget",
            "10s",
            "--format",
            "json",
        ])
        .current_dir(workspace.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let bundle: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(bundle["status"], "incomplete");
    assert!(bundle["unresolved"].as_array().unwrap().iter().any(|item| {
        item["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("unknown evaluator"))
    }));
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("src/lib.rs")).unwrap(),
        original
    );
}
