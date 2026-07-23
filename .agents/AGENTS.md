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

## Core Rules to Prevent Audit Failures
1. **Zero-Entropy Tuning**: NEVER use hardcoded magical numbers. ALWAYS use `covopt_param!` macro. 
2. **Anti-DCE**: ALWAYS wrap loop variables with `std::hint::black_box()` in benchmarks to prevent O(N) -> O(1) DCE.
3. **Lock-Free Critical Paths**: NEVER use standard library `Mutex` or `RwLock` on the critical path.
4. **Strict Clippy Cleanliness**: DO NOT use `#[allow(...)]` to ignore type warnings for macro-generated code.

## Available Commands
- `covopt init`: Initialize a `.covopt.toml` and inject agent rules.
- `covopt audit`: Audit all targets for time/space complexity and IPC coverage.
- `covopt help`: View all other available commands and detailed usage.

