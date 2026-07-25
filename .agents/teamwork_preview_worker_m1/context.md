# Worker 1 Context: Core Engine & CLI Robustness, Clippy Cleaning, Scanner & CI Pipeline Fixes

Scope:
1. Clean all 17 clippy warnings in `covopt_core/src/dummy_heuristics.rs` and `covopt_cli/src/commands.rs`. Remove `#![allow(...)]` and `#[allow(...)]` attributes.
2. In `covopt_core/src/scanner.rs`: Exclude `covopt-macro` and proc-macro directories from `collect_rs_files` and magic number replacement.
3. In `covopt_cli/src/commands.rs`: Add `std::io::stdout().is_terminal()` check in `init_config` so non-interactive CI without `--yes` does not block on stdin. Fix `cargo clippy --fix` `--` flag formatting. Fix `run_advise` for virtual workspace root (scan crate `src/` folders, pass package name if needed, remove public function skip).
4. In `covopt_core/src/runner.rs`: Filter `compile_workspace_tests` executable list to exclude proc-macro binaries (which fail with dyld error on macOS).
5. In `covopt_core/src/profiler.rs`: Check `flamegraph` / `cargo flamegraph` instead of `cargo-flamegraph`.
6. In `covopt_cli/src/main.rs` & `harden.rs`: Fix pre-flight check logic for `cargo mutants` and `cargo fuzz` so `--fast` mode skips missing tools cleanly.
7. In `covopt_cli/src/ci.rs`: Apply `args.base` flag in `run_pipeline`.

Verification:
Run `rtk cargo check --workspace --all-targets` (0 errors)
Run `rtk cargo clippy --workspace --all-targets -- -D warnings` (0 errors, 0 warnings)
Run `rtk cargo test --workspace` (100% pass)
