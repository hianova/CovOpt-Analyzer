//! Shared deterministic search kernel for CovOpt optimization domains.
//!
//! Domain modules provide only state mutation and objective evaluation.  Parameter
//! classes and tags remain metadata; they never select a different search algorithm.

use std::cmp::Ordering;

#[derive(Debug, Clone, Copy)]
pub struct AnnealingConfig {
    pub iterations: usize,
    pub initial_temperature: f64,
    pub final_temperature: f64,
    pub top_k: usize,
}

impl AnnealingConfig {
    pub fn validate(self) -> Result<Self, String> {
        if self.iterations == 0 {
            return Err("annealing iterations must be greater than zero".to_string());
        }
        if self.top_k == 0 {
            return Err("annealing top_k must be greater than zero".to_string());
        }
        if !self.initial_temperature.is_finite()
            || !self.final_temperature.is_finite()
            || self.initial_temperature <= 0.0
            || self.final_temperature <= 0.0
            || self.final_temperature > self.initial_temperature
        {
            return Err(
                "annealing temperatures must be finite, positive, and monotonically cooling"
                    .to_string(),
            );
        }
        Ok(self)
    }
}

#[derive(Debug, Clone)]
pub struct SearchSample<State> {
    pub state: State,
    pub score: f64,
    pub iteration: usize,
}

#[derive(Debug, Clone)]
pub struct SearchOutcome<State> {
    pub best: Option<SearchSample<State>>,
    pub shortlist: Vec<SearchSample<State>>,
    pub evaluated: usize,
    pub accepted: usize,
}

/// Small deterministic generator so a seed completely reproduces a search.
/// This is an implementation detail of the search engine, not a strategy choice.
#[derive(Debug, Clone)]
pub struct SearchRng {
    state: u64,
}

impl SearchRng {
    pub fn new(seed: u64) -> Self {
        // A zero state is valid for the LCG, but mixing the seed avoids a visibly
        // special first proposal while retaining exact reproducibility.
        const SEED_MIX: u64 = 0x9e37_79b9_7f4a_7c15;
        Self {
            state: seed ^ SEED_MIX,
        }
    }

    pub fn unit(&mut self) -> f64 {
        const MULTIPLIER: u64 = 6_364_136_223_846_793_005;
        const INCREMENT: u64 = 1;
        const MANTISSA_BITS: u32 = 53;
        self.state = self.state.wrapping_mul(MULTIPLIER).wrapping_add(INCREMENT);
        (self.state >> (u64::BITS - MANTISSA_BITS)) as f64 / ((1_u64 << MANTISSA_BITS) as f64)
    }

    pub fn signed_unit(&mut self) -> f64 {
        const UNIT_WIDTH: f64 = 2.0;
        self.unit() * UNIT_WIDTH - 1.0
    }

    pub fn index(&mut self, length: usize) -> usize {
        debug_assert!(length > 0);
        ((self.unit() * length as f64) as usize).min(length.saturating_sub(1))
    }
}

/// Maximize an objective with one deterministic simulated-annealing walk.
///
/// Invalid evaluations (`None`, NaN, or infinity) are rejected and remain visible
/// through the evaluated count.  The caller owns all domain constraints inside
/// `propose`; the engine owns cooling, Metropolis acceptance, and top-K retention.
pub fn annealed_monte_carlo<State, Propose, Evaluate>(
    initial: State,
    seed: u64,
    config: AnnealingConfig,
    mut propose: Propose,
    mut evaluate: Evaluate,
) -> Result<SearchOutcome<State>, String>
where
    State: Clone,
    Propose: FnMut(&State, f64, &mut SearchRng) -> State,
    Evaluate: FnMut(&State) -> Option<f64>,
{
    let config = config.validate()?;
    let mut rng = SearchRng::new(seed);
    let mut evaluated = 0;
    let mut accepted = 0;
    let mut samples = Vec::new();
    let mut current = initial;
    let mut current_score = evaluate(&current).filter(|score| score.is_finite());
    evaluated += 1;
    if let Some(score) = current_score {
        samples.push(SearchSample {
            state: current.clone(),
            score,
            iteration: 0,
        });
    }

    for iteration in 1..config.iterations {
        let progress = iteration as f64 / config.iterations.saturating_sub(1).max(1) as f64;
        let temperature = config.initial_temperature
            * (config.final_temperature / config.initial_temperature).powf(progress);
        let candidate = propose(&current, temperature, &mut rng);
        let candidate_score = evaluate(&candidate).filter(|score| score.is_finite());
        evaluated += 1;
        let Some(candidate_score) = candidate_score else {
            continue;
        };
        samples.push(SearchSample {
            state: candidate.clone(),
            score: candidate_score,
            iteration,
        });

        let accept = match current_score {
            None => true,
            Some(score) if candidate_score >= score => true,
            Some(score) => {
                let score_scale = score.abs().max(candidate_score.abs()).max(f64::EPSILON);
                let normalized_delta = (candidate_score - score) / score_scale;
                rng.unit() < (normalized_delta / temperature).exp()
            }
        };
        if accept {
            current = candidate;
            current_score = Some(candidate_score);
            accepted += 1;
        }
    }

    samples.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(Ordering::Equal)
            .then(left.iteration.cmp(&right.iteration))
    });
    let best = samples.first().cloned();
    samples.truncate(config.top_k);
    Ok(SearchOutcome {
        best,
        shortlist: samples,
        evaluated,
        accepted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> AnnealingConfig {
        AnnealingConfig {
            iterations: 256,
            initial_temperature: 1.0,
            final_temperature: 0.01,
            top_k: 4,
        }
    }

    #[test]
    fn seeded_search_is_reproducible_and_converges() {
        let run = || {
            annealed_monte_carlo(
                0.0_f64,
                7,
                config(),
                |current, temperature, rng| {
                    (current + rng.signed_unit() * temperature * 10.0).clamp(-10.0, 10.0)
                },
                |value| Some(-(*value - 3.0).powi(2)),
            )
            .unwrap()
        };
        let first = run();
        let second = run();
        assert_eq!(
            first.best.as_ref().unwrap().score,
            second.best.unwrap().score
        );
        assert!((first.best.unwrap().state - 3.0).abs() < 0.5);
    }

    #[test]
    fn invalid_samples_do_not_replace_valid_state() {
        let outcome = annealed_monte_carlo(
            0_i32,
            1,
            config(),
            |current, _, _| current + 1,
            |value| (*value <= 2).then_some(f64::from(*value)),
        )
        .unwrap();
        assert_eq!(outcome.best.unwrap().state, 2);
        assert_eq!(outcome.evaluated, config().iterations);
    }
}
