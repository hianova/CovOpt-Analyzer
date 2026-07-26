# CovOpt-Analyzer Design Flaws Refactoring & Fix Plan

## Architecture & Scope
CovOpt-Analyzer workspace (`covopt_core`, `covopt_cli`, `covopt-macro`).
Goal: Refactor and fix auto-fix, audit, and CLI noise index mechanisms.

## Milestones

| # | Name | Scope & Description | Dependencies | Status |
|---|------|---------------------|-------------|--------|
| 1 | M1: Const Context & Inner Attributes Auto-Fix (R1 & R2) | Fix AST auto-fix to ignore `const fn`, enum discriminants, `const`/`static` variable blocks, and pattern matching arms (E0015). Preserve file-level inner attributes (`#![no_std]`, `//!` comments) at top of `.rs` files. Add explicit unit tests. | None | DONE |
| 2 | M2: Strict Workspace Audit & CLI Noise Index (R3 & R4) | Ensure `covopt ci` fails (non-zero exit code) if `cargo check --workspace` fails. Exclude `tests/` and `examples/` directories from CLI noise index entropy penalty for stdout/`println!`. Add explicit unit/integration tests. | M1 | DONE |
| 3 | M3: End-to-End Verification & Integrity Audit | Verify zero compilation errors/warnings (`cargo check --workspace`), all passing tests (`cargo test --workspace`), E2E `covopt ci` failure/pass behaviors, CLI noise index calculations, and Forensic Auditor verification. | M1, M2 | DONE |

## Interface Contracts & Rules
1. Zero-Entropy Tuning: All parameters must use `covopt_param!` macro, no hardcoded magical numbers.
2. AST Auto-Fix Safety: Ignore `const fn`, enum discriminants, `const`/`static` blocks, and pattern matching arms.
3. Top-level Inner Attribute Preservation: `#![...]` and `//!` comments must stay at top of file, imports inserted after.
4. Strict Audit Exit Code: Non-zero exit code on cargo check workspace failure during `covopt ci`.
5. Path Exclusion for Noise Index: `tests/` and `examples/` relative paths ignored for `println!` penalty.
