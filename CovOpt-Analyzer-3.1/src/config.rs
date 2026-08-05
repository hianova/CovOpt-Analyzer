use crate::assurance::{AssurancePolicy, DEFAULT_EVIDENCE_THRESHOLD, PlannerKind, PlannerPolicy};
use crate::atomic_model::{AtomicContract, ContractKind, ForbiddenOutcome, ModelBounds};
use crate::trial_selection::TrialSelectionConfig;
use covopt_macro::covopt_param;
use serde::de::Error as DeError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_config_version() -> u32 {
    2
}

fn default_evidence_threshold() -> f64 {
    covopt_param!("COVOPT_EVIDENCE_THRESHOLD", DEFAULT_EVIDENCE_THRESHOLD)
}

#[derive(Deserialize, Debug, Clone)]
pub struct PipelineConfig {
    #[serde(default = "default_true")]
    pub run_fix: bool,
    #[serde(default = "default_true")]
    pub run_audit: bool,
    #[serde(default = "default_false")]
    pub run_optimize: bool,
    #[serde(default = "default_false")]
    pub run_harden: bool,
    #[serde(default)]
    pub assurance: AssurancePolicy,
    #[serde(default = "default_evidence_threshold")]
    pub evidence_threshold: f64,
}

#[derive(Deserialize, Debug, Clone)]
pub struct AssuranceToolsConfig {
    #[serde(default = "default_true")]
    pub llvm_mca: bool,
    #[serde(default = "default_true")]
    pub llvm_cov: bool,
    #[serde(default)]
    pub sanitizer: bool,
    #[serde(default)]
    pub profile: bool,
    #[serde(default = "default_true")]
    pub compiler: bool,
    #[serde(default = "default_true")]
    pub tests: bool,
}

impl Default for AssuranceToolsConfig {
    fn default() -> Self {
        Self {
            llvm_mca: true,
            llvm_cov: true,
            sanitizer: false,
            profile: false,
            compiler: true,
            tests: true,
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct AssuranceConfig {
    #[serde(default)]
    pub mode: AssurancePolicy,
    #[serde(default = "default_evidence_threshold")]
    pub overall_coverage: f64,
    #[serde(default = "default_critical_threshold")]
    pub critical_coverage: f64,
    #[serde(default = "default_evidence_threshold")]
    pub performance_coverage: f64,
    #[serde(default = "default_true")]
    pub fail_on_critical_unknown: bool,
    #[serde(default = "default_budget_seconds")]
    pub budget_seconds: u64,
    #[serde(default)]
    pub planner: PlannerKind,
    #[serde(default)]
    pub static_only: bool,
    #[serde(default)]
    pub exact_threshold: Option<usize>,
    #[serde(default)]
    pub planner_timeout_ms: Option<u64>,
    #[serde(default)]
    pub allowed_external_tools: Option<Vec<String>>,
    #[serde(default)]
    pub tools: AssuranceToolsConfig,
}

fn default_critical_threshold() -> f64 {
    1.0
}

fn default_budget_seconds() -> u64 {
    30
}

impl Default for AssuranceConfig {
    fn default() -> Self {
        Self {
            mode: AssurancePolicy::Adaptive,
            overall_coverage: default_evidence_threshold(),
            critical_coverage: default_critical_threshold(),
            performance_coverage: default_evidence_threshold(),
            fail_on_critical_unknown: true,
            budget_seconds: default_budget_seconds(),
            planner: PlannerKind::Hybrid,
            static_only: false,
            exact_threshold: None,
            planner_timeout_ms: None,
            allowed_external_tools: None,
            tools: AssuranceToolsConfig::default(),
        }
    }
}

impl AssuranceConfig {
    pub fn planner_policy(&self) -> PlannerPolicy {
        let mut policy = PlannerPolicy::for_assurance(self.mode);
        policy.overall_threshold = self.overall_coverage;
        policy.critical_threshold = self.critical_coverage;
        policy.performance_threshold = self.performance_coverage;
        policy.max_time_ms = Some(self.budget_seconds.saturating_mul(1_000));
        policy.planner = self.planner;
        policy.static_only |= self.static_only;
        if let Some(exact_threshold) = self.exact_threshold {
            policy.exact_threshold = exact_threshold;
        }
        if let Some(timeout_ms) = self.planner_timeout_ms {
            policy.timeout_ms = timeout_ms;
        }
        let configured_tools = [
            ("llvm-mca", self.tools.llvm_mca),
            ("llvm-cov", self.tools.llvm_cov),
            ("cargo", self.tools.tests || self.tools.sanitizer),
            ("rustc", self.tools.compiler),
            ("samply", self.tools.profile),
        ]
        .into_iter()
        .filter_map(|(tool, enabled)| enabled.then_some(tool.to_string()))
        .collect();
        if let Some(allowed) = &self.allowed_external_tools {
            policy.allowed_external_tools = allowed.clone();
        } else {
            policy.allowed_external_tools = configured_tools;
        }
        policy
    }
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            run_fix: true,
            run_audit: true,
            run_optimize: false,
            run_harden: false,
            assurance: AssurancePolicy::Adaptive,
            evidence_threshold: default_evidence_threshold(),
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct TargetConfig {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub function: Option<String>,
    #[serde(default)]
    pub complexity: Option<String>,
    #[serde(default)]
    pub policy: Option<String>,
    #[serde(default)]
    pub providers: Option<ProvidersConfig>,
    #[serde(default)]
    pub test: String,
    pub tests: Option<String>,
    pub package: Option<String>,
    pub expected: Option<String>,
    pub n_values: Option<String>,
    pub fuzz_iterations: Option<u32>,
    pub mca_cpu: Option<String>,
    pub ignore: Option<Vec<String>>,
    #[serde(default = "default_true")]
    pub require_cache_padding: bool,
    #[serde(default = "default_true")]
    pub require_branch_hints: bool,
    #[serde(default = "default_true")]
    pub require_aerospace_grade: bool,
    #[serde(default = "default_true")]
    pub require_watchdog_timeout: bool,
    #[serde(default = "default_true")]
    pub require_stress_test: bool,
    pub polling_threshold: Option<u64>,
    #[serde(default)]
    pub assurance: Option<AssurancePolicy>,
    #[serde(default)]
    pub evidence_threshold: Option<f64>,
    #[serde(default)]
    pub budget_seconds: Option<u64>,
    #[serde(default)]
    pub planner: Option<PlannerKind>,
    #[serde(default)]
    pub static_only: Option<bool>,
    #[serde(default)]
    pub atomic: Option<AtomicPolicyConfig>,
    /// Target-owned temporal contracts. Automatic evidence is available only
    /// when at least one explicit contract is configured.
    #[serde(default)]
    pub temporal: Vec<TemporalTargetContract>,
    /// Target-owned relational contracts and their baselines.
    #[serde(default)]
    pub relational: Vec<RelationalTargetContract>,
    #[serde(default, rename = "optimization")]
    pub optimization_override: Option<OptimizationOverride>,
}

#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct TemporalTargetContract {
    pub name: String,
    pub operator: crate::trace::TemporalOperator,
    pub event: String,
    #[serde(default)]
    pub until_event: Option<String>,
    pub bound: usize,
    #[serde(default)]
    pub fairness_assumption: Option<String>,
    #[serde(default)]
    pub trace: Option<String>,
    #[serde(default = "default_trial_timeout_ms")]
    pub timeout_ms: u64,
}

impl TemporalTargetContract {
    pub fn contract(&self) -> crate::trace::TemporalContract {
        crate::trace::TemporalContract {
            name: self.name.clone(),
            operator: self.operator,
            event: self.event.clone(),
            until_event: self.until_event.clone(),
            bound: self.bound,
            fairness_assumption: self.fairness_assumption.clone(),
        }
    }
}

#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct RelationalTargetContract {
    pub name: String,
    /// Baseline Trace IR JSON or Rust source.
    pub base: String,
    #[serde(default)]
    pub current_trace: Option<String>,
    #[serde(default)]
    pub observations: Vec<String>,
    #[serde(default)]
    pub secret_inputs: Vec<String>,
    #[serde(default)]
    pub ignored_side_effects: Vec<String>,
    pub bound: usize,
    #[serde(default = "default_trial_timeout_ms")]
    pub timeout_ms: u64,
}

impl RelationalTargetContract {
    pub fn contract(&self) -> crate::trace::RelationalContract {
        crate::trace::RelationalContract {
            name: self.name.clone(),
            observations: self.observations.clone(),
            secret_inputs: self.secret_inputs.clone(),
            ignored_side_effects: self.ignored_side_effects.clone(),
            bound: self.bound,
        }
    }
}

impl TargetConfig {
    // Deprecated methods removed.
}

#[derive(Debug, Clone)]
pub struct CovOptConfig {
    pub version: u32,
    pub target: Vec<TargetConfig>,
    pub pipeline: PipelineConfig,
    pub assurance: AssuranceConfig,
    pub atomic: AtomicPolicyConfig,
    pub trials: TrialConfig,
    pub optimization: OptimizationConfig,
    /// Optional autonomous GoalSpec. CLI `--spec` takes precedence.
    pub converge: Option<crate::converge::GoalSpec>,
    pub macro_path: Option<String>,
    pub providers: ProvidersConfig,
    pub policies: BTreeMap<String, PolicyConfig>,
    pub target_discovery: TargetDiscoveryConfig,
}

#[derive(Deserialize, Debug, Clone, Default)]
struct RawCovOptConfig {
    #[serde(default)]
    version: Option<u32>,
    #[serde(default)]
    target: Option<toml::Value>,
    #[serde(default)]
    pipeline: PipelineConfig,
    #[serde(default)]
    assurance: AssuranceConfig,
    #[serde(default)]
    atomic: AtomicPolicyConfig,
    #[serde(default)]
    trials: TrialConfig,
    #[serde(default)]
    optimization: OptimizationConfig,
    #[serde(default)]
    converge: Option<crate::converge::GoalSpec>,
    #[serde(default)]
    macro_path: Option<String>,
    #[serde(default)]
    providers: ProvidersConfig,
    #[serde(default)]
    policy: BTreeMap<String, PolicyConfig>,
    #[serde(default)]
    targets: TargetDiscoveryConfig,
}

impl<'de> Deserialize<'de> for CovOptConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawCovOptConfig::deserialize(deserializer)?;
        let mut target = Vec::new();
        if let Some(value) = raw.target {
            match value {
                toml::Value::Array(_) => {
                    target = value.try_into().map_err(|error| {
                        D::Error::custom(format!("invalid target array: {error}"))
                    })?;
                }
                toml::Value::Table(table) => {
                    for (id, value) in table {
                        let mut item: TargetConfig = value.try_into().map_err(|error| {
                            D::Error::custom(format!("invalid target.{id} configuration: {error}"))
                        })?;
                        if item.test.is_empty() {
                            item.test = id.clone();
                        }
                        if item.id.is_none() {
                            item.id = Some(id);
                        }
                        target.push(item);
                    }
                }
                other => {
                    return Err(D::Error::custom(format!(
                        "target must be an array or table, got {other}"
                    )));
                }
            }
        }
        target.sort_by(|left, right| left.test.cmp(&right.test));
        if target.is_empty() && raw.targets.discover.as_deref() == Some("annotations") {
            for metadata in crate::static_analysis::find_all_covopt_target_metadata() {
                let evidence = crate::static_analysis::find_covopt_evidence_metadata(&metadata.id);
                let first_evidence = evidence.first();
                target.push(TargetConfig {
                    id: Some(metadata.id),
                    function: Some(metadata.function),
                    complexity: metadata.complexity.clone(),
                    policy: None,
                    providers: None,
                    test: first_evidence
                        .map(|item| item.function.clone())
                        .unwrap_or_default(),
                    tests: None,
                    package: None,
                    expected: metadata.complexity,
                    n_values: first_evidence.and_then(|item| item.n_values.clone()),
                    fuzz_iterations: None,
                    mca_cpu: None,
                    ignore: None,
                    require_cache_padding: true,
                    require_branch_hints: true,
                    require_aerospace_grade: true,
                    require_watchdog_timeout: true,
                    require_stress_test: true,
                    polling_threshold: None,
                    assurance: if evidence.is_empty() {
                        Some(AssurancePolicy::Static)
                    } else {
                        None
                    },
                    evidence_threshold: None,
                    budget_seconds: None,
                    planner: None,
                    static_only: None,
                    atomic: None,
                    temporal: Vec::new(),
                    relational: Vec::new(),
                    optimization_override: None,
                });
            }
        }
        target.sort_by(|left, right| left.test.cmp(&right.test));
        Ok(Self {
            version: raw.version.unwrap_or_else(default_config_version),
            target,
            pipeline: raw.pipeline,
            assurance: raw.assurance,
            atomic: raw.atomic,
            trials: raw.trials,
            optimization: raw.optimization,
            converge: raw.converge,
            macro_path: raw.macro_path,
            providers: raw.providers,
            policies: raw.policy,
            target_discovery: raw.targets,
        })
    }
}

#[derive(Deserialize, Debug, Clone, Default, Serialize)]
pub struct TargetDiscoveryConfig {
    #[serde(default)]
    pub discover: Option<String>,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProviderMode {
    Required,
    #[default]
    Auto,
    Fallback,
    Disabled,
}

#[derive(Deserialize, Debug, Clone, Default, Serialize)]
pub struct ProvidersConfig {
    #[serde(rename = "static")]
    pub static_ast: Option<ProviderMode>,
    pub mca: Option<ProviderMode>,
    pub coverage: Option<ProviderMode>,
    pub sanitizer: Option<ProviderMode>,
    pub concurrency: Option<ProviderMode>,
    pub profile: Option<ProviderMode>,
    pub temporal: Option<ProviderMode>,
    pub relational: Option<ProviderMode>,
    pub adversarial: Option<ProviderMode>,
}

fn merge_providers(parent: ProvidersConfig, child: ProvidersConfig) -> ProvidersConfig {
    ProvidersConfig {
        static_ast: child.static_ast.or(parent.static_ast),
        mca: child.mca.or(parent.mca),
        coverage: child.coverage.or(parent.coverage),
        sanitizer: child.sanitizer.or(parent.sanitizer),
        concurrency: child.concurrency.or(parent.concurrency),
        profile: child.profile.or(parent.profile),
        temporal: child.temporal.or(parent.temporal),
        relational: child.relational.or(parent.relational),
        adversarial: child.adversarial.or(parent.adversarial),
    }
}

#[derive(Deserialize, Debug, Clone, Default, Serialize)]
pub struct PolicyConfig {
    #[serde(default)]
    pub extends: Option<String>,
    pub overall_coverage: Option<f64>,
    pub critical_coverage: Option<f64>,
    pub performance_coverage: Option<f64>,
    pub budget_seconds: Option<u64>,
    #[serde(default)]
    pub required_obligations: Vec<String>,
    #[serde(default)]
    pub providers: ProvidersConfig,
}

#[derive(Deserialize, Debug, Clone, Default, Serialize)]
pub struct OptimizationOverride {
    #[serde(default)]
    pub enabled: Vec<String>,
    pub budget_seconds: Option<u64>,
    pub apply: Option<String>,
}

#[derive(Deserialize, Debug, Clone, Serialize, Default)]
pub struct OptimizationConfig {
    #[serde(default)]
    pub enabled: Vec<String>,
    #[serde(default = "default_budget_seconds")]
    pub default_budget_seconds: u64,
    #[serde(default = "default_apply_mode")]
    pub apply: String,
    #[serde(default)]
    pub codegen: CodegenOptimizerConfig,
    #[serde(default)]
    pub layout: LayoutOptimizerConfig,
}

#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct CodegenOptimizerConfig {
    #[serde(default = "default_optimizer_max_candidates")]
    pub max_candidates: usize,
    pub lto: Option<String>,
    pub codegen_units: Option<u32>,
    pub opt_level: Option<String>,
    pub target_cpu: Option<String>,
}

impl Default for CodegenOptimizerConfig {
    fn default() -> Self {
        Self {
            max_candidates: default_optimizer_max_candidates(),
            lto: None,
            codegen_units: None,
            opt_level: None,
            target_cpu: None,
        }
    }
}

#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct LayoutOptimizerConfig {
    #[serde(default = "default_optimizer_max_candidates")]
    pub max_candidates: usize,
    #[serde(default = "default_cache_line_bytes")]
    pub cache_line_bytes: usize,
    #[serde(default)]
    pub allow_public_abi_suggestions: bool,
}

impl Default for LayoutOptimizerConfig {
    fn default() -> Self {
        Self {
            max_candidates: default_optimizer_max_candidates(),
            cache_line_bytes: default_cache_line_bytes(),
            allow_public_abi_suggestions: false,
        }
    }
}

fn default_optimizer_max_candidates() -> usize {
    32
}

fn default_apply_mode() -> String {
    "never".to_string()
}

fn default_cache_line_bytes() -> usize {
    64
}

#[derive(Deserialize, Debug, Clone, Serialize, Default)]
pub struct AtomicPolicyConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub synthesize: bool,
    pub contract: Option<String>,
    #[serde(default)]
    pub forbidden_outcomes: Vec<String>,
    #[serde(default)]
    pub max_threads: Option<usize>,
    #[serde(default)]
    pub max_events: Option<usize>,
    #[serde(default)]
    pub max_unroll: Option<usize>,
    #[serde(default)]
    pub max_values: Option<usize>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub mca_cpu: Option<String>,
}

impl AtomicPolicyConfig {
    pub fn bounds(&self) -> ModelBounds {
        let defaults = ModelBounds::default();
        ModelBounds {
            max_threads: self.max_threads.unwrap_or(defaults.max_threads),
            max_events: self.max_events.unwrap_or(defaults.max_events),
            max_unroll: self.max_unroll.unwrap_or(defaults.max_unroll),
            max_values: self.max_values.unwrap_or(defaults.max_values),
            timeout_ms: self.timeout_ms.unwrap_or(defaults.timeout_ms),
        }
    }

    pub fn correctness_contract(&self) -> Option<AtomicContract> {
        let name = self.contract.clone()?;
        let normalized = name.to_ascii_lowercase().replace(['_', ' '], "-");
        let kind = match normalized.as_str() {
            "message-passing" | "messagepassing" => ContractKind::MessagePassing,
            "publication" => ContractKind::Publication,
            "monotonic-counter" | "counter" => ContractKind::MonotonicCounter,
            "mutex-exclusion" | "spinlock" | "mutex" => ContractKind::MutexExclusion,
            "linearizable-queue" | "queue" => ContractKind::LinearizableQueue,
            _ => ContractKind::Custom,
        };
        Some(AtomicContract {
            name,
            kind,
            forbidden_outcomes: self
                .forbidden_outcomes
                .iter()
                .map(|outcome| ForbiddenOutcome {
                    name: outcome.clone(),
                    assignments: Default::default(),
                    description: outcome.clone(),
                })
                .collect(),
            visibility: Vec::new(),
            single_writer: false,
            readers: Vec::new(),
            init_publication: matches!(
                kind,
                ContractKind::MessagePassing | ContractKind::Publication
            ),
            mutex_exclusion: matches!(kind, ContractKind::MutexExclusion),
        })
    }
}

#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct TrialConfig {
    #[serde(default = "default_trial_budget_seconds")]
    pub budget_seconds: u64,
    #[serde(default = "default_seed_count")]
    pub seed_count: usize,
    pub seed: Option<u64>,
    pub seed_start: Option<u64>,
    pub seed_end: Option<u64>,
    #[serde(default = "default_trial_max_candidates")]
    pub max_candidates: usize,
    #[serde(default = "default_trial_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_trial_budget_seconds() -> u64 {
    30
}
fn default_seed_count() -> usize {
    3
}
fn default_trial_max_candidates() -> usize {
    256
}
fn default_trial_timeout_ms() -> u64 {
    5_000
}

impl Default for TrialConfig {
    fn default() -> Self {
        Self {
            budget_seconds: default_trial_budget_seconds(),
            seed_count: default_seed_count(),
            seed: None,
            seed_start: None,
            seed_end: None,
            max_candidates: default_trial_max_candidates(),
            timeout_ms: default_trial_timeout_ms(),
        }
    }
}

impl TrialConfig {
    pub fn selection_config(&self) -> TrialSelectionConfig {
        TrialSelectionConfig {
            budget_ms: self.budget_seconds.saturating_mul(1_000),
            seed_count: self.seed_count,
            seed: self.seed,
            seed_start: self.seed_start,
            seed_end: self.seed_end,
            max_candidates: self.max_candidates,
            timeout_ms: self.timeout_ms,
            ..TrialSelectionConfig::default()
        }
    }
}

impl CovOptConfig {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let content = fs::read_to_string(&path).map_err(|e| {
            format!(
                "Failed to read config file {}: {}",
                path.as_ref().display(),
                e
            )
        })?;
        toml::from_str(&content)
            .map_err(|e| format!("Failed to parse {}: {}", path.as_ref().display(), e))
    }

    /// Load a persisted policy when present, otherwise synthesize the same V3
    /// defaults used by `covopt init` and discover annotated tests in memory.
    /// Malformed or unreadable existing files remain hard errors.
    pub fn load_or_embedded<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        match fs::read_to_string(&path) {
            Ok(content) => toml::from_str(&content)
                .map_err(|e| format!("Failed to parse {}: {}", path.as_ref().display(), e)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                toml::from_str(&default_config_source(false))
                    .map_err(|error| format!("Failed to construct embedded config: {error}"))
            }
            Err(error) => Err(format!(
                "Failed to read config file {}: {}",
                path.as_ref().display(),
                error
            )),
        }
    }

    pub fn target_id(target: &TargetConfig) -> String {
        target
            .id
            .clone()
            .or_else(|| (!target.test.is_empty()).then(|| target.test.clone()))
            .unwrap_or_else(|| "unknown".to_string())
    }

    pub fn policy_for_target(&self, target: &TargetConfig) -> Option<PolicyConfig> {
        let name = target
            .policy
            .as_deref()
            .or(Some("default"))
            .unwrap_or("default");
        self.named_policy(name, &mut Vec::new())
    }

    fn named_policy(&self, name: &str, stack: &mut Vec<String>) -> Option<PolicyConfig> {
        if stack.iter().any(|item| item == name) {
            return None;
        }
        let policy = self.policies.get(name)?.clone();
        let Some(parent) = policy.extends.as_deref() else {
            return Some(policy);
        };
        stack.push(name.to_string());
        let parent = self.named_policy(parent, stack).unwrap_or_default();
        stack.pop();
        Some(PolicyConfig {
            extends: None,
            overall_coverage: policy.overall_coverage.or(parent.overall_coverage),
            critical_coverage: policy.critical_coverage.or(parent.critical_coverage),
            performance_coverage: policy.performance_coverage.or(parent.performance_coverage),
            budget_seconds: policy.budget_seconds.or(parent.budget_seconds),
            required_obligations: if policy.required_obligations.is_empty() {
                parent.required_obligations
            } else {
                policy.required_obligations
            },
            providers: merge_providers(parent.providers, policy.providers),
        })
    }

    pub fn provider_mode(providers: &ProvidersConfig, provider: &str) -> ProviderMode {
        match provider {
            "StaticAst" => providers.static_ast.unwrap_or(ProviderMode::Auto),
            "Mca" => providers.mca.unwrap_or(ProviderMode::Auto),
            "Coverage" => providers.coverage.unwrap_or(ProviderMode::Auto),
            "Sanitizer" => providers.sanitizer.unwrap_or(ProviderMode::Auto),
            "AtomicModel" | "Test" => providers.concurrency.unwrap_or(ProviderMode::Auto),
            "Profiler" => providers.profile.unwrap_or(ProviderMode::Auto),
            "Temporal" => providers.temporal.unwrap_or(ProviderMode::Auto),
            "Relational" => providers.relational.unwrap_or(ProviderMode::Auto),
            "Adversarial" => providers.adversarial.unwrap_or(ProviderMode::Auto),
            _ => ProviderMode::Auto,
        }
    }

    pub fn resolved_target(&self, target: &TargetConfig) -> ResolvedTargetConfig {
        let named_policy = self.policy_for_target(target);
        let mut sources = BTreeMap::new();
        let id = Self::target_id(target);
        sources.insert(
            "id".to_string(),
            if target.id.is_some() {
                "target"
            } else {
                "legacy-test"
            }
            .to_string(),
        );
        let function = target.function.clone().or_else(|| {
            crate::static_analysis::find_covopt_target_metadata(&id)
                .map(|metadata| metadata.function)
        });
        let complexity = target
            .complexity
            .clone()
            .or_else(|| target.expected.clone())
            .or_else(|| {
                crate::static_analysis::find_covopt_target_metadata(&id)
                    .and_then(|metadata| metadata.complexity)
            });
        if target.function.is_some() {
            sources.insert("function".to_string(), "target".to_string());
        } else if function.is_some() {
            sources.insert("function".to_string(), "annotation".to_string());
        }
        if target.complexity.is_some() || target.expected.is_some() {
            sources.insert("complexity".to_string(), "target".to_string());
        } else if complexity.is_some() {
            sources.insert("complexity".to_string(), "annotation".to_string());
        }
        let policy_name = target.policy.clone().or_else(|| {
            self.policies
                .contains_key("default")
                .then(|| "default".to_string())
        });
        if policy_name.is_some() {
            sources.insert(
                "policy".to_string(),
                if target.policy.is_some() {
                    "target"
                } else {
                    "policy"
                }
                .to_string(),
            );
        }
        sources.insert(
            "providers".to_string(),
            if target.providers.is_some() {
                "target"
            } else if named_policy.as_ref().is_some_and(|policy| {
                policy.providers.static_ast.is_some()
                    || policy.providers.mca.is_some()
                    || policy.providers.coverage.is_some()
                    || policy.providers.sanitizer.is_some()
                    || policy.providers.concurrency.is_some()
                    || policy.providers.profile.is_some()
                    || policy.providers.temporal.is_some()
                    || policy.providers.relational.is_some()
                    || policy.providers.adversarial.is_some()
            }) {
                "policy"
            } else {
                "global"
            }
            .to_string(),
        );
        sources.insert(
            "optimization".to_string(),
            if target.optimization_override.is_some() {
                "target"
            } else {
                "global"
            }
            .to_string(),
        );
        ResolvedTargetConfig {
            id,
            function,
            complexity,
            policy: policy_name,
            providers: target
                .providers
                .clone()
                .or_else(|| {
                    named_policy
                        .as_ref()
                        .map(|policy| policy.providers.clone())
                        .filter(|providers| {
                            providers.static_ast.is_some()
                                || providers.mca.is_some()
                                || providers.coverage.is_some()
                                || providers.sanitizer.is_some()
                                || providers.concurrency.is_some()
                                || providers.profile.is_some()
                                || providers.temporal.is_some()
                                || providers.relational.is_some()
                                || providers.adversarial.is_some()
                        })
                })
                .unwrap_or_else(|| self.providers.clone()),
            optimization: target.optimization_override.clone(),
            sources,
        }
    }
}

const DEFAULT_CONFIG_HEADER: &str = r#"version = 3

[assurance]
mode = "adaptive"
overall_coverage = 0.90
critical_coverage = 1.0
performance_coverage = 0.90
budget_seconds = 30
planner = "hybrid"
fail_on_critical_unknown = true

[providers]
static = "required"
mca = "auto"
coverage = "fallback"
sanitizer = "fallback"
concurrency = "fallback"
profile = "fallback"
temporal = "auto"
relational = "auto"
adversarial = "auto"

[optimization]
enabled = ["inputs", "atomic", "codegen", "layout"]
default_budget_seconds = 30
apply = "never"

[targets]
discover = "annotations"

[policy.default]
overall_coverage = 0.90
critical_coverage = 1.0

[optimization.codegen]
max_candidates = 32

[optimization.layout]
max_candidates = 32
cache_line_bytes = 64
allow_public_abi_suggestions = false

# Autonomous convergence defaults. This grants workspace apply authority, not
# git commit/push or external side effects.
[converge]
authority = "apply"

[converge.budget]
wall_time_ms = 30000
max_iterations = 8

"#;

/// Render the optional persisted configuration. Discovery is authoritative;
/// explicit target blocks are emitted only for legacy `covopt_test` metadata
/// that is not represented by the V3 target/evidence annotations.
pub fn default_config_source(include_placeholder: bool) -> String {
    let mut source = String::from(DEFAULT_CONFIG_HEADER);
    let discovered = crate::static_analysis::find_all_covopt_tests();
    if discovered.is_empty() && include_placeholder {
        source.push_str(
            r#"[[target]]
test = "my_benchmark_test"
expected = "O(1)"
n_values = "1,500,10000"
"#,
        );
    } else {
        for (test, expected, n_values) in discovered {
            source.push_str(&format!(
                "[[target]]\ntest = {}\nexpected = {}\nn_values = {}\n\n",
                serde_json::to_string(&test).unwrap_or_default(),
                serde_json::to_string(&expected).unwrap_or_default(),
                serde_json::to_string(&n_values).unwrap_or_default(),
            ));
        }
    }
    source
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedTargetConfig {
    pub id: String,
    pub function: Option<String>,
    pub complexity: Option<String>,
    pub policy: Option<String>,
    pub providers: ProvidersConfig,
    pub optimization: Option<OptimizationOverride>,
    pub sources: BTreeMap<String, String>,
}

pub fn should_color() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal() && std::env::var("NO_COLOR").is_err()
}

#[derive(clap::Args, Debug, Clone)]
pub struct ReportArgs {
    #[arg(long, default_value = "target/covopt")]
    pub output_dir: String,

    /// Output format (html or sarif)
    #[arg(long, default_value = "html")]
    pub format: String,
}

#[derive(clap::Args, Debug, Clone)]
pub struct AuditArgs {
    /// Run audit only on explicitly git staged files
    #[arg(long)]
    pub staged: bool,
    /// Only audit targets affected by the diff from this base branch
    #[arg(long)]
    pub base: Option<String>,
    /// The name of the test target to audit
    #[arg(long)]
    pub test: Option<String>,

    /// Run in fast mode (only use min and max N values)
    #[arg(long)]
    pub fast: bool,

    /// Output report as structured JSON for AI Agents
    #[arg(long)]
    pub json: bool,
    /// Keep LCOV/profile artifacts and emit runner timings
    #[arg(long)]
    pub debug_artifacts: bool,
    /// Assurance policy: static, adaptive, or strict
    #[arg(long, value_enum, default_value_t = AssurancePolicy::Adaptive)]
    pub assurance: AssurancePolicy,
    /// Minimum overall evidence coverage for adaptive/strict assurance
    #[arg(long)]
    pub evidence_threshold: Option<f64>,
    /// Enable adaptive trial selection and replanning metadata
    #[arg(long)]
    pub adaptive_inputs: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct CheckArgs {
    /// Only check targets affected by this git base branch
    #[arg(long)]
    pub base: Option<String>,
    /// Check one target identity
    #[arg(long)]
    pub target: Option<String>,
    /// Assurance mode selected by the CLI
    #[arg(long, value_enum)]
    pub mode: Option<AssurancePolicy>,
    /// Show the evidence plan without executing providers
    #[arg(long)]
    pub plan: bool,
    /// Output format: text, json, sarif, or html
    #[arg(long, default_value = "text")]
    pub format: String,
    /// Use the fast audit path
    #[arg(long)]
    pub fast: bool,
    /// Restrict checking to staged files
    #[arg(long)]
    pub staged: bool,
    /// Keep diagnostic/profile artifacts
    #[arg(long)]
    pub debug_artifacts: bool,
    /// Hard wall-clock budget for the complete check, for example 300s or 5m
    #[arg(long, default_value = "5m")]
    pub budget: String,
}

#[derive(clap::Args, Debug, Clone)]
pub struct InspectCommandArgs {
    /// Target function or target identity
    #[arg(long)]
    pub target: Option<String>,
    /// Source file or directory to inspect
    #[arg(long, default_value = "src/")]
    pub path: String,
    /// Restrict output to a finding
    #[arg(long)]
    pub finding: Option<String>,
    /// Include explanation details
    #[arg(long)]
    pub explain: bool,
    /// Output format: text or json
    #[arg(long, default_value = "text")]
    pub format: String,
    /// Show the resolved configuration and value sources
    #[arg(long)]
    pub config: bool,
    /// Show the hierarchical scope envelope from the latest check
    #[arg(long)]
    pub envelope: bool,
    /// Show the proof frontier from the latest check
    #[arg(long)]
    pub frontier: bool,
    /// Show one scope from the latest envelope
    #[arg(long)]
    pub scope: Option<String>,
    /// Show the assumption ledger from the latest snapshot
    #[arg(long)]
    pub assumptions: bool,
    /// Compare the latest snapshot with a previous snapshot
    #[arg(long)]
    pub drift: bool,
    /// Baseline snapshot path used by --drift
    #[arg(long)]
    pub base: Option<String>,
}

#[derive(clap::Args, Debug, Clone)]
pub struct PlanArgs {
    /// Restrict the plan to one configured test target
    #[arg(long)]
    pub test: Option<String>,
    /// Emit the complete plan as JSON
    #[arg(long)]
    pub json: bool,
    /// Planning budget, for example 30s, 5000ms, or 2m
    #[arg(long, default_value = "30s")]
    pub budget: String,
    /// Permit only Static AST actions
    #[arg(long)]
    pub static_only: bool,
    /// Planner strategy
    #[arg(long, value_enum)]
    pub planner: Option<PlannerKind>,
    /// Assurance mode override used by the unified check command
    #[arg(long, value_enum, hide = true)]
    pub mode: Option<AssurancePolicy>,
}

#[derive(clap::Args, Debug, Clone)]
pub struct SelectTrialsArgs {
    /// Restrict selection to one configured test target
    #[arg(long)]
    pub target: Option<String>,
    /// Planning budget, for example 30s, 5000ms, or 2m
    #[arg(long, default_value = "30s")]
    pub budget: String,
    /// Emit the complete selection as JSON
    #[arg(long)]
    pub json: bool,
    /// Number of deterministic seeds per N
    #[arg(long)]
    pub seed_count: Option<usize>,
    /// Run only candidate generation and selection; never execute tests
    #[arg(long, default_value_t = true)]
    pub dry_run: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct AtomicTargetArgs {
    /// Configured target name, or a source path when --source is supplied
    #[arg(long)]
    pub target: Option<String>,
    /// Rust source file to inspect
    #[arg(long)]
    pub source: Option<String>,
    /// Emit structured JSON
    #[arg(long)]
    pub json: bool,
    /// Bounded model-checking budget
    #[arg(long, default_value = "5s")]
    pub budget: String,
}

#[derive(clap::Args, Debug, Clone)]
pub struct AtomicVerifyArgs {
    /// JSON file containing an AtomicCandidate or AtomicSynthesisResult
    #[arg(long)]
    pub candidate: String,
    #[arg(long)]
    pub target: Option<String>,
    #[arg(long)]
    pub source: Option<String>,
    #[arg(long)]
    pub json: bool,
}

#[derive(clap::Subcommand, Debug, Clone)]
pub enum AtomicSubcommand {
    /// Extract atomic events and run the bounded baseline model
    Analyze(AtomicTargetArgs),
    /// Search legal weaker orderings and emit a diff without modifying source
    Synth(AtomicTargetArgs),
    /// Re-run bounded verification for a saved candidate
    Verify(AtomicVerifyArgs),
}

#[derive(clap::Args, Debug, Clone)]
pub struct AtomicArgs {
    #[command(subcommand)]
    pub command: AtomicSubcommand,
}

#[derive(clap::Args, Debug, Clone)]
pub struct AdviseArgs {
    /// Only analyze files modified compared to the specified git branch
    #[arg(long)]
    pub diff: Option<String>,
    /// Target file or directory to analyze (defaults to "src/")
    #[arg(default_value = "src/")]
    pub path: String,

    /// Optional function name to analyze
    #[arg(short = 'f', long = "function", alias = "func")]
    pub func: Option<String>,

    /// Emit structured findings as JSON
    #[arg(long)]
    pub json: bool,

    /// Explain one stable finding ID
    #[arg(long)]
    pub explain: Option<String>,
}

#[derive(clap::Args, Debug, Clone)]
pub struct InitArgs {
    /// Optional project path (defaults to current directory)
    pub path: Option<String>,

    /// Skip interactive prompts and accept default values
    #[arg(short, long)]
    pub yes: bool,

    /// Install a pre-commit hook in the target git repository
    #[arg(long, default_value_t = false)]
    pub hook: bool,

    /// Upgrade a legacy .covopt.toml to the V3 policy/provider schema
    #[arg(long, default_value_t = false)]
    pub migrate: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct HardenArgs {
    /// Target directory for harness generation
    #[arg(default_value = "src/")]
    pub path: String,

    /// The name of the test target
    #[arg(short, long)]
    pub test: Option<String>,

    /// Ignore uninstalled tools instead of failing
    #[arg(long, default_value_t = false)]
    pub fast: bool,

    /// Generate fuzzing harnesses for public functions instead of running hardening
    #[arg(long, default_value_t = false)]
    pub generate_harness: bool,

    /// Run mutation testing using cargo-mutants
    #[arg(long, default_value_t = false)]
    pub mutate: bool,

    /// Run fuzzing using cargo-fuzz
    #[arg(long, default_value_t = false)]
    pub fuzz: bool,

    /// Run tests with LLVM sanitizers
    #[arg(long, default_value_t = false)]
    pub sanitize: bool,

    /// Sanitizer type (address or thread)
    #[arg(long, default_value = "address")]
    pub san_type: String,

    /// Automatically repair memory safety crashes using LLM
    #[arg(long, default_value_t = false)]
    pub auto_fix: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct CiArgs {
    /// Only run CI on files modified compared to the specified git branch
    #[arg(long)]
    pub base: Option<String>,
    /// Skip the hardening (fuzz/mutate) step
    #[arg(long, default_value_t = false)]
    pub skip_harden: bool,

    /// Fail the CI if any step produces a non-perfect result
    #[arg(long, default_value_t = false)]
    pub strict: bool,

    /// Run in fast mode (skips heavy tuning/fuzzing and uses fast audit)
    #[arg(long, default_value_t = false)]
    pub fast: bool,

    /// Generate an HTML dashboard report after CI completes
    #[arg(long, default_value_t = false)]
    pub report: bool,

    /// Generate a SARIF report after CI completes
    #[arg(long, default_value_t = false)]
    pub sarif: bool,
    /// Assurance policy: static, adaptive, or strict
    #[arg(long, value_enum)]
    pub assurance: Option<AssurancePolicy>,
    /// Minimum overall evidence coverage for adaptive/strict assurance
    #[arg(long)]
    pub evidence_threshold: Option<f64>,
    /// Hard wall-clock budget for the complete CI run
    #[arg(long, default_value = "5m")]
    pub budget: String,
}

#[derive(clap::Args, Debug, Clone)]
pub struct FixArgs {
    /// Optional path to scan and fix (defaults to current directory)
    pub path: Option<String>,

    /// Only run cargo clippy --fix
    #[arg(long, default_value_t = false)]
    pub only_clippy: bool,

    /// Only run magic number to covopt_param! substitution
    #[arg(long, default_value_t = false)]
    pub only_magic: bool,

    /// Generate a structured minimal repair plan
    #[arg(long)]
    pub plan: bool,
    /// Apply only after repair verification and source-hash checks
    #[arg(long)]
    pub apply: bool,
    /// Roll back a committed repair transaction manifest
    #[arg(long, value_name = "MANIFEST")]
    pub rollback: Option<String>,
    /// Permit unsafe/atomic repairs only when specialized evidence is present
    #[arg(long)]
    pub unsafe_evidence: bool,
    /// Restrict repair planning to one finding ID
    #[arg(long)]
    pub finding: Option<String>,
    /// Repair planning budget
    #[arg(long, default_value = "30s")]
    pub budget: String,
    /// Emit repair plan JSON
    #[arg(long)]
    pub json: bool,
    /// Compatibility spelling for the legacy clippy fixer
    #[arg(long)]
    pub legacy_clippy: bool,
    /// Compatibility spelling for the legacy magic-number fixer
    #[arg(long)]
    pub legacy_magic: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct ConvergeArgs {
    /// GoalSpec JSON/TOML. Omit to infer objectives and use safe defaults.
    #[arg(long)]
    pub spec: Option<String>,
    /// Rust source path, configured target ID, or annotated test target.
    #[arg(long)]
    pub target: Option<String>,
    /// Replace inferred objectives; repeat for multiple open metric IDs.
    #[arg(long)]
    pub objective: Vec<String>,
    /// Add a required constraint; repeat for multiple open constraint IDs.
    #[arg(long)]
    pub constraint: Vec<String>,
    /// Complete convergence wall-clock budget.
    #[arg(long)]
    pub budget: Option<String>,
    /// Authority boundary: read-only, suggest, or apply (default/turbo).
    #[arg(long, value_parser = ["read-only", "suggest", "apply"])]
    pub authority: Option<String>,
    /// Output format: text or json. The complete bundle is always persisted.
    #[arg(long, default_value = "text", value_parser = ["text", "json"])]
    pub format: String,
}

#[derive(clap::Args, Debug, Clone)]
pub struct CodegenOptimizeArgs {
    #[arg(long)]
    pub target: Option<String>,
    #[arg(long, default_value = "30s")]
    pub budget: String,
    #[arg(long)]
    pub json: bool,
    #[arg(long)]
    pub apply: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct LayoutOptimizeArgs {
    #[arg(long)]
    pub struct_name: Option<String>,
    #[arg(long)]
    pub target: Option<String>,
    #[arg(long)]
    pub profile: bool,
    #[arg(long)]
    pub json: bool,
    #[arg(long)]
    pub apply: bool,
}

#[derive(clap::Subcommand, Debug, Clone)]
pub enum OptimizeSubcommand {
    /// Search covopt_param candidates through Search -> Confirm -> Robustness
    Parameters(ParameterOptimizeArgs),
    /// Generate code-generation candidates and patches
    Codegen(CodegenOptimizeArgs),
    /// Generate memory-layout candidates and patches
    Layout(LayoutOptimizeArgs),
}

#[derive(clap::Args, Debug, Clone)]
pub struct OptimizeArgs {
    #[command(subcommand)]
    pub command: OptimizeSubcommand,
}

#[derive(clap::Args, Debug, Clone)]
pub struct ParameterOptimizeArgs {
    /// Cargo bench target used to score candidates.
    #[arg(long)]
    pub target: String,
    /// Rust source containing the parameter metadata; defaults to the target source.
    #[arg(long)]
    pub source: Option<String>,
    #[arg(long, default_value_t = 8)]
    pub iterations: usize,
    #[arg(long, default_value_t = 3)]
    pub top_k: usize,
    #[arg(long, default_value_t = 0)]
    pub seed: u64,
    #[arg(long)]
    pub json: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct InputsOptimizeArgs {
    #[arg(long)]
    pub target: Option<String>,
    #[arg(long, default_value = "30s")]
    pub budget: String,
    #[arg(long)]
    pub json: bool,
    #[arg(long)]
    pub apply: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct AtomicOptimizeArgs {
    #[arg(long)]
    pub target: Option<String>,
    #[arg(long)]
    pub source: Option<String>,
    #[arg(long, default_value = "5s")]
    pub budget: String,
    #[arg(long)]
    pub json: bool,
    #[arg(long)]
    pub apply: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct AdversarialOptimizeArgs {
    #[arg(long)]
    pub target: Option<String>,
    #[arg(long, default_value = "30s")]
    pub budget: String,
    #[arg(long, default_value_t = 0)]
    pub seed: u64,
    /// Per-environment target execution timeout
    #[arg(long, default_value_t = 5_000)]
    pub timeout_ms: u64,
    #[arg(long)]
    pub json: bool,
}

#[derive(clap::Subcommand, Debug, Clone)]
pub enum UnifiedOptimizeSubcommand {
    Inputs(InputsOptimizeArgs),
    Parameters(ParameterOptimizeArgs),
    Atomic(AtomicOptimizeArgs),
    Adversarial(AdversarialOptimizeArgs),
    Codegen(CodegenOptimizeArgs),
    Layout(LayoutOptimizeArgs),
}

#[derive(clap::Args, Debug, Clone)]
pub struct UnifiedOptimizeArgs {
    #[command(subcommand)]
    pub command: UnifiedOptimizeSubcommand,
}

#[derive(clap::Args, Debug, Clone)]
pub struct VerifyCoverageArgs {
    #[arg(long)]
    pub target: Option<String>,
    #[arg(long)]
    pub json: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct VerifySafetyArgs {
    #[arg(long)]
    pub target: Option<String>,
    #[arg(long, default_value = "address")]
    pub sanitizer: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct VerifyConcurrencyArgs {
    #[arg(long)]
    pub target: Option<String>,
    #[arg(long, default_value_t = 50)]
    pub timeout_ms: u64,
    #[arg(long, default_value_t = 1_000_000)]
    pub max_iters: usize,
    #[arg(long, default_value_t = 0)]
    pub seed: u64,
    #[arg(long)]
    pub json: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct VerifyRuntimeArgs {
    #[arg(long)]
    pub target: Option<String>,
    #[arg(long, default_value = "flamegraph")]
    pub tool: String,
    #[arg(long)]
    pub bin: Option<String>,
    #[arg(long)]
    pub json: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct VerifyTemporalArgs {
    #[arg(long)]
    pub target: Option<String>,
    #[arg(long, default_value = "eventually")]
    pub operator: String,
    #[arg(long)]
    pub event: String,
    #[arg(long)]
    pub until_event: Option<String>,
    #[arg(long, default_value_t = 32)]
    pub bound: usize,
    /// Required for eventually/bounded liveness claims.
    #[arg(long)]
    pub fairness: Option<String>,
    /// Existing runtime Trace IR JSON; otherwise CovOpt requests one from the target test
    #[arg(long)]
    pub trace: Option<String>,
    #[arg(long, default_value_t = 5_000)]
    pub timeout_ms: u64,
    #[arg(long)]
    pub json: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct VerifyRelationalArgs {
    #[arg(long)]
    pub target: Option<String>,
    /// Baseline Trace IR JSON or Rust source fallback
    #[arg(long)]
    pub base: Option<String>,
    /// Existing current runtime Trace IR JSON; otherwise request one from the target test
    #[arg(long)]
    pub current_trace: Option<String>,
    #[arg(long, default_value = "operation")]
    pub observations: String,
    #[arg(long, default_value_t = 32)]
    pub bound: usize,
    #[arg(long, default_value_t = 5_000)]
    pub timeout_ms: u64,
    #[arg(long)]
    pub json: bool,
}

#[derive(clap::Subcommand, Debug, Clone)]
pub enum VerifySubcommand {
    Coverage(VerifyCoverageArgs),
    Safety(VerifySafetyArgs),
    Concurrency(VerifyConcurrencyArgs),
    Runtime(VerifyRuntimeArgs),
    Temporal(VerifyTemporalArgs),
    Relational(VerifyRelationalArgs),
}

#[derive(clap::Args, Debug, Clone)]
pub struct VerifyArgs {
    #[command(subcommand)]
    pub command: VerifySubcommand,
}

#[derive(clap::Args, Debug, Clone)]
pub struct ProfileArgs {
    /// The name of the test to profile
    #[arg(long)]
    pub test: Option<String>,

    /// The name of the binary to profile
    #[arg(long)]
    pub bin: Option<String>,

    /// Profiling tool to use
    #[arg(long, default_value = "flamegraph", value_name = "flamegraph|samply")]
    pub tool: String,
}

#[derive(clap::Args, Debug, Clone)]
pub struct FuzzArgs {
    /// The target test file to perform concurrency fuzzing on
    #[arg(short, long)]
    pub target: String,

    /// Timeout in milliseconds to detect deadlocks
    #[arg(long, default_value_t = 50)]
    pub timeout_ms: u64,

    /// Max number of iterations for the in-process fuzzer
    #[arg(long, default_value_t = 1000000)]
    pub max_iters: usize,

    /// Explicit deterministic seed for delay generation
    #[arg(long, default_value_t = 0)]
    pub seed: u64,
}

#[derive(clap::Args, Debug, Clone)]
#[command(next_help_heading = "Default Run Mode Options")]
pub struct RunArgs {
    /// The name of the test to run
    #[arg(short, long)]
    pub test: Option<String>,

    /// Expected complexity (e.g. O1, OLogN, ON, ONLogN, ON2)
    #[arg(short, long)]
    pub expected: Option<String>,

    /// Comma-separated list of N values (e.g. 100,1000,10000)
    #[arg(short, long)]
    pub n_values: Option<String>,

    /// Optional LLVM-MCA CPU target (e.g. apple-m1, skylake)
    #[arg(long)]
    pub mca_cpu: Option<String>,

    /// Comma-separated list of symbols to ignore in coverage peak search
    #[arg(long)]
    pub ignore: Option<String>,

    /// Require static cache padding detection
    #[arg(long, hide = true)]
    pub require_cache_padding: bool,

    /// Enable symbolic regression to reinvent Lean 4 style formal mathematical proofs
    #[arg(long, hide = true)]
    pub formalize: bool,

    /// Require static branch prediction hint detection
    #[arg(long, hide = true)]
    pub require_branch_hints: bool,

    /// Require strict aerospace grade static analysis (#![no_std], zero-alloc, TTAS locks, RAII)
    #[arg(long)]
    pub require_aerospace_grade: bool,

    /// Require watchdog timeout detection in the target file
    #[arg(long, hide = true)]
    pub require_watchdog_timeout: bool,

    /// Require high-pressure stress test detection in the target file
    #[arg(long, hide = true)]
    pub require_stress_test: bool,

    /// Optional polling threshold for high-frequency polling detection
    #[arg(long, hide = true)]
    pub polling_threshold: Option<u64>,

    /// Run the discrete diffusion NP-hard solver to superoptimize ASM
    #[arg(long, hide = true)]
    pub optimize: bool,
    /// Output report as structured JSON for AI Agents
    #[arg(long)]
    pub json: bool,
}

#[cfg(test)]
mod tests {
    use super::{CovOptConfig, ProviderMode};

    #[test]
    fn parses_v3_target_table_and_provider_modes() {
        let config: CovOptConfig = toml::from_str(
            r#"
version = 3
[assurance]
mode = "adaptive"
[providers]
static = "required"
mca = "disabled"
[optimization]
enabled = ["inputs", "codegen"]
default_budget_seconds = 45
apply = "never"
[targets]
discover = "annotations"
[policy.default]
overall_coverage = 0.9
[target.matrix_mult]
function = "compute_matrix_mult"
complexity = "O(N^2)"
policy = "default"
"#,
        )
        .expect("V3 config should parse");
        assert_eq!(config.version, 3);
        assert_eq!(config.target.len(), 1);
        assert_eq!(config.target[0].test, "matrix_mult");
        assert_eq!(
            config.target[0].function.as_deref(),
            Some("compute_matrix_mult")
        );
        assert_eq!(config.providers.static_ast, Some(ProviderMode::Required));
        assert_eq!(config.providers.mca, Some(ProviderMode::Disabled));
        assert_eq!(config.optimization.default_budget_seconds, 45);
        assert_eq!(
            config.target_discovery.discover.as_deref(),
            Some("annotations")
        );
    }

    #[test]
    fn discovers_targets_from_source_annotations_when_target_table_is_empty() {
        let config: CovOptConfig = toml::from_str(
            r#"
version = 3
[targets]
discover = "annotations"
"#,
        )
        .expect("annotation-only V3 config should parse");
        assert!(
            config
                .target
                .iter()
                .any(|target| target.id.as_deref() == Some("binary_search"))
        );
    }

    #[test]
    fn resolves_named_policy_inheritance() {
        let config: CovOptConfig = toml::from_str(
            r#"
version = 3
[policy.default]
overall_coverage = 0.9
critical_coverage = 1.0
[policy.performance]
extends = "default"
performance_coverage = 0.95
[target.foo]
policy = "performance"
"#,
        )
        .expect("policy inheritance config should parse");
        let resolved = config.policy_for_target(&config.target[0]).unwrap();
        assert_eq!(resolved.overall_coverage, Some(0.9));
        assert_eq!(resolved.performance_coverage, Some(0.95));
    }

    #[test]
    fn parses_target_owned_temporal_and_relational_contracts() {
        let config: CovOptConfig = toml::from_str(
            r#"
version = 3
[target.worker]
test = "worker"

[[target.worker.temporal]]
name = "completes"
operator = "eventually"
event = "completed"
bound = 32
fairness_assumption = "bounded scheduler"

[[target.worker.relational]]
name = "preserves-operations"
base = "baseline.json"
observations = ["operation"]
bound = 32
"#,
        )
        .expect("target contracts should parse");
        let target = &config.target[0];
        assert_eq!(target.temporal.len(), 1);
        assert_eq!(target.relational.len(), 1);
        assert!(target.temporal[0].contract().validate().is_ok());
        assert_eq!(target.relational[0].contract().bound, 32);
    }

    #[test]
    fn missing_config_uses_embedded_policy_but_malformed_config_fails() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(".covopt.toml");
        let embedded = CovOptConfig::load_or_embedded(&path).unwrap();
        assert_eq!(embedded.version, 3);
        assert_eq!(embedded.providers.static_ast, Some(ProviderMode::Required));
        let converge = embedded.converge.as_ref().unwrap();
        assert_eq!(converge.authority, crate::converge::Authority::Apply);
        assert_eq!(converge.budget.wall_time_ms, 30_000);

        std::fs::write(&path, "this is not valid toml = [").unwrap();
        assert!(CovOptConfig::load_or_embedded(path).is_err());
    }

    #[test]
    fn parses_project_owned_converge_goal_defaults() {
        let config: CovOptConfig = toml::from_str(
            r#"
version = 3
[converge]
authority = "suggest"
future_mode = "experimental"
[converge.budget]
wall_time_ms = 1234
max_iterations = 3
"#,
        )
        .unwrap();
        let goal = config.converge.unwrap();
        assert_eq!(goal.authority, crate::converge::Authority::Suggest);
        assert_eq!(goal.budget.wall_time_ms, 1234);
        assert_eq!(goal.constraints.len(), 3);
        assert_eq!(goal.extensions["future_mode"], "experimental");
    }
}
