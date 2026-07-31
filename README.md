# CovOpt-Analyzer 🚀

**CovOpt-Analyzer (Coverage-Optimized Complexity Analyzer & Auto-Tuner)** is an evidence-driven Rust analyzer. It discovers assurance obligations, plans the lowest-cost applicable evidence, checks complexity growth and safety risks, and verifies optimization or repair candidates before they can be applied.

LLVM source-based coverage supplies deterministic source-region execution counts. CovOpt fits their growth across configured inputs to classify asymptotic behavior; those counts are not CPU instructions, latency measurements, or a formal proof. LLVM-MCA, sanitizers, and runtime tools remain separate opt-in evidence providers.

---

## ✨ Key Features
- **Source-preserving instrumentation**: Coverage instrumentation is confined to analysis builds.
- **Complexity fitting**: Uses regression over source-region counts to detect Big-O regressions.
- **Senior Engineer Advisor**: Detects hot-path heap allocations, Tokio async blocks, and lock contention.
- **Planner-driven guarantees**: `covopt check` selects only the evidence providers required by policy.
- **Unified parameter search**: One seeded annealed Monte Carlo engine explores all numeric parameter classes; tags describe constraints rather than choosing algorithms.
- **Explicit evidence strength**: Reports distinguish `Proven`, `Modeled`, `Observed`, `Assumed`, `Unknown`, and `Failed`; bounded/static checks are never silently promoted to proofs.
- **Git Incremental Audit**: Native support for `--staged` and `--diff main`.

---

## ⚡ Quick Start

### 1. Installation
```bash
cargo install CovOpt-Analyzer
rustup component add llvm-tools-preview
```

### 2. Declare a Target and Evidence
Add to `Cargo.toml`:
```toml
[dev-dependencies]
covopt-macro = "2.0.0"
```

In your tests:
```rust
use covopt_macro::{covopt_evidence, covopt_target, covopt_test};

#[covopt_target(id = "process_data", complexity = "O(N)")]
pub fn process_data(n: usize) { /* ... */ }

#[covopt_evidence(target = "process_data", n = [1000, 5000], seeds = "adaptive")]
#[covopt_test(target_fn = "process_data", expected = "ON", n_values = "1000,5000")]
fn test_process_complexity(n: usize) {
    process_data(n);
}
```

### 3. Usage Cheat Sheet
```bash
covopt init                         # Setup policy and agent rules
covopt check --mode adaptive        # Check guarantees via the planner
covopt inspect --format json        # Explain findings and candidates
covopt optimize codegen             # Generate optimization candidates
covopt optimize parameters --target my_bench # Seeded parameter search
covopt fix --plan                   # Plan a minimal repair set
covopt verify coverage --target foo # Force a dynamic provider
```

Legacy commands (`ci`, `audit`, `advise`, `report`, `profile`, `harden`, and
`fuzz`) remain hidden compatibility aliases for one major version and print a
migration hint. Use `covopt init --migrate` to upgrade an older configuration.

---

## 📚 Documentation
For deeper dives into the architecture and advanced agentic workflows:
- [Architecture & Tech Stack](ARCHITECTURE.md)
- [Advanced Usage & AI Piping](ADVANCED.md)

## 📜 License
MIT License - see [LICENSE](LICENSE) for details.
