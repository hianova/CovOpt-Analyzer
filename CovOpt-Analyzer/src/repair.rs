//! Repair candidates, minimal repair-set planning, verification, and safe apply.

use crate::findings::FindingId;
use crate::model::ObligationId;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

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

pub fn verify_edits_in_sandbox(
    workspace_root: &Path,
    manifest_path: &Path,
    source_path: &Path,
    source: &str,
    edits: &[SourceEdit],
) -> Result<SandboxVerification, String> {
    let updated = apply_edits_safely(source, edits)?;
    let temp = tempfile::tempdir().map_err(|error| error.to_string())?;
    for entry in walkdir::WalkDir::new(workspace_root)
        .into_iter()
        .filter_map(Result::ok)
    {
        let relative = entry
            .path()
            .strip_prefix(workspace_root)
            .map_err(|error| error.to_string())?;
        if relative
            .components()
            .any(|component| matches!(component.as_os_str().to_str(), Some("target" | ".git")))
        {
            continue;
        }
        let destination = temp.path().join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&destination).map_err(|error| error.to_string())?;
        } else {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            fs::copy(entry.path(), &destination).map_err(|error| error.to_string())?;
        }
    }
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
    use crate::assurance::{Severity, SourceLocation};
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
}
