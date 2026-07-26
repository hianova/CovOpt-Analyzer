# Original User Request

## Follow-up — 2026-07-26T07:29:35Z

Refactor and fix severe design flaws in the CovOpt-Analyzer tool's auto-fix, audit, and CLI noise index mechanisms to ensure workspace compilation stability and correct parsing of Rust syntax.

Working directory: /Users/kuangtalin/Documents/CovOpt-Analyzer
Integrity mode: development

### Requirements

#### R1. Fix Const Context Auto-Fix (E0015)
The `covopt` auto-fix mechanism must not break compilation by injecting `covopt_param!` into `const`, `static`, enum discriminants, pattern matching arms, or `const fn` contexts. 

#### R2. Preserve Inner Attributes
The auto-fix mechanism must not insert `use` statements or any other code before file-level inner attributes (e.g., `#![no_std]` or `//!` doc comments) at the top of `.rs` files. The agent team is free to choose the best implementation approach for this.

#### R3. Strict Workspace Audit
The `covopt ci` audit mechanism must verify that the entire workspace compiles successfully and must not report "[CI OK] Audit passed" if there are compilation errors in any workspace member.

#### R4. Refine CLI Noise Index
The CLI noise index calculation must not penalize `println!` or standard output calls within `examples/` and `tests/` directories.

### Acceptance Criteria

#### Const Context & Attributes
- [ ] Explicit unit tests exist in `covopt_cli` or `covopt_core` verifying that the auto-fix AST logic correctly ignores `const fn`, enum discriminants, and `const`/`static` variable blocks.
- [ ] Explicit unit tests exist verifying that auto-fix preserves file-level inner attributes at the absolute top of the file.

#### Audit & Workspace Stability
- [ ] Running `covopt ci` fails and returns a non-zero exit code if `cargo check --workspace` fails.
- [ ] The CLI noise index properly excludes `tests/` and `examples/` directories from its entropy penalty calculations.
