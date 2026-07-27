# CovOpt-Analyzer 🚀

**CovOpt-Analyzer (Coverage-Optimized Complexity Analyzer & Auto-Tuner)** is an innovative, zero-invasive, and highly precise command-line tool designed to mathematically verify algorithmic time complexity ($O(1)$, $O(N)$, $O(N \log N)$, etc.), detect performance bottlenecks, and enforce safety & performance standards in Rust projects.

Instead of relying on fragile execution time measurements (like `criterion`), CovOpt-Analyzer leverages LLVM's source-based code coverage (`-C instrument-coverage`) to observe exact CPU instruction execution frequencies. This allows for an absolutely precise, deterministic, and noise-free evaluation of your code's asymptotic behavior.

---

## 🛠️ Tech Stack

CovOpt-Analyzer is built with a high-performance, modular Rust architecture:

| Domain | Technologies & Libraries |
| :--- | :--- |
| **Core & CLI** | Rust (Edition 2024), `clap` v4 (Derive CLI parser), Workspace modularization (`covopt_core`, `covopt_cli`, `covopt-macro`) |
| **AST & Code Manipulation** | `syn` (AST parsing & visitor traversal), `quote` & `proc-macro2` (AST mutation & macro generation) |
| **Coverage & Dynamic Analysis** | LLVM Source-Based Coverage (`-C instrument-coverage`), `llvm-profdata`, `llvm-cov`, `lcov` parser |
| **Profiling & Assembly** | LLVM-MCA (LLVM Microarchitecture Analysis for IPC & execution ports), `cargo flamegraph` (SVG parser), `samply` |
| **Hardening & Security** | `cargo-mutants` (Mutation Testing), `cargo-fuzz` (Fuzzing), LLVM Sanitizers (`ASan`/`TSan`) |
| **AI Agent & CI Integration** | `serde` / `serde_json` (Structured JSON API), SARIF v2.1.0 (GitHub Actions PR Annotations) |
| **Parallelism & Storage** | `rayon` (Bounded thread pool), `tempfile` (Isolated sandbox execution) |

---

## ✨ Key Features

- **Zero-Invasive Coverage Instrumentation**: Measures exact AST code-path hit counts without modifying production binaries.
- **Mathematical Fitting Engine**: Uses Least Squares regression ($R^2$) to prove theoretical Big-O curves ($O(1) \dots O(2^N)$).
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
covopt-macro = "1.1"
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

CovOpt provides **8 clean, high-cohesion commands**:

```bash
# 1. Quick Setup (creates .covopt.toml and injects AI Agent rules)
covopt init

# 2. Automated Code Repair (Clippy fixes + covopt_param! substitution)
covopt fix

# 3. Audit Complexity & IPC Coverage across targets
covopt audit

# 4. Git Incremental Audit (Super-fast 0.3s pre-commit check)
covopt audit --staged

# 5. Senior Engineer Architectural Advisor
covopt advise

# 6. Profile CPU Hotspots (generates Flamegraph & extracts Top 5 Hotspots)
covopt profile --test test_process_complexity

# 7. Security Hardening (Mutation, Fuzzing, Sanitizers)
covopt harden --test test_process_complexity

# 8. Unified Auto-Pilot CI Pipeline
covopt ci --report
```

---

## 🤖 AI Agent & Piping Integration

CovOpt is designed for Unix command chaining and AI Agent workflows.

### Piping into `jq`
When `--json` is passed, all diagnostic logs stream to `stderr`, leaving `stdout` with clean, machine-readable JSON:
```bash
covopt audit --json | jq '.targets[] | select(.passed == false)'
```

### SARIF Report for GitHub Actions
Generate SARIF v2.1.0 output for inline PR annotations in CI:
```bash
covopt report --format sarif
```

---

## 📖 Recommended Workflows

### 🧑 For Humans (Interactive Development)
- **`covopt init --hook`**: Install a fast git pre-commit hook.
- **`covopt fix`**: Auto-fix Clippy warnings and wrap magic numbers.
- **`covopt advise`**: Get instant warnings on hot-path allocations and lock contention.
- **`covopt profile`**: Profile CPU hotspots and visualize SVG flamegraphs.

### 🤖 For AI Coding Agents (Antigravity / Cursor / CI)
- **`covopt ci`**: Unified one-shot pipeline for self-healing and validation.
- **`covopt audit --json`**: Structured JSON APIs for automated parsing.
- **`covopt advise --diff main`**: Analyze PR diffs for complexity regressions.

---

## 📜 License
This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
