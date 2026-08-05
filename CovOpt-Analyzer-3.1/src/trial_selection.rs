//! Deterministic target/N/seed selection for complexity evidence.
//!
//! The selector is deliberately a planner: it describes candidates and a
//! feasible selection, but never runs a test and never terminates the process.

use crate::assurance::{EvidenceActionId, ObligationId};
use crate::config::TargetConfig;
use crate::model::SampleKey;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};

pub type ActionId = EvidenceActionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComplexityModel {
    Constant,
    LogN,
    SqrtN,
    Linear,
    NLogN,
    Quadratic,
    Exponential,
}

impl ComplexityModel {
    pub fn parse(value: &str) -> Option<Self> {
        let compact = value
            .to_ascii_uppercase()
            .replace([' ', '_', '-', '(', ')', '^'], "");
        match compact.as_str() {
            "O1" | "1" | "CONSTANT" => Some(Self::Constant),
            "OLOGN" | "LOGN" => Some(Self::LogN),
            "OSQRTN" | "SQRTN" | "SQRT" => Some(Self::SqrtN),
            "ON" | "N" | "LINEAR" => Some(Self::Linear),
            "ONLOGN" | "NLOGN" => Some(Self::NLogN),
            "ON2" | "N2" | "QUADRATIC" => Some(Self::Quadratic),
            "O2N" | "2N" | "EXPONENTIAL" => Some(Self::Exponential),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Constant => "O(1)",
            Self::LogN => "O(log N)",
            Self::SqrtN => "O(sqrt N)",
            Self::Linear => "O(N)",
            Self::NLogN => "O(N log N)",
            Self::Quadratic => "O(N^2)",
            Self::Exponential => "O(2^N)",
        }
    }

    fn score(self, n: usize) -> f64 {
        let n = n.max(1) as f64;
        match self {
            Self::Constant => 1.0,
            Self::LogN => n.ln_1p(),
            Self::SqrtN => n.sqrt(),
            Self::Linear => n,
            Self::NLogN => n * n.ln_1p(),
            Self::Quadratic => n * n,
            Self::Exponential => (n.min(32.0)).exp2(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TrialId(pub String);

impl std::fmt::Display for TrialId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ModelPair {
    pub left: ComplexityModel,
    pub right: ComplexityModel,
}

impl ModelPair {
    pub fn new(left: ComplexityModel, right: ComplexityModel) -> Self {
        if left <= right {
            Self { left, right }
        } else {
            Self {
                left: right,
                right: left,
            }
        }
    }

    pub fn separation(&self, n: usize) -> f64 {
        let left = self.left.score(n);
        let right = self.right.score(n);
        ((left.max(right) + 1.0) / (left.min(right) + 1.0)).ln()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Stability {
    Stable,
    Variable,
    Flaky,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrialCandidate {
    pub id: TrialId,
    pub target: String,
    pub n: usize,
    pub seed: u64,
    /// Full sample identity; `n` and `seed` remain as compatibility fields.
    #[serde(default)]
    pub sample_key: SampleKey,
    pub estimated_cost_ms: u64,
    #[serde(default)]
    pub obligations: Vec<ObligationId>,
    #[serde(default)]
    pub discriminates: Vec<ModelPair>,
    #[serde(default)]
    pub stability: Stability,
    #[serde(default)]
    pub prerequisites: Vec<ActionId>,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub critical: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TrialFailure {
    Panic,
    Deadlock,
    Timeout,
    Infrastructure,
    #[default]
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReproducibilityEvidence {
    pub attempts: u32,
    pub matching_outcomes: u32,
    pub ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrialResult {
    pub trial_id: TrialId,
    #[serde(default)]
    pub sample_key: SampleKey,
    pub runtime_ms: u64,
    pub peak_rss_bytes: Option<u64>,
    #[serde(default)]
    pub observed_obligations: Vec<ObligationId>,
    #[serde(default)]
    pub complexity_evidence: BTreeMap<ComplexityModel, f64>,
    #[serde(default)]
    pub failure: TrialFailure,
    #[serde(default)]
    pub reproducibility: ReproducibilityEvidence,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrialPlan {
    pub selected: Vec<TrialId>,
    #[serde(default)]
    pub rejected: Vec<TrialRejection>,
    pub expected_coverage: f64,
    #[serde(default)]
    pub expected_model_discrimination: Vec<ModelPair>,
    pub model_confidence: f64,
    pub estimated_cost_ms: u64,
    #[serde(default)]
    pub reasons: BTreeMap<TrialId, String>,
    pub timed_out: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrialRejection {
    pub trial_id: TrialId,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrialSelectionConfig {
    pub budget_ms: u64,
    pub seed: Option<u64>,
    pub seed_count: usize,
    pub seed_start: Option<u64>,
    pub seed_end: Option<u64>,
    pub max_candidates: usize,
    pub local_search_rounds: usize,
    pub exact_candidate_limit: usize,
    pub timeout_ms: u64,
    pub minimum_model_separation: f64,
}

impl Default for TrialSelectionConfig {
    fn default() -> Self {
        Self {
            budget_ms: 30_000,
            seed: None,
            seed_count: 3,
            seed_start: None,
            seed_end: None,
            max_candidates: 256,
            local_search_rounds: 2,
            exact_candidate_limit: 24,
            timeout_ms: 5_000,
            minimum_model_separation: 0.15,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateGenerationInput {
    pub target: String,
    pub expected: Option<ComplexityModel>,
    pub n_values: Vec<usize>,
    pub obligations: Vec<ObligationId>,
    pub critical_obligations: Vec<ObligationId>,
    pub config_fingerprint: String,
}

fn fnv1a(bytes: impl IntoIterator<Item = u8>) -> u64 {
    bytes.into_iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

pub fn derive_seed(target: &str, revision: &str, config_fingerprint: &str, index: usize) -> u64 {
    let mut input = Vec::new();
    input.extend_from_slice(target.as_bytes());
    input.push(0);
    input.extend_from_slice(revision.as_bytes());
    input.push(0);
    input.extend_from_slice(config_fingerprint.as_bytes());
    input.push(0);
    input.extend_from_slice(&(index as u64).to_le_bytes());
    fnv1a(input).max(1)
}

pub fn seed_pool(input: &CandidateGenerationInput, config: &TrialSelectionConfig) -> Vec<u64> {
    let start = config.seed_start.unwrap_or(0);
    let end = config.seed_end.unwrap_or(u64::MAX);
    let mut seeds = Vec::new();
    if let Some(seed) = config.seed.filter(|seed| *seed >= start && *seed <= end) {
        seeds.push(seed);
    }
    let revision = std::env::var("COVOPT_REVISION").unwrap_or_else(|_| "working-tree".to_string());
    for index in 0..config.seed_count.max(1) {
        let seed = derive_seed(&input.target, &revision, &input.config_fingerprint, index);
        if seed >= start && seed <= end && !seeds.contains(&seed) {
            seeds.push(seed);
        }
    }
    if seeds.is_empty() {
        seeds.push(start.max(1));
    }
    seeds.sort_unstable();
    seeds
}

pub fn parse_n_values(value: Option<&str>) -> Vec<usize> {
    let mut values = value
        .unwrap_or("")
        .split([',', ';', ' '])
        .filter_map(|part| part.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .collect::<Vec<_>>();
    values.sort_unstable();
    values.dedup();
    values
}

pub fn candidate_input_from_target(
    target: &TargetConfig,
    obligations: Vec<ObligationId>,
    critical_obligations: Vec<ObligationId>,
) -> CandidateGenerationInput {
    CandidateGenerationInput {
        target: target.test.clone(),
        expected: target.expected.as_deref().and_then(ComplexityModel::parse),
        n_values: parse_n_values(target.n_values.as_deref()),
        obligations,
        critical_obligations,
        config_fingerprint: format!(
            "{}|{}|{}|{}",
            target.expected.as_deref().unwrap_or(""),
            target.n_values.as_deref().unwrap_or(""),
            target.package.as_deref().unwrap_or(""),
            target.mca_cpu.as_deref().unwrap_or("")
        ),
    }
}

fn candidate_ns(values: &[usize]) -> Vec<usize> {
    let mut result = values
        .iter()
        .copied()
        .filter(|n| *n > 0)
        .collect::<Vec<_>>();
    if let (Some(min), Some(max)) = (result.iter().min().copied(), result.iter().max().copied()) {
        for n in [
            min.saturating_div(2).max(1),
            min,
            min.saturating_add(1),
            max,
            max.saturating_mul(2),
        ] {
            result.push(n.max(1));
        }
        let mut geometric = min.max(1);
        while geometric < max && result.len() < 32 {
            geometric = geometric.saturating_mul(2).max(geometric.saturating_add(1));
            if geometric <= max {
                result.push(geometric);
            }
        }
    }
    result.sort_unstable();
    result.dedup();
    result
}

pub fn model_pairs(expected: Option<ComplexityModel>) -> Vec<ModelPair> {
    let models = [
        ComplexityModel::Constant,
        ComplexityModel::LogN,
        ComplexityModel::SqrtN,
        ComplexityModel::Linear,
        ComplexityModel::NLogN,
        ComplexityModel::Quadratic,
        ComplexityModel::Exponential,
    ];
    let mut pairs = Vec::new();
    for left in models {
        for right in models {
            if left < right && expected.is_none_or(|value| left == value || right == value) {
                pairs.push(ModelPair::new(left, right));
            }
        }
    }
    pairs
}

pub fn generate_candidates(
    input: &CandidateGenerationInput,
    config: &TrialSelectionConfig,
) -> Vec<TrialCandidate> {
    let ns = candidate_ns(&input.n_values);
    let seeds = seed_pool(input, config);
    let pairs = model_pairs(input.expected);
    let mut candidates = Vec::new();
    for n in ns {
        for seed in &seeds {
            let discriminates = pairs
                .iter()
                .filter(|pair| pair.separation(n) >= config.minimum_model_separation)
                .cloned()
                .collect::<Vec<_>>();
            let id = TrialId(format!("{}:n{}:s{}", input.target, n, seed));
            candidates.push(TrialCandidate {
                id,
                target: input.target.clone(),
                n,
                seed: *seed,
                sample_key: SampleKey::complexity(n, *seed),
                estimated_cost_ms: 5u64.saturating_add((n as u64).min(10_000)),
                obligations: input.obligations.clone(),
                discriminates,
                stability: Stability::Unknown,
                prerequisites: Vec::new(),
                reason: if input.n_values.contains(&n) {
                    "configured N"
                } else {
                    "boundary/geometric N"
                }
                .to_string(),
                critical: !input.critical_obligations.is_empty(),
            });
        }
    }
    candidates.sort_by(|left, right| {
        left.target
            .cmp(&right.target)
            .then(left.n.cmp(&right.n))
            .then(left.seed.cmp(&right.seed))
    });
    candidates.truncate(config.max_candidates.max(1));
    candidates
}

fn candidate_gain(candidate: &TrialCandidate, selected: &[&TrialCandidate]) -> (usize, usize, i64) {
    let obligations = selected
        .iter()
        .flat_map(|item| item.obligations.iter())
        .collect::<HashSet<_>>();
    let models = selected
        .iter()
        .flat_map(|item| item.discriminates.iter())
        .collect::<HashSet<_>>();
    let new_obligations = candidate
        .obligations
        .iter()
        .filter(|id| !obligations.contains(id))
        .count();
    let new_models = candidate
        .discriminates
        .iter()
        .filter(|pair| !models.contains(pair))
        .count();
    let stability = match candidate.stability {
        Stability::Stable => 2,
        Stability::Unknown => 1,
        Stability::Variable => 0,
        Stability::Flaky => -2,
    };
    (
        new_obligations,
        new_models,
        stability - candidate.estimated_cost_ms as i64,
    )
}

fn selection_score(
    selected: &[&TrialCandidate],
    required_obligations: &[ObligationId],
) -> (usize, usize, u64, Vec<String>) {
    let required = required_obligations.iter().collect::<HashSet<_>>();
    let covered = selected
        .iter()
        .flat_map(|candidate| candidate.obligations.iter())
        .collect::<HashSet<_>>();
    let models = selected
        .iter()
        .flat_map(|candidate| candidate.discriminates.iter())
        .collect::<HashSet<_>>();
    let cost = selected
        .iter()
        .map(|candidate| candidate.estimated_cost_ms)
        .sum();
    let mut ids = selected
        .iter()
        .map(|candidate| candidate.id.0.clone())
        .collect::<Vec<_>>();
    ids.sort();
    (
        required.intersection(&covered).count(),
        models.len(),
        cost,
        ids,
    )
}

fn better_selection(
    candidate: &[&TrialCandidate],
    best: &[&TrialCandidate],
    required_obligations: &[ObligationId],
) -> bool {
    let left = selection_score(candidate, required_obligations);
    let right = selection_score(best, required_obligations);
    left.0 > right.0
        || (left.0 == right.0
            && (left.1 > right.1
                || (left.1 == right.1
                    && (left.2 < right.2 || (left.2 == right.2 && left.3 < right.3)))))
}

#[allow(clippy::too_many_arguments)]
fn exact_search<'a>(
    candidates: &[&'a TrialCandidate],
    index: usize,
    budget_ms: u64,
    required_obligations: &[ObligationId],
    selected: &mut Vec<&'a TrialCandidate>,
    best: &mut Vec<&'a TrialCandidate>,
    started: std::time::Instant,
    timeout_ms: u64,
) -> bool {
    if started.elapsed().as_millis() as u64 >= timeout_ms {
        return false;
    }
    if index == candidates.len() {
        if better_selection(selected, best, required_obligations) {
            *best = selected.clone();
        }
        return true;
    }
    exact_search(
        candidates,
        index + 1,
        budget_ms,
        required_obligations,
        selected,
        best,
        started,
        timeout_ms,
    );
    let candidate = candidates[index];
    let cost = selected
        .iter()
        .map(|item| item.estimated_cost_ms)
        .sum::<u64>();
    if cost.saturating_add(candidate.estimated_cost_ms) <= budget_ms {
        selected.push(candidate);
        exact_search(
            candidates,
            index + 1,
            budget_ms,
            required_obligations,
            selected,
            best,
            started,
            timeout_ms,
        );
        selected.pop();
    }
    (started.elapsed().as_millis() as u64) < timeout_ms
}

pub fn select_trials(
    candidates: &[TrialCandidate],
    required_obligations: &[ObligationId],
    config: &TrialSelectionConfig,
) -> TrialPlan {
    let started = std::time::Instant::now();
    let mut selected = Vec::<&TrialCandidate>::new();
    let mut timed_out = false;
    if candidates.len() <= config.exact_candidate_limit {
        let ordered = candidates.iter().collect::<Vec<_>>();
        let mut best = Vec::new();
        let completed = exact_search(
            &ordered,
            0,
            config.budget_ms,
            required_obligations,
            &mut selected,
            &mut best,
            started,
            config.timeout_ms,
        );
        if completed {
            selected = best;
        } else {
            timed_out = true;
            selected.clear();
        }
    }
    if candidates.len() > config.exact_candidate_limit || timed_out {
        let mut remaining = candidates.iter().collect::<Vec<_>>();
        remaining.sort_by(|left, right| {
            candidate_gain(right, &[])
                .cmp(&candidate_gain(left, &[]))
                .then(left.id.cmp(&right.id))
        });
        let mut cost = 0u64;
        while !remaining.is_empty() {
            if started.elapsed().as_millis() as u64 >= config.timeout_ms {
                timed_out = true;
                break;
            }
            let best_index = remaining
                .iter()
                .enumerate()
                .filter(|(_, candidate)| {
                    cost.saturating_add(candidate.estimated_cost_ms) <= config.budget_ms
                })
                .max_by(|(_, left), (_, right)| {
                    candidate_gain(left, &selected)
                        .cmp(&candidate_gain(right, &selected))
                        .then_with(|| right.id.cmp(&left.id))
                })
                .map(|(index, _)| index);
            let Some(index) = best_index else { break };
            let candidate = remaining.remove(index);
            let gain = candidate_gain(candidate, &selected);
            if gain.0 == 0 && gain.1 == 0 && !selected.is_empty() {
                break;
            }
            cost = cost.saturating_add(candidate.estimated_cost_ms);
            selected.push(candidate);
        }
    }
    let cost = selected
        .iter()
        .map(|candidate| candidate.estimated_cost_ms)
        .sum::<u64>();

    let required = required_obligations.iter().collect::<HashSet<_>>();
    let covered = selected
        .iter()
        .flat_map(|candidate| candidate.obligations.iter())
        .collect::<HashSet<_>>();
    let expected_coverage = if required.is_empty() {
        1.0
    } else {
        required.intersection(&covered).count() as f64 / required.len() as f64
    };
    let mut pairs = BTreeSet::new();
    let mut reasons = BTreeMap::new();
    let selected_id_set = selected
        .iter()
        .map(|candidate| candidate.id.clone())
        .collect::<HashSet<_>>();
    let mut selected_ids = selected_id_set.iter().cloned().collect::<Vec<_>>();
    selected_ids.sort();
    for candidate in selected {
        pairs.extend(candidate.discriminates.iter().cloned());
        reasons.insert(candidate.id.clone(), candidate.reason.clone());
    }
    let rejected = candidates
        .iter()
        .filter(|candidate| !selected_id_set.contains(&candidate.id))
        .map(|candidate| TrialRejection {
            trial_id: candidate.id.clone(),
            reason: if cost.saturating_add(candidate.estimated_cost_ms) > config.budget_ms {
                "budget"
            } else {
                "dominated or redundant"
            }
            .to_string(),
        })
        .collect();
    let distinct_n = selected_ids
        .iter()
        .filter_map(|id| candidates.iter().find(|candidate| &candidate.id == id))
        .map(|candidate| candidate.n)
        .collect::<HashSet<_>>();
    let model_confidence = match distinct_n.len() {
        0 | 1 => 0.25,
        2 => 0.50,
        _ => 0.75,
    };
    TrialPlan {
        selected: selected_ids,
        rejected,
        expected_coverage,
        expected_model_discrimination: pairs.into_iter().collect(),
        model_confidence,
        estimated_cost_ms: cost,
        reasons,
        timed_out: timed_out || started.elapsed().as_millis() as u64 >= config.timeout_ms,
    }
}

pub fn adaptive_should_stop(
    results: &[TrialResult],
    required_obligations: &[ObligationId],
    minimum_coverage: f64,
    minimum_model_confidence: f64,
) -> bool {
    let required = required_obligations.iter().collect::<HashSet<_>>();
    let observed = results
        .iter()
        .flat_map(|result| result.observed_obligations.iter())
        .collect::<HashSet<_>>();
    let coverage = if required.is_empty() {
        1.0
    } else {
        required.intersection(&observed).count() as f64 / required.len() as f64
    };
    let confidence = model_confidence(results);
    coverage >= minimum_coverage && confidence >= minimum_model_confidence
}

pub fn model_confidence(results: &[TrialResult]) -> f64 {
    let observed = results
        .iter()
        .flat_map(|result| result.complexity_evidence.values())
        .copied()
        .fold(0.0, f64::max);
    if results.len() < 3 {
        observed.min(0.50)
    } else {
        observed
    }
}

/// Keep atomic-model counterexample seeds in the regression corpus even when
/// the normal greedy selector would otherwise prune them as dominated.
pub fn retain_counterexample_seeds(
    candidates: &[TrialCandidate],
    counterexample_seeds: &HashSet<u64>,
) -> Vec<TrialCandidate> {
    let mut retained = candidates
        .iter()
        .filter(|candidate| counterexample_seeds.contains(&candidate.seed))
        .cloned()
        .collect::<Vec<_>>();
    retained.sort_by(|left, right| left.id.cmp(&right.id));
    retained
}

pub fn historical_stability(results: &[TrialResult]) -> Stability {
    if results.is_empty() {
        return Stability::Unknown;
    }
    if results.len() < 3 {
        return Stability::Variable;
    }
    if results
        .iter()
        .any(|result| !matches!(result.failure, TrialFailure::None))
    {
        return Stability::Variable;
    }
    let runtimes = results
        .iter()
        .map(|result| result.runtime_ms as f64)
        .collect::<Vec<_>>();
    let mean = runtimes.iter().sum::<f64>() / runtimes.len() as f64;
    let variance = runtimes
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / runtimes.len() as f64;
    if mean > 0.0 && variance.sqrt() / mean < 0.10 {
        Stability::Stable
    } else if variance.sqrt() / mean < 0.30 {
        Stability::Variable
    } else {
        Stability::Flaky
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> CandidateGenerationInput {
        CandidateGenerationInput {
            target: "scan".to_string(),
            expected: Some(ComplexityModel::Linear),
            n_values: vec![10, 100],
            obligations: vec![ObligationId("complexity".to_string())],
            critical_obligations: Vec::new(),
            config_fingerprint: "test".to_string(),
        }
    }

    #[test]
    fn candidates_are_deterministic_and_unique() {
        let config = TrialSelectionConfig::default();
        let left = generate_candidates(&input(), &config);
        let right = generate_candidates(&input(), &config);
        assert_eq!(
            serde_json::to_string(&left).unwrap(),
            serde_json::to_string(&right).unwrap()
        );
        let ids = left
            .iter()
            .map(|candidate| &candidate.id)
            .collect::<HashSet<_>>();
        assert_eq!(ids.len(), left.len());
    }

    #[test]
    fn selection_respects_budget_and_coverage() {
        let config = TrialSelectionConfig {
            budget_ms: 20,
            ..TrialSelectionConfig::default()
        };
        let mut candidates = generate_candidates(&input(), &config);
        candidates[0].estimated_cost_ms = 10;
        candidates[1].estimated_cost_ms = 10;
        let plan = select_trials(&candidates, &input().obligations, &config);
        assert!(plan.estimated_cost_ms <= config.budget_ms);
        assert!(plan.expected_coverage >= 1.0);
    }

    #[test]
    fn exact_selection_prefers_minimum_cost_full_coverage() {
        let candidates = vec![
            TrialCandidate {
                id: TrialId("a".to_string()),
                target: "t".to_string(),
                n: 1,
                seed: 1,
                sample_key: SampleKey::complexity(1, 1),
                estimated_cost_ms: 1,
                obligations: vec![ObligationId("one".to_string())],
                discriminates: Vec::new(),
                stability: Stability::Stable,
                prerequisites: Vec::new(),
                reason: "a".to_string(),
                critical: false,
            },
            TrialCandidate {
                id: TrialId("b".to_string()),
                target: "t".to_string(),
                n: 2,
                seed: 1,
                sample_key: SampleKey::complexity(2, 1),
                estimated_cost_ms: 1,
                obligations: vec![ObligationId("two".to_string())],
                discriminates: Vec::new(),
                stability: Stability::Stable,
                prerequisites: Vec::new(),
                reason: "b".to_string(),
                critical: false,
            },
            TrialCandidate {
                id: TrialId("c".to_string()),
                target: "t".to_string(),
                n: 3,
                seed: 1,
                sample_key: SampleKey::complexity(3, 1),
                estimated_cost_ms: 5,
                obligations: vec![
                    ObligationId("one".to_string()),
                    ObligationId("two".to_string()),
                ],
                discriminates: Vec::new(),
                stability: Stability::Stable,
                prerequisites: Vec::new(),
                reason: "c".to_string(),
                critical: false,
            },
        ];
        let plan = select_trials(
            &candidates,
            &[
                ObligationId("one".to_string()),
                ObligationId("two".to_string()),
            ],
            &TrialSelectionConfig {
                exact_candidate_limit: 8,
                ..TrialSelectionConfig::default()
            },
        );
        assert_eq!(
            plan.selected,
            vec![TrialId("a".to_string()), TrialId("b".to_string())]
        );
        assert_eq!(plan.estimated_cost_ms, 2);
    }

    #[test]
    fn two_point_history_is_not_stable_by_default() {
        let results = vec![
            TrialResult {
                trial_id: TrialId("a".to_string()),
                sample_key: SampleKey::complexity(1, 1),
                runtime_ms: 10,
                peak_rss_bytes: None,
                observed_obligations: Vec::new(),
                complexity_evidence: BTreeMap::new(),
                failure: TrialFailure::None,
                reproducibility: ReproducibilityEvidence::default(),
            },
            TrialResult {
                trial_id: TrialId("b".to_string()),
                sample_key: SampleKey::complexity(2, 1),
                runtime_ms: 20,
                peak_rss_bytes: None,
                observed_obligations: Vec::new(),
                complexity_evidence: BTreeMap::new(),
                failure: TrialFailure::None,
                reproducibility: ReproducibilityEvidence::default(),
            },
        ];
        assert_eq!(historical_stability(&results), Stability::Variable);
    }
}
