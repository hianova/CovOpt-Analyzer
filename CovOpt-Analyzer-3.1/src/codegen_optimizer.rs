//! Conservative code-generation candidate generation and sandbox evaluation.

use crate::findings::{Finding, FindingKind};
use crate::repair::{RepairCandidate, RepairCandidateId, RepairKind, RiskLevel, SourceEdit};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodegenConfig {
    pub lto: Option<String>,
    pub codegen_units: Option<u32>,
    pub opt_level: Option<String>,
    pub target_cpu: Option<String>,
    pub max_candidates: usize,
}

impl Default for CodegenConfig {
    fn default() -> Self {
        Self {
            lto: None,
            codegen_units: None,
            opt_level: None,
            target_cpu: None,
            max_candidates: 32,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodegenSettings {
    pub inline: Option<String>,
    pub cold: bool,
    pub lto: Option<String>,
    pub codegen_units: Option<u32>,
    pub opt_level: Option<String>,
    pub target_cpu: Option<String>,
    pub dispatch: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CodegenMetrics {
    pub reciprocal_throughput: Option<f64>,
    pub ipc: Option<f64>,
    pub instruction_count: Option<usize>,
    pub code_size: Option<usize>,
    pub loads: Option<usize>,
    pub stores: Option<usize>,
    pub calls: Option<usize>,
    pub compile_time_ms: Option<u64>,
    pub unsupported_mca_instructions: Option<usize>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodegenCandidateStatus {
    Generated,
    CompileFailed,
    VerificationFailed,
    Evaluated,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodegenCandidate {
    pub id: String,
    pub target: String,
    pub function: Option<String>,
    pub baseline: CodegenSettings,
    pub proposed: CodegenSettings,
    pub repair: RepairCandidate,
    pub status: CodegenCandidateStatus,
    pub baseline_metrics: Option<CodegenMetrics>,
    pub candidate_metrics: Option<CodegenMetrics>,
    pub downstream_callers: Vec<String>,
    pub affected_package: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxEvaluation {
    pub candidate_id: String,
    pub passed: bool,
    pub compile_passed: bool,
    pub verification_passed: bool,
    pub source_hash: String,
    pub workspace: String,
    pub stdout: String,
    pub stderr: String,
    #[serde(default)]
    pub baseline_metrics: Option<CodegenMetrics>,
    pub metrics: Option<CodegenMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParetoFrontier {
    pub candidates: Vec<CodegenCandidate>,
}

fn source_hash(source: &str) -> String {
    SourceEdit::hash_source(source)
}

fn has_attr(item: &syn::ItemFn, name: &str) -> bool {
    item.attrs.iter().any(|attr| attr.path().is_ident(name))
}

fn function_start(item: &syn::ItemFn) -> (usize, usize) {
    let start = syn::spanned::Spanned::span(&item.sig.fn_token).start();
    (start.line, start.column)
}

fn candidate_for_finding(
    finding: &Finding,
    item: &syn::ItemFn,
    source: &str,
    file: &Path,
    config: &CodegenConfig,
) -> Option<CodegenCandidate> {
    let function = item.sig.ident.to_string();
    let (line, column) = function_start(item);
    let baseline = CodegenSettings {
        inline: if has_attr(item, "inline") {
            Some("inline".to_string())
        } else {
            None
        },
        cold: has_attr(item, "cold"),
        lto: config.lto.clone(),
        codegen_units: config.codegen_units,
        opt_level: config.opt_level.clone(),
        target_cpu: config.target_cpu.clone(),
        dispatch: None,
    };
    let (kind, proposed, repair_kind, risk, suggestion_only, replacement) = match finding.kind {
        FindingKind::MissingInlining => {
            if baseline.inline.is_some() {
                return None;
            }
            (
                FindingKind::MissingInlining,
                CodegenSettings {
                    inline: Some("inline".to_string()),
                    ..baseline.clone()
                },
                RepairKind::AddInline,
                RiskLevel::Medium,
                false,
                Some("#[inline]\n".to_string()),
            )
        }
        FindingKind::ExcessiveInlining => {
            if !has_attr(item, "inline") {
                return None;
            }
            (
                FindingKind::ExcessiveInlining,
                CodegenSettings {
                    inline: Some("inline(never)".to_string()),
                    ..baseline.clone()
                },
                RepairKind::RemoveInline,
                RiskLevel::Medium,
                false,
                None,
            )
        }
        FindingKind::GenericBloat => (
            FindingKind::GenericBloat,
            CodegenSettings {
                dispatch: Some("dyn-trait on cold path (suggestion)".to_string()),
                ..baseline.clone()
            },
            RepairKind::DiagnosticOnly,
            RiskLevel::High,
            true,
            None,
        ),
        FindingKind::HotColdMixing => (
            FindingKind::HotColdMixing,
            CodegenSettings {
                cold: true,
                ..baseline.clone()
            },
            RepairKind::MarkCold,
            RiskLevel::High,
            true,
            None,
        ),
        _ => return None,
    };
    let change = replacement.and_then(|replacement| {
        SourceEdit::from_source(
            file.display().to_string(),
            source,
            line,
            column,
            line,
            column,
            replacement,
        )
    });
    let id = format!("codegen-{}-{}", finding.id, function);
    Some(CodegenCandidate {
        id: id.clone(),
        target: file.display().to_string(),
        function: Some(function),
        baseline,
        proposed,
        repair: RepairCandidate {
            id: RepairCandidateId(id),
            kind: repair_kind,
            resolves: vec![finding.id.clone()],
            changes: change.into_iter().collect(),
            dependencies: Vec::new(),
            conflicts: Vec::new(),
            semantic_risk: risk,
            api_risk: RiskLevel::Low,
            abi_risk: RiskLevel::Low,
            estimated_benefit: Default::default(),
            verification: finding.obligations.clone(),
            suggestion_only,
            description: format!(
                "Codegen candidate for {:?}; generated code must be verified",
                kind
            ),
        },
        status: CodegenCandidateStatus::Generated,
        baseline_metrics: None,
        candidate_metrics: None,
        downstream_callers: Vec::new(),
        affected_package: None,
    })
}

pub fn generate_candidates(
    source: &str,
    file: &Path,
    findings: &[Finding],
    config: &CodegenConfig,
) -> Result<Vec<CodegenCandidate>, String> {
    let ast = syn::parse_file(source).map_err(|error| error.to_string())?;
    let mut candidates = Vec::new();
    for item in ast.items {
        let syn::Item::Fn(item_fn) = item else {
            continue;
        };
        let name = item_fn.sig.ident.to_string();
        for finding in findings
            .iter()
            .filter(|finding| finding.function.as_deref() == Some(name.as_str()))
        {
            if let Some(candidate) = candidate_for_finding(finding, &item_fn, source, file, config)
            {
                candidates.push(candidate);
            }
        }
    }
    candidates.sort_by(|left, right| left.id.cmp(&right.id));
    candidates.dedup_by(|left, right| left.id == right.id);
    candidates.truncate(config.max_candidates.max(1));
    Ok(candidates)
}

pub fn pareto_frontier(candidates: &[CodegenCandidate]) -> ParetoFrontier {
    let mut frontier = Vec::new();
    for candidate in candidates {
        let dominated = candidates.iter().any(|other| {
            let left = candidate.candidate_metrics.as_ref();
            let right = other.candidate_metrics.as_ref();
            match (left, right) {
                (Some(left), Some(right)) => {
                    right.instruction_count.unwrap_or(usize::MAX)
                        <= left.instruction_count.unwrap_or(usize::MAX)
                        && right.code_size.unwrap_or(usize::MAX)
                            <= left.code_size.unwrap_or(usize::MAX)
                        && right.reciprocal_throughput.unwrap_or(f64::INFINITY)
                            <= left.reciprocal_throughput.unwrap_or(f64::INFINITY)
                        && (right.instruction_count < left.instruction_count
                            || right.code_size < left.code_size
                            || right.reciprocal_throughput < left.reciprocal_throughput)
                }
                _ => false,
            }
        });
        if !dominated {
            frontier.push(candidate.clone());
        }
    }
    frontier.sort_by(|left, right| left.id.cmp(&right.id));
    ParetoFrontier {
        candidates: frontier,
    }
}

pub fn evaluate_in_sandbox(
    workspace_root: &Path,
    manifest_path: &Path,
    candidate: &CodegenCandidate,
) -> Result<SandboxEvaluation, String> {
    let workspace_root = workspace_root
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let source_path = workspace_root.join(&candidate.target);
    let source = fs::read_to_string(&source_path).map_err(|error| error.to_string())?;
    let baseline_metrics = candidate.function.as_deref().and_then(|function| {
        measure_codegen_metrics(&workspace_root, function, &candidate.baseline)
    });
    let temp = tempfile::tempdir().map_err(|error| error.to_string())?;
    copy_tree(&workspace_root, temp.path())?;
    let relative = source_path
        .strip_prefix(&workspace_root)
        .map_err(|error| error.to_string())?;
    let target = temp.path().join(relative);
    let updated = crate::repair::apply_edits_safely(&source, &candidate.repair.changes)?;
    if updated != source {
        fs::write(&target, updated).map_err(|error| error.to_string())?;
    }
    let temp_manifest = temp.path().join(
        manifest_path
            .strip_prefix(workspace_root)
            .map_err(|error| error.to_string())?,
    );
    let output = Command::new("cargo")
        .arg("check")
        .arg("--manifest-path")
        .arg(temp_manifest)
        .output()
        .map_err(|error| error.to_string())?;
    let metrics = output
        .status
        .success()
        .then(|| {
            candidate.function.as_deref().and_then(|function| {
                measure_codegen_metrics(temp.path(), function, &candidate.proposed)
            })
        })
        .flatten();
    let verification_passed = output.status.success()
        && !candidate.repair.suggestion_only
        && baseline_metrics.is_some()
        && metrics.is_some();
    Ok(SandboxEvaluation {
        candidate_id: candidate.id.clone(),
        passed: verification_passed,
        compile_passed: output.status.success(),
        verification_passed,
        source_hash: source_hash(&source),
        workspace: temp.path().display().to_string(),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        baseline_metrics,
        metrics,
    })
}

fn measure_codegen_metrics(
    workspace: &Path,
    function: &str,
    settings: &CodegenSettings,
) -> Option<CodegenMetrics> {
    let started = std::time::Instant::now();
    let extractor = crate::asm_extractor::AsmExtractor::new(workspace);
    let mut environment = Vec::new();
    if let Some(lto) = settings.lto.as_ref() {
        environment.push(("CARGO_PROFILE_RELEASE_LTO".to_string(), lto.clone()));
    }
    if let Some(units) = settings.codegen_units {
        environment.push((
            "CARGO_PROFILE_RELEASE_CODEGEN_UNITS".to_string(),
            units.to_string(),
        ));
    }
    if let Some(level) = settings.opt_level.as_ref() {
        environment.push(("CARGO_PROFILE_RELEASE_OPT_LEVEL".to_string(), level.clone()));
    }
    if let Some(cpu) = settings.target_cpu.as_ref() {
        let inherited = std::env::var("RUSTFLAGS").unwrap_or_default();
        environment.push((
            "RUSTFLAGS".to_string(),
            format!("{} -C target-cpu={cpu}", inherited.trim())
                .trim()
                .to_string(),
        ));
    }
    extractor
        .compile_asm_for_package_with_env(None, &environment)
        .ok()?;
    let assembly = extractor.extract_function(function).ok()?;
    let report = crate::mca::McaRunner::new(settings.target_cpu.clone())
        .run(&assembly)
        .ok()?;
    let unsupported = report.unsupported_instructions;
    let confidence = if report.instructions == 0 {
        0.0
    } else {
        (1.0 - unsupported as f64 / report.instructions as f64).clamp(0.0, 1.0)
    };
    Some(CodegenMetrics {
        reciprocal_throughput: Some(report.block_rthroughput),
        ipc: Some(report.ipc),
        instruction_count: Some(report.instructions),
        code_size: Some(assembly.len()),
        loads: Some(report.loads),
        stores: Some(report.stores),
        calls: Some(report.calls),
        compile_time_ms: Some(started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64),
        unsupported_mca_instructions: Some(unsupported),
        confidence,
    })
}

fn copy_tree(source: &Path, target: &Path) -> Result<(), String> {
    for entry in walkdir::WalkDir::new(source)
        .into_iter()
        .filter_map(Result::ok)
    {
        let relative = entry
            .path()
            .strip_prefix(source)
            .map_err(|error| error.to_string())?;
        if relative
            .components()
            .any(|component| matches!(component.as_os_str().to_str(), Some("target" | ".git")))
        {
            continue;
        }
        let destination = target.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&destination).map_err(|error| error.to_string())?;
        } else {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            fs::copy(entry.path(), destination).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assurance::Severity;
    use crate::findings::Finding;

    #[test]
    fn codegen_candidate_is_deterministic_and_patch_hash_bound() {
        let source = "fn foo() { let x = 1; }";
        let finding = Finding::new(
            "f",
            FindingKind::MissingInlining,
            Severity::Medium,
            "x.rs",
            1,
            Some("foo".to_string()),
        );
        let candidates = generate_candidates(
            source,
            Path::new("x.rs"),
            &[finding],
            &CodegenConfig::default(),
        )
        .unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].repair.changes[0].source_hash,
            SourceEdit::hash_source(source)
        );
    }
}
