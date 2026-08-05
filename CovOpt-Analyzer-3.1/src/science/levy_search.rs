use crate::science::chaos_state::{ChaosState, MicroTweak, RngState, step_forward_nd};
use std::time::Instant;

/// A generic interface for an objective function optimized by Chaotic Lévy Flight.
pub trait ChaoticObjective<const D: usize> {
    /// Evaluates the objective value for a given state. Higher is better (e.g. Yield).
    fn evaluate(&self, state: &[f32; D]) -> f32;

    /// Translates raw mathematical chaos displacements into domain-specific variables.
    /// `raw_chaos` contains values typically around -1.0 to 1.0 but with extreme Lévy fat tails.
    /// Modify `delta` to represent the physical/logical displacement.
    fn scale_displacement(&self, raw_chaos: &[f32; D], delta: &mut [f32; D]);

    /// Applies constraints to the state (e.g., clamp pH between 1 and 14).
    fn constrain_state(&self, state: &mut [f32; D]);
}

/// Generic Chaotic Lévy Flight Search Optimizer
pub struct LevyOptimizer {
    pub max_iters: usize,
    pub rng_seed: u32,
}

impl LevyOptimizer {
    pub fn new(max_iters: usize) -> Self {
        Self {
            max_iters,
            rng_seed: 0x98765432,
        }
    }

    /// Run the chaotic Lévy flight search on the given objective.
    pub fn optimize<const D: usize, O: ChaoticObjective<D>>(
        &self,
        objective: &O,
        initial_state: [f32; D],
    ) -> ([f32; D], f32, f32) {
        let start_time = Instant::now();
        let mut current_state = initial_state;
        objective.constrain_state(&mut current_state);

        let mut best_state = current_state;
        let mut best_yield = objective.evaluate(&current_state);
        let mut current_yield = best_yield;

        // Use 10 macro branches, D dimensional state
        let mut chaos_state = ChaosState::<10, D>::new([0.0; D]);
        let mut rng = RngState::new(self.rng_seed);
        let tweak = MicroTweak {
            max_elements: 1000,
            s_exponent: 1.5,
        };

        for _ in 0..self.max_iters {
            // Mathematical phase space mutation
            chaos_state = step_forward_nd(&chaos_state, &tweak, &mut rng);

            let mut delta = [0.0; D];
            objective.scale_displacement(&chaos_state.base_values, &mut delta);

            let mut next_state = current_state;
            for i in 0..D {
                next_state[i] += delta[i];
            }
            objective.constrain_state(&mut next_state);

            let new_yield = objective.evaluate(&next_state);
            let diff = new_yield - current_yield;

            // Calculate the magnitude of the chaos mutation vector
            let mut cv_sq = 0.0;
            for val in &chaos_state.base_values {
                cv_sq += val * val;
            }
            let cv = cv_sq.sqrt();

            // Simulated Annealing logic: Accept if better, OR if chaos jump is massive (escaping traps)
            let accept = if diff > 0.0 { true } else { cv > 1.5 };

            if accept {
                current_yield = new_yield;
                current_state = next_state;

                if new_yield > best_yield {
                    best_yield = new_yield;
                    best_state = next_state;
                }
            }

            // Reset base values so next jump is from the current location
            // The macro_weights (probability cloud) continue to evolve.
            chaos_state.base_values = [0.0; D];
        }

        (best_state, best_yield, start_time.elapsed().as_secs_f32())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyObjective;
    impl ChaoticObjective<1> for DummyObjective {
        fn evaluate(&self, state: &[f32; 1]) -> f32 {
            -(state[0] - 5.0).abs() // Peak at 5.0
        }
        fn scale_displacement(&self, raw_chaos: &[f32; 1], delta: &mut [f32; 1]) {
            delta[0] = raw_chaos[0];
        }
        fn constrain_state(&self, _state: &mut [f32; 1]) {}
    }

    #[test]
    fn test_levy_optimizer() {
        let opt = LevyOptimizer::new(10);
        let obj = DummyObjective;
        let (_, _, time) = opt.optimize(&obj, [0.0]);
        assert!(time >= 0.0);
    }
}
