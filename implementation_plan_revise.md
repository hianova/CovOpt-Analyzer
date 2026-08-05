# CovOpt 3.0: Universal Design Toolkit (The Holy Trinity)

This document outlines the final architectural blueprint for CovOpt 3.0. Going beyond an AST Evolutionary Engine for Rust, this system is a **Universal Design Toolkit**—a high-level life incubator. It reduces any design problem—whether software engineering, musical composition, or industrial architecture—into a domain-agnostic mathematical core of Graph Theory and Combinatorial Optimization.

## User Review Required
> [!IMPORTANT]
> The engineering execution plan has been completely inverted from Bottom-Up to Top-Down (Macro to Micro). Please review the new 5-Phase TODO list below.
> 
> Once you Approve, I will begin executing **Phase 0** and initialize `task.md`.

---

## The Execution Plan: Top-Down Incubator (由外而內、由宏觀到微觀)

Instead of blinding generating raw AST tokens, CovOpt 3.0 operates top-down: define boundaries, combine macroscopic structural genes (Punnett Square), mathematically fit parameters (Annealing/Z3), and mercilessly cull failures via physics/compilers.

### Phase 0: 創世介面 —— 實作 Macro 與雙重混沌定義 (UX & Boundary)
*Goal: Establish Specification-Driven Development (SDD), defining the exact "survival deadlines".*
- [ ] **Develop `#[covopt_evolve]` Macro**: Implement the procedural macro to intercept marked `trait` or `struct` definitions.
- [ ] **Define Chaos DSL**: Allow developers to specify physical boundaries (e.g., `mem < 50MB`, `latency < 5ms`) and Fuzzer attack models (e.g., `zipfian_traffic`, `random_thread_kill`) in the macro arguments.
- [ ] **Compile-Time Interceptor**: Pause `cargo build` when encountering the macro, handing over control and the `GoalSpec` to the CovOpt core engine.

### Phase 1: 建立基因庫 —— 完備結構庫與數學基底 (The Gene Pool)
*Goal: Eliminate meaningless microscopic brute-force search by providing macroscopic building blocks.*
- [ ] **Data & Concurrency Gene Pool**: Pre-write absolutely correct, verified AST templates (`HashMap`, `LockFreeQueue`, `Actor`, `RwLock`).
- [ ] **Math & Bitwise Gene Pool**: Introduce mathematical constant templates (`\pi`, `e`) and operators (`sin`, `cos`, Bitwise XOR/Shift).
- [ ] **Introduce E-Graphs (e.g., `egg` crate)**: Implement mathematical and logical rewrite rules (e.g., collapsing `x * 1` or `sin^2 + cos^2` to prevent combinatorial explosion).

### Phase 2: 旁氏表雜交 —— 宏觀組合與膠水代碼生長 (The Combinator)
*Goal: Find the optimal topological architecture using logarithmic complexity.*
- [ ] **Flash Gene Extractor (Prior Selector)**: Write prompts for the Flash model to read Phase 0 boundaries and "select" 3~5 highly probable candidate genes from Phase 1.
- [ ] **Punnett Square Matrix (旁氏表矩陣)**: Orthogonally combine the selected genes (e.g., `[Hash, Tree]` $\times$ `[Mutex, LockFree]`) to generate initial candidate AST skeletons.
- [ ] **100-Generation AST Glue Relaxation**: Allow minor, restricted AST mutations on the generated skeletons (strictly for interface alignment and type casting) with millisecond-level pruning via `cargo check`.

### Phase 3: 內層迴圈 —— 5 分鐘參數與魔法數字擬合 (The Inner Loop)
*Goal: Find the optimal mathematical solution within a fixed AST topology.*
- [ ] **Integrate the 5-Minute Pipeline**: Hook up the existing Annealed Monte Carlo engine to adjust `covopt_param!` values (e.g., Cache Capacity, Batch Size) within the generated AST.
- [ ] **Introduce SMT Solver (Z3)**: Call Z3 for constraint solving on bitwise or mathematical approximation ASTs to rigorously derive "Ramanujan Magic Numbers".
- [ ] **Bitwise Annealing Fallback**: If Z3 times out, gracefully degrade to Hamming-distance-based bit-flipping simulated annealing.

### Phase 4: 殘酷沙盒與驗收 —— 雙重混沌測試 (The Crucible)
*Goal: Use physical laws to determine survival and export the final result.*
- [ ] **Dynamic Sandbox**: Compile the parameter-fitted AST from Phase 3 into a binary and deploy it into an isolated environment.
- [ ] **Dual Chaos Strike**: Launch the Fuzzer with extreme traffic while monitoring OS/LLVM-MCA metrics (latency, memory, cache misses).
- [ ] **Survival Export**:
  - *If deadlines are violated*: Mark as Dead. Roll back to Phase 2 for the next Punnett Square combination.
  - *If survived*: Evolution Complete. Hot-swap the AST back into the original source code and generate `decision-bundle.json` (including the Flash model's architectural explanation report).

---

## Architectural Vision (架構願景)

At the mathematical core, all design acts involve finding optimal topological structures within discrete spaces bounded by strict rules and complex environments. CovOpt 3.0 provides the ultimate evolutionary engine for this process.

1. **The Core Engine (Domain-Agnostic)**: Encapsulated cleanly within `src/science`, this pure mathematics core handles Dual Chaos Observation, Monte Carlo Tree Search (MCTS), Simulated Annealing, and Boolean/Probabilistic Relaxation.
2. **Domain Plugins (領域插件)**: 
    - `plugin-rust`: Rust AST structural motifs + `rustc` / Borrow Checker constraints.
    - `plugin-music`: MIDI/Chord motif libraries + Acoustic/Music Theory constraints.
    - `plugin-architecture`: Geometric/Topological libraries + Physical Finite Element Analysis (FEA) constraints.
3. **Global Optimization Oracle**: The engine observes chaos, formulates mathematical constraints (Intent), constructs topologies from structural libraries (Builder), and enforces absolute physical laws (Crucible) to discover mathematically proven optimal designs.
