// pub mod boolean_relaxation;
// pub mod assembly_funnel;
// pub mod chaos_runner;
// pub mod discrete_diffusion;
pub mod funnel_pipeline;
pub mod levy_search;
pub mod plugins;

pub mod chaos_state;
// pub mod fft_chaos;
// pub mod sat_compiler;
// pub mod solver_pool;
// pub mod speculative_engine;
// pub mod storage;
// pub mod temporal_filter;
// pub mod universal_solver;

/// A generalized interface for physical, logical, or SAT/SMT constraints
pub trait GeneralizedValidator<T> {
    /// Validates a candidate against physical or logical constraints.
    /// Returns Ok(()) if it passes, or Err(&'static str) with the failure reason.
    fn validate(&self, candidate: &T) -> Result<(), &'static str>;
}
// pub mod coevolution_context;
pub mod motif_tree;
pub mod multipole_solver;
pub mod quantized_context;

pub mod crucible;
// pub use boolean_relaxation::*;
// pub use assembly_funnel::{AssemblyFunnel, FunnelObserver};
// pub use chaos_runner::ChaosRunner;

/// A generic interface representing a scientific exploration or optimization objective.
/// This allows the engine to run Physics, Math, Cybersecurity, or Biology without being coupled
/// to the specific domain.
pub trait ScienceObjective<T: Clone + Send + Sync>: Sync {
    /// Computes the fitness score of a candidate. Lower is better (e.g., Energy, or negative Yield).
    fn evaluate_fitness(&self, candidate: &T) -> (u32, u32);

    /// Evaluates a batch of candidates for vectorized or batched optimizations.
    /// By default, falls back to sequential evaluation.
    fn evaluate_fitness_batch(&self, candidates: &[T], out_fitness: &mut [(u32, u32)]) {
        out_fitness.iter_mut().enumerate().for_each(|(i, out)| {
            *out = self.evaluate_fitness(&candidates[i]);
        });
    }

    /// Generates a single initial seed (Tier 1).
    /// `parent` is provided if the system decides to mutate from an existing survivor.
    /// `seed` is a deterministic randomness parameter.
    fn generate_seed(&self, seed: usize, parent: Option<&T>) -> T;

    /// Applies a deep search perturbation (Tier 3).
    /// `scale` is the mutation severity derived dynamically from the Chaos engine (Lévy Flight).
    fn perturb(&self, candidate: &T, scale: f32, seed: usize) -> T;

    /// Evaluates hard validation constraints (e.g., spatial collisions, syntax validity).
    /// Returning `false` will immediately reject the candidate.
    fn is_valid(&self, candidate: &T) -> bool;

    /// Evaluates if the candidate meets the critical archival threshold (e.g., extremely low energy or successful exploit).
    /// This function should handle saving to database/logs.
    /// Returns `true` if the archival is successful and the current generational lineage should be terminated (Big Bang Reset).
    fn check_archival(&self, candidate: &T, fitness: (u32, u32)) -> bool;

    /// Periodically validates the best candidate and triggers Python visualizations.
    /// To prevent cache thrashing and IO bottlenecks, this is only called infrequently (e.g., every 120s).
    fn periodic_validate_and_visualize(&self, _candidate: &T) {}

    /// Invoked when a significant fitness jump is detected.
    /// This allows the objective to distill theoretical macros (e.g. LLM-Guided Symbolic Search).
    fn distill_theory(&self, _old_candidate: &T, _new_candidate: &T, _fitness_jump: u32) {}

    /// Performs a genetic crossover between two parents, producing multiple offspring combinations.
    /// Default implementation simply returns 4 clones to prevent breaking implementations that don't support it.
    fn crossover(&self, parent_a: &T, parent_b: &T, _seed: usize) -> [T; 4] {
        [
            parent_a.clone(),
            parent_b.clone(),
            parent_a.clone(),
            parent_b.clone(),
        ]
    }

    /// 引擎在遇到未知節點時，向 Archive 詢問是否有定理可以 O(1) 套用
    /// 如果成功套用捷徑，回傳 true
    fn apply_theory_shortcuts(&self, _candidate: &mut T) -> bool {
        false
    }

    /// 擴散生成核心：根據目前的雜訊比例 (noise_level: 1.0 = 全隨機, 0.0 = 完全去噪)
    /// 對拓撲結構進行單步修復與啟發式合法化。
    fn denoise_step(&self, _candidate: &mut T, _noise_level: f32, _seed: usize) {
        // 預設為空實作，供未支援的目標使用
    }
}
// pub mod auto_research;
pub mod guardrail;
// pub mod jit_compiler;
pub mod topology;
