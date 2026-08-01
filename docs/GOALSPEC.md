# GoalSpec v1 reference (CovOpt 3.0)

`GoalSpec` describes what convergence should improve, what it must preserve,
how much work it may perform, and whether verified changes may be written. Its
serialized `schema_version` is independent from the Cargo package version.

## Loading and precedence

Lowest to highest precedence:

1. built-in defaults;
2. `[converge]` in `.covopt.toml`;
3. JSON/TOML passed through `covopt converge --spec PATH`;
4. CLI `--target`, repeated `--objective`, repeated `--constraint`, `--budget`,
   and `--authority` overrides.

Repeated CLI objectives replace inferred/spec objectives. Repeated constraints
are added if their ID is not already present.

## Defaults

```text
schema_version = 1
target.selector = "auto"
objectives = inferred from findings
budget.wall_time_ms = 30000
budget.max_iterations = 8
authority = "apply"
```

Default required constraints:

- `preserve-semantics`;
- `no-critical-safety-regression`;
- `no-evidence-strength-regression`.

## Complete JSON example

```json
{
  "schema_version": 1,
  "target": {
    "selector": "target",
    "value": "queue"
  },
  "objectives": [
    {
      "id": "queue-codegen",
      "metric": {
        "id": "codegen-overhead",
        "unit": "modeled-cycles"
      },
      "direction": "minimize",
      "weight": 1.0,
      "acceptance": {
        "operator": "improve"
      }
    }
  ],
  "constraints": [
    { "id": "preserve-semantics", "required": true },
    { "id": "no-critical-safety-regression", "required": true },
    { "id": "no-evidence-strength-regression", "required": true }
  ],
  "budget": {
    "wall_time_ms": 60000,
    "max_iterations": 8
  },
  "authority": "suggest",
  "team": "runtime"
}
```

`team` is not a built-in field; it is retained in the GoalSpec extension map.
Target, metric, objective, constraint, evaluator, acceptance, and budget
objects likewise accept extension fields.

## Target selector

| Selector | Value | Behavior |
| --- | --- | --- |
| `auto` | omitted | First configured/annotated target, then `src/lib.rs`, `src/main.rs`, or another Rust source |
| `path` | Rust file | Analyze that workspace-contained source |
| `target` | configured ID/test | Resolve through target metadata |
| `function` | function name/target | Resolve the source and filter function analysis |

Regardless of selector spelling, a value that is an existing file is resolved
as a path. Sources outside the current workspace are rejected.

## Objectives

An objective contains:

- stable clause `id`;
- open metric ID and optional unit/evaluator;
- direction: `minimize`, `maximize`, or `target`;
- finite positive weight;
- acceptance operator: currently `improve` or `no-regression`.

When objectives are omitted, findings infer these built-ins:

| Finding family | Inferred metric |
| --- | --- |
| Code generation | `codegen-overhead` |
| Memory layout | `memory-layout` |
| Manual CAS/ordering | `atomic-ordering` |
| Unsafe/guard scope | `safety` |
| Other runtime findings | `runtime-overhead` |

## Built-in evaluator registry

| Evaluator/metric IDs | Candidate-bound providers |
| --- | --- |
| `codegen-overhead`, `runtime-overhead`, `latency`, `reciprocal-throughput`, `ipc`, `code-size` | Static AST, compiler, LLVM-MCA baseline/candidate comparison |
| `memory-layout`, `field-locality`, `contention`, `no-memory-regression` | Static AST, compiler, tests plus deterministic layout model |
| `atomic-ordering` | Static AST, compiler, tests, bounded atomic model |
| `safety`, `preserve-semantics` | Static AST, compiler, tests |
| `no-critical-safety-regression` | Static AST comparison, compiler |
| `no-evidence-strength-regression` | Compiler plus every provider added by risk routing |
| `coverage`, `line-coverage` | Compiler and candidate-bound coverage |

Coverage is included only when requested by a goal/constraint; it is not an
implicit performance objective.

## Custom evaluator reference

```json
{
  "id": "vendor-check",
  "metric": {
    "id": "vendor.metric",
    "evaluator": {
      "id": "vendor.metric.v1",
      "version": 1,
      "provider": "Test",
      "config": {}
    }
  },
  "direction": "target"
}
```

Candidate-bound provider adapters currently exposed to GoalSpec are
`StaticAst`, `Compiler`, `Test`, `Coverage`, `Mca`, and `AtomicModel`. Naming an
unsupported provider or omitting an evaluator for an unknown metric produces
an unresolved goal. Custom config is preserved but never executed as an
arbitrary shell command.

## Authority

- `read-only`: analyze and verify; do not select/apply workspace edits.
- `suggest`: select verified candidates; do not write them.
- `apply`: transactionally write verified candidates and post-verify.

Authority does not include commit, push, publish, deployment, or external API
mutation. Risk is not authority: it adds evidence requirements.

## Fail-closed conditions

Convergence remains incomplete when it encounters an unknown evaluator,
unsupported candidate-bound provider, missing atomic/ABI/alignment contract,
missing deterministic materializer, stale source hash, failed evidence,
unreproducible environment, or exhausted budget. These conditions are recorded
in `DecisionBundle.unresolved` rather than converted to success.
