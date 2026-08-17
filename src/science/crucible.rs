//! The generic Crucible for optimizing any parameter space via Monte Carlo Annealing.

use serde::Serialize;
use std::sync::OnceLock;
use tokio::sync::broadcast;

#[derive(Clone, Debug, Serialize)]
pub struct CrucibleEvent {
    pub iteration: usize,
    pub temp: f64,
    pub fitness: f64,
    pub is_epiphany: bool,
}

pub static TELEMETRY_TX: OnceLock<broadcast::Sender<CrucibleEvent>> = OnceLock::new();

#[derive(Clone, Debug)]
pub struct Gene {
    pub name: String,
    pub bounds: (f64, f64),
    pub current_value: f64,
}

pub struct TheCrucible;

impl TheCrucible {
    // compute_novelty is removed! We rely purely on the fat-tail of chaos (Lévy Flight).

    pub fn anneal<F>(genes: Vec<Gene>, mut evaluate: F, iterations: usize) -> (f64, Vec<Gene>)
    where
        F: FnMut(&[Gene]) -> f64,
    {
        let (fit, _, g) = Self::anneal_with_sublime(
            genes,
            crate::science::oracle::DomainContext::Architecture { height: 0.5, stress: 0.5 },
            |genes_slice| (evaluate(genes_slice), 0.0),
            iterations,
        );
        (fit, g)
    }

    /// Takes a set of genes, an evaluation function returning (Primary Fitness, Sublime Metric),
    /// and optimizes them using Simulated Annealing with Aesthetic Epiphany.
    pub fn anneal_with_sublime<F>(
        mut genes: Vec<Gene>,
        domain_context: crate::science::oracle::DomainContext,
        mut evaluate: F,
        iterations: usize,
    ) -> (f64, f64, Vec<Gene>)
    where
        F: FnMut(&[Gene]) -> (f64, f64),
    {
        // Consult dynamic prior distribution
        let (initial_temp, bounds_scale) = {
            let mut oracle = crate::science::oracle::get_oracle().lock().unwrap();
            oracle.predict_prior(domain_context)
        };

        let final_temp = 0.001_f64;
        let cooling_rate = (final_temp / initial_temp).powf(1.0 / (iterations as f64));

        let (mut current_fitness, initial_sublime) = evaluate(&genes);
        let mut best_fitness = current_fitness;
        let mut best_sublime = initial_sublime;
        let mut best_genes = genes.clone();

        let mut current_temp = initial_temp;
        let tx = TELEMETRY_TX.get();

        for i in 0..iterations {
            let mut candidate_genes = genes.clone();

            // Perturb genes using Native Black Swan Engine (next_zipf from chaos_state)
            let mut is_black_swan = false;
            let tweak = crate::science::chaos_state::MicroTweak { s_exponent: 1.5, max_elements: 1 };
            let mut rng_state = crate::science::chaos_state::RngState::new(rand::random::<u32>());

            for gene in candidate_genes.iter_mut() {
                let range = gene.bounds.1 - gene.bounds.0;
                let max_step = range * (current_temp / initial_temp).max(0.01) * bounds_scale;
                
                // Native Black Swan Engine (Zipf distribution)
                let zipf_val = rng_state.next_zipf(&tweak) as f64;
                let sign = if rand::random::<bool>() { 1.0 } else { -1.0 };
                let zipf_multiplier = zipf_val * sign;
                
                // If the zipf value is capped at 10000.0, this is a massive Black Swan event!
                if zipf_val >= 9999.0 {
                    is_black_swan = true;
                }

                // Clamp the step to avoid extreme overflows destroying the parameters instantly
                let step = (zipf_multiplier * max_step * 0.001).clamp(-range, range);
                
                gene.current_value =
                    (gene.current_value + step).clamp(gene.bounds.0, gene.bounds.1);
            }

            let (candidate_fitness, candidate_sublime) = evaluate(&candidate_genes);
            let mut is_epiphany = false;

            // 1. Check for exploration breakthrough (Black Swan + High Harmony)
            if candidate_fitness > current_fitness && is_black_swan && candidate_sublime > 0.8 {
                is_epiphany = true;
                // Re-heat temperature to explore new region!
                current_temp = (current_temp * 5.0).min(initial_temp);

                genes = candidate_genes.clone();
                current_fitness = candidate_fitness;
            }

            // 2. Standard Acceptance
            if !is_epiphany {
                if candidate_fitness < current_fitness {
                    genes = candidate_genes.clone();
                    current_fitness = candidate_fitness;

                    if current_fitness < best_fitness {
                        best_fitness = current_fitness;
                        best_sublime = candidate_sublime;
                        best_genes = candidate_genes;
                    }
                } else {
                    let diff = candidate_fitness - current_fitness;
                    // To avoid NaN/Infinity if diff is too large and temp is tiny
                    let acceptance_prob = (-diff / current_temp).exp();
                    if rand::random::<f64>() < acceptance_prob {
                        genes = candidate_genes;
                        current_fitness = candidate_fitness;
                    }
                }
            }

            // No history buffer tracking needed anymore!

            current_temp *= cooling_rate;

            // Dispatch telemetry every 10 iterations (or instantly if epiphany)
            if i % 10 == 0 || is_epiphany {
                if let Some(sender) = tx {
                    let _ = sender.send(CrucibleEvent {
                        iteration: i,
                        temp: current_temp,
                        fitness: current_fitness,
                        is_epiphany,
                    });
                }
            }
        }

        // Feedback Loop: Send the best results back to Oracle to persist the Chaos State
        let is_epiphany = best_sublime > 0.8 && best_fitness > 0.8;
        {
            let mut oracle = crate::science::oracle::get_oracle().lock().unwrap();
            let seed = rand::random::<u64>(); // Capture the random seed that led to this epiphany
            oracle.learn_chaos(best_fitness, is_epiphany, final_temp, seed);
        }

        (best_fitness, best_sublime, best_genes)
    }
}
