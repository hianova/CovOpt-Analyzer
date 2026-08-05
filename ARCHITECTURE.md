# Architecture of CovOpt-Analyzer 3.1

CovOpt 3.1 is built around the **Ramanujan Pipeline**, an automated software engineering (ASE) engine that designs, relaxes, anneals, and fuzzes Rust source code automatically.

## The Ramanujan Pipeline (4 Phases)

CovOpt applies a strict 4-phase pipeline to evolve code:

### Phase 1: Target Scanning & AST Expansion
- The `#[covopt_evolve]` macro marks a struct or function as an evolutionary target.
- CovOpt scans for metadata boundaries (e.g. `bounds = "latency < 10us"`, `fuzzer = "zipfian_traffic"`).
- The existing structure is dissolved, preparing it to be replaced by a synthesized AST.

### Phase 2: Punnett Square Combinator
- **Flash LLM Architect**: CovOpt prompts a lightweight LLM with the fuzzer models and constraints. The LLM acts purely as an architectural prior, returning JSON that selects candidate components (e.g. `RwLock`, `LockFreeQueue`, `HashMap`).
- **Orthogonal Combination**: The `PunnettSquareMatrix` creates all combinations of these genes.
- **AST Glue Relaxation**: For each combination, CovOpt attempts to compile the code. If it fails due to glue-code errors (e.g., missing `.clone()`, `Box`, or `Into`), the system applies heuristic mutations and recompiles up to 100 generations in milliseconds.

### Phase 3: The Crucible (SMT + Annealing)
- **Z3 SMT Solver**: Magic numbers and constants (like `0x5f3759df` in FastInvSqrt) or optimal thread configurations are mapped to variables. CovOpt uses Z3 to formally verify error bounds and deduce exact constants.
- **Monte Carlo Annealing**: For non-linear hardware topologies (like CPU cache limits leading to false sharing), CovOpt treats variables like `chunk_size` and `thread_pool_size` as hyperparameters, annealing them to find the "sweet spot" that minimizes Cache Thrashing.

### Phase 4: Double Chaos Sandbox
- Evolved candidates are tossed into a rigorous, isolated sandbox.
- **Fuzzer Engine**: Bombards the candidate with highly contentious, concurrent load (e.g., readers/writers fighting for locks).
- **Time Localizer**: If a structure (e.g., a poorly tuned `rayon` pool) causes a deadlock or a thread freeze, the sandbox hits a strict time limit and massacres the candidate.
- Only the AST configuration that meets all constraints signs the **Survival Contract**, output to `decision-bundle.json`.

## Ecosystem Injection (Plugin Registry)

Introduced in 3.1, CovOpt is not limited to the standard library. By declaring a `.covopt.toml`:
```toml
[plugins]
[[plugins.external]]
crate_name = "no_std_tool"
genes = ["no_std_tool::qsbr::QsbrCell"]
```
CovOpt's `PluginRegistry` dynamically ingests these custom structures as `External(String)` variants into the `GenePool`. The Flash LLM is made aware of these external crates, allowing CovOpt to construct architectures using advanced ecosystem components like `Tokio`, `Rayon`, or `QSBR`.
