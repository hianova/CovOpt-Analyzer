# CovOpt-Analyzer 🚀

**CovOpt-Analyzer (Coverage-Optimized Complexity Analyzer)** is an innovative, zero-invasive, and highly precise command-line tool designed to mathematically verify the algorithmic time complexity ($O(1)$, $O(N)$, $O(N \log N)$, etc.) of any Rust project.

Instead of relying on fragile execution time measurements (like `criterion` or `samply`), CovOpt-Analyzer leverages LLVM's source-based code coverage (`-C instrument-coverage`) to directly observe CPU instruction execution frequencies. This allows for an absolutely precise, deterministic, and noise-free evaluation of your code's time complexity.

## Features ✨

- **Zero-Invasive**: No macros, no timers, no changes required to your production code.
- **Noise-Free Precision**: Immune to CPU caches, OS scheduling, and background processes. Measures exact code-path hits.
- **Mathematical Convergence Engine**: Automatically calculates Least Squares regression and $R^2$ values to confidently match execution data against Big-O theoretical curves.
- **Robust AST-Based Static Analysis**: Replaces fragile string-matching with full `syn` AST parsing to enforce strict Aerospace Grade standards, verifying `#![no_std]`, memory allocation, cache padding, and accurate thread lifecycles.
- **Hardening Toolkit**: Built-in support for advanced security testing:
  - **Mutation Testing** (`covopt harden run --mutate`): Integrates with `cargo-mutants`.
  - **Fuzzing** (`covopt harden fuzz`): Integrates with `cargo-fuzz`.
  - **Sanitizers** (`covopt harden run --sanitize`): Detects Use-After-Free and data races via LLVM Address/Thread Sanitizers (`-Zsanitizer`).
- **Automated Stress Testing**: Automatically instruments your binaries, injects `COVOPT_N` environment variables, generates `.profraw` data, merges them, and exports LLVM JSON profiles.
- **LLM-Powered Auto-Fix** (`covopt harden run --sanitize --auto-fix`): Connects with Gemini or local LLM servers (Ollama/LM Studio) to automatically patch safety leaks caught by sanitizers.
- **Unified Auto-Pilot CI** (`covopt ci`): A fully integrated pipeline that automatically runs `fix`, `audit`, `optimize`, and `harden` in sequence based on your `.covopt.toml` configuration. Achieves perfect **Zero-Entropy** maintenance.

---

## Recommended Workflows: Humans vs. AI Agents 👥🤖

Depending on whether you are running `covopt` manually in a terminal, or configuring it for an autonomous coding agent (like Google Antigravity), we recommend two distinct workflow pipelines:

### 🧑 For Humans (Interactive Development)
- **Harden & Secure (`covopt harden run`)**: Interactively fuzz your functions, inject mutations, or run sanitizers to find loopholes in test assertions.
- **Visualize Hotspots (`covopt tune profile --tool flamegraph`)**: Profile your CPU hotspots and analyze lock contention using interactive flamegraphs.
- **Parameter Optimization (`covopt tune params`)**: Auto-tune performance parameters to find the most optimal configuration.

### 🤖 For AI Agents (Automated Pipelines & CI)
- **Zero-Warning Pipeline (`covopt ci`)**: Run the fully automated CI pipeline to execute fixes, audits, and parameter tunings in one shot.
- **Clutter-Free Checks (`covopt check audit`)**: Runs all checks defined in `.covopt.toml` compactly. Suppresses noisy cargo build logs and intermediate test execution lines to keep agent context clean. Only reports anomalies or entropy threshold violations.
- **CPU Optimization (`covopt tune profile`)**: Automatically parses the generated `flamegraph.svg` into clean, text-based CPU hotspots and statistics.
- **Self-Healing Loop (`covopt harden run --sanitize --auto-fix`)**: Hook `covopt` with the agent's LLM environment to automatically patch memory bugs.

---

## How It Works 🛠️

1. You define a standard Rust `#[test]` that scales its input size based on an environment variable (`COVOPT_N`).
2. CovOpt-Analyzer automates `cargo test` while injecting `-C instrument-coverage`.
3. It scales $N$ through your provided inputs (e.g., 1000, 5000, 10000).
4. For each $N$, it reads the exact **Hit Count** of a targeted line of code from the underlying `llvm-cov` export.
5. Finally, it feeds the $(N, \text{Hit Count})$ dataset into a regression analyzer to mathematically prove the algorithm's time complexity.

---

## Installation 📦

Since CovOpt-Analyzer is published on crates.io, you can easily install it via Cargo:

```bash
cargo install CovOpt-Analyzer
```

Alternatively, to build from source:
```bash
git clone https://github.com/hianova/CovOpt-Analyzer.git
cd CovOpt-Analyzer
cargo build --release
export PATH=$PATH:$(pwd)/target/release
```

**Prerequisites**:
Ensure you have the `llvm-tools-preview` component installed:
```bash
rustup component add llvm-tools-preview
```

---

## Simple Tutorial 📖

Let's say you have a target crate `my_crate` and you want to test the time complexity of a loop inside `src/lib.rs`.

### 1. Write the Test
In `my_crate`, create a simple test that reads `COVOPT_N`:

```rust
// my_crate/src/lib.rs

#[inline(never)] // Prevents inlining so LLVM-MCA can analyze the function
pub fn process_data(n: usize) {
    let mut sum = 0;
    for i in 0..n {
        // CovOpt-Analyzer will automatically discover and track the most heavily hit line!
        // Use black_box to prevent Dead Code Elimination
        sum += std::hint::black_box(i);
    }
    std::hint::black_box(sum);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_complexity() {
        let n: usize = std::env::var("COVOPT_N")
            .unwrap_or_else(|_| "10".to_string())
            .parse()
            .unwrap();
            
        process_data(n);
    }
}
```

### 2. Run the Analyzer
Navigate to your `my_crate` directory and run CovOpt-Analyzer:

```bash
covopt check audit \
  --test test_process_complexity \
  --expected ON \
  --n-values "1000,5000,10000" \
  --mca-cpu apple-m1
```

### 3. Read the Report
The tool will automatically handle all compilation and profiling, eventually outputting:

```text
Starting CovOpt Analysis for test 'test_process_complexity'...
[Auto-Discovery] Found peak complexity target at src/lib.rs:85
Expected Complexity: ON
Testing N values: [1000, 5000, 10000]
---------------------------------------------------
Running for N = 1000...
  -> Hit count = 1000
Running for N = 5000...
  -> Hit count = 5000
Running for N = 10000...
  -> Hit count = 10000
---------------------------------------------------
Analysis Results:
AnalysisReport {
    is_converged: true,
    expected: ON,
    r_squared: 1.0,
    actual_trend: ON,
}
```
Boom! Your code is mathematically proven to be $O(N)$ with a perfect $R^2 = 1.0$ score.

---

## Configuration via `.covopt.toml`

Instead of passing arguments via CLI, you can initialize a config file and run automated audits.

```bash
covopt init
```

This will generate a `.covopt.toml` file where you can define your targets:

```toml
[[target]]
test = "test_process_complexity"
expected = "ON"
n_values = "100,500,1000"
require_cache_padding = true
require_branch_hints = true
require_aerospace_grade = true
require_watchdog_timeout = true
require_stress_test = true
```

Then simply run:

```bash
covopt check audit
```

This will run the analysis for all the configured targets automatically.

---

## Continuous Integration (GitHub Actions) 🐙

You can easily integrate CovOpt-Analyzer into your CI pipeline using `covopt check audit`. Here is a sample GitHub Actions workflow (`.github/workflows/covopt.yml`):

```yaml
name: CovOpt Analysis

on: [push, pull_request]

jobs:
  audit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install Rust stable
        uses: dtolnay/rust-toolchain@stable
        with:
          components: llvm-tools-preview
          
      - name: Install CovOpt-Analyzer
        run: cargo install CovOpt-Analyzer
        
      - name: Run Audit
        run: covopt check audit
```

## Supported Expected Complexities (`--expected`)
- `O1` or `O(1)` - Constant Time
- `OLogN` or `O(LogN)` - Logarithmic Time
- `ON` or `O(N)` - Linear Time
- `ONLogN` or `O(NLogN)` - Linearithmic Time
- `ON2` or `O(N2)` - Quadratic Time
- `O2N` or `O(2^N)` - Exponential Time
- `OSqrtN` or `O(SQRT(N))` - Square-Root Time

---

## Tips: Preventing Compiler Optimization (Anti-DCE)
Because CovOpt-Analyzer relies on LLVM source-based coverage, aggressive compiler optimizations (like Dead Code Elimination or Loop Unrolling) in `--release` mode might eliminate your targeted loop entirely, resulting in a **0 Hit Count** and failing the LLVM-MCA analysis.

To ensure your algorithmic loop is preserved and accurately counted:
1. **Use `std::hint::black_box`**: Wrap the inputs and the final output of your loop in `black_box`. This tells LLVM that the value is opaque and cannot be optimized away.
2. **Use `#[inline(never)]`**: Add this macro to the function you are profiling. If the function gets inlined into the test runner, LLVM-MCA might fail to locate the target assembly block.


## License 📜
This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
