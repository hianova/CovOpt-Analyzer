# Changelog

All notable changes are documented here. Package versions follow Semantic
Versioning; metadata artifacts carry independent schema versions.

## [3.0.0] - Unreleased

### Breaking

- Added the v3 public policy/configuration model and open GoalSpec types.
- Changed the primary automation model from agent-orchestrated command chains
  to `covopt converge`, whose default authority is transactional workspace
  apply.
- Changed `covopt init` to persist only `.covopt.toml`; it no longer edits
  Cargo/gitignore/agent files.
- Changed `covopt_param!` normal compilation to retain the declared default;
  candidate injection is limited to explicit search/confirmation modes.
- Split shared metadata into the independently published `covopt-schema` crate.
- Reworked structured findings, evidence planning, repair candidates, and
  public report/config types.

### Added

- Open `GoalSpec`, evaluator contracts, authority levels, and deterministic
  target/objective inference.
- Discover → Compile Goal → Plan Evidence → Generate → Verify → Replan → Apply
  → Post-Verify convergence state machine.
- Candidate-bound sandbox evidence with fail-closed provider adapters.
- Recoverable multi-file transactions and automatic post-apply rollback.
- Versioned DecisionBundle with replay and proof-frontier data.
- Proof frontier, robustness envelope, assumption/semantic drift, temporal,
  relational, adversarial, and bounded atomic evidence surfaces.
- Seeded input/seed selection and one annealed Monte Carlo parameter optimizer.
- Codegen/Cargo fingerprint candidates, baseline/candidate LLVM-MCA comparison,
  memory-layout materializers, and atomic ordering synthesis.
- Source annotations for targets, evidence axes, atomic contracts, benches,
  QSBR registries, and structured parameter domains.

### Changed

- Risk now routes to stronger evidence rather than acting as an apply
  permission switch.
- Coverage is an explicit/fallback provider instead of an implicit optimization
  target.
- Runtime profiling and hardening tools remain optional providers.
- Hidden 2.x commands delegate to v3 replacements and print migration hints.

### Compatibility

- Cargo packages: `CovOpt-Analyzer`, `covopt-macro`, and `covopt-schema` 3.0.0.
- Metadata wire format: `covopt_schema::SCHEMA_VERSION == 1`.
- DecisionBundle and GoalSpec serialization: schema version 1.

## [2.0.0]

- Published analyzer and procedural-macro baseline preceding the v3 evidence
  and convergence architecture.
