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
use covopt_macro::test as covopt_test;

#[covopt_test(expected = "ON", n_values = "1000,5000")]
fn test_my_algorithm(n: usize) {
    // CovOpt will dynamically test this against N=1000 and N=5000
    // to mathematically prove it is O(N).
}
```

### 2. `#[no_dce]` (Anti-Dead Code Elimination Shield)

Prevents LLVM from aggressively optimizing away your pure, side-effect-free algorithms during `--release` benchmarking. It automatically wraps all inputs and outputs in `std::hint::black_box`.

```rust
use covopt_macro::no_dce;

#[no_dce]
fn process(n: usize) -> usize {
    let mut sum = 0;
    for i in 0..n {
        sum += i;
    }
    sum
}
```

### 3. `covopt_track!()` (Precision Anchoring)

Allows you to explicitly anchor the static hit-count extraction to a specific line in your code, bypassing the heuristic Auto-Discovery engine.

```rust
use covopt_macro::track as covopt_track;

fn complex_algorithm(n: usize) {
    for i in 0..n {
        // CovOpt-Analyzer will lock onto this exact line for hit counting
        covopt_track!(); 
    }
}
```

### 4. `covopt_param!` (Zero-Entropy Auto-Tuning)

Eliminates hardcoded magic numbers. It serves as an environment-aware probe that allows the external `covopt` CLI engine to dynamically inject and tune performance parameters (like cache sizes or thresholds) during Monte Carlo optimizations.

```rust
use covopt_macro::covopt_param;

fn process_data() {
    let chunk_size = covopt_param!("chunk_size", 1024);
}
```

### 5. `#[derive(SoA)]` (Data-Oriented Design)

Automatically generates a Structure-of-Arrays (SoA) variant for your Array-of-Structures (AoS) definitions, enabling seamless A/B testing of memory layouts.

```rust
use covopt_macro::SoA;

#[derive(SoA)]
pub struct Particle {
    pub x: f32,
    pub y: f32,
}
// Automatically generates:
// pub struct ParticleSoa { pub x: Vec<f32>, pub y: Vec<f32> }
```

## Ecosystem

To utilize the full power of these macros (such as parameter tuning and complexity analysis), install the CLI tool:

```bash
cargo install covopt-analyzer
```
