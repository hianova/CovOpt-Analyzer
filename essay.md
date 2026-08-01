# CovOpt 3.0: From heuristic advice to evidence-driven convergence

## Why another optimization tool?

Rust's compiler can reject invalid ownership and type relationships, while
tests can reject behavior represented by their inputs. Neither automatically
answers a different engineering question: given a performance or assurance
goal, which change should be attempted, what evidence is appropriate for its
risk, and can the exact change be recovered if later verification fails?

CovOpt 3.0 is organized around that question. It does not treat every signal as
equivalent. Source coverage, an AST warning, compiler success, a bounded atomic
model, a workload test, and an LLVM-MCA report each make different claims. The
tool's job is to compose those claims without exaggerating them.

## Goals before tactics

Earlier optimization workflows often exposed a collection of commands and
left an agent or developer to invent the control loop. That creates a steep
tool-use curve: the caller must know which analyzer to run, how to interpret its
output, when to replan, and when a patch is safe enough to write.

Version 3 introduces `covopt converge`. Its open GoalSpec identifies objectives,
constraints, budget, target, and authority. The IDs remain extensible, but each
required clause must compile to an evaluator contract. Unknown custom goals do
not receive optimistic defaults; they become an explicit proof-frontier item.

This distinction is important. Extensibility without evaluation is only an
unverifiable label. GoalSpec permits future metrics while preserving a truthful
current result.

## Risk is an evidence router

Many automatic tools use risk labels as a static permission gate. That appears
safe, but in practice it pushes medium/high-risk work back into an unstructured
manual path. CovOpt instead separates authority from risk.

Authority answers whether the workspace may be written: read-only, suggest, or
apply. Risk answers which evidence the candidate must produce. A code-generation
candidate may require baseline/candidate assembly modeling; an atomic change
requires a bounded correctness contract; a public ABI layout proposal remains
suggestion-only when the ABI evaluator is missing. The reason for withholding a
change is therefore testable and actionable, not simply "risk too high."

## Exact-candidate evidence

Evidence collected before a patch exists cannot verify that patch. CovOpt
materializes source-hash-bound edits, copies the workspace, applies those edits
in isolation, validates syntax and metadata, and then executes the planned
providers. Results include a hash of the candidate and original source hashes.

LLVM-MCA illustrates the policy. A candidate does not pass because llvm-mca ran
successfully. CovOpt models baseline and candidate functions, rejects guarded
metric regression, and requires at least one modeled improvement. Conversely,
MCA is not used to claim cache-miss improvement for a memory layout: instruction
modeling cannot manufacture workload locality evidence.

## Apply that can be undone

In v3, `converge` defaults to apply because useful automation should complete
ordinary in-scope work. This is not unlimited permission. Apply is confined to
the current workspace, and every file is validated and backed up before the
first replacement.

After commit of the source transaction, CovOpt checks the resulting workspace
again. Failure causes automatic rollback. Manual rollback is hash-guarded: it
will not overwrite developer changes made after convergence. Git commit, push,
crates.io publishing, deployment, and external APIs remain outside authority.

This gives "turbo mode" a precise meaning: maximize useful local function while
keeping side effects bounded and recoverable.

## Parameters without production coupling

`covopt_param!` declares a default and optional structured domain. Normal,
`const`, and `no_std` compilation retains the literal default. Search and
confirmation are explicit driver modes, and confirmation requires a candidate
hash. Adding tunability therefore does not silently make production behavior
depend on an environment variable or resident configuration service.

Parameter classes such as capacity, timeout, or threshold describe the domain
and impact. They do not select separate search engines. One seeded annealed
Monte Carlo kernel handles numeric exploration, making the algorithm easier to
reproduce and improve.

## The DecisionBundle as the interface

Agents need a smaller cognitive interface, not more raw commands. CovOpt emits
one DecisionBundle containing the resolved goal, state transitions, candidates,
evidence plans/results, transactions, replay data, and unresolved frontier.
Lower-level inspect, optimize, fix, check, and verify commands remain available
for diagnosis, while routine automation can consume a single artifact.

The bundle also preserves epistemic limits. A bounded model is labeled modeled,
a test is observed, and missing evidence stays unknown. Completion means the
declared local goal reached its verified frontier—not that every possible
program property has been proven.

## What v3 does not promise

CovOpt does not prove global UB freedom, turn line coverage into correctness,
derive cache behavior from assembly alone, exhaust all schedules, or guarantee
that a finite stochastic search found a global optimum. It records bounds,
seeds, assumptions, tool failures, and unsupported evaluators so those limits
can guide the next iteration.

The durable contribution of v3 is therefore not a magical optimizer. It is a
control architecture that lets static analysis, algorithms, runtime evidence,
and agents cooperate without hiding where certainty ends.
