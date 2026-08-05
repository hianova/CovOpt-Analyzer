//! Parameter optimization backed by the shared annealed Monte Carlo engine.
//!
//! Parameter classes and tags are metadata only. Every numeric parameter is
//! explored by the same search walk, including coupled parameters.

use crate::search::{AnnealingConfig, SearchRng, annealed_monte_carlo};
use covopt_macro::covopt_param;
use covopt_schema::{EvaluationMode, ParameterDomain, ParameterValue};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct ParamRange {
    pub min: f64,
    pub max: f64,
    pub is_int: bool,
}

pub struct ParameterOptimizer {
    pub params: HashMap<String, ParamRange>,
    pub target_test: String,
    pub iterations: usize,
    pub seed: u64,
    pub top_k: usize,
    pub compile_time_parameters: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptimizationPhase {
    Search,
    Confirm,
    Robustness,
}

#[derive(Debug, Clone)]
pub struct PhasedOptimizationResult {
    pub phase: OptimizationPhase,
    pub best_params: HashMap<String, f64>,
    pub candidate_hash: String,
    pub search_score: f64,
    pub confirmation_score: Option<f64>,
    pub robustness_scores: Vec<f64>,
    pub robustness_observations: Vec<RobustnessObservation>,
    pub robustness_score_floor: Option<f64>,
    pub confirmed: bool,
    pub robustness_verified: bool,
    pub search_elapsed_ms: u128,
    pub confirmation_elapsed_ms: Option<u128>,
    pub evaluated_candidates: usize,
    pub accepted_transitions: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RobustnessObservation {
    pub label: String,
    pub parameters: HashMap<String, f64>,
    pub score: Option<f64>,
    pub passed: bool,
}

impl ParameterOptimizer {
    pub fn new(target_test: String, param_str: &str, iterations: usize) -> Self {
        Self::new_with_seed(target_test, param_str, iterations, 0)
    }

    pub fn new_with_seed(
        target_test: String,
        param_str: &str,
        iterations: usize,
        seed: u64,
    ) -> Self {
        let mut params = HashMap::new();
        for parameter in param_str.split(',') {
            let parts = parameter.split(':').collect::<Vec<_>>();
            if parts.len() != 2 {
                continue;
            }
            let range_parts = parts[1].split("..").collect::<Vec<_>>();
            if range_parts.len() != 2 {
                continue;
            }
            let (Ok(min), Ok(max)) = (range_parts[0].parse::<f64>(), range_parts[1].parse::<f64>())
            else {
                continue;
            };
            if !min.is_finite() || !max.is_finite() || min > max {
                continue;
            }
            params.insert(
                parts[0].trim().to_string(),
                ParamRange {
                    min,
                    max,
                    is_int: !parts[1].replace("..", "").contains('.'),
                },
            );
        }
        Self {
            params,
            target_test,
            iterations,
            seed,
            top_k: 3,
            compile_time_parameters: BTreeSet::new(),
        }
    }

    pub fn with_top_k(mut self, top_k: usize) -> Self {
        self.top_k = top_k.max(1);
        self
    }

    pub fn with_compile_time_parameters(
        mut self,
        parameters: impl IntoIterator<Item = String>,
    ) -> Self {
        self.compile_time_parameters = parameters.into_iter().collect();
        self
    }

    pub fn from_parameter_graph(
        target_test: String,
        graph: &crate::parameters::ParameterDependencyGraph,
        iterations: usize,
        seed: u64,
    ) -> Self {
        let mut optimizer = Self {
            params: HashMap::new(),
            target_test,
            iterations,
            seed,
            top_k: 3,
            compile_time_parameters: BTreeSet::new(),
        };
        for record in graph.parameters.values() {
            if let Some(range) = descriptor_range(&record.descriptor.domain) {
                optimizer
                    .params
                    .insert(record.descriptor.id.0.clone(), range);
            }
            if record.descriptor.evaluation == EvaluationMode::CompileTime {
                optimizer
                    .compile_time_parameters
                    .insert(record.descriptor.id.0.clone());
            }
        }
        optimizer
    }

    /// Compatibility entry point. It delegates to the same phased annealing
    /// workflow used by `covopt optimize parameters`; there is no second engine.
    pub fn run(&self) {
        let result = self.run_phased();
        println!("CovOpt annealed Monte Carlo parameter search");
        println!("Target: {}", self.target_test);
        println!(
            "Evaluated: {}, accepted transitions: {}",
            result.evaluated_candidates, result.accepted_transitions
        );
        println!("Best score: {:.4}", result.search_score);
        println!("Best parameters: {:?}", result.best_params);
        if !result.confirmed || !result.robustness_verified {
            println!("[blocked] Candidate did not pass confirmation and robustness checks.");
            return;
        }
        if let Err(error) = persist_tuned_environment(&self.target_test, &result) {
            println!("[blocked] Could not persist confirmed candidate: {error}");
        }
    }

    /// Execute Search -> Confirm -> Robustness without editing Rust source.
    pub fn run_phased(&self) -> PhasedOptimizationResult {
        let mut names = self.params.keys().cloned().collect::<Vec<_>>();
        names.sort();
        let initial = initial_state(&names, &self.params);
        let annealing = AnnealingConfig {
            iterations: self.iterations.max(1),
            initial_temperature: covopt_param!("parameter.search.initial_temperature", 1.0),
            final_temperature: covopt_param!("parameter.search.final_temperature", 0.01),
            top_k: self.top_k.max(1),
        };
        let search_started = std::time::Instant::now();
        let outcome = annealed_monte_carlo(
            initial,
            self.seed,
            annealing,
            |current, temperature, rng| {
                propose_parameters(current, &names, &self.params, temperature, annealing, rng)
            },
            |candidate| self.evaluate_mode(candidate, "search"),
        );
        let search_elapsed_ms = search_started.elapsed().as_millis();
        let Ok(outcome) = outcome else {
            return empty_result(search_elapsed_ms);
        };
        let evaluated_candidates = outcome.evaluated;
        let accepted_transitions = outcome.accepted;
        let Some(best) = outcome.best else {
            return PhasedOptimizationResult {
                evaluated_candidates,
                accepted_transitions,
                ..empty_result(search_elapsed_ms)
            };
        };

        let mut search_score = best.score;
        let mut best_params = best.state;
        let mut selected_hash = candidate_hash(&best_params, self.seed);
        let confirmation_started = std::time::Instant::now();
        let mut confirmation_score = None;
        for sample in outcome.shortlist {
            let Some(score) = self.evaluate_mode(&sample.state, "confirm") else {
                continue;
            };
            if confirmation_score.is_none_or(|current| score > current) {
                search_score = sample.score;
                best_params = sample.state;
                selected_hash = candidate_hash(&best_params, self.seed);
                confirmation_score = Some(score);
            }
        }
        let confirmation_elapsed_ms =
            confirmation_score.map(|_| confirmation_started.elapsed().as_millis());
        let confirmed = confirmation_score.is_some();
        // Robustness is an executable boundary check, not a second search
        // strategy. The winner is replayed under new seeds and at every
        // declared parameter-domain edge. The admissible score floor is
        // derived from the observed Search/Confirm spread, so no arbitrary
        // degradation percentage is hidden here.
        let robustness_score_floor = confirmation_score
            .map(|confirmed_score| confirmed_score - (confirmed_score - search_score).abs());
        let robustness_observations = if confirmed {
            robustness_candidates(&best_params, &names, &self.params, self.seed)
                .into_iter()
                .map(|(label, parameters)| {
                    let score = self.evaluate_mode(&parameters, "robustness");
                    let passed = score.is_some_and(|score| {
                        score.is_finite()
                            && robustness_score_floor.is_some_and(|floor| score >= floor)
                    });
                    RobustnessObservation {
                        label,
                        parameters,
                        score,
                        passed,
                    }
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let robustness_scores = robustness_observations
            .iter()
            .filter_map(|observation| observation.score)
            .collect::<Vec<_>>();
        let robustness_verified = confirmed
            && !robustness_observations.is_empty()
            && robustness_observations
                .iter()
                .all(|observation| observation.passed);

        PhasedOptimizationResult {
            phase: if robustness_verified {
                OptimizationPhase::Robustness
            } else {
                OptimizationPhase::Confirm
            },
            best_params,
            candidate_hash: selected_hash,
            search_score,
            confirmation_score,
            robustness_scores,
            robustness_observations,
            robustness_score_floor,
            confirmed,
            robustness_verified,
            search_elapsed_ms,
            confirmation_elapsed_ms,
            evaluated_candidates,
            accepted_transitions,
        }
    }

    fn evaluate_mode(&self, params: &HashMap<String, f64>, mode: &str) -> Option<f64> {
        let compile_time_search = mode == "search" && !self.compile_time_parameters.is_empty();
        let effective_mode = if mode == "robustness" {
            "robustness"
        } else if compile_time_search {
            "confirm"
        } else {
            mode
        };
        let mut command = Command::new("cargo");
        command
            .args(["bench", "--bench", &self.target_test])
            .env("COVOPT_PARAM_MODE", effective_mode);

        if matches!(effective_mode, "confirm" | "robustness") {
            let hash = candidate_hash(params, self.seed);
            command
                .env("COVOPT_CONFIRM_CANDIDATE_HASH", &hash)
                .env("CARGO_TARGET_DIR", format!("target/covopt/confirm/{hash}"));
        }

        for (name, value) in params {
            command.env(format!("COVOPT_PARAM_{name}"), value.to_string());
            if name == "COVOPT_SEED" {
                command.env("COVOPT_FUZZ_SEED", value.to_string());
            }
            if matches!(effective_mode, "confirm" | "robustness")
                && (mode == "confirm" || self.compile_time_parameters.contains(name))
            {
                command.env(
                    format!("COVOPT_CONFIRM_{}", name.replace([':', '-', '.'], "_")),
                    value.to_string(),
                );
            }
        }

        let output = command
            .output()
            .ok()
            .filter(|output| output.status.success())?;
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|line| {
                line.split_once("COVOPT_SCORE:")
                    .and_then(|(_, score)| score.trim().parse::<f64>().ok())
                    .filter(|score| score.is_finite())
            })
    }
}

fn robustness_candidates(
    best: &HashMap<String, f64>,
    names: &[String],
    ranges: &HashMap<String, ParamRange>,
    seed: u64,
) -> Vec<(String, HashMap<String, f64>)> {
    let mut candidates = [seed.wrapping_add(1), seed.wrapping_add(2)]
        .into_iter()
        .map(|sample_seed| {
            let mut parameters = best.clone();
            parameters.insert("COVOPT_SEED".to_string(), sample_seed as f64);
            (format!("seed:{sample_seed}"), parameters)
        })
        .collect::<Vec<_>>();
    for name in names {
        let Some(range) = ranges.get(name) else {
            continue;
        };
        for (edge, value) in [("min", range.min), ("max", range.max)] {
            if best.get(name).is_some_and(|best| *best == value) {
                continue;
            }
            let mut parameters = best.clone();
            parameters.insert(name.clone(), value);
            parameters.insert("COVOPT_SEED".to_string(), seed as f64);
            candidates.push((format!("{name}:{edge}"), parameters));
        }
    }
    candidates
}

fn initial_state(names: &[String], ranges: &HashMap<String, ParamRange>) -> HashMap<String, f64> {
    names
        .iter()
        .map(|name| {
            let range = &ranges[name];
            let mut value = range.min + (range.max - range.min) / 2.0;
            if range.is_int {
                value = value.round();
            }
            (name.clone(), value)
        })
        .collect()
}

fn propose_parameters(
    current: &HashMap<String, f64>,
    names: &[String],
    ranges: &HashMap<String, ParamRange>,
    temperature: f64,
    config: AnnealingConfig,
    rng: &mut SearchRng,
) -> HashMap<String, f64> {
    if names.is_empty() {
        return current.clone();
    }
    let mut candidate = current.clone();
    let temperature_ratio = (temperature / config.initial_temperature).clamp(0.0, 1.0);
    let minimum_radius = covopt_param!("parameter.search.minimum_radius", 0.01);
    let mutation_radius = temperature_ratio.max(minimum_radius);
    let guaranteed = rng.index(names.len());
    for (index, name) in names.iter().enumerate() {
        // High temperature naturally perturbs coupled dimensions together; low
        // temperature narrows to local coordinate moves without class dispatch.
        if index != guaranteed && rng.unit() > temperature_ratio {
            continue;
        }
        let range = &ranges[name];
        let span = range.max - range.min;

        let is_bitwise_mode = std::env::var("COVOPT_BITWISE_MODE").unwrap_or_default() == "1";

        let mut value = current[name];
        if range.is_int && is_bitwise_mode {
            let int_val = value as i64;
            let mutation_type = rng.index(3);
            let new_int_val = match mutation_type {
                0 => int_val ^ (1 << rng.index(32)), // Bit-Flip
                1 => if rng.unit() < 0.5 { int_val << 1 } else { int_val >> 1 }, // Bit-Shift
                _ => int_val ^ (rng.index(u32::MAX as usize) as i64), // XOR-Mutation
            };
            value = (new_int_val as f64).clamp(range.min, range.max);
        } else {
            let mut delta = rng.signed_unit() * span * mutation_radius;
            if range.is_int && delta.abs() < 1.0 && span >= 1.0 {
                delta = if delta.is_sign_negative() { -1.0 } else { 1.0 };
            }
            value = (value + delta).clamp(range.min, range.max);
            if range.is_int {
                value = value.round();
            }
        }
        
        candidate.insert(name.clone(), value);
    }
    candidate
}

fn descriptor_range(domain: &ParameterDomain) -> Option<ParamRange> {
    let ParameterDomain::Range(range) = domain else {
        return None;
    };
    let to_float = |value: &ParameterValue| match value {
        ParameterValue::Signed(value) => Some(*value as f64),
        ParameterValue::Unsigned(value)
        | ParameterValue::DurationNs(value)
        | ParameterValue::Count(value)
        | ParameterValue::Bytes(value) => Some(*value as f64),
        ParameterValue::Float(value) => Some(*value),
        ParameterValue::Categorical(_) => None,
    };
    let min = to_float(&range.min)?;
    let max = to_float(&range.max)?;
    (min.is_finite() && max.is_finite() && min <= max).then_some(ParamRange {
        min,
        max,
        is_int: !matches!(range.min, ParameterValue::Float(_)),
    })
}

fn empty_result(search_elapsed_ms: u128) -> PhasedOptimizationResult {
    PhasedOptimizationResult {
        phase: OptimizationPhase::Search,
        best_params: HashMap::new(),
        candidate_hash: String::new(),
        search_score: f64::NEG_INFINITY,
        confirmation_score: None,
        robustness_scores: Vec::new(),
        robustness_observations: Vec::new(),
        robustness_score_floor: None,
        confirmed: false,
        robustness_verified: false,
        search_elapsed_ms,
        confirmation_elapsed_ms: None,
        evaluated_candidates: 0,
        accepted_transitions: 0,
    }
}

fn persist_tuned_environment(
    target_test: &str,
    result: &PhasedOptimizationResult,
) -> Result<(), String> {
    let mut parameters = result.best_params.iter().collect::<Vec<_>>();
    parameters.sort_by(|left, right| left.0.cmp(right.0));
    let environment = parameters
        .into_iter()
        .map(|(name, value)| format!("COVOPT_PARAM_{name}={value}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(
        ".covopt_tuned.env",
        format!(
            "# Auto-generated by CovOpt-Analyzer\n# Target Test: {target_test}\n# Best Score: {:.4}\n{environment}",
            result.search_score
        ),
    )
    .map_err(|error| error.to_string())?;
    std::fs::create_dir_all("target/covopt").map_err(|error| error.to_string())?;
    std::fs::write(
        "target/covopt/parameter-confirmation.json",
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": covopt_schema::SCHEMA_VERSION,
            "algorithm": "annealed-monte-carlo",
            "candidate_hash": result.candidate_hash,
            "confirmation_score": result.confirmation_score,
            "robustness_scores": result.robustness_scores,
            "robustness_observations": result.robustness_observations,
            "robustness_score_floor": result.robustness_score_floor,
            "evaluated_candidates": result.evaluated_candidates,
            "accepted_transitions": result.accepted_transitions,
            "observed": true,
        }))
        .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod robustness_tests {
    use super::*;

    #[test]
    fn robustness_candidates_cover_seeds_and_declared_domain_edges() {
        let best = HashMap::from([("capacity".to_string(), 8.0)]);
        let ranges = HashMap::from([(
            "capacity".to_string(),
            ParamRange {
                min: 1.0,
                max: 64.0,
                is_int: true,
            },
        )]);
        let candidates = robustness_candidates(&best, &["capacity".to_string()], &ranges, 7);
        let labels = candidates
            .iter()
            .map(|(label, _)| label.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            labels,
            vec!["seed:8", "seed:9", "capacity:min", "capacity:max"]
        );
    }
}

fn candidate_hash(params: &HashMap<String, f64>, seed: u64) -> String {
    let mut entries = params.iter().collect::<Vec<_>>();
    entries.sort_by(|left, right| left.0.cmp(right.0));
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = FNV_OFFSET ^ seed;
    for (name, value) in entries {
        for byte in format!("{name}={value};").bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME);
        }
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_hash_is_reproducible_and_order_independent() {
        let first = HashMap::from([(String::from("b"), 2.0), (String::from("a"), 1.0)]);
        let second = HashMap::from([(String::from("a"), 1.0), (String::from("b"), 2.0)]);
        assert_eq!(candidate_hash(&first, 7), candidate_hash(&second, 7));
        assert_ne!(candidate_hash(&first, 7), candidate_hash(&first, 8));
    }

    #[test]
    fn one_kernel_mutates_all_parameter_classes_within_bounds() {
        let names = vec!["threshold".to_string(), "timeout".to_string()];
        let ranges = HashMap::from([
            (
                "threshold".to_string(),
                ParamRange {
                    min: 1.0,
                    max: 64.0,
                    is_int: true,
                },
            ),
            (
                "timeout".to_string(),
                ParamRange {
                    min: 0.1,
                    max: 2.0,
                    is_int: false,
                },
            ),
        ]);
        let config = AnnealingConfig {
            iterations: 8,
            initial_temperature: 1.0,
            final_temperature: 0.01,
            top_k: 2,
        };
        let current = initial_state(&names, &ranges);
        let candidate = propose_parameters(
            &current,
            &names,
            &ranges,
            config.initial_temperature,
            config,
            &mut SearchRng::new(3),
        );
        assert!(candidate["threshold"] >= 1.0 && candidate["threshold"] <= 64.0);
        assert!(candidate["timeout"] >= 0.1 && candidate["timeout"] <= 2.0);
        assert_eq!(candidate["threshold"].fract(), 0.0);
    }
}
