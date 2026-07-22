# CovOpt Optimization & Tuning Rules (Google Antigravity)

**Usage**: AI Agent instructions for performance verification and parameter tuning.

## Rule 1: Zero-Entropy Tuning
- **NEVER** use hardcoded magical numbers for caching thresholds, buffer sizes, or critical performance parameters.
- **ALWAYS** use the `covopt_param!` macro to define these parameters.

## Rule 2: Anti-DCE in Benchmarks
- When writing tests or benchmarks for `covopt` or `llvm-mca`, **ALWAYS** wrap loop variables and return values with `std::hint::black_box()`.
- This prevents LLVM Dead Code Elimination (DCE) from optimizing `O(N)` loops into `O(1)` during `--release` coverage builds.

## Rule 3: Lock-Free Critical Paths & fetch_update
- For extreme performance tiers (e.g., `< 50ns` p50 latency), **NEVER** use standard library `Mutex`, `RwLock`, or manual Spin-Locks on the critical path.
- **NEVER** use manual `compare_exchange` loops for atomic state transitions, as they cause thread starvation under contention. **ALWAYS** use `Atomic::fetch_update`.

## Rule 4: Cache Dynamics, False Sharing & Padding
- **NEVER** pass massive structs with many fields by value into hot loops.
- **ALWAYS** use `#[repr(align(64))]` for structs containing cross-thread atomic variables to avoid False Sharing and MESI cache line bouncing.
- Pack related small atomic variables into a single `AtomicU64` to prevent torn reads and branch overhead.

## Rule 5: Concurrency Disasters & Async Poisoning
- **NEVER** call blocking APIs (like `std::thread::sleep`, `std::fs::read`, or `Mutex::lock()`) inside an `async fn`. This blocks the async reactor (e.g., Tokio) and stalls the entire thread pool.

## Rule 6: Thread Physical Overbound
- **NEVER** spawn an unbounded number of threads inside a loop. Spawning unbounded threads shreds the L3 cache via massive OS context-switching overhead. Pre-allocate a ThreadPool bounded strictly by the physical CPU core count.

## Rule 7: Hidden Hot-Path Overheads & Allocations
- **NEVER** use implicit allocations (`.clone()`, `.to_string()`, `format!()`, `vec![]`) inside hot loops. This destroys Global Allocator performance and fragments memory. Use `Cow<str>`, `compact_str`, or pre-allocated arenas instead.

## Rule 8: Synchronous IO in Hot Paths
- Avoid synchronous IO (e.g., `println!`, `print!`) inside a hot loop. Synchronous IO completely stalls the CPU pipeline. Accumulate results and print them outside the loop.

## Rule 9: Generic Bloat & God Functions
- Split functions with cyclomatic complexity > 10. Extract cold paths (like error handling) into separate functions tagged with `#[inline(never)]`.
- **DO NOT** excessively use Generic type parameters on core application logic. If a generic function is instantiated > 3 times, consider using dynamic dispatch (`Box<dyn Trait>`) for cold paths.

## Rule 10: Branch Prediction Thrashing
- Heavily nested `if` statements traversing chaotic data will stall the CPU pipeline (low IPC). Sort data before the loop (`data.sort_unstable()`) or use branchless bitwise operations.
- Use `core::intrinsics::likely` or structure `if` branches linearly to instruct the compiler to layout the assembly code such that the hot path requires zero CPU branch jumps.

## Rule 11: Amortization & Asynchronous Decoupling
- For global contention points (like a global Arena allocator), use **Amortized Batch Allocation** (e.g., pulling 64 nodes at once) to reduce CAS contention and MESI invalidation broadcasts.
- Decouple Hit Rate tracking from Latency by shifting statistical updates to a background Daemon using asynchronous lock-free batching.

## Available Commands (CovOpt-Analyzer)
- `covopt init`: Initializes a `.covopt.toml` and injects these rules.
- `covopt check advise`: Advanced qualitative analysis. Detect micro-architectural pipeline stalls, God Functions, and Semantic Clones.
- `covopt tune params`: Performance Parameter Auto-Tuning & Optimization.
- `covopt harden run`: Robustness & Security Hardening (Mutation, Fuzzing, Sanitizers).
- `covopt --help`: View all available commands and detailed usage instructions.
