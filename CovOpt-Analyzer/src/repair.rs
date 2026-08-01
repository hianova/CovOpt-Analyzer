//! Repair candidates, minimal repair-set planning, verification, and safe apply.

use crate::findings::FindingId;
use crate::model::ObligationId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RepairCandidateId(pub String);

impl std::fmt::Display for RepairCandidateId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RepairKind {
    AddInline,
    RemoveInline,
    MarkCold,
    SplitHotCold,
    ReorderFields,
    AddPadding,
    AlignCacheLine,
    SeparateAtomic,
    BorrowInsteadOfClone,
    MoveBlockingToSpawnBlocking,
    NarrowLockScope,
    ReplaceManualCas,
    DiagnosticOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceEdit {
    pub file: String,
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub replacement: String,
    pub original_text: String,
    pub source_hash: String,
}

impl SourceEdit {
    pub fn hash_source(source: &str) -> String {
        let hash = source.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        });
        format!("{:016x}", hash)
    }

    pub fn from_source(
        file: impl Into<String>,
        source: &str,
        start_line: usize,
        start_column: usize,
        end_line: usize,
        end_column: usize,
        replacement: impl Into<String>,
    ) -> Option<Self> {
        let original_text = slice_source(source, start_line, start_column, end_line, end_column)?;
        Some(Self {
            file: file.into(),
            start_line,
            start_column,
            end_line,
            end_column,
            replacement: replacement.into(),
            original_text,
            source_hash: Self::hash_source(source),
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PerformanceDelta {
    pub reciprocal_throughput_percent: Option<f64>,
    pub ipc_percent: Option<f64>,
    pub instruction_count_delta: Option<i64>,
    pub code_size_delta: Option<i64>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairCandidate {
    pub id: RepairCandidateId,
    pub kind: RepairKind,
    #[serde(default)]
    pub resolves: Vec<FindingId>,
    #[serde(default)]
    pub changes: Vec<SourceEdit>,
    #[serde(default)]
    pub dependencies: Vec<RepairCandidateId>,
    #[serde(default)]
    pub conflicts: Vec<RepairCandidateId>,
    pub semantic_risk: RiskLevel,
    pub api_risk: RiskLevel,
    pub abi_risk: RiskLevel,
    #[serde(default)]
    pub estimated_benefit: PerformanceDelta,
    #[serde(default)]
    pub verification: Vec<ObligationId>,
    #[serde(default)]
    pub suggestion_only: bool,
    #[serde(default)]
    pub description: String,
}

impl RepairCandidate {
    pub fn high_risk(&self) -> bool {
        matches!(self.semantic_risk, RiskLevel::High | RiskLevel::Unknown)
            || matches!(self.api_risk, RiskLevel::High | RiskLevel::Unknown)
            || matches!(self.abi_risk, RiskLevel::High | RiskLevel::Unknown)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairPolicy {
    pub budget_ms: u64,
    pub max_time_ms: u64,
    pub exact_candidate_limit: usize,
    pub allow_high_risk: bool,
    pub allow_api_changes: bool,
    pub allow_abi_changes: bool,
    pub max_changed_lines: usize,
}

impl Default for RepairPolicy {
    fn default() -> Self {
        Self {
            budget_ms: 30_000,
            max_time_ms: 5_000,
            exact_candidate_limit: 18,
            allow_high_risk: false,
            allow_api_changes: false,
            allow_abi_changes: false,
            max_changed_lines: 100,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairRejection {
    pub candidate_id: RepairCandidateId,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockingFinding {
    pub finding_id: FindingId,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairPlan {
    pub selected: Vec<RepairCandidateId>,
    #[serde(default)]
    pub rejected: Vec<RepairRejection>,
    #[serde(default)]
    pub blocking_findings: Vec<BlockingFinding>,
    pub critical_resolved: bool,
    pub changed_lines: usize,
    pub verification_cost_ms: u64,
    pub estimated_benefit: PerformanceDelta,
    pub timed_out: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub candidate_id: RepairCandidateId,
    pub passed: bool,
    pub compile_passed: bool,
    pub safety_passed: bool,
    pub regression: bool,
    pub summary: String,
    pub actual_cost_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyResult {
    pub applied: Vec<RepairCandidateId>,
    pub rejected: Vec<RepairRejection>,
    pub changed_files: Vec<String>,
    pub manifest_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxVerification {
    pub passed: bool,
    pub compile_passed: bool,
    pub source_hash: String,
    pub stdout: String,
    pub stderr: String,
}

/// Result of executing one planner-selected evidence action against a patched
/// candidate workspace. An unavailable executor is a failed action, never
/// evidence that the candidate passed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateActionResult {
    pub action_id: String,
    pub provider: String,
    pub passed: bool,
    pub status: crate::assurance::ObligationStatus,
    #[serde(default)]
    pub command: Vec<String>,
    pub stdout: String,
    pub stderr: String,
    pub actual_cost_ms: u64,
}

/// Candidate-bound verification record. The candidate hash covers the source
/// hashes and every edit, so evidence cannot be reused for a different patch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateEvidenceVerification {
    pub passed: bool,
    pub compile_passed: bool,
    pub candidate_hash: String,
    pub source_hashes: BTreeMap<String, String>,
    pub actions: Vec<CandidateActionResult>,
    #[serde(default)]
    pub failed_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionFileRecord {
    pub file: String,
    pub before_hash: String,
    pub after_hash: String,
    pub backup: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransactionStatus {
    Prepared,
    Committed,
    RolledBack,
}

/// Recoverable multi-file repair transaction. Original bytes live beside this
/// manifest under `target/covopt/transactions/<candidate>/original/`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairTransaction {
    pub version: u32,
    pub candidate_hash: String,
    pub workspace: String,
    pub status: TransactionStatus,
    pub files: Vec<TransactionFileRecord>,
    pub manifest_path: String,
}

pub fn plan_repairs(
    findings: &[crate::findings::Finding],
    candidates: &[RepairCandidate],
    policy: &RepairPolicy,
) -> RepairPlan {
    let start = std::time::Instant::now();
    let critical = findings
        .iter()
        .filter(|finding| finding.severity == crate::assurance::Severity::Critical)
        .map(|finding| finding.id.clone())
        .collect::<HashSet<_>>();
    let finding_ids = findings
        .iter()
        .map(|finding| finding.id.clone())
        .collect::<HashSet<_>>();
    let mut rejected = Vec::new();
    let mut eligible = candidates
        .iter()
        .filter(|candidate| {
            let reason = if candidate.high_risk() && !policy.allow_high_risk {
                Some("high-risk repair requires explicit opt-in")
            } else if !policy.allow_api_changes && candidate.api_risk != RiskLevel::Low {
                Some("API risk is disabled by policy")
            } else if !policy.allow_abi_changes && candidate.abi_risk != RiskLevel::Low {
                Some("ABI risk is disabled by policy")
            } else if candidate
                .resolves
                .iter()
                .any(|finding| !finding_ids.contains(finding))
            {
                Some("candidate references an unknown finding")
            } else {
                None
            };
            if let Some(reason) = reason {
                rejected.push(RepairRejection {
                    candidate_id: candidate.id.clone(),
                    reason: reason.to_string(),
                });
                false
            } else {
                true
            }
        })
        .collect::<Vec<_>>();
    eligible.sort_by(|left, right| {
        left.dependencies
            .len()
            .cmp(&right.dependencies.len())
            .then(left.id.cmp(&right.id))
    });
    let mut selected = if eligible.len() <= policy.exact_candidate_limit {
        exact_select(&eligible, &critical, policy, start)
    } else {
        greedy_select(&eligible, &critical, policy, start)
    };
    selected.sort_by(|left, right| left.id.cmp(&right.id));
    let selected_ids = selected
        .iter()
        .map(|candidate| candidate.id.clone())
        .collect::<HashSet<_>>();
    let covered = selected
        .iter()
        .flat_map(|candidate| candidate.resolves.iter())
        .collect::<HashSet<_>>();
    let blocking_findings = findings
        .iter()
        .filter(|finding| {
            !covered.contains(&finding.id)
                && finding.severity == crate::assurance::Severity::Critical
        })
        .map(|finding| BlockingFinding {
            finding_id: finding.id.clone(),
            reason: "no feasible non-conflicting repair selected".to_string(),
        })
        .collect::<Vec<_>>();
    let mut benefit = PerformanceDelta::default();
    let mut changed_lines = 0usize;
    let mut verification_cost_ms = 0u64;
    for candidate in &selected {
        changed_lines += candidate
            .changes
            .iter()
            .map(|change| change.original_text.lines().count().max(1))
            .sum::<usize>();
        verification_cost_ms =
            verification_cost_ms.saturating_add(500 + candidate.verification.len() as u64 * 250);
        benefit.confidence = benefit
            .confidence
            .max(candidate.estimated_benefit.confidence);
        benefit.reciprocal_throughput_percent = sum_optional(
            benefit.reciprocal_throughput_percent,
            candidate.estimated_benefit.reciprocal_throughput_percent,
        );
        benefit.ipc_percent =
            sum_optional(benefit.ipc_percent, candidate.estimated_benefit.ipc_percent);
    }
    let rejected_ids = candidates
        .iter()
        .filter(|candidate| !selected_ids.contains(&candidate.id))
        .map(|candidate| candidate.id.clone())
        .collect::<HashSet<_>>();
    rejected.extend(
        candidates
            .iter()
            .filter(|candidate| rejected_ids.contains(&candidate.id))
            .map(|candidate| RepairRejection {
                candidate_id: candidate.id.clone(),
                reason: "not needed for the selected minimal set".to_string(),
            }),
    );
    RepairPlan {
        selected: selected
            .iter()
            .map(|candidate| candidate.id.clone())
            .collect(),
        rejected,
        blocking_findings,
        critical_resolved: critical.iter().all(|finding| covered.contains(finding)),
        changed_lines,
        verification_cost_ms,
        estimated_benefit: benefit,
        timed_out: start.elapsed().as_millis() as u64 >= policy.max_time_ms,
    }
}

fn sum_optional(left: Option<f64>, right: Option<f64>) -> Option<f64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left + right),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn conflicts(left: &RepairCandidate, selected: &[&RepairCandidate]) -> bool {
    selected.iter().any(|right| {
        left.conflicts.contains(&right.id)
            || right.conflicts.contains(&left.id)
            || left.dependencies.contains(&right.id) && right.conflicts.contains(&left.id)
    })
}

fn dependencies_satisfied(candidate: &RepairCandidate, selected: &[&RepairCandidate]) -> bool {
    candidate
        .dependencies
        .iter()
        .all(|dependency| selected.iter().any(|candidate| &candidate.id == dependency))
}

fn better_set(
    left: &[&RepairCandidate],
    right: &[&RepairCandidate],
    critical: &HashSet<FindingId>,
) -> bool {
    let left_covered = left
        .iter()
        .flat_map(|candidate| candidate.resolves.iter().cloned())
        .collect::<HashSet<_>>();
    let right_covered = right
        .iter()
        .flat_map(|candidate| candidate.resolves.iter().cloned())
        .collect::<HashSet<_>>();
    let left_critical = critical.intersection(&left_covered).count();
    let right_critical = critical.intersection(&right_covered).count();
    if left_critical != right_critical {
        return left_critical > right_critical;
    }
    let left_risk = left
        .iter()
        .map(|candidate| risk_score(candidate))
        .sum::<u8>();
    let right_risk = right
        .iter()
        .map(|candidate| risk_score(candidate))
        .sum::<u8>();
    if left_risk != right_risk {
        return left_risk < right_risk;
    }
    if left.len() != right.len() {
        return left.len() < right.len();
    }
    let left_lines = left
        .iter()
        .flat_map(|candidate| candidate.changes.iter())
        .map(|change| change.original_text.lines().count().max(1))
        .sum::<usize>();
    let right_lines = right
        .iter()
        .flat_map(|candidate| candidate.changes.iter())
        .map(|change| change.original_text.lines().count().max(1))
        .sum::<usize>();
    if left_lines != right_lines {
        return left_lines < right_lines;
    }
    let left_ids = left
        .iter()
        .map(|candidate| candidate.id.0.clone())
        .collect::<Vec<_>>();
    let right_ids = right
        .iter()
        .map(|candidate| candidate.id.0.clone())
        .collect::<Vec<_>>();
    left_ids < right_ids
}

fn risk_score(candidate: &RepairCandidate) -> u8 {
    [
        candidate.semantic_risk,
        candidate.api_risk,
        candidate.abi_risk,
    ]
    .iter()
    .map(|risk| match risk {
        RiskLevel::Low => 0,
        RiskLevel::Medium => 1,
        RiskLevel::High => 2,
        RiskLevel::Unknown => 3,
    })
    .sum()
}

fn exact_select<'a>(
    candidates: &[&'a RepairCandidate],
    critical: &HashSet<FindingId>,
    policy: &RepairPolicy,
    start: std::time::Instant,
) -> Vec<&'a RepairCandidate> {
    fn visit<'a>(
        candidates: &[&'a RepairCandidate],
        index: usize,
        selected: &mut Vec<&'a RepairCandidate>,
        best: &mut Vec<&'a RepairCandidate>,
        critical: &HashSet<FindingId>,
        policy: &RepairPolicy,
        start: std::time::Instant,
    ) {
        if start.elapsed().as_millis() as u64 >= policy.max_time_ms || index >= candidates.len() {
            return;
        }
        visit(
            candidates,
            index + 1,
            selected,
            best,
            critical,
            policy,
            start,
        );
        let candidate = candidates[index];
        if !conflicts(candidate, selected) && dependencies_satisfied(candidate, selected) {
            selected.push(candidate);
            if better_set(selected, best, critical) {
                *best = selected.clone();
            }
            visit(
                candidates,
                index + 1,
                selected,
                best,
                critical,
                policy,
                start,
            );
            selected.pop();
        }
    }
    let mut best = Vec::new();
    visit(
        candidates,
        0,
        &mut Vec::new(),
        &mut best,
        critical,
        policy,
        start,
    );
    best
}

fn greedy_select<'a>(
    candidates: &[&'a RepairCandidate],
    critical: &HashSet<FindingId>,
    policy: &RepairPolicy,
    start: std::time::Instant,
) -> Vec<&'a RepairCandidate> {
    let mut remaining = candidates.to_vec();
    remaining.sort_by(|left, right| {
        left.dependencies
            .len()
            .cmp(&right.dependencies.len())
            .then(right.resolves.len().cmp(&left.resolves.len()))
            .then(left.id.cmp(&right.id))
    });
    let mut selected = Vec::new();
    let mut lines = 0usize;
    while let Some(candidate) = remaining.first().copied() {
        if start.elapsed().as_millis() as u64 >= policy.max_time_ms {
            break;
        }
        remaining.remove(0);
        let added_lines = candidate
            .changes
            .iter()
            .map(|change| change.original_text.lines().count().max(1))
            .sum::<usize>();
        if lines + added_lines > policy.max_changed_lines
            || conflicts(candidate, &selected)
            || !dependencies_satisfied(candidate, &selected)
        {
            continue;
        }
        selected.push(candidate);
        lines += added_lines;
        let covered = selected
            .iter()
            .flat_map(|item| item.resolves.iter())
            .collect::<HashSet<_>>();
        if critical.iter().all(|finding| covered.contains(finding)) {
            break;
        }
    }
    selected
}

pub fn apply_edits_safely(source: &str, edits: &[SourceEdit]) -> Result<String, String> {
    let expected_hash = SourceEdit::hash_source(source);
    if edits.iter().any(|edit| edit.source_hash != expected_hash) {
        return Err("source hash changed; refusing to apply repair edits".to_string());
    }
    let mut updated = source.to_string();
    let mut ordered = edits.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        right
            .start_line
            .cmp(&left.start_line)
            .then(right.start_column.cmp(&left.start_column))
    });
    for edit in ordered {
        let start = source_offset(&updated, edit.start_line, edit.start_column)
            .ok_or_else(|| "invalid edit start".to_string())?;
        let end = source_offset(&updated, edit.end_line, edit.end_column)
            .ok_or_else(|| "invalid edit end".to_string())?;
        if updated.get(start..end) != Some(edit.original_text.as_str()) {
            return Err(format!(
                "source text mismatch in {}:{}",
                edit.file, edit.start_line
            ));
        }
        updated.replace_range(start..end, &edit.replacement);
    }
    Ok(updated)
}

fn source_offset(source: &str, line: usize, column: usize) -> Option<usize> {
    if line == 0 {
        return None;
    }
    let mut offset = 0usize;
    for (index, current) in source.split_inclusive('\n').enumerate() {
        if index + 1 == line {
            return Some(offset + column.min(current.trim_end_matches('\n').len()));
        }
        offset += current.len();
    }
    None
}

fn slice_source(
    source: &str,
    start_line: usize,
    start_column: usize,
    end_line: usize,
    end_column: usize,
) -> Option<String> {
    let start = source_offset(source, start_line, start_column)?;
    let end = source_offset(source, end_line, end_column)?;
    source.get(start..end).map(str::to_string)
}

pub fn write_manifest(
    path: impl AsRef<Path>,
    plan: &RepairPlan,
    results: &[VerificationResult],
) -> Result<(), String> {
    let document = serde_json::json!({ "version": 1, "plan": plan, "verification": results });
    let bytes = serde_json::to_vec_pretty(&document).map_err(|error| error.to_string())?;
    fs::write(path, bytes).map_err(|error| error.to_string())
}

fn candidate_hash(edits: &[SourceEdit]) -> String {
    let mut edits = edits.iter().collect::<Vec<_>>();
    edits.sort_by(|left, right| {
        left.file
            .cmp(&right.file)
            .then(left.start_line.cmp(&right.start_line))
            .then(left.start_column.cmp(&right.start_column))
            .then(left.end_line.cmp(&right.end_line))
            .then(left.end_column.cmp(&right.end_column))
    });
    let mut hash = 0xcbf29ce484222325u64;
    for edit in edits {
        let record = format!(
            "{}:{}:{}:{}:{}:{}:{};",
            edit.file,
            edit.start_line,
            edit.start_column,
            edit.end_line,
            edit.end_column,
            edit.source_hash,
            edit.replacement
        );
        for byte in record.bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
    }
    format!("{hash:016x}")
}

fn copy_workspace(source: &Path, target: &Path) -> Result<(), String> {
    for entry in walkdir::WalkDir::new(source)
        .into_iter()
        .filter_map(Result::ok)
    {
        let relative = entry
            .path()
            .strip_prefix(source)
            .map_err(|error| error.to_string())?;
        if relative.components().any(|component| {
            matches!(
                component.as_os_str().to_str(),
                Some("target" | ".git" | ".covopt_backup")
            )
        }) {
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

fn relative_workspace_path(workspace_root: &Path, file: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(file);
    let absolute = if path.is_absolute() {
        path
    } else {
        workspace_root.join(path)
    };
    absolute
        .strip_prefix(workspace_root)
        .map(Path::to_path_buf)
        .map_err(|_| format!("repair edit escapes workspace: {}", absolute.display()))
}

fn run_candidate_command(
    sandbox_root: &Path,
    manifest_path: &Path,
    action_id: &str,
    provider: &str,
    arguments: &[String],
    timeout: Duration,
) -> CandidateActionResult {
    let started = Instant::now();
    let mut command = Command::new("cargo");
    command
        .args(arguments)
        .arg("--manifest-path")
        .arg(manifest_path)
        .current_dir(sandbox_root)
        .env(
            "CARGO_TARGET_DIR",
            sandbox_root.join("target/covopt-candidate"),
        );
    let rendered = std::iter::once("cargo".to_string())
        .chain(arguments.iter().cloned())
        .chain([
            "--manifest-path".to_string(),
            manifest_path.display().to_string(),
        ])
        .collect::<Vec<_>>();
    match crate::runner::command_output_with_timeout(
        &mut command,
        &format!("candidate {provider} evidence"),
        timeout,
    ) {
        Ok(output) => CandidateActionResult {
            action_id: action_id.to_string(),
            provider: provider.to_string(),
            passed: output.status.success(),
            status: if output.status.success() {
                crate::assurance::ObligationStatus::Observed
            } else {
                crate::assurance::ObligationStatus::Failed
            },
            command: rendered,
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            actual_cost_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        },
        Err(error) => CandidateActionResult {
            action_id: action_id.to_string(),
            provider: provider.to_string(),
            passed: false,
            status: crate::assurance::ObligationStatus::Unknown,
            command: rendered,
            stdout: String::new(),
            stderr: error.to_string(),
            actual_cost_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        },
    }
}

fn run_candidate_atomic_model(
    sandbox_root: &Path,
    files: &[PathBuf],
    action_id: &str,
    timeout: Duration,
) -> CandidateActionResult {
    let started = Instant::now();
    let mut reports = Vec::new();
    let mut passed = true;
    for relative in files {
        let request = crate::atomic_synth::request_from_file(
            sandbox_root.join(relative),
            None,
            crate::atomic_model::ModelBounds::default(),
            timeout.as_millis().min(u128::from(u64::MAX)) as u64,
            false,
        );
        match request {
            Ok(request) => {
                let report = crate::atomic_synth::analyze_atomic(&request);
                let modeled = matches!(
                    report.baseline.as_ref().map(|result| result.status),
                    Some(crate::atomic_model::ModelStatus::Modeled)
                );
                passed &= modeled;
                reports.push(serde_json::to_value(report).unwrap_or(serde_json::Value::Null));
            }
            Err(error) => {
                passed = false;
                reports.push(serde_json::json!({
                    "file": relative,
                    "error": error,
                }));
            }
        }
    }
    CandidateActionResult {
        action_id: action_id.to_string(),
        provider: "AtomicModel".to_string(),
        passed,
        status: if passed {
            crate::assurance::ObligationStatus::Modeled
        } else {
            crate::assurance::ObligationStatus::Unknown
        },
        command: Vec::new(),
        stdout: serde_json::to_string(&reports).unwrap_or_default(),
        stderr: if passed {
            String::new()
        } else {
            "one or more changed files could not be modeled within the candidate bound".to_string()
        },
        actual_cost_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
    }
}

fn package_for_changed_file(sandbox_root: &Path, relative: &Path) -> Option<String> {
    let mut directory = sandbox_root.join(relative).parent()?.to_path_buf();
    while directory.starts_with(sandbox_root) {
        let manifest = directory.join("Cargo.toml");
        if let Ok(content) = fs::read_to_string(manifest)
            && let Ok(document) = toml::from_str::<toml::Value>(&content)
            && let Some(name) = document
                .get("package")
                .and_then(|package| package.get("name"))
                .and_then(toml::Value::as_str)
        {
            return Some(name.to_string());
        }
        if !directory.pop() {
            break;
        }
    }
    None
}

fn run_candidate_mca(
    sandbox_root: &Path,
    files: &[PathBuf],
    action_id: &str,
    timeout: Duration,
) -> CandidateActionResult {
    #[derive(Default)]
    struct FunctionCollector(Vec<String>);
    impl<'ast> syn::visit::Visit<'ast> for FunctionCollector {
        fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
            self.0.push(node.sig.ident.to_string());
            syn::visit::visit_item_fn(self, node);
        }

        fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
            self.0.push(node.sig.ident.to_string());
            syn::visit::visit_impl_item_fn(self, node);
        }
    }

    let started = Instant::now();
    let mut functions = Vec::new();
    for relative in files {
        if let Ok(source) = fs::read_to_string(sandbox_root.join(relative))
            && let Ok(syntax) = syn::parse_file(&source)
        {
            let mut collector = FunctionCollector::default();
            syn::visit::Visit::visit_file(&mut collector, &syntax);
            functions.extend(collector.0);
        }
    }
    functions.sort();
    functions.dedup();
    let package = files
        .first()
        .and_then(|file| package_for_changed_file(sandbox_root, file));
    let extractor = crate::asm_extractor::AsmExtractor::new(sandbox_root);
    let compile =
        extractor.compile_asm_for_package_with_env_timeout(package.as_deref(), &[], Some(timeout));
    let mut reports = BTreeMap::new();
    let mut errors = Vec::new();
    if let Err(error) = compile {
        errors.push(error);
    } else {
        for function in functions {
            match extractor
                .extract_function(&function)
                .and_then(|assembly| crate::mca::McaRunner::new(None).run(&assembly))
            {
                Ok(report)
                    if report.instructions > 0
                        && report.unsupported_instructions < report.instructions =>
                {
                    reports.insert(function, report);
                }
                Ok(report) => errors.push(format!(
                    "{function}: no supported instruction stream ({}/{})",
                    report.unsupported_instructions, report.instructions
                )),
                Err(error) => errors.push(format!("{function}: {error}")),
            }
        }
    }
    let passed = !reports.is_empty();
    CandidateActionResult {
        action_id: action_id.to_string(),
        provider: "Mca".to_string(),
        passed,
        status: if passed {
            crate::assurance::ObligationStatus::Modeled
        } else {
            crate::assurance::ObligationStatus::Unknown
        },
        command: vec![
            "cargo rustc --release -- --emit=asm".to_string(),
            "llvm-mca <changed-functions>".to_string(),
        ],
        stdout: serde_json::to_string(&reports).unwrap_or_default(),
        stderr: errors.join("\n"),
        actual_cost_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
    }
}

fn compare_candidate_mca(
    baseline: &CandidateActionResult,
    mut candidate: CandidateActionResult,
) -> CandidateActionResult {
    let baseline_reports =
        serde_json::from_str::<BTreeMap<String, crate::mca::McaReport>>(&baseline.stdout)
            .unwrap_or_default();
    let candidate_reports =
        serde_json::from_str::<BTreeMap<String, crate::mca::McaReport>>(&candidate.stdout)
            .unwrap_or_default();
    let mut compared = 0usize;
    let mut improved = false;
    let mut regressions = Vec::new();
    for (function, before) in &baseline_reports {
        let Some(after) = candidate_reports.get(function) else {
            continue;
        };
        compared += 1;
        let epsilon = 1e-9;
        let no_regression = after.instructions <= before.instructions
            && after.total_cycles <= before.total_cycles
            && after.block_rthroughput <= before.block_rthroughput + epsilon
            && after.ipc + epsilon >= before.ipc
            && after.unsupported_instructions <= before.unsupported_instructions;
        if !no_regression {
            regressions.push(function.clone());
        }
        improved |= after.instructions < before.instructions
            || after.total_cycles < before.total_cycles
            || after.block_rthroughput + epsilon < before.block_rthroughput
            || after.ipc > before.ipc + epsilon;
    }
    candidate.passed =
        baseline.passed && candidate.passed && compared > 0 && regressions.is_empty() && improved;
    candidate.status = if candidate.passed {
        crate::assurance::ObligationStatus::Modeled
    } else {
        crate::assurance::ObligationStatus::Unknown
    };
    candidate.actual_cost_ms = candidate
        .actual_cost_ms
        .saturating_add(baseline.actual_cost_ms);
    candidate.stdout = serde_json::json!({
        "baseline": baseline_reports,
        "candidate": candidate_reports,
        "compared_functions": compared,
        "improved": improved,
        "regressions": regressions,
    })
    .to_string();
    if !candidate.passed {
        let explanation = if !baseline.passed {
            "baseline llvm-mca evidence was unavailable"
        } else if compared == 0 {
            "no baseline/candidate function pair could be compared"
        } else if !regressions.is_empty() {
            "candidate regressed at least one llvm-mca metric"
        } else {
            "candidate produced no llvm-mca improvement"
        };
        if !candidate.stderr.is_empty() {
            candidate.stderr.push('\n');
        }
        candidate.stderr.push_str(explanation);
    }
    candidate
}

/// Apply all edits to an isolated workspace and execute the selected evidence
/// actions against that exact candidate. Unsupported providers fail closed.
pub fn verify_candidate_evidence_in_sandbox(
    workspace_root: &Path,
    manifest_path: &Path,
    edits: &[SourceEdit],
    plan: &crate::assurance::EvidencePlan,
    test_filter: Option<&str>,
) -> Result<CandidateEvidenceVerification, String> {
    let workspace_root = workspace_root
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let manifest_path = if manifest_path.is_absolute() {
        manifest_path.to_path_buf()
    } else {
        workspace_root.join(manifest_path)
    }
    .canonicalize()
    .map_err(|error| error.to_string())?;
    let manifest_relative = manifest_path
        .strip_prefix(&workspace_root)
        .map_err(|_| "manifest path escapes workspace".to_string())?;
    let temp = tempfile::tempdir().map_err(|error| error.to_string())?;
    copy_workspace(&workspace_root, temp.path())?;

    let mut grouped = BTreeMap::<PathBuf, Vec<SourceEdit>>::new();
    for edit in edits {
        grouped
            .entry(relative_workspace_path(&workspace_root, &edit.file)?)
            .or_default()
            .push(edit.clone());
    }
    let changed_rust_files = grouped
        .keys()
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .cloned()
        .collect::<Vec<_>>();
    let baseline_mca = plan
        .selected_action_details
        .iter()
        .find(|action| action.provider.0 == "Mca")
        .map(|action| {
            run_candidate_mca(
                temp.path(),
                &changed_rust_files,
                "candidate-mca-baseline",
                Duration::from_millis(action.estimated_cost_ms.max(1)),
            )
        });
    let mut source_hashes = BTreeMap::new();
    for (relative, file_edits) in grouped {
        let original_path = workspace_root.join(&relative);
        let source = fs::read_to_string(&original_path).map_err(|error| error.to_string())?;
        let updated = apply_edits_safely(&source, &file_edits)?;
        if relative
            .extension()
            .is_some_and(|extension| extension == "rs")
        {
            syn::parse_file(&updated).map_err(|error| {
                format!(
                    "candidate AST validation failed for {}: {error}",
                    relative.display()
                )
            })?;
            crate::parameters::ParameterDependencyGraph::from_source(
                &updated,
                &relative.display().to_string(),
            )
            .map_err(|error| {
                format!(
                    "candidate parameter metadata validation failed for {}: {error}",
                    relative.display()
                )
            })?;
        }
        source_hashes.insert(
            relative.display().to_string(),
            SourceEdit::hash_source(&source),
        );
        fs::write(temp.path().join(relative), updated).map_err(|error| error.to_string())?;
    }

    let sandbox_manifest = temp.path().join(manifest_relative);
    let compiler = run_candidate_command(
        temp.path(),
        &sandbox_manifest,
        "candidate-mandatory-compiler",
        "Compiler",
        &["check".to_string()],
        Duration::from_millis(plan.estimated_cost_ms.max(1)),
    );
    let compile_passed = compiler.passed;
    let mut actions = vec![compiler];
    let mut failed_actions = Vec::new();
    for action in &plan.selected_action_details {
        let provider = action.provider.0.as_str();
        if provider == "Compiler" {
            continue;
        }
        let result = match provider {
            "StaticAst" => CandidateActionResult {
                action_id: action.id.0.clone(),
                provider: provider.to_string(),
                passed: true,
                status: crate::assurance::ObligationStatus::Modeled,
                command: Vec::new(),
                stdout: "candidate Rust AST and parameter metadata parsed successfully".to_string(),
                stderr: String::new(),
                actual_cost_ms: 0,
            },
            "Test" => {
                let mut arguments = vec!["test".to_string()];
                if let Some(filter) = test_filter.filter(|filter| !filter.is_empty()) {
                    arguments.push(filter.to_string());
                }
                run_candidate_command(
                    temp.path(),
                    &sandbox_manifest,
                    &action.id.0,
                    provider,
                    &arguments,
                    Duration::from_millis(action.estimated_cost_ms.max(1)),
                )
            }
            "Coverage" => run_candidate_command(
                temp.path(),
                &sandbox_manifest,
                &action.id.0,
                provider,
                &[
                    "llvm-cov".to_string(),
                    "--workspace".to_string(),
                    "--summary-only".to_string(),
                ],
                Duration::from_millis(action.estimated_cost_ms.max(1)),
            ),
            "Mca" => compare_candidate_mca(
                baseline_mca.as_ref().expect("Mca baseline was planned"),
                run_candidate_mca(
                    temp.path(),
                    &changed_rust_files,
                    &action.id.0,
                    Duration::from_millis(action.estimated_cost_ms.max(1)),
                ),
            ),
            "AtomicModel" => run_candidate_atomic_model(
                temp.path(),
                &changed_rust_files,
                &action.id.0,
                Duration::from_millis(action.estimated_cost_ms.max(1)),
            ),
            _ => CandidateActionResult {
                action_id: action.id.0.clone(),
                provider: provider.to_string(),
                passed: false,
                status: crate::assurance::ObligationStatus::Unknown,
                command: Vec::new(),
                stdout: String::new(),
                stderr: format!(
                    "candidate sandbox has no executable adapter for provider {provider}"
                ),
                actual_cost_ms: 0,
            },
        };
        if !result.passed {
            failed_actions.push(result.action_id.clone());
        }
        actions.push(result);
    }
    let passed = compile_passed && actions.iter().all(|action| action.passed);
    Ok(CandidateEvidenceVerification {
        passed,
        compile_passed,
        candidate_hash: candidate_hash(edits),
        source_hashes,
        actions,
        failed_actions,
    })
}

fn write_transaction_manifest(transaction: &RepairTransaction) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(transaction).map_err(|error| error.to_string())?;
    fs::write(&transaction.manifest_path, bytes).map_err(|error| error.to_string())
}

fn atomic_replace(path: &Path, contents: &str, candidate_hash: &str) -> Result<(), String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("invalid repair target path: {}", path.display()))?;
    let temporary = path.with_file_name(format!(".{name}.covopt-{candidate_hash}.tmp"));
    fs::copy(path, &temporary).map_err(|error| error.to_string())?;
    if let Err(error) = fs::write(&temporary, contents) {
        let _ = fs::remove_file(&temporary);
        return Err(error.to_string());
    }
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error.to_string());
    }
    Ok(())
}

/// Atomically apply a source-hash-bound set of edits and persist complete
/// originals for deterministic rollback. Every file is validated and backed up
/// before the first workspace file is changed.
pub fn apply_edits_transactionally(
    workspace_root: &Path,
    edits: &[SourceEdit],
) -> Result<RepairTransaction, String> {
    if edits.is_empty() {
        return Err("repair transaction requires at least one edit".to_string());
    }
    let workspace_root = workspace_root
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let candidate_hash = candidate_hash(edits);
    let mut grouped = BTreeMap::<PathBuf, Vec<SourceEdit>>::new();
    for edit in edits {
        grouped
            .entry(relative_workspace_path(&workspace_root, &edit.file)?)
            .or_default()
            .push(edit.clone());
    }

    struct PreparedFile {
        relative: PathBuf,
        absolute: PathBuf,
        before: String,
        after: String,
    }
    let mut prepared = Vec::new();
    for (relative, file_edits) in grouped {
        let absolute = workspace_root.join(&relative);
        let before = fs::read_to_string(&absolute).map_err(|error| error.to_string())?;
        let after = apply_edits_safely(&before, &file_edits)?;
        if relative
            .extension()
            .is_some_and(|extension| extension == "rs")
        {
            syn::parse_file(&after).map_err(|error| {
                format!(
                    "transaction AST validation failed for {}: {error}",
                    relative.display()
                )
            })?;
        }
        prepared.push(PreparedFile {
            relative,
            absolute,
            before,
            after,
        });
    }

    let transaction_base = workspace_root.join("target/covopt/transactions");
    fs::create_dir_all(&transaction_base).map_err(|error| error.to_string())?;
    let mut transaction_dir = transaction_base.join(&candidate_hash);
    let mut suffix = 1usize;
    while transaction_dir.exists() {
        transaction_dir = transaction_base.join(format!("{candidate_hash}-{suffix}"));
        suffix += 1;
    }
    let originals = transaction_dir.join("original");
    fs::create_dir_all(&originals).map_err(|error| error.to_string())?;

    let mut files = Vec::new();
    for file in &prepared {
        let backup = originals.join(&file.relative);
        if let Some(parent) = backup.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::write(&backup, &file.before).map_err(|error| error.to_string())?;
        files.push(TransactionFileRecord {
            file: file.relative.display().to_string(),
            before_hash: SourceEdit::hash_source(&file.before),
            after_hash: SourceEdit::hash_source(&file.after),
            backup: backup.display().to_string(),
        });
    }
    let manifest_path = transaction_dir.join("manifest.json");
    let mut transaction = RepairTransaction {
        version: 1,
        candidate_hash: candidate_hash.clone(),
        workspace: workspace_root.display().to_string(),
        status: TransactionStatus::Prepared,
        files,
        manifest_path: manifest_path.display().to_string(),
    };
    write_transaction_manifest(&transaction)?;

    let mut committed = Vec::<&PreparedFile>::new();
    for file in &prepared {
        if let Err(error) = atomic_replace(&file.absolute, &file.after, &candidate_hash) {
            for previous in committed.into_iter().rev() {
                let _ = atomic_replace(&previous.absolute, &previous.before, &candidate_hash);
            }
            transaction.status = TransactionStatus::RolledBack;
            let _ = write_transaction_manifest(&transaction);
            return Err(format!(
                "transaction failed while writing {}: {error}",
                file.relative.display()
            ));
        }
        committed.push(file);
    }
    transaction.status = TransactionStatus::Committed;
    write_transaction_manifest(&transaction)?;
    Ok(transaction)
}

/// Restore a committed repair transaction only when every current file still
/// matches the recorded candidate hash, preventing rollback from overwriting
/// subsequent developer edits.
pub fn rollback_transaction(manifest_path: &Path) -> Result<RepairTransaction, String> {
    let bytes = fs::read(manifest_path).map_err(|error| error.to_string())?;
    let mut transaction: RepairTransaction =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if transaction.status != TransactionStatus::Committed {
        return Err("only a committed repair transaction can be rolled back".to_string());
    }
    let workspace = PathBuf::from(&transaction.workspace);
    for file in &transaction.files {
        let current =
            fs::read_to_string(workspace.join(&file.file)).map_err(|error| error.to_string())?;
        if SourceEdit::hash_source(&current) != file.after_hash {
            return Err(format!(
                "rollback refused because {} changed after the repair",
                file.file
            ));
        }
    }
    for file in &transaction.files {
        let original = fs::read_to_string(&file.backup).map_err(|error| error.to_string())?;
        if SourceEdit::hash_source(&original) != file.before_hash {
            return Err(format!("rollback backup hash mismatch for {}", file.file));
        }
        atomic_replace(
            &workspace.join(&file.file),
            &original,
            &transaction.candidate_hash,
        )?;
    }
    transaction.status = TransactionStatus::RolledBack;
    write_transaction_manifest(&transaction)?;
    Ok(transaction)
}

pub fn verify_edits_in_sandbox(
    workspace_root: &Path,
    manifest_path: &Path,
    source_path: &Path,
    source: &str,
    edits: &[SourceEdit],
) -> Result<SandboxVerification, String> {
    let updated = apply_edits_safely(source, edits)?;
    let temp = tempfile::tempdir().map_err(|error| error.to_string())?;
    copy_workspace(workspace_root, temp.path())?;
    let relative_source = source_path
        .strip_prefix(workspace_root)
        .map_err(|error| error.to_string())?;
    let sandbox_source = temp.path().join(relative_source);
    fs::write(&sandbox_source, updated).map_err(|error| error.to_string())?;
    let relative_manifest = manifest_path
        .strip_prefix(workspace_root)
        .map_err(|error| error.to_string())?;
    let output = std::process::Command::new("cargo")
        .arg("check")
        .arg("--manifest-path")
        .arg(temp.path().join(relative_manifest))
        .output()
        .map_err(|error| error.to_string())?;
    Ok(SandboxVerification {
        passed: output.status.success(),
        compile_passed: output.status.success(),
        source_hash: SourceEdit::hash_source(source),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assurance::{
        EvidenceAction, EvidenceCoverage, EvidencePlan, PlanStatus, Severity, SourceLocation,
    };
    use crate::findings::{Finding, FindingKind};

    fn finding(id: &str, severity: Severity) -> Finding {
        Finding {
            id: FindingId(id.to_string()),
            kind: FindingKind::CloneInHotLoop,
            severity,
            location: SourceLocation {
                file: "x.rs".to_string(),
                line: 1,
            },
            function: Some("f".to_string()),
            evidence: Vec::new(),
            explanation: "x".to_string(),
            obligations: Vec::new(),
            repair_candidates: Vec::new(),
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

    #[test]
    fn source_hash_change_is_rejected() {
        let edit = SourceEdit::from_source("x.rs", "let x = 1;", 1, 8, 1, 9, "2").unwrap();
        assert!(apply_edits_safely("let x = 2;", &[edit]).is_err());
    }

    #[test]
    fn exact_repair_set_prefers_two_low_risk_edits_over_high_risk_combined_edit() {
        let findings = vec![
            finding("a", Severity::Critical),
            finding("b", Severity::Critical),
        ];
        let low = |id: &str, resolves: Vec<&str>| RepairCandidate {
            id: RepairCandidateId(id.to_string()),
            kind: RepairKind::BorrowInsteadOfClone,
            resolves: resolves
                .into_iter()
                .map(|id| FindingId(id.to_string()))
                .collect(),
            changes: Vec::new(),
            dependencies: Vec::new(),
            conflicts: Vec::new(),
            semantic_risk: RiskLevel::Low,
            api_risk: RiskLevel::Low,
            abi_risk: RiskLevel::Low,
            estimated_benefit: PerformanceDelta::default(),
            verification: Vec::new(),
            suggestion_only: false,
            description: String::new(),
        };
        let plan = plan_repairs(
            &findings,
            &[
                low("a", vec!["a"]),
                low("b", vec!["b"]),
                low("both", vec!["a", "b"]),
            ],
            &RepairPolicy::default(),
        );
        assert_eq!(plan.selected, vec![RepairCandidateId("both".to_string())]);
    }

    #[test]
    fn repair_transaction_round_trips_and_refuses_stale_rollback() {
        let workspace = tempfile::tempdir().unwrap();
        let source_dir = workspace.path().join("src");
        fs::create_dir_all(&source_dir).unwrap();
        let source_path = source_dir.join("lib.rs");
        let source = "pub fn value() -> usize { 1 }\n";
        fs::write(&source_path, source).unwrap();
        let start = source.find('1').unwrap();
        let edit =
            SourceEdit::from_source("src/lib.rs", source, 1, start, 1, start + 1, "2").unwrap();

        let transaction = apply_edits_transactionally(workspace.path(), &[edit]).unwrap();
        assert_eq!(transaction.status, TransactionStatus::Committed);
        assert_eq!(
            fs::read_to_string(&source_path).unwrap(),
            "pub fn value() -> usize { 2 }\n"
        );

        fs::write(&source_path, "pub fn value() -> usize { 3 }\n").unwrap();
        assert!(rollback_transaction(Path::new(&transaction.manifest_path)).is_err());
        fs::write(&source_path, "pub fn value() -> usize { 2 }\n").unwrap();
        let rolled_back = rollback_transaction(Path::new(&transaction.manifest_path)).unwrap();
        assert_eq!(rolled_back.status, TransactionStatus::RolledBack);
        assert_eq!(fs::read_to_string(source_path).unwrap(), source);
    }

    #[test]
    fn candidate_evidence_is_bound_to_the_patched_sandbox() {
        let workspace = tempfile::tempdir().unwrap();
        fs::create_dir_all(workspace.path().join("src")).unwrap();
        fs::write(
            workspace.path().join("Cargo.toml"),
            "[package]\nname = \"candidate-evidence\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        let source = "pub fn value() -> usize { 1 }\n";
        fs::write(workspace.path().join("src/lib.rs"), source).unwrap();
        let column = source.find('1').unwrap();
        let edit =
            SourceEdit::from_source("src/lib.rs", source, 1, column, 1, column + 1, "2").unwrap();
        let action = EvidenceAction::new("candidate-static", "StaticAst", Vec::new(), 1, 1.0);
        let plan = EvidencePlan {
            status: PlanStatus::Feasible,
            selected_actions: vec![action.id.clone()],
            selected_action_details: vec![action.clone()],
            candidate_actions: vec![action],
            rejected_actions: Vec::new(),
            coverage_before: empty_coverage(),
            expected_coverage: empty_coverage(),
            actual_coverage: None,
            estimated_cost_ms: 30_000,
            actual_cost_ms: None,
            infeasible_obligations: Vec::new(),
            validator_errors: Vec::new(),
        };

        let result = verify_candidate_evidence_in_sandbox(
            workspace.path(),
            &workspace.path().join("Cargo.toml"),
            &[edit],
            &plan,
            None,
        )
        .unwrap();
        assert!(result.passed, "{:#?}", result.failed_actions);
        assert!(result.compile_passed);
        assert_eq!(result.actions.len(), 2);
        assert_ne!(result.candidate_hash, SourceEdit::hash_source(source));
    }

    #[test]
    fn candidate_mca_requires_improvement_without_regression() {
        fn action(id: &str, report: crate::mca::McaReport) -> CandidateActionResult {
            CandidateActionResult {
                action_id: id.to_string(),
                provider: "Mca".to_string(),
                passed: true,
                status: crate::assurance::ObligationStatus::Modeled,
                command: Vec::new(),
                stdout: serde_json::to_string(&BTreeMap::from([("work", report)])).unwrap(),
                stderr: String::new(),
                actual_cost_ms: 1,
            }
        }
        let baseline = action(
            "baseline",
            crate::mca::McaReport {
                instructions: 10,
                total_cycles: 12,
                block_rthroughput: 4.0,
                ipc: 1.0,
                unsupported_instructions: 0,
                ..Default::default()
            },
        );
        let improved = action(
            "candidate",
            crate::mca::McaReport {
                instructions: 9,
                total_cycles: 10,
                block_rthroughput: 3.5,
                ipc: 1.1,
                unsupported_instructions: 0,
                ..Default::default()
            },
        );
        assert!(compare_candidate_mca(&baseline, improved).passed);

        let equal: BTreeMap<String, crate::mca::McaReport> =
            serde_json::from_str(&baseline.stdout).unwrap();
        let equal = action("equal", equal.into_values().next().unwrap());
        assert!(!compare_candidate_mca(&baseline, equal).passed);
    }
}
