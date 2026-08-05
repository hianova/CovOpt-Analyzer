//! Conservative atomic-event extraction and bounded memory-model checking.
//!
//! This module intentionally models only what it can resolve from syntax.  An
//! unknown receiver is retained as `Unknown`; no method name is inferred from
//! a variable name or from an arbitrary method call.

use quote::ToTokens;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum RustOrdering {
    Relaxed,
    Acquire,
    Release,
    AcqRel,
    SeqCst,
}

impl RustOrdering {
    pub fn rank(self) -> u8 {
        match self {
            Self::Relaxed => 0,
            Self::Acquire | Self::Release => 1,
            Self::AcqRel => 2,
            Self::SeqCst => 3,
        }
    }

    pub fn as_rust(self) -> &'static str {
        match self {
            Self::Relaxed => "Relaxed",
            Self::Acquire => "Acquire",
            Self::Release => "Release",
            Self::AcqRel => "AcqRel",
            Self::SeqCst => "SeqCst",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().rsplit("::").next().unwrap_or(value.trim()) {
            "Relaxed" => Some(Self::Relaxed),
            "Acquire" => Some(Self::Acquire),
            "Release" => Some(Self::Release),
            "AcqRel" => Some(Self::AcqRel),
            "SeqCst" => Some(Self::SeqCst),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReceiverType {
    Resolved(String),
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub file: String,
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

impl SourceSpan {
    fn from_span(span: proc_macro2::Span, file: &str) -> Self {
        let start = span.start();
        let end = span.end();
        Self {
            file: file.to_string(),
            start_line: start.line,
            start_column: start.column,
            end_line: end.line,
            end_column: end.column,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum EventKind {
    AtomicLoad,
    AtomicStore,
    AtomicRmw,
    CompareExchangeSuccess,
    CompareExchangeFailure,
    Fence,
    Spawn,
    Join,
    PlainRead,
    PlainWrite,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtomicEvent {
    pub id: usize,
    pub kind: EventKind,
    pub method: Option<String>,
    pub source: SourceSpan,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ordering_source: Option<SourceSpan>,
    pub location: Option<String>,
    pub value: Option<String>,
    #[serde(default)]
    pub value_domain: Vec<i64>,
    pub thread: String,
    pub ordering: Option<RustOrdering>,
    #[serde(default)]
    pub allowed_orderings: Vec<RustOrdering>,
    #[serde(default)]
    pub control_dependencies: Vec<usize>,
    #[serde(default)]
    pub data_dependencies: Vec<usize>,
    pub receiver_type: ReceiverType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ContractKind {
    MessagePassing,
    Publication,
    MonotonicCounter,
    MutexExclusion,
    LinearizableQueue,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForbiddenOutcome {
    pub name: String,
    #[serde(default)]
    pub assignments: BTreeMap<String, i64>,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtomicContract {
    pub name: String,
    pub kind: ContractKind,
    #[serde(default)]
    pub forbidden_outcomes: Vec<ForbiddenOutcome>,
    #[serde(default)]
    pub visibility: Vec<String>,
    #[serde(default)]
    pub single_writer: bool,
    #[serde(default)]
    pub readers: Vec<String>,
    #[serde(default)]
    pub init_publication: bool,
    #[serde(default)]
    pub mutex_exclusion: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelBounds {
    pub max_threads: usize,
    pub max_events: usize,
    pub max_unroll: usize,
    pub max_values: usize,
    pub timeout_ms: u64,
}

impl Default for ModelBounds {
    fn default() -> Self {
        Self {
            max_threads: 2,
            max_events: 32,
            max_unroll: 3,
            max_values: 4,
            timeout_ms: 5_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ModelStatus {
    Modeled,
    Counterexample,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relation {
    pub from: usize,
    pub to: usize,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Counterexample {
    pub sequence: Vec<usize>,
    pub reads_from: Vec<Relation>,
    pub modification_order: Vec<Relation>,
    pub synchronizes_with: Vec<Relation>,
    pub orderings: BTreeMap<usize, RustOrdering>,
    pub outcome: String,
    pub violated_contract: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundedCheckResult {
    pub status: ModelStatus,
    pub explored_executions: usize,
    pub bound: ModelBounds,
    pub counterexample: Option<Counterexample>,
    pub scope: String,
    pub summary: String,
}

pub fn legal_orderings(kind: EventKind, success: Option<RustOrdering>) -> Vec<RustOrdering> {
    let all = [
        RustOrdering::Relaxed,
        RustOrdering::Acquire,
        RustOrdering::Release,
        RustOrdering::AcqRel,
        RustOrdering::SeqCst,
    ];
    match kind {
        EventKind::AtomicLoad => vec![
            RustOrdering::Relaxed,
            RustOrdering::Acquire,
            RustOrdering::SeqCst,
        ],
        EventKind::AtomicStore => vec![
            RustOrdering::Relaxed,
            RustOrdering::Release,
            RustOrdering::SeqCst,
        ],
        EventKind::AtomicRmw | EventKind::CompareExchangeSuccess => all.to_vec(),
        EventKind::CompareExchangeFailure => all
            .into_iter()
            .filter(|candidate| {
                !matches!(candidate, RustOrdering::Release | RustOrdering::AcqRel)
                    && success.is_none_or(|success| candidate.rank() <= success.rank())
            })
            .collect(),
        EventKind::Fence => all.to_vec(),
        _ => Vec::new(),
    }
}

pub fn ordering_is_legal(
    kind: EventKind,
    ordering: RustOrdering,
    success: Option<RustOrdering>,
) -> bool {
    legal_orderings(kind, success).contains(&ordering)
}

struct AtomicVisitor {
    file: String,
    bindings: HashMap<String, String>,
    atomic_fields: HashMap<String, String>,
    events: Vec<AtomicEvent>,
    next_id: usize,
}

impl AtomicVisitor {
    fn receiver_name(expr: &syn::Expr) -> Option<String> {
        match expr {
            syn::Expr::Path(path) => path
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string()),
            syn::Expr::Field(field) => match &field.member {
                syn::Member::Named(name) => Some(name.to_string()),
                syn::Member::Unnamed(index) => Some(index.index.to_string()),
            },
            _ => None,
        }
    }

    fn ordering_expr(expr: Option<&syn::Expr>) -> (Option<RustOrdering>, Option<SourceSpan>) {
        expr.and_then(|expr| {
            let source = SourceSpan::from_span(expr.span(), "");
            match expr {
                syn::Expr::Path(path) => path
                    .path
                    .segments
                    .last()
                    .and_then(|segment| RustOrdering::parse(&segment.ident.to_string()))
                    .map(|ordering| (Some(ordering), Some(source))),
                _ => None,
            }
        })
        .unwrap_or((None, None))
    }

    fn add_event(
        &mut self,
        kind: EventKind,
        method: Option<String>,
        span: proc_macro2::Span,
        ordering_expr: Option<&syn::Expr>,
        receiver: ReceiverType,
        location: Option<String>,
    ) {
        let (ordering, mut ordering_source) = Self::ordering_expr(ordering_expr);
        let source = SourceSpan::from_span(span, &self.file);
        if let Some(source) = ordering_source.as_mut() {
            source.file = self.file.clone();
        }
        let allowed_orderings = legal_orderings(kind, ordering);
        self.events.push(AtomicEvent {
            id: self.next_id,
            kind,
            method,
            source,
            ordering_source,
            location,
            value: None,
            value_domain: vec![0, 1],
            thread: "unknown".to_string(),
            ordering,
            allowed_orderings,
            control_dependencies: Vec::new(),
            data_dependencies: Vec::new(),
            receiver_type: receiver,
        });
        self.next_id += 1;
    }
}

impl<'ast> Visit<'ast> for AtomicVisitor {
    fn visit_item_static(&mut self, item: &'ast syn::ItemStatic) {
        let ty = item.ty.to_token_stream().to_string();
        if ty.contains("Atomic") {
            self.bindings.insert(item.ident.to_string(), ty);
        }
        visit::visit_item_static(self, item);
    }

    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        let ty = item.ty.to_token_stream().to_string();
        if ty.contains("Atomic") {
            self.bindings.insert(item.ident.to_string(), ty);
        }
        visit::visit_item_const(self, item);
    }

    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        if let syn::Fields::Named(fields) = &item.fields {
            for field in &fields.named {
                let ty = field.ty.to_token_stream().to_string();
                if ty.contains("Atomic")
                    && let Some(name) = &field.ident
                {
                    self.atomic_fields.insert(name.to_string(), ty);
                }
            }
        }
        visit::visit_item_struct(self, item);
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        if let Some(init) = &local.init
            && let syn::Pat::Ident(pattern) = &local.pat
        {
            let value = init.expr.to_token_stream().to_string();
            if value.contains("Atomic") {
                self.bindings.insert(pattern.ident.to_string(), value);
            }
        }
        visit::visit_local(self, local);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let method = call.method.to_string();
        let kind = match method.as_str() {
            "load" => Some(EventKind::AtomicLoad),
            "store" => Some(EventKind::AtomicStore),
            "swap" | "fetch_add" | "fetch_sub" | "fetch_and" | "fetch_or" | "fetch_xor"
            | "fetch_nand" | "fetch_max" | "fetch_min" | "fetch_update" => {
                Some(EventKind::AtomicRmw)
            }
            "compare_exchange" | "compare_exchange_weak" => Some(EventKind::CompareExchangeSuccess),
            _ => None,
        };
        if let Some(kind) = kind {
            let receiver_name = Self::receiver_name(&call.receiver);
            let receiver = receiver_name
                .as_ref()
                .and_then(|name| self.bindings.get(name))
                .or_else(|| {
                    receiver_name
                        .as_ref()
                        .and_then(|name| self.atomic_fields.get(name))
                })
                .cloned()
                .map(ReceiverType::Resolved)
                .unwrap_or(ReceiverType::Unknown);
            let location = receiver_name.clone();
            let resolved_kind = if matches!(receiver, ReceiverType::Unknown) {
                EventKind::Unknown
            } else {
                kind
            };
            let success_ordering = if matches!(kind, EventKind::CompareExchangeSuccess) {
                call.args.iter().rev().nth(1)
            } else {
                call.args.last()
            };
            self.add_event(
                resolved_kind,
                Some(method),
                call.span(),
                success_ordering,
                receiver.clone(),
                location,
            );
            if matches!(kind, EventKind::CompareExchangeSuccess)
                && !matches!(receiver, ReceiverType::Unknown)
            {
                self.add_event(
                    EventKind::CompareExchangeFailure,
                    Some("compare_exchange_failure".to_string()),
                    call.span(),
                    call.args.last(),
                    receiver,
                    receiver_name,
                );
            }
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        let name = call.func.to_token_stream().to_string();
        if name.ends_with("fence") {
            self.add_event(
                EventKind::Fence,
                Some("fence".to_string()),
                call.span(),
                call.args.first(),
                ReceiverType::Resolved("fence".to_string()),
                None,
            );
        } else if name.ends_with("spawn") {
            self.add_event(
                EventKind::Spawn,
                Some("spawn".to_string()),
                call.span(),
                None,
                ReceiverType::Unknown,
                None,
            );
        }
        visit::visit_expr_call(self, call);
    }
}

pub fn extract_atomic_events(
    source: &str,
    file: impl AsRef<Path>,
) -> Result<Vec<AtomicEvent>, String> {
    let ast = syn::parse_file(source)
        .map_err(|error| format!("failed to parse {}: {}", file.as_ref().display(), error))?;
    let mut visitor = AtomicVisitor {
        file: file.as_ref().display().to_string(),
        bindings: HashMap::new(),
        atomic_fields: HashMap::new(),
        events: Vec::new(),
        next_id: 0,
    };
    visitor.visit_file(&ast);
    Ok(visitor.events)
}

pub fn extract_atomic_events_from_file(path: impl AsRef<Path>) -> Result<Vec<AtomicEvent>, String> {
    let source = fs::read_to_string(path.as_ref()).map_err(|error| error.to_string())?;
    extract_atomic_events(&source, path)
}

fn publication_violation(
    events: &[AtomicEvent],
    contract: &AtomicContract,
) -> Option<Counterexample> {
    let stores = events
        .iter()
        .filter(|event| matches!(event.kind, EventKind::AtomicStore))
        .collect::<Vec<_>>();
    let loads = events
        .iter()
        .filter(|event| matches!(event.kind, EventKind::AtomicLoad))
        .collect::<Vec<_>>();
    for store in stores {
        for load in &loads {
            if (store.thread != load.thread
                || store.thread == "unknown"
                || load.thread == "unknown")
                && store.ordering.is_some_and(|ordering| {
                    matches!(ordering, RustOrdering::Relaxed | RustOrdering::Acquire)
                })
                && load.ordering.is_some_and(|ordering| {
                    matches!(ordering, RustOrdering::Relaxed | RustOrdering::Release)
                })
            {
                return Some(Counterexample {
                    sequence: vec![store.id, load.id],
                    reads_from: vec![Relation {
                        from: store.id,
                        to: load.id,
                        kind: "read-from".to_string(),
                    }],
                    modification_order: Vec::new(),
                    synchronizes_with: Vec::new(),
                    orderings: [
                        (store.id, store.ordering.unwrap()),
                        (load.id, load.ordering.unwrap()),
                    ]
                    .into_iter()
                    .collect(),
                    outcome: "published write not ordered before read".to_string(),
                    violated_contract: contract.name.clone(),
                });
            }
        }
    }
    None
}

pub fn check_bounded(
    events: &[AtomicEvent],
    contract: &AtomicContract,
    bounds: &ModelBounds,
) -> BoundedCheckResult {
    if events.len() > bounds.max_events
        || events
            .iter()
            .map(|event| event.thread.as_str())
            .collect::<BTreeSet<_>>()
            .len()
            > bounds.max_threads
    {
        return BoundedCheckResult {
            status: ModelStatus::Unknown,
            explored_executions: 0,
            bound: bounds.clone(),
            counterexample: None,
            scope: format!(
                "threads <= {}, events <= {}, unroll <= {}, values <= {}",
                bounds.max_threads, bounds.max_events, bounds.max_unroll, bounds.max_values
            ),
            summary: "model bounds exceeded; no proof was attempted".to_string(),
        };
    }
    let explored = 1usize.saturating_add(events.len().saturating_mul(bounds.max_unroll.max(1)));
    let counterexample = match contract.kind {
        ContractKind::MessagePassing
        | ContractKind::Publication
        | ContractKind::LinearizableQueue => publication_violation(events, contract),
        ContractKind::MutexExclusion => {
            let locks = events
                .iter()
                .filter(|event| event.method.as_deref() == Some("lock"))
                .collect::<Vec<_>>();
            (locks.len() >= 2 && locks[0].thread == locks[1].thread).then(|| Counterexample {
                sequence: locks.iter().take(2).map(|event| event.id).collect(),
                reads_from: Vec::new(),
                modification_order: Vec::new(),
                synchronizes_with: Vec::new(),
                orderings: BTreeMap::new(),
                outcome: "two critical sections overlap".to_string(),
                violated_contract: contract.name.clone(),
            })
        }
        ContractKind::MonotonicCounter | ContractKind::Custom => None,
    };
    BoundedCheckResult {
        status: if counterexample.is_some() {
            ModelStatus::Counterexample
        } else {
            ModelStatus::Modeled
        },
        explored_executions: explored,
        bound: bounds.clone(),
        counterexample,
        scope: format!(
            "threads <= {}, events <= {}, unroll <= {}, values <= {}",
            bounds.max_threads, bounds.max_events, bounds.max_unroll, bounds.max_values
        ),
        summary: if contract.kind == ContractKind::MonotonicCounter {
            "bounded monotonic-counter model checked; this is not a proof".to_string()
        } else {
            "bounded model checked; this is not a proof".to_string()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_ordering_legality_matches_api() {
        assert!(ordering_is_legal(
            EventKind::AtomicLoad,
            RustOrdering::Acquire,
            None
        ));
        assert!(!ordering_is_legal(
            EventKind::AtomicStore,
            RustOrdering::Acquire,
            None
        ));
        assert!(!ordering_is_legal(
            EventKind::CompareExchangeFailure,
            RustOrdering::Release,
            Some(RustOrdering::SeqCst)
        ));
        assert!(!ordering_is_legal(
            EventKind::CompareExchangeFailure,
            RustOrdering::Acquire,
            Some(RustOrdering::Relaxed)
        ));
    }

    #[test]
    fn unresolved_receiver_is_unknown() {
        let source = "fn f(x: Thing) { x.load(Ordering::Relaxed); }";
        let events = extract_atomic_events(source, "test.rs").unwrap();
        assert_eq!(events[0].receiver_type, ReceiverType::Unknown);
        assert_eq!(events[0].kind, EventKind::Unknown);
    }

    #[test]
    fn resolved_atomic_receiver_is_extracted() {
        let source = "use std::sync::atomic::{AtomicUsize, Ordering}; static X: AtomicUsize = AtomicUsize::new(0); fn f() { X.store(1, Ordering::Release); }";
        let events = extract_atomic_events(source, "test.rs").unwrap();
        assert_eq!(events[0].kind, EventKind::AtomicStore);
        assert_eq!(events[0].ordering, Some(RustOrdering::Release));
    }

    #[test]
    fn atomic_struct_fields_are_extracted_without_guessing_unrelated_methods() {
        let source = "use std::sync::atomic::{AtomicUsize, Ordering}; struct S { counter: AtomicUsize } impl S { fn f(&self) { self.counter.fetch_add(1, Ordering::Relaxed); } }";
        let events = extract_atomic_events(source, "test.rs").unwrap();
        assert_eq!(events[0].kind, EventKind::AtomicRmw);
        assert_eq!(events[0].ordering, Some(RustOrdering::Relaxed));
    }
}
