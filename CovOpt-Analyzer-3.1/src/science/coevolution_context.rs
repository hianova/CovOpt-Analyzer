/// Decoupled Co-Evolution Interface representing the P vs NP boundary.
/// T is the Candidate (Generator), trying to adapt and survive.
/// E is the Environment (Adversary), trying to mutate and break the Candidate.
pub trait CoEvolutionObjective<T: Clone + Send + Sync, E: Clone + Send + Sync>: Sync {
    /// Evaluates the fitness of a candidate under a specific environment.
    /// Returns (candidate_fitness, environment_fitness).
    /// Typically:
    /// - Candidate minimizes `candidate_fitness` (e.g. energy).
    /// - Environment minimizes `environment_fitness` (e.g. inverse energy, to maximize candidate pain).
    fn evaluate_fitness(&self, candidate: &T, env: &E) -> (u32, u32);

    fn generate_candidate_seed(&self, seed: usize) -> T;
    fn generate_env_seed(&self, seed: usize) -> E;

    fn perturb_candidate(&self, candidate: &T, scale: f32, seed: usize) -> T;
    fn perturb_env(&self, env: &E, scale: f32, seed: usize) -> E;

    /// Triggered when the Environment fails to break the Candidate after N mutations (Nash Equilibrium).
    fn check_archival(&self, candidate: &T, env: &E, fitness: (u32, u32)) -> bool;
}

pub struct DualChaosRunner {
    pub max_generations: u64,
    /// Nash Equilibrium Threshold: How many failed environment mutations before we consider the candidate absolutely robust.
    pub nash_patience: usize,
}

impl Default for DualChaosRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl DualChaosRunner {
    pub fn new() -> Self {
        Self {
            max_generations: 10_000_000,
            nash_patience: 10_000,
        }
    }

    /// Launches the Min-Max Adversarial loop.
    pub fn launch<T, E>(&self, objective: impl CoEvolutionObjective<T, E>)
    where
        T: std::fmt::Debug + Clone + Send + Sync + 'static,
        E: std::fmt::Debug + Clone + Send + Sync + 'static,
    {
        println!("[Dual-Chaos] Igniting Co-Evolution Engine (Min-Max)...");

        let thread_id = std::thread::current().id();
        let offset = format!("{:?}", thread_id)
            .chars()
            .map(|c| c as usize)
            .sum::<usize>()
            * 1000000;
        let base_seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos() as usize
            + offset;

        // Initialize with random distinct seeds
        let mut candidate = objective.generate_candidate_seed(base_seed);
        let mut env = objective.generate_env_seed(base_seed.wrapping_add(42));

        let mut best_fitness = objective.evaluate_fitness(&candidate, &env);

        let mut cand_scale = 1.0f32;
        let mut env_scale = 1.0f32;
        let mut env_stagnation = 0;

        let start_time = std::time::Instant::now();

        for generation in 0..self.max_generations {
            let seed1 = base_seed.wrapping_add(generation as usize);
            let seed2 = base_seed.wrapping_add((generation * 17) as usize);

            // 1. Generator Step: Mutate Candidate to survive the current environment
            let next_cand = objective.perturb_candidate(&candidate, cand_scale, seed1);
            let next_fit = objective.evaluate_fitness(&next_cand, &env);

            if next_fit.0 < best_fitness.0 {
                // Candidate strictly improved!
                candidate = next_cand;
                best_fitness.0 = next_fit.0;
                cand_scale = (cand_scale * 0.9).max(0.1);
            } else {
                // Accept equal to allow random walk on flat landscapes
                if next_fit.0 == best_fitness.0 {
                    candidate = next_cand;
                }
                cand_scale = (cand_scale * 1.01).min(10.0);
            }

            // 2. Adversary Step: Mutate Environment to break the current candidate
            let next_env = objective.perturb_env(&env, env_scale, seed2);
            let env_test_fit = objective.evaluate_fitness(&candidate, &next_env);

            if env_test_fit.1 < best_fitness.1 {
                // Environment strictly improved
                env = next_env;
                best_fitness.1 = env_test_fit.1;
                best_fitness.0 = env_test_fit.0;
                env_stagnation = 0;
                env_scale = (env_scale * 0.9).max(0.1);
            } else {
                if env_test_fit.1 == best_fitness.1 {
                    env = next_env;
                    best_fitness.0 = env_test_fit.0;
                }
                env_stagnation += 1;
                env_scale = (env_scale * 1.01).min(10.0);
            }

            // 3. Nash Equilibrium Check
            // If the environment has tried `nash_patience` times and failed to break the candidate,
            // we have reached a localized Nash Equilibrium (Absolute Immunity).
            if env_stagnation > self.nash_patience {
                if objective.check_archival(&candidate, &env, best_fitness) {
                    println!(
                        "[Dual-Chaos] Absolute Robustness Achieved! Nash Equilibrium reached."
                    );
                    break;
                }
                candidate = objective.generate_candidate_seed(generation as usize);
                env = objective.generate_env_seed((generation * 13) as usize);
                best_fitness = objective.evaluate_fitness(&candidate, &env);
                cand_scale = 1.0;
                env_scale = 1.0;
                env_stagnation = 0;
            }

            if generation > 0 && generation % 100_000 == 0 {
                println!(
                    "[Dual-Chaos] Gen: {}, Cand Loss: {}, Env Loss: {}, Time: {:.2}s",
                    generation,
                    best_fitness.0,
                    best_fitness.1,
                    start_time.elapsed().as_secs_f32()
                );
            }
        }

        println!("[Dual-Chaos] Evolution Terminated.");
    }

    /// Launches the Min-Max Adversarial loop across multiple threads (Parallel Universes)
    pub fn launch_multicore<O, T, E>(&self, objective: O, num_threads: usize)
    where
        O: CoEvolutionObjective<T, E> + Clone + Send + Sync + 'static,
        T: std::fmt::Debug + Clone + Send + Sync + 'static,
        E: std::fmt::Debug + Clone + Send + Sync + 'static,
    {
        println!(
            "[Dual-Chaos] Igniting {} parallel universes...",
            num_threads
        );

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut handles = vec![];

            for _ in 0..num_threads {
                let obj_clone = objective.clone();
                // Clone the runner's configuration
                let runner_clone = Self {
                    max_generations: self.max_generations,
                    nash_patience: self.nash_patience,
                };

                handles.push(tokio::task::spawn_blocking(move || {
                    let _tracker = no_std_tool::debug::ScopedResource::new();
                    runner_clone.launch(obj_clone);
                }));
            }

            for h in handles {
                let _ = tokio::time::timeout(std::time::Duration::from_secs(3600), h).await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyCoEvolution;

    impl CoEvolutionObjective<f32, f32> for DummyCoEvolution {
        fn evaluate_fitness(&self, candidate: &f32, env: &f32) -> (u32, u32) {
            // Distance squared
            let dist = (candidate - env).abs();
            // Candidate wants to minimize distance
            let cand_fit = (dist * 1000.0) as u32;
            // Environment wants to maximize distance (minimize inverse)
            let env_fit = ((100.0 - dist).max(0.0) * 1000.0) as u32;
            (cand_fit, env_fit)
        }

        fn generate_candidate_seed(&self, _seed: usize) -> f32 {
            0.0
        }
        fn generate_env_seed(&self, _seed: usize) -> f32 {
            10.0
        }

        fn perturb_candidate(&self, candidate: &f32, scale: f32, seed: usize) -> f32 {
            let sign = if seed.is_multiple_of(2) { 1.0 } else { -1.0 };
            candidate + sign * scale
        }

        fn perturb_env(&self, env: &f32, scale: f32, seed: usize) -> f32 {
            let sign = if seed.is_multiple_of(2) { 1.0 } else { -1.0 };
            env + sign * scale
        }

        fn check_archival(&self, _candidate: &f32, _env: &f32, _fitness: (u32, u32)) -> bool {
            true // Terminate on first Nash Equilibrium
        }
    }

    #[test]
    fn test_dual_chaos_runner() {
        let runner = DualChaosRunner {
            max_generations: 10_000,
            nash_patience: 100,
        };
        runner.launch(DummyCoEvolution);
    }
}
