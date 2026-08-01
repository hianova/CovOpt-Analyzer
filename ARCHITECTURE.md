# CovOpt 3.0 architecture

## Design objective

CovOpt turns a developer goal into a reproducible decision, not an opaque code
rewrite. Every automatic change must connect four things:

1. an objective or required constraint;
2. a deterministic candidate materializer;
3. evidence executed against that exact candidate;
4. a recoverable workspace transaction.

When one link is missing, the result stays at the proof frontier as
`unresolved` or `suggestion_only`.

## Crate boundaries

| Crate | Boundary |
| --- | --- |
| `covopt-schema` | Stable identifiers and serializable target, evidence, atomic, and parameter metadata |
| `covopt-macro` | Transparent source annotations and explicit test/search adapters |
| `CovOpt-Analyzer` | Discovery, planning, evidence execution, optimization, convergence, transactions, and reports |

The package versions are synchronized at 3.0.0 for release management. The
serialized metadata protocol has its own integer `SCHEMA_VERSION`; package and
wire versions must not be conflated.

## Convergence state machine

```text
Discover
  -> Compile Goal
  -> Plan Evidence
  -> Execute / Generate
  -> Verify exact candidate
  -> Replan (when rejected or more work remains)
  -> Apply transaction
  -> Post-Verify
  -> Complete
```

`GoalSpec` is open at extension points: objective, metric, constraint, and
evaluator IDs are strings with flattened extension maps. Extensibility does not
mean optimistic fallback. An unknown evaluator without an executable
candidate-bound contract remains unresolved.

Authority is orthogonal to risk:

| Authority | Analysis/evidence | Workspace write |
| --- | --- | --- |
| `read-only` | Yes | Never |
| `suggest` | Yes | Never |
| `apply` | Yes | After verification |

Semantic/API/ABI risk routes candidates to stronger evidence. It does not
disable a candidate merely because its label is medium or high. A high-risk
candidate is withheld only when the specialized evaluator, correctness
contract, materializer, reproducibility boundary, or budget is unavailable.

## Evidence model

Evidence has explicit strength and scope. The core statuses are `Proven`,
`Modeled`, `Observed`, `Assumed`, `Unknown`, and `Failed`; providers have a
soundness ceiling, so compiler success or a bounded model cannot become an
unbounded proof.

| Provider | Establishes | Does not establish |
| --- | --- | --- |
| Static AST | Structure and modeled findings | Runtime behavior or UB absence |
| Compiler | Candidate compiles | Semantic equivalence |
| Test | Observed configured behavior | Exhaustive behavior |
| LLVM-MCA | Modeled instruction-level deltas | Cache misses or wall-clock latency |
| Coverage | Executed source regions | CPU overhead or correctness of unexecuted paths |
| Sanitizer | Observed failures in one run/configuration | Global UB freedom |
| Atomic model | Bounded contract result | Unbounded concurrency proof |
| Temporal/relational | Bounded trace property | Behavior outside the declared bound |
| Adversarial search | Seeded environment exploration | Exhaustive environment coverage |

Candidate MCA evidence compares baseline and patched functions. Availability
alone is insufficient: the candidate must improve at least one modeled metric
without regressing the guarded metrics.

## Candidate and transaction boundary

Each `SourceEdit` contains its original text and source hash. Verification:

1. copies the workspace without `.git` or build artifacts;
2. rejects edits escaping the workspace;
3. applies edits with hash and overlap checks;
4. parses changed Rust and parameter metadata;
5. runs compiler plus planned providers;
6. binds results to a candidate hash.

Apply then creates `target/covopt/transactions/<candidate>/`, stores complete
original files, and atomically replaces workspace files. Post-apply checks run
against the new workspace. Rollback refuses to overwrite files changed after
the transaction.

No convergence authority includes git commit/push, package publishing, network
deployment, or other external side effects.

## Optimizers

- **Inputs/seed selection**: deterministic budgeted selection.
- **Parameters**: one seeded annealed Monte Carlo kernel for all numeric
  classes; tags express domain and risk rather than selecting algorithms.
- **Atomic ordering**: legal ordering search gated by an explicit bounded
  correctness contract.
- **Code generation**: source/Cargo candidates, compiler fingerprints, and
  baseline/candidate assembly evidence.
- **Memory layout**: deterministic field/alignment materializers; public ABI or
  packed layouts require the missing specialized contract rather than a manual
  risk override.
- **Adversarial environments**: seeded, bounded runtime schedules and inputs.

## Artifacts

| Path | Purpose |
| --- | --- |
| `target/covopt/decision-bundle.json` | Goal, phases, candidates, evidence, transactions, replay, unresolved frontier |
| `target/covopt/transactions/*/manifest.json` | Source hashes, backups, and transaction state |
| `target/covopt/assurance-snapshot.json` | Versioned assurance state and proof frontier |
| `target/covopt/findings.json` | Shared structured findings |

See [GoalSpec](docs/GOALSPEC.md) and
[DecisionBundle](docs/DECISION_BUNDLE.md) for the serialized contracts.
