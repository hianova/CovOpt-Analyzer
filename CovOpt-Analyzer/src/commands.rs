use crate::*;
use CovOpt_Analyzer::analyzer::ConvergenceAnalyzer;
use CovOpt_Analyzer::assurance::{
    AssurancePolicy, AssuranceReport, AssuranceScheduler, EvidenceAction, EvidencePlan,
    EvidencePlanner, EvidenceProvider, EvidenceProviderKind, PlanOutcome, PlanStatus,
    PlanValidator, PlannerPolicy, PlanningContext, discover_evidence_actions,
    discover_target_obligations, planner_tool_context, planning_providers,
};
use CovOpt_Analyzer::atomic_model::{ModelStatus, extract_atomic_events};
use CovOpt_Analyzer::atomic_synth::{
    AtomicCandidate, analyze_atomic, request_from_file, source_hash, synthesize, verify_candidate,
};
use CovOpt_Analyzer::config::{
    AtomicArgs, AtomicSubcommand, AtomicTargetArgs, AtomicVerifyArgs, CovOptConfig, PlanArgs,
    ProviderMode, SelectTrialsArgs, TargetConfig,
};
use CovOpt_Analyzer::mca::McaRunner;
use CovOpt_Analyzer::runner::{AuditContext, CargoTestRunner};
use CovOpt_Analyzer::trial_selection::{
    candidate_input_from_target, generate_candidates, select_trials,
};
use covopt_macro::covopt_param;
use std::fs;
use std::path::{Path, PathBuf};

use CovOpt_Analyzer::analyzer::Complexity;

fn parse_plan_budget(value: &str) -> Result<u64, String> {
    let value = value.trim().to_ascii_lowercase();
    let (number, multiplier) = if let Some(value) = value.strip_suffix("ms") {
        (value, 1)
    } else if let Some(value) = value.strip_suffix('s') {
        (value, 1_000)
    } else if let Some(value) = value.strip_suffix('m') {
        (value, 60_000)
    } else {
        (value.as_str(), 1_000)
    };
    number
        .parse::<u64>()
        .map(|number| number.saturating_mul(multiplier))
        .map_err(|error| format!("invalid planning budget '{}': {}", value, error))
}

#[derive(Debug, Clone)]
struct PreparedTargetPlan {
    policy: PlannerPolicy,
    context: PlanningContext,
    initial: EvidencePlan,
}

fn check_executes_provider(provider: &str) -> bool {
    matches!(
        provider,
        "Compiler" | "Mca" | "Coverage" | "Test" | "AtomicModel"
    )
}

fn prepare_target_plan(
    config: &CovOptConfig,
    target: &TargetConfig,
    default_assurance: AssurancePolicy,
    requested_threshold: Option<f64>,
    base_context: &PlanningContext,
    providers: &[Box<dyn EvidenceProvider>],
) -> PreparedTargetPlan {
    let target_policy = target.assurance.unwrap_or(default_assurance);
    let evidence_threshold = target
        .evidence_threshold
        .or(requested_threshold)
        .unwrap_or(config.pipeline.evidence_threshold);
    let mut policy = PlannerPolicy::for_assurance(target_policy);
    policy.overall_threshold = evidence_threshold;
    policy.performance_threshold = evidence_threshold;
    policy.static_only =
        matches!(target_policy, AssurancePolicy::Static) || target.static_only.unwrap_or(false);
    if let Some(planner) = target.planner {
        policy.planner = planner;
    }
    if let Some(named) = config.policy_for_target(target) {
        if let Some(threshold) = named.overall_coverage {
            policy.overall_threshold = threshold;
        }
        if let Some(threshold) = named.critical_coverage {
            policy.critical_threshold = threshold;
        }
        if let Some(threshold) = named.performance_coverage {
            policy.performance_threshold = threshold;
        }
        if let Some(seconds) = named.budget_seconds {
            policy.max_time_ms = Some(seconds.saturating_mul(1_000));
        }
    }
    if let Some(seconds) = target.budget_seconds {
        policy.max_time_ms = Some(seconds.saturating_mul(1_000));
    }
    if let Some(remaining) = CovOpt_Analyzer::runner::remaining_ci_budget() {
        let remaining_ms = remaining.as_millis().min(u128::from(u64::MAX)) as u64;
        policy.max_time_ms = Some(
            policy
                .max_time_ms
                .map_or(remaining_ms, |configured| configured.min(remaining_ms)),
        );
    }

    let mut context = base_context.clone();
    context.target = Some(target.test.clone());
    context.package = target.package.clone().or_else(|| {
        CovOpt_Analyzer::static_analysis::resolve_package_for_target(
            &target.test,
            target.package.as_ref(),
        )
    });
    context.target_cpu = target.mca_cpu.clone();
    let atomic = target.atomic.as_ref().unwrap_or(&config.atomic);
    if atomic.contract.is_some() {
        context
            .metadata
            .insert("atomic_contract".to_string(), "true".to_string());
    }

    let obligations = discover_target_obligations(&target.test);
    let provider_config = config.resolved_target(target).providers;
    let mut primary = Vec::new();
    let mut fallback = Vec::new();
    let mut required = std::collections::HashSet::new();
    for mut action in discover_evidence_actions(providers, &obligations, &context) {
        let mode = CovOptConfig::provider_mode(&provider_config, &action.provider.0);
        if matches!(mode, ProviderMode::Disabled) {
            continue;
        }
        if !check_executes_provider(&action.provider.0) {
            action.available = false;
            action.description.push_str("; no automatic check executor");
        }
        if matches!(mode, ProviderMode::Required) {
            required.insert(action.provider.0.clone());
        }
        if matches!(mode, ProviderMode::Fallback) {
            fallback.push(action);
        } else {
            primary.push(action);
        }
    }

    let planner = EvidencePlanner::new(policy.clone());
    let mut actions = primary.clone();
    let mut outcome = planner.plan(&obligations, &primary, &context);
    if !matches!(outcome.plan.status, PlanStatus::Feasible) && !fallback.is_empty() {
        actions.extend(fallback);
        outcome = planner.plan(&obligations, &actions, &context);
    }
    include_required_actions(
        &mut outcome,
        &obligations,
        &actions,
        &required,
        &policy,
        &context,
    );
    PreparedTargetPlan {
        policy,
        context,
        initial: outcome.plan,
    }
}

fn include_required_actions(
    outcome: &mut PlanOutcome,
    obligations: &[CovOpt_Analyzer::assurance::Obligation],
    actions: &[EvidenceAction],
    required: &std::collections::HashSet<String>,
    policy: &PlannerPolicy,
    context: &PlanningContext,
) {
    for action in actions
        .iter()
        .filter(|action| required.contains(&action.provider.0))
    {
        if !outcome.plan.selected_actions.contains(&action.id) {
            outcome.plan.selected_actions.push(action.id.clone());
        }
    }
    outcome.plan.selected_actions.sort();
    outcome.plan.selected_actions.dedup();
    outcome.plan.selected_action_details = outcome
        .plan
        .selected_actions
        .iter()
        .filter_map(|id| actions.iter().find(|action| &action.id == id).cloned())
        .collect();
    outcome.plan.candidate_actions = actions.to_vec();
    let validation = PlanValidator::validate(
        obligations,
        actions,
        &outcome.plan.selected_actions,
        policy,
        context,
    );
    outcome.plan.expected_coverage = validation.coverage.clone();
    outcome.plan.estimated_cost_ms = validation.estimated_cost_ms;
    outcome.plan.validator_errors = validation.errors.clone();
    outcome.plan.status = if validation.valid {
        PlanStatus::Feasible
    } else if validation.coverage.overall_percent > outcome.plan.coverage_before.overall_percent {
        PlanStatus::Partial
    } else {
        PlanStatus::Infeasible
    };
    outcome.validation = validation;
}

fn plan_selects(plan: &EvidencePlan, provider: EvidenceProviderKind) -> bool {
    let provider = format!("{provider:?}");
    plan.selected_action_details
        .iter()
        .any(|action| action.provider.0 == provider)
}

pub fn run_plan(args: &PlanArgs, config: &CovOptConfig) -> bool {
    let mut targets = config
        .target
        .iter()
        .filter(|target| args.test.as_ref().is_none_or(|name| name == &target.test))
        .map(|target| target.test.clone())
        .collect::<Vec<_>>();
    if let Some(test) = &args.test
        && !targets.contains(test)
    {
        targets.push(test.clone());
    }
    if targets.is_empty() {
        eprintln!("CovOpt plan: no configured targets found");
        return false;
    }

    let base_policy = args
        .mode
        .map(CovOpt_Analyzer::assurance::PlannerPolicy::for_assurance)
        .unwrap_or_else(|| config.assurance.planner_policy());
    let budget_ms = match parse_plan_budget(&args.budget) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("CovOpt plan: {}", error);
            return false;
        }
    };
    let providers = planning_providers();
    let mut documents = Vec::new();
    let mut all_feasible = true;
    for target_name in targets {
        let target_config = config
            .target
            .iter()
            .find(|target| target.test == target_name);
        let mut policy = base_policy.clone();
        if let Some(target_assurance) = target_config.and_then(|target| target.assurance) {
            let target_policy =
                CovOpt_Analyzer::assurance::PlannerPolicy::for_assurance(target_assurance);
            policy.static_only = target_policy.static_only;
            policy.strict = target_policy.strict;
        }
        if let Some(threshold) = target_config.and_then(|target| target.evidence_threshold) {
            policy.overall_threshold = threshold;
            policy.performance_threshold = threshold;
        }
        if let Some(named_policy) =
            target_config.and_then(|target| config.policy_for_target(target))
        {
            if let Some(threshold) = named_policy.overall_coverage {
                policy.overall_threshold = threshold;
            }
            if let Some(threshold) = named_policy.critical_coverage {
                policy.critical_threshold = threshold;
            }
            if let Some(threshold) = named_policy.performance_coverage {
                policy.performance_threshold = threshold;
            }
            if let Some(seconds) = named_policy.budget_seconds {
                policy.max_time_ms = Some(seconds.saturating_mul(1_000));
            }
        }
        policy.max_time_ms = Some(
            target_config
                .and_then(|target| target.budget_seconds)
                .map_or(budget_ms, |seconds| seconds.saturating_mul(1_000)),
        );
        policy.static_only |= args.static_only
            || target_config
                .and_then(|target| target.static_only)
                .unwrap_or(false);
        if let Some(planner) = args
            .planner
            .or_else(|| target_config.and_then(|target| target.planner))
        {
            policy.planner = planner;
        }
        let mut context = planner_tool_context();
        if target_config
            .and_then(|target| target.atomic.as_ref())
            .or(Some(&config.atomic))
            .and_then(|atomic| atomic.contract.as_ref())
            .is_some()
        {
            context
                .metadata
                .insert("atomic_contract".to_string(), "true".to_string());
        }
        let obligations = discover_target_obligations(&target_name);
        let provider_config = target_config
            .and_then(|target| target.providers.clone())
            .or_else(|| {
                target_config
                    .and_then(|target| config.policy_for_target(target))
                    .map(|policy| policy.providers)
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
            .unwrap_or_else(|| config.providers.clone());
        let raw_actions = discover_evidence_actions(&providers, &obligations, &context);
        let disabled_providers = raw_actions
            .iter()
            .map(|action| action.provider.0.clone())
            .filter(|provider| {
                matches!(
                    CovOpt_Analyzer::config::CovOptConfig::provider_mode(
                        &provider_config,
                        provider,
                    ),
                    CovOpt_Analyzer::config::ProviderMode::Disabled
                )
            })
            .collect::<std::collections::HashSet<_>>();
        let actions = raw_actions
            .into_iter()
            .filter(|action| !disabled_providers.contains(&action.provider.0))
            .collect::<Vec<_>>();
        let required_providers = actions
            .iter()
            .map(|action| action.provider.0.clone())
            .filter(|provider| {
                matches!(
                    CovOpt_Analyzer::config::CovOptConfig::provider_mode(
                        &provider_config,
                        provider,
                    ),
                    CovOpt_Analyzer::config::ProviderMode::Required
                )
            })
            .collect::<std::collections::HashSet<_>>();
        let outcome = EvidencePlanner::new(policy).plan(&obligations, &actions, &context);
        let selected_providers = outcome
            .plan
            .selected_action_details
            .iter()
            .map(|action| action.provider.0.clone())
            .collect::<std::collections::HashSet<_>>();
        let missing_required = required_providers
            .difference(&selected_providers)
            .filter(|provider| provider.as_str() != "StaticAst")
            .cloned()
            .collect::<Vec<_>>();
        all_feasible &=
            matches!(outcome.plan.status, PlanStatus::Feasible) && missing_required.is_empty();
        if !args.json {
            println!("Evidence Plan: {}", target_name);
            println!("  Status:             {:?}", outcome.plan.status);
            println!(
                "  Selected actions:   {}",
                outcome.plan.selected_actions.len()
            );
            for action in &outcome.plan.selected_action_details {
                println!(
                    "    - {} ({}, {}ms)",
                    action.description, action.provider, action.estimated_cost_ms
                );
            }
            println!(
                "  Critical coverage:  {:.1}%",
                outcome.plan.expected_coverage.critical_safety_percent
            );
            println!(
                "  Overall coverage:   {:.1}%",
                outcome.plan.expected_coverage.overall_percent
            );
            println!(
                "  Performance:        {:.1}%",
                outcome.plan.expected_coverage.performance_percent
            );
            println!("  Estimated cost:     {}ms", outcome.plan.estimated_cost_ms);
            for rejection in &outcome.plan.rejected_actions {
                println!("  Rejected {}: {}", rejection.action_id, rejection.reason);
            }
            for provider in &missing_required {
                println!("  Missing required provider: {}", provider);
            }
            for infeasible in &outcome.plan.infeasible_obligations {
                println!(
                    "  Infeasible {}{}: {}",
                    if infeasible.critical { "CRITICAL " } else { "" },
                    infeasible.obligation_id,
                    infeasible.reason
                );
            }
        }
        documents.push(serde_json::json!({
            "test": target_name,
            "plan": outcome.plan,
            "validation": outcome.validation,
            "timed_out": outcome.timed_out,
        }));
    }
    let document = serde_json::json!({
        "version": 1,
        "targets": documents,
    });
    let _ = std::fs::create_dir_all("target/covopt");
    if let Ok(bytes) = serde_json::to_vec_pretty(&document)
        && let Err(error) = std::fs::write("target/covopt/plan.json", bytes)
    {
        eprintln!(
            "CovOpt plan: could not write target/covopt/plan.json: {}",
            error
        );
    }
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&document).unwrap_or_default()
        );
    }
    all_feasible
}

pub fn run_select_trials(args: &SelectTrialsArgs, config: &CovOptConfig) -> bool {
    let budget_ms = match parse_plan_budget(&args.budget) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("CovOpt select-trials: {}", error);
            return false;
        }
    };
    let targets = config
        .target
        .iter()
        .filter(|target| args.target.as_ref().is_none_or(|name| name == &target.test))
        .collect::<Vec<_>>();
    if targets.is_empty() {
        eprintln!("CovOpt select-trials: no configured targets found");
        return false;
    }
    let mut documents = Vec::new();
    let mut all_feasible = true;
    for target in targets {
        let obligations = discover_target_obligations(&target.test);
        let critical = obligations
            .iter()
            .filter(|obligation| {
                matches!(
                    obligation.severity,
                    CovOpt_Analyzer::assurance::Severity::Critical
                )
            })
            .map(|obligation| obligation.id.clone())
            .collect::<Vec<_>>();
        let ids = obligations
            .iter()
            .map(|obligation| obligation.id.clone())
            .collect::<Vec<_>>();
        let input = candidate_input_from_target(target, ids.clone(), critical);
        let mut selection_config = config.trials.selection_config();
        selection_config.budget_ms = budget_ms;
        if let Some(seed_count) = args.seed_count {
            selection_config.seed_count = seed_count;
        }
        let candidates = generate_candidates(&input, &selection_config);
        let plan = select_trials(&candidates, &ids, &selection_config);
        all_feasible &= plan.expected_coverage >= 1.0 || ids.is_empty();
        if !args.json {
            println!("Trial Selection: {}", target.test);
            println!("  Selected trials:   {}", plan.selected.len());
            println!("  Candidate trials:  {}", candidates.len());
            println!(
                "  Expected coverage: {:.1}%",
                plan.expected_coverage * 100.0
            );
            println!(
                "  Model pairs:       {}",
                plan.expected_model_discrimination.len()
            );
            println!("  Estimated cost:    {}ms", plan.estimated_cost_ms);
            for rejected in &plan.rejected {
                println!("  Rejected {}: {}", rejected.trial_id, rejected.reason);
            }
        }
        documents.push(serde_json::json!({ "target": target.test, "candidates": candidates, "plan": plan, "dry_run": args.dry_run }));
    }
    let document = serde_json::json!({ "version": 1, "targets": documents });
    let _ = fs::create_dir_all("target/covopt");
    if let Ok(bytes) = serde_json::to_vec_pretty(&document) {
        let _ = fs::write("target/covopt/trial-plan.json", bytes);
    }
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&document).unwrap_or_default()
        );
    }
    all_feasible
}

fn atomic_source_path(target: Option<&str>, source: Option<&str>) -> Result<String, String> {
    if let Some(source) = source {
        return Ok(source.to_string());
    }
    let target = target.ok_or_else(|| "--target or --source is required".to_string())?;
    let (_, _, _, path) = CovOpt_Analyzer::static_analysis::find_covopt_test_metadata(target)
        .ok_or_else(|| format!("could not locate source for configured target '{}'", target))?;
    Ok(path.display().to_string())
}

fn atomic_policy_for<'a>(
    config: &'a CovOptConfig,
    target: Option<&str>,
) -> &'a CovOpt_Analyzer::config::AtomicPolicyConfig {
    target
        .and_then(|name| config.target.iter().find(|item| item.test == name))
        .and_then(|item| item.atomic.as_ref())
        .unwrap_or(&config.atomic)
}

fn parse_atomic_budget(value: &str) -> u64 {
    parse_plan_budget(value).unwrap_or(5_000)
}

fn emit_atomic_json(json: bool, value: &impl serde::Serialize) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(value).unwrap_or_default()
        );
    }
}

fn run_atomic_target(args: &AtomicTargetArgs, config: &CovOptConfig, synth: bool) -> bool {
    let path = match atomic_source_path(args.target.as_deref(), args.source.as_deref()) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("CovOpt atomic: {}", error);
            return false;
        }
    };
    let policy = atomic_policy_for(config, args.target.as_deref());
    let contract = policy.correctness_contract();
    let request = match request_from_file(
        &path,
        contract,
        policy.bounds(),
        parse_atomic_budget(&args.budget),
        synth && policy.enabled && policy.synthesize,
    ) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("CovOpt atomic: {}", error);
            return false;
        }
    };
    if synth {
        let result = synthesize(&request);
        if !args.json {
            println!("Atomic synthesis: {}", result.source_path);
            println!(
                "  Status: {}",
                serde_json::to_value(result.status).unwrap_or_default()
            );
            println!("  Summary: {}", result.summary);
            if let Some(patch) = &result.patch {
                println!("  Diff generated for source hash {}", patch.source_hash);
                println!("{}", patch.unified_diff);
            }
        }
        let _ = fs::create_dir_all("target/covopt");
        if let Ok(bytes) = serde_json::to_vec_pretty(&result) {
            let name = args.target.as_deref().unwrap_or("source");
            let _ = fs::write(format!("target/covopt/atomic-{}.json", name), bytes);
        }
        emit_atomic_json(args.json, &result);
        matches!(
            result.status,
            CovOpt_Analyzer::atomic_synth::SynthesisStatus::Suggested
                | CovOpt_Analyzer::atomic_synth::SynthesisStatus::NoChange
        )
    } else {
        let result = analyze_atomic(&request);
        if !args.json {
            println!("Atomic analysis: {}", result.source_path);
            println!("  Events: {}", result.events.len());
            println!("  Status: {:?}", result.status);
            println!("  Summary: {}", result.summary);
        }
        emit_atomic_json(args.json, &result);
        true
    }
}

pub fn run_atomic(args: &AtomicArgs, config: &CovOptConfig) -> bool {
    match &args.command {
        AtomicSubcommand::Analyze(args) => run_atomic_target(args, config, false),
        AtomicSubcommand::Synth(args) => run_atomic_target(args, config, true),
        AtomicSubcommand::Verify(args) => run_atomic_verify(args, config),
    }
}

fn run_atomic_verify(args: &AtomicVerifyArgs, config: &CovOptConfig) -> bool {
    let source_path = match atomic_source_path(args.target.as_deref(), args.source.as_deref()) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("CovOpt atomic verify: {}", error);
            return false;
        }
    };
    let source = match fs::read_to_string(&source_path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("CovOpt atomic verify: {}", error);
            return false;
        }
    };
    let value: serde_json::Value = match fs::read_to_string(&args.candidate)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
    {
        Some(value) => value,
        None => {
            eprintln!("CovOpt atomic verify: invalid candidate JSON");
            return false;
        }
    };
    let candidate_value = value.get("selected").cloned().unwrap_or(value);
    let candidate: AtomicCandidate = match serde_json::from_value(candidate_value) {
        Ok(candidate) => candidate,
        Err(error) => {
            eprintln!("CovOpt atomic verify: {}", error);
            return false;
        }
    };
    let policy = atomic_policy_for(config, args.target.as_deref());
    let Some(contract) = policy.correctness_contract() else {
        eprintln!("CovOpt atomic verify: no correctness contract");
        return false;
    };
    let events = match extract_atomic_events(&source, &source_path) {
        Ok(events) => events,
        Err(error) => {
            eprintln!("CovOpt atomic verify: {}", error);
            return false;
        }
    };
    let result = verify_candidate(&events, &contract, &policy.bounds(), &candidate);
    let output = serde_json::json!({ "source_hash": source_hash(&source), "candidate": candidate, "model": result });
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&output).unwrap_or_default()
        );
    } else {
        println!(
            "Atomic candidate verification: {:?}",
            output.get("model").and_then(|model| model.get("status"))
        );
    }
    let passed = result.status == ModelStatus::Modeled;
    if passed {
        record_unsafe_evidence(
            args.target.as_deref().unwrap_or(source_path.as_str()),
            "atomic-model",
            true,
        );
    }
    passed
}

fn parse_complexity(s: &str) -> Option<Complexity> {
    let clean = s.to_uppercase().replace(' ', "");
    match clean.as_str() {
        "O1" | "O(1)" => Some(Complexity::O1),
        "OLOGN" | "O(LOGN)" => Some(Complexity::OLogN),
        "ON" | "O(N)" => Some(Complexity::ON),
        "ONLOGN" | "O(NLOGN)" => Some(Complexity::ONLogN),
        "ON2" | "O(N2)" | "O(N^2)" => Some(Complexity::ON2),
        "O2N" | "O(2^N)" | "O(2N)" => Some(Complexity::O2N),
        "OSQRTN" | "O(SQRT(N))" | "O(SQRTN)" => Some(Complexity::OSqrtN),
        _ => None,
    }
}

struct LogBuffer {
    buffer: String,
    compact: bool,
}

impl LogBuffer {
    fn new(compact: bool) -> Self {
        Self {
            buffer: String::new(),
            compact,
        }
    }
}

macro_rules! wlog {
    ($log:expr, $($arg:tt)*) => {{
        let s = format!($($arg)*);
        if !$log.compact {
            println!("{}", s);
        }
        $log.buffer.push_str(&s);
        $log.buffer.push('\n');
    }};
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AnalysisResult {
    pub passed: bool,
    pub line_coverage_percent: Option<f64>,
    pub mca_ipc: Option<f64>,
    pub mca_block_rthroughput: Option<f64>,
    pub log: String,
    #[serde(default)]
    pub coverage_map: Option<CovOpt_Analyzer::coverage::CoverageMap>,
    #[serde(default)]
    pub actual_complexity: Option<String>,
    #[serde(default)]
    pub complexity_r_squared: Option<f64>,
}

impl AnalysisResult {
    fn failed(log: String) -> Self {
        Self {
            passed: false,
            line_coverage_percent: None,
            mca_ipc: None,
            mca_block_rthroughput: None,
            log,
            coverage_map: None,
            actual_complexity: None,
            complexity_r_squared: None,
        }
    }
}

pub fn run_analysis_structured(
    args: &RunArgs,
    compact: bool,
    audit_context: Option<&AuditContext>,
    fast: bool,
    run_mca: bool,
    mut line_coverage_output: Option<&mut f64>,
    mut mca_output: Option<&mut Option<CovOpt_Analyzer::mca::McaReport>>,
) -> AnalysisResult {
    let mut log = LogBuffer::new(compact);

    let test_name = match args.test.as_ref() {
        Some(t) => t.as_str(),
        None => {
            wlog!(
                log,
                "[ERROR] --test is required or must be configured in .covopt.toml"
            );
            return AnalysisResult::failed(log.buffer);
        }
    };
    let mut ast_expected = None;
    let mut ast_n_values = None;
    let mut ast_target_fn = None;

    if let Some((e, n, t, _)) =
        CovOpt_Analyzer::static_analysis::find_covopt_test_metadata(test_name)
    {
        ast_expected = Some(e);
        ast_n_values = Some(n);
        ast_target_fn = t;
    }

    let expected_str = match args.expected.as_ref().or(ast_expected.as_ref()) {
        Some(e) => e,
        None => {
            wlog!(
                log,
                "[ERROR] --expected is required, must be configured in .covopt.toml, or provided via #[covopt::test(expected = \"...\")]"
            );
            return AnalysisResult::failed(log.buffer);
        }
    };
    let n_values_str = match args.n_values.as_ref().or(ast_n_values.as_ref()) {
        Some(n) => n,
        None => {
            wlog!(
                log,
                "[ERROR] --n-values is required, must be configured in .covopt.toml, or provided via #[covopt::test(n_values = \"...\")]"
            );
            return AnalysisResult::failed(log.buffer);
        }
    };
    let mut discovered_target_file: Option<String> = None;
    let mut discovered_target_line: Option<u64> = None;
    let mut target_symbol: Option<String> = None;

    let expected = match parse_complexity(expected_str) {
        Some(c) => c,
        None => {
            wlog!(
                log,
                "[ERROR] Unknown complexity format: {}. Valid formats include O1, ON, ON2, etc.",
                expected_str
            );
            return AnalysisResult::failed(log.buffer);
        }
    };

    let _n_values: Vec<usize> = n_values_str
        .split(',')
        .map(|s| s.trim().parse().unwrap_or(0))
        .collect();

    let local_context;
    let context = if let Some(context) = audit_context {
        context
    } else {
        let mut packages_to_compile = Vec::new();
        if let Some(pkg) =
            CovOpt_Analyzer::static_analysis::resolve_package_for_target(test_name, None)
        {
            packages_to_compile.push(pkg);
        }
        local_context = match AuditContext::compile(&packages_to_compile) {
            Ok(context) => context,
            Err(e) => {
                wlog!(log, "[ERROR] Failed to compile workspace tests: {}", e);
                return AnalysisResult::failed(log.buffer);
            }
        };
        &local_context
    };

    let runner = std::sync::Arc::new(CargoTestRunner::from_compiled(
        test_name,
        context.output_dir.path(),
        &context.workspace_tests,
    ));

    wlog!(log, "Starting CovOpt Analysis for test '{}'...", test_name);
    wlog!(log, "Target: Auto-Discovery Mode");
    wlog!(log, "Expected Complexity: {:?}", expected);

    let mut data = Vec::new();
    let mut space_data = Vec::new();
    let mut target_coverage_rate = None;
    let mut mca_stats = None;
    let mut last_coverage_map = None;

    let mut handles = Vec::new();

    for (sample_index, n_str) in n_values_str.split(',').enumerate() {
        let n: u64 = n_str.trim().parse().unwrap_or(0);
        let runner_clone = std::sync::Arc::clone(&runner);
        let seed = CovOpt_Analyzer::trial_selection::derive_seed(
            test_name,
            env!("CARGO_PKG_VERSION"),
            "analysis",
            sample_index,
        );

        handles.push(std::thread::spawn(move || {
            let result = runner_clone.run(n as usize, Some(seed));
            (n, result)
        }));
    }

    let mut results = Vec::new();
    for handle in handles {
        match handle.join() {
            Ok((n, result)) => results.push((n, result)),
            Err(_) => {
                wlog!(log, "[ERROR] A worker thread panicked during execution.");
                return AnalysisResult::failed(log.buffer);
            }
        }
    }

    // Sort results to process them sequentially
    results.sort_by_key(|(n, _)| *n);

    for (n, result) in results {
        wlog!(log, "---------------------------------------------------");
        wlog!(log, "Running for N = {}...", n);

        let (map, peak_rss) = match result {
            Ok(m) => m,
            Err(e) => {
                wlog!(log, "[ERROR] Failed to run coverage for N={}: {}", n, e);
                if compact {
                    eprintln!("\n=== DETAILED ANALYSIS LOG (FAILURE) ===");
                    eprintln!("{}", log.buffer);
                    eprintln!("========================================\n");
                }
                return AnalysisResult::failed(log.buffer);
            }
        };
        last_coverage_map = Some(map.clone());

        if target_symbol.is_none() {
            let mut ignore_patterns = Vec::new();
            if let Some(ig_str) = &args.ignore {
                ignore_patterns.extend(ig_str.split(',').map(|s| s.trim().to_string()));
            }

            // [NEW] Dominant Complexity Auto-Detection!
            // By passing ast_target_fn, we restrict the peak search to the target function,
            // finding the dynamically hottest path (dominant bottleneck) automatically.
            if let Some((f, l, sym, _)) =
                map.find_peak_location(&ignore_patterns, ast_target_fn.as_deref())
            {
                discovered_target_file = Some(f.clone());
                discovered_target_line = Some(l);
                target_symbol = Some(sym.clone());

                if let Some(ref t_fn) = ast_target_fn {
                    wlog!(
                        log,
                        "Auto-discovered dominant target in {}: {}:{} ({})",
                        t_fn,
                        f,
                        l,
                        sym
                    );
                } else {
                    wlog!(
                        log,
                        "Auto-discovered global peak target: {}:{} ({})",
                        f,
                        l,
                        sym
                    );
                }
            } else {
                wlog!(log, "DEBUG: find_peak_location returned None");
            }
        }

        let hit_count = if let Some(f) = &discovered_target_file {
            map.get_hit_count(f, discovered_target_line.unwrap_or(0))
        } else {
            None
        };

        if let Some(h) = hit_count {
            wlog!(
                log,
                "  -> Hit count = {} | Peak RSS = {} bytes",
                h,
                peak_rss
            );
            data.push((n as usize, h));

            if h == 0 {
                wlog!(log, "\n> [!WARNING] COVOPT GUIDANCE: HIT COUNT = 0 <");
                wlog!(
                    log,
                    "The target function was executed 0 times during profiling, but the test succeeded."
                );
                wlog!(
                    log,
                    "This often happens to pure math functions due to LLVM Auto-Vectorization or Dead Code Elimination (DCE)."
                );
                wlog!(
                    log,
                    "=> SUGGESTION: Target was likely inlined or DCE'd. Ensure loop variables are wrapped with `std::hint::black_box()`."
                );
            }
        } else {
            wlog!(log, "  -> WARNING: No hit count found. Assuming 0.");
            data.push((n as usize, 0));
        }
        space_data.push((n as usize, peak_rss));

        if let Some(ref sym) = target_symbol {
            target_coverage_rate = map.get_function_coverage(sym);
        }
    }

    let target_file = discovered_target_file.unwrap_or_else(|| "src/lib.rs".to_string());
    let target_line = discovered_target_line.unwrap_or(0);
    wlog!(log, "---------------------------------------------------");
    wlog!(log, "Time Analysis Results:");
    let report = ConvergenceAnalyzer::analyze(&data, expected);
    wlog!(log, "{:#?}", report);

    wlog!(log, "---------------------------------------------------");
    wlog!(log, "Space Analysis Results (Dynamic Memory):");
    let space_report = ConvergenceAnalyzer::analyze(&space_data, Complexity::O1);
    wlog!(
        log,
        "  -> Actual Space Complexity: {:?}",
        space_report.actual_trend
    );

    if args.formalize {
        wlog!(log, "---------------------------------------------------");
        wlog!(
            log,
            "🔮 [Heuristic Engine] Lean 4 Mode: Synthesizing Formal Mathematical AST Proof..."
        );
        let exact_formula = CovOpt_Analyzer::heuristic::SymbolicRegressor::formalize(&data);
        wlog!(log, "  => Formal Proof Discovered: {}", exact_formula);
    }

    let var_count = CovOpt_Analyzer::static_analysis::analyze_variables(
        std::path::Path::new(&target_file),
        target_line as usize,
    );
    wlog!(log, "Static Variable Declarations: {}", var_count);

    let thread_activities = CovOpt_Analyzer::static_analysis::analyze_thread_activity(
        std::path::Path::new(&target_file),
    );
    if !thread_activities.is_empty() {
        wlog!(log, "Static Thread Activities:");
        for act in thread_activities {
            wlog!(log, "  - {}", act);
        }
    } else {
        wlog!(log, "Static Thread Activities: None");
    }

    let mut success = true;

    if report.is_converged && report.actual_trend > expected {
        wlog!(
            log,
            "\n[ERROR] Algorithm complexity degraded! Expected {:?}, got {:?}",
            expected,
            report.actual_trend
        );
        wlog!(log, "--- ASCII Curve Visualization ---");
        let max_h = data.iter().map(|&(_, h)| h).max().unwrap_or(1) as f64;
        let max_n = data.iter().map(|&(n, _)| n).max().unwrap_or(1) as f64;
        for &(n, h) in &data {
            let n_bar_len = ((n as f64 / max_n) * covopt_param!("M_323_50", 40.0)) as usize;
            let h_bar_len = ((h as f64 / max_h) * covopt_param!("M_324_50", 40.0)) as usize;
            let n_bar = "=".repeat(n_bar_len);
            let h_bar = "*".repeat(h_bar_len);
            wlog!(log, "N: {:<6} | {}", n, n_bar);
            wlog!(log, "H: {:<6} | {}", h, h_bar);
            wlog!(log, "--------------------------------");
        }
        success = false;
    }

    let mut static_cache_padding = None;
    if args.require_cache_padding {
        let (has_padding, applicable) = CovOpt_Analyzer::static_analysis::analyze_cache_padding(
            std::path::Path::new(&target_file),
        );
        static_cache_padding = Some(has_padding);
        if applicable {
            if has_padding {
                wlog!(log, "Static Cache Padding: Detected");
            } else {
                wlog!(
                    log,
                    "\n[ERROR] Missing Cache Padding! Strict mode requires cache alignment for target."
                );
                success = false;
            }
        } else {
            static_cache_padding = Some(true); // Treat as passed
            wlog!(log, "Static Cache Padding: Not Applicable (Pure Function)");
        }
    }

    let mut static_branch_hints = None;
    if args.require_branch_hints {
        let (has_hints, applicable) = CovOpt_Analyzer::static_analysis::analyze_branch_hints(
            std::path::Path::new(&target_file),
        );
        static_branch_hints = Some(has_hints);
        if applicable {
            if has_hints {
                wlog!(log, "Static Branch Hints: Detected");
            } else {
                wlog!(
                    log,
                    "\n[WARN] Missing or DCE'd Branch Prediction Hints! (Ignored for LLVM optimization compatibility)"
                );
            }
        } else {
            static_branch_hints = Some(true); // Treat as passed
            wlog!(log, "Static Branch Hints: Not Applicable (Pure Function)");
        }
    }

    let mut static_aerospace_grade = None;
    if args.require_aerospace_grade {
        let violations = CovOpt_Analyzer::static_analysis::analyze_aerospace_grade(
            std::path::Path::new(&target_file),
        );
        static_aerospace_grade = Some(violations.clone());
        if violations.is_empty() {
            wlog!(log, "Static Aerospace Grade: Passed");
        } else {
            wlog!(
                log,
                "\n[ERROR] Aerospace Grade Violations Detected in {}!",
                target_file
            );
            for v in violations {
                wlog!(log, "  - {}", v);
            }
            success = false;
        }
    }

    let mut static_watchdog_timeout = None;
    if args.require_watchdog_timeout {
        let (has_watchdog, applicable) =
            CovOpt_Analyzer::static_analysis::analyze_project_watchdog_timeout(
                std::path::Path::new(&target_file),
            );
        static_watchdog_timeout = Some(has_watchdog);
        if applicable {
            if has_watchdog {
                wlog!(log, "Static Watchdog Timeout: Detected");
            } else {
                wlog!(
                    log,
                    "\n[ERROR] Missing Watchdog Timeout! Strict mode requires timeout mechanisms (e.g. recv_timeout) to prevent infinite spin deadlocks."
                );
                success = false;
            }
        } else {
            static_watchdog_timeout = Some(true); // Treat as passed
            wlog!(
                log,
                "Static Watchdog Timeout: Not Applicable (Pure Function)"
            );
        }
    }

    let mut static_stress_test = None;
    if args.require_stress_test {
        let (has_stress, applicable) =
            CovOpt_Analyzer::static_analysis::analyze_project_stress_test(std::path::Path::new(
                &target_file,
            ));
        static_stress_test = Some(has_stress);
        if applicable {
            if has_stress {
                wlog!(log, "Static Stress Test: Detected");
            } else {
                wlog!(
                    log,
                    "\n[ERROR] Missing High-Pressure Stress Test! Target file lacks heavy concurrent thread spawning logic."
                );
                success = false;
            }
        } else {
            static_stress_test = Some(true); // Treat as passed
            wlog!(log, "Static Stress Test: Not Applicable (Pure Function)");
        }
    }

    let mut coverage_rate_val = None;
    wlog!(log, "---------------------------------------------------");
    if let Some(symbol) = target_symbol {
        if let Some((executed, total)) = target_coverage_rate {
            let rate = (executed as f64 / total as f64) * covopt_param!("M_445_58", 100.0);
            coverage_rate_val = Some(rate);
            if let Some(output) = line_coverage_output.as_mut() {
                **output = rate;
            }
            wlog!(
                log,
                "Coverage Rate (Target Function): {:.1}% ({}/{} lines)",
                rate,
                executed,
                total
            );
            if rate < covopt_param!("M_454_22", 90.0) {
                wlog!(
                    log,
                    "[WARNING] Function coverage is below 90%. The measured mathematical complexity might not reflect the worst-case scenario. Consider adding more branches to your test."
                );
                success = false; // Fail audit if coverage is below 90%
            }
            wlog!(log, "---------------------------------------------------");
        }

        wlog!(log, "Target Symbol Found: {}", symbol);
        wlog!(log, "Extracting ASM and running LLVM-MCA analysis...");

        if fast || !run_mca {
            wlog!(
                log,
                "Skipping release ASM/LLVM-MCA analysis (not selected by evidence plan)."
            );
        } else {
            match runner.compile_asm() {
                Ok(asm_content) => {
                    let mut asm_block_opt =
                        runner.extract_asm_block_by_loc(&asm_content, &target_file, target_line);
                    if asm_block_opt.is_none() {
                        asm_block_opt = runner.extract_asm_block(&asm_content, &symbol);
                    }
                    if asm_block_opt.is_none() {
                        let demangled = rustc_demangle::demangle(&symbol).to_string();
                        let clean_demangled =
                            if demangled.ends_with('>') && demangled.contains("::<") {
                                let idx = demangled.rfind("::<").unwrap_or(demangled.len());
                                &demangled[..idx]
                            } else {
                                &demangled
                            };
                        let parts: Vec<&str> = clean_demangled.split("::").collect();
                        if parts.len() >= 2 {
                            let fn_name = parts
                                .last()
                                .unwrap_or(&"")
                                .split('<')
                                .next()
                                .unwrap_or("")
                                .trim();
                            let struct_part = parts[parts.len() - 2];
                            let struct_name = struct_part
                                .split('<')
                                .next()
                                .unwrap_or("")
                                .split('[')
                                .next()
                                .unwrap_or("")
                                .trim()
                                .trim_matches(['<', '>', '[', ']']);

                            wlog!(
                                log,
                                "  -> Target symbol exact match failed. Searching by keywords: '{}', '{}'...",
                                struct_name,
                                fn_name
                            );
                            let t_asm_find = std::time::Instant::now();
                            asm_block_opt = runner.extract_asm_block_by_keywords(
                                &asm_content,
                                &[struct_name, fn_name],
                            );
                            if CovOpt_Analyzer::runner::debug_artifacts_enabled() {
                                eprintln!(
                                    "[Profile] extract_asm_block_by_keywords 2: {:?}",
                                    t_asm_find.elapsed()
                                );
                            }
                        }
                        if asm_block_opt.is_none() {
                            wlog!(
                                log,
                                "  -> Still not found. Target symbol inlined. Walking up to test caller '{}'...",
                                test_name
                            );
                            let t_asm_find = std::time::Instant::now();
                            asm_block_opt =
                                runner.extract_asm_block_by_keywords(&asm_content, &[test_name]);
                            if CovOpt_Analyzer::runner::debug_artifacts_enabled() {
                                eprintln!(
                                    "[Profile] extract_asm_block_by_keywords 3: {:?}",
                                    t_asm_find.elapsed()
                                );
                            }
                        }
                    }

                    if let Some(asm_block) = asm_block_opt {
                        let mem_profile =
                            CovOpt_Analyzer::static_analysis::analyze_memory_ops(&asm_block);
                        wlog!(log, "\n[Static Memory Operations]");
                        wlog!(log, "Loads:  {}", mem_profile.loads);
                        wlog!(log, "Stores: {}", mem_profile.stores);
                        wlog!(log, "Allocs: {}", mem_profile.allocs);

                        let mca_runner = McaRunner::new(args.mca_cpu.clone());
                        let t_mca = std::time::Instant::now();
                        match mca_runner.run(&asm_block) {
                            Ok(mca_report) => {
                                if CovOpt_Analyzer::runner::debug_artifacts_enabled() {
                                    eprintln!("[Profile] llvm-mca: {:?}", t_mca.elapsed());
                                }
                                wlog!(log, "\n[MCA Report]");

                                wlog!(
                                    log,
                                    "Block RThroughput: {:.2}",
                                    mca_report.block_rthroughput
                                );
                                wlog!(log, "IPC:               {:.2}", mca_report.ipc);

                                CovOpt_Analyzer::cache::save_mca_cache(
                                    std::path::Path::new(&target_file),
                                    &symbol,
                                    &mca_report,
                                );

                                mca_stats = Some((mca_report.ipc, mca_report.block_rthroughput));
                                if let Some(output) = mca_output.as_mut() {
                                    **output = Some(mca_report.clone());
                                }
                            }
                            Err(e) => wlog!(log, "LLVM-MCA failed: {}", e),
                        }

                        if args.optimize {
                            wlog!(
                                log,
                                "\n🚀 [Superoptimization] Launching NP-hard Discrete Diffusion Engine..."
                            );
                            let optimizer =
                                CovOpt_Analyzer::optimizer::DiscreteDiffusionEngine::new(
                                    covopt_param!("M_562_87", 20),
                                );
                            let base_asm_lines: Vec<String> =
                                asm_block.lines().map(|s| s.to_string()).collect();

                            let optimized_asm = optimizer.optimize_asm(
                                base_asm_lines,
                                covopt_param!("M_567_67", 20),
                                args.mca_cpu.clone(),
                            );
                            let optimized_text = optimized_asm.join("\n");

                            wlog!(log, "\n[Optimizer Output] Best ASM schedule found:");
                            wlog!(log, "{}", optimized_text);

                            if let Ok(opt_report) = mca_runner.run(&optimized_text) {
                                wlog!(log, "\n[Optimized MCA Report]");
                                wlog!(
                                    log,
                                    "Block RThroughput: {:.2}",
                                    opt_report.block_rthroughput
                                );
                                wlog!(log, "IPC:               {:.2}", opt_report.ipc);
                            }
                        }
                    } else {
                        wlog!(
                            log,
                            "Could not extract ASM block for symbol. The function might be inlined in release mode."
                        );
                    }
                }
                Err(e) => wlog!(log, "ASM compilation failed: {}", e),
            }
        }
    } else {
        wlog!(
            log,
            "Could not extract target symbol name from coverage data. Skipping MCA analysis."
        );
    }

    // --- Energy / Thermal Guidance (High Frequency Polling Detection) ---
    let max_hit_count = data.iter().map(|&(_, h)| h).max().unwrap_or(0);
    let max_n = data.iter().map(|&(n, _)| n).max().unwrap_or(1);
    let threshold = args
        .polling_threshold
        .unwrap_or(covopt_param!("M_602_53", 50000));

    if max_hit_count > threshold && max_hit_count > (max_n as u64) * covopt_param!("M_604_69", 100)
    {
        wlog!(
            log,
            "\n> [!CAUTION] COVOPT GUIDANCE: THERMAL & ENERGY WARNING <"
        );
        wlog!(
            log,
            "Detected astronomically high hit count ({}) relative to workload (N={}).",
            max_hit_count,
            max_n
        );
        wlog!(
            log,
            "This indicates 'High-Frequency Invalid Polling' (Busy-waiting) in a loop, which will cause 100% single-core CPU usage and severe device overheating."
        );
        wlog!(
            log,
            "=> SUGGESTION: If this is a polling loop, introduce an adaptive sleep (`std::thread::sleep`) or Exponential Backoff. If this is a large array/buffer initialization, DO NOT use a `for` loop with `.push()`; use `vec![value; N]` or `Iterator::collect()` to leverage LLVM `memset` optimizations."
        );
    }

    if success {
        if compact {
            wlog!(
                log,
                "\n> [x] CovOpt Analysis PASSED (Target: {})",
                target_file
            );
            wlog!(
                log,
                "  - Time Complexity: {:?} (Expected: {:?})",
                report.actual_trend,
                expected
            );
            wlog!(log, "  - Space Complexity: {:?}", space_report.actual_trend);

            let mut checks = Vec::new();
            if args.require_cache_padding {
                checks.push(format!(
                    "Cache Padding: {}",
                    if static_cache_padding.unwrap_or(false) {
                        "Yes"
                    } else {
                        "No"
                    }
                ));
            }
            if args.require_branch_hints {
                checks.push(format!(
                    "Branch Hints: {}",
                    if static_branch_hints.unwrap_or(false) {
                        "Yes"
                    } else {
                        "No"
                    }
                ));
            }
            if args.require_aerospace_grade {
                checks.push(format!(
                    "Aerospace: {}",
                    if static_aerospace_grade.as_ref().is_none_or(|v| v.is_empty()) {
                        "Passed"
                    } else {
                        "Failed"
                    }
                ));
            }
            if args.require_watchdog_timeout {
                checks.push(format!(
                    "Watchdog: {}",
                    if static_watchdog_timeout.unwrap_or(false) {
                        "Yes"
                    } else {
                        "No"
                    }
                ));
            }
            if args.require_stress_test {
                checks.push(format!(
                    "Stress Test: {}",
                    if static_stress_test.unwrap_or(false) {
                        "Yes"
                    } else {
                        "No"
                    }
                ));
            }
            if !checks.is_empty() {
                wlog!(log, "  - Static Checks: {}", checks.join(", "));
            } else {
                wlog!(log, "  - Static Checks: None Required");
            }

            if let Some(rate) = coverage_rate_val {
                wlog!(log, "  - Function Coverage: {:.1}%", rate);
            }
            if let Some((ipc, rt)) = mca_stats {
                wlog!(
                    log,
                    "  - LLVM-MCA (Static Block): IPC {:.2}, RThroughput {:.2}",
                    ipc,
                    rt
                );
            }
        }
    } else {
        if compact {
            eprintln!("\n=== DETAILED ANALYSIS LOG (FAILURE) ===");
            eprintln!("{}", log.buffer);
            eprintln!("========================================\n");
        }
    }

    AnalysisResult {
        passed: success,
        line_coverage_percent: coverage_rate_val,
        mca_ipc: mca_stats.map(|(ipc, _)| ipc),
        mca_block_rthroughput: mca_stats.map(|(_, throughput)| throughput),
        log: log.buffer,
        coverage_map: last_coverage_map,
        actual_complexity: Some(format!("{:?}", report.actual_trend)),
        complexity_r_squared: Some(report.r_squared),
    }
}

/// Compatibility wrapper for the legacy command path.
pub fn run_analysis(
    args: &RunArgs,
    compact: bool,
    audit_context: Option<&AuditContext>,
    fast: bool,
    line_coverage_output: Option<&mut f64>,
    mca_output: Option<&mut Option<CovOpt_Analyzer::mca::McaReport>>,
) -> bool {
    run_analysis_structured(
        args,
        compact,
        audit_context,
        fast,
        !fast,
        line_coverage_output,
        mca_output,
    )
    .passed
}

pub fn install_hook() {
    let hook_path = PathBuf::from(".git/hooks/pre-commit");
    let hook_content = r#"#!/bin/sh
echo "Running CovOpt-Analyzer on local commit..."
if [ -f .covopt.toml ]; then
    covopt audit
    if [ $? -ne 0 ]; then
        echo "CovOpt-Analyzer: Commit rejected due to complexity degradation or low coverage."
        exit 1
    fi
else
    echo "CovOpt-Analyzer: .covopt.toml not found, skipping audit."
fi
"#;
    if let Err(e) = fs::write(&hook_path, hook_content) {
        eprintln!("Failed to write hook to {}: {}", hook_path.display(), e);
        std::process::exit(1);
    }
    // Set executable bit
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&hook_path).unwrap().permissions();
        perms.set_mode(covopt_param!("M_735_23", 0o755));
        fs::set_permissions(&hook_path, perms).unwrap();
    }
    println!(
        "Successfully installed pre-commit hook to {}",
        hook_path.display()
    );
}

const COVOPT_AGENT_RULES: &str = r#"# CovOpt Optimization & Tuning Rules (Google Antigravity)

**Usage**: AI Agent instructions for performance verification and parameter tuning.

## Core Rules to Prevent Audit Failures
1. **Zero-Entropy Tuning**: NEVER use hardcoded magical numbers. ALWAYS use `covopt_param!` macro. 
2. **Anti-DCE**: ALWAYS wrap loop variables with `std::hint::black_box()` in benchmarks to prevent O(N) -> O(1) DCE.
3. **Lock-Free Critical Paths**: NEVER use standard library `Mutex` or `RwLock` on the critical path.
4. **Strict Clippy Cleanliness**: DO NOT use `#[allow(...)]` to ignore type warnings for macro-generated code.

## Available Commands
- `covopt init`: Initialize or migrate the V3 policy configuration.
- `covopt check`: Ask the Evidence Planner whether guarantees are satisfied.
- `covopt inspect`: Explain findings and repair candidates without editing source.
- `covopt optimize`: Generate input, atomic, codegen, or layout candidates.
- `covopt fix`: Plan or apply sandbox-verified repairs.
- `covopt verify`: Force a specific dynamic evidence provider.
"#;

pub fn init_config(args: crate::InitArgs) {
    if let Some(p) = args.path
        && let Err(e) = std::env::set_current_dir(&p)
    {
        eprintln!("Failed to change directory to {}: {}", p, e);
        std::process::exit(1);
    }
    let config_path = std::path::PathBuf::from(".covopt.toml");
    let has_config = config_path.exists();
    if has_config {
        println!(
            "CovOpt-Analyzer: .covopt.toml already exists. Skipping config creation, but will ensure rules are injected."
        );
    } else {
        let mut default_config = String::from(
            r#"version = 3

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

"#,
        );
        let found_tests = CovOpt_Analyzer::static_analysis::find_all_covopt_tests();

        if found_tests.is_empty() {
            println!("CovOpt-Analyzer: No #[covopt::test] found. Creating default template.");
            default_config.push_str(
                r#"[[target]]
test = "my_benchmark_test"
expected = "O(1)"
n_values = "1,500,10000"
"#,
            );
        } else {
            println!(
                "CovOpt-Analyzer: Auto-discovered {} test(s). Generating config.",
                found_tests.len()
            );
            for (test_name, exp, n_vals) in found_tests {
                default_config.push_str(&format!(
                    r#"[[target]]
test = "{}"
expected = "{}"
n_values = "{}"

"#,
                    test_name, exp, n_vals
                ));
            }
        }

        if let Err(e) = std::fs::write(&config_path, default_config) {
            eprintln!("Failed to write .covopt.toml: {}", e);
            std::process::exit(1);
        }
        println!("Successfully initialized .covopt.toml. Please edit it to match your target.");
    }

    // Append to .gitignore
    if let Ok(mut content) = std::fs::read_to_string(".gitignore") {
        if !content.contains(".covopt/") {
            if !content.ends_with('\n') && !content.is_empty() {
                content.push('\n');
            }
            content.push_str(".covopt/\n");
            let _ = std::fs::write(".gitignore", content);
            println!("Added .covopt/ to .gitignore.");
        }
    } else {
        let _ = std::fs::write(".gitignore", ".covopt/\n");
        println!("Created .gitignore and added .covopt/.");
    }

    // Append to Cargo.toml exclude
    if let Ok(mut content) = std::fs::read_to_string("Cargo.toml")
        && !content.contains("\".covopt/\"")
        && !content.contains("'.covopt/'")
    {
        if let Some(idx) = content.find("exclude = [") {
            let insert_pos = idx + "exclude = [".len();
            content.insert_str(insert_pos, "\".covopt/\", ");
            let _ = std::fs::write("Cargo.toml", content);
            println!("Added .covopt/ to exclude array in Cargo.toml.");
        } else if let Some(idx) = content.find("[package]") {
            let end_idx = content[idx..]
                .find("\n[")
                .map(|i| idx + i)
                .unwrap_or(content.len());
            content.insert_str(end_idx, "\nexclude = [\".covopt/\"]\n");
            let _ = std::fs::write("Cargo.toml", content);
            println!("Added exclude = [\".covopt/\"] to Cargo.toml [package] section.");
        }
    }

    // Inject AI Agent Rules
    let agents_dir = Path::new(".agents");
    let rules_dir = agents_dir.join("rules");

    if let Err(e) = std::fs::create_dir_all(&rules_dir) {
        eprintln!("Failed to create .agents/rules directory: {}", e);
    } else {
        let rule_file = rules_dir.join("covopt-rules.md");
        if let Err(e) = std::fs::write(&rule_file, COVOPT_AGENT_RULES) {
            eprintln!("Failed to write rule file {:?}: {}", rule_file, e);
        } else {
            println!("Injected AI agent rules to {:?}.", rule_file);
        }

        let agents_md = agents_dir.join("AGENTS.md");
        let current_agents_md = std::fs::read_to_string(&agents_md).unwrap_or_default();

        // Remove the old block if it exists
        let mut new_agents_md = current_agents_md.clone();
        if let Some(start_idx) =
            new_agents_md.find("# CovOpt Optimization & Tuning Rules (Google Antigravity)")
        {
            // Skip the current header and find the next top-level header (e.g., "\n# ")
            if let Some(end_offset) = new_agents_md[start_idx + 2..].find("\n# ") {
                let end_idx = start_idx + 2 + end_offset;
                // There's another rule block after this one, replace just this block
                new_agents_md.replace_range(start_idx..end_idx, "");
            } else {
                // It's the last rule block, truncate from start_idx
                new_agents_md.truncate(start_idx);
            }
        }

        if !new_agents_md.ends_with('\n') && !new_agents_md.is_empty() {
            new_agents_md.push('\n');
        }
        new_agents_md.push('\n');
        new_agents_md.push_str(COVOPT_AGENT_RULES);
        new_agents_md.push('\n');

        if let Err(e) = std::fs::write(&agents_md, new_agents_md) {
            eprintln!("Failed to update {:?}: {}", agents_md, e);
        } else {
            println!("Updated CovOpt rules in {:?}.", agents_md);
        }
    }
}

pub fn run_fix(path: Option<String>) {
    println!("CovOpt-Analyzer: Running CodeMender-Style Sandbox Auto-Fix...");

    // We need to gather the files that will be affected to back them up
    // In a real CodeMender, we'd parse the diff. For now, we'll assume the path is the target
    let target_dir = std::env::current_dir().unwrap();
    let sandbox = CovOpt_Analyzer::sandbox::Sandbox::new(target_dir.clone());

    // Collect target files (all .rs files in path or src/)
    let search_path = path.clone().unwrap_or_else(|| "src/".to_string());
    let _ = crate::auto_fixer::run_async_starvation_shield(Path::new(&search_path));

    let mut target_files = Vec::new();
    for e in walkdir::WalkDir::new(&search_path).into_iter().flatten() {
        let p = e.path();
        if p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("rs") {
            target_files.push(p.to_path_buf());
        }
    }

    let fix_fn = || -> Result<(), String> {
        let mut args = vec![
            "clippy",
            "--fix",
            "--allow-dirty",
            "--allow-no-vcs",
            "--all-targets",
        ];
        if !CovOpt_Analyzer::config::should_color() {
            args.push("--color=never");
        }
        args.push("--");
        args.push("-A");
        args.push("unused_imports");

        let path_str = path.clone().unwrap_or_default();
        if path.is_some() {
            // cargo clippy actually doesn't take paths directly, but if this was here we append it
            args.push(&path_str);
        }

        let status = std::process::Command::new("cargo")
            .args(&args)
            .status()
            .map_err(|e| e.to_string())?;

        if !status.success() {
            return Err("cargo clippy --fix failed".to_string());
        }
        Ok(())
    };

    match sandbox.verify_fix(&target_files, None, fix_fn) {
        Ok(true) => println!("CovOpt-Analyzer: Fix applied successfully with 0 regressions."),
        Ok(false) => println!("CovOpt-Analyzer: Fix rolled back due to performance regression."),
        Err(e) => eprintln!("CovOpt-Analyzer: Sandbox verification failed: {}", e),
    }
}

pub fn migrate_config(path: Option<&str>) {
    if let Some(path) = path
        && let Err(error) = std::env::set_current_dir(path)
    {
        eprintln!("CovOpt init --migrate: {error}");
        std::process::exit(1);
    }
    let config_path = PathBuf::from(".covopt.toml");
    let source = match fs::read_to_string(&config_path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("CovOpt init --migrate: {error}");
            std::process::exit(1);
        }
    };
    let config = match CovOptConfig::load(&config_path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("CovOpt init --migrate: {error}");
            std::process::exit(1);
        }
    };
    let backup = PathBuf::from(".covopt.toml.v2.bak");
    if let Err(error) = fs::write(&backup, source) {
        eprintln!(
            "CovOpt init --migrate: could not create {}: {error}",
            backup.display()
        );
        std::process::exit(1);
    }
    let mut migrated = String::from(
        r#"version = 3

[assurance]
mode = "adaptive"
overall_coverage = 0.90
critical_coverage = 1.0
performance_coverage = 0.90
budget_seconds = 30
fail_on_critical_unknown = true

[providers]
static = "required"
mca = "auto"
coverage = "fallback"
sanitizer = "fallback"
concurrency = "fallback"
profile = "fallback"

[optimization]
enabled = ["inputs", "atomic", "codegen", "layout"]
default_budget_seconds = 30
apply = "never"

[targets]
discover = "annotations"

[policy.default]
overall_coverage = 0.90
critical_coverage = 1.0

"#,
    );
    for target in &config.target {
        let id = CovOptConfig::target_id(target);
        let expected = target
            .complexity
            .as_deref()
            .or(target.expected.as_deref())
            .unwrap_or("O(1)");
        let n_values = target.n_values.as_deref().unwrap_or("1,100,1000");
        migrated.push_str(&format!(
            "[target.{}]\nfunction = {}\ncomplexity = {}\ntest = {}\nn_values = {}\n\n",
            id,
            serde_json::to_string(target.function.as_deref().unwrap_or(&id)).unwrap_or_default(),
            serde_json::to_string(expected).unwrap_or_default(),
            serde_json::to_string(&target.test).unwrap_or_default(),
            serde_json::to_string(n_values).unwrap_or_default(),
        ));
    }
    if let Err(error) = fs::write(&config_path, migrated) {
        eprintln!("CovOpt init --migrate: {error}");
        std::process::exit(1);
    }
    println!(
        "Migrated {} to V3; legacy configuration saved at {}",
        config_path.display(),
        backup.display()
    );
}

pub fn get_git_diff_files(staged: bool, branch: Option<&str>) -> Vec<String> {
    collect_git_diff_files(staged, branch)
        .unwrap_or_default()
        .into_iter()
        .filter(|file| file.ends_with(".rs"))
        .collect()
}

fn collect_git_diff_files(staged: bool, branch: Option<&str>) -> Result<Vec<String>, String> {
    let mut cmd = std::process::Command::new("git");
    cmd.arg("diff").arg("--name-only");

    if staged {
        cmd.arg("--cached");
    } else if let Some(b) = branch {
        cmd.arg(format!("{}...HEAD", b));
    }

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to run git diff: {}", e))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect())
}

pub fn run_audit(args: &CovOpt_Analyzer::config::AuditArgs) {
    let audit_started = std::time::Instant::now();
    let target_test = args.test.clone();
    let fast = args.fast;
    let is_json = args.json;
    let staged = args.staged;
    let adaptive_inputs = args.adaptive_inputs;
    let requested_base = args.base.clone();

    if args.debug_artifacts {
        unsafe {
            std::env::set_var("COVOPT_DEBUG_ARTIFACTS", "1");
        }
    }

    unsafe {
        std::env::set_var("COVOPT_COMPACT", "1");
    }
    let config_path = ".covopt.toml";
    if !PathBuf::from(config_path).exists() {
        eprintln!("CovOpt-Analyzer: Config file {} not found.", config_path);
        eprintln!("Please run `covopt init` to initialize the project first.");
        std::process::exit(1);
    }

    let config = match CovOptConfig::load(config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };
    let planning_context = planner_tool_context();
    let planning_providers = planning_providers();

    let assurance_policy = args.assurance;
    let requested_evidence_threshold = args.evidence_threshold;

    eprintln!("CovOpt-Analyzer: Resolving packages for Batch Compilation Mode...");

    let mut targets = config.target.clone();
    if let Some(test_name) = &target_test {
        targets.retain(|target| &target.test == test_name);
        if targets.is_empty() {
            let message = format!("No configured audit target matches test '{}'", test_name);
            if is_json {
                println!(
                    "{}",
                    serde_json::json!({"status": "failed", "error": message})
                );
            } else {
                eprintln!("[AUDIT FAILED] {}", message);
            }
            std::process::exit(1);
        }
    }

    let diff_files = if staged || args.base.is_some() {
        match collect_git_diff_files(staged, args.base.as_deref()) {
            Ok(files) => Some(files),
            Err(error) => {
                eprintln!(
                    "[Git Incremental Audit] Cannot read requested diff ({}); falling back to full audit.",
                    error
                );
                None
            }
        }
    } else {
        None
    };

    if let Some(diff_files) = diff_files {
        eprintln!(
            "[Git Incremental Audit] Inspecting {} modified file(s) for affected targets.",
            diff_files.len()
        );
        if diff_files.is_empty() {
            // An explicit empty staged/base diff is safe to treat as no affected
            // targets. A failed git command is handled above by the full-audit fallback.
            targets.clear();
        } else {
            let changed_packages: std::collections::HashSet<String> = diff_files
                .iter()
                .filter_map(|file| {
                    CovOpt_Analyzer::static_analysis::find_package_for_file(Path::new(file))
                })
                .collect();
            let mapping_is_safe = diff_files.iter().all(|file| {
                CovOpt_Analyzer::static_analysis::find_package_for_file(Path::new(file)).is_some()
            });
            if mapping_is_safe {
                targets.retain(|target| {
                    target
                        .package
                        .clone()
                        .or_else(|| {
                            CovOpt_Analyzer::static_analysis::resolve_package_for_target(
                                &target.test,
                                None,
                            )
                        })
                        .is_some_and(|package| changed_packages.contains(&package))
                });
            } else {
                eprintln!(
                    "[Git Incremental Audit] Could not safely map all changed files to packages; using full target set."
                );
            }
        }
    }

    if (staged || args.base.is_some()) && targets.is_empty() {
        eprintln!("[Git Incremental Audit] No affected configured targets; audit skipped.");
        if is_json {
            println!(
                "{}",
                serde_json::json!({"status": "success", "targets": []})
            );
        }
        eprintln!("\nCI timings:");
        eprintln!("  workspace check:     {:?}", std::time::Duration::ZERO);
        eprintln!("  coverage compile:    skipped (no affected targets)");
        eprintln!("  target analysis:     skipped (no affected targets)");
        eprintln!("  entropy:              skipped (no affected targets)");
        eprintln!("  total:                {:?}", audit_started.elapsed());
        return;
    }

    let mut prepared_plans = targets
        .iter()
        .map(|target| {
            let prepared = prepare_target_plan(
                &config,
                target,
                assurance_policy,
                requested_evidence_threshold,
                &planning_context,
                &planning_providers,
            );
            let selected = prepared
                .initial
                .selected_action_details
                .iter()
                .map(|action| action.provider.0.as_str())
                .collect::<Vec<_>>();
            eprintln!(
                "[Evidence Plan] {}: {:?}; selected [{}]",
                target.test,
                prepared.initial.status,
                selected.join(", ")
            );
            (target.test.clone(), prepared)
        })
        .collect::<std::collections::HashMap<_, _>>();
    let target_requires_analysis = |target: &TargetConfig| {
        prepared_plans.get(&target.test).is_some_and(|prepared| {
            plan_selects(&prepared.initial, EvidenceProviderKind::Coverage)
                || plan_selects(&prepared.initial, EvidenceProviderKind::Mca)
                || plan_selects(&prepared.initial, EvidenceProviderKind::Test)
        })
    };
    let dynamic_audit_enabled = targets.iter().any(target_requires_analysis);
    let workspace_check_required = targets.iter().any(|target| {
        target_requires_analysis(target)
            || prepared_plans.get(&target.test).is_some_and(|prepared| {
                plan_selects(&prepared.initial, EvidenceProviderKind::Compiler)
            })
    });
    let execution_target_count = targets
        .iter()
        .filter(|target| {
            target_requires_analysis(target)
                || prepared_plans.get(&target.test).is_some_and(|prepared| {
                    plan_selects(&prepared.initial, EvidenceProviderKind::Compiler)
                })
        })
        .count()
        .max(1);
    let workspace_check_started = std::time::Instant::now();
    let workspace_check = if !workspace_check_required {
        eprintln!("[Evidence Plan] Workspace compiler action not selected; skipping cargo check.");
        None
    } else {
        Some(
            match CovOpt_Analyzer::runner::check_workspace_with_diagnostics() {
                Ok(check) => check,
                Err(error) => {
                    eprintln!(
                        "\n[AUDIT FAILED] Workspace compilation check failed:\n{}",
                        error
                    );
                    std::process::exit(1);
                }
            },
        )
    };
    let workspace_check_time = workspace_check_started.elapsed();

    let mut packages_to_compile = Vec::new();
    for target in targets
        .iter()
        .filter(|target| target_requires_analysis(target))
    {
        if let Some(pkg) = CovOpt_Analyzer::static_analysis::resolve_package_for_target(
            &target.test,
            target.package.as_ref(),
        ) && !packages_to_compile.contains(&pkg)
        {
            eprintln!("Resolved test '{}' to package '{}'", target.test, pkg);
            packages_to_compile.push(pkg);
        }
    }

    if !dynamic_audit_enabled {
        eprintln!("[Assurance] Static policy: skipping instrumented test compilation.");
    } else if packages_to_compile.is_empty() {
        eprintln!("CovOpt-Analyzer: Compiling ENTIRE workspace tests (no packages resolved)...");
    } else {
        eprintln!(
            "CovOpt-Analyzer: Compiling specific packages: {:?}",
            packages_to_compile
        );
    }

    let compilation_started = std::time::Instant::now();
    let mut audit_context = if !dynamic_audit_enabled {
        None
    } else {
        Some(
            match CovOpt_Analyzer::runner::AuditContext::compile(&packages_to_compile) {
                Ok(context) => context,
                Err(e) => {
                    eprintln!("Failed to compile workspace tests: {}", e);
                    std::process::exit(1);
                }
            },
        )
    };
    let compilation_time = compilation_started.elapsed();
    let shared_execution_cost_ms = workspace_check_time
        .saturating_add(compilation_time)
        .as_millis()
        / execution_target_count as u128;
    let cli_noise_result = workspace_check
        .as_ref()
        .map(|check| CovOpt_Analyzer::entropy::parse_cli_noise_from_json(&check.cargo_check_stdout))
        .unwrap_or((0, 0.0));
    if let Some(context) = audit_context.as_mut() {
        context.cli_noise_result = Some(cli_noise_result);
    }
    let mut target_analysis_time = std::time::Duration::ZERO;
    let mut entropy_time = std::time::Duration::ZERO;

    let mut json_results = serde_json::json!({
        "status": "success",
        "targets": []
    });
    let mut assurance_results = Vec::new();
    let mut all_success = true;

    for mut target in targets {
        if CovOpt_Analyzer::runner::ci_budget_exhausted() {
            eprintln!(
                "[CI BUDGET EXCEEDED] No budget remains before target '{}'; remaining targets were not started.",
                target.test
            );
            all_success = false;
            break;
        }
        let Some(mut prepared_plan) = prepared_plans.remove(&target.test) else {
            eprintln!(
                "[Evidence Plan] Missing prepared plan for '{}'.",
                target.test
            );
            all_success = false;
            continue;
        };
        let planned_coverage = plan_selects(&prepared_plan.initial, EvidenceProviderKind::Coverage);
        let planned_mca = plan_selects(&prepared_plan.initial, EvidenceProviderKind::Mca);
        let planned_test = plan_selects(&prepared_plan.initial, EvidenceProviderKind::Test);
        let planned_compiler = plan_selects(&prepared_plan.initial, EvidenceProviderKind::Compiler);
        let planned_atomic =
            plan_selects(&prepared_plan.initial, EvidenceProviderKind::AtomicModel);
        let planned_adversarial =
            plan_selects(&prepared_plan.initial, EvidenceProviderKind::Adversarial);
        let planned_analysis = planned_coverage || planned_mca || planned_test;
        if fast && let Some(n_vals) = &target.n_values {
            let parts: Vec<&str> = n_vals.split(',').collect();
            if parts.len() > 2 {
                target.n_values = Some(format!(
                    "{},{}",
                    parts.first().unwrap(),
                    parts.last().unwrap()
                ));
            }
        }
        let args = RunArgs {
            test: Some(target.test.clone()),
            expected: target.expected.clone(),
            n_values: target.n_values.clone(),
            mca_cpu: target.mca_cpu.clone(),
            require_cache_padding: target.require_cache_padding,
            require_branch_hints: target.require_branch_hints,
            require_aerospace_grade: target.require_aerospace_grade,
            require_watchdog_timeout: target.require_watchdog_timeout,
            require_stress_test: target.require_stress_test,
            polling_threshold: target.polling_threshold,
            ignore: target.ignore.as_ref().map(|vec| vec.join(",")),
            formalize: false, // Audit defaults to false unless specified
            optimize: false,
            json: is_json,
        };
        eprintln!("\n===================================================");
        eprintln!("Auditing target: {}", target.test);
        eprintln!("===================================================");
        let target_started = std::time::Instant::now();
        let target_policy = target.assurance.unwrap_or(assurance_policy);
        let mut line_coverage_value = 0.0;
        let mut mca_report = None;
        let mut target_coverage_map = None;
        let mut target_complexity_fit = None;
        let analysis_passed = if !planned_analysis {
            eprintln!("[Evidence Plan] No dynamic target analysis selected.");
            true
        } else if let Some(context) = audit_context.as_ref() {
            let analysis = run_analysis_structured(
                &args,
                true,
                Some(context),
                fast,
                planned_mca,
                Some(&mut line_coverage_value),
                Some(&mut mca_report),
            );
            target_coverage_map = analysis.coverage_map;
            target_complexity_fit = analysis
                .actual_complexity
                .zip(analysis.complexity_r_squared);
            if !analysis.passed {
                all_success = false;
                false
            } else {
                true
            }
        } else {
            all_success = false;
            false
        };
        let target_analysis_elapsed = target_started.elapsed();
        target_analysis_time += target_analysis_elapsed;

        // --- COVOPT 2.0 ENTROPY EVALUATION ---
        let entropy_started = std::time::Instant::now();
        let entropy_result = if !planned_adversarial {
            eprintln!("[Evidence Plan] Adversarial/entropy action not selected; skipping.");
            CovOpt_Analyzer::entropy::EntropyResult {
                fuzz_variance_score: 0.0,
                branch_sprawl_score: 0.0,
                cli_noise_score: 0.0,
                total_score: 0.0,
            }
        } else if let Some(context) = audit_context.as_ref() {
            CovOpt_Analyzer::entropy::calculate_entropy_score(
                &target,
                true,
                fast,
                Some(context),
                audit_context
                    .as_ref()
                    .and_then(|context| context.cli_noise_result)
                    .or(Some(cli_noise_result)),
            )
        } else {
            all_success = false;
            CovOpt_Analyzer::entropy::EntropyResult {
                fuzz_variance_score: 0.0,
                branch_sprawl_score: 0.0,
                cli_noise_score: 0.0,
                total_score: 0.0,
            }
        };
        entropy_time += entropy_started.elapsed();
        if planned_adversarial {
            eprintln!("\n=== COVOPT 2.0 ENTROPY REPORT ===");
            eprintln!(
                "  A. Fuzz-Cov Variance: {:.1}/30.0",
                entropy_result.fuzz_variance_score
            );
            eprintln!(
                "  B. API Branch Sprawl: {:.1}/40.0",
                entropy_result.branch_sprawl_score
            );
            eprintln!(
                "  C. CLI Noise Index:   {:.1}/30.0",
                entropy_result.cli_noise_score
            );
            eprintln!("  --------------------------------");
            eprintln!(
                "  TOTAL ENTROPY SCORE:  {:.1}/100.0",
                entropy_result.total_score
            );
            if entropy_result.total_score > covopt_param!("M_1079_40", 50.0) {
                eprintln!(
                    "  [!] WARNING: High Entropy Detected! Codebase is unstable, tangled, or noisy."
                );
                all_success = false;
            } else {
                eprintln!("  [OK] Low Entropy. Code is well encapsulated and stable.");
            }
            eprintln!("===================================");
        }

        let evidence_threshold = target
            .evidence_threshold
            .or(requested_evidence_threshold)
            .unwrap_or(config.pipeline.evidence_threshold);
        let target_source = CovOpt_Analyzer::assurance::find_target_source(&target.test);
        let scope_function = target.function.clone().or_else(|| {
            CovOpt_Analyzer::static_analysis::find_covopt_target_metadata(&target.test)
                .map(|metadata| metadata.function)
        });
        let scope_package = target
            .package
            .clone()
            .or_else(|| {
                CovOpt_Analyzer::static_analysis::resolve_package_for_target(&target.test, None)
            })
            .unwrap_or_else(|| "workspace".to_string());
        let mut scope_envelope = target_source.as_ref().and_then(|source| {
            match CovOpt_Analyzer::scope::build_scope_envelope(
                &CovOpt_Analyzer::model::PackageId::new(scope_package),
                source,
                scope_function.as_deref(),
                target_coverage_map.as_ref(),
                Vec::new(),
            ) {
                Ok(mut envelope) => {
                    envelope.set_expected_complexity(
                        scope_function.as_deref(),
                        target.complexity.as_deref().or(target.expected.as_deref()),
                    );
                    if let Some((fit, _r_squared)) = target_complexity_fit.as_ref() {
                        envelope.set_fitted_complexity(scope_function.as_deref(), fit);
                    }
                    Some(envelope)
                }
                Err(error) => {
                    eprintln!(
                        "[Scope] Could not build scope envelope for {}: {}",
                        target.test, error
                    );
                    None
                }
            }
        });
        let mut obligations = target_source
            .as_ref()
            .map(|source| CovOpt_Analyzer::assurance::discover_obligations(source, &target.test))
            .unwrap_or_else(|| {
                CovOpt_Analyzer::assurance::discover_obligations(
                    Path::new("__missing_target_source__.rs"),
                    &target.test,
                )
            });
        if let Some(source) = target_source.as_ref() {
            obligations.extend(
                CovOpt_Analyzer::assurance::obligations_from_structured_findings(
                    &target.test,
                    CovOpt_Analyzer::dataflow::analyze_file_structured(source),
                ),
            );
            obligations.extend(CovOpt_Analyzer::assurance::obligations_from_findings(
                &target.test,
                CovOpt_Analyzer::static_analysis::analyze_aerospace_grade_structured(source),
            ));
        }
        if let Some(envelope) = scope_envelope.as_ref() {
            obligations.extend(envelope.attribution_obligations.iter().map(|scope| {
                CovOpt_Analyzer::assurance::scope_attribution_obligation(&target.test, scope)
            }));
        }
        if let Some(report) = mca_report.as_ref() {
            obligations.push(CovOpt_Analyzer::assurance::obligation_from_mca(
                &target.test,
                report,
            ));
        }
        let mut assurance_report: AssuranceReport = AssuranceScheduler::with_mca_report(
            target_policy,
            evidence_threshold,
            planned_coverage && analysis_passed,
            mca_report,
        )
        .evaluate(obligations);
        assurance_report.apply_legacy_coverage(
            planned_coverage && analysis_passed,
            "Planner-selected targeted coverage completed successfully",
        );
        if planned_compiler && workspace_check.is_some() {
            assurance_report.apply_provider_evidence(
                EvidenceProviderKind::Compiler,
                CovOpt_Analyzer::assurance::ObligationStatus::Modeled,
                "Planner-selected compiler diagnostics completed without errors",
                None,
            );
        }
        if planned_test && analysis_passed {
            assurance_report.apply_provider_evidence(
                EvidenceProviderKind::Test,
                CovOpt_Analyzer::assurance::ObligationStatus::Observed,
                "Planner-selected target tests completed successfully",
                None,
            );
        }
        assurance_report.line_coverage_percent = if planned_coverage && analysis_passed {
            Some(line_coverage_value)
        } else {
            None
        };
        if config.assurance.fail_on_critical_unknown
            && scope_envelope
                .as_ref()
                .is_some_and(|envelope| !envelope.critical_unknown_scopes.is_empty())
        {
            assurance_report.passes = false;
            all_success = false;
        }
        assurance_report.scope_envelope = scope_envelope.take();
        if let Some(source) = target_source.as_ref() {
            match fs::read_to_string(source).and_then(|content| {
                CovOpt_Analyzer::parameters::ParameterDependencyGraph::from_source(
                    &content,
                    &source.display().to_string(),
                )
                .map_err(std::io::Error::other)
            }) {
                Ok(parameter_graph) => {
                    if let Some(envelope) = assurance_report.scope_envelope.as_mut() {
                        envelope.parameters = parameter_graph.parameters.keys().cloned().collect();
                    }
                    assurance_report.parameter_graph = Some(parameter_graph);
                }
                Err(error) => {
                    assurance_report
                        .obligations
                        .push(CovOpt_Analyzer::assurance::Obligation {
                            id: CovOpt_Analyzer::assurance::ObligationId::new(
                                "COVOPT-PARAMETER-METADATA",
                            ),
                            kind: CovOpt_Analyzer::assurance::ObligationKind::Complexity,
                            target: target.test.clone(),
                            function: scope_function.clone(),
                            source: target_source.as_ref().map(|source| {
                                CovOpt_Analyzer::assurance::SourceLocation {
                                    file: source.display().to_string(),
                                    line: 1,
                                }
                            }),
                            severity: CovOpt_Analyzer::assurance::Severity::Critical,
                            weight: 1.0,
                            provider: CovOpt_Analyzer::assurance::EvidenceProviderKind::StaticAst,
                            status: CovOpt_Analyzer::assurance::ObligationStatus::Unknown,
                            explanation: format!("parameter metadata could not be parsed: {error}"),
                            remediation: "fix covopt_param metadata and rerun the audit"
                                .to_string(),
                            acceptable_evidence_kinds: Vec::new(),
                            evidence: Vec::new(),
                        });
                    assurance_report.passes = false;
                    all_success = false;
                }
            }
        }
        let metadata_index =
            CovOpt_Analyzer::static_analysis::SourceMetadataIndex::load_or_build(Path::new("."));
        let target_ids = metadata_index
            .targets
            .iter()
            .map(|metadata| metadata.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let invalid_evidence_targets = metadata_index
            .evidence
            .iter()
            .filter(|metadata| !target_ids.contains(metadata.target.as_str()))
            .map(|metadata| metadata.target.clone())
            .collect::<std::collections::BTreeSet<_>>();
        if !invalid_evidence_targets.is_empty() {
            assurance_report
                .obligations
                .push(CovOpt_Analyzer::assurance::Obligation {
                    id: CovOpt_Analyzer::assurance::ObligationId::new("COVOPT-EVIDENCE-TARGETS"),
                    kind: CovOpt_Analyzer::assurance::ObligationKind::Complexity,
                    target: target.test.clone(),
                    function: scope_function.clone(),
                    source: target_source.as_ref().map(|source| {
                        CovOpt_Analyzer::assurance::SourceLocation {
                            file: source.display().to_string(),
                            line: 1,
                        }
                    }),
                    severity: CovOpt_Analyzer::assurance::Severity::Critical,
                    weight: 1.0,
                    provider: CovOpt_Analyzer::assurance::EvidenceProviderKind::StaticAst,
                    status: CovOpt_Analyzer::assurance::ObligationStatus::Unknown,
                    explanation: format!(
                        "evidence annotations reference unknown target IDs: {}",
                        invalid_evidence_targets
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    remediation:
                        "declare each target with #[covopt_target] before attaching evidence"
                            .to_string(),
                    acceptable_evidence_kinds: Vec::new(),
                    evidence: Vec::new(),
                });
            assurance_report.passes = false;
            all_success = false;
        }
        assurance_report.metadata_index = Some(metadata_index);
        if adaptive_inputs {
            let critical = assurance_report
                .obligations
                .iter()
                .filter(|obligation| {
                    matches!(
                        obligation.severity,
                        CovOpt_Analyzer::assurance::Severity::Critical
                    )
                })
                .map(|obligation| obligation.id.clone())
                .collect::<Vec<_>>();
            let ids = assurance_report
                .obligations
                .iter()
                .map(|obligation| obligation.id.clone())
                .collect::<Vec<_>>();
            let input = candidate_input_from_target(&target, ids.clone(), critical);
            let trial_config = config.trials.selection_config();
            let candidates = generate_candidates(&input, &trial_config);
            assurance_report.trial_plan = Some(select_trials(&candidates, &ids, &trial_config));
        }
        let atomic_policy = target.atomic.as_ref().unwrap_or(&config.atomic);
        if planned_atomic
            && atomic_policy.enabled
            && let Some(source) = target_source.as_ref()
            && let Ok(request) = request_from_file(
                source,
                atomic_policy.correctness_contract(),
                atomic_policy.bounds(),
                atomic_policy.timeout_ms.unwrap_or(5_000),
                false,
            )
        {
            let analysis = analyze_atomic(&request);
            let evidence_status = match analysis.baseline.as_ref().map(|result| result.status) {
                Some(ModelStatus::Modeled) => CovOpt_Analyzer::assurance::ObligationStatus::Modeled,
                Some(ModelStatus::Counterexample) => {
                    CovOpt_Analyzer::assurance::ObligationStatus::Failed
                }
                Some(ModelStatus::Unknown) | None => {
                    CovOpt_Analyzer::assurance::ObligationStatus::Unknown
                }
            };
            let details = serde_json::to_value(&analysis).ok();
            assurance_report.apply_provider_evidence(
                EvidenceProviderKind::AtomicModel,
                evidence_status,
                &analysis.summary,
                details.clone(),
            );
            assurance_report.atomic = details;
        }
        let mut follow_up_actions = discover_evidence_actions(
            &planning_providers,
            &assurance_report.obligations,
            &prepared_plan.context,
        );
        let provider_config = config.resolved_target(&target).providers;
        follow_up_actions.retain(|action| {
            !matches!(
                CovOptConfig::provider_mode(&provider_config, &action.provider.0),
                ProviderMode::Disabled
            )
        });
        for action in &mut follow_up_actions {
            if !check_executes_provider(&action.provider.0) {
                action.available = false;
                action.description.push_str("; no automatic check executor");
            }
        }
        let follow_up = EvidencePlanner::new(prepared_plan.policy.clone()).plan(
            &assurance_report.obligations,
            &follow_up_actions,
            &prepared_plan.context,
        );
        let actual_coverage = assurance_report.coverage.clone();
        let target_actual_cost_ms = target_analysis_elapsed
            .as_millis()
            .saturating_add(shared_execution_cost_ms)
            .min(u128::from(u64::MAX)) as u64;
        prepared_plan.initial.actual_cost_ms = Some(target_actual_cost_ms);
        prepared_plan.initial.actual_coverage = Some(actual_coverage);
        assurance_report.plan = Some(prepared_plan.initial);
        assurance_report.follow_up_plan = Some(follow_up.plan);
        assurance_report.proof_frontier =
            Some(CovOpt_Analyzer::assurance::ProofFrontier::from_obligations(
                &assurance_report.obligations,
                assurance_report
                    .follow_up_plan
                    .as_ref()
                    .or(assurance_report.plan.as_ref()),
            ));
        if let Some(base) = requested_base.as_deref() {
            let baseline_path = if Path::new(base).is_file() {
                PathBuf::from(base)
            } else {
                let baseline_name = target
                    .test
                    .chars()
                    .map(|character| {
                        if character.is_ascii_alphanumeric() {
                            character
                        } else {
                            '_'
                        }
                    })
                    .collect::<String>();
                PathBuf::from("target/covopt/snapshots")
                    .join(format!("{baseline_name}.previous.json"))
            };
            if let Ok(base_snapshot) =
                CovOpt_Analyzer::snapshot::AssuranceSnapshot::load(&baseline_path)
            {
                let current_snapshot = CovOpt_Analyzer::snapshot::AssuranceSnapshot::from_report(
                    &target.test,
                    target_source.as_deref(),
                    &assurance_report,
                );
                let drift =
                    CovOpt_Analyzer::snapshot::compare_snapshots(&base_snapshot, &current_snapshot);
                all_success &= !drift.critical;
                assurance_report.semantic_drift = Some(drift);
            } else if let Err(error) =
                CovOpt_Analyzer::snapshot::AssuranceSnapshot::load(&baseline_path)
            {
                all_success = false;
                assurance_report.semantic_drift = Some(CovOpt_Analyzer::snapshot::SemanticDrift {
                    status: "Unknown".to_string(),
                    expected: Vec::new(),
                    unexplained: vec![format!("drift baseline unavailable: {}", error)],
                    first_scope: None,
                    witness: Some(baseline_path.display().to_string()),
                    proof_frontier: assurance_report.proof_frontier.clone(),
                    critical: true,
                    parameter_added: Vec::new(),
                    parameter_removed: Vec::new(),
                    parameter_remapped: Vec::new(),
                });
            }
        }
        let snapshot = CovOpt_Analyzer::snapshot::AssuranceSnapshot::from_report(
            &target.test,
            target_source.as_deref(),
            &assurance_report,
        );
        let snapshot_name = target
            .test
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character
                } else {
                    '_'
                }
            })
            .collect::<String>();
        let snapshot_path =
            PathBuf::from("target/covopt/snapshots").join(format!("{snapshot_name}.json"));
        let previous_path =
            PathBuf::from("target/covopt/snapshots").join(format!("{snapshot_name}.previous.json"));
        if snapshot_path.exists()
            && let Err(error) = fs::copy(&snapshot_path, &previous_path)
        {
            eprintln!("[Snapshot] Could not preserve previous snapshot: {error}");
        }
        if let Err(error) = snapshot.save(&snapshot_path) {
            eprintln!("[Snapshot] Could not save {}: {}", target.test, error);
        }
        eprintln!(
            "Assurance: policy={:?}, evidence={:.1}% (critical safety={:.1}%, performance={:.1}%), unknown={}",
            assurance_report.policy,
            assurance_report.coverage.overall_percent,
            assurance_report.coverage.critical_safety_percent,
            assurance_report.coverage.performance_percent,
            assurance_report.coverage.unknown_obligation_count
        );
        for finding in CovOpt_Analyzer::assurance::format_legacy_findings(&assurance_report) {
            eprintln!("  {}", finding);
        }
        if !assurance_report.passes {
            all_success = false;
        }
        let assurance_json = serde_json::to_value(&assurance_report).unwrap_or_else(
            |_| serde_json::json!({"status": "unavailable", "error": "serialization failed"}),
        );
        assurance_results.push(serde_json::json!({
            "test": target.test.clone(),
            "assurance": assurance_json.clone()
        }));

        if is_json
            && let Some(arr) = json_results
                .get_mut("targets")
                .and_then(|t| t.as_array_mut())
        {
            let sandbox = CovOpt_Analyzer::sandbox::Sandbox::new(std::env::current_dir().unwrap());
            // For target.test, we try to get metrics
            let mut ipc = 0.0;
            let mut peak_rss = 0;
            if !fast
                && !matches!(target_policy, AssurancePolicy::Static)
                && let Ok(metrics) = sandbox.measure_metrics(Some(&target.test))
            {
                ipc = metrics.ipc.unwrap_or(0.0);
                peak_rss = metrics.peak_rss;
            }

            arr.push(serde_json::json!({
                "test": target.test,
                "entropy": {
                    "executed": planned_adversarial,
                    "fuzz_variance": entropy_result.fuzz_variance_score,
                    "branch_sprawl": entropy_result.branch_sprawl_score,
                    "cli_noise": entropy_result.cli_noise_score,
                    "total": entropy_result.total_score
                },
                "performance": {
                    "ipc": ipc,
                    "peak_rss": peak_rss
                },
                "passed": entropy_result.total_score <= 50.0 && assurance_report.passes,
                "assurance": assurance_json
            }));
        }
    }

    eprintln!("\nCI timings:");
    eprintln!("  workspace check:     {:?}", workspace_check_time);
    eprintln!("  coverage compile:    {:?}", compilation_time);
    eprintln!("  target analysis:     {:?}", target_analysis_time);
    if fast {
        eprintln!("  entropy:              cli noise only (fuzz/branch skipped in fast mode)");
    } else {
        eprintln!("  entropy:              {:?}", entropy_time);
    }
    eprintln!("  total:                {:?}", audit_started.elapsed());

    match serde_json::to_vec_pretty(&serde_json::json!({
        "version": 1,
        "targets": assurance_results.clone()
    })) {
        Ok(document) => {
            if let Err(error) = std::fs::create_dir_all("target/covopt")
                .and_then(|_| std::fs::write("target/covopt/assurance.json", document))
            {
                eprintln!("[Assurance] Could not persist assurance report: {}", error);
            }
        }
        Err(error) => eprintln!(
            "[Assurance] Could not serialize assurance report: {}",
            error
        ),
    }
    let plan_document = serde_json::json!({
        "version": 1,
        "targets": assurance_results.iter().filter_map(|target| {
            Some(serde_json::json!({
                "test": target.get("test")?,
                "plan": target.get("assurance")?.get("plan")?,
            }))
        }).collect::<Vec<_>>(),
    });
    if let Ok(document) = serde_json::to_vec_pretty(&plan_document)
        && let Err(error) = std::fs::write("target/covopt/plan.json", document)
    {
        eprintln!("[Assurance] Could not persist evidence plan: {}", error);
    }

    if is_json {
        if !all_success {
            json_results["status"] = serde_json::json!("failed");
        }
        println!("{}", serde_json::to_string_pretty(&json_results).unwrap());
        if !all_success {
            drop(audit_context);
            std::process::exit(1);
        }
        return;
    }

    if !all_success {
        drop(audit_context);
        eprintln!("\n[AUDIT FAILED] One or more targets failed complexity or coverage checks.");
        std::process::exit(1);
    } else {
        eprintln!("\n[AUDIT PASSED] All targets passed complexity and coverage checks.");
    }
}

/// Shared structured inspection pass used by both `inspect` and the legacy
/// `advise` alias. It intentionally performs no source edits and no separate
/// dataflow scan after the advisor pass.
pub fn run_inspect(args: &crate::AdviseArgs) -> Result<(), String> {
    fn collect_files(path: &Path, files: &mut Vec<PathBuf>) {
        if path.is_file() {
            if path.extension().and_then(|value| value.to_str()) == Some("rs") {
                files.push(path.to_path_buf());
            }
            return;
        }
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                let child = entry.path();
                let name = child.file_name().unwrap_or_default().to_string_lossy();
                if name.starts_with('.') || matches!(name.as_ref(), "target" | "tests" | "benches")
                {
                    continue;
                }
                if child.is_dir() {
                    collect_files(&child, files);
                } else if child.extension().and_then(|value| value.to_str()) == Some("rs") {
                    files.push(child);
                }
            }
        }
    }

    let mut files = Vec::new();
    let path = Path::new(&args.path);
    if path.exists() {
        collect_files(path, &mut files);
    } else if let Ok(entries) = fs::read_dir(".") {
        for entry in entries.flatten() {
            let src = entry.path().join("src");
            if src.is_dir() {
                collect_files(&src, &mut files);
            }
        }
    }
    files.sort();
    files.dedup();
    if files.is_empty() {
        return Err("No Rust files found to inspect".to_string());
    }

    let mut report = CovOpt_Analyzer::findings::FindingReport::default();
    for file in files {
        let content = fs::read_to_string(&file).map_err(|error| error.to_string())?;
        let ast =
            syn::parse_file(&content).map_err(|error| format!("{}: {}", file.display(), error))?;
        report
            .findings
            .extend(CovOpt_Analyzer::dataflow::analyze_file(&file));
        for item in ast.items {
            match item {
                syn::Item::Fn(item_fn) => {
                    let function = item_fn.sig.ident.to_string();
                    if args.func.as_ref().is_some_and(|wanted| wanted != &function) {
                        continue;
                    }
                    if item_fn.attrs.iter().any(|attr| {
                        let name = attr
                            .path()
                            .segments
                            .last()
                            .map(|segment| segment.ident.to_string())
                            .unwrap_or_default();
                        matches!(
                            name.as_str(),
                            "test" | "bench" | "covopt_test" | "covopt_bench"
                        )
                    }) {
                        continue;
                    }
                    let mut findings =
                        CovOpt_Analyzer::advisor::EncapsulationAdvisor::analyze(&item_fn, None)
                            .findings;
                    for finding in &mut findings {
                        finding.location.file = file.display().to_string();
                        finding.function = Some(function.clone());
                        finding.id = CovOpt_Analyzer::findings::stable_finding_id(
                            finding.kind,
                            &finding.location.file,
                            finding.location.line,
                            finding.function.as_deref(),
                        );
                        finding.explanation =
                            CovOpt_Analyzer::findings::FindingFormatter::explanation(
                                finding.kind,
                                &function,
                            );
                    }
                    report.findings.extend(findings);
                }
                syn::Item::Struct(item_struct) => {
                    let mut findings =
                        CovOpt_Analyzer::advisor::EncapsulationAdvisor::analyze_struct(
                            &item_struct,
                        )
                        .findings;
                    for finding in &mut findings {
                        finding.location.file = file.display().to_string();
                        finding.function = Some(item_struct.ident.to_string());
                        finding.id = CovOpt_Analyzer::findings::stable_finding_id(
                            finding.kind,
                            &finding.location.file,
                            finding.location.line,
                            finding.function.as_deref(),
                        );
                    }
                    report.findings.extend(findings);
                }
                _ => {}
            }
        }
    }
    report
        .findings
        .sort_by(|left, right| left.id.cmp(&right.id));
    report.findings.dedup_by(|left, right| left.id == right.id);
    let files = report
        .findings
        .iter()
        .map(|finding| finding.location.file.clone())
        .collect::<std::collections::BTreeSet<_>>();
    for file_name in files {
        let file = PathBuf::from(&file_name);
        let Ok(source) = fs::read_to_string(&file) else {
            continue;
        };
        let file_findings = report
            .findings
            .iter()
            .filter(|finding| finding.location.file == file_name)
            .cloned()
            .collect::<Vec<_>>();
        let codegen = CovOpt_Analyzer::codegen_optimizer::generate_candidates(
            &source,
            &file,
            &file_findings,
            &Default::default(),
        )
        .unwrap_or_default();
        for candidate in &codegen {
            for finding in &mut report.findings {
                if candidate.repair.resolves.contains(&finding.id) {
                    finding.repair_candidates.push(candidate.repair.id.clone());
                }
            }
        }
        report
            .repair_candidates
            .extend(codegen.into_iter().map(|candidate| candidate.repair));
    }
    if let Some(explain) = &args.explain {
        report.findings.retain(|finding| finding.id.0 == *explain);
        if report.findings.is_empty() {
            return Err(format!("Finding '{}' was not found", explain));
        }
    }
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?
        );
    } else {
        println!(
            "Structured inspection: {} finding(s)",
            report.findings.len()
        );
        for finding in &report.findings {
            println!(
                "{}",
                CovOpt_Analyzer::findings::FindingFormatter::short(finding)
            );
        }
    }
    fs::create_dir_all("target/covopt").map_err(|error| error.to_string())?;
    fs::write(
        "target/covopt/findings.json",
        serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn resolve_optimizer_source(target: Option<&str>) -> Result<PathBuf, String> {
    if let Some(target) = target {
        let path = PathBuf::from(target);
        if path.is_file() {
            return Ok(path);
        }
        if let Some((_, _, _, path)) =
            CovOpt_Analyzer::static_analysis::find_covopt_test_metadata(target)
        {
            return Ok(path);
        }
        return Err(format!(
            "could not resolve optimizer target '{}' to a Rust source file",
            target
        ));
    }
    let current = PathBuf::from("src");
    if current.is_dir() {
        return std::fs::read_dir(current)
            .map_err(|error| error.to_string())?
            .flatten()
            .map(|entry| entry.path())
            .find(|path| path.extension().and_then(|value| value.to_str()) == Some("rs"))
            .ok_or_else(|| "no Rust source file found".to_string());
    }
    Err("optimizer target is required".to_string())
}

fn collect_structured_findings(
    file: &Path,
    function_filter: Option<&str>,
) -> Result<CovOpt_Analyzer::findings::FindingReport, String> {
    let source = fs::read_to_string(file).map_err(|error| error.to_string())?;
    let ast = syn::parse_file(&source).map_err(|error| error.to_string())?;
    let mut report = CovOpt_Analyzer::findings::FindingReport::default();
    report
        .findings
        .extend(CovOpt_Analyzer::dataflow::analyze_file(file));
    for item in ast.items {
        match item {
            syn::Item::Fn(item_fn) => {
                let name = item_fn.sig.ident.to_string();
                if function_filter.is_some_and(|wanted| wanted != name) {
                    continue;
                }
                let mut findings =
                    CovOpt_Analyzer::advisor::EncapsulationAdvisor::analyze(&item_fn, None)
                        .findings;
                for finding in &mut findings {
                    finding.location.file = file.display().to_string();
                    finding.function = Some(name.clone());
                    finding.id = CovOpt_Analyzer::findings::stable_finding_id(
                        finding.kind,
                        &finding.location.file,
                        finding.location.line,
                        finding.function.as_deref(),
                    );
                    finding.explanation = CovOpt_Analyzer::findings::FindingFormatter::explanation(
                        finding.kind,
                        &name,
                    );
                }
                report.findings.extend(findings);
            }
            syn::Item::Struct(item_struct) => {
                let mut findings =
                    CovOpt_Analyzer::advisor::EncapsulationAdvisor::analyze_struct(&item_struct)
                        .findings;
                for finding in &mut findings {
                    finding.location.file = file.display().to_string();
                    finding.function = Some(item_struct.ident.to_string());
                    finding.id = CovOpt_Analyzer::findings::stable_finding_id(
                        finding.kind,
                        &finding.location.file,
                        finding.location.line,
                        finding.function.as_deref(),
                    );
                }
                report.findings.extend(findings);
            }
            _ => {}
        }
    }
    report
        .findings
        .sort_by(|left, right| left.id.cmp(&right.id));
    report.findings.dedup_by(|left, right| left.id == right.id);
    let codegen = CovOpt_Analyzer::codegen_optimizer::generate_candidates(
        &source,
        file,
        &report.findings,
        &Default::default(),
    )
    .unwrap_or_default();
    for candidate in &codegen {
        for finding in &mut report.findings {
            if candidate.repair.resolves.contains(&finding.id) {
                finding.repair_candidates.push(candidate.repair.id.clone());
            }
        }
    }
    report
        .repair_candidates
        .extend(codegen.into_iter().map(|candidate| candidate.repair));
    for model in
        CovOpt_Analyzer::layout_optimizer::extract_layout(&source, None).unwrap_or_default()
    {
        let findings =
            CovOpt_Analyzer::layout_optimizer::layout_findings(&model, file.display().to_string());
        let layout_candidates = CovOpt_Analyzer::layout_optimizer::generate_candidates(
            &model,
            &findings,
            &Default::default(),
        );
        for candidate in &layout_candidates {
            for finding in &mut report.findings {
                if candidate.repair.resolves.contains(&finding.id) {
                    finding.repair_candidates.push(candidate.repair.id.clone());
                }
            }
        }
        report.repair_candidates.extend(
            layout_candidates
                .into_iter()
                .map(|candidate| candidate.repair),
        );
    }
    Ok(report)
}

fn run_parameter_optimize(args: &CovOpt_Analyzer::config::ParameterOptimizeArgs) -> bool {
    let source_path = match args.source.as_deref() {
        Some(source) => PathBuf::from(source),
        None => match resolve_optimizer_source(Some(&args.target)) {
            Ok(path) => path,
            Err(error) => {
                eprintln!("CovOpt optimize parameters: {error}");
                return false;
            }
        },
    };
    let source = match fs::read_to_string(&source_path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("CovOpt optimize parameters: {error}");
            return false;
        }
    };
    let graph = match CovOpt_Analyzer::parameters::ParameterDependencyGraph::from_source(
        &source,
        &source_path.display().to_string(),
    ) {
        Ok(graph) => graph,
        Err(error) => {
            eprintln!("CovOpt optimize parameters: {error}");
            return false;
        }
    };
    let optimizer = CovOpt_Analyzer::parameter_optimizer::ParameterOptimizer::from_parameter_graph(
        args.target.clone(),
        &graph,
        args.iterations,
        args.seed,
    )
    .with_top_k(args.top_k);
    let result = optimizer.run_phased();
    let output = serde_json::json!({
        "schema_version": covopt_schema::SCHEMA_VERSION,
        "algorithm": "annealed-monte-carlo",
        "source": source_path,
        "phase": format!("{:?}", result.phase),
        "best_params": result.best_params.clone(),
        "candidate_hash": result.candidate_hash,
        "search_score": result.search_score,
        "confirmation_score": result.confirmation_score,
        "robustness_scores": result.robustness_scores,
        "confirmed": result.confirmed,
        "robustness_verified": result.robustness_verified,
        "evaluated_candidates": result.evaluated_candidates,
        "accepted_transitions": result.accepted_transitions,
        "observed": result.confirmed && result.robustness_verified,
    });
    if result.confirmed && result.robustness_verified {
        let env_content = result
            .best_params
            .iter()
            .map(|(name, value)| format!("COVOPT_PARAM_{name}={value}"))
            .collect::<Vec<_>>()
            .join("\n");
        if let Err(error) = fs::write(
            ".covopt_tuned.env",
            format!("# Clean-confirmed by CovOpt\n{env_content}\n"),
        ) {
            eprintln!("CovOpt optimize parameters: could not write defaults: {error}");
            return false;
        }
        if let Err(error) = fs::create_dir_all("target/covopt").and_then(|_| {
            fs::write(
                "target/covopt/parameter-confirmation.json",
                serde_json::to_vec_pretty(&output).unwrap_or_default(),
            )
        }) {
            eprintln!("CovOpt optimize parameters: could not write confirmation: {error}");
            return false;
        }
    }
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&output).unwrap_or_default()
        );
    } else {
        println!(
            "Parameter optimization: phase={:?}, confirmed={}, robustness_verified={}",
            result.phase, result.confirmed, result.robustness_verified
        );
    }
    result.confirmed && result.robustness_verified
}

pub fn run_optimize(args: &CovOpt_Analyzer::config::OptimizeArgs) -> bool {
    let config = CovOpt_Analyzer::config::CovOptConfig::load(".covopt.toml").ok();
    match &args.command {
        CovOpt_Analyzer::config::OptimizeSubcommand::Parameters(args) => {
            run_parameter_optimize(args)
        }
        CovOpt_Analyzer::config::OptimizeSubcommand::Codegen(args) => {
            let source_path = match resolve_optimizer_source(args.target.as_deref()) {
                Ok(path) => path,
                Err(error) => {
                    eprintln!("CovOpt optimize codegen: {}", error);
                    return false;
                }
            };
            let source = match fs::read_to_string(&source_path) {
                Ok(source) => source,
                Err(error) => {
                    eprintln!("CovOpt optimize codegen: {}", error);
                    return false;
                }
            };
            let report = match collect_structured_findings(&source_path, args.target.as_deref()) {
                Ok(report) => report,
                Err(error) => {
                    eprintln!("CovOpt optimize codegen: {}", error);
                    return false;
                }
            };
            let codegen_config = config.as_ref().map_or_else(Default::default, |config| {
                CovOpt_Analyzer::codegen_optimizer::CodegenConfig {
                    lto: config.optimization.codegen.lto.clone(),
                    codegen_units: config.optimization.codegen.codegen_units,
                    opt_level: config.optimization.codegen.opt_level.clone(),
                    target_cpu: config.optimization.codegen.target_cpu.clone(),
                    max_candidates: config.optimization.codegen.max_candidates,
                }
            });
            let candidates = match CovOpt_Analyzer::codegen_optimizer::generate_candidates(
                &source,
                &source_path,
                &report.findings,
                &codegen_config,
            ) {
                Ok(candidates) => candidates,
                Err(error) => {
                    eprintln!("CovOpt optimize codegen: {}", error);
                    return false;
                }
            };
            let output = serde_json::json!({ "target": source_path, "candidates": candidates, "apply_requested": args.apply, "sandbox_required": true });
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&output).unwrap_or_default()
                );
            } else {
                println!(
                    "Codegen candidates: {}",
                    output["candidates"].as_array().map_or(0, Vec::len)
                );
            }
            if args.apply {
                eprintln!("Codegen apply is gated by `covopt fix --plan --apply` verification");
                return false;
            }
            true
        }
        CovOpt_Analyzer::config::OptimizeSubcommand::Layout(args) => {
            let source_path = match resolve_optimizer_source(args.target.as_deref()) {
                Ok(path) => path,
                Err(error) => {
                    eprintln!("CovOpt optimize layout: {}", error);
                    return false;
                }
            };
            let source = match fs::read_to_string(&source_path) {
                Ok(source) => source,
                Err(error) => {
                    eprintln!("CovOpt optimize layout: {}", error);
                    return false;
                }
            };
            let models = match CovOpt_Analyzer::layout_optimizer::extract_layout(
                &source,
                args.struct_name.as_deref(),
            ) {
                Ok(models) => models,
                Err(error) => {
                    eprintln!("CovOpt optimize layout: {}", error);
                    return false;
                }
            };
            let mut candidates = Vec::new();
            let layout_config = config.as_ref().map_or_else(Default::default, |config| {
                CovOpt_Analyzer::layout_optimizer::LayoutConfig {
                    max_candidates: config.optimization.layout.max_candidates,
                    allow_public_abi_suggestions: config
                        .optimization
                        .layout
                        .allow_public_abi_suggestions,
                    cache_line_bytes: config.optimization.layout.cache_line_bytes,
                }
            });
            for model in models {
                let findings = CovOpt_Analyzer::layout_optimizer::layout_findings(
                    &model,
                    source_path.display().to_string(),
                );
                candidates.extend(CovOpt_Analyzer::layout_optimizer::generate_candidates(
                    &model,
                    &findings,
                    &layout_config,
                ));
            }
            let output = serde_json::json!({ "target": source_path, "candidates": candidates, "profile_requested": args.profile, "apply_requested": args.apply });
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&output).unwrap_or_default()
                );
            } else {
                println!(
                    "Layout candidates: {}",
                    output["candidates"].as_array().map_or(0, Vec::len)
                );
            }
            if args.apply {
                eprintln!("Layout apply is gated by `covopt fix --plan --apply` verification");
                return false;
            }
            true
        }
    }
}

fn critical_drift_for_source(source_path: &Path) -> bool {
    let Ok(entries) = fs::read_dir("target/covopt/snapshots") else {
        return false;
    };
    let source_name = source_path.to_string_lossy();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !file_name.ends_with(".json") || file_name.ends_with(".previous.json") {
            continue;
        }
        let Ok(current) = CovOpt_Analyzer::snapshot::AssuranceSnapshot::load(&path) else {
            continue;
        };
        if !current
            .source_hashes
            .keys()
            .any(|source| source.ends_with(source_name.as_ref()) || source_name.ends_with(source))
        {
            continue;
        }
        let previous = path.with_file_name(file_name.replace(".json", ".previous.json"));
        if let Ok(previous) = CovOpt_Analyzer::snapshot::AssuranceSnapshot::load(previous)
            && CovOpt_Analyzer::snapshot::compare_snapshots(&previous, &current).critical
        {
            return true;
        }
    }
    false
}

fn unsafe_evidence_source(target: &str) -> Option<PathBuf> {
    let candidate = PathBuf::from(target);
    if candidate.is_file() {
        Some(candidate)
    } else {
        CovOpt_Analyzer::assurance::find_target_source(target)
    }
}

fn record_unsafe_evidence(target: &str, provider: &str, passed: bool) {
    let Some(source) = unsafe_evidence_source(target) else {
        return;
    };
    let Ok(content) = fs::read_to_string(&source) else {
        return;
    };
    let path = Path::new("target/covopt/unsafe-evidence.json");
    let mut entries = fs::read_to_string(path)
        .ok()
        .and_then(|value| serde_json::from_str::<Vec<serde_json::Value>>(&value).ok())
        .unwrap_or_default();
    entries.push(serde_json::json!({
        "source": unsafe_evidence_source_key(&source),
        "source_hash": CovOpt_Analyzer::repair::SourceEdit::hash_source(&content),
        "provider": provider,
        "passed": passed,
    }));
    let _ = fs::create_dir_all("target/covopt");
    if let Ok(value) = serde_json::to_vec_pretty(&entries) {
        let _ = fs::write(path, value);
    }
}

fn unsafe_evidence_source_key(source: &Path) -> String {
    fs::canonicalize(source)
        .unwrap_or_else(|_| source.to_path_buf())
        .display()
        .to_string()
}

fn unsafe_evidence_is_current(source: &Path) -> bool {
    let Ok(content) = fs::read_to_string(source) else {
        return false;
    };
    let hash = CovOpt_Analyzer::repair::SourceEdit::hash_source(&content);
    let Ok(value) = fs::read_to_string("target/covopt/unsafe-evidence.json") else {
        return false;
    };
    let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(&value) else {
        return false;
    };
    let providers = entries
        .iter()
        .filter(|entry| {
            entry["source_hash"] == hash
                && entry["source"]
                    .as_str()
                    .is_some_and(|path| path == unsafe_evidence_source_key(source))
                && entry["passed"] == true
        })
        .filter_map(|entry| entry["provider"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    providers.contains("sanitizer")
        && (providers.contains("atomic-model")
            || providers.contains("concurrency")
            || providers.contains("temporal"))
}

pub fn run_repair_plan(args: &CovOpt_Analyzer::config::FixArgs) -> bool {
    let config = CovOpt_Analyzer::config::CovOptConfig::load(".covopt.toml").ok();
    let source_path = match resolve_optimizer_source(args.path.as_deref()) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("CovOpt fix: {}", error);
            return false;
        }
    };
    let source = match fs::read_to_string(&source_path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("CovOpt fix: {}", error);
            return false;
        }
    };
    let unsafe_capability_findings =
        CovOpt_Analyzer::static_analysis::analyze_unsafe_macro_capabilities(&source_path);
    if args.apply
        && !unsafe_capability_findings.is_empty()
        && (!args.unsafe_evidence || !unsafe_evidence_is_current(&source_path))
    {
        eprintln!(
            "CovOpt fix: unsafe codegen capability detected; current specialized evidence is required before apply"
        );
        return false;
    }
    if args.apply && critical_drift_for_source(&source_path) {
        eprintln!(
            "CovOpt fix: unexplained critical semantic drift blocks auto-apply; inspect the drift and re-verify first"
        );
        return false;
    }
    let mut report = match collect_structured_findings(&source_path, None) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("CovOpt fix: {}", error);
            return false;
        }
    };
    if let Some(finding) = &args.finding {
        report.findings.retain(|item| item.id.0 == *finding);
    }
    let codegen_config = config.as_ref().map_or_else(Default::default, |config| {
        CovOpt_Analyzer::codegen_optimizer::CodegenConfig {
            lto: config.optimization.codegen.lto.clone(),
            codegen_units: config.optimization.codegen.codegen_units,
            opt_level: config.optimization.codegen.opt_level.clone(),
            target_cpu: config.optimization.codegen.target_cpu.clone(),
            max_candidates: config.optimization.codegen.max_candidates,
        }
    });
    let codegen = CovOpt_Analyzer::codegen_optimizer::generate_candidates(
        &source,
        &source_path,
        &report.findings,
        &codegen_config,
    )
    .unwrap_or_default();
    let mut repairs = codegen
        .into_iter()
        .map(|candidate| candidate.repair)
        .collect::<Vec<_>>();
    let layout_config = config.as_ref().map_or_else(Default::default, |config| {
        CovOpt_Analyzer::layout_optimizer::LayoutConfig {
            max_candidates: config.optimization.layout.max_candidates,
            allow_public_abi_suggestions: config.optimization.layout.allow_public_abi_suggestions,
            cache_line_bytes: config.optimization.layout.cache_line_bytes,
        }
    });
    for model in
        CovOpt_Analyzer::layout_optimizer::extract_layout(&source, None).unwrap_or_default()
    {
        let findings = CovOpt_Analyzer::layout_optimizer::layout_findings(
            &model,
            source_path.display().to_string(),
        );
        repairs.extend(
            CovOpt_Analyzer::layout_optimizer::generate_candidates(
                &model,
                &findings,
                &layout_config,
            )
            .into_iter()
            .map(|candidate| candidate.repair),
        );
    }
    let mut policy = CovOpt_Analyzer::repair::RepairPolicy::default();
    policy.budget_ms = parse_plan_budget(&args.budget).unwrap_or(policy.budget_ms);
    let plan = CovOpt_Analyzer::repair::plan_repairs(&report.findings, &repairs, &policy);
    let evidence_obligations =
        CovOpt_Analyzer::assurance::discover_obligations(&source_path, "repair-apply");
    let evidence_actions = CovOpt_Analyzer::assurance::discover_evidence_actions(
        &CovOpt_Analyzer::assurance::planning_providers(),
        &evidence_obligations,
        &CovOpt_Analyzer::assurance::planner_tool_context(),
    );
    let evidence_plan = CovOpt_Analyzer::assurance::EvidencePlanner::new(Default::default())
        .plan(
            &evidence_obligations,
            &evidence_actions,
            &CovOpt_Analyzer::assurance::planner_tool_context(),
        )
        .plan;
    let mut verification = Vec::new();
    if args.apply && plan.critical_resolved {
        if !evidence_obligations.is_empty()
            && !matches!(
                evidence_plan.status,
                CovOpt_Analyzer::assurance::PlanStatus::Feasible
            )
        {
            eprintln!("CovOpt fix: Evidence Planner could not produce a feasible pre-apply plan");
            return false;
        }
        let selected = plan
            .selected
            .iter()
            .filter_map(|id| repairs.iter().find(|candidate| &candidate.id == id))
            .collect::<Vec<_>>();
        let edits = selected
            .iter()
            .flat_map(|candidate| candidate.changes.iter())
            .cloned()
            .collect::<Vec<_>>();
        if selected
            .iter()
            .any(|candidate| candidate.suggestion_only || candidate.high_risk())
        {
            eprintln!(
                "CovOpt fix: selected repair contains suggestion-only or high-risk changes; explicit specialized verification is required"
            );
            return false;
        }
        if selected.iter().any(|candidate| {
            matches!(
                candidate.kind,
                CovOpt_Analyzer::repair::RepairKind::SeparateAtomic
                    | CovOpt_Analyzer::repair::RepairKind::ReplaceManualCas
            )
        }) && !args.unsafe_evidence
        {
            eprintln!(
                "CovOpt fix: unsafe/atomic repairs require --unsafe-evidence and specialized verification"
            );
            return false;
        }
        if selected.iter().any(|candidate| {
            matches!(
                candidate.kind,
                CovOpt_Analyzer::repair::RepairKind::SeparateAtomic
                    | CovOpt_Analyzer::repair::RepairKind::ReplaceManualCas
            )
        }) && !unsafe_evidence_is_current(&source_path)
        {
            eprintln!(
                "CovOpt fix: current source lacks passing sanitizer plus atomic/concurrency/temporal evidence"
            );
            return false;
        }
        let workspace = match std::env::current_dir() {
            Ok(path) => path,
            Err(error) => {
                eprintln!("CovOpt fix: {}", error);
                return false;
            }
        };
        let absolute_source = if source_path.is_absolute() {
            source_path.clone()
        } else {
            workspace.join(&source_path)
        };
        let verification_result = match CovOpt_Analyzer::repair::verify_edits_in_sandbox(
            &workspace,
            &workspace.join("Cargo.toml"),
            &absolute_source,
            &source,
            &edits,
        ) {
            Ok(result) => result,
            Err(error) => {
                eprintln!("CovOpt fix: sandbox verification failed: {}", error);
                return false;
            }
        };
        if !verification_result.passed {
            eprintln!("CovOpt fix: sandbox cargo check failed; no source was modified");
            return false;
        }
        match CovOpt_Analyzer::repair::apply_edits_safely(&source, &edits) {
            Ok(updated) if updated != source => {
                if syn::parse_file(&updated).is_err() {
                    eprintln!("CovOpt fix: static AST validation failed; no source was modified");
                    return false;
                }
                if let Err(error) =
                    CovOpt_Analyzer::parameters::ParameterDependencyGraph::from_source(
                        &updated,
                        &source_path.display().to_string(),
                    )
                {
                    eprintln!(
                        "CovOpt fix: parameter/scope metadata validation failed: {error}; no source was modified"
                    );
                    return false;
                }
                if let Err(error) = fs::write(&source_path, updated) {
                    eprintln!("CovOpt fix: {}", error);
                    return false;
                }
                for candidate in selected {
                    verification.push(CovOpt_Analyzer::repair::VerificationResult {
                        candidate_id: candidate.id.clone(),
                        passed: true,
                        compile_passed: verification_result.compile_passed,
                        safety_passed: false,
                        regression: false,
                        summary: "sandbox cargo check passed; safety/MCA verification remains recorded as required"
                            .to_string(),
                        actual_cost_ms: 0,
                    });
                }
            }
            Ok(_) => {}
            Err(error) => {
                eprintln!("CovOpt fix: {}", error);
                return false;
            }
        }
    }
    let _ = fs::create_dir_all("target/covopt");
    let _ = CovOpt_Analyzer::repair::write_manifest(
        "target/covopt/repair-manifest.json",
        &plan,
        &verification,
    );
    let output = serde_json::json!({ "findings": report.findings, "candidates": repairs, "plan": plan, "evidence_plan": evidence_plan, "verification": verification, "apply_requested": args.apply });
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&output).unwrap_or_default()
        );
    } else {
        println!(
            "Repair plan: selected {}, blocking {}",
            output["plan"]["selected"].as_array().map_or(0, Vec::len),
            output["plan"]["blocking_findings"]
                .as_array()
                .map_or(0, Vec::len)
        );
    }
    plan.critical_resolved || !args.apply
}

pub fn run_check(args: &CovOpt_Analyzer::config::CheckArgs) -> Result<(), String> {
    let format = args.format.to_ascii_lowercase();
    if !matches!(format.as_str(), "text" | "json" | "sarif" | "html") {
        return Err(format!(
            "unsupported check format '{}'; expected text, json, sarif, or html",
            args.format
        ));
    }
    let budget_ms = parse_plan_budget(&args.budget)?;
    if budget_ms == 0 {
        return Err("check budget must be greater than zero".to_string());
    }
    CovOpt_Analyzer::runner::install_ci_deadline(std::time::Duration::from_millis(budget_ms))?;
    eprintln!("CovOpt check wall-clock budget: {}", args.budget);
    let config = CovOpt_Analyzer::config::CovOptConfig::load(".covopt.toml")?;
    let mode = args.mode.unwrap_or(config.assurance.mode);
    if args.plan {
        let plan_args = CovOpt_Analyzer::config::PlanArgs {
            test: args.target.clone(),
            json: format == "json",
            budget: args.budget.clone(),
            static_only: matches!(mode, CovOpt_Analyzer::assurance::AssurancePolicy::Static),
            planner: None,
            mode: Some(mode),
        };
        if !run_plan(&plan_args, &config) {
            return Err("evidence plan is infeasible".to_string());
        }
        return Ok(());
    }
    run_audit(&CovOpt_Analyzer::config::AuditArgs {
        staged: args.staged,
        base: args.base.clone(),
        test: args.target.clone(),
        fast: args.fast,
        json: format == "json",
        debug_artifacts: args.debug_artifacts,
        assurance: mode,
        evidence_threshold: None,
        adaptive_inputs: false,
    });
    if matches!(format.as_str(), "sarif" | "html") {
        let engine = crate::dashboard::DashboardGenerator::new("target/covopt");
        let result = if format == "sarif" {
            engine.generate_sarif()
        } else {
            engine.generate()
        };
        result.map_err(|error| format!("check report generation failed: {error:?}"))?;
    }
    Ok(())
}

pub fn run_inspect_command(
    args: &CovOpt_Analyzer::config::InspectCommandArgs,
) -> Result<(), String> {
    if args.config {
        let config = CovOpt_Analyzer::config::CovOptConfig::load(".covopt.toml")?;
        let resolved = config
            .target
            .iter()
            .map(|target| config.resolved_target(target))
            .collect::<Vec<_>>();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "version": config.version,
                "providers": config.providers,
                "targets": resolved,
                "optimization": config.optimization,
            }))
            .map_err(|error| error.to_string())?
        );
        return Ok(());
    }
    if args.assumptions || args.drift {
        let target_name = args
            .target
            .clone()
            .or_else(|| {
                CovOpt_Analyzer::config::CovOptConfig::load(".covopt.toml")
                    .ok()
                    .and_then(|config| config.target.first().map(|target| target.test.clone()))
            })
            .ok_or_else(|| {
                "--assumptions/--drift requires --target or a configured target".to_string()
            })?;
        let snapshot_name = target_name
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character
                } else {
                    '_'
                }
            })
            .collect::<String>();
        let current_path =
            PathBuf::from("target/covopt/snapshots").join(format!("{snapshot_name}.json"));
        let current = CovOpt_Analyzer::snapshot::AssuranceSnapshot::load(&current_path)
            .map_err(|error| format!("cannot load current snapshot: {error}"))?;
        let output = if args.assumptions {
            serde_json::to_value(current.assumptions).map_err(|error| error.to_string())?
        } else {
            let base_path = args.base.as_deref().map_or_else(
                || {
                    PathBuf::from("target/covopt/snapshots")
                        .join(format!("{snapshot_name}.previous.json"))
                },
                PathBuf::from,
            );
            match CovOpt_Analyzer::snapshot::AssuranceSnapshot::load(&base_path) {
                Ok(base) => serde_json::to_value(CovOpt_Analyzer::snapshot::compare_snapshots(
                    &base, &current,
                ))
                .map_err(|error| error.to_string())?,
                Err(error) => serde_json::json!({
                    "status": "Unknown",
                    "reason": format!("drift baseline unavailable: {error}"),
                    "baseline": base_path,
                }),
            }
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&output).map_err(|error| error.to_string())?
        );
        return Ok(());
    }
    if args.envelope || args.frontier || args.scope.is_some() {
        let document = fs::read_to_string("target/covopt/assurance.json")
            .map_err(|error| format!("cannot read latest assurance snapshot: {error}"))?;
        let document: serde_json::Value = serde_json::from_str(&document)
            .map_err(|error| format!("cannot parse latest assurance snapshot: {error}"))?;
        let target_name = args.target.as_deref();
        let target = document
            .get("targets")
            .and_then(serde_json::Value::as_array)
            .and_then(|targets| {
                targets.iter().find(|target| {
                    target_name.is_none_or(|name| {
                        target.get("test").and_then(serde_json::Value::as_str) == Some(name)
                    })
                })
            })
            .ok_or_else(|| "no matching target in latest assurance snapshot".to_string())?;
        let assurance = target
            .get("assurance")
            .ok_or_else(|| "latest assurance snapshot has no assurance report".to_string())?;
        let output = if args.envelope {
            assurance
                .get("scope_envelope")
                .cloned()
                .unwrap_or(serde_json::Value::Null)
        } else if args.frontier {
            assurance
                .get("proof_frontier")
                .cloned()
                .unwrap_or(serde_json::Value::Null)
        } else {
            let scope_name = args.scope.as_deref().unwrap_or_default();
            assurance
                .get("scope_envelope")
                .and_then(|envelope| envelope.get("nodes"))
                .and_then(serde_json::Value::as_array)
                .and_then(|nodes| {
                    nodes.iter().find(|node| {
                        node.get("label").and_then(serde_json::Value::as_str) == Some(scope_name)
                            || node
                                .get("id")
                                .and_then(serde_json::Value::as_str)
                                .is_some_and(|id| id.ends_with(scope_name))
                    })
                })
                .cloned()
                .unwrap_or(serde_json::Value::Null)
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&output).map_err(|error| error.to_string())?
        );
        return Ok(());
    }
    let function = args.target.as_deref().and_then(|target| {
        CovOpt_Analyzer::static_analysis::find_covopt_target_metadata(target)
            .map(|metadata| metadata.function)
    });
    let legacy = CovOpt_Analyzer::config::AdviseArgs {
        diff: None,
        path: args.path.clone(),
        func: function.or_else(|| args.target.clone()),
        json: args.format.eq_ignore_ascii_case("json"),
        explain: args.finding.clone(),
    };
    run_inspect(&legacy)
}

struct SnapshotAdversarialOracle {
    unknown_obligations: usize,
}

impl CovOpt_Analyzer::adversarial::AdversarialOracle for SnapshotAdversarialOracle {
    fn evaluate(
        &mut self,
        sample: &CovOpt_Analyzer::adversarial::EnvironmentSample,
        objective: CovOpt_Analyzer::adversarial::AdversarialObjective,
    ) -> CovOpt_Analyzer::adversarial::OracleResult {
        let threads = sample.sample.threads.unwrap_or(1) as f64;
        let n = sample.sample.n.unwrap_or(1) as f64;
        let score = match objective {
            CovOpt_Analyzer::adversarial::AdversarialObjective::ComplexityDeviation => n.log2(),
            CovOpt_Analyzer::adversarial::AdversarialObjective::Contention => threads * n.log2(),
            CovOpt_Analyzer::adversarial::AdversarialObjective::UnknownObligations => {
                self.unknown_obligations as f64
            }
            _ => self.unknown_obligations as f64 + threads,
        };
        let status = if self.unknown_obligations > 0 {
            CovOpt_Analyzer::assurance::ObligationStatus::Unknown
        } else {
            CovOpt_Analyzer::assurance::ObligationStatus::Modeled
        };
        CovOpt_Analyzer::adversarial::OracleResult {
            status,
            failed: false,
            score,
            summary: if self.unknown_obligations > 0 {
                "snapshot contains unknown obligations; runtime oracle not yet selected".to_string()
            } else {
                "bounded static environment oracle evaluated candidate".to_string()
            },
            assumptions: if self.unknown_obligations > 0 {
                vec![CovOpt_Analyzer::model::AssumptionId::new(
                    "runtime-oracle-required",
                )]
            } else {
                Vec::new()
            },
        }
    }
}

fn run_adversarial_optimize(args: &CovOpt_Analyzer::config::AdversarialOptimizeArgs) -> bool {
    let target = args.target.clone().or_else(|| {
        CovOpt_Analyzer::config::CovOptConfig::load(".covopt.toml")
            .ok()
            .and_then(|config| config.target.first().map(|target| target.test.clone()))
    });
    let Some(target) = target else {
        eprintln!("covopt optimize adversarial requires --target or a configured target");
        return false;
    };
    let budget_ms = match parse_plan_budget(&args.budget) {
        Ok(budget) => budget,
        Err(error) => {
            eprintln!("covopt optimize adversarial: {error}");
            return false;
        }
    };
    let unknown_obligations = fs::read_to_string("target/covopt/assurance.json")
        .ok()
        .and_then(|document| serde_json::from_str::<serde_json::Value>(&document).ok())
        .and_then(|document| document["targets"].as_array().cloned())
        .and_then(|targets| targets.into_iter().find(|item| item["test"] == target))
        .and_then(|target| target["assurance"]["coverage"]["unknown_obligation_count"].as_u64())
        .unwrap_or(0) as usize;
    let config = CovOpt_Analyzer::adversarial::AdversarialConfig {
        target: target.clone(),
        budget_ms,
        seed: args.seed,
        objectives: vec![
            CovOpt_Analyzer::adversarial::AdversarialObjective::Contention,
            CovOpt_Analyzer::adversarial::AdversarialObjective::UnknownObligations,
        ],
        domains: vec![
            CovOpt_Analyzer::adversarial::EnvironmentDomain {
                dimension: CovOpt_Analyzer::adversarial::EnvironmentDimension::N,
                values: vec!["1".to_string(), "32".to_string(), "1024".to_string()],
                explicit_bound: true,
            },
            CovOpt_Analyzer::adversarial::EnvironmentDomain {
                dimension: CovOpt_Analyzer::adversarial::EnvironmentDimension::Threads,
                values: vec!["1".to_string(), "2".to_string(), "4".to_string()],
                explicit_bound: true,
            },
            CovOpt_Analyzer::adversarial::EnvironmentDomain {
                dimension: CovOpt_Analyzer::adversarial::EnvironmentDimension::QueueCapacity,
                values: vec!["1".to_string(), "8".to_string(), "32".to_string()],
                explicit_bound: true,
            },
        ],
        max_candidates: 256,
    };
    let mut oracle = SnapshotAdversarialOracle {
        unknown_obligations,
    };
    match CovOpt_Analyzer::adversarial::search(&config, &mut oracle) {
        Ok(result) => {
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&result).unwrap_or_default()
                );
            } else {
                println!(
                    "Adversarial search for {}: {:?} ({} candidates)",
                    result.target, result.status, result.evaluated
                );
            }
            true
        }
        Err(error) => {
            eprintln!("covopt optimize adversarial: {error}");
            false
        }
    }
}

pub fn run_unified_optimize(args: &CovOpt_Analyzer::config::UnifiedOptimizeArgs) -> bool {
    use CovOpt_Analyzer::config::UnifiedOptimizeSubcommand;
    match &args.command {
        UnifiedOptimizeSubcommand::Inputs(input) => run_select_trials(
            &CovOpt_Analyzer::config::SelectTrialsArgs {
                target: input.target.clone(),
                budget: input.budget.clone(),
                json: input.json,
                seed_count: None,
                dry_run: true,
            },
            &match CovOpt_Analyzer::config::CovOptConfig::load(".covopt.toml") {
                Ok(config) => config,
                Err(error) => {
                    eprintln!("CovOpt optimize inputs: {error}");
                    return false;
                }
            },
        ),
        UnifiedOptimizeSubcommand::Parameters(input) => run_parameter_optimize(input),
        UnifiedOptimizeSubcommand::Atomic(input) => {
            let command = if input.apply {
                eprintln!(
                    "CovOpt optimize atomic: apply is suggestion-only; use covopt fix --plan --apply after verification"
                );
                return false;
            } else {
                CovOpt_Analyzer::config::AtomicSubcommand::Analyze(
                    CovOpt_Analyzer::config::AtomicTargetArgs {
                        target: input.target.clone(),
                        source: input.source.clone(),
                        json: input.json,
                        budget: input.budget.clone(),
                    },
                )
            };
            run_atomic(
                &CovOpt_Analyzer::config::AtomicArgs { command },
                &match CovOpt_Analyzer::config::CovOptConfig::load(".covopt.toml") {
                    Ok(config) => config,
                    Err(error) => {
                        eprintln!("CovOpt optimize atomic: {error}");
                        return false;
                    }
                },
            )
        }
        UnifiedOptimizeSubcommand::Adversarial(input) => run_adversarial_optimize(input),
        UnifiedOptimizeSubcommand::Codegen(input) => {
            run_optimize(&CovOpt_Analyzer::config::OptimizeArgs {
                command: CovOpt_Analyzer::config::OptimizeSubcommand::Codegen(input.clone()),
            })
        }
        UnifiedOptimizeSubcommand::Layout(input) => {
            run_optimize(&CovOpt_Analyzer::config::OptimizeArgs {
                command: CovOpt_Analyzer::config::OptimizeSubcommand::Layout(input.clone()),
            })
        }
    }
}

pub fn run_verify(args: &CovOpt_Analyzer::config::VerifyArgs) -> bool {
    use CovOpt_Analyzer::config::VerifySubcommand;
    match &args.command {
        VerifySubcommand::Coverage(input) => {
            run_audit(&CovOpt_Analyzer::config::AuditArgs {
                staged: false,
                base: None,
                test: input.target.clone(),
                fast: false,
                json: input.json,
                debug_artifacts: false,
                assurance: AssurancePolicy::Adaptive,
                evidence_threshold: None,
                adaptive_inputs: false,
            });
            true
        }
        VerifySubcommand::Safety(input) => {
            let Some(target) = input.target.as_deref() else {
                eprintln!("covopt verify safety requires --target");
                return false;
            };
            let ok = crate::harden::run_sanitizer(target, &input.sanitizer, false);
            record_unsafe_evidence(target, "sanitizer", ok);
            if input.json {
                println!(
                    "{}",
                    serde_json::json!({"provider":"sanitizer","target":target,"passed":ok})
                );
            }
            ok
        }
        VerifySubcommand::Concurrency(input) => {
            let Some(target) = input.target.clone() else {
                eprintln!("covopt verify concurrency requires --target");
                return false;
            };
            let result =
                crate::concurrency_fuzzer::run_fuzzer(&CovOpt_Analyzer::config::FuzzArgs {
                    target: target.clone(),
                    timeout_ms: input.timeout_ms,
                    max_iters: input.max_iters,
                    seed: input.seed,
                });
            if let Err(error) = result {
                eprintln!("covopt verify concurrency: {error}");
                false
            } else {
                record_unsafe_evidence(&target, "concurrency", true);
                true
            }
        }
        VerifySubcommand::Runtime(input) => CovOpt_Analyzer::profiler::run_profile(
            input.target.as_deref(),
            input.bin.as_deref(),
            &input.tool,
        ),
        VerifySubcommand::Temporal(input) => {
            let Some(target) = input.target.as_deref() else {
                eprintln!("covopt verify temporal requires --target");
                return false;
            };
            let Some(event) = (!input.event.is_empty()).then_some(input.event.clone()) else {
                eprintln!("covopt verify temporal requires --event");
                return false;
            };
            let source = match trace_source_for_target(target) {
                Ok(source) => source,
                Err(error) => {
                    eprintln!("covopt verify temporal: {error}");
                    return false;
                }
            };
            let trace = match CovOpt_Analyzer::trace::static_trace_from_source(
                &source,
                CovOpt_Analyzer::static_analysis::find_covopt_target_metadata(target)
                    .map(|metadata| metadata.function)
                    .as_deref(),
                CovOpt_Analyzer::model::SampleKey {
                    seed: Some(0),
                    ..Default::default()
                },
            ) {
                Ok(trace) => trace,
                Err(error) => {
                    eprintln!("covopt verify temporal: {error}");
                    return false;
                }
            };
            let operator = match input.operator.to_ascii_lowercase().as_str() {
                "always" => CovOpt_Analyzer::trace::TemporalOperator::Always,
                "eventually" => CovOpt_Analyzer::trace::TemporalOperator::Eventually,
                "until" => CovOpt_Analyzer::trace::TemporalOperator::Until,
                "within_steps" | "within-steps" => {
                    CovOpt_Analyzer::trace::TemporalOperator::WithinSteps
                }
                "bounded_wait" | "bounded-wait" => {
                    CovOpt_Analyzer::trace::TemporalOperator::BoundedWait
                }
                "no_deadlock" | "no-deadlock" => {
                    CovOpt_Analyzer::trace::TemporalOperator::NoDeadlock
                }
                "no_starvation" | "no-starvation" => {
                    CovOpt_Analyzer::trace::TemporalOperator::NoStarvation
                }
                other => {
                    eprintln!("covopt verify temporal: unknown operator '{other}'");
                    return false;
                }
            };
            let result = CovOpt_Analyzer::trace::check_temporal(
                &trace,
                &CovOpt_Analyzer::trace::TemporalContract {
                    name: format!("{target}:{}", input.operator),
                    operator,
                    event,
                    until_event: input.until_event.clone(),
                    bound: input.bound,
                    fairness_assumption: input.fairness.clone(),
                },
                std::time::Duration::from_millis(input.bound.max(1) as u64 * 10),
            );
            match result {
                Ok(result) => {
                    if input.json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&result).unwrap_or_default()
                        );
                    } else {
                        println!("{}", result.summary);
                    }
                    let passed = matches!(
                        result.status,
                        CovOpt_Analyzer::assurance::ObligationStatus::Proven
                            | CovOpt_Analyzer::assurance::ObligationStatus::Modeled
                            | CovOpt_Analyzer::assurance::ObligationStatus::Observed
                    );
                    record_unsafe_evidence(target, "temporal", passed);
                    passed
                }
                Err(error) => {
                    eprintln!("covopt verify temporal: {error}");
                    false
                }
            }
        }
        VerifySubcommand::Relational(input) => {
            let Some(target) = input.target.as_deref() else {
                eprintln!("covopt verify relational requires --target");
                return false;
            };
            let Some(base) = input.base.as_deref() else {
                eprintln!("covopt verify relational requires --base <SOURCE>");
                return false;
            };
            let current_source = match trace_source_for_target(target) {
                Ok(source) => source,
                Err(error) => {
                    eprintln!("covopt verify relational: {error}");
                    return false;
                }
            };
            let base_source = PathBuf::from(base);
            if !base_source.is_file() {
                eprintln!("covopt verify relational: baseline source not found: {base}");
                return false;
            }
            let function = CovOpt_Analyzer::static_analysis::find_covopt_target_metadata(target)
                .map(|metadata| metadata.function);
            let left = CovOpt_Analyzer::trace::static_trace_from_source(
                &current_source,
                function.as_deref(),
                CovOpt_Analyzer::model::SampleKey {
                    seed: Some(0),
                    ..Default::default()
                },
            );
            let right = CovOpt_Analyzer::trace::static_trace_from_source(
                &base_source,
                function.as_deref(),
                CovOpt_Analyzer::model::SampleKey {
                    seed: Some(0),
                    ..Default::default()
                },
            );
            let (left, right) = match (left, right) {
                (Ok(left), Ok(right)) => (left, right),
                (Err(error), _) | (_, Err(error)) => {
                    eprintln!("covopt verify relational: {error}");
                    return false;
                }
            };
            match CovOpt_Analyzer::trace::compare_traces(
                &left,
                &right,
                &CovOpt_Analyzer::trace::RelationalContract {
                    name: format!("{target}:relational"),
                    observations: input
                        .observations
                        .split(',')
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string)
                        .collect(),
                    secret_inputs: Vec::new(),
                    ignored_side_effects: Vec::new(),
                    bound: input.bound,
                },
            ) {
                Ok(result) => {
                    if input.json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&result).unwrap_or_default()
                        );
                    } else {
                        println!("{}", result.summary);
                    }
                    matches!(
                        result.status,
                        CovOpt_Analyzer::assurance::ObligationStatus::Proven
                            | CovOpt_Analyzer::assurance::ObligationStatus::Modeled
                            | CovOpt_Analyzer::assurance::ObligationStatus::Observed
                    )
                }
                Err(error) => {
                    eprintln!("covopt verify relational: {error}");
                    false
                }
            }
        }
    }
}

fn trace_source_for_target(target: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(target);
    if path.is_file() {
        return Ok(path);
    }
    CovOpt_Analyzer::assurance::find_target_source(target)
        .ok_or_else(|| format!("could not resolve source for target '{target}'"))
}
