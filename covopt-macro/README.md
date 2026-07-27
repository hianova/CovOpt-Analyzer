# covopt-macro

This crate provides the core procedural macros for the **CovOpt-Analyzer** performance and complexity testing framework.

## Features

- `#[covopt_test]`: Anchors a benchmark test for complexity analysis. It allows you to specify the target function, expected Big-O complexity (e.g., "ON", "O1", "ONlogN"), and the scaling `N` values.
- `covopt_param!`: Defines an auto-tunable parameter for CovOpt-Analyzer's heuristic engine. This allows the AI agent to superoptimize magical numbers and constants safely without breaking your build.

## Usage

This crate is designed to be used in conjunction with the `CovOpt-Analyzer` CLI. 

Add it to your `Cargo.toml`:
```toml
[dev-dependencies]
covopt-macro = "2.0.0"
```

### Example
```rust
use covopt_macro::{covopt_test, covopt_param};

#[cfg_attr(test, inline(never))]
pub fn process_data(n: usize) -> usize {
    let mut sum = 0;
    
    // Magic numbers can be automatically tuned by the CovOpt agent
    let step = covopt_param!("M_STEP_SIZE", 1); 
    
    for i in (0..n).step_by(step) {
        sum += std::hint::black_box(i); // Prevent dead-code elimination
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;
    use covopt_macro::covopt_test;

    // Automatically generates N scaling loops, AST anchoring, and Big-O assertions
    #[covopt_test(target_fn = "process_data", expected = "ON", n_values = "1000,5000,10000")]
    fn test_process_complexity(n: usize) {
        process_data(n);
    }
}
```

## Learn More
For the full documentation, architecture details, and CLI usage, please visit the [main CovOpt-Analyzer repository](https://github.com/hianova/CovOpt-Analyzer).
