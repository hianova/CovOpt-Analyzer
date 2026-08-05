//! Obligation-based assurance model.
//!
//! This module deliberately sits beside the legacy audit pipeline.  The legacy
//! checks still produce their existing output, while assurance turns static
//! findings and selected runtime evidence into a traceable, policy-aware report.

use clap::ValueEnum;
use quote::ToTokens;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Expr, ExprCall, ExprMacro, ExprMethodCall, ExprUnary, ItemFn};

pub use crate::model::{
    AssuranceStatus as ObligationStatus, EvidenceActionId, ObligationId, ProviderId,
};

pub const DEFAULT_EVIDENCE_THRESHOLD: f64 = 0.90;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum AssurancePolicy {
    Static,
    #[default]
    Adaptive,
    Strict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ObligationKind {
    MemorySafety,
    BoundsSafety,
    AliasingSafety,
    FfiSafety,
    AtomicOrdering,
    Complexity,
    CpuOverhead,
    MemoryHierarchy,
    ControlFlow,
    ExternalCall,
}

impl ObligationKind {
    pub fn is_safety(self) -> bool {
        matches!(
            self,
            Self::MemorySafety
                | Self::BoundsSafety
                | Self::AliasingSafety
                | Self::FfiSafety
                | Self::AtomicOrdering
        )
    }

    pub fn is_performance(self) -> bool {
        matches!(
            self,
            Self::Complexity
                | Self::CpuOverhead
                | Self::MemoryHierarchy
                | Self::ControlFlow
                | Self::ExternalCall
        )
    }
}

pub type EvidenceStatus = ObligationStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceProviderKind {
    StaticAst,
    Compiler,
    Mca,
    Coverage,
    Test,
    Sanitizer,
    Profiler,
    AtomicModel,
    Temporal,
    Relational,
    Adversarial,
}

impl From<EvidenceProviderKind> for ProviderId {
    fn from(kind: EvidenceProviderKind) -> Self {
        Self(format!("{:?}", kind))
    }
}

/// Strongest status a provider may produce from its own evidence alone.
/// A bounded model, compiler success, or static heuristic must never be
/// promoted to a proof merely because an action completed successfully.
pub fn provider_status_ceiling(provider: EvidenceProviderKind) -> ObligationStatus {
    match provider {
        EvidenceProviderKind::StaticAst
        | EvidenceProviderKind::Compiler
        | EvidenceProviderKind::AtomicModel => ObligationStatus::Modeled,
        EvidenceProviderKind::Mca
        | EvidenceProviderKind::Coverage
        | EvidenceProviderKind::Test
        | EvidenceProviderKind::Sanitizer
        | EvidenceProviderKind::Profiler
        | EvidenceProviderKind::Relational
        | EvidenceProviderKind::Temporal
        | EvidenceProviderKind::Adversarial => ObligationStatus::Observed,
    }
}

fn sound_provider_status(
    provider: EvidenceProviderKind,
    requested: ObligationStatus,
) -> ObligationStatus {
    if matches!(
        requested,
        ObligationStatus::Failed | ObligationStatus::Unknown
    ) {
        return requested;
    }
    let ceiling = provider_status_ceiling(provider);
    if requested.confidence() > ceiling.confidence() {
        ceiling
    } else {
        requested
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLocation {
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub provider: EvidenceProviderKind,
    pub status: ObligationStatus,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Obligation {
    pub id: ObligationId,
    pub kind: ObligationKind,
    pub target: String,
    pub function: Option<String>,
    pub source: Option<SourceLocation>,
    pub severity: Severity,
    pub weight: f64,
    pub provider: EvidenceProviderKind,
    pub status: ObligationStatus,
    pub explanation: String,
    pub remediation: String,
    #[serde(default)]
    pub acceptable_evidence_kinds: Vec<EvidenceProviderKind>,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticFinding {
    pub kind: ObligationKind,
    pub status: ObligationStatus,
    pub severity: Severity,
    pub source: Option<SourceLocation>,
    pub explanation: String,
    pub remediation: String,
}

pub fn finding_from_legacy_text(text: &str, file: Option<&Path>) -> StaticFinding {
    let lower = text.to_ascii_lowercase();
    let kind = if lower.contains("lock") || lower.contains("blocking") {
        ObligationKind::CpuOverhead
    } else if lower.contains("cache") || lower.contains("ipc") {
        ObligationKind::MemoryHierarchy
    } else if lower.contains("thread") || lower.contains("atomic") {
        ObligationKind::AtomicOrdering
    } else if lower.contains("ffi") || lower.contains("foreign") {
        ObligationKind::FfiSafety
    } else if lower.contains("pointer") || lower.contains("unsafe") {
        ObligationKind::MemorySafety
    } else {
        ObligationKind::ControlFlow
    };
    let severity = if kind.is_safety() {
        Severity::High
    } else {
        Severity::Medium
    };
    let status = if kind.is_safety() {
        ObligationStatus::Unknown
    } else {
        ObligationStatus::Modeled
    };
    let line = text
        .split("Line ")
        .nth(1)
        .and_then(|value| value.split(|ch: char| !ch.is_ascii_digit()).next())
        .and_then(|value| value.parse().ok())
        .unwrap_or(1);
    StaticFinding {
        kind,
        status,
        severity,
        source: file.map(|path| SourceLocation {
            file: path.display().to_string(),
            line,
        }),
        explanation: text.trim().to_string(),
        remediation: "Review the finding and collect the cheapest applicable evidence".to_string(),
    }
}

pub fn structured_findings<I>(warnings: I, file: Option<&Path>) -> Vec<StaticFinding>
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    warnings
        .into_iter()
        .map(|warning| finding_from_legacy_text(warning.as_ref(), file))
        .collect()
}

pub fn obligations_from_findings(
    target: &str,
    findings: impl IntoIterator<Item = StaticFinding>,
) -> Vec<Obligation> {
    let mut id = 1000;
    findings
        .into_iter()
        .map(|finding| {
            let mut obligation = make_obligation(
                &mut id,
                finding.kind,
                target,
                None,
                finding.source,
                finding.severity,
                finding.status,
                finding.explanation,
                finding.remediation,
            );
            obligation.provider = EvidenceProviderKind::StaticAst;
            obligation
        })
        .collect()
}

pub fn obligations_from_structured_findings(
    target: &str,
    findings: impl IntoIterator<Item = crate::findings::Finding>,
) -> Vec<Obligation> {
    let mut id = 2000;
    findings
        .into_iter()
        .map(|finding| {
            let kind = match finding.kind {
                crate::findings::FindingKind::GenericBloat
                | crate::findings::FindingKind::ExcessiveParameters
                | crate::findings::FindingKind::MissingInlining
                | crate::findings::FindingKind::ExcessiveInlining
                | crate::findings::FindingKind::HotColdMixing
                | crate::findings::FindingKind::SemanticAsmClone => ObligationKind::CpuOverhead,
                crate::findings::FindingKind::FalseSharing
                | crate::findings::FindingKind::ExcessivePadding
                | crate::findings::FindingKind::PoorFieldLocality => {
                    ObligationKind::MemoryHierarchy
                }
                crate::findings::FindingKind::UnsafeRisk => ObligationKind::MemorySafety,
                crate::findings::FindingKind::BlockingInAsync
                | crate::findings::FindingKind::LockInHotLoop
                | crate::findings::FindingKind::IoInHotLoop
                | crate::findings::FindingKind::AllocationInHotLoop
                | crate::findings::FindingKind::CloneInHotLoop
                | crate::findings::FindingKind::ManualCasLoop
                | crate::findings::FindingKind::LockGuardEscape => ObligationKind::ControlFlow,
            };
            let mut obligation = make_obligation(
                &mut id,
                kind,
                target,
                finding.function,
                Some(finding.location),
                finding.severity,
                ObligationStatus::Modeled,
                finding.explanation,
                "Review the finding and select a verified repair candidate",
            );
            obligation.provider = EvidenceProviderKind::StaticAst;
            obligation
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceCoverage {
    pub resolved_weight: f64,
    pub total_weight: f64,
    pub overall_percent: f64,
    pub critical_safety_resolved_weight: f64,
    pub critical_safety_total_weight: f64,
    pub critical_safety_percent: f64,
    pub performance_resolved_weight: f64,
    pub performance_total_weight: f64,
    pub performance_percent: f64,
    pub unknown_obligation_count: usize,
    #[serde(default)]
    pub failed_obligation_count: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UncertaintyReport {
    pub unknown_critical_obligations: usize,
    pub unsupported_instructions: usize,
    pub opaque_calls: usize,
    pub unmodeled_branches: usize,
    pub missing_dynamic_evidence: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundedResult {
    pub obligation_id: ObligationId,
    pub bound: String,
    pub status: ObligationStatus,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Counterexample {
    pub id: crate::model::TraceId,
    pub obligation_id: Option<ObligationId>,
    pub scope: Option<crate::model::ScopeId>,
    pub summary: String,
    #[serde(default)]
    pub minimized: bool,
    #[serde(default)]
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofFrontier {
    pub schema_version: u32,
    #[serde(default)]
    pub resolved: Vec<ObligationId>,
    #[serde(default)]
    pub bounded: Vec<BoundedResult>,
    #[serde(default)]
    pub assumptions: Vec<crate::model::AssumptionId>,
    #[serde(default)]
    pub unknown: Vec<ObligationId>,
    #[serde(default)]
    pub counterexamples: Vec<Counterexample>,
    #[serde(default)]
    pub next_actions: Vec<EvidenceActionId>,
}

impl ProofFrontier {
    pub fn from_obligations(obligations: &[Obligation], plan: Option<&EvidencePlan>) -> Self {
        let mut frontier = Self {
            schema_version: crate::model::MODEL_SCHEMA_VERSION,
            resolved: Vec::new(),
            bounded: Vec::new(),
            assumptions: Vec::new(),
            unknown: Vec::new(),
            counterexamples: Vec::new(),
            next_actions: Vec::new(),
        };
        for obligation in obligations {
            match obligation.status {
                ObligationStatus::Proven
                | ObligationStatus::Modeled
                | ObligationStatus::Observed => frontier.resolved.push(obligation.id.clone()),
                ObligationStatus::Assumed => {
                    frontier
                        .assumptions
                        .push(crate::model::AssumptionId::new(format!(
                            "obligation::{}",
                            obligation.id
                        )));
                }
                ObligationStatus::Unknown => frontier.unknown.push(obligation.id.clone()),
                ObligationStatus::Failed => frontier.unknown.push(obligation.id.clone()),
            }
            for evidence in &obligation.evidence {
                if matches!(evidence.status, ObligationStatus::Modeled)
                    && evidence
                        .details
                        .as_ref()
                        .and_then(|details| details.get("bound"))
                        .is_some()
                {
                    frontier.bounded.push(BoundedResult {
                        obligation_id: obligation.id.clone(),
                        bound: evidence
                            .details
                            .as_ref()
                            .and_then(|details| details.get("bound"))
                            .map_or_else(|| "bounded".to_string(), |value| value.to_string()),
                        status: evidence.status,
                        summary: evidence.summary.clone(),
                    });
                }
            }
        }
        if let Some(plan) = plan {
            frontier.next_actions = plan
                .candidate_actions
                .iter()
                .filter(|action| {
                    action
                        .covers
                        .iter()
                        .any(|obligation| frontier.unknown.contains(obligation))
                })
                .map(|action| action.id.clone())
                .collect();
        }
        frontier
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssuranceReport {
    #[serde(default = "default_model_schema_version")]
    pub schema_version: u32,
    pub policy: AssurancePolicy,
    pub evidence_threshold: f64,
    pub obligations: Vec<Obligation>,
    pub coverage: EvidenceCoverage,
    pub uncertainty: UncertaintyReport,
    pub passes: bool,
    #[serde(default)]
    pub plan: Option<EvidencePlan>,
    #[serde(default)]
    pub follow_up_plan: Option<EvidencePlan>,
    #[serde(default)]
    pub trial_plan: Option<crate::trial_selection::TrialPlan>,
    #[serde(default)]
    pub atomic: Option<serde_json::Value>,
    #[serde(default)]
    pub unavailable_providers: Vec<String>,
    #[serde(default)]
    pub line_coverage_percent: Option<f64>,
    #[serde(default)]
    pub scope_envelope: Option<crate::scope::ScopeEnvelope>,
    #[serde(default)]
    pub proof_frontier: Option<ProofFrontier>,
    #[serde(default)]
    pub semantic_drift: Option<crate::snapshot::SemanticDrift>,
    #[serde(default)]
    pub parameter_graph: Option<crate::parameters::ParameterDependencyGraph>,
    #[serde(default)]
    pub metadata_index: Option<crate::static_analysis::SourceMetadataIndex>,
}

impl AssuranceReport {
    pub fn apply_legacy_coverage(&mut self, observed: bool, summary: &str) {
        if observed {
            for obligation in &mut self.obligations {
                if matches!(
                    obligation.kind,
                    ObligationKind::Complexity | ObligationKind::ControlFlow
                ) && ObligationStatus::Observed.confidence() > obligation.status.confidence()
                {
                    obligation.status = ObligationStatus::Observed;
                    obligation.evidence.push(Evidence {
                        provider: EvidenceProviderKind::Coverage,
                        status: ObligationStatus::Observed,
                        summary: summary.to_string(),
                        details: None,
                    });
                }
            }
            self.recalculate();
        }
    }

    pub fn apply_provider_evidence(
        &mut self,
        provider: EvidenceProviderKind,
        status: ObligationStatus,
        summary: &str,
        details: Option<serde_json::Value>,
    ) {
        let status = sound_provider_status(provider, status);
        for obligation in &mut self.obligations {
            if obligation.acceptable_evidence_kinds.contains(&provider)
                && (!matches!(obligation.status, ObligationStatus::Failed)
                    && (matches!(status, ObligationStatus::Failed)
                        || status.confidence() > obligation.status.confidence()))
            {
                obligation.status = status;
                obligation.evidence.push(Evidence {
                    provider,
                    status,
                    summary: summary.to_string(),
                    details: details.clone(),
                });
            }
        }
        self.recalculate();
    }

    fn recalculate(&mut self) {
        self.coverage = calculate_coverage(&self.obligations);
        self.uncertainty = calculate_uncertainty(&self.obligations);
        self.passes = policy_passes(self.policy, &self.coverage, self.evidence_threshold);
    }
}

fn default_model_schema_version() -> u32 {
    crate::model::MODEL_SCHEMA_VERSION
}

pub struct ProviderResult {
    pub status: ObligationStatus,
    pub summary: String,
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub enum ProviderAvailability {
    Available,
    Unavailable(String),
}

pub trait EvidenceProvider: Send + Sync {
    fn kind(&self) -> EvidenceProviderKind;
    fn supports(&self, obligation: &Obligation) -> bool;
    fn availability(&self) -> ProviderAvailability;
    fn evaluate(&self, obligation: &Obligation) -> ProviderResult;

    fn provider_id(&self) -> ProviderId {
        self.kind().into()
    }

    fn discover_actions(
        &self,
        obligations: &[Obligation],
        context: &PlanningContext,
    ) -> Vec<EvidenceAction> {
        discover_actions_for_provider(self.provider_id(), self.kind(), obligations, context)
    }

    fn execute(
        &self,
        action: &EvidenceAction,
        context: &ExecutionContext,
    ) -> Result<EvidenceResult, EvidenceError> {
        let _ = (action, context);
        Err(EvidenceError::Unavailable(format!(
            "{} does not expose an executable action",
            self.provider_id()
        )))
    }
}

#[derive(Default)]
pub struct StaticAstProvider;

impl EvidenceProvider for StaticAstProvider {
    fn kind(&self) -> EvidenceProviderKind {
        EvidenceProviderKind::StaticAst
    }

    fn supports(&self, obligation: &Obligation) -> bool {
        obligation.provider == EvidenceProviderKind::StaticAst
    }

    fn availability(&self) -> ProviderAvailability {
        ProviderAvailability::Available
    }

    fn evaluate(&self, _obligation: &Obligation) -> ProviderResult {
        ProviderResult {
            status: ObligationStatus::Unknown,
            summary: "Static AST could not prove this obligation".to_string(),
            details: None,
        }
    }

    fn discover_actions(
        &self,
        _obligations: &[Obligation],
        _context: &PlanningContext,
    ) -> Vec<EvidenceAction> {
        // Discovery has already run before planning. Re-running the same static
        // pass cannot resolve an obligation it just classified as Unknown.
        Vec::new()
    }

    fn execute(
        &self,
        action: &EvidenceAction,
        _context: &ExecutionContext,
    ) -> Result<EvidenceResult, EvidenceError> {
        Ok(EvidenceResult {
            action_id: action.id.clone(),
            status: action.effective_status(),
            summary: "Static AST evidence collected".to_string(),
            covered: action.covers.clone(),
            actual_cost_ms: action.estimated_cost_ms,
            confidence: action.confidence,
            details: None,
        })
    }
}

pub struct LegacyCoverageProvider {
    pub observed: bool,
    pub summary: String,
}

pub type CoverageProvider = LegacyCoverageProvider;

pub struct McaProvider {
    pub report: Option<crate::mca::McaReport>,
}

#[derive(Default)]
pub struct AtomicModelProvider;

impl EvidenceProvider for AtomicModelProvider {
    fn kind(&self) -> EvidenceProviderKind {
        EvidenceProviderKind::AtomicModel
    }

    fn supports(&self, obligation: &Obligation) -> bool {
        obligation.kind == ObligationKind::AtomicOrdering
    }

    fn availability(&self) -> ProviderAvailability {
        ProviderAvailability::Available
    }

    fn evaluate(&self, _obligation: &Obligation) -> ProviderResult {
        ProviderResult {
            status: ObligationStatus::Unknown,
            summary: "bounded atomic model checking requires an explicit correctness contract"
                .to_string(),
            details: None,
        }
    }

    fn discover_actions(
        &self,
        obligations: &[Obligation],
        context: &PlanningContext,
    ) -> Vec<EvidenceAction> {
        let mut actions =
            discover_actions_for_provider(self.provider_id(), self.kind(), obligations, context);
        let contract = context
            .metadata
            .get("atomic_contract")
            .is_some_and(|value| value == "true");
        for action in &mut actions {
            if !contract {
                action.available = false;
                action.result_status = ObligationStatus::Unknown;
                action.description =
                    "atomic model action blocked: no correctness contract".to_string();
            }
            action.required_tools = Vec::new();
        }
        actions
    }
}

pub struct ContractProvider {
    pub kind: EvidenceProviderKind,
    pub supported: fn(ObligationKind) -> bool,
    pub summary: &'static str,
}

impl EvidenceProvider for ContractProvider {
    fn kind(&self) -> EvidenceProviderKind {
        self.kind
    }

    fn supports(&self, obligation: &Obligation) -> bool {
        (self.supported)(obligation.kind)
    }

    fn availability(&self) -> ProviderAvailability {
        ProviderAvailability::Available
    }

    fn evaluate(&self, _obligation: &Obligation) -> ProviderResult {
        ProviderResult {
            status: ObligationStatus::Unknown,
            summary: format!("{} requires an explicit contract execution", self.summary),
            details: None,
        }
    }
}

impl EvidenceProvider for McaProvider {
    fn kind(&self) -> EvidenceProviderKind {
        EvidenceProviderKind::Mca
    }

    fn supports(&self, obligation: &Obligation) -> bool {
        supports_mca(obligation.kind)
    }

    fn availability(&self) -> ProviderAvailability {
        if self.report.is_some() {
            ProviderAvailability::Available
        } else {
            match Command::new("llvm-mca").arg("--version").output() {
                Ok(output) if output.status.success() => ProviderAvailability::Available,
                Ok(output) => ProviderAvailability::Unavailable(format!(
                    "llvm-mca returned status {}",
                    output.status
                )),
                Err(error) => {
                    ProviderAvailability::Unavailable(format!("llvm-mca is unavailable: {}", error))
                }
            }
        }
    }

    fn evaluate(&self, _obligation: &Obligation) -> ProviderResult {
        if let Some(report) = &self.report {
            ProviderResult {
                status: ObligationStatus::Observed,
                summary: "LLVM-MCA evidence collected from the target assembly".to_string(),
                details: serde_json::to_value(report).ok(),
            }
        } else {
            ProviderResult {
                status: ObligationStatus::Unknown,
                summary: "LLVM-MCA is available but target assembly evidence was not collected"
                    .to_string(),
                details: None,
            }
        }
    }

    fn execute(
        &self,
        action: &EvidenceAction,
        _context: &ExecutionContext,
    ) -> Result<EvidenceResult, EvidenceError> {
        let Some(report) = &self.report else {
            return Err(EvidenceError::Unavailable(
                "LLVM-MCA report was not collected for this action".to_string(),
            ));
        };
        Ok(EvidenceResult {
            action_id: action.id.clone(),
            status: ObligationStatus::Observed,
            summary: "LLVM-MCA evidence collected".to_string(),
            covered: action.covers.clone(),
            actual_cost_ms: action.estimated_cost_ms,
            confidence: action.confidence,
            details: serde_json::to_value(report).ok(),
        })
    }
}

impl EvidenceProvider for LegacyCoverageProvider {
    fn kind(&self) -> EvidenceProviderKind {
        EvidenceProviderKind::Coverage
    }

    fn supports(&self, obligation: &Obligation) -> bool {
        matches!(
            obligation.kind,
            ObligationKind::Complexity | ObligationKind::ControlFlow
        )
    }

    fn availability(&self) -> ProviderAvailability {
        if self.observed {
            ProviderAvailability::Available
        } else {
            ProviderAvailability::Unavailable("targeted coverage was not executed".to_string())
        }
    }

    fn evaluate(&self, _obligation: &Obligation) -> ProviderResult {
        ProviderResult {
            status: if self.observed {
                ObligationStatus::Observed
            } else {
                ObligationStatus::Unknown
            },
            summary: self.summary.clone(),
            details: None,
        }
    }

    fn execute(
        &self,
        action: &EvidenceAction,
        _context: &ExecutionContext,
    ) -> Result<EvidenceResult, EvidenceError> {
        if !self.observed {
            return Err(EvidenceError::Unavailable(
                "targeted coverage was not executed".to_string(),
            ));
        }
        Ok(EvidenceResult {
            action_id: action.id.clone(),
            status: ObligationStatus::Observed,
            summary: self.summary.clone(),
            covered: action.covers.clone(),
            actual_cost_ms: action.estimated_cost_ms,
            confidence: action.confidence,
            details: None,
        })
    }
}

macro_rules! named_command_provider {
    ($name:ident, $kind:expr, $command:expr, $supports:ident) => {
        #[derive(Debug, Default)]
        pub struct $name;

        impl EvidenceProvider for $name {
            fn kind(&self) -> EvidenceProviderKind {
                $kind
            }

            fn supports(&self, obligation: &Obligation) -> bool {
                $supports(obligation.kind)
            }

            fn availability(&self) -> ProviderAvailability {
                CommandProvider {
                    kind: $kind,
                    command: $command,
                    supported: $supports,
                }
                .availability()
            }

            fn evaluate(&self, obligation: &Obligation) -> ProviderResult {
                CommandProvider {
                    kind: $kind,
                    command: $command,
                    supported: $supports,
                }
                .evaluate(obligation)
            }
        }
    };
}

named_command_provider!(
    CompilerProvider,
    EvidenceProviderKind::Compiler,
    "rustc",
    supports_safety
);
named_command_provider!(
    SanitizerProvider,
    EvidenceProviderKind::Sanitizer,
    "cargo",
    supports_safety
);
named_command_provider!(
    ProfileProvider,
    EvidenceProviderKind::Profiler,
    "samply",
    supports_profile
);
named_command_provider!(
    TestProvider,
    EvidenceProviderKind::Test,
    "cargo",
    supports_test
);

pub struct CommandProvider {
    kind: EvidenceProviderKind,
    command: &'static str,
    supported: fn(ObligationKind) -> bool,
}

impl EvidenceProvider for CommandProvider {
    fn kind(&self) -> EvidenceProviderKind {
        self.kind
    }

    fn supports(&self, obligation: &Obligation) -> bool {
        (self.supported)(obligation.kind)
    }

    fn availability(&self) -> ProviderAvailability {
        match Command::new(self.command).arg("--version").output() {
            Ok(output) if output.status.success() => ProviderAvailability::Available,
            Ok(output) => ProviderAvailability::Unavailable(format!(
                "{} returned status {}",
                self.command, output.status
            )),
            Err(error) => ProviderAvailability::Unavailable(format!(
                "{} is unavailable: {}",
                self.command, error
            )),
        }
    }

    fn evaluate(&self, _obligation: &Obligation) -> ProviderResult {
        ProviderResult {
            status: ObligationStatus::Unknown,
            summary: format!(
                "{} is available but targeted evidence was not collected",
                self.command
            ),
            details: None,
        }
    }
}

fn supports_safety(kind: ObligationKind) -> bool {
    kind.is_safety()
}

fn supports_mca(kind: ObligationKind) -> bool {
    matches!(
        kind,
        ObligationKind::CpuOverhead | ObligationKind::MemoryHierarchy
    )
}

fn supports_coverage(kind: ObligationKind) -> bool {
    matches!(
        kind,
        ObligationKind::Complexity | ObligationKind::ControlFlow
    )
}

fn supports_profile(kind: ObligationKind) -> bool {
    matches!(
        kind,
        ObligationKind::CpuOverhead
            | ObligationKind::MemoryHierarchy
            | ObligationKind::ExternalCall
    )
}

fn supports_test(kind: ObligationKind) -> bool {
    kind.is_safety() || matches!(kind, ObligationKind::ControlFlow)
}

fn supports_temporal(kind: ObligationKind) -> bool {
    matches!(
        kind,
        ObligationKind::ControlFlow | ObligationKind::AtomicOrdering
    )
}

fn supports_relational(kind: ObligationKind) -> bool {
    matches!(
        kind,
        ObligationKind::Complexity | ObligationKind::ControlFlow
    )
}

fn supports_adversarial(kind: ObligationKind) -> bool {
    kind.is_safety() || kind.is_performance()
}

pub struct AssuranceScheduler {
    policy: AssurancePolicy,
    evidence_threshold: f64,
    providers: Vec<Box<dyn EvidenceProvider>>,
}

impl AssuranceScheduler {
    pub fn new(policy: AssurancePolicy, evidence_threshold: f64, legacy_coverage: bool) -> Self {
        Self::with_mca_report(policy, evidence_threshold, legacy_coverage, None)
    }

    pub fn with_mca_report(
        policy: AssurancePolicy,
        evidence_threshold: f64,
        legacy_coverage: bool,
        mca_report: Option<crate::mca::McaReport>,
    ) -> Self {
        let mut providers: Vec<Box<dyn EvidenceProvider>> = vec![Box::new(StaticAstProvider)];
        if !matches!(policy, AssurancePolicy::Static) {
            providers.insert(
                0,
                Box::new(LegacyCoverageProvider {
                    observed: legacy_coverage,
                    summary: "Legacy targeted audit coverage and complexity evidence".to_string(),
                }),
            );
            providers.push(Box::new(CompilerProvider));
            providers.push(Box::new(McaProvider { report: mca_report }));
            providers.push(Box::new(AtomicModelProvider));
            providers.push(Box::new(CommandProvider {
                kind: EvidenceProviderKind::Coverage,
                command: "llvm-cov",
                supported: supports_coverage,
            }));
            providers.push(Box::new(SanitizerProvider));
            providers.push(Box::new(ProfileProvider));
            providers.push(Box::new(TestProvider));
            providers.push(Box::new(ContractProvider {
                kind: EvidenceProviderKind::Temporal,
                supported: supports_temporal,
                summary: "bounded temporal contract checking",
            }));
            providers.push(Box::new(ContractProvider {
                kind: EvidenceProviderKind::Relational,
                supported: supports_relational,
                summary: "relational trace comparison",
            }));
            providers.push(Box::new(ContractProvider {
                kind: EvidenceProviderKind::Adversarial,
                supported: supports_adversarial,
                summary: "bounded adversarial environment search",
            }));
        }
        Self {
            policy,
            evidence_threshold,
            providers,
        }
    }

    pub fn evaluate(&self, mut obligations: Vec<Obligation>) -> AssuranceReport {
        let mut unavailable_providers = Vec::new();
        for obligation in &mut obligations {
            if matches!(
                obligation.status,
                ObligationStatus::Proven | ObligationStatus::Failed
            ) {
                continue;
            }
            for provider in &self.providers {
                if !provider.supports(obligation) {
                    continue;
                }
                match provider.availability() {
                    ProviderAvailability::Unavailable(reason) => {
                        unavailable_providers.push(format!("{:?}: {}", provider.kind(), reason));
                        obligation.evidence.push(Evidence {
                            provider: provider.kind(),
                            status: ObligationStatus::Unknown,
                            summary: reason,
                            details: None,
                        });
                    }
                    ProviderAvailability::Available => {
                        let mut result = provider.evaluate(obligation);
                        result.status = sound_provider_status(provider.kind(), result.status);
                        obligation.evidence.push(Evidence {
                            provider: provider.kind(),
                            status: result.status,
                            summary: result.summary,
                            details: result.details,
                        });
                        if result.status.confidence() > obligation.status.confidence() {
                            obligation.status = result.status;
                        }
                    }
                }
            }
        }
        let coverage = calculate_coverage(&obligations);
        let uncertainty = calculate_uncertainty(&obligations);
        let passes = policy_passes(self.policy, &coverage, self.evidence_threshold);
        AssuranceReport {
            schema_version: crate::model::MODEL_SCHEMA_VERSION,
            policy: self.policy,
            evidence_threshold: self.evidence_threshold,
            obligations,
            coverage,
            uncertainty,
            passes,
            plan: None,
            follow_up_plan: None,
            trial_plan: None,
            atomic: None,
            unavailable_providers,
            line_coverage_percent: None,
            scope_envelope: None,
            proof_frontier: None,
            semantic_drift: None,
            parameter_graph: None,
            metadata_index: None,
        }
    }
}

pub fn planning_providers() -> Vec<Box<dyn EvidenceProvider>> {
    vec![
        Box::new(StaticAstProvider),
        Box::new(CompilerProvider),
        Box::new(McaProvider { report: None }),
        Box::new(AtomicModelProvider),
        Box::new(CommandProvider {
            kind: EvidenceProviderKind::Coverage,
            command: "llvm-cov",
            supported: supports_coverage,
        }),
        Box::new(TestProvider),
        Box::new(SanitizerProvider),
        Box::new(ProfileProvider),
        Box::new(ContractProvider {
            kind: EvidenceProviderKind::Temporal,
            supported: supports_temporal,
            summary: "bounded temporal contract checking",
        }),
        Box::new(ContractProvider {
            kind: EvidenceProviderKind::Relational,
            supported: supports_relational,
            summary: "relational trace comparison",
        }),
        Box::new(ContractProvider {
            kind: EvidenceProviderKind::Adversarial,
            supported: supports_adversarial,
            summary: "bounded adversarial environment search",
        }),
    ]
}

fn policy_passes(policy: AssurancePolicy, coverage: &EvidenceCoverage, threshold: f64) -> bool {
    if coverage.failed_obligation_count > 0 {
        return false;
    }
    if coverage.critical_safety_percent < 100.0 {
        return false;
    }
    if matches!(policy, AssurancePolicy::Static) {
        return true;
    }
    coverage.overall_percent >= threshold * 100.0
}

fn calculate_coverage(obligations: &[Obligation]) -> EvidenceCoverage {
    let mut total = 0.0;
    let mut resolved = 0.0;
    let mut critical_total = 0.0;
    let mut critical_resolved = 0.0;
    let mut performance_total = 0.0;
    let mut performance_resolved = 0.0;
    let mut unknown = 0;
    let mut failed = 0;
    for obligation in obligations {
        total += obligation.weight;
        resolved += obligation.weight * obligation.status.confidence();
        if matches!(obligation.severity, Severity::Critical) && obligation.kind.is_safety() {
            critical_total += obligation.weight;
            critical_resolved += obligation.weight * obligation.status.confidence();
        }
        if obligation.kind.is_performance() {
            performance_total += obligation.weight;
            performance_resolved += obligation.weight * obligation.status.confidence();
        }
        if matches!(obligation.status, ObligationStatus::Unknown) {
            unknown += 1;
        }
        if matches!(obligation.status, ObligationStatus::Failed) {
            failed += 1;
        }
    }
    EvidenceCoverage {
        resolved_weight: resolved,
        total_weight: total,
        overall_percent: percent(resolved, total),
        critical_safety_resolved_weight: critical_resolved,
        critical_safety_total_weight: critical_total,
        critical_safety_percent: percent(critical_resolved, critical_total),
        performance_resolved_weight: performance_resolved,
        performance_total_weight: performance_total,
        performance_percent: percent(performance_resolved, performance_total),
        unknown_obligation_count: unknown,
        failed_obligation_count: failed,
    }
}

fn calculate_uncertainty(obligations: &[Obligation]) -> UncertaintyReport {
    let mut report = UncertaintyReport::default();
    for obligation in obligations {
        if matches!(obligation.status, ObligationStatus::Unknown) {
            report.missing_dynamic_evidence += 1;
            if matches!(obligation.severity, Severity::Critical) {
                report.unknown_critical_obligations += 1;
            }
            if obligation.kind == ObligationKind::ControlFlow {
                report.unmodeled_branches += 1;
            }
            if obligation.kind == ObligationKind::ExternalCall {
                report.opaque_calls += 1;
            }
        }
        for evidence in &obligation.evidence {
            if let Some(details) = &evidence.details {
                report.unsupported_instructions += details
                    .get("unsupported_instructions")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0) as usize;
            }
        }
    }
    report
}

fn percent(value: f64, total: f64) -> f64 {
    if total == 0.0 {
        100.0
    } else {
        value / total * 100.0
    }
}

#[allow(clippy::too_many_arguments)]
fn make_obligation(
    id: &mut usize,
    kind: ObligationKind,
    target: &str,
    function: Option<String>,
    source: Option<SourceLocation>,
    severity: Severity,
    status: ObligationStatus,
    explanation: impl Into<String>,
    remediation: impl Into<String>,
) -> Obligation {
    *id += 1;
    Obligation {
        id: ObligationId(format!("COVOPT-OBL-{:04}", *id)),
        kind,
        target: target.to_string(),
        function,
        source,
        severity,
        weight: if matches!(severity, Severity::Critical) {
            2.0
        } else {
            1.0
        },
        provider: if status.resolved() {
            EvidenceProviderKind::StaticAst
        } else {
            match kind {
                ObligationKind::Complexity | ObligationKind::ControlFlow => {
                    EvidenceProviderKind::Coverage
                }
                ObligationKind::CpuOverhead | ObligationKind::MemoryHierarchy => {
                    EvidenceProviderKind::Mca
                }
                ObligationKind::AtomicOrdering => EvidenceProviderKind::AtomicModel,
                ObligationKind::ExternalCall => EvidenceProviderKind::Profiler,
                _ => EvidenceProviderKind::Sanitizer,
            }
        },
        status,
        explanation: explanation.into(),
        remediation: remediation.into(),
        acceptable_evidence_kinds: acceptable_evidence_for(kind),
        evidence: Vec::new(),
    }
}

pub fn scope_attribution_obligation(target: &str, scope: &str) -> Obligation {
    Obligation {
        id: ObligationId::new(scope),
        kind: ObligationKind::ControlFlow,
        target: target.to_string(),
        function: Some(scope.to_string()),
        source: None,
        severity: Severity::High,
        weight: 1.0,
        provider: EvidenceProviderKind::Coverage,
        status: ObligationStatus::Unknown,
        explanation: "Coverage could not be reliably attributed to this reachable scope"
            .to_string(),
        remediation: "Collect function/branch coverage with a source mapping for this scope"
            .to_string(),
        acceptable_evidence_kinds: vec![EvidenceProviderKind::Coverage],
        evidence: Vec::new(),
    }
}

fn acceptable_evidence_for(kind: ObligationKind) -> Vec<EvidenceProviderKind> {
    match kind {
        ObligationKind::Complexity => vec![
            EvidenceProviderKind::StaticAst,
            EvidenceProviderKind::Coverage,
            EvidenceProviderKind::Relational,
        ],
        ObligationKind::ControlFlow => {
            vec![
                EvidenceProviderKind::StaticAst,
                EvidenceProviderKind::Coverage,
                EvidenceProviderKind::Test,
                EvidenceProviderKind::Relational,
                EvidenceProviderKind::Temporal,
            ]
        }
        ObligationKind::CpuOverhead | ObligationKind::MemoryHierarchy => {
            vec![
                EvidenceProviderKind::StaticAst,
                EvidenceProviderKind::Mca,
                EvidenceProviderKind::Profiler,
                EvidenceProviderKind::Adversarial,
            ]
        }
        ObligationKind::ExternalCall => {
            vec![
                EvidenceProviderKind::Compiler,
                EvidenceProviderKind::Profiler,
                EvidenceProviderKind::Test,
            ]
        }
        ObligationKind::AtomicOrdering => vec![
            EvidenceProviderKind::StaticAst,
            EvidenceProviderKind::Compiler,
            EvidenceProviderKind::AtomicModel,
            EvidenceProviderKind::Test,
            EvidenceProviderKind::Temporal,
            EvidenceProviderKind::Adversarial,
        ],
        _ => vec![
            EvidenceProviderKind::StaticAst,
            EvidenceProviderKind::Compiler,
            EvidenceProviderKind::Sanitizer,
            EvidenceProviderKind::Test,
            EvidenceProviderKind::Adversarial,
        ],
    }
}

pub fn obligation_from_mca(target: &str, report: &crate::mca::McaReport) -> Obligation {
    let mut id = 0;
    let mut obligation = make_obligation(
        &mut id,
        ObligationKind::CpuOverhead,
        target,
        Some(target.to_string()),
        None,
        Severity::Medium,
        ObligationStatus::Observed,
        "LLVM-MCA measured the target instruction stream",
        "Track throughput, IPC, and unsupported instructions against the performance baseline",
    );
    obligation.provider = EvidenceProviderKind::Mca;
    obligation.evidence.push(Evidence {
        provider: EvidenceProviderKind::Mca,
        status: ObligationStatus::Observed,
        summary: "LLVM-MCA target report".to_string(),
        details: serde_json::to_value(report).ok(),
    });
    obligation
}

struct DiscoveryVisitor {
    file: String,
    target: String,
    function: Option<String>,
    async_function: bool,
    loop_depth: usize,
    obligations: Vec<Obligation>,
    next_id: usize,
    seen: HashSet<String>,
}

impl DiscoveryVisitor {
    fn source(&self, line: usize) -> SourceLocation {
        SourceLocation {
            file: self.file.clone(),
            line,
        }
    }

    fn add(
        &mut self,
        kind: ObligationKind,
        line: usize,
        severity: Severity,
        status: ObligationStatus,
        explanation: &str,
        remediation: &str,
    ) {
        let function = self.function.clone();
        let key = format!("{:?}:{}:{:?}", kind, line, function);
        if self.seen.insert(key) {
            let source = Some(self.source(line));
            self.obligations.push(make_obligation(
                &mut self.next_id,
                kind,
                &self.target,
                function,
                source,
                severity,
                status,
                explanation,
                remediation,
            ));
        }
    }
}

impl<'ast> Visit<'ast> for DiscoveryVisitor {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let previous = self.function.clone();
        let previous_async = self.async_function;
        self.function = Some(node.sig.ident.to_string());
        self.async_function = node.sig.asyncness.is_some();
        if node.sig.ident == self.target.as_str() {
            self.add(
                ObligationKind::Complexity,
                node.sig.ident.span().start().line,
                Severity::High,
                ObligationStatus::Unknown,
                "Complexity requires targeted runtime or MCA evidence",
                "Run targeted coverage/MCA and compare against the configured complexity model",
            );
        }
        if matches!(node.sig.safety, syn::Safety::Unsafe(_)) {
            self.add(
                ObligationKind::MemorySafety,
                node.sig.ident.span().start().line,
                Severity::Critical,
                ObligationStatus::Unknown,
                "An unsafe function requires proof of its safety contract",
                "Document and validate the unsafe contract with compiler/sanitizer evidence",
            );
        }
        if node.attrs.iter().any(|attribute| {
            attribute
                .path()
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "covopt_atomic")
        }) {
            self.add(
                ObligationKind::AtomicOrdering,
                node.sig.ident.span().start().line,
                Severity::Critical,
                ObligationStatus::Unknown,
                "covopt_atomic declares an ordering/liveness contract that requires bounded atomic and temporal evidence",
                "Run the atomic model and Temporal Trace IR checks for this contract",
            );
        }
        visit::visit_item_fn(self, node);
        self.function = previous;
        self.async_function = previous_async;
    }

    fn visit_expr_unsafe(&mut self, node: &'ast syn::ExprUnsafe) {
        self.add(
            ObligationKind::MemorySafety,
            node.unsafe_token.span().start().line,
            Severity::Critical,
            ObligationStatus::Unknown,
            "Unsafe block cannot be proven safe by AST shape alone",
            "Add an explicit safety invariant and run targeted sanitizer/Miri evidence",
        );
        visit::visit_expr_unsafe(self, node);
    }

    fn visit_expr_unary(&mut self, node: &'ast ExprUnary) {
        let expression = quote::quote!(#node).to_string().to_ascii_lowercase();
        let looks_like_raw_pointer = expression.contains("*const")
            || expression.contains("*mut")
            || expression.contains("as_ptr")
            || expression.contains("from_raw")
            || expression.contains("raw_ptr")
            || expression.contains("rawptr");
        if matches!(node.op, syn::UnOp::Deref(_)) && looks_like_raw_pointer {
            self.add(
                ObligationKind::MemorySafety,
                node.op.span().start().line,
                Severity::Critical,
                ObligationStatus::Unknown,
                "Raw-pointer dereference requires a validity, alignment, and lifetime proof",
                "Replace with a safe reference or document and validate the pointer invariant",
            );
        }
        visit::visit_expr_unary(self, node);
    }

    fn visit_expr_if(&mut self, node: &'ast syn::ExprIf) {
        self.add(
            ObligationKind::ControlFlow,
            node.if_token.span().start().line,
            Severity::Medium,
            ObligationStatus::Unknown,
            "A conditional branch requires execution evidence to establish its exercised paths",
            "Collect targeted coverage for both branch outcomes",
        );
        visit::visit_expr_if(self, node);
    }

    fn visit_expr_loop(&mut self, node: &'ast syn::ExprLoop) {
        self.loop_depth += 1;
        visit::visit_expr_loop(self, node);
        self.loop_depth -= 1;
    }

    fn visit_expr_for_loop(&mut self, node: &'ast syn::ExprForLoop) {
        self.loop_depth += 1;
        visit::visit_expr_for_loop(self, node);
        self.loop_depth -= 1;
    }

    fn visit_expr_while(&mut self, node: &'ast syn::ExprWhile) {
        self.loop_depth += 1;
        visit::visit_expr_while(self, node);
        self.loop_depth -= 1;
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        let name = node.method.to_string();
        let line = node.method.span().start().line;
        if matches!(name.as_str(), "get_unchecked" | "get_unchecked_mut") {
            self.add(
                ObligationKind::BoundsSafety,
                line,
                Severity::Critical,
                ObligationStatus::Unknown,
                "Unchecked indexing bypasses bounds checks",
                "Prove the index invariant or replace with checked access",
            );
        }
        if self.loop_depth > 0 && name == "clone" {
            self.add(
                ObligationKind::CpuOverhead,
                line,
                Severity::Medium,
                ObligationStatus::Modeled,
                "A clone occurs inside a loop and may add allocation/copy overhead",
                "Prefer borrowing or move allocation outside the hot loop",
            );
        }
        if self.loop_depth > 0 && matches!(name.as_str(), "lock" | "read" | "write") {
            self.add(
                ObligationKind::CpuOverhead,
                line,
                Severity::High,
                ObligationStatus::Modeled,
                "Locking or synchronization occurs inside a loop",
                "Measure contention and consider batching or lock-free alternatives",
            );
        }
        if self.async_function && matches!(name.as_str(), "lock" | "read" | "write") {
            self.add(
                ObligationKind::CpuOverhead,
                line,
                Severity::High,
                ObligationStatus::Modeled,
                "Potential blocking operation occurs in an async function",
                "Use an async-native primitive or spawn_blocking with a bounded pool",
            );
        }
        if name.starts_with("fetch_") || matches!(name.as_str(), "load" | "store" | "swap") {
            self.add(
                ObligationKind::AtomicOrdering,
                line,
                Severity::High,
                ObligationStatus::Modeled,
                "Atomic operation ordering was identified statically but semantic sufficiency is not proven",
                "Document the happens-before requirement and validate ordering under concurrency",
            );
        }
        visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        let name = match &*node.func {
            Expr::Path(path) => path.path.to_token_stream().to_string().replace(' ', ""),
            _ => String::new(),
        };
        let line = node.func.span().start().line;
        let (kind, explanation, remediation) = if name.contains("transmute") {
            (
                ObligationKind::AliasingSafety,
                "transmute can violate type, aliasing, and validity invariants",
                "Replace with a safe conversion or prove layout, validity, and aliasing invariants",
            )
        } else if name.contains("from_raw_parts") {
            (
                ObligationKind::AliasingSafety,
                "from_raw_parts constructs a view from raw memory",
                "Prove pointer validity, alignment, initialization, and lifetime",
            )
        } else if name.contains("assume_init") {
            (
                ObligationKind::MemorySafety,
                "assume_init bypasses initialization checks",
                "Prove every byte is initialized before assuming initialization",
            )
        } else if name.to_ascii_lowercase().contains("ffi")
            || name.to_ascii_lowercase().starts_with("extern")
        {
            (
                ObligationKind::ExternalCall,
                "Opaque external call has no Rust-side implementation evidence",
                "Record the external contract and collect a targeted profile or integration trace",
            )
        } else {
            visit::visit_expr_call(self, node);
            return;
        };
        self.add(
            kind,
            line,
            Severity::Critical,
            ObligationStatus::Unknown,
            explanation,
            remediation,
        );
        visit::visit_expr_call(self, node);
    }

    fn visit_expr_macro(&mut self, node: &'ast ExprMacro) {
        if self.loop_depth > 0 {
            let text = node.mac.path.to_token_stream().to_string();
            if matches!(text.as_str(), "println" | "print" | "format" | "vec") {
                self.add(
                    ObligationKind::CpuOverhead,
                    node.mac.path.span().start().line,
                    Severity::Medium,
                    ObligationStatus::Modeled,
                    "Allocation or synchronous I/O occurs inside a loop",
                    "Move formatting/I/O out of the hot loop or measure its cost",
                );
            }
        }
        visit::visit_expr_macro(self, node);
    }

    fn visit_item_static(&mut self, node: &'ast syn::ItemStatic) {
        if node.to_token_stream().to_string().contains("static mut") {
            self.add(
                ObligationKind::AliasingSafety,
                node.static_token.span().start().line,
                Severity::Critical,
                ObligationStatus::Unknown,
                "Mutable global state requires aliasing and synchronization proof",
                "Replace with synchronized interior mutability or document exclusive access",
            );
        }
        visit::visit_item_static(self, node);
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        if node.unsafety.is_some() {
            self.add(
                ObligationKind::MemorySafety,
                node.impl_token.span().start().line,
                Severity::Critical,
                ObligationStatus::Unknown,
                "Unsafe impl requires proof that the trait contract is upheld",
                "Document Send/Sync or trait invariants and validate with targeted tests",
            );
        }
        visit::visit_item_impl(self, node);
    }

    fn visit_item_foreign_mod(&mut self, node: &'ast syn::ItemForeignMod) {
        self.add(
            ObligationKind::FfiSafety,
            node.abi.span().start().line,
            Severity::Critical,
            ObligationStatus::Unknown,
            "FFI boundary has no Rust-side proof of ABI, pointer, or ownership contracts",
            "Document the foreign contract and validate it with compiler/sanitizer evidence",
        );
        visit::visit_item_foreign_mod(self, node);
    }
}

pub fn discover_obligations(file: &Path, target: &str) -> Vec<Obligation> {
    let Ok(content) = std::fs::read_to_string(file) else {
        let mut id = 0;
        return vec![make_obligation(
            &mut id,
            ObligationKind::Complexity,
            target,
            Some(target.to_string()),
            None,
            Severity::High,
            ObligationStatus::Unknown,
            "Target source file could not be read",
            "Restore the source file and rerun assurance analysis",
        )];
    };
    let Ok(ast) = syn::parse_file(&content) else {
        let mut id = 0;
        return vec![make_obligation(
            &mut id,
            ObligationKind::ControlFlow,
            target,
            Some(target.to_string()),
            Some(SourceLocation {
                file: file.display().to_string(),
                line: 1,
            }),
            Severity::High,
            ObligationStatus::Unknown,
            "Target source file could not be parsed",
            "Fix the syntax error before relying on assurance evidence",
        )];
    };
    let mut visitor = DiscoveryVisitor {
        file: file.display().to_string(),
        target: target.to_string(),
        function: None,
        async_function: false,
        loop_depth: 0,
        obligations: Vec::new(),
        next_id: 0,
        seen: HashSet::new(),
    };
    visitor.visit_file(&ast);
    if visitor.obligations.is_empty() {
        visitor.obligations.push(make_obligation(
            &mut visitor.next_id,
            ObligationKind::Complexity,
            target,
            Some(target.to_string()),
            Some(SourceLocation {
                file: file.display().to_string(),
                line: 1,
            }),
            Severity::High,
            ObligationStatus::Unknown,
            "No static proof was discovered for the configured target",
            "Run targeted coverage or MCA evidence for the target",
        ));
    }
    visitor.obligations
}

pub fn find_target_source(target: &str) -> Option<PathBuf> {
    crate::static_analysis::find_covopt_test_metadata(target)
        .map(|(_, _, _, path)| path)
        .or_else(|| {
            crate::static_analysis::find_covopt_target_metadata(target)
                .map(|metadata| metadata.file)
        })
}

pub fn discover_target_obligations(target: &str) -> Vec<Obligation> {
    let target_source = find_target_source(target);
    let mut obligations = target_source
        .as_ref()
        .map(|source| discover_obligations(source, target))
        .unwrap_or_else(|| discover_obligations(Path::new("__missing_target_source__.rs"), target));
    if let Some(source) = target_source {
        obligations.extend(obligations_from_structured_findings(
            target,
            crate::dataflow::analyze_file_structured(&source),
        ));
        obligations.extend(obligations_from_findings(
            target,
            crate::static_analysis::analyze_aerospace_grade_structured(&source),
        ));
    }
    obligations
}

pub fn format_legacy_findings(report: &AssuranceReport) -> Vec<String> {
    report
        .obligations
        .iter()
        .filter(|obligation| {
            matches!(
                obligation.status,
                ObligationStatus::Unknown | ObligationStatus::Failed
            )
        })
        .map(|obligation| {
            format!(
                "[Assurance {:?}/{:?}] {}{}",
                obligation.kind,
                obligation.status,
                obligation.explanation,
                obligation
                    .source
                    .as_ref()
                    .map(|source| format!(" ({}:{})", source.file, source.line))
                    .unwrap_or_default()
            )
        })
        .collect()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum PlannerKind {
    Greedy,
    #[default]
    Hybrid,
    Exact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerPolicy {
    pub overall_threshold: f64,
    pub critical_threshold: f64,
    pub performance_threshold: f64,
    pub max_time_ms: Option<u64>,
    #[serde(default)]
    pub allowed_external_tools: Vec<String>,
    #[serde(default)]
    pub static_only: bool,
    #[serde(default)]
    pub strict: bool,
    #[serde(default)]
    pub planner: PlannerKind,
    #[serde(default = "default_exact_threshold")]
    pub exact_threshold: usize,
    #[serde(default = "default_planner_timeout")]
    pub timeout_ms: u64,
    #[serde(default = "default_flakiness_penalty")]
    pub flakiness_penalty: f64,
    #[serde(default = "default_external_tool_penalty")]
    pub external_tool_penalty: f64,
}

fn default_exact_threshold() -> usize {
    18
}

fn default_planner_timeout() -> u64 {
    2_000
}

fn default_flakiness_penalty() -> f64 {
    0.25
}

fn default_external_tool_penalty() -> f64 {
    0.10
}

impl Default for PlannerPolicy {
    fn default() -> Self {
        Self {
            overall_threshold: DEFAULT_EVIDENCE_THRESHOLD,
            critical_threshold: 1.0,
            performance_threshold: DEFAULT_EVIDENCE_THRESHOLD,
            max_time_ms: Some(30_000),
            allowed_external_tools: Vec::new(),
            static_only: false,
            strict: false,
            planner: PlannerKind::Hybrid,
            exact_threshold: default_exact_threshold(),
            timeout_ms: default_planner_timeout(),
            flakiness_penalty: default_flakiness_penalty(),
            external_tool_penalty: default_external_tool_penalty(),
        }
    }
}

impl PlannerPolicy {
    pub fn for_assurance(policy: AssurancePolicy) -> Self {
        let mut planner = Self {
            static_only: matches!(policy, AssurancePolicy::Static),
            strict: matches!(policy, AssurancePolicy::Strict),
            ..Self::default()
        };
        if planner.strict {
            planner.critical_threshold = 1.0;
            planner.overall_threshold = DEFAULT_EVIDENCE_THRESHOLD;
        }
        planner
    }
}

#[derive(Debug, Clone, Default)]
pub struct PlanningContext {
    pub target: Option<String>,
    pub package: Option<String>,
    pub target_cpu: Option<String>,
    pub available_tools: HashSet<String>,
    pub metadata: HashMap<String, String>,
}

impl PlanningContext {
    pub fn with_tools(tools: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            available_tools: tools.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }

    pub fn tool_available(&self, tool: &str) -> bool {
        self.available_tools.contains(tool)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ExecutionContext {
    pub target: Option<String>,
    pub package: Option<String>,
    pub target_cpu: Option<String>,
    pub available_tools: HashSet<String>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionTelemetry {
    pub cache_key: String,
    pub target: String,
    pub action: EvidenceActionId,
    pub provider: ProviderId,
    pub source_hash: String,
    pub toolchain_version: String,
    pub target_cpu: String,
    pub config_fingerprint: String,
    pub runtime_ms: u64,
    pub success: bool,
    pub flakiness: f64,
    #[serde(default)]
    pub obligations_covered: Vec<ObligationId>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CostCache {
    pub observations: HashMap<String, Vec<u64>>,
}

impl CostCache {
    pub fn record(&mut self, telemetry: &ActionTelemetry) {
        if telemetry.success {
            self.observations
                .entry(telemetry.cache_key.clone())
                .or_default()
                .push(telemetry.runtime_ms);
        }
    }

    pub fn estimate(&self, key: &str) -> Option<u64> {
        let mut values = self.observations.get(key)?.clone();
        if values.is_empty() {
            return None;
        }
        values.sort_unstable();
        Some(values[values.len() / 2])
    }

    pub fn apply_to_action(&self, action: &mut EvidenceAction, key: &str) {
        if let Some(estimate) = self.estimate(key) {
            action.estimated_cost_ms = estimate;
        }
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
        serde_json::from_str(&content).map_err(|error| error.to_string())
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let content = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        std::fs::write(path, content).map_err(|error| error.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceAction {
    pub id: EvidenceActionId,
    pub provider: ProviderId,
    pub covers: Vec<ObligationId>,
    pub estimated_cost_ms: u64,
    #[serde(default)]
    pub setup_cost_ms: u64,
    pub confidence: f64,
    pub flakiness: f64,
    #[serde(default)]
    pub requires: Vec<EvidenceActionId>,
    pub setup_group: Option<String>,
    #[serde(default)]
    pub required_tools: Vec<String>,
    pub available: bool,
    #[serde(default)]
    pub mutually_exclusive_with: Vec<EvidenceActionId>,
    #[serde(default = "default_action_status")]
    pub result_status: ObligationStatus,
    #[serde(default)]
    pub description: String,
}

fn default_action_status() -> ObligationStatus {
    ObligationStatus::Observed
}

impl EvidenceAction {
    pub fn new(
        id: impl Into<String>,
        provider: impl Into<String>,
        covers: Vec<ObligationId>,
        estimated_cost_ms: u64,
        confidence: f64,
    ) -> Self {
        Self {
            id: EvidenceActionId(id.into()),
            provider: ProviderId(provider.into()),
            covers,
            estimated_cost_ms,
            setup_cost_ms: 0,
            confidence,
            flakiness: 0.0,
            requires: Vec::new(),
            setup_group: None,
            required_tools: Vec::new(),
            available: true,
            mutually_exclusive_with: Vec::new(),
            result_status: ObligationStatus::Observed,
            description: String::new(),
        }
    }

    fn effective_status(&self) -> ObligationStatus {
        if self.result_status == ObligationStatus::Unknown {
            if self.confidence >= 0.90 {
                ObligationStatus::Observed
            } else {
                ObligationStatus::Modeled
            }
        } else {
            self.result_status
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceResult {
    pub action_id: EvidenceActionId,
    pub status: ObligationStatus,
    pub summary: String,
    #[serde(default)]
    pub covered: Vec<ObligationId>,
    #[serde(default)]
    pub actual_cost_ms: u64,
    #[serde(default)]
    pub confidence: f64,
    #[serde(default)]
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EvidenceError {
    Unavailable(String),
    Failed(String),
    InvalidAction(String),
}

impl std::fmt::Display for EvidenceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(message) | Self::Failed(message) | Self::InvalidAction(message) => {
                message.fmt(formatter)
            }
        }
    }
}

impl std::error::Error for EvidenceError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRejection {
    pub action_id: EvidenceActionId,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlanStatus {
    Feasible,
    Partial,
    Infeasible,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfeasibleObligation {
    pub obligation_id: ObligationId,
    pub reason: String,
    pub critical: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidencePlan {
    pub status: PlanStatus,
    pub selected_actions: Vec<EvidenceActionId>,
    #[serde(default)]
    pub selected_action_details: Vec<EvidenceAction>,
    #[serde(default)]
    pub candidate_actions: Vec<EvidenceAction>,
    #[serde(default)]
    pub rejected_actions: Vec<ActionRejection>,
    pub coverage_before: EvidenceCoverage,
    pub expected_coverage: EvidenceCoverage,
    #[serde(default)]
    pub actual_coverage: Option<EvidenceCoverage>,
    pub estimated_cost_ms: u64,
    #[serde(default)]
    pub actual_cost_ms: Option<u64>,
    #[serde(default)]
    pub infeasible_obligations: Vec<InfeasibleObligation>,
    #[serde(default)]
    pub validator_errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanValidation {
    pub valid: bool,
    pub errors: Vec<String>,
    pub coverage: EvidenceCoverage,
    pub estimated_cost_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanOutcome {
    pub plan: EvidencePlan,
    pub validation: PlanValidation,
    pub timed_out: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionReport {
    pub obligations: Vec<Obligation>,
    pub results: Vec<EvidenceResult>,
    pub failed_actions: Vec<ActionRejection>,
    pub replanned: bool,
    pub expected_cost_ms: u64,
    pub actual_cost_ms: u64,
    #[serde(default)]
    pub telemetry: Vec<ActionTelemetry>,
}

fn required_tool_for_provider(provider: EvidenceProviderKind) -> Option<&'static str> {
    match provider {
        EvidenceProviderKind::StaticAst => None,
        EvidenceProviderKind::Compiler => Some("rustc"),
        EvidenceProviderKind::Mca => Some("llvm-mca"),
        EvidenceProviderKind::Coverage => Some("llvm-cov"),
        EvidenceProviderKind::Test => Some("cargo"),
        EvidenceProviderKind::Sanitizer => Some("sanitizer"),
        EvidenceProviderKind::Profiler => Some("samply"),
        EvidenceProviderKind::AtomicModel => None,
        EvidenceProviderKind::Temporal
        | EvidenceProviderKind::Relational
        | EvidenceProviderKind::Adversarial => None,
    }
}

fn discover_actions_for_provider(
    provider_id: ProviderId,
    kind: EvidenceProviderKind,
    obligations: &[Obligation],
    context: &PlanningContext,
) -> Vec<EvidenceAction> {
    let ceiling = provider_status_ceiling(kind);
    let covers: Vec<_> = obligations
        .iter()
        .filter(|obligation| {
            obligation.acceptable_evidence_kinds.contains(&kind)
                && ceiling.confidence() > obligation.status.confidence()
        })
        .map(|obligation| obligation.id.clone())
        .collect();
    if covers.is_empty() {
        return Vec::new();
    }

    let (cost, setup_cost, confidence, flakiness, setup_group, status, description) = match kind {
        EvidenceProviderKind::StaticAst => (
            100,
            0,
            0.75,
            0.0,
            None,
            ObligationStatus::Modeled,
            "Static AST analysis",
        ),
        EvidenceProviderKind::Compiler => (
            1_500,
            1_000,
            0.85,
            0.02,
            Some("workspace-compile".to_string()),
            ObligationStatus::Modeled,
            "Compiler diagnostics (not a proof of unsafe contracts)",
        ),
        EvidenceProviderKind::Mca => (
            250,
            3_000,
            0.95,
            0.03,
            Some("release-asm".to_string()),
            ObligationStatus::Observed,
            "LLVM-MCA instruction throughput analysis",
        ),
        EvidenceProviderKind::Coverage => (
            1_000,
            8_000,
            0.95,
            0.04,
            Some("instrumented-build".to_string()),
            ObligationStatus::Observed,
            "Targeted line and branch coverage",
        ),
        EvidenceProviderKind::Test => (
            1_200,
            4_000,
            0.90,
            0.08,
            Some("workspace-tests".to_string()),
            ObligationStatus::Observed,
            "Targeted test execution",
        ),
        EvidenceProviderKind::Sanitizer => (
            3_000,
            5_000,
            0.98,
            0.12,
            Some("sanitizer-build".to_string()),
            ObligationStatus::Observed,
            "Sanitizer-backed safety execution",
        ),
        EvidenceProviderKind::Profiler => (
            2_500,
            3_000,
            0.85,
            0.15,
            Some("profile-build".to_string()),
            ObligationStatus::Observed,
            "Targeted CPU/profile evidence",
        ),
        EvidenceProviderKind::AtomicModel => (
            2_000,
            1_000,
            0.75,
            0.05,
            Some("atomic-model".to_string()),
            ObligationStatus::Modeled,
            "Bounded atomic model checking (not a proof)",
        ),
        EvidenceProviderKind::Temporal => (
            1_000,
            250,
            0.80,
            0.02,
            Some("temporal-contract".to_string()),
            ObligationStatus::Modeled,
            "Bounded temporal contract checking (not a proof)",
        ),
        EvidenceProviderKind::Relational => (
            1_000,
            250,
            0.85,
            0.02,
            Some("relational-trace".to_string()),
            ObligationStatus::Modeled,
            "Bounded relational trace comparison",
        ),
        EvidenceProviderKind::Adversarial => (
            2_500,
            500,
            0.80,
            0.05,
            Some("adversarial-search".to_string()),
            ObligationStatus::Modeled,
            "Bounded adversarial environment search (not a proof)",
        ),
    };
    let required_tools = required_tool_for_provider(kind)
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let available = required_tools
        .iter()
        .all(|tool| context.tool_available(tool));
    let suffix = provider_id.0.to_ascii_lowercase();
    vec![EvidenceAction {
        id: EvidenceActionId(format!("{}-target", suffix)),
        provider: provider_id,
        covers,
        estimated_cost_ms: cost,
        setup_cost_ms: setup_cost,
        confidence,
        flakiness,
        requires: Vec::new(),
        setup_group,
        required_tools,
        available,
        mutually_exclusive_with: Vec::new(),
        result_status: status,
        description: description.to_string(),
    }]
}

pub fn discover_evidence_actions(
    providers: &[Box<dyn EvidenceProvider>],
    obligations: &[Obligation],
    context: &PlanningContext,
) -> Vec<EvidenceAction> {
    providers
        .iter()
        .flat_map(|provider| provider.discover_actions(obligations, context))
        .collect()
}

pub struct PlanValidator;

impl PlanValidator {
    pub fn validate(
        obligations: &[Obligation],
        actions: &[EvidenceAction],
        selected: &[EvidenceActionId],
        policy: &PlannerPolicy,
        context: &PlanningContext,
    ) -> PlanValidation {
        let action_map: HashMap<_, _> = actions.iter().map(|action| (&action.id, action)).collect();
        let mut errors = Vec::new();
        let selected_set: HashSet<_> = selected.iter().collect();
        let mut cost = 0;
        let mut groups = HashSet::new();
        for id in selected {
            let Some(action) = action_map.get(id) else {
                errors.push(format!("selected action {} is not a candidate", id));
                continue;
            };
            if !action.available {
                errors.push(format!("selected action {} is unavailable", id));
            }
            if policy.static_only && action.provider.0 != "StaticAst" {
                errors.push(format!("static-only policy forbids {}", id));
            }
            if !policy.allowed_external_tools.is_empty()
                && action
                    .required_tools
                    .iter()
                    .any(|tool| !policy.allowed_external_tools.contains(tool))
            {
                errors.push(format!("selected action {} requires a forbidden tool", id));
            }
            if action
                .required_tools
                .iter()
                .any(|tool| !context.tool_available(tool))
            {
                errors.push(format!("required tool is unavailable for {}", id));
            }
            for prerequisite in &action.requires {
                if !selected_set.contains(prerequisite) {
                    errors.push(format!("{} is missing prerequisite {}", id, prerequisite));
                }
            }
            if action
                .mutually_exclusive_with
                .iter()
                .any(|other| selected_set.contains(other))
            {
                errors.push(format!("{} conflicts with a selected action", id));
            }
            if action
                .setup_group
                .as_ref()
                .is_none_or(|group| groups.insert(group.clone()))
            {
                cost += action.setup_cost_ms;
            }
            cost += action.estimated_cost_ms;
        }

        let simulated = simulate_actions(obligations, actions, selected);
        let coverage = calculate_coverage(&simulated);
        if cost_exceeds(policy, cost) {
            errors.push(format!(
                "estimated cost {}ms exceeds the {}ms budget",
                cost,
                policy.max_time_ms.unwrap_or_default()
            ));
        }
        if !planner_policy_passes(policy, &coverage) {
            errors.push("selected plan does not meet policy thresholds".to_string());
        }
        PlanValidation {
            valid: errors.is_empty(),
            errors,
            coverage,
            estimated_cost_ms: cost,
        }
    }
}

fn cost_exceeds(policy: &PlannerPolicy, cost: u64) -> bool {
    policy.max_time_ms.is_some_and(|budget| cost > budget)
}

fn simulate_actions(
    obligations: &[Obligation],
    actions: &[EvidenceAction],
    selected: &[EvidenceActionId],
) -> Vec<Obligation> {
    let action_map: HashMap<_, _> = actions.iter().map(|action| (&action.id, action)).collect();
    let mut result = obligations.to_vec();
    let by_id: HashMap<_, _> = result
        .iter_mut()
        .enumerate()
        .map(|(index, obligation)| (obligation.id.clone(), index))
        .collect();
    for selected_id in selected {
        let Some(action) = action_map.get(selected_id) else {
            continue;
        };
        let status = action.effective_status();
        for obligation_id in &action.covers {
            if let Some(index) = by_id.get(obligation_id)
                && !matches!(result[*index].status, ObligationStatus::Failed)
                && status.confidence() > result[*index].status.confidence()
            {
                result[*index].status = status;
            }
        }
    }
    result
}

fn planner_policy_passes(policy: &PlannerPolicy, coverage: &EvidenceCoverage) -> bool {
    coverage.failed_obligation_count == 0
        && coverage.critical_safety_percent >= policy.critical_threshold * 100.0
        && coverage.overall_percent >= policy.overall_threshold * 100.0
        && coverage.performance_percent >= policy.performance_threshold * 100.0
}

fn action_cost(actions: &[EvidenceAction], selected: &[EvidenceActionId]) -> u64 {
    let map: HashMap<_, _> = actions.iter().map(|action| (&action.id, action)).collect();
    let mut groups = HashSet::new();
    selected
        .iter()
        .filter_map(|id| map.get(id))
        .map(|action| {
            let setup = action
                .setup_group
                .as_ref()
                .filter(|group| groups.insert((*group).clone()))
                .map(|_| action.setup_cost_ms)
                .unwrap_or(0);
            setup + action.estimated_cost_ms
        })
        .sum()
}

fn expand_prerequisites(
    id: &EvidenceActionId,
    map: &HashMap<&EvidenceActionId, &EvidenceAction>,
    selected: &mut Vec<EvidenceActionId>,
) -> Result<(), String> {
    let Some(action) = map.get(id) else {
        return Err(format!("missing prerequisite action {}", id));
    };
    for prerequisite in &action.requires {
        expand_prerequisites(prerequisite, map, selected)?;
    }
    if !selected.contains(id) {
        selected.push(id.clone());
    }
    Ok(())
}

fn preprocess_actions(
    obligations: &[Obligation],
    actions: &[EvidenceAction],
    policy: &PlannerPolicy,
    context: &PlanningContext,
) -> (Vec<EvidenceAction>, Vec<ActionRejection>) {
    let obligation_ids: HashSet<_> = obligations
        .iter()
        .map(|obligation| &obligation.id)
        .collect();
    let prerequisite_ids: HashSet<_> = actions
        .iter()
        .flat_map(|action| action.requires.iter().cloned())
        .collect();
    let mut accepted = Vec::new();
    let mut rejected = Vec::new();
    let mut seen = HashSet::new();
    for action in actions {
        let reason = if !action.available {
            Some("provider or required tool unavailable".to_string())
        } else if policy.static_only && action.provider.0 != "StaticAst" {
            Some("static-only policy".to_string())
        } else if !policy.allowed_external_tools.is_empty()
            && action
                .required_tools
                .iter()
                .any(|tool| !policy.allowed_external_tools.contains(tool))
        {
            Some("external tool is not allowed by policy".to_string())
        } else if action
            .required_tools
            .iter()
            .any(|tool| !context.tool_available(tool))
        {
            Some("required tool unavailable".to_string())
        } else if !seen.insert(action.id.clone()) {
            Some("duplicate action id".to_string())
        } else if action.covers.iter().all(|id| !obligation_ids.contains(id))
            && !prerequisite_ids.contains(&action.id)
        {
            Some("action covers no known obligation".to_string())
        } else {
            None
        };
        if let Some(reason) = reason {
            rejected.push(ActionRejection {
                action_id: action.id.clone(),
                reason,
            });
        } else {
            accepted.push(action.clone());
        }
    }

    let mut dominated = HashSet::new();
    for (index, left) in accepted.iter().enumerate() {
        for (other_index, right) in accepted.iter().enumerate() {
            if index == other_index {
                continue;
            }
            let left_covers: HashSet<_> = left.covers.iter().collect();
            let right_covers: HashSet<_> = right.covers.iter().collect();
            let dominates = left_covers.is_superset(&right_covers)
                && left.estimated_cost_ms + left.setup_cost_ms
                    <= right.estimated_cost_ms + right.setup_cost_ms
                && left.confidence >= right.confidence
                && left.flakiness <= right.flakiness
                && (left_covers.len() > right_covers.len()
                    || left.estimated_cost_ms + left.setup_cost_ms
                        < right.estimated_cost_ms + right.setup_cost_ms
                    || left.confidence > right.confidence
                    || left.flakiness < right.flakiness);
            if dominates {
                dominated.insert(other_index);
            }
        }
    }
    let mut kept = Vec::new();
    for (index, action) in accepted.into_iter().enumerate() {
        if dominated.contains(&index) {
            rejected.push(ActionRejection {
                action_id: action.id,
                reason: "dominated by a cheaper or stronger action".to_string(),
            });
        } else {
            kept.push(action);
        }
    }
    (kept, rejected)
}

fn action_score(
    obligations: &[Obligation],
    actions: &[EvidenceAction],
    selected: &[EvidenceActionId],
    candidate: &EvidenceAction,
    policy: &PlannerPolicy,
) -> f64 {
    let before = calculate_coverage(&simulate_actions(obligations, actions, selected));
    let mut with_candidate = selected.to_vec();
    with_candidate.push(candidate.id.clone());
    let after = calculate_coverage(&simulate_actions(obligations, actions, &with_candidate));
    let benefit = (after.overall_percent - before.overall_percent).max(0.0)
        + (after.critical_safety_percent - before.critical_safety_percent).max(0.0) * 3.0
        + (after.performance_percent - before.performance_percent).max(0.0);
    let incremental_cost = action_cost(actions, &with_candidate)
        .saturating_sub(action_cost(actions, selected))
        .max(1) as f64;
    let penalties = 1.0
        + candidate.flakiness * policy.flakiness_penalty
        + if candidate.required_tools.is_empty() {
            0.0
        } else {
            policy.external_tool_penalty
        };
    benefit * candidate.confidence / (incremental_cost * penalties)
}

fn better_plan(
    left: &[EvidenceActionId],
    right: &[EvidenceActionId],
    obligations: &[Obligation],
    actions: &[EvidenceAction],
    policy: &PlannerPolicy,
    context: &PlanningContext,
) -> bool {
    let left_validation = PlanValidator::validate(obligations, actions, left, policy, context);
    let right_validation = PlanValidator::validate(obligations, actions, right, policy, context);
    if left_validation.valid != right_validation.valid {
        return left_validation.valid;
    }
    let left_coverage = &left_validation.coverage;
    let right_coverage = &right_validation.coverage;
    if (left_coverage.critical_safety_percent - right_coverage.critical_safety_percent).abs()
        > f64::EPSILON
    {
        return left_coverage.critical_safety_percent > right_coverage.critical_safety_percent;
    }
    if (left_coverage.overall_percent - right_coverage.overall_percent).abs() > f64::EPSILON {
        return left_coverage.overall_percent > right_coverage.overall_percent;
    }
    if left_validation.estimated_cost_ms != right_validation.estimated_cost_ms {
        return left_validation.estimated_cost_ms < right_validation.estimated_cost_ms;
    }
    let left_external = left
        .iter()
        .filter_map(|id| actions.iter().find(|action| &action.id == id))
        .filter(|action| !action.required_tools.is_empty())
        .count();
    let right_external = right
        .iter()
        .filter_map(|id| actions.iter().find(|action| &action.id == id))
        .filter(|action| !action.required_tools.is_empty())
        .count();
    if left_external != right_external {
        return left_external < right_external;
    }
    left.iter().map(ToString::to_string).collect::<Vec<_>>()
        < right.iter().map(ToString::to_string).collect::<Vec<_>>()
}

pub struct EvidencePlanner {
    pub policy: PlannerPolicy,
    pub cost_cache: Option<CostCache>,
}

impl EvidencePlanner {
    pub fn new(policy: PlannerPolicy) -> Self {
        Self {
            policy,
            cost_cache: None,
        }
    }

    pub fn with_cost_cache(policy: PlannerPolicy, cost_cache: CostCache) -> Self {
        Self {
            policy,
            cost_cache: Some(cost_cache),
        }
    }

    pub fn plan(
        &self,
        obligations: &[Obligation],
        actions: &[EvidenceAction],
        context: &PlanningContext,
    ) -> PlanOutcome {
        let coverage_before = calculate_coverage(obligations);
        let mut costed_actions = actions.to_vec();
        if let Some(cache) = &self.cost_cache {
            for action in &mut costed_actions {
                let key = action.id.to_string();
                cache.apply_to_action(action, &key);
            }
        }
        let (candidates, rejected_actions) =
            preprocess_actions(obligations, &costed_actions, &self.policy, context);
        let deadline = Instant::now() + Duration::from_millis(self.policy.timeout_ms);
        let mut selected = self.greedy(obligations, &candidates, context, deadline);
        let mut timed_out = false;
        if matches!(
            self.policy.planner,
            PlannerKind::Hybrid | PlannerKind::Exact
        ) && candidates.len() <= self.policy.exact_threshold
        {
            let (exact, exact_timed_out) = exact_search(
                obligations,
                &candidates,
                &selected,
                &self.policy,
                context,
                deadline,
            );
            timed_out = exact_timed_out;
            if better_plan(
                &exact,
                &selected,
                obligations,
                &candidates,
                &self.policy,
                context,
            ) {
                selected = exact;
            }
        }
        if matches!(
            self.policy.planner,
            PlannerKind::Hybrid | PlannerKind::Greedy
        ) {
            selected = local_improve(
                selected,
                obligations,
                &candidates,
                &self.policy,
                context,
                deadline,
            );
        }
        let validation =
            PlanValidator::validate(obligations, &candidates, &selected, &self.policy, context);
        let expected_coverage = validation.coverage.clone();
        let required_confidence = |obligation: &Obligation| {
            if obligation.kind.is_safety() && matches!(obligation.severity, Severity::Critical) {
                self.policy.critical_threshold
            } else if obligation.kind.is_performance() {
                self.policy.performance_threshold
            } else {
                self.policy.overall_threshold
            }
        };
        let infeasible_obligations = obligations
            .iter()
            .filter(|obligation| {
                if obligation.status.confidence() >= required_confidence(obligation) {
                    return false;
                }
                let has_selected_cover = selected.iter().any(|action_id| {
                    candidates.iter().any(|action| {
                        &action.id == action_id && action.covers.contains(&obligation.id)
                    })
                });
                let has_resolving_action = candidates.iter().any(|action| {
                    action.covers.contains(&obligation.id)
                        && action.effective_status().confidence() >= required_confidence(obligation)
                });
                !has_selected_cover || !has_resolving_action
            })
            .map(|obligation| InfeasibleObligation {
                obligation_id: obligation.id.clone(),
                reason: if candidates.iter().any(|action| {
                    action.covers.contains(&obligation.id)
                        && action.effective_status().confidence() >= required_confidence(obligation)
                }) {
                    "no selected action covers this obligation".to_string()
                } else {
                    "no available provider can meet the required evidence confidence".to_string()
                },
                critical: obligation.kind.is_safety()
                    && matches!(obligation.severity, Severity::Critical),
            })
            .collect::<Vec<_>>();
        let status = if validation.valid {
            PlanStatus::Feasible
        } else if expected_coverage.overall_percent > coverage_before.overall_percent {
            PlanStatus::Partial
        } else {
            PlanStatus::Infeasible
        };
        let selected_action_details = selected
            .iter()
            .filter_map(|id| candidates.iter().find(|action| &action.id == id).cloned())
            .collect::<Vec<_>>();
        let mut validator_errors = validation.errors.clone();
        for obligation in &infeasible_obligations {
            validator_errors.push(format!(
                "{}: {}",
                obligation.obligation_id, obligation.reason
            ));
        }
        PlanOutcome {
            plan: EvidencePlan {
                status,
                selected_actions: selected,
                selected_action_details,
                candidate_actions: candidates,
                rejected_actions,
                coverage_before,
                expected_coverage,
                actual_coverage: None,
                estimated_cost_ms: validation.estimated_cost_ms,
                actual_cost_ms: None,
                infeasible_obligations,
                validator_errors,
            },
            validation,
            timed_out,
        }
    }

    fn greedy(
        &self,
        obligations: &[Obligation],
        actions: &[EvidenceAction],
        context: &PlanningContext,
        deadline: Instant,
    ) -> Vec<EvidenceActionId> {
        let map: HashMap<_, _> = actions.iter().map(|action| (&action.id, action)).collect();
        let mut selected = Vec::new();
        loop {
            if Instant::now() >= deadline {
                break;
            }
            let current = simulate_actions(obligations, actions, &selected);
            let current_coverage = calculate_coverage(&current);
            if planner_policy_passes(&self.policy, &current_coverage) {
                break;
            }
            let mut best: Option<(&EvidenceAction, f64)> = None;
            for action in actions {
                if selected.contains(&action.id)
                    || action
                        .mutually_exclusive_with
                        .iter()
                        .any(|id| selected.contains(id))
                {
                    continue;
                }
                let mut with_action = selected.clone();
                if expand_prerequisites(&action.id, &map, &mut with_action).is_err() {
                    continue;
                }
                if action_cost(actions, &with_action) > self.policy.max_time_ms.unwrap_or(u64::MAX)
                {
                    continue;
                }
                let score = action_score(obligations, actions, &selected, action, &self.policy);
                let improves_score = best.as_ref().is_none_or(|(best_action, best_score)| {
                    score > *best_score || (score == *best_score && action.id < best_action.id)
                });
                if score > 0.0 && improves_score {
                    best = Some((action, score));
                }
            }
            let Some((action, _)) = best else {
                break;
            };
            if expand_prerequisites(&action.id, &map, &mut selected).is_err() {
                break;
            }
            if selected.iter().any(|id| {
                map.get(id).is_some_and(|selected_action| {
                    selected_action
                        .mutually_exclusive_with
                        .iter()
                        .any(|other| selected.contains(other))
                })
            }) {
                selected.retain(|id| id != &action.id);
                break;
            }
        }
        let _ = context;
        selected
    }
}

fn exact_search(
    obligations: &[Obligation],
    actions: &[EvidenceAction],
    greedy_upper_bound: &[EvidenceActionId],
    policy: &PlannerPolicy,
    context: &PlanningContext,
    deadline: Instant,
) -> (Vec<EvidenceActionId>, bool) {
    let mut best = greedy_upper_bound.to_vec();
    let mut timed_out = false;
    exact_recurse(
        0,
        actions,
        &mut Vec::new(),
        &mut best,
        obligations,
        policy,
        context,
        deadline,
        &mut timed_out,
    );
    (best, timed_out)
}

#[allow(clippy::too_many_arguments)]
fn exact_recurse(
    index: usize,
    actions: &[EvidenceAction],
    selected: &mut Vec<EvidenceActionId>,
    best: &mut Vec<EvidenceActionId>,
    obligations: &[Obligation],
    policy: &PlannerPolicy,
    context: &PlanningContext,
    deadline: Instant,
    timed_out: &mut bool,
) {
    if Instant::now() >= deadline {
        *timed_out = true;
        return;
    }
    if better_plan(selected, best, obligations, actions, policy, context) {
        *best = selected.clone();
    }
    if index >= actions.len() {
        return;
    }
    let best_coverage =
        PlanValidator::validate(obligations, actions, best, policy, context).coverage;
    let optimistic_ids = selected
        .iter()
        .cloned()
        .chain(actions[index..].iter().map(|action| action.id.clone()))
        .collect::<Vec<_>>();
    let optimistic =
        PlanValidator::validate(obligations, actions, &optimistic_ids, policy, context).coverage;
    if optimistic.critical_safety_percent < best_coverage.critical_safety_percent
        || (optimistic.critical_safety_percent - best_coverage.critical_safety_percent).abs()
            < f64::EPSILON
            && optimistic.overall_percent < best_coverage.overall_percent
    {
        return;
    }

    exact_recurse(
        index + 1,
        actions,
        selected,
        best,
        obligations,
        policy,
        context,
        deadline,
        timed_out,
    );
    let mut included = selected.clone();
    let action_map: HashMap<_, _> = actions.iter().map(|action| (&action.id, action)).collect();
    if expand_prerequisites(&actions[index].id, &action_map, &mut included).is_ok() {
        exact_recurse(
            index + 1,
            actions,
            &mut included,
            best,
            obligations,
            policy,
            context,
            deadline,
            timed_out,
        );
    }
}

fn local_improve(
    mut selected: Vec<EvidenceActionId>,
    obligations: &[Obligation],
    actions: &[EvidenceAction],
    policy: &PlannerPolicy,
    context: &PlanningContext,
    deadline: Instant,
) -> Vec<EvidenceActionId> {
    let mut changed = true;
    while changed && Instant::now() < deadline {
        changed = false;
        for index in 0..selected.len() {
            let mut candidate = selected.clone();
            candidate.remove(index);
            if better_plan(&candidate, &selected, obligations, actions, policy, context) {
                selected = candidate;
                changed = true;
                break;
            }
        }
        if changed {
            continue;
        }

        let mut best_replacement = selected.clone();
        for remove_index in 0..selected.len() {
            if Instant::now() >= deadline {
                break;
            }
            let base = selected
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != remove_index)
                .map(|(_, id)| id.clone())
                .collect::<Vec<_>>();
            for (first_index, first) in actions.iter().enumerate() {
                if base.contains(&first.id) {
                    continue;
                }
                let mut one_for_one = base.clone();
                one_for_one.push(first.id.clone());
                if better_plan(
                    &one_for_one,
                    &best_replacement,
                    obligations,
                    actions,
                    policy,
                    context,
                ) {
                    best_replacement = one_for_one;
                }
                for second in actions.iter().skip(first_index + 1) {
                    if base.contains(&second.id) || second.id == first.id {
                        continue;
                    }
                    let mut one_for_two = base.clone();
                    one_for_two.push(first.id.clone());
                    one_for_two.push(second.id.clone());
                    if better_plan(
                        &one_for_two,
                        &best_replacement,
                        obligations,
                        actions,
                        policy,
                        context,
                    ) {
                        best_replacement = one_for_two;
                    }
                }
            }
        }
        for first_remove in 0..selected.len() {
            for second_remove in (first_remove + 1)..selected.len() {
                if Instant::now() >= deadline {
                    break;
                }
                let base = selected
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| *index != first_remove && *index != second_remove)
                    .map(|(_, id)| id.clone())
                    .collect::<Vec<_>>();
                for action in actions {
                    if base.contains(&action.id) {
                        continue;
                    }
                    let mut two_for_one = base.clone();
                    two_for_one.push(action.id.clone());
                    if better_plan(
                        &two_for_one,
                        &best_replacement,
                        obligations,
                        actions,
                        policy,
                        context,
                    ) {
                        best_replacement = two_for_one;
                    }
                }
            }
        }
        if best_replacement != selected {
            selected = best_replacement;
            changed = true;
        }
    }
    selected
}

pub struct PlanExecutor;

impl PlanExecutor {
    pub fn execute(
        &self,
        plan: &EvidencePlan,
        obligations: &mut [Obligation],
        providers: &[Box<dyn EvidenceProvider>],
        context: &ExecutionContext,
    ) -> ExecutionReport {
        let provider_map: HashMap<_, _> = providers
            .iter()
            .map(|provider| (provider.provider_id(), provider))
            .collect();
        let mut results = Vec::new();
        let mut failed_actions = Vec::new();
        let mut actual_cost_ms = 0;
        let mut telemetry = Vec::new();
        for action in &plan.selected_action_details {
            let Some(provider) = provider_map.get(&action.provider) else {
                failed_actions.push(ActionRejection {
                    action_id: action.id.clone(),
                    reason: "provider was not registered".to_string(),
                });
                continue;
            };
            match provider.execute(action, context) {
                Ok(mut result) => {
                    result.status = sound_provider_status(provider.kind(), result.status);
                    actual_cost_ms += result.actual_cost_ms;
                    telemetry.push(ActionTelemetry {
                        cache_key: action.id.to_string(),
                        target: context.target.clone().unwrap_or_default(),
                        action: action.id.clone(),
                        provider: action.provider.clone(),
                        source_hash: context
                            .metadata
                            .get("source_hash")
                            .cloned()
                            .unwrap_or_default(),
                        toolchain_version: context
                            .metadata
                            .get("toolchain_version")
                            .cloned()
                            .unwrap_or_default(),
                        target_cpu: context.target_cpu.clone().unwrap_or_default(),
                        config_fingerprint: context
                            .metadata
                            .get("config_fingerprint")
                            .cloned()
                            .unwrap_or_default(),
                        runtime_ms: result.actual_cost_ms,
                        success: result.status != ObligationStatus::Failed,
                        flakiness: action.flakiness,
                        obligations_covered: result.covered.clone(),
                    });
                    for obligation in obligations.iter_mut() {
                        if result.covered.contains(&obligation.id)
                            && result.status.confidence() > obligation.status.confidence()
                        {
                            obligation.status = result.status;
                            obligation.evidence.push(Evidence {
                                provider: provider.kind(),
                                status: result.status,
                                summary: result.summary.clone(),
                                details: result.details.clone(),
                            });
                        }
                    }
                    results.push(result);
                }
                Err(error) => failed_actions.push(ActionRejection {
                    action_id: action.id.clone(),
                    reason: error.to_string(),
                }),
            }
        }
        ExecutionReport {
            obligations: obligations.to_vec(),
            results,
            failed_actions,
            replanned: false,
            expected_cost_ms: plan.estimated_cost_ms,
            actual_cost_ms,
            telemetry,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn execute_with_replanning(
        &self,
        obligations: Vec<Obligation>,
        initial_plan: EvidencePlan,
        all_actions: &[EvidenceAction],
        planner: &EvidencePlanner,
        planning_context: &PlanningContext,
        providers: &[Box<dyn EvidenceProvider>],
        execution_context: &ExecutionContext,
    ) -> ExecutionReport {
        let mut current_obligations = obligations;
        let mut current_plan = initial_plan;
        let mut all_results = Vec::new();
        let mut all_failures = Vec::new();
        let mut expected_cost_ms = 0;
        let mut actual_cost_ms = 0;
        let mut telemetry = Vec::new();
        let mut replanned = false;
        let mut attempted_ids = HashSet::new();
        const MAX_EXECUTION_ROUNDS: usize = 3;
        for round in 0..MAX_EXECUTION_ROUNDS {
            expected_cost_ms += current_plan.estimated_cost_ms;
            attempted_ids.extend(current_plan.selected_actions.iter().cloned());
            let report = self.execute(
                &current_plan,
                &mut current_obligations,
                providers,
                execution_context,
            );
            actual_cost_ms += report.actual_cost_ms;
            telemetry.extend(report.telemetry);
            all_results.extend(report.results);
            all_failures.extend(report.failed_actions.clone());
            if round + 1 == MAX_EXECUTION_ROUNDS {
                break;
            }
            let remaining_actions = all_actions
                .iter()
                .filter(|action| !attempted_ids.contains(&action.id))
                .cloned()
                .collect::<Vec<_>>();
            let outcome = planner.plan(&current_obligations, &remaining_actions, planning_context);
            if outcome.plan.selected_actions.is_empty() {
                break;
            }
            current_plan = outcome.plan;
            replanned = true;
        }
        ExecutionReport {
            obligations: current_obligations,
            results: all_results,
            failed_actions: all_failures,
            replanned,
            expected_cost_ms,
            actual_cost_ms,
            telemetry,
        }
    }
}

pub fn planner_tool_context() -> PlanningContext {
    let mut context = PlanningContext::default();
    for tool in ["rustc", "cargo", "llvm-mca", "llvm-cov", "samply"] {
        if Command::new(tool)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
        {
            context.available_tools.insert(tool.to_string());
        }
    }
    if context.tool_available("cargo") {
        context.available_tools.insert("sanitizer".to_string());
    }
    context
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_is_not_resolved_or_passed_for_adaptive_policy() {
        let mut id = 0;
        let obligations = vec![make_obligation(
            &mut id,
            ObligationKind::MemorySafety,
            "test",
            None,
            None,
            Severity::Critical,
            ObligationStatus::Unknown,
            "unknown",
            "remediate",
        )];
        let report =
            AssuranceScheduler::new(AssurancePolicy::Adaptive, 0.9, false).evaluate(obligations);
        assert_eq!(report.coverage.unknown_obligation_count, 1);
        assert!(!report.passes);
    }

    #[test]
    fn static_policy_can_pass_with_unknown_performance_but_not_safety() {
        let mut id = 0;
        let obligations = vec![make_obligation(
            &mut id,
            ObligationKind::Complexity,
            "test",
            None,
            None,
            Severity::High,
            ObligationStatus::Unknown,
            "unknown",
            "remediate",
        )];
        let report =
            AssuranceScheduler::new(AssurancePolicy::Static, 0.9, false).evaluate(obligations);
        assert!(report.passes);
        assert_eq!(report.coverage.unknown_obligation_count, 1);
    }

    #[test]
    fn discovers_unsafe_obligation_without_claiming_proof() {
        let obligations = discover_obligations(
            Path::new("tests/uaf_thread_exit.rs"),
            "test_uaf_on_thread_exit",
        );
        assert!(obligations.iter().any(|obligation| {
            obligation.kind == ObligationKind::MemorySafety
                && obligation.status == ObligationStatus::Unknown
                && obligation.severity == Severity::Critical
        }));
    }

    #[test]
    fn provider_soundness_caps_compiler_success_below_proven() {
        let mut id = 0;
        let obligation = make_obligation(
            &mut id,
            ObligationKind::MemorySafety,
            "soundness",
            None,
            None,
            Severity::Critical,
            ObligationStatus::Unknown,
            "unsafe contract",
            "collect evidence",
        );
        let mut report =
            AssuranceScheduler::new(AssurancePolicy::Static, 0.9, false).evaluate(vec![obligation]);
        report.apply_provider_evidence(
            EvidenceProviderKind::Compiler,
            ObligationStatus::Proven,
            "compiler completed",
            None,
        );
        assert_eq!(report.obligations[0].status, ObligationStatus::Modeled);
        assert!(!report.passes);
    }

    #[test]
    fn planner_does_not_advertise_already_exhausted_static_pass() {
        let mut id = 0;
        let obligation = make_obligation(
            &mut id,
            ObligationKind::Complexity,
            "static",
            None,
            None,
            Severity::High,
            ObligationStatus::Unknown,
            "unknown after discovery",
            "collect evidence",
        );
        let actions =
            StaticAstProvider.discover_actions(&[obligation], &PlanningContext::default());
        assert!(actions.is_empty());
    }

    #[test]
    fn planner_can_upgrade_modeled_performance_evidence() {
        let mut id = 0;
        let obligation = make_obligation(
            &mut id,
            ObligationKind::CpuOverhead,
            "mca-upgrade",
            None,
            None,
            Severity::High,
            ObligationStatus::Modeled,
            "static overhead model",
            "collect MCA evidence",
        );
        let context = PlanningContext::with_tools(["llvm-mca"]);
        let actions = McaProvider { report: None }.discover_actions(&[obligation], &context);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].result_status, ObligationStatus::Observed);
    }

    fn planner_obligations(count: usize) -> Vec<Obligation> {
        let mut id = 0;
        (0..count)
            .map(|index| {
                make_obligation(
                    &mut id,
                    ObligationKind::Complexity,
                    "planner-test",
                    Some(format!("target_{index}")),
                    None,
                    Severity::High,
                    ObligationStatus::Unknown,
                    "planner obligation",
                    "collect evidence",
                )
            })
            .collect()
    }

    fn planner_action(
        id: &str,
        obligations: &[Obligation],
        indexes: &[usize],
        cost: u64,
    ) -> EvidenceAction {
        EvidenceAction::new(
            id,
            "test",
            indexes
                .iter()
                .map(|index| obligations[*index].id.clone())
                .collect(),
            cost,
            0.95,
        )
    }

    fn planner_policy() -> PlannerPolicy {
        PlannerPolicy {
            max_time_ms: Some(10_000),
            planner: PlannerKind::Hybrid,
            ..PlannerPolicy::default()
        }
    }

    #[test]
    fn planner_selects_minimum_cost_set_cover() {
        let obligations = planner_obligations(2);
        let actions = vec![
            planner_action("both", &obligations, &[0, 1], 50),
            planner_action("left", &obligations, &[0], 100),
            planner_action("right", &obligations, &[1], 100),
        ];
        let outcome = EvidencePlanner::new(planner_policy()).plan(
            &obligations,
            &actions,
            &PlanningContext::default(),
        );
        assert!(outcome.validation.valid);
        assert_eq!(
            outcome.plan.selected_actions,
            vec![EvidenceActionId("both".to_string())]
        );
    }

    #[test]
    fn exact_planner_matches_bruteforce_cost_oracle() {
        let obligations = planner_obligations(2);
        let actions = vec![
            planner_action("left", &obligations, &[0], 10),
            planner_action("right", &obligations, &[1], 10),
            planner_action("combined", &obligations, &[0, 1], 30),
        ];
        let policy = PlannerPolicy {
            planner: PlannerKind::Exact,
            ..planner_policy()
        };
        let context = PlanningContext::default();
        let mut oracle = u64::MAX;
        for mask in 0..(1usize << actions.len()) {
            let selected = actions
                .iter()
                .enumerate()
                .filter(|(index, _)| mask & (1usize << index) != 0)
                .map(|(_, action)| action.id.clone())
                .collect::<Vec<_>>();
            let validation =
                PlanValidator::validate(&obligations, &actions, &selected, &policy, &context);
            if validation.valid {
                oracle = oracle.min(validation.estimated_cost_ms);
            }
        }
        let outcome = EvidencePlanner::new(policy).plan(&obligations, &actions, &context);
        assert_eq!(outcome.plan.estimated_cost_ms, oracle);
    }

    #[test]
    fn planner_accounts_for_shared_setup_cost() {
        let obligations = planner_obligations(2);
        let mut left = planner_action("left", &obligations, &[0], 10);
        left.setup_group = Some("coverage".to_string());
        left.setup_cost_ms = 100;
        let mut right = planner_action("right", &obligations, &[1], 10);
        right.setup_group = Some("coverage".to_string());
        right.setup_cost_ms = 100;
        let mut combined = planner_action("combined", &obligations, &[0, 1], 30);
        combined.setup_group = Some("other".to_string());
        combined.setup_cost_ms = 100;
        let outcome = EvidencePlanner::new(planner_policy()).plan(
            &obligations,
            &[left, right, combined],
            &PlanningContext::default(),
        );
        assert!(outcome.validation.valid);
        assert_eq!(outcome.plan.estimated_cost_ms, 120);
        assert_eq!(outcome.plan.selected_actions.len(), 2);
    }

    #[test]
    fn planner_expands_prerequisites_and_validator_catches_missing_ones() {
        let obligations = planner_obligations(1);
        let setup = EvidenceAction::new("setup", "test", Vec::new(), 10, 1.0);
        let mut action = planner_action("run", &obligations, &[0], 10);
        action.requires = vec![setup.id.clone()];
        let outcome = EvidencePlanner::new(planner_policy()).plan(
            &obligations,
            &[setup.clone(), action.clone()],
            &PlanningContext::default(),
        );
        assert!(outcome.validation.valid);
        assert!(outcome.plan.selected_actions.contains(&setup.id));
        let invalid = PlanValidator::validate(
            &obligations,
            &[setup, action],
            &[EvidenceActionId("run".to_string())],
            &planner_policy(),
            &PlanningContext::default(),
        );
        assert!(!invalid.valid);
        assert!(
            invalid
                .errors
                .iter()
                .any(|error| error.contains("prerequisite"))
        );
    }

    #[test]
    fn unavailable_actions_are_rejected_and_reported_infeasible() {
        let obligations = planner_obligations(1);
        let mut action = planner_action("missing-tool", &obligations, &[0], 1);
        action.available = false;
        let outcome = EvidencePlanner::new(planner_policy()).plan(
            &obligations,
            &[action],
            &PlanningContext::default(),
        );
        assert_eq!(outcome.plan.status, PlanStatus::Infeasible);
        assert!(!outcome.plan.rejected_actions.is_empty());
        assert!(!outcome.plan.infeasible_obligations.is_empty());
    }

    #[test]
    fn dominance_pruning_removes_weaker_action() {
        let obligations = planner_obligations(1);
        let cheap = planner_action("cheap", &obligations, &[0], 10);
        let expensive = planner_action("expensive", &obligations, &[0], 20);
        let outcome = EvidencePlanner::new(planner_policy()).plan(
            &obligations,
            &[cheap, expensive],
            &PlanningContext::default(),
        );
        assert!(
            outcome
                .plan
                .rejected_actions
                .iter()
                .any(|rejection| rejection.action_id == EvidenceActionId("expensive".to_string()))
        );
    }

    #[test]
    fn deterministic_ties_use_action_id_order() {
        let obligations = planner_obligations(1);
        let first = planner_action("a", &obligations, &[0], 10);
        let second = planner_action("b", &obligations, &[0], 10);
        let outcome = EvidencePlanner::new(planner_policy()).plan(
            &obligations,
            &[second, first],
            &PlanningContext::default(),
        );
        assert_eq!(
            outcome.plan.selected_actions,
            vec![EvidenceActionId("a".to_string())]
        );
    }

    #[test]
    fn budget_and_mutual_exclusion_are_validated() {
        let obligations = planner_obligations(2);
        let mut left = planner_action("left", &obligations, &[0], 100);
        let mut right = planner_action("right", &obligations, &[1], 100);
        left.mutually_exclusive_with = vec![right.id.clone()];
        right.mutually_exclusive_with = vec![left.id.clone()];
        let mut policy = planner_policy();
        policy.max_time_ms = Some(150);
        let outcome = EvidencePlanner::new(policy).plan(
            &obligations,
            &[left, right],
            &PlanningContext::default(),
        );
        assert!(!outcome.validation.valid);
        assert!(
            outcome.validation.errors.iter().any(|error| {
                error.contains("policy thresholds") || error.contains("conflicts")
            })
        );
    }

    #[test]
    fn exact_planner_reports_timeout_without_claiming_success() {
        let obligations = planner_obligations(2);
        let actions = vec![
            planner_action("a", &obligations, &[0], 10),
            planner_action("b", &obligations, &[1], 10),
        ];
        let mut policy = planner_policy();
        policy.planner = PlannerKind::Exact;
        policy.timeout_ms = 0;
        let outcome =
            EvidencePlanner::new(policy).plan(&obligations, &actions, &PlanningContext::default());
        assert!(outcome.timed_out);
        assert_ne!(outcome.plan.status, PlanStatus::Feasible);
    }

    struct MockProvider {
        name: &'static str,
        fails: bool,
    }

    impl EvidenceProvider for MockProvider {
        fn kind(&self) -> EvidenceProviderKind {
            EvidenceProviderKind::Test
        }

        fn provider_id(&self) -> ProviderId {
            ProviderId(self.name.to_string())
        }

        fn supports(&self, _obligation: &Obligation) -> bool {
            true
        }

        fn availability(&self) -> ProviderAvailability {
            ProviderAvailability::Available
        }

        fn evaluate(&self, _obligation: &Obligation) -> ProviderResult {
            ProviderResult {
                status: ObligationStatus::Unknown,
                summary: "mock".to_string(),
                details: None,
            }
        }

        fn execute(
            &self,
            action: &EvidenceAction,
            _context: &ExecutionContext,
        ) -> Result<EvidenceResult, EvidenceError> {
            if self.fails {
                return Err(EvidenceError::Failed("mock action failed".to_string()));
            }
            Ok(EvidenceResult {
                action_id: action.id.clone(),
                status: action.effective_status(),
                summary: "mock success".to_string(),
                covered: action.covers.clone(),
                actual_cost_ms: 1,
                confidence: action.confidence,
                details: None,
            })
        }
    }

    #[test]
    fn executor_replans_after_action_failure() {
        let obligations = planner_obligations(1);
        let mut failing = planner_action("failing", &obligations, &[0], 1);
        failing.provider = ProviderId("fail".to_string());
        let mut fallback = planner_action("fallback", &obligations, &[0], 2);
        fallback.provider = ProviderId("fallback".to_string());
        fallback.result_status = ObligationStatus::Observed;
        let actions = vec![failing.clone(), fallback.clone()];
        let validation = PlanValidator::validate(
            &obligations,
            &actions,
            &[failing.id.clone()],
            &planner_policy(),
            &PlanningContext::default(),
        );
        let initial_plan = EvidencePlan {
            status: PlanStatus::Partial,
            selected_actions: vec![failing.id.clone()],
            selected_action_details: vec![failing],
            candidate_actions: actions.clone(),
            rejected_actions: Vec::new(),
            coverage_before: calculate_coverage(&obligations),
            expected_coverage: validation.coverage.clone(),
            actual_coverage: None,
            estimated_cost_ms: validation.estimated_cost_ms,
            actual_cost_ms: None,
            infeasible_obligations: Vec::new(),
            validator_errors: validation.errors.clone(),
        };
        let providers: Vec<Box<dyn EvidenceProvider>> = vec![
            Box::new(MockProvider {
                name: "fail",
                fails: true,
            }),
            Box::new(MockProvider {
                name: "fallback",
                fails: false,
            }),
        ];
        let report = PlanExecutor.execute_with_replanning(
            obligations,
            initial_plan,
            &actions,
            &EvidencePlanner::new(planner_policy()),
            &PlanningContext::default(),
            &providers,
            &ExecutionContext::default(),
        );
        assert!(report.replanned);
        assert_eq!(report.failed_actions.len(), 1);
        assert_eq!(report.obligations[0].status, ObligationStatus::Observed);
    }

    #[test]
    fn cost_cache_learns_median_without_turning_cache_into_evidence() {
        let mut cache = CostCache::default();
        let mut telemetry = ActionTelemetry {
            cache_key: "action".to_string(),
            target: "target".to_string(),
            action: EvidenceActionId("action".to_string()),
            provider: ProviderId("Test".to_string()),
            source_hash: "source".to_string(),
            toolchain_version: "toolchain".to_string(),
            target_cpu: "cpu".to_string(),
            config_fingerprint: "config".to_string(),
            runtime_ms: 30,
            success: true,
            flakiness: 0.0,
            obligations_covered: Vec::new(),
        };
        cache.record(&telemetry);
        telemetry.runtime_ms = 10;
        cache.record(&telemetry);
        telemetry.runtime_ms = 20;
        cache.record(&telemetry);
        telemetry.runtime_ms = 100;
        telemetry.success = false;
        cache.record(&telemetry);
        assert_eq!(cache.estimate("action"), Some(20));
    }

    #[test]
    fn failed_evidence_cannot_pass_policy() {
        let mut id = 0;
        let obligations = vec![make_obligation(
            &mut id,
            ObligationKind::Complexity,
            "failed",
            None,
            None,
            Severity::High,
            ObligationStatus::Failed,
            "provider failed",
            "use a replacement provider",
        )];
        let report =
            AssuranceScheduler::new(AssurancePolicy::Adaptive, 0.9, false).evaluate(obligations);
        assert_eq!(report.coverage.failed_obligation_count, 1);
        assert!(!report.passes);
    }
}
