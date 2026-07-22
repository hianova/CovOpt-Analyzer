# RTK - Rust Token Killer (Google Antigravity)

**Usage**: Token-optimized CLI proxy for shell commands.

## Rule

Always prefix shell commands with `rtk` to minimize token consumption.

Examples:

```bash
rtk git status
rtk cargo test
rtk ls src/
rtk grep "pattern" src/
rtk find "*.rs" .
rtk docker ps
rtk gh pr list
```

## Meta Commands

```bash
rtk gain              # Show token savings
rtk gain --history    # Command history with savings
rtk discover          # Find missed RTK opportunities
rtk proxy <cmd>       # Run raw (no filtering, for debugging)
```

## Why

RTK filters and compresses command output before it reaches the LLM context, saving 60-90% tokens on common operations. Always use `rtk <cmd>` instead of raw commands.


# Some Other Agent Rule
Testing 123



# CovOpt Optimization & Tuning Rules (Google Antigravity)

**Usage**: AI Agent instructions for performance verification and parameter tuning.

## Rule 1: Zero-Entropy Tuning
- **NEVER** use hardcoded magical numbers for caching thresholds, buffer sizes, or critical performance parameters.
- **ALWAYS** use the `covopt_param!` macro to define these parameters.
- Before submitting performance-related code, you MUST run `covopt audit` and ensure the Entropy Score is `0.0`. If it is not, use `covopt optimize` or `covopt optimize --explore` to converge the parameters.

## Rule 2: Anti-DCE in Benchmarks
- When writing tests or benchmarks for `covopt` or `llvm-mca`, **ALWAYS** wrap loop variables and return values with `std::hint::black_box()`.
- This prevents LLVM Dead Code Elimination (DCE) from optimizing `O(N)` loops into `O(1)` during `--release` coverage builds.

## Rule 3: Lock-Free Critical Paths
- For extreme performance tiers (e.g., `< 50ns` p50 latency), **NEVER** use standard library `Mutex` or `RwLock` on the critical path.
- **ALWAYS** use QSBR (Quiescent State Based Reclamation) or Thread Local Storage (TLS) for thread-safe memory and state management.

## Rule 4: Strict Clippy Cleanliness
- All macro-generated code from `covopt_param!` must be strictly typed. **DO NOT** use `#[allow(...)]` to ignore `as u8` or `as u16` cast warnings. Fix the underlying type inference instead.

## Rule 5: No Magic Numbers
- All magic numbers must be uniformly defined using `covopt_parm!`.

## Available Commands (CovOpt-Analyzer)
- `covopt init`: Initializes a `.covopt.toml` and injects these rules. Use `--hook` to install a pre-commit hook.
- `covopt ci`: Unified Auto-Pilot Pipeline (Fix -> Audit -> Optimize).
- `covopt fix`: Automatically fix Clippy warnings and formatting.
- `covopt report`: Generate an HTML dashboard report.

- `covopt check audit`: Fast, low-entropy verification checking with quiet checklist output.
- `covopt check magic`: Scan Rust files for hardcoded magic numbers.
- `covopt check advise`: Advanced qualitative analysis. Detect micro-architectural pipeline stalls, God Functions, and cross-file Semantic Clones.

- `covopt tune params`: Performance Parameter Auto-Tuning & Optimization.
- `covopt tune profile`: Automatically parses flamegraph SVGs into text-based CPU hotspots for AI tuning.
- `covopt tune layout`: Tune struct memory layouts for cache efficiency.
- `covopt tune vectorize`: Scan for SIMD auto-vectorization opportunities.
- `covopt tune pgo`: Inject dynamic PGO (likely/unlikely) probes based on coverage.
- `covopt tune refactor`: Scaffold Advanced AI Refactoring (O(N^2) -> O(N log N)).

- `covopt harden run`: Robustness & Security Hardening (Mutation, Fuzzing, Sanitizers). Use `--sanitize --auto-fix` to automate self-healing.
- `covopt harden fuzz`: Generate fuzzing harnesses for public functions.

- `covopt --test <TEST> --expected <EXPECTED>`: Runs a direct mathematical complexity analysis on a specific test target.
- `covopt --help`: View all available commands and detailed usage instructions.

