use crate::science::ScienceObjective;
use rayon::prelude::*;

/// A structural diffusion operator that simulates discrete block-diffusion
/// by generating pure noise and iteratively denoising it into structured topologies.
pub struct DiscreteDiffusionEngine {
    /// Number of denoising iterations (T)
    pub diffusion_steps: usize,
}

impl Default for DiscreteDiffusionEngine {
    fn default() -> Self {
        Self { diffusion_steps: 4 }
    }
}

impl DiscreteDiffusionEngine {
    pub fn new(diffusion_steps: usize) -> Self {
        Self { diffusion_steps }
    }

    /// Generates N candidates from pure noise, iteratively denoising them in parallel.
    pub fn generate_parallel_canvas<O, T>(
        &self,
        objective: &O,
        num_candidates: usize,
        mut base_seed: usize,
    ) -> Vec<T>
    where
        O: ScienceObjective<T> + Sync,
        T: Clone + Send + Sync,
    {
        if num_candidates == 0 {
            return Vec::new();
        }

        // Step 1: Initialize canvas with full noise (by passing None as parent)
        let mut seeds = Vec::with_capacity(num_candidates);
        for _ in 0..num_candidates {
            base_seed = base_seed.wrapping_mul(1664525).wrapping_add(1013904223);
            seeds.push(base_seed);
        }

        let mut canvas: Vec<T> = seeds
            .par_iter()
            .map(|&s| objective.generate_seed(s, None))
            .collect();

        // Step 2: Iterative Denoising (Diffusion steps)
        for step in 0..self.diffusion_steps {
            // noise_level goes from 1.0 (start) down to a fraction above 0.0
            let noise_level = 1.0 - (step as f32 / self.diffusion_steps as f32);

            canvas
                .par_iter_mut()
                .enumerate()
                .for_each(|(i, candidate)| {
                    // Mix in the step to the seed to keep it deterministic but varying per step
                    let step_seed = seeds[i].wrapping_add(step * 1234567);
                    objective.denoise_step(candidate, noise_level, step_seed);
                });
        }

        // Step 3: Snap into focus (Final structural cleanup with noise_level = 0.0)
        canvas
            .par_iter_mut()
            .enumerate()
            .for_each(|(i, candidate)| {
                let final_seed = seeds[i].wrapping_add(self.diffusion_steps * 1234567);
                objective.denoise_step(candidate, 0.0, final_seed);
            });

        canvas
    }
}
