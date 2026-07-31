//! Versioned assurance snapshots, assumption ledger, and drift comparison.

use crate::assurance::{AssuranceReport, Evidence, Obligation, ProofFrontier};
use crate::model::{AssumptionId, SnapshotId};
use crate::scope::ScopeEnvelope;
use covopt_schema::ParameterId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssumptionRecord {
    pub id: AssumptionId,
    pub source: String,
    #[serde(default)]
    pub affected_scopes: Vec<String>,
    pub validity_domain: String,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    pub last_verification: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AssumptionLedger {
    #[serde(default = "default_model_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub assumptions: Vec<AssumptionRecord>,
}

fn default_model_schema_version() -> u32 {
    crate::model::MODEL_SCHEMA_VERSION
}

impl AssumptionLedger {
    pub fn from_report(report: &AssuranceReport) -> Self {
        let mut ids = BTreeSet::new();
        let mut assumptions = Vec::new();
        if let Some(envelope) = &report.scope_envelope {
            for id in &envelope.assumptions {
                ids.insert(id.clone());
            }
            for node in &envelope.nodes {
                for id in &node.assumptions {
                    ids.insert(id.clone());
                }
            }
        }
        if let Some(frontier) = &report.proof_frontier {
            ids.extend(frontier.assumptions.iter().cloned());
        }
        for id in ids {
            let affected_scopes = report
                .scope_envelope
                .as_ref()
                .map(|envelope| {
                    envelope
                        .nodes
                        .iter()
                        .filter(|node| node.assumptions.contains(&id))
                        .map(|node| node.id.0.clone())
                        .collect()
                })
                .unwrap_or_default();
            assumptions.push(AssumptionRecord {
                id,
                source: "scope/proof-frontier".to_string(),
                affected_scopes,
                validity_domain: "current source/config/toolchain snapshot".to_string(),
                evidence: Vec::new(),
                last_verification: "current-check".to_string(),
                confidence: 0.5,
            });
        }
        Self {
            schema_version: crate::model::MODEL_SCHEMA_VERSION,
            assumptions,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssuranceSnapshot {
    pub schema_version: u32,
    pub id: SnapshotId,
    pub target: String,
    #[serde(default)]
    pub source_hashes: BTreeMap<String, String>,
    #[serde(default)]
    pub scope_envelope: Option<ScopeEnvelope>,
    pub obligations: Vec<Obligation>,
    #[serde(default)]
    pub assumptions: AssumptionLedger,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    #[serde(default)]
    pub toolchain_cpu_config: BTreeMap<String, String>,
    #[serde(default)]
    pub selected_trials: Option<crate::trial_selection::TrialPlan>,
    #[serde(default)]
    pub counterexamples: Vec<crate::assurance::Counterexample>,
    #[serde(default)]
    pub proof_frontier: Option<ProofFrontier>,
    #[serde(default)]
    pub parameter_graph: Option<crate::parameters::ParameterDependencyGraph>,
    #[serde(default)]
    pub metadata_index: Option<crate::static_analysis::SourceMetadataIndex>,
}

impl AssuranceSnapshot {
    pub fn from_report(target: &str, source: Option<&Path>, report: &AssuranceReport) -> Self {
        let mut source_hashes = BTreeMap::new();
        if let Some(source) = source
            && let Ok(content) = fs::read_to_string(source)
        {
            source_hashes.insert(
                source.to_string_lossy().to_string(),
                crate::repair::SourceEdit::hash_source(&content),
            );
        }
        let evidence = report
            .obligations
            .iter()
            .flat_map(|obligation| obligation.evidence.clone())
            .collect();
        Self {
            schema_version: crate::model::MODEL_SCHEMA_VERSION,
            id: SnapshotId::new(format!(
                "{target}:{}",
                source_hashes.values().next().cloned().unwrap_or_default()
            )),
            target: target.to_string(),
            source_hashes,
            scope_envelope: report.scope_envelope.clone(),
            obligations: report.obligations.clone(),
            assumptions: AssumptionLedger::from_report(report),
            evidence,
            toolchain_cpu_config: BTreeMap::from([
                ("rustc".to_string(), rustc_version()),
                (
                    "target".to_string(),
                    std::env::var("TARGET").unwrap_or_else(|_| "unknown".to_string()),
                ),
            ]),
            selected_trials: report.trial_plan.clone(),
            counterexamples: report
                .proof_frontier
                .as_ref()
                .map(|frontier| frontier.counterexamples.clone())
                .unwrap_or_default(),
            proof_frontier: report.proof_frontier.clone(),
            parameter_graph: report.parameter_graph.clone(),
            metadata_index: report.metadata_index.clone(),
        }
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), String> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let data = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        fs::write(path, data).map_err(|error| error.to_string())
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let data = fs::read_to_string(path.as_ref()).map_err(|error| error.to_string())?;
        let value = crate::model::migrate_legacy_json(
            serde_json::from_str(&data).map_err(|error| error.to_string())?,
        )?;
        serde_json::from_value(value).map_err(|error| error.to_string())
    }
}

fn rustc_version() -> String {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssumptionDrift {
    pub changed: Vec<AssumptionId>,
    pub invalidated_evidence: Vec<String>,
    pub affected_scopes: Vec<String>,
    pub critical: bool,
}

pub fn compare_assumptions(base: &AssumptionLedger, current: &AssumptionLedger) -> AssumptionDrift {
    let base_map = base
        .assumptions
        .iter()
        .map(|assumption| (assumption.id.clone(), assumption))
        .collect::<BTreeMap<_, _>>();
    let current_map = current
        .assumptions
        .iter()
        .map(|assumption| (assumption.id.clone(), assumption))
        .collect::<BTreeMap<_, _>>();
    let changed = current_map
        .iter()
        .filter(|(id, assumption)| {
            base_map.get(id).is_none_or(|previous| {
                previous.validity_domain != assumption.validity_domain
                    || (previous.confidence - assumption.confidence).abs() > f64::EPSILON
            })
        })
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    let affected_scopes = current_map
        .iter()
        .filter(|(id, _)| changed.contains(id))
        .flat_map(|(_, assumption)| assumption.affected_scopes.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    AssumptionDrift {
        invalidated_evidence: changed.iter().map(|id| id.0.clone()).collect(),
        critical: !changed.is_empty(),
        changed,
        affected_scopes,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticDrift {
    #[serde(default = "default_drift_status")]
    pub status: String,
    pub expected: Vec<String>,
    pub unexplained: Vec<String>,
    pub first_scope: Option<String>,
    pub witness: Option<String>,
    pub proof_frontier: Option<ProofFrontier>,
    pub critical: bool,
    #[serde(default)]
    pub parameter_added: Vec<ParameterId>,
    #[serde(default)]
    pub parameter_removed: Vec<ParameterId>,
    #[serde(default)]
    pub parameter_remapped: Vec<String>,
}

fn default_drift_status() -> String {
    "Unknown".to_string()
}

pub fn compare_snapshots(base: &AssuranceSnapshot, current: &AssuranceSnapshot) -> SemanticDrift {
    let mut expected = Vec::new();
    let mut unexplained = Vec::new();
    let (parameter_added, parameter_removed, parameter_remapped) = compare_parameter_graphs(
        base.parameter_graph.as_ref(),
        current.parameter_graph.as_ref(),
    );
    unexplained.extend(
        parameter_added
            .iter()
            .map(|id| format!("new parameter: {id}")),
    );
    unexplained.extend(
        parameter_removed
            .iter()
            .map(|id| format!("removed parameter: {id}")),
    );
    if let (Some(base_metadata), Some(current_metadata)) =
        (&base.metadata_index, &current.metadata_index)
    {
        let base_targets = base_metadata
            .targets
            .iter()
            .map(|target| target.id.clone())
            .collect::<BTreeSet<_>>();
        let current_targets = current_metadata
            .targets
            .iter()
            .map(|target| target.id.clone())
            .collect::<BTreeSet<_>>();
        unexplained.extend(
            current_targets
                .difference(&base_targets)
                .map(|id| format!("new target metadata: {id}")),
        );
        unexplained.extend(
            base_targets
                .difference(&current_targets)
                .map(|id| format!("removed target metadata: {id}")),
        );
    }
    expected.extend(
        parameter_remapped
            .iter()
            .map(|mapping| format!("parameter remapped: {mapping}")),
    );
    for (path, hash) in &current.source_hashes {
        if base.source_hashes.get(path) != Some(hash) {
            expected.push(format!("source changed: {path}"));
        }
    }
    let base_nodes = base
        .scope_envelope
        .as_ref()
        .map(|envelope| {
            envelope
                .nodes
                .iter()
                .map(|node| (&node.id, node))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let current_nodes = current
        .scope_envelope
        .as_ref()
        .map(|envelope| {
            envelope
                .nodes
                .iter()
                .map(|node| (&node.id, node))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let mut first_scope = None;
    for (id, node) in &current_nodes {
        let Some(previous) = base_nodes.get(id) else {
            unexplained.push(format!("new scope: {}", id.0));
            first_scope.get_or_insert_with(|| id.0.clone());
            continue;
        };
        if node.local_fitted_complexity != previous.local_fitted_complexity
            || node.inclusive_fitted_complexity != previous.inclusive_fitted_complexity
        {
            unexplained.push(format!("complexity fit drift: {}", id.0));
            first_scope.get_or_insert_with(|| id.0.clone());
        }
    }
    let assumption_drift = compare_assumptions(&base.assumptions, &current.assumptions);
    unexplained.extend(
        assumption_drift
            .changed
            .iter()
            .map(|id| format!("assumption drift: {}", id.0)),
    );
    SemanticDrift {
        status: if unexplained.is_empty() {
            "Stable".to_string()
        } else {
            "Critical".to_string()
        },
        critical: !unexplained.is_empty(),
        witness: unexplained.first().cloned(),
        proof_frontier: current.proof_frontier.clone(),
        expected,
        unexplained,
        first_scope,
        parameter_added,
        parameter_removed,
        parameter_remapped,
    }
}

fn compare_parameter_graphs(
    base: Option<&crate::parameters::ParameterDependencyGraph>,
    current: Option<&crate::parameters::ParameterDependencyGraph>,
) -> (Vec<ParameterId>, Vec<ParameterId>, Vec<String>) {
    let Some(base) = base else {
        return (Vec::new(), Vec::new(), Vec::new());
    };
    let Some(current) = current else {
        return (Vec::new(), Vec::new(), Vec::new());
    };
    let (added, removed) = base.diff_ids(current);
    let mut remapped = Vec::new();
    let mut remapped_added = BTreeSet::new();
    let mut remapped_removed = BTreeSet::new();
    for removed_id in &removed {
        let Some(previous) = base.parameters.get(removed_id) else {
            continue;
        };
        let candidates = added
            .iter()
            .filter_map(|added_id| {
                current
                    .parameters
                    .get(added_id)
                    .map(|record| (added_id, record))
            })
            .filter(|(_, record)| {
                record.descriptor.default == previous.descriptor.default
                    && record.descriptor.class == previous.descriptor.class
            })
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        if candidates.len() == 1 {
            remapped.push(format!("{} -> {}", removed_id, candidates[0]));
            remapped_removed.insert(removed_id.clone());
            remapped_added.insert(candidates[0].clone());
        }
    }
    (
        added
            .into_iter()
            .filter(|id| !remapped_added.contains(id))
            .collect(),
        removed
            .into_iter()
            .filter(|id| !remapped_removed.contains(id))
            .collect(),
        remapped,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assumption_drift_invalidates_affected_evidence() {
        let base = AssumptionLedger {
            schema_version: 1,
            assumptions: vec![AssumptionRecord {
                id: AssumptionId::new("cpu"),
                source: "config".to_string(),
                affected_scopes: vec!["scope-a".to_string()],
                validity_domain: "x86".to_string(),
                evidence: Vec::new(),
                last_verification: "old".to_string(),
                confidence: 1.0,
            }],
        };
        let mut current = base.clone();
        current.assumptions[0].validity_domain = "arm".to_string();
        let drift = compare_assumptions(&base, &current);
        assert!(drift.critical);
        assert_eq!(drift.affected_scopes, vec!["scope-a"]);
    }
}
