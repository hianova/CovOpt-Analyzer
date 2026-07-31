# CovOpt-Analyzer 🚀

**CovOpt-Analyzer (Coverage-Optimized Complexity Analyzer & Auto-Tuner)** is an evidence-driven Rust analyzer for complexity growth, safety obligations, optimization candidates, and verified repairs.

LLVM source-based coverage (`-C instrument-coverage`) provides deterministic source-region execution counts. CovOpt fits those counts across input sizes to classify growth. Coverage does not count CPU instructions, model cache behavior, prove UB absence, or replace a profiler; those concerns use separate evidence providers.

---

## 🛠️ Tech Stack

CovOpt-Analyzer is built with a high-performance, modular Rust architecture:

| Domain | Technologies & Libraries |
| :--- | :--- |
| **Core & CLI** | Rust (Edition 2024), `clap` v4, workspace crates `CovOpt-Analyzer`, `covopt-schema`, and `covopt-macro` |
| **AST & Code Manipulation** | `syn` (AST parsing & visitor traversal), `quote` & `proc-macro2` (AST mutation & macro generation) |
| **Coverage & Dynamic Analysis** | LLVM Source-Based Coverage (`-C instrument-coverage`), `llvm-profdata`, `llvm-cov`, `lcov` parser |
| **Profiling & Assembly** | LLVM-MCA for target assembly; optional runtime profiling through `covopt verify runtime` |
| **Hardening & Security** | `cargo-mutants` (Mutation Testing), `cargo-fuzz` (Fuzzing), LLVM Sanitizers (`ASan`/`TSan`) |
| **AI Agent & CI Integration** | `serde` / `serde_json` (Structured JSON API), SARIF v2.1.0 (GitHub Actions PR Annotations) |
| **Search & Storage** | Seeded annealed Monte Carlo search, versioned JSON evidence, `tempfile` repair sandboxes |

---

## ✨ Key Features

- **Source-preserving Coverage Instrumentation**: Measures source-region execution counts in analysis builds without editing production source.
- **Complexity Fitting Engine**: Uses regression ($R^2$) to classify and detect regressions in Big-O growth ($O(1) \dots O(2^N)$).
- **Senior Engineer Advisor (`covopt advise`)**: Detects hot-path heap allocations (`.clone()`, `vec![]`), Tokio async blocking calls, thread overbounds, and lock contention.
- **Auto-Pilot Pipeline (`covopt ci`)**: Runs unified Fix ➔ Audit ➔ Report pipeline in one command.
- **AI Agent & Unix Piping Ready**: Pure JSON output mode (`covopt audit --json | jq .`) with strict `stdout`/`stderr` separation.
- **Git Incremental Audit**: Native support for `--staged` (0.3s pre-commit hook) and `--diff main` (PR differential checks).

---

## ⚡ Quick Start (Getting Started)

### 1. Installation

Install via crates.io:
```bash
cargo install CovOpt-Analyzer
```

Ensure the LLVM tools preview component is installed:
```bash
rustup component add llvm-tools-preview
```

### 2. Write a Benchmark Test

Add `covopt-macro` to your `Cargo.toml`:
```toml
[dev-dependencies]
covopt-macro = "2.0.0"
```

In your Rust code (`src/lib.rs`):
```rust
#[cfg_attr(test, inline(never))]
pub fn process_data(n: usize) -> usize {
    let mut sum = 0;
    for i in 0..n {
        sum += std::hint::black_box(i); // Anti-DCE
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;
    use covopt_macro::covopt_test;

    // Automatically generates N scaling loop, AST anchoring, and Big-O assertions
    #[covopt_test(target_fn = "process_data", expected = "ON", n_values = "1000,5000,10000")]
    fn test_process_complexity(n: usize) {
        process_data(n);
    }
}
```

### 3. Core Commands Cheat Sheet

CovOpt exposes six primary commands (`init`, `check`, `inspect`, `optimize`, `fix`, and `verify`); older command names remain hidden compatibility aliases.

```bash
# 1. Quick Setup (creates .covopt.toml and injects AI Agent rules)
covopt init

# 2. Automated Code Repair (Clippy fixes + covopt_param! substitution)
covopt fix

# 3. Check guarantees using the Evidence Planner
covopt check --mode adaptive

# 4. Git Incremental Check
covopt check --staged

# 5. Explain findings and repair candidates
covopt inspect --format json

# 6. Force a coverage provider
covopt verify coverage --target test_process_complexity

# 7. Search code-generation candidates
covopt optimize codegen --target test_process_complexity

# 8. Plan or apply minimal repairs
covopt fix --plan

# 9. Inspect the hierarchical scope/proof frontier
covopt inspect --envelope --target test_process_complexity
covopt inspect --frontier --target test_process_complexity

# 10. Check bounded temporal or relational contracts
covopt verify temporal --target test_process_complexity --operator eventually --event return --bound 32
covopt verify relational --target test_process_complexity --base tests/baseline.rs

# 11. Search bounded adversarial environments
covopt optimize adversarial --target test_process_complexity --budget 30s --seed 7
```

---

## 🤖 AI Agent & Piping Integration

CovOpt is designed for Unix command chaining and AI Agent workflows.

### Piping into `jq`
When `--json` is passed, all diagnostic logs stream to `stderr`, leaving `stdout` with clean, machine-readable JSON:
```bash
covopt check --format json | jq '.targets[] | select(.passed == false)'
```

### SARIF Report for GitHub Actions
Generate SARIF v2.1.0 output for inline PR annotations in CI:
```bash
covopt check --format sarif
```

---

## 📖 Recommended Workflows

### 🧑 For Humans (Interactive Development)
- **`covopt init --hook`**: Install a fast git pre-commit hook.
- **`covopt check`**: Evaluate guarantees under the configured policy.
- **`covopt inspect`**: Get findings, evidence, and repair candidates.
- **`covopt verify runtime`**: Force an optional runtime profiling provider when static/coverage evidence is insufficient.

### 🤖 For AI Coding Agents (Antigravity / Cursor / CI)
- **`covopt check --format json`**: Structured APIs for automated parsing.
- **`covopt check --format sarif`**: Produce SARIF for CI annotations.
- **`covopt inspect --target foo`**: Analyze a target without source edits.

### Bounded evidence and reproducibility

Checks persist a versioned assurance snapshot containing source hashes, stable
scope/function IDs, assumptions, evidence, samples, and the proof frontier.
Temporal, relational, atomic, and adversarial analyses are bounded evidence;
they report their bound, assumptions, timeout, or counterexample and do not
silently claim an unbounded proof. Use explicit `--seed` values for stochastic
search and `covopt inspect --drift --target foo` to compare against the prior
snapshot.

`covopt check` performs planning before expensive work, executes only providers
with an automatic check executor, records actual evidence, and emits a
`follow_up_plan` for obligations that remain unresolved. Parameter classes and
tags are metadata; all numeric parameter optimization uses the same seeded
annealed Monte Carlo engine.

---

## 📜 License
This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
