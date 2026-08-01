# Releasing CovOpt 3.0 to crates.io

This repository publishes three crates with registry dependencies. Publishing
must follow dependency order.

## Release order

1. `covopt-schema`
2. `covopt-macro`
3. `CovOpt-Analyzer`

Path dependencies are retained for workspace development, but crates.io removes
the path and resolves the declared version. Consequently macro/analyzer package
verification cannot succeed until the preceding crate version is visible in
the index.

## Preflight

From a clean release commit:

```bash
rg 'version = "3.0.0"' \
  covopt-schema/Cargo.toml \
  covopt-macro/Cargo.toml \
  CovOpt-Analyzer/Cargo.toml

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets
cargo doc --workspace --no-deps

cargo package -p covopt-schema
```

Review packaged files and metadata:

```bash
cargo package -p covopt-schema --list
cargo package -p covopt-macro --list
cargo package -p CovOpt-Analyzer --list
```

Confirm no secrets, local artifacts, `.agents`, `.covopt`, transaction backups,
or generated reports are included.

## Publish

Publishing changes external state. Run these only after the release commit and
credentials are confirmed:

```bash
cargo publish -p covopt-schema
```

Wait until `cargo search covopt-schema` shows 3.0.0, then:

```bash
cargo publish -p covopt-macro
```

Wait until `cargo search covopt-macro` shows 3.0.0, then:

```bash
cargo publish -p CovOpt-Analyzer
```

Do not use `--no-verify` to bypass an unavailable dependency. Index propagation
is part of release correctness.

## Post-publish verification

Use a directory outside the repository so path dependencies cannot mask the
registry packages:

```bash
release_check_dir="$(mktemp -d)"
cd "$release_check_dir"

cargo install CovOpt-Analyzer --version 3.0.0 --locked
covopt --version
covopt converge --help

cargo new consumer
cd consumer
cargo add covopt-schema@3.0.0
cargo add covopt-macro@3.0.0 --dev
cargo check --all-targets
```

Then tag the exact published commit:

```bash
git tag -a v3.0.0 -m "CovOpt 3.0.0"
git push origin v3.0.0
```

## Version invariants

- All three Cargo package versions and internal dependency requirements must
  agree on 3.0.0.
- `SCHEMA_VERSION` changes only for an incompatible serialized metadata change.
- GoalSpec/DecisionBundle schema versions change only for incompatible document
  changes.
- Release notes must name any new default authority, external dependency, or
  migration requirement.
