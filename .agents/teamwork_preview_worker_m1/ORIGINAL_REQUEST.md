## 2026-07-25T01:57:26Z
You are Worker 1 assigned to implement Milestone 1: Core Engine & CLI Robustness, Scanner Isolation, Dyld Fix, Clippy Cleaning, and CI Pipeline Fixes for CovOpt-Analyzer v2.0 upgrade.
Your working directory is: /Users/kuangtalin/Documents/CovOpt-Analyzer/.agents/teamwork_preview_worker_m1

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A Forensic Auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

Tasks:
1. Read instructions in /Users/kuangtalin/Documents/CovOpt-Analyzer/.agents/teamwork_preview_worker_m1/context.md and diagnostic handoffs at:
   - /Users/kuangtalin/Documents/CovOpt-Analyzer/.agents/teamwork_preview_explorer_m1_1/handoff.md
   - /Users/kuangtalin/Documents/CovOpt-Analyzer/.agents/teamwork_preview_explorer_m1_3/handoff.md
2. Fix all 17 clippy warnings in `covopt_core/src/dummy_heuristics.rs` and `covopt_cli/src/commands.rs`. Remove all `#![allow(...)]` and `#[allow(...)]` attributes to achieve 100% strict clippy cleanliness under `rtk cargo clippy --workspace --all-targets -- -D warnings`.
3. In `covopt_core/src/scanner.rs`: Exclude `covopt-macro` and proc-macro directories from `collect_rs_files` and magic number substitution.
4. In `covopt_cli/src/commands.rs`:
   - In `init_config`: Add `std::io::stdout().is_terminal()` check before reading stdin so non-interactive CI without `--yes` falls back safely without hanging.
   - Fix `cargo clippy --fix` `--` argument formatting in `commands.rs:960`.
   - Fix `run_advise` for virtual workspace root (scan member crate `src/` directories if root `src/` is missing, pass package name if needed, remove public function exclusion filter).
5. In `covopt_core/src/runner.rs`: Filter `compile_workspace_tests` executable list to exclude proc-macro binaries (which fail with dyld error on macOS).
6. In `covopt_core/src/profiler.rs`: Check `flamegraph` / `cargo flamegraph` instead of `cargo-flamegraph`.
7. In `covopt_cli/src/main.rs` & `covopt_cli/src/harden.rs`: Fix pre-flight tool checks for `cargo mutants` and `cargo fuzz` so `--fast` mode skips missing external binaries cleanly without failing exit status. Update `auto_harness.rs` output path to use crate target directory.
8. In `covopt_cli/src/ci.rs`: Implement support for `args.base` flag in `run_pipeline`.
9. Verify implementation by running:
   - `rtk cargo check --workspace --all-targets`
   - `rtk cargo clippy --workspace --all-targets -- -D warnings`
   - `rtk cargo test --workspace`
   - `rtk ./target/debug/covopt init --yes`
   - `rtk ./target/debug/covopt ci --fast`
10. Write detailed handoff report to `/Users/kuangtalin/Documents/CovOpt-Analyzer/.agents/teamwork_preview_worker_m1/handoff.md` with build/test outputs, changed files, and verified functionality.
11. Send message to orchestrator upon completion. Remember: always prefix shell commands with `rtk`.
