# Advanced CovOpt 3.0 workflows

## One-call agent workflow

Agents should normally invoke one command and consume one artifact:

```bash
covopt converge --format json
jq '{status, selected, unresolved, phases}' target/covopt/decision-bundle.json
```

Use `--authority suggest` for a verified patch plan without workspace writes,
or `--authority read-only` when even selecting an apply set is undesirable.
`apply` is the default and is limited to recoverable workspace transactions.

Lower-level commands remain useful for diagnosis:

```bash
covopt inspect --format json
covopt check --mode strict --format json
covopt optimize codegen --target process
covopt fix --plan --json
covopt verify safety --target process --sanitizer address
```

## Project-owned GoalSpec

Persist defaults in `.covopt.toml`:

```toml
[converge]
authority = "apply"

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

An external JSON/TOML GoalSpec passed with `--spec` takes precedence; target,
objective, constraint, budget, and authority CLI options override the loaded
document. Unknown custom fields are preserved in extension maps. Unknown
evaluator IDs do not pass unless they name a supported candidate-bound provider
contract. See [the complete reference](docs/GOALSPEC.md).

## Decision and recovery

Every completed run writes `target/covopt/decision-bundle.json`, including the
compiled evaluator contracts, required providers, exact verification records,
committed or rolled-back transactions, replay command, and unresolved frontier.

Inspect and recover a committed transaction:

```bash
jq '.transactions[] | {status, manifest_path, files}' \
  target/covopt/decision-bundle.json

covopt fix --rollback \
  target/covopt/transactions/<candidate>/manifest.json --json
```

Rollback succeeds only if current files still match the transaction's
post-apply hashes. This protects developer edits made after convergence.

## Parameter optimization

Declare a default plus an explicit domain:

```rust
use covopt_macro::covopt_param;

const QUEUE_CAPACITY: usize = covopt_param!(
    "queue.capacity",
    256usize,
    range = 16..=4096,
    class = "capacity",
    evaluation = "compile_time",
    scale = "pow2",
    unit = "items",
    risk = ["memory", "latency"]
);
```

Normal and `no_std` compilation uses `256`. Search and robustness modes are
explicit; confirmation requires a candidate hash, preventing an accidental
environment variable from changing production defaults.

```bash
covopt optimize parameters \
  --target queue_bench \
  --iterations 20 \
  --top-k 5 \
  --seed 7 \
  --json
```

All numeric classes use the same seeded annealed Monte Carlo engine. Class,
scale, unit, and risk tags constrain or explain the domain; they do not select
separate search algorithms.

## Target, temporal, and relational contracts

```rust
use covopt_macro::{covopt_atomic, covopt_evidence, covopt_target};

#[covopt_target(id = "worker", complexity = "O(N)", criticality = "high")]
fn worker() {}

#[covopt_evidence(target = "worker", seeds = "7,11", threads = [1, 2, 4])]
fn worker_evidence() {}

#[covopt_atomic(
    target = "worker",
    ordering = "acq-rel",
    liveness = "bounded",
    bounds = "threads=4,events=32"
)]
fn worker_atomic_contract() {}
```

Target-owned runtime contracts belong in `.covopt.toml`:

```toml
[target.worker]
test = "worker_trace"

[[target.worker.temporal]]
name = "eventually-completes"
operator = "eventually"
event = "completed"
bound = 64
fairness_assumption = "bounded scheduler fairness"
timeout_ms = 5000

[[target.worker.relational]]
name = "preserves-observations"
base = "tests/traces/worker-baseline.json"
observations = ["operation", "value"]
bound = 64
timeout_ms = 5000
```

The target test should emit runtime Trace IR through
`CovOpt_Analyzer::trace::write_trace_to_requested_path`. Missing contracts or
runtime traces are reported unavailable; static source similarity is never
reported as observed runtime equivalence.

## CI and machine-readable output

```bash
covopt check --staged --fast
covopt check --format json | jq '.targets[] | select(.passed == false)'
covopt check --format sarif
```

Diagnostics go to `stderr`; structured output stays on `stdout`. The managed
pre-commit block installed by `covopt init --hook` runs
`covopt check --staged --fast` and preserves existing hook content.

For deterministic automation, pin seeds, bounds, target CPU, and toolchain.
Record unavailable tools as an unresolved environment dependency instead of
weakening the evaluator contract.

## External tools

| Capability | Tool | Default role |
| --- | --- | --- |
| Compilation/tests | Cargo/Rust toolchain | Core |
| Assembly model | `llvm-mca` | Routed for codegen objectives/risk |
| Source coverage | `cargo llvm-cov`/LLVM tools | Explicit/fallback evidence |
| Sanitizers | Rust sanitizer toolchain | Explicit safety evidence |
| Runtime profiling | configured profiler | Optional diagnosis |
| Mutation/fuzzing | `cargo-mutants`, `cargo-fuzz` | Optional hardening |

CovOpt does not require flamegraphs or profiles merely to run convergence.
Memory-layout improvement needs an appropriate layout/workload contract; an
instruction model cannot invent cache-miss evidence.
