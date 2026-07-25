# Worker 2 Context: Milestone 2 — Comprehensive Benchmark Suite & Rule Conformance

Scope:
1. Workspace Test Structure: Move/co-locate integration test fixtures from root `/tests/` (`dummy_test.rs`, `no_macro_test.rs`, `ruinsos_scheduler.rs`, `spin_deadlock.rs`, `uaf_thread_exit.rs`) into `covopt_cli/tests/` so `rtk cargo test --workspace` automatically runs all integration tests.
2. Complete Big-O Benchmark Target Fixtures: Add/update benchmark fixtures covering all 5 complexity models in `covopt_cli/tests/`:
   - $O(1)$: `ruinsos_scheduler.rs`
   - $O(\log N)$: `binary_search.rs` (using `#[covopt::test]` and `covopt_param!`)
   - $O(N)$: `linear_scan.rs` (using `#[covopt::test]` and `covopt_param!`)
   - $O(N \log N)$: `merge_sort.rs` (using `#[covopt::test]` and `covopt_param!`)
   - $O(N^2)$: `matrix_mult.rs` (using `#[covopt::test]` and `covopt_param!`)
3. Macro & Static Analysis Fixes:
   - Update `covopt-macro/src/lib.rs`: `covopt_test` proc macro must parse attributes and use `covopt_param!` for defaults instead of hardcoded numbers.
   - Update `covopt_core/src/static_analysis.rs`: recognize both `#[covopt::test]` and `#[covopt_test]`, fix attribute key-value parsing for array/string args without naive comma splitting bugs.
4. Rule Conformance:
   - Zero-Entropy Tuning: Replace any remaining hardcoded values in macro defaults, test fixtures, or static analysis fallback logic with `covopt_param!`.
   - Anti-DCE: Wrap loop range iterators / variables with `std::hint::black_box()` in all benchmark functions.
   - Strict Clippy Cleanliness: `rtk cargo check --workspace --all-targets` (0 errors) and `rtk cargo clippy --workspace --all-targets -- -D warnings` (0 warnings/errors). No `#[allow(...)]` attributes.

Verification:
Run `rtk cargo check --workspace --all-targets`
Run `rtk cargo clippy --workspace --all-targets -- -D warnings`
Run `rtk cargo test --workspace`
Run `rtk cargo run --bin covopt -- audit --json --fast`
Write handoff report to `.agents/teamwork_preview_worker_m2/handoff.md`.
