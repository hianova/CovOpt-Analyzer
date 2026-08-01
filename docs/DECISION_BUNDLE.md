# DecisionBundle v1 and recovery

Every successful invocation of the convergence engine returns a
`DecisionBundle`; the CLI persists it atomically at:

```text
target/covopt/decision-bundle.json
```

The bundle is the complete explanation and recovery surface for one run.

## Top-level fields

| Field | Meaning |
| --- | --- |
| `schema_version` | DecisionBundle serialization version |
| `status` | Final convergence outcome |
| `goal` | Fully resolved GoalSpec after config/CLI overrides and inference |
| `workspace`, `source`, `manifest` | Exact local scope |
| `phases` | Ordered state-machine transitions and elapsed time |
| `evaluator_contracts` | Clause-to-provider compilation |
| `initial_analysis`, `final_analysis` | Shared structured findings/candidates before and after |
| `candidate_decisions` | Eligibility, risk-routed providers, exact verification, rejection reason |
| `selected` | Candidate IDs selected by this run |
| `evidence_plans` | Candidate-bound action plans |
| `transactions` | Committed or rolled-back source transactions |
| `post_apply_evidence` | Verification performed after workspace writes |
| `unresolved` | Proof frontier with clause/candidate reasons |
| `replay` | Working directory, command, manifests, and rollback template |

## Status values

| Status | CLI success | Meaning |
| --- | --- | --- |
| `assessed` | Yes | Read-only assessment completed |
| `ready-to-apply` | Yes | Suggest authority found a verified candidate |
| `converged` | Yes | Apply authority reached a verified local frontier |
| `no-change` | Yes | No current finding required a change |
| `incomplete` | No | Required evaluator/materializer/evidence/budget remains unresolved |
| `rolled-back` | No | Applied candidate failed post-verification and was restored |
| `failed` | No | Convergence failed before a valid decision could complete |

`converged` does not claim an unbounded proof; `unresolved` and final findings
remain visible even when a local objective was satisfied.

## Candidate evidence binding

Verification records include:

- a candidate hash covering every edit;
- baseline source hashes;
- provider action IDs, commands, status, output, and actual cost;
- mandatory compiler result;
- failed action IDs.

Evidence from one candidate cannot be reused for another patch. Unsupported
providers fail rather than producing placeholder success.

## Transaction manifest

Committed transactions live under:

```text
target/covopt/transactions/<candidate-hash>/manifest.json
```

Each file record contains its relative workspace path, before/after hash, and
complete backup path. All files are validated and backed up before the first
workspace replacement.

Inspect transactions:

```bash
jq '.transactions[] | {status, candidate_hash, manifest_path, files}' \
  target/covopt/decision-bundle.json
```

## Automatic and manual rollback

Post-apply compiler/test/static regression failure automatically rolls back the
current transaction. Manual recovery uses:

```bash
covopt fix --rollback \
  target/covopt/transactions/<candidate-hash>/manifest.json \
  --json
```

Rollback is intentionally strict. It restores only a committed transaction and
only when every current file still has the recorded after-hash. If a developer
edited a file after convergence, rollback stops instead of overwriting that
work.

## CI retention

Archive the DecisionBundle and referenced transaction manifests for incomplete
or failed jobs. Do not treat the artifact as a portable patch: it contains
absolute workspace paths and local backup locations. The source/candidate
hashes and replay recipe are the reproducibility contract.
