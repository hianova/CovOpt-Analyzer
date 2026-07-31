//! Shared static/dynamic trace IR and bounded temporal/relational checking.
//!
//! Atomic, temporal, relational, and adversarial providers consume this IR.
//! The checker is deliberately bounded and reports `Unknown` when a requested
//! proof exceeds its explicit bound or lacks a fairness assumption.

use crate::assurance::{Counterexample, ObligationStatus};
use crate::atomic_model::RustOrdering;
use crate::model::{SampleKey, ScopeId, TraceId};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};
use syn::spanned::Spanned;
use syn::visit::Visit;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TraceEventKind {
    FunctionEnter,
    FunctionExit,
    Branch,
    Read,
    Write,
    AtomicLoad,
    AtomicStore,
    AtomicRmw,
    Fence,
    Lock,
    Unlock,
    Spawn,
    Join,
    Await,
    Wake,
    Allocate,
    Free,
    Panic,
    Error,
    Return,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEvent {
    pub trace: TraceId,
    pub sequence: u64,
    pub thread: String,
    pub logical_time: u64,
    pub scope: Option<ScopeId>,
    pub kind: TraceEventKind,
    pub operation: String,
    pub observed_value: Option<String>,
    pub ordering: Option<RustOrdering>,
    pub source: Option<crate::assurance::SourceLocation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEdge {
    pub from: u64,
    pub to: u64,
    pub relation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticEventGraph {
    pub schema_version: u32,
    pub events: Vec<TraceEvent>,
    #[serde(default)]
    pub edges: Vec<TraceEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trace {
    pub schema_version: u32,
    pub id: TraceId,
    pub sample: SampleKey,
    pub events: Vec<TraceEvent>,
    #[serde(default)]
    pub static_graph: Option<StaticEventGraph>,
}

impl Trace {
    pub fn new(id: TraceId, sample: SampleKey) -> Self {
        Self {
            schema_version: crate::model::MODEL_SCHEMA_VERSION,
            id,
            sample,
            events: Vec::new(),
            static_graph: None,
        }
    }

    pub fn push(&mut self, mut event: TraceEvent) {
        event.trace = self.id.clone();
        event.sequence = self.events.len() as u64;
        event.logical_time = event.sequence;
        self.events.push(event);
    }

    pub fn deterministic_bytes(&self) -> Result<Vec<u8>, String> {
        let mut normalized = self.clone();
        normalized.events.sort_by_key(|event| event.sequence);
        serde_json::to_vec(&normalized).map_err(|error| error.to_string())
    }

    pub fn operation_names(&self) -> impl Iterator<Item = &str> {
        self.events.iter().map(|event| event.operation.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemporalOperator {
    Always,
    Eventually,
    Until,
    WithinSteps,
    BoundedWait,
    NoDeadlock,
    NoStarvation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalContract {
    pub name: String,
    pub operator: TemporalOperator,
    pub event: String,
    #[serde(default)]
    pub until_event: Option<String>,
    pub bound: usize,
    #[serde(default)]
    pub fairness_assumption: Option<String>,
}

impl TemporalContract {
    pub fn validate(&self) -> Result<(), String> {
        if self.bound == 0 {
            return Err("temporal bound must be greater than zero".to_string());
        }
        if matches!(self.operator, TemporalOperator::Until) && self.until_event.is_none() {
            return Err("until contracts require until_event".to_string());
        }
        if matches!(
            self.operator,
            TemporalOperator::Eventually
                | TemporalOperator::BoundedWait
                | TemporalOperator::NoStarvation
        ) && self.fairness_assumption.is_none()
        {
            return Err("liveness contracts require an explicit fairness assumption".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalCheckResult {
    pub status: ObligationStatus,
    pub explored_events: usize,
    pub bound: usize,
    pub counterexample: Option<Counterexample>,
    pub summary: String,
}

pub fn check_temporal(
    trace: &Trace,
    contract: &TemporalContract,
    timeout: Duration,
) -> Result<TemporalCheckResult, String> {
    contract.validate()?;
    let started = Instant::now();
    let events = trace.events.iter().take(contract.bound).collect::<Vec<_>>();
    if started.elapsed() > timeout {
        return Ok(TemporalCheckResult {
            status: ObligationStatus::Unknown,
            explored_events: events.len(),
            bound: contract.bound,
            counterexample: None,
            summary: "temporal search timed out before completing the bound".to_string(),
        });
    }
    let violation_at = match contract.operator {
        TemporalOperator::Always => events
            .iter()
            .position(|event| event.operation == contract.event),
        TemporalOperator::Eventually
        | TemporalOperator::WithinSteps
        | TemporalOperator::BoundedWait => {
            (!events.iter().any(|event| event.operation == contract.event)).then_some(events.len())
        }
        TemporalOperator::Until => {
            let Some(until) = contract.until_event.as_deref() else {
                return Err("until contracts require until_event".to_string());
            };
            let until_index = events.iter().position(|event| event.operation == until);
            match until_index {
                Some(index) => events[..index]
                    .iter()
                    .position(|event| event.operation == contract.event),
                None => Some(events.len()),
            }
        }
        TemporalOperator::NoDeadlock => detect_deadlock(&events),
        TemporalOperator::NoStarvation => detect_starvation(&events),
    };
    if let Some(index) = violation_at {
        let event = events
            .get(index.saturating_sub(1))
            .or_else(|| events.last());
        return Ok(TemporalCheckResult {
            status: ObligationStatus::Failed,
            explored_events: events.len(),
            bound: contract.bound,
            counterexample: Some(Counterexample {
                id: TraceId::new(format!("{}:counterexample:{index}", trace.id)),
                obligation_id: None,
                scope: event.and_then(|event| event.scope.clone()),
                summary: format!("temporal contract '{}' violated", contract.name),
                minimized: true,
                details: Some(serde_json::json!({
                    "operator": contract.operator,
                    "event": contract.event,
                    "sequence": events.iter().take(index.saturating_add(1)).collect::<Vec<_>>(),
                })),
            }),
            summary: format!("bounded temporal violation at event {index}"),
        });
    }
    Ok(TemporalCheckResult {
        status: ObligationStatus::Modeled,
        explored_events: events.len(),
        bound: contract.bound,
        counterexample: None,
        summary: format!("no violation found within {} events", events.len()),
    })
}

fn detect_deadlock(events: &[&TraceEvent]) -> Option<usize> {
    if events.len() < 2 {
        return None;
    }
    let last = events.last()?;
    (matches!(last.kind, TraceEventKind::Lock | TraceEventKind::Await)
        && !events.iter().skip(events.len() / 2).any(|event| {
            matches!(
                event.kind,
                TraceEventKind::Unlock | TraceEventKind::Wake | TraceEventKind::Join
            )
        }))
    .then_some(events.len().saturating_sub(1))
}

fn detect_starvation(events: &[&TraceEvent]) -> Option<usize> {
    let mut spawned = std::collections::BTreeSet::new();
    let mut progressed = std::collections::BTreeSet::new();
    for event in events {
        if matches!(event.kind, TraceEventKind::Spawn) {
            spawned.insert(event.thread.clone());
        }
        if matches!(
            event.kind,
            TraceEventKind::FunctionEnter
                | TraceEventKind::FunctionExit
                | TraceEventKind::Wake
                | TraceEventKind::Join
        ) {
            progressed.insert(event.thread.clone());
        }
    }
    spawned
        .difference(&progressed)
        .next()
        .map(|_| events.len().saturating_sub(1))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationalContract {
    pub name: String,
    #[serde(default)]
    pub observations: Vec<String>,
    #[serde(default)]
    pub secret_inputs: Vec<String>,
    #[serde(default)]
    pub ignored_side_effects: Vec<String>,
    pub bound: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationalCheckResult {
    pub status: ObligationStatus,
    pub compared_events: usize,
    pub divergence_index: Option<usize>,
    pub counterexample: Option<Counterexample>,
    pub summary: String,
}

pub fn compare_traces(
    left: &Trace,
    right: &Trace,
    contract: &RelationalContract,
) -> Result<RelationalCheckResult, String> {
    if contract.bound == 0 {
        return Err("relational bound must be greater than zero".to_string());
    }
    let limit = contract
        .bound
        .min(left.events.len())
        .min(right.events.len());
    let divergence = (0..limit).find(|index| {
        let left_event = &left.events[*index];
        let right_event = &right.events[*index];
        left_event.kind != right_event.kind
            || left_event.operation != right_event.operation
            || (contract
                .observations
                .iter()
                .any(|observation| observation == "value")
                && left_event.observed_value != right_event.observed_value)
    });
    let length_divergence = (left.events.len() != right.events.len()).then_some(limit);
    let divergence = divergence.or(length_divergence);
    if let Some(index) = divergence {
        return Ok(RelationalCheckResult {
            status: ObligationStatus::Failed,
            compared_events: limit,
            divergence_index: Some(index),
            counterexample: Some(Counterexample {
                id: TraceId::new(format!("{}:relational:{index}", left.id)),
                obligation_id: None,
                scope: left.events.get(index).and_then(|event| event.scope.clone()),
                summary: format!("relational contract '{}' diverged", contract.name),
                minimized: true,
                details: Some(serde_json::json!({
                    "left": left.events.get(index),
                    "right": right.events.get(index),
                })),
            }),
            summary: format!("first relational divergence at event {index}"),
        });
    }
    let status = if left.static_graph.is_some() || right.static_graph.is_some() {
        ObligationStatus::Modeled
    } else {
        ObligationStatus::Observed
    };
    Ok(RelationalCheckResult {
        status,
        compared_events: limit,
        divergence_index: None,
        counterexample: None,
        summary: format!("traces matched for {limit} bounded events"),
    })
}

/// Build a deterministic static trace from a Rust source function.  This is
/// the shared fallback when a runtime provider is not available; its results
/// remain `Modeled`, never `Proven`.
pub fn static_trace_from_source(
    source_path: impl AsRef<Path>,
    target_function: Option<&str>,
    sample: SampleKey,
) -> Result<Trace, String> {
    let source_path = source_path.as_ref();
    let content = fs::read_to_string(source_path).map_err(|error| error.to_string())?;
    let ast = syn::parse_file(&content).map_err(|error| error.to_string())?;
    let function = ast.items.iter().find_map(|item| match item {
        syn::Item::Fn(function)
            if target_function.is_none_or(|target| function.sig.ident == target) =>
        {
            Some(function)
        }
        _ => None,
    });
    let function = function.ok_or_else(|| "target function not found in source".to_string())?;
    let target = function.sig.ident.to_string();
    let trace_id = TraceId::new(format!(
        "{}::{}",
        source_path.display(),
        sample.fingerprint()
    ));
    let mut trace = Trace::new(trace_id, sample);
    trace.push(TraceEvent {
        trace: trace.id.clone(),
        sequence: 0,
        thread: "main".to_string(),
        logical_time: 0,
        scope: Some(ScopeId::new(target.clone())),
        kind: TraceEventKind::FunctionEnter,
        operation: target,
        observed_value: None,
        ordering: None,
        source: Some(crate::assurance::SourceLocation {
            file: source_path.to_string_lossy().to_string(),
            line: function.sig.ident.span().start().line,
        }),
    });
    {
        let mut visitor = StaticTraceVisitor {
            trace: &mut trace,
            source: source_path.to_string_lossy().to_string(),
        };
        visitor.visit_block(&function.block);
    }
    trace.push(TraceEvent {
        trace: trace.id.clone(),
        sequence: 0,
        thread: "main".to_string(),
        logical_time: 0,
        scope: None,
        kind: TraceEventKind::FunctionExit,
        operation: "return".to_string(),
        observed_value: None,
        ordering: None,
        source: None,
    });
    trace.static_graph = Some(StaticEventGraph {
        schema_version: crate::model::MODEL_SCHEMA_VERSION,
        events: trace.events.clone(),
        edges: trace
            .events
            .windows(2)
            .map(|events| TraceEdge {
                from: events[0].sequence,
                to: events[1].sequence,
                relation: "source-order".to_string(),
            })
            .collect(),
    });
    Ok(trace)
}

struct StaticTraceVisitor<'a> {
    trace: &'a mut Trace,
    source: String,
}

impl<'ast> syn::visit::Visit<'ast> for StaticTraceVisitor<'_> {
    fn visit_expr_if(&mut self, node: &'ast syn::ExprIf) {
        self.trace.push(TraceEvent {
            trace: self.trace.id.clone(),
            sequence: 0,
            thread: "main".to_string(),
            logical_time: 0,
            scope: None,
            kind: TraceEventKind::Branch,
            operation: "if".to_string(),
            observed_value: None,
            ordering: None,
            source: Some(crate::assurance::SourceLocation {
                file: self.source.clone(),
                line: node.if_token.span.start().line,
            }),
        });
        syn::visit::visit_expr_if(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let operation = node.method.to_string();
        let kind = match operation.as_str() {
            "load" => TraceEventKind::AtomicLoad,
            "store" => TraceEventKind::AtomicStore,
            name if name.starts_with("fetch_") || name.contains("compare_exchange") => {
                TraceEventKind::AtomicRmw
            }
            "lock" | "read" | "write" => TraceEventKind::Lock,
            "unlock" => TraceEventKind::Unlock,
            "join" => TraceEventKind::Join,
            "wake" | "notify_one" | "notify_all" => TraceEventKind::Wake,
            "await" => TraceEventKind::Await,
            _ => TraceEventKind::Read,
        };
        self.trace.push(TraceEvent {
            trace: self.trace.id.clone(),
            sequence: 0,
            thread: "main".to_string(),
            logical_time: 0,
            scope: None,
            kind,
            operation,
            observed_value: None,
            ordering: None,
            source: Some(crate::assurance::SourceLocation {
                file: self.source.clone(),
                line: node.method.span().start().line,
            }),
        });
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = &*node.func
            && let Some(segment) = path.path.segments.last()
        {
            let operation = segment.ident.to_string();
            let kind = match operation.as_str() {
                "spawn" => TraceEventKind::Spawn,
                "allocate" | "alloc" => TraceEventKind::Allocate,
                "free" | "dealloc" => TraceEventKind::Free,
                _ => TraceEventKind::FunctionEnter,
            };
            self.trace.push(TraceEvent {
                trace: self.trace.id.clone(),
                sequence: 0,
                thread: "main".to_string(),
                logical_time: 0,
                scope: None,
                kind,
                operation,
                observed_value: None,
                ordering: None,
                source: Some(crate::assurance::SourceLocation {
                    file: self.source.clone(),
                    line: node.func.span().start().line,
                }),
            });
        }
        syn::visit::visit_expr_call(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: TraceEventKind, operation: &str) -> TraceEvent {
        TraceEvent {
            trace: TraceId::new("ignored"),
            sequence: 0,
            thread: "main".to_string(),
            logical_time: 0,
            scope: None,
            kind,
            operation: operation.to_string(),
            observed_value: None,
            ordering: None,
            source: None,
        }
    }

    #[test]
    fn temporal_checker_returns_short_bounded_counterexample() {
        let mut trace = Trace::new(TraceId::new("t"), SampleKey::complexity(1, 1));
        trace.push(event(TraceEventKind::Lock, "lock"));
        trace.push(event(TraceEventKind::Lock, "lock"));
        let result = check_temporal(
            &trace,
            &TemporalContract {
                name: "unlock eventually".to_string(),
                operator: TemporalOperator::Eventually,
                event: "unlock".to_string(),
                until_event: None,
                bound: 8,
                fairness_assumption: Some("scheduler eventually runs owner".to_string()),
            },
            Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(result.status, ObligationStatus::Failed);
        assert!(result.counterexample.unwrap().minimized);
    }

    #[test]
    fn relational_checker_reports_first_divergence() {
        let mut left = Trace::new(TraceId::new("left"), SampleKey::complexity(1, 1));
        let mut right = Trace::new(TraceId::new("right"), SampleKey::complexity(1, 1));
        left.push(event(TraceEventKind::Return, "ok"));
        right.push(event(TraceEventKind::Error, "err"));
        let result = compare_traces(
            &left,
            &right,
            &RelationalContract {
                name: "same result".to_string(),
                observations: vec!["operation".to_string()],
                secret_inputs: Vec::new(),
                ignored_side_effects: Vec::new(),
                bound: 4,
            },
        )
        .unwrap();
        assert_eq!(result.divergence_index, Some(0));
        assert_eq!(result.status, ObligationStatus::Failed);
    }

    #[test]
    fn static_relational_match_is_modeled_not_observed() {
        let source = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(source.path(), "fn target() { let _ = 1; }").unwrap();
        let left =
            static_trace_from_source(source.path(), Some("target"), SampleKey::default()).unwrap();
        let right = left.clone();
        let result = compare_traces(
            &left,
            &right,
            &RelationalContract {
                name: "static".to_string(),
                observations: Vec::new(),
                secret_inputs: Vec::new(),
                ignored_side_effects: Vec::new(),
                bound: 8,
            },
        )
        .unwrap();
        assert_eq!(result.status, ObligationStatus::Modeled);
    }
}
