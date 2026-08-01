# covopt-schema 3.0

Shared serializable metadata types used by `covopt-macro` and
`CovOpt-Analyzer`.

This crate contains data contracts, not the optimizer or procedural macros. It
is useful to tooling that reads CovOpt metadata without depending on the full
analyzer.

## Versioning

Two versions intentionally coexist:

- Cargo package version `3.0.0` tracks the compatible CovOpt release family.
- `covopt_schema::SCHEMA_VERSION` tracks the serialized metadata wire format.

The v3 release keeps `SCHEMA_VERSION == 1`; package changes did not require an
incompatible wire representation. Serialized producers must write the schema
constant into their envelope, and consumers must check it before interpreting
the payload.

## Types

- stable IDs: `TargetId`, `ParameterId`, `CouplingGroupId`;
- source contracts: `TargetDescriptor`, `EvidenceDescriptor`,
  `AtomicContractDescriptor`;
- parameter model: `ParameterDescriptor`, `ParameterValue`, `ParameterDomain`,
  `ParameterClass`, `ParameterTag`;
- optimizer state: `ParameterPhase`, `ParameterProperty`,
  `ParameterDisposition`, `ParameterRecord`;
- versioned payloads: `MetadataEnvelope<T>`.

## Usage

```toml
[dependencies]
covopt-schema = "3.0.0"
serde_json = "1"
```

```rust
use covopt_schema::{MetadataEnvelope, ParameterId, SCHEMA_VERSION};

let envelope = MetadataEnvelope {
    schema_version: SCHEMA_VERSION,
    value: ParameterId::new("queue.capacity"),
};

let json = serde_json::to_string(&envelope)?;
let decoded: MetadataEnvelope<ParameterId> = serde_json::from_str(&json)?;
assert_eq!(decoded.schema_version, SCHEMA_VERSION);
# Ok::<(), serde_json::Error>(())
```

## Compatibility rules

- Adding optional/defaulted serialized fields may retain the wire version.
- Renaming/removing fields, changing enum representation, or changing meaning
  requires a new `SCHEMA_VERSION` and an explicit migration path.
- Cargo major versions describe Rust API compatibility and coordinated CovOpt
  releases; they are not substituted for the wire version.
- Unknown schema versions must be rejected or preserved opaquely, never parsed
  as the current representation.

## Publishing order

`covopt-schema` must be published before `covopt-macro`, which must be published
before `CovOpt-Analyzer`, because crates.io resolves path dependencies by their
declared registry version during packaging.
