//! Contract-gated atomic ordering synthesis.
//!
//! Synthesis produces suggestions and unified diffs only.  It never edits the
//! source tree.  A bounded `Modeled` result is explicitly not a proof.

use crate::atomic_model::{
    AtomicContract, AtomicEvent, BoundedCheckResult, EventKind, ModelBounds, ModelStatus,
    RustOrdering, SourceSpan, check_bounded, legal_orderings,
};
use crate::mca::{McaReport, McaRunner};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SynthesisStatus {
    Suggested,
    NoChange,
    MissingContract,
    BaselineUnsafe,
    Unknown,
    TimedOut,
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtomicCandidate {
    pub id: String,
    pub parent: Option<String>,
    pub orderings: BTreeMap<usize, RustOrdering>,
    pub changed_events: Vec<usize>,
    pub legality: String,
    pub model: Option<BoundedCheckResult>,
    pub estimated_fence_cost: usize,
    pub source_changes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McaEvaluation {
    pub cpu: String,
    pub report: Option<McaReport>,
    pub error: Option<String>,
    pub confidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtomicPatch {
    pub source_path: String,
    pub source_hash: String,
    pub unified_diff: String,
    pub replacements: Vec<AtomicReplacement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtomicReplacement {
    pub event_id: usize,
    pub span: SourceSpan,
    pub old: String,
    pub new: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtomicAnalysis {
    pub source_path: String,
    pub source_hash: String,
    pub events: Vec<AtomicEvent>,
    pub unresolved_events: Vec<usize>,
    pub baseline: Option<BoundedCheckResult>,
    pub status: SynthesisStatus,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtomicSynthesisRequest {
    pub source_path: String,
    pub source: String,
    pub events: Vec<AtomicEvent>,
    pub contract: Option<AtomicContract>,
    pub bounds: ModelBounds,
    pub budget_ms: u64,
    #[serde(default)]
    pub mca_assembly: Option<String>,
    #[serde(default)]
    pub mca_cpus: Vec<String>,
    #[serde(default)]
    pub allow_synthesis: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtomicSynthesisResult {
    pub source_path: String,
    pub source_hash: String,
    pub status: SynthesisStatus,
    pub baseline: Option<BoundedCheckResult>,
    pub selected: Option<AtomicCandidate>,
    pub patch: Option<AtomicPatch>,
    #[serde(default)]
    pub mca: Vec<McaEvaluation>,
    #[serde(default)]
    pub rejected: Vec<AtomicCandidate>,
    pub summary: String,
    pub bounded_scope: String,
}

fn fnv1a(value: &str) -> String {
    let hash = value.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    format!("{:016x}", hash)
}

pub fn source_hash(source: &str) -> String {
    fnv1a(source)
}

pub fn analyze_atomic(request: &AtomicSynthesisRequest) -> AtomicAnalysis {
    let baseline = request
        .contract
        .as_ref()
        .map(|contract| check_bounded(&request.events, contract, &request.bounds));
    let unresolved_events = request
        .events
        .iter()
        .filter(|event| {
            matches!(
                event.receiver_type,
                crate::atomic_model::ReceiverType::Unknown
            )
        })
        .map(|event| event.id)
        .collect::<Vec<_>>();
    let status = if request.contract.is_none() {
        SynthesisStatus::MissingContract
    } else if baseline
        .as_ref()
        .is_some_and(|result| result.status == ModelStatus::Counterexample)
    {
        SynthesisStatus::BaselineUnsafe
    } else if baseline
        .as_ref()
        .is_some_and(|result| result.status == ModelStatus::Unknown)
    {
        SynthesisStatus::Unknown
    } else {
        SynthesisStatus::NoChange
    };
    AtomicAnalysis {
        source_path: request.source_path.clone(),
        source_hash: source_hash(&request.source),
        events: request.events.clone(),
        unresolved_events,
        baseline,
        status,
        summary: "atomic analysis is bounded and does not establish a proof".to_string(),
    }
}

fn candidate_orderings(event: &AtomicEvent) -> Vec<RustOrdering> {
    let current = event.ordering.unwrap_or(RustOrdering::SeqCst);
    legal_orderings(event.kind, Some(current))
        .into_iter()
        .filter(|ordering| ordering.rank() <= current.rank())
        .collect()
}

pub fn generate_candidates(events: &[AtomicEvent], max_candidates: usize) -> Vec<AtomicCandidate> {
    let mut result = Vec::new();
    for event in events {
        if event.ordering.is_none()
            || !matches!(
                event.kind,
                EventKind::AtomicLoad
                    | EventKind::AtomicStore
                    | EventKind::AtomicRmw
                    | EventKind::CompareExchangeSuccess
                    | EventKind::CompareExchangeFailure
                    | EventKind::Fence
            )
        {
            continue;
        }
        let current = event.ordering.unwrap();
        for ordering in candidate_orderings(event) {
            if ordering == current {
                continue;
            }
            let mut assignments = BTreeMap::new();
            assignments.insert(event.id, ordering);
            result.push(AtomicCandidate {
                id: format!("weaken-{}-{}", event.id, ordering.as_rust()),
                parent: None,
                orderings: assignments,
                changed_events: vec![event.id],
                legality: "Rust ordering domain accepted".to_string(),
                model: None,
                estimated_fence_cost: usize::from(ordering != RustOrdering::Relaxed),
                source_changes: 1,
            });
            if result.len() >= max_candidates.max(1) {
                return result;
            }
        }
    }
    result
}

fn apply_candidate(events: &[AtomicEvent], candidate: &AtomicCandidate) -> Vec<AtomicEvent> {
    events
        .iter()
        .cloned()
        .map(|mut event| {
            if let Some(ordering) = candidate.orderings.get(&event.id) {
                event.ordering = Some(*ordering);
            }
            event
        })
        .collect()
}

pub fn verify_candidate(
    events: &[AtomicEvent],
    contract: &AtomicContract,
    bounds: &ModelBounds,
    candidate: &AtomicCandidate,
) -> BoundedCheckResult {
    let updated = apply_candidate(events, candidate);
    check_bounded(&updated, contract, bounds)
}

fn offset_for(source: &str, line: usize, column: usize) -> Option<usize> {
    if line == 0 {
        return None;
    }
    let mut offset: usize = 0;
    for (index, current) in source.split_inclusive('\n').enumerate() {
        if index + 1 == line {
            return Some(offset.saturating_add(column.min(current.trim_end_matches('\n').len())));
        }
        offset = offset.saturating_add(current.len());
    }
    (line == source.lines().count() + 1).then_some(source.len())
}

fn make_patch(
    source_path: &str,
    source: &str,
    events: &[AtomicEvent],
    candidate: &AtomicCandidate,
) -> Option<AtomicPatch> {
    let mut replacements = Vec::new();
    let mut updated = source.to_string();
    let mut locations = candidate
        .orderings
        .iter()
        .filter_map(|(id, ordering)| {
            events
                .iter()
                .find(|event| event.id == *id)
                .map(|event| (*id, event, *ordering))
        })
        .collect::<Vec<_>>();
    locations.sort_by(|left, right| {
        right
            .1
            .ordering_source
            .as_ref()
            .map(|span| span.start_line)
            .cmp(&left.1.ordering_source.as_ref().map(|span| span.start_line))
    });
    for (event_id, event, ordering) in locations {
        let span = event.ordering_source.as_ref()?;
        let start = offset_for(&updated, span.start_line, span.start_column)?;
        let end = offset_for(&updated, span.end_line, span.end_column)?;
        if end < start || end > updated.len() {
            return None;
        }
        let old = updated.get(start..end)?.to_string();
        let new = format!("Ordering::{}", ordering.as_rust());
        let replacement = if old.contains("::") {
            new
        } else {
            ordering.as_rust().to_string()
        };
        updated.replace_range(start..end, &replacement);
        replacements.push(AtomicReplacement {
            event_id,
            span: span.clone(),
            old,
            new: replacement,
        });
    }
    if updated == source {
        return None;
    }
    let old_lines = source.lines().collect::<Vec<_>>();
    let new_lines = updated.lines().collect::<Vec<_>>();
    let mut diff = format!("--- {}\n+++ {}\n", source_path, source_path);
    for (old, new) in old_lines.iter().zip(new_lines.iter()) {
        if old != new {
            diff.push_str(&format!("@@\n-{}\n+{}\n", old, new));
        }
    }
    Some(AtomicPatch {
        source_path: source_path.to_string(),
        source_hash: source_hash(source),
        unified_diff: diff,
        replacements,
    })
}

/// Convert a synthesized atomic patch into the shared repair representation so
/// it can pass through candidate-bound sandbox verification and transactional
/// apply instead of remaining a display-only diff.
pub fn patch_source_edits(patch: &AtomicPatch) -> Vec<crate::repair::SourceEdit> {
    patch
        .replacements
        .iter()
        .map(|replacement| crate::repair::SourceEdit {
            file: patch.source_path.clone(),
            start_line: replacement.span.start_line,
            start_column: replacement.span.start_column,
            end_line: replacement.span.end_line,
            end_column: replacement.span.end_column,
            replacement: replacement.new.clone(),
            original_text: replacement.old.clone(),
            source_hash: patch.source_hash.clone(),
        })
        .collect()
}

fn run_mca(request: &AtomicSynthesisRequest) -> Vec<McaEvaluation> {
    let Some(assembly) = &request.mca_assembly else {
        return Vec::new();
    };
    request
        .mca_cpus
        .iter()
        .map(
            |cpu| match McaRunner::new(Some(cpu.clone())).run(assembly) {
                Ok(report) => McaEvaluation {
                    cpu: cpu.clone(),
                    report: Some(report),
                    error: None,
                    confidence: "measured".to_string(),
                },
                Err(error) => McaEvaluation {
                    cpu: cpu.clone(),
                    report: None,
                    error: Some(error),
                    confidence: "unsupported-or-unavailable".to_string(),
                },
            },
        )
        .collect()
}

pub fn synthesize(request: &AtomicSynthesisRequest) -> AtomicSynthesisResult {
    let started = Instant::now();
    let source_hash = source_hash(&request.source);
    let Some(contract) = &request.contract else {
        return AtomicSynthesisResult {
            source_path: request.source_path.clone(),
            source_hash,
            status: SynthesisStatus::MissingContract,
            baseline: None,
            selected: None,
            patch: None,
            mca: Vec::new(),
            rejected: Vec::new(),
            summary: "no correctness contract: analysis only; synthesis is blocked".to_string(),
            bounded_scope: "not modeled".to_string(),
        };
    };
    if !request.allow_synthesis {
        return AtomicSynthesisResult {
            source_path: request.source_path.clone(),
            source_hash,
            status: SynthesisStatus::Invalid,
            baseline: None,
            selected: None,
            patch: None,
            mca: Vec::new(),
            rejected: Vec::new(),
            summary: "atomic synthesis is not enabled for this target".to_string(),
            bounded_scope: "not modeled".to_string(),
        };
    }
    let baseline = check_bounded(&request.events, contract, &request.bounds);
    if baseline.status == ModelStatus::Unknown {
        return AtomicSynthesisResult {
            source_path: request.source_path.clone(),
            source_hash,
            status: SynthesisStatus::Unknown,
            baseline: Some(baseline.clone()),
            selected: None,
            patch: None,
            mca: run_mca(request),
            rejected: Vec::new(),
            summary: "baseline exceeded bounded model; no weakening was suggested".to_string(),
            bounded_scope: baseline.scope.clone(),
        };
    }
    let mut candidates = generate_candidates(&request.events, 256);
    let mut rejected = Vec::new();
    let mut selected = None;
    while let Some(mut candidate) = candidates.pop() {
        if started.elapsed().as_millis() as u64 >= request.budget_ms {
            break;
        }
        let model = verify_candidate(&request.events, contract, &request.bounds, &candidate);
        candidate.model = Some(model.clone());
        if model.status == ModelStatus::Modeled {
            selected = Some(candidate);
            break;
        }
        rejected.push(candidate);
    }
    let Some(candidate) = selected else {
        let status = if started.elapsed().as_millis() as u64 >= request.budget_ms {
            SynthesisStatus::TimedOut
        } else {
            SynthesisStatus::NoChange
        };
        return AtomicSynthesisResult {
            source_path: request.source_path.clone(),
            source_hash,
            status,
            baseline: Some(baseline.clone()),
            selected: None,
            patch: None,
            mca: run_mca(request),
            rejected,
            summary: "no bounded-safe ordering candidate found".to_string(),
            bounded_scope: baseline.scope.clone(),
        };
    };
    let patch = make_patch(
        &request.source_path,
        &request.source,
        &request.events,
        &candidate,
    );
    let mca = run_mca(request);
    let status = if patch.is_some() {
        SynthesisStatus::Suggested
    } else {
        SynthesisStatus::Invalid
    };
    AtomicSynthesisResult {
        source_path: request.source_path.clone(),
        source_hash,
        status,
        baseline: Some(baseline.clone()),
        selected: Some(candidate),
        patch,
        mca,
        rejected,
        summary: "bounded-safe suggestion generated; verify before applying".to_string(),
        bounded_scope: baseline.scope,
    }
}

pub fn request_from_file(
    path: impl AsRef<Path>,
    contract: Option<AtomicContract>,
    bounds: ModelBounds,
    budget_ms: u64,
    allow_synthesis: bool,
) -> Result<AtomicSynthesisRequest, String> {
    let source = fs::read_to_string(path.as_ref()).map_err(|error| error.to_string())?;
    let events = crate::atomic_model::extract_atomic_events(&source, path.as_ref())?;
    Ok(AtomicSynthesisRequest {
        source_path: path.as_ref().display().to_string(),
        source,
        events,
        contract,
        bounds,
        budget_ms,
        mca_assembly: None,
        mca_cpus: Vec::new(),
        allow_synthesis,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atomic_model::{ContractKind, extract_atomic_events};

    fn contract() -> AtomicContract {
        AtomicContract {
            name: "publication".to_string(),
            kind: ContractKind::Publication,
            forbidden_outcomes: Vec::new(),
            visibility: Vec::new(),
            single_writer: false,
            readers: Vec::new(),
            init_publication: true,
            mutex_exclusion: false,
        }
    }

    #[test]
    fn missing_contract_blocks_synthesis() {
        let source = "use std::sync::atomic::{AtomicUsize, Ordering}; static X: AtomicUsize = AtomicUsize::new(0); fn f() { X.store(1, Ordering::SeqCst); }";
        let events = extract_atomic_events(source, "x.rs").unwrap();
        let result = synthesize(&AtomicSynthesisRequest {
            source_path: "x.rs".to_string(),
            source: source.to_string(),
            events,
            contract: None,
            bounds: ModelBounds::default(),
            budget_ms: 1000,
            mca_assembly: None,
            mca_cpus: Vec::new(),
            allow_synthesis: true,
        });
        assert_eq!(result.status, SynthesisStatus::MissingContract);
    }

    #[test]
    fn synthesis_does_not_modify_source() {
        let source = "use std::sync::atomic::{AtomicUsize, Ordering}; static X: AtomicUsize = AtomicUsize::new(0); fn f() { X.store(1, Ordering::SeqCst); }";
        let events = extract_atomic_events(source, "x.rs").unwrap();
        let result = synthesize(&AtomicSynthesisRequest {
            source_path: "x.rs".to_string(),
            source: source.to_string(),
            events,
            contract: Some(contract()),
            bounds: ModelBounds::default(),
            budget_ms: 1000,
            mca_assembly: None,
            mca_cpus: Vec::new(),
            allow_synthesis: true,
        });
        assert_eq!(source_hash(source), result.source_hash);
        assert!(
            result
                .patch
                .as_ref()
                .is_none_or(|patch| patch.source_hash == source_hash(source))
        );
        if let Some(patch) = result.patch {
            let edits = patch_source_edits(&patch);
            let updated = crate::repair::apply_edits_safely(source, &edits).unwrap();
            assert_ne!(updated, source);
            assert!(syn::parse_file(&updated).is_ok());
        }
    }
}
