# CovOpt Optimization & Tuning Rules (Google Antigravity)

**Usage**: AI Agent instructions for performance verification and parameter tuning.

## Core Rules to Prevent Audit Failures
1. **Zero-Entropy Tuning**: NEVER use hardcoded magical numbers. ALWAYS use `covopt_param!` macro. 
2. **Anti-DCE**: ALWAYS wrap loop variables with `std::hint::black_box()` in benchmarks to prevent O(N) -> O(1) DCE.
3. **Lock-Free Critical Paths**: NEVER use standard library `Mutex` or `RwLock` on the critical path.
4. **Strict Clippy Cleanliness**: DO NOT use `#[allow(...)]` to ignore type warnings for macro-generated code.

## Available Commands
- `covopt init`: Initialize a `.covopt.toml` and inject agent rules.
- `covopt audit`: Audit all targets for time/space complexity and IPC coverage.
- `covopt help`: View all other available commands and detailed usage.
