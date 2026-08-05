//! Shared structured findings used by inspection, optimizers, reports, and repairs.

use crate::assurance::{Evidence, Severity, SourceLocation};
pub use crate::model::ObligationId;
use crate::repair::RepairCandidateId;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;

pub type FindingEvidence = Evidence;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FindingId(pub String);

impl fmt::Display for FindingId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FindingKind {
    GenericBloat,
    ExcessiveParameters,
    MissingInlining,
    ExcessiveInlining,
    HotColdMixing,
    AllocationInHotLoop,
    CloneInHotLoop,
    BlockingInAsync,
    LockInHotLoop,
    IoInHotLoop,
    ManualCasLoop,
    FalseSharing,
    ExcessivePadding,
    PoorFieldLocality,
    LockGuardEscape,
    SemanticAsmClone,
    UnsafeRisk,
}

impl FindingKind {
    pub fn is_codegen(self) -> bool {
        matches!(
            self,
            Self::GenericBloat
                | Self::ExcessiveParameters
                | Self::MissingInlining
                | Self::ExcessiveInlining
                | Self::HotColdMixing
                | Self::SemanticAsmClone
        )
    }

    pub fn is_layout(self) -> bool {
        matches!(
            self,
            Self::FalseSharing | Self::ExcessivePadding | Self::PoorFieldLocality
        )
    }

    pub fn is_repairable(self) -> bool {
        !matches!(self, Self::UnsafeRisk | Self::SemanticAsmClone)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: FindingId,
    pub kind: FindingKind,
    pub severity: Severity,
    pub location: SourceLocation,
    #[serde(default)]
    pub function: Option<String>,
    #[serde(default)]
    pub evidence: Vec<FindingEvidence>,
    pub explanation: String,
    #[serde(default)]
    pub obligations: Vec<ObligationId>,
    #[serde(default)]
    pub repair_candidates: Vec<RepairCandidateId>,
}

impl Finding {
    pub fn new(
        id: impl Into<String>,
        kind: FindingKind,
        severity: Severity,
        file: impl Into<String>,
        line: usize,
        function: Option<String>,
    ) -> Self {
        let function_name = function
            .clone()
            .unwrap_or_else(|| "unknown function".to_string());
        let explanation = FindingFormatter::explanation(kind, &function_name);
        Self {
            id: FindingId(id.into()),
            kind,
            severity,
            location: SourceLocation {
                file: file.into(),
                line,
            },
            function,
            evidence: Vec::new(),
            explanation,
            obligations: Vec::new(),
            repair_candidates: Vec::new(),
        }
    }

    pub fn modeled(mut self) -> Self {
        self.evidence.push(Evidence {
            provider: crate::assurance::EvidenceProviderKind::StaticAst,
            status: crate::assurance::ObligationStatus::Modeled,
            summary: "heuristic static AST evidence; not a proof".to_string(),
            details: None,
        });
        self
    }

    pub fn with_obligation(mut self, obligation: ObligationId) -> Self {
        self.obligations.push(obligation);
        self
    }
}

pub struct FindingFormatter;

impl FindingFormatter {
    pub fn explanation(kind: FindingKind, function: &str) -> String {
        match kind {
            FindingKind::GenericBloat => format!(
                "{function} has enough generic structure to increase monomorphization and code-size pressure."
            ),
            FindingKind::ExcessiveParameters => format!(
                "{function} has many parameters; grouping related state may reduce register and stack pressure."
            ),
            FindingKind::MissingInlining => format!(
                "{function} is a small or hot candidate without an inlining hint; verify generated code before changing it."
            ),
            FindingKind::ExcessiveInlining => format!(
                "{function} is an inlining candidate whose expansion may increase code size or instruction-cache pressure."
            ),
            FindingKind::HotColdMixing => {
                format!("{function} mixes likely hot work with cold control-flow paths.")
            }
            FindingKind::AllocationInHotLoop => {
                format!("{function} performs an allocation-like operation inside a loop.")
            }
            FindingKind::CloneInHotLoop => format!(
                "{function} calls clone-like code inside a loop; borrowing or reuse may reduce hot-path work."
            ),
            FindingKind::BlockingInAsync => {
                format!("{function} contains a blocking operation in an async context.")
            }
            FindingKind::LockInHotLoop => {
                format!("{function} acquires a lock inside a loop, which may serialize a hot path.")
            }
            FindingKind::IoInHotLoop => {
                format!("{function} performs synchronous I/O inside a loop.")
            }
            FindingKind::ManualCasLoop => format!(
                "{function} contains a manual compare-exchange loop that needs concurrency-specific validation."
            ),
            FindingKind::FalseSharing => format!(
                "{function} or its containing layout has fields that may share a cache line across threads."
            ),
            FindingKind::ExcessivePadding => {
                format!("{function} or its containing layout appears to contain avoidable padding.")
            }
            FindingKind::PoorFieldLocality => format!(
                "{function} accesses fields with a layout that may be poor for the observed locality pattern."
            ),
            FindingKind::LockGuardEscape => format!(
                "{function} may return or otherwise let a lock guard escape its intended scope."
            ),
            FindingKind::SemanticAsmClone => format!(
                "{function} has machine-level instruction similarity with another function."
            ),
            FindingKind::UnsafeRisk => format!(
                "{function} contains an unsafe or otherwise safety-sensitive construct requiring explicit evidence."
            ),
        }
    }

    pub fn short(finding: &Finding) -> String {
        format!(
            "[{}] {}:{} {}",
            finding.id, finding.location.file, finding.location.line, finding.explanation
        )
    }
}

pub fn stable_finding_id(
    kind: FindingKind,
    file: &str,
    line: usize,
    function: Option<&str>,
) -> FindingId {
    let input = format!("{:?}|{}|{}|{}", kind, file, line, function.unwrap_or(""));
    let hash = input.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    FindingId(format!("COVOPT-FND-{:016x}", hash))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FindingReport {
    pub findings: Vec<Finding>,
    #[serde(default)]
    pub repair_candidates: Vec<crate::repair::RepairCandidate>,
}

impl FindingReport {
    pub fn format_lines(&self) -> Vec<String> {
        self.findings.iter().map(FindingFormatter::short).collect()
    }
}

/// Run the shared source analysis used by inspection, repair planning, and the
/// autonomous convergence loop. Candidate generation lives here so every
/// caller sees the same findings, stable IDs, and source-bound materializers.
pub fn analyze_source(
    file: &Path,
    function_filter: Option<&str>,
    codegen_config: &crate::codegen_optimizer::CodegenConfig,
    layout_config: &crate::layout_optimizer::LayoutConfig,
) -> Result<FindingReport, String> {
    let source = std::fs::read_to_string(file).map_err(|error| error.to_string())?;
    let ast = syn::parse_file(&source).map_err(|error| error.to_string())?;
    let mut report = FindingReport::default();
    report.findings.extend(crate::dataflow::analyze_file(file));

    for item in ast.items {
        match item {
            syn::Item::Fn(item_fn) => {
                let name = item_fn.sig.ident.to_string();
                if function_filter.is_some_and(|wanted| wanted != name) {
                    continue;
                }
                let mut findings =
                    crate::advisor::EncapsulationAdvisor::analyze(&item_fn, None).findings;
                for finding in &mut findings {
                    normalize_finding(finding, file, Some(&name));
                }
                report.findings.extend(findings);
            }
            syn::Item::Struct(item_struct) => {
                let name = item_struct.ident.to_string();
                let mut findings =
                    crate::advisor::EncapsulationAdvisor::analyze_struct(&item_struct).findings;
                for finding in &mut findings {
                    normalize_finding(finding, file, Some(&name));
                }
                report.findings.extend(findings);
            }
            _ => {}
        }
    }

    for model in crate::layout_optimizer::extract_layout(&source, None).unwrap_or_default() {
        report
            .findings
            .extend(crate::layout_optimizer::layout_findings(
                &model,
                file.display().to_string(),
            ));
    }
    report
        .findings
        .sort_by(|left, right| left.id.cmp(&right.id));
    report.findings.dedup_by(|left, right| left.id == right.id);

    let codegen = crate::codegen_optimizer::generate_candidates(
        &source,
        file,
        &report.findings,
        codegen_config,
    )
    .unwrap_or_default();
    report
        .repair_candidates
        .extend(codegen.into_iter().map(|candidate| candidate.repair));

    for model in crate::layout_optimizer::extract_layout(&source, None).unwrap_or_default() {
        let findings = crate::layout_optimizer::layout_findings(&model, file.display().to_string());
        for mut candidate in
            crate::layout_optimizer::generate_candidates(&model, &findings, layout_config)
        {
            match crate::layout_optimizer::materialize_candidate(
                &source,
                file,
                &candidate,
                layout_config.cache_line_bytes,
            )? {
                Some(edit) => candidate.repair.changes.push(edit),
                None => candidate.repair.suggestion_only = true,
            }
            report.repair_candidates.push(candidate.repair);
        }
    }

    for candidate in &report.repair_candidates {
        for finding in &mut report.findings {
            if candidate.resolves.contains(&finding.id)
                && !finding.repair_candidates.contains(&candidate.id)
            {
                finding.repair_candidates.push(candidate.id.clone());
            }
        }
    }
    report
        .repair_candidates
        .sort_by(|left, right| left.id.cmp(&right.id));
    report
        .repair_candidates
        .dedup_by(|left, right| left.id == right.id);
    Ok(report)
}

fn normalize_finding(finding: &mut Finding, file: &Path, function: Option<&str>) {
    finding.location.file = file.display().to_string();
    finding.function = function.map(str::to_string);
    finding.id = stable_finding_id(
        finding.kind,
        &finding.location.file,
        finding.location.line,
        finding.function.as_deref(),
    );
    finding.explanation = FindingFormatter::explanation(
        finding.kind,
        finding.function.as_deref().unwrap_or("unknown function"),
    );
}
