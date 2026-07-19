# CovOpt-Macro

`covopt-macro` is a **zero-dependency**, extremely lightweight instrumentation crate for the [CovOpt-Analyzer](https://github.com/hianova/CovOpt-Analyzer) performance tuning ecosystem.

## Overview

In system programming, hardcoded "magic numbers" (like cache thresholds, buffer sizes, retry counts, or batch sizes) limit your ability to optimize your algorithms. CovOpt-Analyzer aims to eliminate these magic numbers to achieve a **Zero-Entropy** architecture.

This crate provides a single, simple declarative macro: `covopt_param!`. 
It serves as an environment-aware **probe** that allows the external `covopt` CLI engine to dynamically inject and tune performance parameters during benchmarking without modifying your source code.

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
covopt-macro = "1.0"
```

*Note: This crate has **0 dependencies** and introduces absolutely no overhead in your production binaries.*

## Usage

Replace your magic numbers with the `covopt_param!` macro:

```rust
use covopt_macro::covopt_param;

fn process_data(data: &[u8]) {
    // Before:
    // let chunk_size = 1024;
    
    // After:
    let chunk_size = covopt_param!("chunk_size", 1024);
    
    // ... logic ...
}
```

### How it works

1. **Normal Execution** (e.g., `cargo run`, `cargo build --release`):
   The macro simply evaluates the environment variable (if present). If not present, it safely and instantly falls back to the `$default` value (`1024` in the example).
   
2. **Auto-Tuning Execution** (`covopt optimize --params`):
   When you run the CovOpt-Analyzer CLI tool, it orchestrates a Monte Carlo random walk (or logarithmic discrete diffusion). It will repeatedly inject dynamically generated environment variables (like `COVOPT_PARAM_chunk_size=2048`) and execute your `cargo bench` to find the absolute optimal threshold for your specific hardware.

## Ecosystem

This crate is just the "probe". To use the tuning engine, you need to install the CLI tool:

```bash
cargo install covopt-analyzer
```

Learn more at the main [CovOpt-Analyzer repository](https://github.com/hianova/CovOpt-Analyzer).
