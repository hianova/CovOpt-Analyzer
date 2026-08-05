use crate::science::ScienceObjective;
use crate::science::assembly_funnel::{AssemblyFunnel, FunnelConfig};
use crate::science::auto_research::{AutoResearchConfig, AutoResearcher};
use std::time::SystemTime;

pub struct ChaosRunner;

impl ChaosRunner {
    pub fn launch<T: Clone + Send + Sync + 'static>(
        objective: impl ScienceObjective<T>,
        description: &str,
        tier1_population: usize,
    ) {
        println!("Initializing Chaos Engine...");

        let config_ar = AutoResearchConfig {
            mode: "Dual".to_string(),
        };

        let mut observer = AutoResearcher::new(config_ar).with_generation_log(true);

        println!("Igniting ModelGo AutoResearcher... {}", description);

        let config_loop = FunnelConfig {
            tier1_population,
            tier2_retention_ratio: 0.05,
            tier3_dfs_depth: 3,
            #[cfg(not(test))]
            stagnation_patience: 30,
            #[cfg(test)]
            stagnation_patience: 0,
            stagnation_delta: 0.5,
            rng_seed: SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as u32,
            min_slope_window: 0,
            min_slope_threshold: 0.0,
            hard_limit_gen: 0,
            hard_limit_score: u32::MAX,
            use_diffusion: true,
        };

        let mut funnel = AssemblyFunnel::new(config_loop);

        #[cfg(not(test))]
        funnel.run_infinite_mode(&objective, &mut observer);

        #[cfg(test)]
        funnel.run_evolution_loop(&objective, &mut observer);

        println!("Chaos Engine Optimization Complete.");
    }

    pub fn launch_tunable<T: Clone + Send + Sync + 'static>(
        objective: impl ScienceObjective<T>,
        config: FunnelConfig,
        description: &str,
    ) -> crate::science::assembly_funnel::EvolutionResult {
        let config_ar = AutoResearchConfig {
            mode: "Dual".to_string(),
        };
        let mut observer = AutoResearcher::new(config_ar).with_generation_log(true);
        println!("Igniting Tunable Chaos Engine... {}", description);

        let mut funnel = AssemblyFunnel::new(config);
        funnel.run_evolution_loop(&objective, &mut observer)
    }

    pub fn evaluate_nested<T: Clone + Send + Sync + 'static>(
        objective: impl ScienceObjective<T>,
        description: &str,
        population: usize,
        hard_limit_gen: u64,
        hard_limit_score: u32,
    ) -> u32 {
        let config = FunnelConfig {
            tier1_population: population,
            tier2_retention_ratio: 0.1,
            tier3_dfs_depth: 1,
            stagnation_patience: 3,
            stagnation_delta: 0.1,
            rng_seed: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as u32
                % 1000,
            min_slope_window: 0,
            min_slope_threshold: 0.0,
            hard_limit_gen,
            hard_limit_score,
            use_diffusion: true,
        };
        let result = Self::launch_tunable(objective, config, description);
        result.best_score()
    }

    /// Launches the Chaos Engine across multiple threads (Parallel Universes)
    pub fn launch_multicore<O, T>(
        objective: O,
        description: &str,
        tier1_population: usize,
        num_threads: usize,
    ) where
        O: ScienceObjective<T> + Clone + Send + Sync + 'static,
        T: Clone + Send + Sync + 'static,
    {
        println!("Igniting {} parallel universes...", num_threads);

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut handles = vec![];

            for _ in 0..num_threads {
                let obj_clone = objective.clone();
                let desc_clone = description.to_string();

                handles.push(tokio::task::spawn_blocking(move || {
                    let _tracker = no_std_tool::debug::ScopedResource::new();
                    Self::launch(obj_clone, &desc_clone, tier1_population);
                }));
            }

            for h in handles {
                let _ = tokio::time::timeout(std::time::Duration::from_secs(3600), h).await;
            }
        });
    }
}
