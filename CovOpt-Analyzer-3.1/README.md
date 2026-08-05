# CovOpt-Analyzer 3.0

Evidence-driven Rust analysis, optimization, and transactional convergence.
The package installs both `covopt` and the Cargo subcommand adapter
`cargo-covopt`.

Version 3 introduces an autonomous loop around an open `GoalSpec`. CovOpt
discovers current findings, generates deterministic candidates, routes risk to
the required evidence, verifies the exact patch in a copied workspace, and can
apply it with post-verification and automatic rollback.

## Install

```bash
cargo install CovOpt-Analyzer --version 3.0.0
```

Source annotations are optional. Projects that use them should add:

```toml
[dev-dependencies]
covopt-macro = "3.0.0"
```

## Start here

```bash
# Autonomous, candidate-verified, workspace-only apply (the default authority)
covopt converge

# Verify the same decision path without writing source
covopt converge --authority suggest --format json

# Inspect or verify lower-level evidence
covopt inspect --format json
covopt check --mode adaptive
```

Each convergence run writes
`target/covopt/decision-bundle.json`. It records the goal, state transitions,
evaluator contracts, candidates, exact evidence, transactions, replay command,
rollback command, and unresolved proof frontier.

## Commands

| Command | Purpose |
| --- | --- |
| `converge` | Compile a goal, verify candidates, optionally apply and post-verify |
| `init` | Optionally persist `.covopt.toml` or install the managed git hook |
| `check` | Plan and execute evidence for configured guarantees |
| `inspect` | Explain findings, candidates, envelopes, assumptions, and drift |
| `optimize` | Search inputs, parameters, atomic orderings, codegen, layout, or adversarial environments |
| `fix` | Run legacy fixers or plan/apply/rollback a repair transaction |
| `verify` | Force coverage, safety, concurrency, runtime, temporal, or relational evidence |

Legacy `ci`, `audit`, `advise`, `report`, `profile`, `harden`, and `fuzz`
commands are hidden compatibility aliases. New automation should not use them.

## GoalSpec

With no GoalSpec, CovOpt selects a source target, infers objectives from
findings, uses a 30-second/eight-iteration budget, and requires:

- semantic preservation;
- no new critical safety finding;
- no evidence-strength regression.

Project defaults may be stored in `.covopt.toml`:

```toml
[converge]
authority = "apply" # read-only | suggest | apply

[converge.target]
selector = "auto"

[converge.budget]
wall_time_ms = 30000
max_iterations = 8

[[converge.objectives]]
id = "codegen-overhead"
direction = "minimize"

[converge.objectives.metric]
id = "codegen-overhead"
```

An external JSON/TOML document may be passed through `--spec`. CLI target,
objective, constraint, budget, and authority options override loaded values.
Goal/evaluator IDs are open strings, but unknown evaluators fail closed unless
they bind a supported candidate-bound provider.

## Evidence boundaries

- Coverage measures source regions; it is not CPU overhead or proof of UB
  absence and is not a default optimization objective.
- LLVM-MCA models instruction behavior. Candidate evidence compares baseline
  and patch; no improvement or a guarded regression is rejected.
- Memory-layout changes cannot use MCA as invented cache-miss evidence; they
  require layout/workload evidence.
- Atomic, temporal, relational, and adversarial results state their finite
  bound, assumptions, seed, timeout, or counterexample.
- Unknown/unavailable evidence remains unresolved.

## Apply safety boundary

`apply` is intentionally useful, not a global side-effect permission. It may
write only source/config files inside the current workspace through a
source-hash-bound transaction. It never commits, pushes, publishes, deploys, or
contacts an external service.

Original files and the transaction manifest are stored under
`target/covopt/transactions/`. Post-apply failure triggers rollback. Manual
rollback refuses to overwrite a file changed after the transaction:

```bash
covopt fix --rollback target/covopt/transactions/<candidate>/manifest.json --json
```

## Optional tooling

Cargo and the Rust compiler provide the baseline. Depending on the selected
goal/evidence route, CovOpt may use `llvm-mca`, `cargo llvm-cov`, sanitizers,
profilers, `cargo-mutants`, or `cargo-fuzz`. Missing optional tooling does not
silently weaken a required evaluator.

Full architecture, GoalSpec, migration, and release documentation is available
in the [repository](https://github.com/hianova/CovOpt-Analyzer).

## License

MIT.
