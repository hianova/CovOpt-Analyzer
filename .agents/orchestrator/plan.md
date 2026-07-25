# CovOpt-Analyzer v2.0 Production Quality Upgrade Plan

## Architecture & Scope
CovOpt-Analyzer is a Rust AST complexity analysis, LLVM profiling, auto-tuning & AI Agent performance analyzer workspace consisting of crates:
- `covopt_core`
- `covopt_cli`
- `covopt-macro`

## Milestones

| # | Name | Scope & Description | Dependencies | Status |
|---|------|---------------------|-------------|--------|
| 1 | R1: CLI & Core Engine Robustness | Ensure all 8 core subcommands (`init`, `ci`, `report`, `fix`, `audit`, `advise`, `profile`, `harden`) execute cleanly across workspace crates (`covopt_core`, `covopt_cli`, `covopt-macro`) without unexpected panics or stdin blocking in non-interactive CI environments. | None | DONE |
| 2 | R2: Comprehensive Benchmark Suite | Provide clean integration tests and benchmark target fixtures verifying complexity analysis models ($O(1)$, $O(\log N)$, $O(N)$, $O(N \log N)$, $O(N^2)$) using `#[covopt::test]` and `covopt_param!`. | M1 | DONE |
| 3 | R3: Automated CI & Report Quality | Ensure `covopt ci` executes end-to-end successfully and produces valid SARIF v2.1.0 and structured JSON output for automated CI/CD and AI Agent integration. Verify all acceptance criteria (`cargo check --workspace`, `cargo test --workspace`, `covopt ci`, `covopt audit --json`). | M1, M2 | DONE |

## Interface Contracts & Rules
1. Zero-Entropy Tuning: All parameters must use `covopt_param!` macro, no hardcoded magical numbers.
2. Anti-DCE: Benchmark loops must wrap loop variables with `std::hint::black_box()`.
3. Lock-Free Critical Paths: Critical paths must not use standard `Mutex` or `RwLock`.
4. Strict Clippy Cleanliness: Zero compiler warnings or clippy warnings across workspace (`cargo check --workspace` 0 warnings).
5. Non-interactive CI execution: Subcommands must default/fallback safely without hanging on stdin.
6. Valid JSON/SARIF format: `covopt audit --json` stdout must be valid JSON parseable by `jq`.
