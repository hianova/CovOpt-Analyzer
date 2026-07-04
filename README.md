# CovOpt-Analyzer 🚀

**CovOpt-Analyzer (Coverage-Optimized Complexity Analyzer)** is an innovative, zero-invasive, and highly precise command-line tool designed to mathematically verify the algorithmic time complexity ($O(1)$, $O(N)$, $O(N \log N)$, etc.) of any Rust project.

Instead of relying on fragile execution time measurements (like `criterion` or `samply`), CovOpt-Analyzer leverages LLVM's source-based code coverage (`-C instrument-coverage`) to directly observe CPU instruction execution frequencies. This allows for an absolutely precise, deterministic, and noise-free evaluation of your code's time complexity.

## Features ✨

- **Zero-Invasive**: No macros, no timers, no changes required to your production code.
- **Noise-Free Precision**: Immune to CPU caches, OS scheduling, and background processes. Measures exact code-path hits.
- **Mathematical Convergence Engine**: Automatically calculates Least Squares regression and $R^2$ values to confidently match execution data against Big-O theoretical curves.
- **Automated Stress Testing**: Automatically instruments your binaries, injects `COVOPT_N` environment variables, generates `.profraw` data, merges them, and exports LLVM JSON profiles.

---

## How It Works 🛠️

1. You define a standard Rust `#[test]` that scales its input size based on an environment variable (`COVOPT_N`).
2. CovOpt-Analyzer automates `cargo test` while injecting `-C instrument-coverage`.
3. It scales $N$ through your provided inputs (e.g., 1000, 5000, 10000).
4. For each $N$, it reads the exact **Hit Count** of a targeted line of code from the underlying `llvm-cov` export.
5. Finally, it feeds the $(N, \text{Hit Count})$ dataset into a regression analyzer to mathematically prove the algorithm's time complexity.

---

## Installation 📦

Clone the repository and build it locally:

```bash
git clone https://github.com/your-username/CovOpt-Analyzer.git
cd CovOpt-Analyzer
cargo build --release

# Optional: Add the executable to your PATH
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
        // We want to track the hit count of this line! (e.g., line 7)
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
covopt \
  --test test_process_complexity \
  --expected ON \
  --n-values "1000,5000,10000" \
  --target-file src/lib.rs \
  --target-line 7 \
  --mca-cpu apple-m1
```

### 3. Read the Report
The tool will automatically handle all compilation and profiling, eventually outputting:

```text
Starting CovOpt Analysis for test 'test_process_complexity'...
Target: src/lib.rs:6
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

## [Simulating Time With Square-Root Space](https://arxiv.org/abs/2502.17779)

## License 📜
This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
