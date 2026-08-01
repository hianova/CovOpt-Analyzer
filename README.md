# CovOpt-Analyzer 3.0

CovOpt is an evidence-driven Rust optimizer and assurance tool. Version 3 adds
a goal-driven convergence loop: it discovers a target, compiles an open
`GoalSpec`, generates repair candidates, verifies the exact patch in a sandbox,
and can apply it through a recoverable transaction.

CovOpt keeps different claims separate. Source coverage measures executed
regions; LLVM-MCA models target assembly; tests and sanitizers observe runtime
behavior; atomic, temporal, relational, and adversarial checks are bounded
evidence. None is silently promoted into a proof of something it cannot show.

## Workspace crates

| Crate | Purpose |
| --- | --- |
| `CovOpt-Analyzer` | `covopt` and `cargo-covopt` binaries plus the analysis library |
| `covopt-macro` | Source annotations, parameter metadata, benchmark/test adapters |
| `covopt-schema` | Shared Rust metadata types and wire-format version |

The three crates use package version `3.0.0`. The independent metadata wire
format remains `covopt_schema::SCHEMA_VERSION == 1` until its serialized shape
becomes incompatible.

## Install

```bash
cargo install CovOpt-Analyzer --version 3.0.0
```

Add annotations when the project needs declared targets or parameter domains:

```toml
[dev-dependencies]
covopt-macro = "3.0.0"
```

```rust
use covopt_macro::{covopt_evidence, covopt_param, covopt_target, covopt_test};

#[covopt_target(id = "process", complexity = "O(N)", criticality = "normal")]
pub fn process(n: usize) -> usize {
    let batch = covopt_param!(
        "process.batch",
        64usize,
        range = 1..=4096,
        class = "capacity",
        scale = "pow2"
    );
    (0..n).step_by(batch).sum()
}

#[covopt_evidence(target = "process", n = [64, 1024], seeds = "7,11")]
#[covopt_test(target_fn = "process", expected = "ON", n_values = "64,1024")]
fn process_complexity(n: usize) {
    std::hint::black_box(process(n));
}
```

`covopt_param!` compiles to its declared default in normal builds. Candidate
injection occurs only in explicit search/confirmation modes, so merely adding
the macro does not make production compilation depend on CovOpt.

## Primary workflow

```bash
covopt converge                                  # Default: transactional workspace apply
covopt converge --authority suggest --format json
covopt check --mode adaptive                     # Evaluate configured guarantees
covopt inspect --format json                     # Findings and candidates
covopt optimize parameters --target my_bench     # Seeded search
covopt verify temporal --target worker --operator eventually --event completed
```

`converge` defaults to `apply`, but its authority stops at the current
workspace. It does not commit, push, publish, or call an external service. A
candidate must pass candidate-bound evidence before a write; post-apply failure
causes automatic rollback. The complete outcome is written to
`target/covopt/decision-bundle.json`.

Initialization is optional. Without `.covopt.toml`, CovOpt uses embedded v3
defaults and source annotation discovery. `covopt init` only persists policy;
it does not edit `Cargo.toml`, `.gitignore`, or `.agents`.

## Evidence and dependency policy

- Static AST and compiler checks are the baseline.
- Risk selects stronger evidence; it is not a permission switch.
- LLVM-MCA is used for candidate-bound code-generation comparison when needed.
- Coverage is explicit evidence, not a default optimization target.
- Runtime profiling, sanitizers, and fuzzing remain optional providers.
- Unknown evaluators, missing contracts, unavailable tools, and non-materialized
  candidates remain `unresolved`; they never pass by fallback.

## Documentation

- [GoalSpec reference](docs/GOALSPEC.md)
- [DecisionBundle and rollback](docs/DECISION_BUNDLE.md)
- [Architecture](ARCHITECTURE.md)
- [Advanced workflows](ADVANCED.md)
- [Migration from 2.x](MIGRATION.md)
- [Release and crates.io procedure](RELEASING.md)
- [Changelog](CHANGELOG.md)

## License

MIT — see [LICENSE](LICENSE).
