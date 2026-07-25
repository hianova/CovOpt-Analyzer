# BRIEFING — 2026-07-25T02:09:40Z

## Mission
Implement Milestone 1: Core Engine & CLI Robustness, Scanner Isolation, Dyld Fix, Clippy Cleaning, and CI Pipeline Fixes for CovOpt-Analyzer v2.0 upgrade.

## 🔒 My Identity
- Archetype: implementer, qa
- Roles: implementer, qa, specialist
- Working directory: /Users/kuangtalin/Documents/CovOpt-Analyzer/.agents/teamwork_preview_worker_m1
- Original parent: e73b8d90-04c0-4cf6-9c58-00afd44446a8
- Milestone: Milestone 1

## 🔒 Key Constraints
- Prefix all shell commands with `rtk`.
- ZERO-ENTROPY TUNING: Use `covopt_param!` macro where appropriate, no magical hardcoded numbers.
- ANTI-DCE: Wrap loop variables with `std::hint::black_box()` in benchmarks.
- LOCK-FREE CRITICAL PATHS: Never use std Mutex/RwLock on critical paths.
- STRICT CLIPPY CLEANLINESS: Remove all `#![allow(...)]` and `#[allow(...)]` attributes. Achieve 100% cleanliness with `rtk cargo clippy --workspace --all-targets -- -D warnings`.
- Genuine implementations only, no hardcoded test shortcuts.

## Current Parent
- Conversation ID: e73b8d90-04c0-4cf6-9c58-00afd44446a8
- Updated: 2026-07-25T02:09:40Z

## Task Summary
- **What to build**: Fixed Clippy warnings, scanner proc-macro isolation, runner dyld filter, profiler flamegraph command check, CLI init terminal check, clippy fix args, virtual workspace advise support, pre-flight tool checks & target path fix, CI pipeline base flag support.
- **Success criteria**: 0 clippy warnings under `-D warnings`, 100% workspace tests pass, `covopt init --yes` & `covopt ci --fast` work cleanly.
- **Interface contracts**: PROJECT.md / context.md / handoff reports from explorers m1_1 and m1_3.

## Key Decisions Made
- Excluded `covopt-macro` and any proc-macro directory/crate in `scanner::collect_rs_files` and magic number replacement.
- Excluded proc-macro test binaries in `compile_workspace_tests` to prevent macOS dyld runtime crashes.
- Replaced `cargo-flamegraph` check with `flamegraph` / `cargo flamegraph` check in `profiler.rs`.
- Fixed pre-flight tool checks in `harden.rs` and `main.rs` to inspect `output.status.success()`.
- Updated `auto_harness.rs` output path to `target/fuzz/fuzz_targets`.
- Added `args.base` support to `ci.rs`.

## Change Tracker
- **Files modified**:
  - `covopt_core/src/dummy_heuristics.rs` — Removed allow attributes, cleaned clippy warnings.
  - `covopt_core/src/sandbox.rs` — Fixed collapsible_if and unnecessary map_or clippy warnings.
  - `covopt_core/src/scanner.rs` — Excluded proc-macro crates (`covopt-macro`) from file collection & magic number replacement.
  - `covopt_core/src/runner.rs` — Excluded proc-macro test binaries in `compile_workspace_tests`.
  - `covopt_core/src/asm_extractor.rs` — Added virtual workspace manifest support for `--package`.
  - `covopt_core/src/profiler.rs` — Updated flamegraph check for `flamegraph` / `cargo flamegraph`.
  - `covopt_core/src/entropy.rs` — Handled cargo check execution safely without expect panics.
  - `covopt_cli/src/commands.rs` — Fixed clippy warnings, terminal check in `init_config`, clippy fix args, virtual workspace support & public function filter removal in `run_advise`.
  - `covopt_cli/src/main.rs` — Fixed pre-flight tool checks to inspect `status.success()`.
  - `covopt_cli/src/harden.rs` — Fixed pre-flight tool checks for subcommands using `strip_prefix`.
  - `covopt_cli/src/auto_harness.rs` — Updated fuzz output directory to `target/fuzz/fuzz_targets`.
  - `covopt_cli/src/ci.rs` — Implemented `args.base` flag support.
- **Build status**: PASS (0 errors, 0 warnings)
- **Pending issues**: None

## Quality Status
- **Build/test result**: 0 errors, 21/21 tests passed across 6 suites
- **Lint status**: 0 warnings under `rtk cargo clippy --workspace --all-targets -- -D warnings`
- **Tests added/modified**: All existing tests pass cleanly; scanner isolation and runner proc-macro filter verified via CI & unit tests

## Loaded Skills
- None

## Artifact Index
- `/Users/kuangtalin/Documents/CovOpt-Analyzer/.agents/teamwork_preview_worker_m1/handoff.md` — Final Handoff Report
