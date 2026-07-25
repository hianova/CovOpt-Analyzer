## 2026-07-25T05:51:56Z
You are Worker 2 assigned to implement Milestone 2: Comprehensive Benchmark Suite & Rule Conformance for CovOpt-Analyzer v2.0 upgrade.
Your working directory is: /Users/kuangtalin/Documents/CovOpt-Analyzer/.agents/teamwork_preview_worker_m2

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A Forensic Auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

Tasks:
1. Read instructions in /Users/kuangtalin/Documents/CovOpt-Analyzer/.agents/teamwork_preview_worker_m2/context.md and diagnostic handoff at /Users/kuangtalin/Documents/CovOpt-Analyzer/.agents/teamwork_preview_explorer_m1_2/handoff.md.
2. Workspace Test Structure: Move/co-locate integration tests from root `/tests/` into `covopt_cli/tests/` so `rtk cargo test --workspace` automatically executes all integration tests.
3. Complete Big-O Benchmark Target Fixtures: Provide clean integration test fixtures in `covopt_cli/tests/` covering all 5 complexity models:
   - $O(1)$: `ruinsos_scheduler.rs`
   - $O(\log N)$: `binary_search.rs` (using `#[covopt::test]` and `covopt_param!`)
   - $O(N)$: `linear_scan.rs` / `dummy_test.rs` (using `#[covopt::test]` and `covopt_param!`)
   - $O(N \log N)$: `merge_sort.rs` (using `#[covopt::test]` and `covopt_param!`)
   - $O(N^2)$: `matrix_mult.rs` (using `#[covopt::test]` and `covopt_param!`)
4. Macro & Static Analysis Parsing:
   - Update `covopt-macro/src/lib.rs` (`covopt_test` proc macro) to parse attribute args and use `covopt_param!` for default parameters instead of hardcoded numbers.
   - Update `covopt_core/src/static_analysis.rs`: match both `#[covopt::test]` and `#[covopt_test]`, fix attribute key-value parsing for array/string args without naive comma splits.
5. Rule Conformance Enforcement:
   - Zero-Entropy Tuning: Replace hardcoded values with `covopt_param!`.
   - Anti-DCE: Wrap all loop range iterators / variables with `std::hint::black_box()` in benchmark functions.
   - Lock-Free Critical Paths: Maintain lock-free critical paths.
   - Strict Clippy Cleanliness: `rtk cargo check --workspace --all-targets` (0 errors) and `rtk cargo clippy --workspace --all-targets -- -D warnings` (0 warnings/errors). No `#[allow(...)]` attributes.
6. Verification:
   - Run `rtk cargo check --workspace --all-targets`
   - Run `rtk cargo clippy --workspace --all-targets -- -D warnings`
   - Run `rtk cargo test --workspace` (verify all benchmark tests execute and pass)
   - Run `rtk cargo run --bin covopt -- audit --json --fast`
7. Write detailed handoff report to `/Users/kuangtalin/Documents/CovOpt-Analyzer/.agents/teamwork_preview_worker_m2/handoff.md`.
8. Send message to orchestrator upon completion. Always prefix shell commands with `rtk`.
