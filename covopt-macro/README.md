# CovOpt-Macro

`covopt-macro` is a powerful procedural macro crate that provides advanced static analysis, auto-tuning, and anti-DCE capabilities for the [CovOpt-Analyzer](https://github.com/hianova/CovOpt-Analyzer) performance ecosystem.

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
covopt-macro = "1.1"
```

## Features

### 1. `#[covopt::test]` (Complexity Testing)

Wraps your test function in standard Rust test boilerplate while explicitly defining the expected mathematical complexity and testing parameters for the `CovOpt-Analyzer` regression engine.

```rust
use covopt_macro::covopt_test;

#[covopt_test(expected = "ON", n_values = "1000,5000")]
fn test_my_algorithm(n: usize) {
    // CovOpt will dynamically test this against N=1000 and N=5000
    // to mathematically prove it is O(N).
}
```

### 2. `covopt_param!` (Zero-Entropy Auto-Tuning)

Eliminates hardcoded magic numbers. It serves as an environment-aware probe that allows the external `covopt` CLI engine to dynamically inject and tune performance parameters (like cache sizes or thresholds) during Monte Carlo optimizations.

```rust
use covopt_macro::covopt_param;

fn process_data() {
    let chunk_size = covopt_param!("chunk_size", 1024);
}
```



## Ecosystem

To utilize the full power of these macros (such as parameter tuning and complexity analysis), install the CLI tool:

```bash
cargo install covopt-analyzer
```
