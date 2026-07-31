# CovOpt CLI migration

The public command surface is now:

```text
init  check  inspect  optimize  fix  verify
```

For one major version, the old commands remain hidden compatibility aliases:

| Old command | Replacement |
| --- | --- |
| `ci` | `check` |
| `audit` | `check --mode strict` |
| `advise` | `inspect` |
| `report` | `check --format json|sarif|html` |
| `profile` | `verify runtime` |
| `harden` | `verify safety` |
| `fuzz` | `verify concurrency` |

Aliases print a migration hint and use the replacement implementation. Upgrade
legacy configuration with:

```bash
covopt init --migrate
```

The command keeps a `.covopt.toml.v2.bak` backup. V3 configuration expresses
assurance policies and provider modes; target and evidence metadata should live
in source annotations where possible.

New bounded assurance surfaces keep the same six-command public API:

```bash
covopt inspect --envelope --target foo
covopt inspect --frontier --target foo
covopt verify temporal --target foo --operator eventually --event return --bound 32
covopt verify relational --target foo --base path/to/baseline.rs
covopt optimize adversarial --target foo --budget 30s --seed 7
covopt inspect --assumptions --target foo
covopt inspect --drift --target foo
```

Provider modes may additionally name `temporal`, `relational`, and
`adversarial`. Their results are bounded evidence and remain distinct from
unbounded proof; every stochastic search must carry an explicit seed.
