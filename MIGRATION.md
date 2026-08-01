# Migrating from CovOpt 2.x to 3.0

CovOpt 3.0 is a major release because the public Rust configuration types,
macro execution contract, initialization behavior, and primary automation
workflow changed. Existing 2.x projects should migrate intentionally rather
than accepting it as a routine minor dependency update.

## Dependency versions

Keep the workspace family on one major version:

```toml
[dependencies]
covopt-schema = "3.0.0" # only when consuming metadata types directly

[dev-dependencies]
covopt-macro = "3.0.0"
```

Install the matching CLI:

```bash
cargo install CovOpt-Analyzer --version 3.0.0 --force
```

`covopt-schema` package version becomes 3.0.0, while the serialized metadata
constant remains `SCHEMA_VERSION = 1`. Consumers should check the wire constant
inside metadata rather than deriving it from the Cargo package version.

## Primary command model

The v3 autonomous entry point is:

```bash
covopt converge
```

It replaces agent-side orchestration of repeated inspect/optimize/fix/check
calls. `converge` defaults to transactional workspace apply. To retain a v2-like
advisory workflow during migration:

```bash
covopt converge --authority suggest --format json
```

The diagnostic command surface remains:

```text
init  check  inspect  optimize  fix  verify
```

Hidden compatibility aliases remain temporarily:

| 2.x command | 3.0 replacement |
| --- | --- |
| `ci` | `check` or `converge` depending on intent |
| `audit` | `check --mode strict` |
| `advise` | `inspect` |
| `report` | `check --format json|sarif|html` |
| `profile` | `verify runtime` |
| `harden` | `verify safety` |
| `fuzz` | `verify concurrency` or `optimize adversarial` |

Do not build new automation around these aliases.

## Configuration

Upgrade an older policy file with:

```bash
covopt init --migrate
```

CovOpt keeps `.covopt.toml.v2.bak`. V3 adds an optional project-owned GoalSpec:

```toml
[converge]
authority = "suggest" # use this first when migrating

[converge.budget]
wall_time_ms = 30000
max_iterations = 8
```

After reviewing DecisionBundles and rollback behavior, change authority to
`apply` or remove the override; `apply` is the v3 default.

`CovOptConfig` gained public fields, including `assurance`, `atomic`, `trials`,
`optimization`, `converge`, provider policies, and target discovery. External
Rust code constructing it with a struct literal must switch to TOML loading or
populate the new fields. Future-facing code should avoid exhaustive struct
literals for policy documents.

## Initialization side effects

`covopt init` now creates only `.covopt.toml`, and only when it does not already
exist. It no longer edits:

- `Cargo.toml`;
- `.gitignore`;
- `.agents` or agent rule files.

`covopt init --hook` preserves an existing pre-commit hook and manages one
marked block that runs `covopt check --staged --fast`.

Any project relying on generated agent files should own those files directly.

## `covopt_param!` behavior

Normal compilation now evaluates to the declared default expression. Merely
setting a similarly named environment variable does not alter production code.
Candidate injection is available only in explicit search/robustness modes;
confirmation also requires `COVOPT_CONFIRM_CANDIDATE_HASH`.

Prefer structured metadata:

```rust
const LIMIT: usize = covopt_macro::covopt_param!(
    "worker.limit",
    64usize,
    range = 1..=4096,
    class = "capacity",
    evaluation = "compile_time",
    scale = "pow2",
    unit = "items",
    risk = ["memory"]
);
```

Parameter classes and tags no longer imply different optimizers. Numeric
parameters share one seeded annealed Monte Carlo search engine.

## Evidence and apply semantics

- Risk routes stronger evidence instead of requiring an `allow-risk` switch.
- Unknown evaluators and unsupported candidate-bound providers fail closed.
- LLVM-MCA optimization evidence compares baseline and candidate; mere tool
  availability is not success.
- Coverage remains available but is not an implicit optimization objective.
- `suggestion_only` means a missing materializer, evaluator, contract,
  reproducibility boundary, or budget—not merely a high risk label.
- Automatic apply is workspace-only and never includes git or publishing.

Every convergence run persists `target/covopt/decision-bundle.json`. Archive
this file in CI when diagnosing an incomplete or rolled-back result.

## Migration checklist

1. Update all three CovOpt dependencies/tools to major version 3.
2. Run `covopt init --migrate` if a v2 policy exists.
3. Start with `covopt converge --authority suggest --format json`.
4. Inspect evaluator contracts, unresolved items, and candidate hashes.
5. Exercise `covopt fix --rollback <manifest>` on a disposable branch.
6. Enable default `apply` only after project tests cover required semantics.
7. Replace compatibility aliases in scripts and CI.
8. Pin seeds, bounds, and external tools needed for reproducible evidence.
