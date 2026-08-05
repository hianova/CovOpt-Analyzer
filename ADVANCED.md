# Advanced Workflows (CovOpt 3.1)

This document covers advanced usages of CovOpt, focusing on The Crucible (Z3 + Annealing) and how to configure custom fuzzers and bounds.

## The Crucible: Parameter Fitting & Optimization

CovOpt relies on **The Crucible** to resolve hyperparameters and magic numbers. It combines formal methods with empirical trials.

### Magic Numbers (Z3 SMT Solver)
When CovOpt relaxes an AST to uncover unknown constants (e.g., bit shifts `(x >> C1) & C2`, or the famous Quake `0x5f3759df` FastInvSqrt magic number), it does not randomly guess. Instead:
1. CovOpt translates the Rust AST and its mathematical bounds into SAT/CNF (Conjunctive Normal Form).
2. The Z3 Theorem Prover receives an Oracle (e.g., standard `f32::sqrt`) and an error margin (`< 1%`).
3. Z3 solves for the exact coefficients that satisfy the error margin, allowing CovOpt to "rediscover" highly optimized mathematical approximations instantly.

### Thread/Hardware Topologies (Monte Carlo Annealing)
External ecosystems (e.g., `rayon`, `tokio`) often introduce thread pools where performance degrades at high counts due to Cache Thrashing or False Sharing.
- CovOpt treats variables like `chunk_size` or `thread_pool_size` as evolutionary parameters.
- It deploys Monte Carlo Annealing within the Double Chaos Sandbox.
- Under heavy load (like `zipfian_traffic`), it constantly perturbs these parameters, rapidly descending toward the global optimum for the specific CPU cache topology, bypassing the typical developer guesswork.

## Custom Fuzzers and Strict Bounds

The `#[covopt_evolve]` macro dictates the life-and-death criteria in the Double Chaos Sandbox.

```rust
#[covopt_evolve(bounds = "latency < 10us", fuzzer = "high_contention_no_std")]
pub struct UltraLowLatencyCache {
    // ...
}
```

- **Bounds**: The execution constraint. CovOpt measures empirical runs against this bound. If the evolved AST violates it, the Sandbox's Time Localizer immediately kills the thread, failing the generation.
- **Fuzzer**: Specifies the workload simulator (e.g., `zipfian_traffic`, `high_contention_no_std`). The Fuzzer Engine stresses the generated structure to uncover race conditions, thread starvation, or deadlocks.

If you are writing high-performance lock-free data structures, these bounds serve as the ultimate defense against regressions.
