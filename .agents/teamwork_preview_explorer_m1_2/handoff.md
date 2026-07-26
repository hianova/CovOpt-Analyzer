# Handoff Report — Explorer 2 (Milestone 2: R3 Strict Workspace Audit)

## 1. Observation

### Key Code Locations & Verbatim Snippets

1. **`covopt_cli/src/ci.rs` (Lines 24–33)**
   ```rust
   if config.pipeline.run_audit {
       println!("▶️ Step 2: Running `covopt audit`...");
       commands::run_audit(&covopt_core::config::AuditArgs {
           test: None,
           fast: args.fast,
           json: false,
           staged: args.base.is_some(),
       });
       println!("✅ [CI OK] Audit passed.");
   }
   ```
   *Observation*: `ci::run_pipeline()` executes `commands::run_audit()` and immediately prints `println!("✅ [CI OK] Audit passed.");` without checking return values or process exit state, because `run_audit()` returns `()`.

2. **`covopt_cli/src/commands.rs` (Lines 1135–1194)**
   ```rust
   if entropy_result.total_score > covopt_param!("M_1079_40", 50.0) {
       eprintln!(
           "  [!] WARNING: High Entropy Detected! Codebase is unstable, tangled, or noisy."
       );
       all_success = false;
   } else {
       eprintln!("  [OK] Low Entropy. Code is well encapsulated and stable.");
   }
   ...
   if !all_success {
       eprintln!("\n[AUDIT FAILED] One or more targets failed complexity or coverage checks.");
       std::process::exit(1);
   } else {
       eprintln!("\n[AUDIT PASSED] All targets passed complexity and coverage checks.");
   }
   ```
   *Observation*: `run_audit()` only sets `all_success = false` if `entropy_result.total_score > 50.0` or performance/complexity analysis fails.

3. **`covopt_core/src/entropy.rs` (Lines 35–57)**
   ```rust
   fn compute_cli_noise(details: &mut String) -> f64 {
       let _ = writeln!(details, "  -> Calculating CLI Noise Index (C)...");
       let output = Command::new("cargo")
           .args(["check", "--message-format=json"])
           .output();

       let mut warning_count = 0;

       if let Ok(output) = output {
           let stdout = String::from_utf8_lossy(&output.stdout);
           for line in stdout.lines() {
               if let Ok(v) = serde_json::from_str::<serde_json::Value>(line)
                   && let Some(msg) = v.get("message")
                   && let Some(level) = msg.get("level").and_then(|l| l.as_str())
               {
                   if level == "warning" {
                       warning_count += 1;
                   } else if level == "error" || level == "error: internal compiler error" {
                       warning_count += covopt_param!("M_52_33", 5); // Heavily penalize errors/ICE
                   }
               }
           }
       }

       let score =
           (warning_count as f64 * covopt_param!("M_58_40", 2.0)).min(covopt_param!("M_58_49", 30.0));
       ...
       score
   }
   ```
   *Observation*: `compute_cli_noise()` runs `cargo check` (without `--workspace` or `--all-targets`), ignores `output.status.success()`, and maps errors to warning score points capped at `30.0`.

---

## 2. Logic Chain

1. **Observation 1 & 3**: `compute_cli_noise` in `covopt_core/src/entropy.rs` runs `cargo check --message-format=json`. It does not check process exit status (`output.status.success()`), nor does it run with `--workspace` or `--all-targets`.
2. **Observation 3**: Compiler errors in `cargo check` only increment `warning_count` by 5 points. The maximum CLI noise score contribution is capped at `30.0` points (via `min(30.0)`).
3. **Observation 2**: Total entropy score is calculated as `fuzz_variance + branch_sprawl + cli_noise`. If a compilation error occurs (`cli_noise_score = 10.0`), and other components are `0.0`, total entropy is `10.0`.
4. **Observation 2**: Since `10.0 <= 50.0`, `commands::run_audit` evaluates the check as `[OK] Low Entropy` and keeps `all_success = true`.
5. **Observation 1 & 2**: `commands::run_audit` prints `[AUDIT PASSED]` and returns normally to `ci::run_pipeline`.
6. **Observation 1**: `ci::run_pipeline` prints `✅ [CI OK] Audit passed.` and exits with code `0`.
7. **Conclusion**: `covopt ci` falsely reports success on workspace compilation failure because `cargo check --workspace` is never executed and compilation exit status is never validated.

---

## 3. Caveats

- **No Caveats**: The codebase was thoroughly searched using `grep_search` and `find_by_name`. The call hierarchy for `covopt ci` -> `covopt audit` -> `compute_cli_noise` is completely deterministic.

---

## 4. Conclusion

### Actionable Implementation Plan for Implementer Agent:
1. **Add `check_workspace()`** in `covopt_core/src/runner.rs`:
   Execute `cargo check --workspace --all-targets --message-format=json`. If `!output.status.success()`, parse errors and return `Err(String)`.
2. **Enforce `check_workspace()` in `covopt_cli/src/commands.rs`**:
   In `run_audit()`, call `covopt_core::runner::check_workspace()`. If it returns `Err(e)`, print failure message and call `std::process::exit(1)`.
3. **Update `compute_cli_noise()` in `covopt_core/src/entropy.rs`**:
   Update `Command::new("cargo")` arguments to `["check", "--workspace", "--all-targets", "--message-format=json"]`.
4. **Add Unit & Integration Tests**:
   - Unit test in `covopt_core/src/runner.rs` validating `check_workspace()`.
   - Integration test verifying `covopt ci` non-zero exit code on workspace compilation failure.

---

## 5. Verification Method

### Verification Commands
To verify baseline health:
```bash
rtk cargo check --workspace --all-targets
rtk cargo test --workspace
```

To verify R3 strict audit behavior after implementation:
1. Inject a intentional syntax error into a workspace crate (e.g., `covopt_core/src/lib.rs`).
2. Run `rtk cargo run --bin covopt -- ci --fast`.
3. Confirm command fails immediately during Step 2 (`covopt audit`), outputs non-zero exit code (1), and does NOT print `[CI OK] Audit passed.`.
