# CovOpt 3.0: Universal Design Toolkit (The Holy Trinity)

This document outlines the final architectural blueprint for CovOpt 3.0. Going beyond an AST Evolutionary Engine for Rust, this system is a **Universal Design Toolkit**. It reduces any design problem—whether software engineering, musical composition, or industrial architecture—into a domain-agnostic mathematical core of Graph Theory and Combinatorial Optimization.

## User Review Required
> [!IMPORTANT]
> Please review the planned refactoring of `src/science` to separate the core math engine from domain plugins, and the renaming of `asic_objective`.
> 
> Once you Approve, I will execute the refactoring described in **Phase 0**.

## Phase 0: Science Architecture Refactoring (確認與補齊 `src/science`)

Based on our final architecture review, `src/science` is currently missing the clean boundary between the **Domain-Agnostic Core** and **Domain Plugins**. We will perform the following structural refactoring:

### 1. Renaming `asic_objective` to `boolean_relaxation`
The continuous/differentiable logic gate operations currently inside `asic_objective.rs` represent the core of Boolean Relaxation. 
- Rename `src/science/asic_objective.rs` to `src/science/boolean_relaxation.rs`.
- Update references in `mod.rs` and `sat_compiler.rs`.
- Rename `AsicObjective` to `BooleanRelaxationObjective` to reflect its true mathematical nature.

### 2. Missing Component: The `plugins/` Encapsulation
Currently, domain logic (`math_objective.rs`, `emergent_objective.rs`, `quantum/`) is mixed with the pure math engine (`universal_solver.rs`, `discrete_diffusion.rs`). We need to establish the plugin boundary.
- **Action**: Create a `src/science/plugins/` directory.
- **Action**: Move existing domain-specific modules into `plugins/`.
- **Action**: Create stub modules for the Holy Trinity vision:
  - `src/science/plugins/plugin_rust/` (AST constraints and `rustc` bounds)
  - `src/science/plugins/plugin_music/` (Acoustic and music theory constraints)
  - `src/science/plugins/plugin_architecture/` (Topological and FEA constraints)

---

## Architectural Vision (架構願景)

At the mathematical core, all design acts involve finding optimal topological structures within discrete spaces bounded by strict rules and complex environments. CovOpt 3.0 provides the ultimate evolutionary engine for this process.

1. **The Core Engine (Domain-Agnostic)**: Encapsulated cleanly within `src/science`, this pure mathematics core handles Dual Chaos Observation, Monte Carlo Tree Search (MCTS), Simulated Annealing, and Boolean/Probabilistic Relaxation.
2. **Domain Plugins (領域插件)**: 
    - `plugin-rust`: Rust AST structural motifs + `rustc` / Borrow Checker constraints.
    - `plugin-music`: MIDI/Chord motif libraries + Acoustic/Music Theory constraints.
    - `plugin-architecture`: Geometric/Topological libraries + Physical Finite Element Analysis (FEA) constraints.
3. **Global Optimization Oracle**: The engine observes chaos, formulates mathematical constraints (Intent), constructs topologies from structural libraries (Builder), and enforces absolute physical laws (Crucible) to discover mathematically proven optimal designs.

---

## The Three-Layer Architecture (The Holy Trinity)

The system operates across three distinct layers. The core mechanics remain entirely domain-agnostic, while domain plugins define the specific constraints and structural libraries.

### 1. Top Layer: The Architect (意圖與契約層)

*Objective: Observe dual chaos and generate survival contracts without human intervention.*

- **Driver**: Dual Chaos (Adversarial / Coupled Chaos) + Flash LLM Model.
    - **Chaos A (Environmental/Physical Disorder)**: Hardware faults in software, dissonance/frequency clashes in music, or earthquakes/wind loads in architecture.
    - **Chaos B (Demand/Behavioral Disorder)**: Malicious packets in software, unexpected tension/release expectations in music, or unpredictable human crowd movement in architecture.
- **Mechanism**: The Flash model acts as a high-level observer of these colliding chaotic systems. It formulates a **"Survival Contract"** (GoalSpec)—such as a Rust `pub trait`, a target BPM/emotional arc, or a structural boundary condition (e.g., survive an 8.0 earthquake).
- **Output**: Formal definitions of boundaries and property-based rules.

---

### 2. Middle Layer: The Builder (演化與組合層)

*Objective: Assemble and fine-tune topological structures to fulfill the Architect's contract.*

- **Driver**: Monte Carlo Tree Search (MCTS) + Simulated Annealing (SA) + Genetic Algorithms (GA) + Continuous Relaxation.
- **Mechanism**: 
    - Receives strict boundary contracts from the Top Layer.
    - Pulls macroscopic building blocks from the **Complete Structural Library** (e.g., Actors/B-Trees for code, Sonata forms/II-V-I progressions for music, Honeycombs/Trusses for architecture).
    - Utilizes continuous relaxation (mapping discrete topological jumps into differentiable continuous spaces) to calculate smooth probabilities for structural mutations.
    - Performs combinatorics without brute-force random generation.
- **Output**: A population of Candidate Topologies (Probabilistic structures instantiated into concrete forms).

---

### 3. Bottom Layer: The Crucible (殘酷淘汰層)

*Objective: Prune unviable architectures using absolute physical and logical laws.*

- **Driver**: Domain-Specific Physical Constraints (The Evaluator).
- **Mechanism**:
    1. **Speed-of-Light Pruning**: Candidates face absolute physical laws. `rustc` borrow-checker kills bad memory access; Music Theory constraints prune extreme dissonance; Physics Engines (FEA) shatter structurally unsound buildings. 
    2. **Crucible Sandbox**: Surviving structures are subjected to simulated Dual Chaos (Fuzzing, acoustic resonance simulations, disaster simulations).
    3. **Performance Constraint**: Evaluated for ultimate efficiency (LLVM-MCA ports, acoustic harmony, minimal material stress).
- **Output**: The ultimate `decision-bundle.json`. A mathematically optimal, physically robust topology that has survived evolutionary pressure, proving exactly how the system *must* be structured.
