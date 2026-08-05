use crate::io::universal_tensor::UniversalTensor;
use crate::science::chaos_state::{ChaosState, MicroTweak, RngState, step_forward_nd};

/// The Interface for external application plugins.
/// A biotech or semiconductor plugin implements this trait
/// to translate ModelGo's mathematical tensor into domain physics
/// and returns a generic fitness/loss scalar gradient.
pub trait ObjectiveFunction<const D: usize> {
    /// Evaluates the physical/chemical fitness of the given tensor state.
    /// Returns a scalar gradient where closer to 0.0 means optimal, and higher means worse.
    fn evaluate(&self, state: &UniversalTensor<D>) -> f32;
}

/// A wrapper around StagnationFeedback that bridges the ObjectiveFunction to ChaosState.
pub struct DomainGradientFeedback {
    pub last_gradient: f32,
}

impl crate::science::chaos_state::StagnationFeedback for DomainGradientFeedback {
    fn current_gradient(&self) -> f32 {
        // Clamp the domain gradient between 0.0 (fast progress) and 1.0 (stagnant)
        // to map it correctly into the Chaos tweaking equation.
        self.last_gradient.clamp(0.0, 1.0)
    }
}

/// The main Kernel layer for Universal Optimization.
/// N = branches/options (e.g. 10), D = dimensions of the tensor.
pub struct UniversalOptimizationEngine<const N: usize, const D: usize> {
    pub chaos: ChaosState<N, D>,
    pub rng: RngState,
    pub current_tweak: MicroTweak,
}

impl<const N: usize, const D: usize> UniversalOptimizationEngine<N, D> {
    pub fn new(initial: UniversalTensor<D>, seed: u32) -> Self {
        Self {
            chaos: ChaosState::new(initial.values),
            rng: RngState::new(seed),
            current_tweak: MicroTweak {
                s_exponent: 2.0, // balanced starting point
                max_elements: 1000,
            },
        }
    }

    /// Steps the chaotic engine forward by one iteration, optimizing the tensor
    /// based on the physical feedback of the injected ObjectiveFunction.
    pub fn step_evolution(&mut self, objective: &impl ObjectiveFunction<D>) -> UniversalTensor<D> {
        let current_tensor = UniversalTensor::new(self.chaos.base_values);
        let current_loss = objective.evaluate(&current_tensor);

        // Generate a mathematically simulated future state
        let candidate_chaos = step_forward_nd(&self.chaos, &self.current_tweak, &mut self.rng);
        let candidate_tensor = UniversalTensor::new(candidate_chaos.base_values);
        let candidate_loss = objective.evaluate(&candidate_tensor);

        // Zero-Order Rejection Sampling: Only accept if it descends the gradient
        if candidate_loss < current_loss {
            self.chaos = candidate_chaos;
            // Progress! Reduce chaos (stagnation = 0.0)
            self.chaos.adapt_tweak(
                &mut self.current_tweak,
                &DomainGradientFeedback { last_gradient: 0.0 },
            );
        } else {
            // Stagnant! Induce chaos (stagnation = 1.0) to jump out of local minimum
            self.chaos.adapt_tweak(
                &mut self.current_tweak,
                &DomainGradientFeedback { last_gradient: 1.0 },
            );
        }

        UniversalTensor::new(self.chaos.base_values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyObjective;
    impl ObjectiveFunction<1> for DummyObjective {
        fn evaluate(&self, state: &UniversalTensor<1>) -> f32 {
            (state.values[0] - 5.0).abs() // Optimal is 5.0
        }
    }

    #[test]
    fn test_domain_gradient_feedback() {
        use crate::science::chaos_state::StagnationFeedback;
        let feedback = DomainGradientFeedback { last_gradient: 1.5 };
        assert_eq!(feedback.current_gradient(), 1.0); // clamped
        let feedback2 = DomainGradientFeedback {
            last_gradient: -0.5,
        };
        assert_eq!(feedback2.current_gradient(), 0.0); // clamped
    }

    #[test]
    fn test_universal_optimization_engine() {
        let initial = UniversalTensor::<1>::new([0.0]);
        let mut engine = UniversalOptimizationEngine::<10, 1>::new(initial, 42);
        let objective = DummyObjective;

        let mut _best = initial.values[0];
        for _ in 0..10 {
            let res = engine.step_evolution(&objective);
            _best = res.values[0];
        }
    }
}
