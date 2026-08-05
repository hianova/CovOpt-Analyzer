# CovOpt-Analyzer 3.1 (Ecosystem Update)

CovOpt 3.1 is an evidence-driven, goal-oriented Rust optimizer and assurance tool that introduces the **Ramanujan Pipeline** and **Ecosystem Injection**. 

Instead of guessing optimizations, CovOpt uses a mathematical approach to structurally evolve your codebase. It discovers a target, uses Flash LLM to build architectural priors, combines them orthogonally via a Punnett Square Matrix, and then ignites **The Crucible (Z3 SMT Solver + Monte Carlo Annealing)** to mathematically fit parameters (like Thread Pool sizes, chunk boundaries, or magic numbers). Finally, it verifies the exact patch in a **Double Chaos Sandbox**, applying it through a recoverable transaction if it survives.

## Core Highlights

1. **The Crucible (Z3 + Annealing)**: Automatically solves for optimal constants (e.g., `chunk_size`, `FastInvSqrt` magic numbers) under constrained hardware topologies.
2. **Double Chaos Sandbox**: Forces the evolved structure to survive Fuzzer traffic and Strict Time Localizer boundaries. If a threading structure deadlocks, it's immediately killed.
3. **Ecosystem Plugin Registry (New in 3.1)**: CovOpt is no longer limited to the Rust Standard Library! You can inject external tools like `rayon`, `tokio`, or even your own `no_std` lock-free primitives (e.g. `QSBR`) directly into the gene pool via `.covopt.toml`.

## Workspace crates

| Crate | Purpose |
| --- | --- |
| `CovOpt-Analyzer` | `covopt` and `cargo-covopt` binaries plus the core Ramanujan Pipeline engine |
| `covopt-macro` | Source annotations (`#[covopt_evolve]`), parameter metadata, benchmark/test adapters |
| `covopt-schema` | Shared Rust metadata types and wire-format version |

## Install

```bash
cargo install CovOpt-Analyzer --version 3.1.0
```

Add annotations when the project needs declared targets or parameter domains:

```toml
[dev-dependencies]
covopt-macro = "3.1.0"
```

```rust
use covopt_macro::covopt_evolve;

#[covopt_evolve(bounds = "throughput > 10000", fuzzer = "zipfian_traffic")]
pub struct UltraLowLatencyCache {
    // Waiting for Flash Architect to inject optimal structures
}
```

## Zero-Invasive Ecosystem Injection (3.1)

You can easily register external crates to let the LLM and the Punnett Square Matrix evolve them. Just create a `.covopt.toml` in your project root:

```toml
[plugins]
[[plugins.external]]
crate_name = "rayon"
genes = ["rayon::ThreadPool"]

[[plugins.external]]
crate_name = "no_std_tool"
genes = ["no_std_tool::qsbr::QsbrCell"]
```
CovOpt will automatically ingest these as `External` genes, parse them, and put them through the ultimate survival test inside The Crucible!

## Documentation

- [Architecture (Ramanujan Pipeline)](ARCHITECTURE.md)
- [Advanced workflows (Z3 & Sandboxing)](ADVANCED.md)
- [Migration from 2.x](MIGRATION.md)
- [Release and crates.io procedure](RELEASING.md)

## License

MIT — see [LICENSE](LICENSE).
