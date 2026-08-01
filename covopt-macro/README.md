# covopt-macro 3.0

Procedural macros for declaring CovOpt targets, evidence axes, bounded atomic
contracts, parameter domains, and benchmark/test adapters.

The macros emit source-visible metadata consumed by `CovOpt-Analyzer`. They do
not require the analyzer at runtime.

## Install

```toml
[dev-dependencies]
covopt-macro = "3.0.0"
```

`covopt-macro` depends on the matching `covopt-schema` major version.

## Target and evidence annotations

```rust
use covopt_macro::{covopt_evidence, covopt_target, covopt_test};

#[covopt_target(id = "sort", complexity = "O(NlogN)", criticality = "normal")]
fn sort(values: &mut [u64]) {
    values.sort_unstable();
}

#[covopt_evidence(target = "sort", n = [64, 1024], seeds = "7,11")]
#[covopt_test(target_fn = "sort", expected = "ONlogN", n_values = "64,1024")]
fn sort_complexity(n: usize) {
    let mut values = (0..n as u64).rev().collect::<Vec<_>>();
    sort(&mut values);
}
```

- `#[covopt_target]` declares the stable target ID and expected contract.
- `#[covopt_evidence]` associates input/seed/thread/environment axes.
- `#[covopt_test]` creates a test adapter with injected `n`, optional `seed`,
  and optional `threads` arguments.
- `#[covopt_bench]` marks and black-boxes a benchmark body.

## Tunable parameters

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

Normal compilation expands to the declared default (`256` above). Search or
robustness candidate injection occurs only when the CovOpt driver explicitly
selects that mode. Confirmation additionally requires a candidate hash. This
keeps ordinary, `const`, and `no_std` builds independent from the optimizer.

Supported parameter classes include `threshold`, `capacity`, `budget`,
`timeout`, `retry`, `tolerance`, `coefficient`, `seed`, `layout`, and
`ordering`. Classes and tags describe domain/impact; all numeric classes use the
same optimizer.

## Atomic metadata

```rust
use covopt_macro::covopt_atomic;

#[covopt_atomic(
    target = "queue",
    ordering = "acq-rel",
    liveness = "bounded",
    forbidden_outcomes = "lost-item",
    bounds = "threads=4,events=32"
)]
fn queue_contract() {}
```

This annotation does not claim correctness. It records the explicit contract
used by bounded atomic synthesis and verification.

## QSBR registry adapter

`covopt_qsbr_registry!` generates a registry wrapper only when explicit
register and unregister functions are provided. It does not guess lifecycle
behavior. Refer to the macro rustdoc for the exact input grammar.

## Compile-time validation

Unknown fields, duplicate keys, invalid IDs/tags/ranges, unsupported generic
tests, and reversed domains are compile errors. Metadata embeds
`covopt_schema::SCHEMA_VERSION` so analyzer and macro compatibility is explicit.

Full project documentation:
[github.com/hianova/CovOpt-Analyzer](https://github.com/hianova/CovOpt-Analyzer).
