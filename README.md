# CovOpt-Analyzer 🚀

**CovOpt-Analyzer (Coverage-Optimized Complexity Analyzer & Auto-Tuner)** is an innovative, zero-invasive command-line tool designed to mathematically verify algorithmic time complexity ($O(1)$, $O(N)$, etc.), detect performance bottlenecks, and enforce safety standards in Rust projects.

It leverages LLVM's source-based code coverage for an absolutely precise, deterministic, and noise-free evaluation of your code's asymptotic behavior.

---

## ✨ Key Features
- **Zero-Invasive**: Measures exact AST code-path hit counts without modifying production binaries.
- **Mathematical Fitting Engine**: Uses Least Squares regression to prove theoretical Big-O curves.
- **Senior Engineer Advisor**: Detects hot-path heap allocations, Tokio async blocks, and lock contention.
- **Auto-Pilot Pipeline**: Unified Fix ➔ Audit ➔ Report pipeline via `covopt ci`.
- **Git Incremental Audit**: Native support for `--staged` and `--diff main`.

---

## ⚡ Quick Start

### 1. Installation
```bash
cargo install CovOpt-Analyzer
rustup component add llvm-tools-preview
```

### 2. Write a Benchmark Test
Add to `Cargo.toml`:
```toml
[dev-dependencies]
covopt-macro = "2.0.0"
```

In your tests:
```rust
use covopt_macro::covopt_test;

#[covopt_test(target_fn = "process_data", expected = "ON", n_values = "1000,5000")]
fn test_process_complexity(n: usize) {
    process_data(n); // Ensure anti-DCE is used internally!
}
```

### 3. Usage Cheat Sheet
```bash
covopt init                  # Setup & AI Agent rules
covopt fix                   # Auto-repair warnings
covopt audit                 # Audit complexity
covopt advise                # Senior Engineer checks
covopt profile --test <name> # Generate SVG Flamegraphs
covopt ci                    # Run unified pipeline
```

---

## 📚 Documentation
For deeper dives into the architecture and advanced agentic workflows:
- [Architecture & Tech Stack](ARCHITECTURE.md)
- [Advanced Usage & AI Piping](ADVANCED.md)

## 📜 License
MIT License - see [LICENSE](LICENSE) for details.
