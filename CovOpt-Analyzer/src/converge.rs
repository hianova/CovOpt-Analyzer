//! Goal-driven autonomous convergence with candidate-bound evidence and
//! transactional workspace apply.

use crate::assurance::{
    EvidenceAction, EvidenceCoverage, EvidencePlan, EvidenceProviderKind, PlanStatus, Severity,
};
use crate::config::CovOptConfig;
use crate::findings::{FindingKind, FindingReport};
use crate::repair::{
    CandidateEvidenceVerification, RepairCandidate, RepairCandidateId, RepairKind,
    RepairTransaction, RiskLevel, apply_edits_transactionally, rollback_transaction,
    verify_candidate_evidence_in_sandbox,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Instant;

pub const DECISION_BUNDLE_PATH: &str = "target/covopt/decision-bundle.json";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Authority {
    ReadOnly,
    Suggest,
    #[default]
    Apply,
}

impl FromStr for Authority {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "read-only" | "readonly" | "read" => Ok(Self::ReadOnly),
            "suggest" | "advice" | "advise" => Ok(Self::Suggest),
            "apply" | "turbo" => Ok(Self::Apply),
            _ => Err(format!(
                "unknown authority '{value}'; expected read-only, suggest, or apply"
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetSelector {
    #[serde(default = "default_target_selector")]
    pub selector: String,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default, flatten)]
    pub extensions: BTreeMap<String, Value>,
}

fn default_target_selector() -> String {
    "auto".to_string()
}

impl Default for TargetSelector {
    fn default() -> Self {
        Self {
            selector: default_target_selector(),
            value: None,
            extensions: BTreeMap::new(),
        }
    }
}

/// Open evaluator reference. `id` is registry-resolved; a custom evaluator may
/// declare a candidate-bound provider explicitly. Unknown IDs never pass by
/// default.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluatorSpec {
    pub id: String,
    #[serde(default = "default_evaluator_version")]
    pub version: u32,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub config: Value,
    #[serde(default, flatten)]
    pub extensions: BTreeMap<String, Value>,
}

fn default_evaluator_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSpec {
    pub id: String,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub evaluator: Option<EvaluatorSpec>,
    #[serde(default, flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceptanceSpec {
    #[serde(default = "default_acceptance_operator")]
    pub operator: String,
    #[serde(default)]
    pub value: Option<f64>,
    #[serde(default, flatten)]
    pub extensions: BTreeMap<String, Value>,
}

fn default_acceptance_operator() -> String {
    "improve".to_string()
}

impl Default for AcceptanceSpec {
    fn default() -> Self {
        Self {
            operator: default_acceptance_operator(),
            value: None,
            extensions: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectiveSpec {
    pub id: String,
    pub metric: MetricSpec,
    #[serde(default = "default_direction")]
    pub direction: String,
    #[serde(default = "default_weight")]
    pub weight: f64,
    #[serde(default)]
    pub acceptance: AcceptanceSpec,
    #[serde(default, flatten)]
    pub extensions: BTreeMap<String, Value>,
}

fn default_direction() -> String {
    "minimize".to_string()
}

fn default_weight() -> f64 {
    1.0
}

impl ObjectiveSpec {
    pub fn named(id: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            id: id.clone(),
            metric: MetricSpec {
                id,
                unit: None,
                evaluator: None,
                extensions: BTreeMap::new(),
            },
            direction: default_direction(),
            weight: default_weight(),
            acceptance: AcceptanceSpec::default(),
            extensions: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintSpec {
    pub id: String,
    #[serde(default = "default_required")]
    pub required: bool,
    #[serde(default)]
    pub evaluator: Option<EvaluatorSpec>,
    #[serde(default)]
    pub config: Value,
    #[serde(default, flatten)]
    pub extensions: BTreeMap<String, Value>,
}

fn default_required() -> bool {
    true
}

impl ConstraintSpec {
    pub fn required(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            required: true,
            evaluator: None,
            config: Value::Null,
            extensions: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalBudget {
    #[serde(default = "default_budget_ms")]
    pub wall_time_ms: u64,
    #[serde(default = "default_iterations")]
    pub max_iterations: usize,
    #[serde(default, flatten)]
    pub extensions: BTreeMap<String, Value>,
}

fn default_budget_ms() -> u64 {
    30_000
}

fn default_iterations() -> usize {
    8
}

impl Default for GoalBudget {
    fn default() -> Self {
        Self {
            wall_time_ms: default_budget_ms(),
            max_iterations: default_iterations(),
            extensions: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalSpec {
    #[serde(default = "default_goal_schema")]
    pub schema_version: u32,
    #[serde(default)]
    pub target: TargetSelector,
    /// Empty means infer objectives from current findings.
    #[serde(default)]
    pub objectives: Vec<ObjectiveSpec>,
    #[serde(default = "default_constraints")]
    pub constraints: Vec<ConstraintSpec>,
    #[serde(default)]
    pub budget: GoalBudget,
    #[serde(default)]
    pub authority: Authority,
    #[serde(default, flatten)]
    pub extensions: BTreeMap<String, Value>,
}

fn default_goal_schema() -> u32 {
    1
}

fn default_constraints() -> Vec<ConstraintSpec> {
    [
        "preserve-semantics",
        "no-critical-safety-regression",
        "no-evidence-strength-regression",
    ]
    .into_iter()
    .map(ConstraintSpec::required)
    .collect()
}

impl Default for GoalSpec {
    fn default() -> Self {
        Self {
            schema_version: default_goal_schema(),
            target: TargetSelector::default(),
            objectives: Vec::new(),
            constraints: default_constraints(),
            budget: GoalBudget::default(),
            authority: Authority::Apply,
            extensions: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConvergeRequest {
    pub spec_path: Option<PathBuf>,
    pub target: Option<String>,
    pub objectives: Vec<String>,
    pub constraints: Vec<String>,
    pub budget_ms: Option<u64>,
    pub authority: Option<Authority>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConvergePhase {
    Discover,
    CompileGoal,
    PlanEvidence,
    ExecuteGenerate,
    Verify,
    Replan,
    Apply,
    PostVerify,
    Complete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseTransition {
    pub phase: ConvergePhase,
    pub status: String,
    pub summary: String,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DecisionStatus {
    Assessed,
    ReadyToApply,
    Converged,
    NoChange,
    Incomplete,
    RolledBack,
    Failed,
}

impl DecisionStatus {
    pub fn successful(self) -> bool {
        matches!(
            self,
            Self::Assessed | Self::ReadyToApply | Self::Converged | Self::NoChange
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnresolvedGoal {
    pub id: String,
    pub reason: String,
    #[serde(default)]
    pub candidate_id: Option<RepairCandidateId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluatorContract {
    pub clause_id: String,
    pub evaluator_id: String,
    #[serde(default)]
    pub required_providers: Vec<String>,
    pub candidate_bound: bool,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateDecision {
    pub candidate_id: RepairCandidateId,
    pub eligible: bool,
    #[serde(default)]
    pub required_providers: Vec<String>,
    pub reason: String,
    #[serde(default)]
    pub verification: Option<CandidateEvidenceVerification>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayRecipe {
    pub working_directory: String,
    pub command: Vec<String>,
    #[serde(default)]
    pub transaction_manifests: Vec<String>,
    pub rollback_command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionBundle {
    pub schema_version: u32,
    pub status: DecisionStatus,
    pub goal: GoalSpec,
    pub workspace: String,
    pub source: String,
    pub manifest: String,
    pub phases: Vec<PhaseTransition>,
    pub evaluator_contracts: Vec<EvaluatorContract>,
    pub initial_analysis: FindingReport,
    pub final_analysis: FindingReport,
    pub candidate_decisions: Vec<CandidateDecision>,
    pub selected: Vec<RepairCandidateId>,
    pub evidence_plans: Vec<EvidencePlan>,
    pub transactions: Vec<RepairTransaction>,
    #[serde(default)]
    pub post_apply_evidence: Vec<CandidateEvidenceVerification>,
    pub unresolved: Vec<UnresolvedGoal>,
    pub replay: ReplayRecipe,
}

pub fn load_goal_spec(request: &ConvergeRequest) -> Result<GoalSpec, String> {
    load_goal_spec_with_fallback(request, None)
}

fn load_goal_spec_with_fallback(
    request: &ConvergeRequest,
    fallback: Option<&GoalSpec>,
) -> Result<GoalSpec, String> {
    let mut goal = if let Some(path) = &request.spec_path {
        let source = std::fs::read_to_string(path)
            .map_err(|error| format!("failed to read GoalSpec {}: {error}", path.display()))?;
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("json") => serde_json::from_str(&source)
                .map_err(|error| format!("invalid GoalSpec JSON: {error}"))?,
            Some("toml") => toml::from_str(&source)
                .map_err(|error| format!("invalid GoalSpec TOML: {error}"))?,
            _ => serde_json::from_str(&source)
                .or_else(|_| toml::from_str(&source))
                .map_err(|error| format!("invalid GoalSpec: {error}"))?,
        }
    } else {
        fallback.cloned().unwrap_or_default()
    };
    if let Some(target) = &request.target {
        goal.target = TargetSelector {
            selector: if Path::new(target).is_file() {
                "path".to_string()
            } else {
                "target".to_string()
            },
            value: Some(target.clone()),
            extensions: BTreeMap::new(),
        };
    }
    if !request.objectives.is_empty() {
        goal.objectives = request
            .objectives
            .iter()
            .cloned()
            .map(ObjectiveSpec::named)
            .collect();
    }
    for constraint in &request.constraints {
        if !goal.constraints.iter().any(|item| item.id == *constraint) {
            goal.constraints
                .push(ConstraintSpec::required(constraint.clone()));
        }
    }
    if let Some(budget_ms) = request.budget_ms {
        goal.budget.wall_time_ms = budget_ms;
    }
    if let Some(authority) = request.authority {
        goal.authority = authority;
    }
    if goal.budget.wall_time_ms == 0 || goal.budget.max_iterations == 0 {
        return Err("GoalSpec budget and max_iterations must be greater than zero".to_string());
    }
    Ok(goal)
}

pub fn converge(request: &ConvergeRequest) -> Result<DecisionBundle, String> {
    let started = Instant::now();
    let config = CovOptConfig::load_or_embedded(".covopt.toml")?;
    let mut goal = load_goal_spec_with_fallback(request, config.converge.as_ref())?;
    let workspace = std::env::current_dir()
        .map_err(|error| error.to_string())?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let manifest = workspace.join("Cargo.toml");
    if !manifest.is_file() {
        return Err(format!(
            "converge requires Cargo.toml in workspace {}",
            workspace.display()
        ));
    }
    let source = resolve_source(&goal.target, &config, &workspace)?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !source.starts_with(&workspace) {
        return Err("GoalSpec target escapes the current workspace".to_string());
    }

    let mut phases = Vec::new();
    transition(
        &mut phases,
        ConvergePhase::Discover,
        "complete",
        format!("resolved {}", source.display()),
        started,
    );
    let codegen_config = crate::codegen_optimizer::CodegenConfig {
        lto: config.optimization.codegen.lto.clone(),
        codegen_units: config.optimization.codegen.codegen_units,
        opt_level: config.optimization.codegen.opt_level.clone(),
        target_cpu: config.optimization.codegen.target_cpu.clone(),
        max_candidates: config.optimization.codegen.max_candidates,
    };
    let layout_config = crate::layout_optimizer::LayoutConfig {
        max_candidates: config.optimization.layout.max_candidates,
        allow_public_abi_suggestions: config.optimization.layout.allow_public_abi_suggestions,
        cache_line_bytes: config.optimization.layout.cache_line_bytes,
    };
    let mut report = crate::findings::analyze_source(
        &source,
        goal.target
            .value
            .as_deref()
            .filter(|_| goal.target.selector == "function"),
        &codegen_config,
        &layout_config,
    )?;
    add_atomic_candidate(&source, &config, &mut report, goal.budget.wall_time_ms);
    let initial_analysis = report.clone();
    infer_objectives(&mut goal, &report);
    let (evaluator_contracts, mut unresolved, base_providers) = compile_goal(&goal);
    transition(
        &mut phases,
        ConvergePhase::CompileGoal,
        if unresolved.is_empty() {
            "complete"
        } else {
            "unresolved"
        },
        format!(
            "compiled {} evaluator contracts; {} unresolved",
            evaluator_contracts.len(),
            unresolved.len()
        ),
        started,
    );

    let mut bundle = DecisionBundle {
        schema_version: 1,
        status: DecisionStatus::Incomplete,
        goal,
        workspace: workspace.display().to_string(),
        source: source.display().to_string(),
        manifest: manifest.display().to_string(),
        phases,
        evaluator_contracts,
        initial_analysis,
        final_analysis: report.clone(),
        candidate_decisions: Vec::new(),
        selected: Vec::new(),
        evidence_plans: Vec::new(),
        transactions: Vec::new(),
        post_apply_evidence: Vec::new(),
        unresolved: Vec::new(),
        replay: ReplayRecipe {
            working_directory: workspace.display().to_string(),
            command: vec!["covopt".to_string(), "converge".to_string()],
            transaction_manifests: Vec::new(),
            rollback_command: "covopt fix --rollback <manifest>".to_string(),
        },
    };

    if !unresolved.is_empty() {
        bundle.unresolved.append(&mut unresolved);
        bundle.status = DecisionStatus::Incomplete;
        complete_transition(&mut bundle, started);
        return Ok(bundle);
    }
    if report.findings.is_empty() {
        bundle.status = DecisionStatus::NoChange;
        complete_transition(&mut bundle, started);
        return Ok(bundle);
    }

    let mut attempted = HashSet::new();
    let mut applied_any = false;
    let mut rolled_back_any = false;
    let mut ready = false;
    for iteration in 0..bundle.goal.budget.max_iterations {
        if started.elapsed().as_millis() as u64 >= bundle.goal.budget.wall_time_ms {
            bundle.unresolved.push(UnresolvedGoal {
                id: "budget".to_string(),
                reason: "convergence wall-clock budget was exhausted".to_string(),
                candidate_id: None,
            });
            break;
        }
        let mut candidates = report.repair_candidates.clone();
        candidates.sort_by(|left, right| {
            candidate_rank(right, &report).cmp(&candidate_rank(left, &report))
        });
        let Some(candidate) = candidates
            .into_iter()
            .find(|candidate| !attempted.contains(&candidate.id))
        else {
            break;
        };
        attempted.insert(candidate.id.clone());
        transition(
            &mut bundle.phases,
            ConvergePhase::ExecuteGenerate,
            "complete",
            format!("iteration {} generated {}", iteration + 1, candidate.id),
            started,
        );

        if candidate.suggestion_only || candidate.changes.is_empty() {
            let missing_materializer = candidate.changes.is_empty();
            let reason = if missing_materializer {
                "candidate has no deterministic materializer".to_string()
            } else {
                format!(
                    "candidate is materialized but an evaluator/contract boundary is unresolved: {}",
                    candidate.description
                )
            };
            bundle.candidate_decisions.push(CandidateDecision {
                candidate_id: candidate.id.clone(),
                eligible: false,
                required_providers: Vec::new(),
                reason: reason.clone(),
                verification: None,
            });
            bundle.unresolved.push(UnresolvedGoal {
                id: if missing_materializer {
                    "materializer"
                } else {
                    "evaluator-contract"
                }
                .to_string(),
                reason,
                candidate_id: Some(candidate.id),
            });
            continue;
        }

        let required = match candidate_providers(&candidate, &base_providers) {
            Ok(providers) => providers,
            Err(reason) => {
                bundle.candidate_decisions.push(CandidateDecision {
                    candidate_id: candidate.id.clone(),
                    eligible: false,
                    required_providers: Vec::new(),
                    reason: reason.clone(),
                    verification: None,
                });
                bundle.unresolved.push(UnresolvedGoal {
                    id: "evidence-route".to_string(),
                    reason,
                    candidate_id: Some(candidate.id),
                });
                continue;
            }
        };
        let remaining = match remaining_budget(&bundle.goal, started) {
            Ok(remaining) => remaining,
            Err(reason) => {
                bundle.unresolved.push(UnresolvedGoal {
                    id: "budget".to_string(),
                    reason,
                    candidate_id: Some(candidate.id.clone()),
                });
                break;
            }
        };
        let plan = evidence_plan(&required, remaining);
        transition(
            &mut bundle.phases,
            ConvergePhase::PlanEvidence,
            "complete",
            format!(
                "routed {} through {} providers",
                candidate.id,
                required.len()
            ),
            started,
        );
        let edits = candidate.changes.clone();
        let verification =
            verify_candidate_evidence_in_sandbox(&workspace, &manifest, &edits, &plan, None)?;
        bundle.evidence_plans.push(plan);
        bundle.candidate_decisions.push(CandidateDecision {
            candidate_id: candidate.id.clone(),
            eligible: verification.passed,
            required_providers: required.iter().cloned().collect(),
            reason: if verification.passed {
                "all candidate-bound evaluator contracts passed".to_string()
            } else {
                format!(
                    "candidate-bound evidence failed: {}",
                    verification.failed_actions.join(", ")
                )
            },
            verification: Some(verification.clone()),
        });
        transition(
            &mut bundle.phases,
            ConvergePhase::Verify,
            if verification.passed {
                "passed"
            } else {
                "failed"
            },
            format!("verified {} against its exact patch", candidate.id),
            started,
        );
        if !verification.passed {
            transition(
                &mut bundle.phases,
                ConvergePhase::Replan,
                "complete",
                format!("rejected {} and continued", candidate.id),
                started,
            );
            continue;
        }

        match bundle.goal.authority {
            Authority::ReadOnly => {
                ready = true;
                break;
            }
            Authority::Suggest => {
                bundle.selected.push(candidate.id);
                ready = true;
                break;
            }
            Authority::Apply => {}
        }

        let transaction = apply_edits_transactionally(&workspace, &edits)?;
        transition(
            &mut bundle.phases,
            ConvergePhase::Apply,
            "committed",
            format!("transactionally applied {}", candidate.id),
            started,
        );
        let post_result = (|| -> Result<_, String> {
            let post_plan = evidence_plan(
                &BTreeSet::from([
                    "StaticAst".to_string(),
                    "Compiler".to_string(),
                    "Test".to_string(),
                ]),
                remaining_budget(&bundle.goal, started)?,
            );
            let post_evidence =
                verify_candidate_evidence_in_sandbox(&workspace, &manifest, &[], &post_plan, None)?;
            let mut post_report = crate::findings::analyze_source(
                &source,
                bundle
                    .goal
                    .target
                    .value
                    .as_deref()
                    .filter(|_| bundle.goal.target.selector == "function"),
                &codegen_config,
                &layout_config,
            )?;
            add_atomic_candidate(
                &source,
                &config,
                &mut post_report,
                bundle.goal.budget.wall_time_ms,
            );
            Ok((post_evidence, post_report))
        })();
        let (post_evidence, post_report) = match post_result {
            Ok(result) => result,
            Err(error) => {
                let rolled_back = rollback_transaction(Path::new(&transaction.manifest_path))?;
                bundle.transactions.push(rolled_back);
                rolled_back_any = true;
                if let Some(decision) = bundle.candidate_decisions.last_mut() {
                    decision.eligible = false;
                    decision.reason = format!(
                        "post-apply verification could not complete; transaction rolled back: {error}"
                    );
                }
                transition(
                    &mut bundle.phases,
                    ConvergePhase::PostVerify,
                    "rolled-back",
                    format!("post-apply verification error; automatic rollback completed: {error}"),
                    started,
                );
                continue;
            }
        };
        let critical_regression = has_new_critical_finding(&report, &post_report);
        if !post_evidence.passed || critical_regression {
            let manifest_path = transaction.manifest_path.clone();
            let rolled_back = rollback_transaction(Path::new(&manifest_path))?;
            bundle.transactions.push(rolled_back);
            bundle.post_apply_evidence.push(post_evidence);
            rolled_back_any = true;
            transition(
                &mut bundle.phases,
                ConvergePhase::PostVerify,
                "rolled-back",
                if critical_regression {
                    "new critical finding detected; automatic rollback completed".to_string()
                } else {
                    "post-apply evidence failed; automatic rollback completed".to_string()
                },
                started,
            );
            transition(
                &mut bundle.phases,
                ConvergePhase::Replan,
                "complete",
                format!("rolled back {} and continued", candidate.id),
                started,
            );
            continue;
        }
        bundle
            .replay
            .transaction_manifests
            .push(transaction.manifest_path.clone());
        bundle.transactions.push(transaction);
        bundle.post_apply_evidence.push(post_evidence);
        bundle.selected.push(candidate.id);
        applied_any = true;
        report = post_report;
        bundle.final_analysis = report.clone();
        transition(
            &mut bundle.phases,
            ConvergePhase::PostVerify,
            "passed",
            "workspace matches the verified patch and post-apply checks passed",
            started,
        );
        if report.findings.is_empty() {
            break;
        }
        transition(
            &mut bundle.phases,
            ConvergePhase::Replan,
            "continue",
            format!(
                "{} findings remain; compile the next evidence-backed repair",
                report.findings.len()
            ),
            started,
        );
    }

    bundle.final_analysis = report.clone();
    if ready {
        bundle.status = match bundle.goal.authority {
            Authority::ReadOnly => DecisionStatus::Assessed,
            Authority::Suggest => DecisionStatus::ReadyToApply,
            Authority::Apply => DecisionStatus::Incomplete,
        };
    } else if applied_any && bundle.unresolved.is_empty() {
        bundle.status = DecisionStatus::Converged;
    } else if applied_any {
        bundle.status = DecisionStatus::Incomplete;
        bundle.unresolved.push(UnresolvedGoal {
            id: "proof-frontier".to_string(),
            reason: format!(
                "{} findings remain after verified repairs",
                report.findings.len()
            ),
            candidate_id: None,
        });
    } else if rolled_back_any {
        bundle.status = DecisionStatus::RolledBack;
    } else {
        bundle.status = DecisionStatus::Incomplete;
        if bundle.unresolved.is_empty() {
            bundle.unresolved.push(UnresolvedGoal {
                id: "proof-frontier".to_string(),
                reason: "no materialized candidate satisfied every evaluator contract".to_string(),
                candidate_id: None,
            });
        }
    }
    complete_transition(&mut bundle, started);
    Ok(bundle)
}

pub fn write_decision_bundle(bundle: &DecisionBundle, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(bundle).map_err(|error| error.to_string())?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, path).map_err(|error| error.to_string())
}

fn resolve_source(
    selector: &TargetSelector,
    config: &CovOptConfig,
    workspace: &Path,
) -> Result<PathBuf, String> {
    if let Some(value) = selector.value.as_deref() {
        let path = PathBuf::from(value);
        if path.is_file() {
            return Ok(path);
        }
        if let Some(source) = crate::assurance::find_target_source(value) {
            return Ok(source);
        }
        if let Some(target) = config
            .target
            .iter()
            .find(|target| CovOptConfig::target_id(target) == value || target.test == value)
            && let Some(source) = crate::assurance::find_target_source(&target.test)
        {
            return Ok(source);
        }
        return Err(format!("could not resolve GoalSpec target '{value}'"));
    }
    for target in &config.target {
        if let Some(source) = crate::assurance::find_target_source(&target.test) {
            return Ok(source);
        }
    }
    for path in [workspace.join("src/lib.rs"), workspace.join("src/main.rs")] {
        if path.is_file() {
            return Ok(path);
        }
    }
    let source_dir = workspace.join("src");
    if source_dir.is_dir()
        && let Some(path) = std::fs::read_dir(source_dir)
            .map_err(|error| error.to_string())?
            .flatten()
            .map(|entry| entry.path())
            .find(|path| path.extension().and_then(|value| value.to_str()) == Some("rs"))
    {
        return Ok(path);
    }
    Err("GoalSpec target=auto found no Rust source".to_string())
}

fn infer_objectives(goal: &mut GoalSpec, report: &FindingReport) {
    if !goal.objectives.is_empty() {
        return;
    }
    let mut ids = BTreeSet::new();
    for finding in &report.findings {
        let id = if finding.kind.is_codegen() {
            "codegen-overhead"
        } else if finding.kind.is_layout() {
            "memory-layout"
        } else if finding.kind == FindingKind::ManualCasLoop {
            "atomic-ordering"
        } else if matches!(
            finding.kind,
            FindingKind::UnsafeRisk | FindingKind::LockGuardEscape
        ) {
            "safety"
        } else {
            "runtime-overhead"
        };
        ids.insert(id.to_string());
    }
    goal.objectives = ids.into_iter().map(ObjectiveSpec::named).collect();
}

fn compile_goal(
    goal: &GoalSpec,
) -> (
    Vec<EvaluatorContract>,
    Vec<UnresolvedGoal>,
    BTreeSet<String>,
) {
    let mut contracts = Vec::new();
    let mut unresolved = Vec::new();
    let mut providers = BTreeSet::new();
    for objective in &goal.objectives {
        if !objective.weight.is_finite() || objective.weight <= 0.0 {
            unresolved.push(UnresolvedGoal {
                id: objective.id.clone(),
                reason: "objective weight must be finite and greater than zero".to_string(),
                candidate_id: None,
            });
            continue;
        }
        if !matches!(
            objective.direction.as_str(),
            "minimize" | "maximize" | "target"
        ) {
            unresolved.push(UnresolvedGoal {
                id: objective.id.clone(),
                reason: format!("unknown objective direction '{}'", objective.direction),
                candidate_id: None,
            });
            continue;
        }
        if !matches!(
            objective.acceptance.operator.as_str(),
            "improve" | "no-regression"
        ) {
            unresolved.push(UnresolvedGoal {
                id: objective.id.clone(),
                reason: format!(
                    "acceptance rule '{}' has no registered executable evaluator",
                    objective.acceptance.operator
                ),
                candidate_id: None,
            });
            continue;
        }
        let evaluator = objective.metric.evaluator.as_ref();
        compile_evaluator(
            &objective.id,
            evaluator.map_or(objective.metric.id.as_str(), |item| item.id.as_str()),
            evaluator.and_then(|item| item.provider.as_deref()),
            &mut contracts,
            &mut unresolved,
            &mut providers,
        );
    }
    for constraint in goal
        .constraints
        .iter()
        .filter(|constraint| constraint.required)
    {
        compile_evaluator(
            &constraint.id,
            constraint
                .evaluator
                .as_ref()
                .map_or(constraint.id.as_str(), |item| item.id.as_str()),
            constraint
                .evaluator
                .as_ref()
                .and_then(|item| item.provider.as_deref()),
            &mut contracts,
            &mut unresolved,
            &mut providers,
        );
    }
    (contracts, unresolved, providers)
}

fn compile_evaluator(
    clause_id: &str,
    evaluator_id: &str,
    explicit_provider: Option<&str>,
    contracts: &mut Vec<EvaluatorContract>,
    unresolved: &mut Vec<UnresolvedGoal>,
    providers: &mut BTreeSet<String>,
) {
    let normalized = evaluator_id.to_ascii_lowercase().replace('_', "-");
    let (required, summary): (Vec<&str>, &str) = match normalized.as_str() {
        "codegen-overhead"
        | "runtime-overhead"
        | "latency"
        | "reciprocal-throughput"
        | "ipc"
        | "code-size" => (
            vec!["StaticAst", "Compiler", "Mca"],
            "compare the exact candidate's generated instructions with llvm-mca",
        ),
        "memory-layout" | "field-locality" | "contention" | "no-memory-regression" => (
            vec!["StaticAst", "Compiler", "Test"],
            "validate the materialized layout model, compiled layout, and workload tests",
        ),
        "atomic-ordering" => (
            vec!["StaticAst", "Compiler", "Test", "AtomicModel"],
            "check the exact atomic patch against the configured bounded contract",
        ),
        "safety" | "preserve-semantics" => (
            vec!["StaticAst", "Compiler", "Test"],
            "require parse, compile, and project-test preservation",
        ),
        "no-critical-safety-regression" => (
            vec!["StaticAst", "Compiler"],
            "compare critical static findings before and after apply",
        ),
        "no-evidence-strength-regression" => (
            vec!["Compiler"],
            "require every risk-routed provider to pass for the exact candidate",
        ),
        "coverage" | "line-coverage" => (
            vec!["Compiler", "Coverage"],
            "run candidate-bound coverage when explicitly requested",
        ),
        _ => {
            if let Some(provider) = explicit_provider {
                if supported_candidate_provider(provider) {
                    (
                        vec![provider],
                        "execute the explicitly declared candidate-bound provider",
                    )
                } else {
                    unresolved.push(UnresolvedGoal {
                        id: clause_id.to_string(),
                        reason: format!(
                            "evaluator '{evaluator_id}' declares provider '{provider}', but no candidate-bound adapter is installed"
                        ),
                        candidate_id: None,
                    });
                    return;
                }
            } else {
                unresolved.push(UnresolvedGoal {
                    id: clause_id.to_string(),
                    reason: format!(
                        "unknown evaluator '{evaluator_id}'; declare a registered evaluator or an executable provider contract"
                    ),
                    candidate_id: None,
                });
                return;
            }
        }
    };
    for provider in &required {
        providers.insert((*provider).to_string());
    }
    contracts.push(EvaluatorContract {
        clause_id: clause_id.to_string(),
        evaluator_id: evaluator_id.to_string(),
        required_providers: required.into_iter().map(str::to_string).collect(),
        candidate_bound: true,
        summary: summary.to_string(),
    });
}

fn supported_candidate_provider(provider: &str) -> bool {
    matches!(
        provider,
        "StaticAst" | "Compiler" | "Test" | "Coverage" | "Mca" | "AtomicModel"
    )
}

fn candidate_providers(
    candidate: &RepairCandidate,
    base: &BTreeSet<String>,
) -> Result<BTreeSet<String>, String> {
    let mut providers = base.clone();
    providers.insert("StaticAst".to_string());
    providers.insert("Compiler".to_string());
    let strongest = [
        candidate.semantic_risk,
        candidate.api_risk,
        candidate.abi_risk,
    ]
    .into_iter()
    .max()
    .unwrap_or(RiskLevel::Low);
    if strongest >= RiskLevel::Medium {
        providers.insert("Test".to_string());
    }
    if strongest >= RiskLevel::High {
        match candidate.kind {
            RepairKind::ReplaceManualCas | RepairKind::SeparateAtomic => {
                providers.insert("AtomicModel".to_string());
            }
            RepairKind::AddInline
            | RepairKind::RemoveInline
            | RepairKind::MarkCold
            | RepairKind::SplitHotCold => {
                providers.insert("Mca".to_string());
            }
            RepairKind::ReorderFields | RepairKind::AddPadding | RepairKind::AlignCacheLine => {
                providers.insert("Test".to_string());
            }
            _ => {
                return Err(format!(
                    "{} is high/unknown risk and has no specialized candidate-bound evidence route",
                    candidate.id
                ));
            }
        }
    }
    Ok(providers)
}

fn evidence_plan(providers: &BTreeSet<String>, budget_ms: u64) -> EvidencePlan {
    // Compiler is mandatory and MCA executes both baseline and candidate, so
    // reserve two extra slices rather than letting each command consume the
    // complete GoalSpec wall-clock budget.
    let action_budget = (budget_ms / (providers.len() as u64 + 2)).max(1);
    let details = providers
        .iter()
        .map(|provider| {
            let mut action = EvidenceAction::new(
                format!("converge-{}", provider.to_ascii_lowercase()),
                provider.clone(),
                Vec::new(),
                action_budget,
                1.0,
            );
            action.result_status = crate::assurance::provider_status_ceiling(
                provider_kind(provider).unwrap_or(EvidenceProviderKind::StaticAst),
            );
            action
        })
        .collect::<Vec<_>>();
    let coverage = empty_coverage();
    EvidencePlan {
        status: PlanStatus::Feasible,
        selected_actions: details.iter().map(|action| action.id.clone()).collect(),
        selected_action_details: details.clone(),
        candidate_actions: details,
        rejected_actions: Vec::new(),
        coverage_before: coverage.clone(),
        expected_coverage: coverage,
        actual_coverage: None,
        estimated_cost_ms: action_budget,
        actual_cost_ms: None,
        infeasible_obligations: Vec::new(),
        validator_errors: Vec::new(),
    }
}

fn provider_kind(provider: &str) -> Option<EvidenceProviderKind> {
    match provider {
        "StaticAst" => Some(EvidenceProviderKind::StaticAst),
        "Compiler" => Some(EvidenceProviderKind::Compiler),
        "Mca" => Some(EvidenceProviderKind::Mca),
        "Coverage" => Some(EvidenceProviderKind::Coverage),
        "Test" => Some(EvidenceProviderKind::Test),
        "AtomicModel" => Some(EvidenceProviderKind::AtomicModel),
        _ => None,
    }
}

fn empty_coverage() -> EvidenceCoverage {
    EvidenceCoverage {
        resolved_weight: 0.0,
        total_weight: 0.0,
        overall_percent: 100.0,
        critical_safety_resolved_weight: 0.0,
        critical_safety_total_weight: 0.0,
        critical_safety_percent: 100.0,
        performance_resolved_weight: 0.0,
        performance_total_weight: 0.0,
        performance_percent: 100.0,
        unknown_obligation_count: 0,
        failed_obligation_count: 0,
    }
}

fn candidate_rank(candidate: &RepairCandidate, report: &FindingReport) -> (u8, u64, usize) {
    let severity = candidate
        .resolves
        .iter()
        .filter_map(|id| report.findings.iter().find(|finding| &finding.id == id))
        .map(|finding| match finding.severity {
            Severity::Critical => 4,
            Severity::High => 3,
            Severity::Medium => 2,
            Severity::Low => 1,
        })
        .max()
        .unwrap_or(0);
    (
        severity,
        (candidate.estimated_benefit.confidence.clamp(0.0, 1.0) * 1_000_000.0) as u64,
        usize::MAX.saturating_sub(candidate.changes.len()),
    )
}

fn has_new_critical_finding(before: &FindingReport, after: &FindingReport) -> bool {
    let previous = before
        .findings
        .iter()
        .filter(|finding| finding.severity == Severity::Critical)
        .map(|finding| finding.id.clone())
        .collect::<HashSet<_>>();
    after
        .findings
        .iter()
        .filter(|finding| finding.severity == Severity::Critical)
        .any(|finding| !previous.contains(&finding.id))
}

fn remaining_budget(goal: &GoalSpec, started: Instant) -> Result<u64, String> {
    goal.budget
        .wall_time_ms
        .checked_sub(started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64)
        .filter(|remaining| *remaining > 0)
        .ok_or_else(|| "convergence wall-clock budget was exhausted".to_string())
}

fn add_atomic_candidate(
    source: &Path,
    config: &CovOptConfig,
    report: &mut FindingReport,
    budget_ms: u64,
) {
    let resolves = report
        .findings
        .iter()
        .filter(|finding| finding.kind == FindingKind::ManualCasLoop)
        .map(|finding| finding.id.clone())
        .collect::<Vec<_>>();
    if resolves.is_empty() || !config.atomic.enabled || !config.atomic.synthesize {
        return;
    }
    let Ok(request) = crate::atomic_synth::request_from_file(
        source,
        config.atomic.correctness_contract(),
        config.atomic.bounds(),
        config
            .atomic
            .timeout_ms
            .unwrap_or(budget_ms.clamp(1, 5_000)),
        true,
    ) else {
        return;
    };
    let synthesis = crate::atomic_synth::synthesize(&request);
    let (Some(selected), Some(patch)) = (synthesis.selected, synthesis.patch) else {
        return;
    };
    let id = RepairCandidateId(format!("atomic-{}", selected.id));
    report.repair_candidates.push(RepairCandidate {
        id: id.clone(),
        kind: RepairKind::ReplaceManualCas,
        resolves: resolves.clone(),
        changes: crate::atomic_synth::patch_source_edits(&patch),
        dependencies: Vec::new(),
        conflicts: Vec::new(),
        semantic_risk: RiskLevel::High,
        api_risk: RiskLevel::Low,
        abi_risk: RiskLevel::Low,
        estimated_benefit: Default::default(),
        verification: report
            .findings
            .iter()
            .filter(|finding| resolves.contains(&finding.id))
            .flat_map(|finding| finding.obligations.iter().cloned())
            .collect(),
        suggestion_only: false,
        description: format!("bounded atomic repair; {}", synthesis.bounded_scope),
    });
    for finding in &mut report.findings {
        if resolves.contains(&finding.id) && !finding.repair_candidates.contains(&id) {
            finding.repair_candidates.push(id.clone());
        }
    }
}

fn transition(
    phases: &mut Vec<PhaseTransition>,
    phase: ConvergePhase,
    status: impl Into<String>,
    summary: impl Into<String>,
    started: Instant,
) {
    phases.push(PhaseTransition {
        phase,
        status: status.into(),
        summary: summary.into(),
        elapsed_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
    });
}

fn complete_transition(bundle: &mut DecisionBundle, started: Instant) {
    transition(
        &mut bundle.phases,
        ConvergePhase::Complete,
        format!("{:?}", bundle.status).to_ascii_lowercase(),
        format!(
            "selected {}; unresolved {}",
            bundle.selected.len(),
            bundle.unresolved.len()
        ),
        started,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goal_defaults_to_apply_with_open_default_constraints() {
        let goal = GoalSpec::default();
        assert_eq!(goal.authority, Authority::Apply);
        assert!(
            goal.constraints
                .iter()
                .any(|constraint| constraint.id == "preserve-semantics")
        );
    }

    #[test]
    fn unknown_evaluator_is_unresolved_instead_of_passing() {
        let goal = GoalSpec {
            objectives: vec![ObjectiveSpec::named("future-unknown-metric")],
            ..Default::default()
        };
        let (_, unresolved, _) = compile_goal(&goal);
        assert!(
            unresolved
                .iter()
                .any(|item| item.id == "future-unknown-metric")
        );
    }

    #[test]
    fn custom_evaluator_can_bind_a_supported_provider() {
        let mut objective = ObjectiveSpec::named("future-metric");
        objective.metric.evaluator = Some(EvaluatorSpec {
            id: "vendor.metric.v1".to_string(),
            version: 1,
            provider: Some("Test".to_string()),
            config: Value::Null,
            extensions: BTreeMap::new(),
        });
        let goal = GoalSpec {
            objectives: vec![objective],
            ..Default::default()
        };
        let (_, unresolved, providers) = compile_goal(&goal);
        assert!(unresolved.is_empty());
        assert!(providers.contains("Test"));
    }
}
