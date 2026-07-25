# Milestone 1: Core Engine, CLI Robustness & Cleanup Handoff Report

## 1. Observation

All 8 tasks for Milestone 1 have been successfully implemented and verified across the workspace (`covopt_core`, `covopt_cli`, `covopt-macro`):

### 1. Clippy Cleanliness & Attribute Removal (Task 2)
- **Files**: `covopt_core/src/dummy_heuristics.rs`, `covopt_core/src/sandbox.rs`, `covopt_cli/src/commands.rs`, `covopt_core/src/asm_extractor.rs`, `covopt_core/src/profiler.rs`, `covopt_core/src/entropy.rs`.
- **Changes**: Removed all `#![allow(...)]` and `#[allow(...)]` attributes. Resolved all 17 initial Clippy warnings (`clippy::useless_format`, `clippy::extra_unused_type_parameters`, `clippy::collapsible_if`, `clippy::lines_filter_map_ok`, `clippy::unnecessary_map_or`, `clippy::manual_strip`).
- **Result**: `rtk cargo clippy --workspace --all-targets -- -D warnings` completes with **0 errors and 0 warnings**.

### 2. Scanner Isolation (Task 3)
- **File**: `covopt_core/src/scanner.rs`
- **Changes**: Modified `collect_rs_files` to exclude `covopt-macro`, `covopt_macro`, `proc-macro`, and `proc_macro` directories and file paths from being collected for magic number scanning/replacement.
- **Result**: `covopt-macro/src/lib.rs` is protected from destructive macro injection during `covopt fix` and `covopt ci`.

### 3. CLI Subcommand Enhancements & Fixes (Task 4)
- **File**: `covopt_cli/src/commands.rs`, `covopt_core/src/asm_extractor.rs`
- **Changes**:
  - `init_config`: Added `std::io::stdout().is_terminal()` and `COVOPT_NON_INTERACTIVE` / `CI` checks before reading stdin so non-interactive CI environments default safely without hanging.
  - `cargo clippy --fix`: Removed invalid `--` argument formatting when passing path arguments.
  - `run_advise`: Added virtual workspace root fallback (scans member crate `src/` directories if root `src/` does not exist), added package resolution for `cargo rustc --package <pkg>`, and removed public function exclusion filter so public functions are analyzed.

### 4. Core Runner macOS Dyld Fix (Task 5)
- **File**: `covopt_core/src/runner.rs`
- **Changes**: Updated `compile_workspace_tests` JSON compiler-artifact filtering to exclude proc-macro test binaries (which fail on macOS with dyld `LC_RPATH` missing errors when executed directly).

### 5. Core Profiler Command Check Fix (Task 6)
- **File**: `covopt_core/src/profiler.rs`
- **Changes**: Replaced `cargo-flamegraph` check with `check_flamegraph_exists()`, which checks `flamegraph` binary as well as `cargo flamegraph` subcommand execution.

### 6. Pre-Flight Tool Checks & Output Path Update (Task 7)
- **Files**: `covopt_cli/src/main.rs`, `covopt_cli/src/harden.rs`, `covopt_cli/src/auto_harness.rs`
- **Changes**:
  - `main.rs` & `harden.rs`: Updated pre-flight binary availability checks to evaluate `output.status.success()` instead of `.is_err()`, ensuring `--fast` mode skips missing external tools (`cargo mutants`, `cargo fuzz`) cleanly with exit code 0.
  - `auto_harness.rs`: Changed output directory from `src/fuzz/fuzz_targets` to `target/fuzz/fuzz_targets` to prevent root directory pollution.

### 7. CI Pipeline `base` Flag Support (Task 8)
- **File**: `covopt_cli/src/ci.rs`
- **Changes**: Implemented `args.base` flag support in `run_pipeline`, passing base git ref to `run_fix`, `run_scan`, and `run_audit`.

---

## 2. Logic Chain

1. **Clippy & Code Hygiene**:
   By fixing `dummy_heuristics.rs` (removing unused type parameters, converting `format!("test")` to `.to_string()`, collapsing nested `if`s, exporting public functions), `sandbox.rs` (using `.is_some_and()` and Option combinators), `commands.rs` (using `.map_while(Result::ok)` and `.into_iter().flatten()`), and `profiler.rs` / `entropy.rs` / `asm_extractor.rs`, we eliminated all 17 clippy errors and ensured 100% strict clippy compliance without resorting to `allow` suppression attributes.

2. **Proc-Macro Isolation & Build Stability**:
   `covopt-macro` defines procedural macros and does not depend on `covopt_param!`. Skipping proc-macro directories in `scanner.rs` prevents broken syntax insertion (`use covopt_macro::covopt_param;`), preserving workspace compilability after auto-fix runs.

3. **macOS Dyld Crash Prevention**:
   Proc-macro binaries compiled during `cargo test --no-run` are dynamic host plugins that lack runtime `LC_RPATH` for standalone execution on macOS. Filtering `compiler-artifact` JSON to ignore `kind == ["proc-macro"]` prevents `CargoTestRunner` from attempting to execute non-standalone proc-macro libraries, resolving macOS test crashes.

4. **Non-Interactive CI Stability**:
   Checking `stdout().is_terminal()` in `init_config` and `scanner.rs` allows CI scripts without `--yes` or TTY input to execute non-interactively without stdin blockages or hangs.

5. **Tool Detection Accuracy**:
   Cargo subcommands return exit status 101 when missing rather than OS process spawn errors. Evaluating `.status.success()` ensures accurate detection of external tools (`cargo-mutants`, `cargo-fuzz`, `flamegraph`), allowing `--fast` mode to cleanly log pre-flight skip notices and exit successfully.

---

## 3. Caveats

- External tools (`cargo-mutants`, `cargo-fuzz`, `llvm-mca`, `flamegraph`) require appropriate host toolchains if executed outside `--fast` mode. In `--fast` mode, missing tools are cleanly skipped with informative log messages.
- Stdin prompting is bypassed when `CI` or `COVOPT_NON_INTERACTIVE` environment variables are present or when stdout is not a TTY terminal.

---

## 4. Conclusion

Milestone 1 is complete and fully verified:
- **Build Status**: `rtk cargo check --workspace --all-targets` passes with 0 errors.
- **Clippy Status**: `rtk cargo clippy --workspace --all-targets -- -D warnings` passes with 0 warnings/errors.
- **Test Suite**: `rtk cargo test --workspace` passes 100% (21/21 tests passed across 6 test suites).
- **Subcommands**: `rtk ./target/debug/covopt init --yes` and `rtk ./target/debug/covopt ci --fast` complete cleanly.

---

## 5. Verification Method

To verify these results independently:

```bash
# 1. Workspace compilation
rtk cargo check --workspace --all-targets

# 2. Strict Clippy verification (0 warnings allowed)
rtk cargo clippy --workspace --all-targets -- -D warnings

# 3. Full test suite execution
rtk cargo test --workspace

# 4. CLI init non-interactive execution
rtk ./target/debug/covopt init --yes

# 5. CLI CI fast pipeline execution
rtk ./target/debug/covopt ci --fast
```
