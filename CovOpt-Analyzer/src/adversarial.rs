//! Deterministic adversarial environment search.
//!
//! The search is an orchestration layer: concrete providers implement the
//! oracle, while this module owns domains, explicit seeds, caching, budget
//! handling, failure prioritization, and reproducible output.

use crate::assurance::ObligationStatus;
use crate::model::{AssumptionId, SampleKey};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EnvironmentDimension {
    N,
    Seed,
    Threads,
    QueueCapacity,
    Timeout,
    CancellationPoint,
    AllocationFailure,
    Cpu,
    Toolchain,
    FeatureSet,
    DependencySet,
}

impl EnvironmentDimension {
    pub fn name(self) -> &'static str {
        match self {
            Self::N => "n",
            Self::Seed => "seed",
            Self::Threads => "threads",
            Self::QueueCapacity => "queue_capacity",
            Self::Timeout => "timeout",
            Self::CancellationPoint => "cancellation_point",
            Self::AllocationFailure => "allocation_failure",
            Self::Cpu => "cpu",
            Self::Toolchain => "toolchain",
            Self::FeatureSet => "feature_set",
            Self::DependencySet => "dependency_set",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentDomain {
    pub dimension: EnvironmentDimension,
    pub values: Vec<String>,
    pub explicit_bound: bool,
}

impl EnvironmentDomain {
    pub fn validate(&self) -> Result<(), String> {
        if self.values.is_empty() {
            return Err(format!(
                "environment domain {} is empty",
                self.dimension.name()
            ));
        }
        if !self.explicit_bound {
            return Err(format!(
                "environment domain {} requires an explicit bound",
                self.dimension.name()
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentSample {
    pub sample: SampleKey,
    pub values: BTreeMap<String, String>,
}

impl EnvironmentSample {
    fn from_values(values: BTreeMap<String, String>) -> Self {
        let sample = SampleKey {
            n: values.get("n").and_then(|value| value.parse().ok()),
            seed: values.get("seed").and_then(|value| value.parse().ok()),
            threads: values.get("threads").and_then(|value| value.parse().ok()),
            queue_capacity: values
                .get("queue_capacity")
                .and_then(|value| value.parse().ok()),
            cpu: values.get("cpu").cloned(),
            toolchain: values.get("toolchain").cloned(),
            dependency_set: values.get("dependency_set").cloned(),
        };
        Self { sample, values }
    }

    fn values_to_fingerprint(&self) -> String {
        serde_json::to_string(&self.values).unwrap_or_else(|_| "{}".to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AdversarialObjective {
    ComplexityDeviation,
    RuntimeRegression,
    Contention,
    UncoveredScope,
    UnknownObligations,
    TemporalViolationProbability,
    RelationalDivergence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleResult {
    pub status: ObligationStatus,
    pub failed: bool,
    pub score: f64,
    pub summary: String,
    #[serde(default)]
    pub assumptions: Vec<AssumptionId>,
}

pub trait AdversarialOracle {
    fn evaluate(
        &mut self,
        sample: &EnvironmentSample,
        objective: AdversarialObjective,
    ) -> OracleResult;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdversarialConfig {
    pub target: String,
    pub budget_ms: u64,
    pub seed: u64,
    pub objectives: Vec<AdversarialObjective>,
    pub domains: Vec<EnvironmentDomain>,
    pub max_candidates: usize,
}

impl AdversarialConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.budget_ms == 0 {
            return Err("adversarial budget must be greater than zero".to_string());
        }
        if self.objectives.is_empty() {
            return Err("at least one adversarial objective is required".to_string());
        }
        for domain in &self.domains {
            domain.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SearchStatus {
    FailureFound,
    NoFailureWithinBound,
    SearchIncomplete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdversarialSearchResult {
    pub schema_version: u32,
    pub target: String,
    pub status: SearchStatus,
    pub evaluated: usize,
    pub cache_hits: usize,
    pub smallest_failing_environment: Option<EnvironmentSample>,
    pub most_damaging_environment: Option<EnvironmentSample>,
    #[serde(default)]
    pub verified_parameter_envelope: Vec<EnvironmentDomain>,
    #[serde(default)]
    pub assumptions: Vec<AssumptionId>,
    #[serde(default)]
    pub unsearched_dimensions: Vec<EnvironmentDimension>,
    #[serde(default)]
    pub pareto_candidates: Vec<EnvironmentSample>,
    pub best_known: Option<OracleResult>,
}

pub fn search<O: AdversarialOracle>(
    config: &AdversarialConfig,
    oracle: &mut O,
) -> Result<AdversarialSearchResult, String> {
    config.validate()?;
    let started = Instant::now();
    let candidates = generate_candidates(config);
    let mut cache: HashMap<String, OracleResult> = HashMap::new();
    let mut cache_hits = 0;
    let mut evaluated = 0;
    let mut failures = Vec::new();
    let mut best: Option<(EnvironmentSample, OracleResult)> = None;
    let mut assumptions = HashSet::new();
    let mut pareto = Vec::new();
    let mut incomplete = false;

    'candidate: for sample in candidates.iter().take(config.max_candidates.max(1)) {
        if started.elapsed() >= Duration::from_millis(config.budget_ms) {
            incomplete = true;
            break;
        }
        for objective in &config.objectives {
            let key = format!("{}::{:?}", sample.values_to_fingerprint(), objective);
            let result = if let Some(result) = cache.get(&key) {
                cache_hits += 1;
                result.clone()
            } else {
                let result = oracle.evaluate(sample, *objective);
                cache.insert(key, result.clone());
                evaluated += 1;
                result
            };
            for assumption in &result.assumptions {
                assumptions.insert(assumption.clone());
            }
            if result.failed {
                failures.push((sample.clone(), result.clone()));
            }
            if best
                .as_ref()
                .is_none_or(|(_, previous)| result.score > previous.score)
            {
                best = Some((sample.clone(), result.clone()));
            }
            if result.status == ObligationStatus::Unknown {
                incomplete = true;
            }
            if result.failed {
                pareto.push(sample.clone());
                continue 'candidate;
            }
        }
    }
    if evaluated < candidates.len().min(config.max_candidates.max(1)) {
        incomplete = true;
    }
    failures.sort_by(|(left, _), (right, _)| {
        sample_order(&left.sample).cmp(&sample_order(&right.sample))
    });
    let status = if !failures.is_empty() {
        SearchStatus::FailureFound
    } else if incomplete {
        SearchStatus::SearchIncomplete
    } else {
        SearchStatus::NoFailureWithinBound
    };
    let most_damaging = best.as_ref().map(|(sample, _)| sample.clone());
    let best_known = best.map(|(_, result)| result);
    let searched = config
        .domains
        .iter()
        .filter(|domain| {
            candidates
                .iter()
                .any(|candidate| candidate.values.contains_key(domain.dimension.name()))
        })
        .map(|domain| domain.dimension)
        .collect::<HashSet<_>>();
    let unsearched_dimensions = config
        .domains
        .iter()
        .map(|domain| domain.dimension)
        .filter(|dimension| !searched.contains(dimension))
        .collect();
    Ok(AdversarialSearchResult {
        schema_version: crate::model::MODEL_SCHEMA_VERSION,
        target: config.target.clone(),
        status,
        evaluated,
        cache_hits,
        smallest_failing_environment: failures.first().map(|(sample, _)| sample.clone()),
        most_damaging_environment: most_damaging,
        verified_parameter_envelope: config.domains.clone(),
        assumptions: assumptions.into_iter().collect(),
        unsearched_dimensions,
        pareto_candidates: pareto,
        best_known,
    })
}

fn generate_candidates(config: &AdversarialConfig) -> Vec<EnvironmentSample> {
    let mut candidates = vec![BTreeMap::from([(
        "seed".to_string(),
        config.seed.to_string(),
    )])];
    for domain in &config.domains {
        let mut next = Vec::new();
        let mut values = domain.values.clone();
        values.sort();
        values.dedup();
        for candidate in &candidates {
            for value in &values {
                let mut next_candidate = candidate.clone();
                next_candidate.insert(domain.dimension.name().to_string(), value.clone());
                next.push(next_candidate);
            }
        }
        candidates = next;
    }
    candidates
        .into_iter()
        .map(EnvironmentSample::from_values)
        .collect()
}

fn sample_order(sample: &SampleKey) -> (usize, usize, usize, u64) {
    (
        sample.n.unwrap_or(0),
        sample.threads.unwrap_or(0),
        sample.queue_capacity.unwrap_or(0),
        sample.seed.unwrap_or(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Oracle;

    impl AdversarialOracle for Oracle {
        fn evaluate(
            &mut self,
            sample: &EnvironmentSample,
            _: AdversarialObjective,
        ) -> OracleResult {
            let threads = sample.sample.threads.unwrap_or(0);
            OracleResult {
                status: ObligationStatus::Observed,
                failed: threads >= 4,
                score: threads as f64,
                summary: "test oracle".to_string(),
                assumptions: Vec::new(),
            }
        }
    }

    #[test]
    fn search_is_seeded_and_returns_smallest_failure() {
        let config = AdversarialConfig {
            target: "queue".to_string(),
            budget_ms: 1_000,
            seed: 7,
            objectives: vec![AdversarialObjective::Contention],
            domains: vec![EnvironmentDomain {
                dimension: EnvironmentDimension::Threads,
                values: vec!["1".to_string(), "4".to_string(), "8".to_string()],
                explicit_bound: true,
            }],
            max_candidates: 8,
        };
        let result = search(&config, &mut Oracle).unwrap();
        assert_eq!(result.status, SearchStatus::FailureFound);
        assert_eq!(
            result.smallest_failing_environment.unwrap().sample.threads,
            Some(4)
        );
    }
}
