## 2026-07-26T15:32:45Z
You are Worker 1 (teamwork_preview_worker) for CovOpt-Analyzer refactoring and design flaw fixes (R1, R2, R3, R4).

Working directory: /Users/kuangtalin/Documents/CovOpt-Analyzer/.agents/teamwork_preview_worker_m1
Workspace root: /Users/kuangtalin/Documents/CovOpt-Analyzer

Please read the handoff reports from the 3 Explorers before starting:
1. Explorer 1 handoff (R1 & R2): `/Users/kuangtalin/Documents/CovOpt-Analyzer/.agents/teamwork_preview_explorer_m1_1/handoff.md`
2. Explorer 2 handoff (R3): `/Users/kuangtalin/Documents/CovOpt-Analyzer/.agents/teamwork_preview_explorer_m1_2/handoff.md`
3. Explorer 3 handoff (R4): `/Users/kuangtalin/Documents/CovOpt-Analyzer/.agents/teamwork_preview_explorer_m1_3/handoff.md`

Tasks to Implement:

1. **R1: Fix Const Context Auto-Fix (E0015)**
   - In `covopt_core/src/scanner.rs`, update `MagicNumberScanner` (`syn::visit::Visit<'ast>`) to skip traversal of all const contexts:
     - `visit_item_static` (skip)
     - `visit_impl_item_const` (skip)
     - `visit_trait_item_const` (skip)
     - `visit_item_fn` (if `sig.constness.is_some()`, skip)
     - `visit_impl_item_fn` (if `sig.constness.is_some()`, skip)
     - `visit_trait_item_fn` (if `sig.constness.is_some()`, skip)
     - `visit_variant` (skip)
     - `visit_pat` (skip)
     - `visit_expr_const` (skip)
     - `visit_attribute` (skip)
   - Write explicit unit tests in `covopt_core/src/scanner.rs` or `covopt_cli` verifying that auto-fix correctly ignores `const fn`, enum discriminants, `const`/`static` variable blocks, and pattern matching arms.

2. **R2: Preserve Inner Attributes**
   - In `covopt_core/src/scanner.rs` and `covopt_cli/src/auto_fixer.rs`, replace `lines.insert(0, ...)` with a helper function `find_import_insert_index(&lines)` that finds the first line index after module doc comments (`//!`), inner attributes (`#![no_std]`, `#![...]`), and leading blank/comment lines.
   - Write explicit unit tests verifying that auto-fix preserves file-level inner attributes at the absolute top of `.rs` files.

3. **R3: Strict Workspace Audit**
   - In `covopt_core/src/runner.rs` / `covopt_core/src/entropy.rs` / `covopt_cli/src/commands.rs`, enforce strict workspace checking.
   - Ensure `covopt ci` and `covopt audit` invoke `cargo check --workspace --all-targets` and check `output.status.success()`.
   - If `cargo check --workspace` fails, `covopt ci` must fail and return a non-zero exit code.
   - Write explicit unit/integration tests verifying that `covopt ci` fails with non-zero exit status if workspace compilation fails.

4. **R4: Refine CLI Noise Index**
   - In `covopt_core/src/entropy.rs`, inspect Cargo JSON diagnostic `spans` / `file_name` using `std::path::Path::new(file_name).components()`.
   - Filter out compiler warnings/errors originating from files in `tests/` and `examples/` directories from noise index entropy penalties.
   - Refactor parsing into `parse_cli_noise_from_json(stdout: &str) -> (usize, f64)`.
   - Write explicit unit tests verifying that warnings in `tests/` and `examples/` are excluded and yield 0 penalty score.

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A Forensic Auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

Rules:
- Always prefix shell commands with `rtk` (e.g. `rtk cargo check --workspace`, `rtk cargo test --workspace`).
- Run build and test commands (`rtk cargo check --workspace`, `rtk cargo test --workspace`) and document output in your handoff report.
- Zero-Entropy Tuning: NEVER use hardcoded magical numbers. ALWAYS use `covopt_param!` macro.
- Anti-DCE: ALWAYS wrap loop variables with `std::hint::black_box()` in benchmarks.
- Lock-Free Critical Paths: NEVER use standard library `Mutex` or `RwLock` on critical path.
- Strict Clippy Cleanliness: DO NOT use `#[allow(...)]` to ignore type warnings.

Write your handoff report to `/Users/kuangtalin/Documents/CovOpt-Analyzer/.agents/teamwork_preview_worker_m1/handoff.md` and send a message when done.
