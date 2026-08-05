//! Hierarchical static scope graph and coverage envelope.
//!
//! The graph is intentionally finite: definitions are collected once and
//! recursive calls become edges in the same graph instead of recursively
//! expanding the AST.  Unresolved calls are represented as opaque nodes so
//! callers can keep their uncertainty instead of silently dropping them.

use crate::assurance::SourceLocation;
use crate::coverage::CoverageMap;
use crate::model::{AssumptionId, CallEdgeId, FunctionId, PackageId, SampleKey, ScopeId};
use quote::ToTokens;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScopeNodeKind {
    Function,
    Method,
    Closure,
    AsyncBlock,
    Opaque,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScopeEdgeKind {
    Call,
    DynamicDispatch,
    Ffi,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ScopeRisk {
    #[default]
    Normal,
    CriticalSafety,
    CriticalConcurrency,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeNode {
    pub id: ScopeId,
    pub function_id: Option<FunctionId>,
    pub label: String,
    pub kind: ScopeNodeKind,
    #[serde(default)]
    pub risk: ScopeRisk,
    pub source: Option<SourceLocation>,
    pub opaque: bool,
    pub entry_count: Option<u64>,
    pub self_count: Option<u64>,
    pub inclusive_count: Option<u64>,
    pub executed_branches: Option<u64>,
    pub total_branches: Option<u64>,
    pub line_coverage: Option<f64>,
    pub expected_complexity: Option<String>,
    pub local_fitted_complexity: Option<String>,
    pub inclusive_fitted_complexity: Option<String>,
    #[serde(default)]
    pub assumptions: Vec<AssumptionId>,
    #[serde(default)]
    pub coverage_obligations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeEdge {
    pub id: CallEdgeId,
    pub caller: ScopeId,
    pub callee: ScopeId,
    pub kind: ScopeEdgeKind,
    pub source: Option<SourceLocation>,
    pub call_multiplicity: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeGraph {
    pub schema_version: u32,
    pub root: ScopeId,
    pub nodes: Vec<ScopeNode>,
    pub edges: Vec<ScopeEdge>,
    #[serde(default)]
    pub recursive_components: Vec<Vec<ScopeId>>,
    #[serde(default)]
    pub assumptions: Vec<AssumptionId>,
}

impl ScopeGraph {
    pub fn reachable_nodes(&self) -> Vec<&ScopeNode> {
        let mut seen = HashSet::new();
        let mut stack = vec![self.root.clone()];
        while let Some(id) = stack.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            for edge in self.edges.iter().filter(|edge| edge.caller == id) {
                stack.push(edge.callee.clone());
            }
        }
        self.nodes
            .iter()
            .filter(|node| seen.contains(&node.id))
            .collect()
    }

    pub fn node_for_function(&self, function: &str) -> Option<&ScopeNode> {
        self.nodes.iter().find(|node| {
            node.label == function
                || node
                    .function_id
                    .as_ref()
                    .is_some_and(|id| id.0.ends_with(function))
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeEnvelope {
    pub schema_version: u32,
    pub root: ScopeId,
    pub nodes: Vec<ScopeNode>,
    pub edges: Vec<ScopeEdge>,
    #[serde(default)]
    pub samples: Vec<SampleKey>,
    #[serde(default)]
    pub assumptions: Vec<AssumptionId>,
    #[serde(default)]
    pub attribution_obligations: Vec<String>,
    #[serde(default)]
    pub scope_coverage_percent: Option<f64>,
    #[serde(default)]
    pub critical_unknown_scopes: Vec<ScopeId>,
    #[serde(default)]
    pub parameters: Vec<covopt_schema::ParameterId>,
}

impl ScopeEnvelope {
    pub fn from_graph(graph: ScopeGraph, samples: Vec<SampleKey>) -> Self {
        Self {
            schema_version: crate::model::MODEL_SCHEMA_VERSION,
            root: graph.root,
            nodes: graph.nodes,
            edges: graph.edges,
            samples,
            assumptions: graph.assumptions,
            attribution_obligations: Vec::new(),
            scope_coverage_percent: None,
            critical_unknown_scopes: Vec::new(),
            parameters: Vec::new(),
        }
    }

    pub fn apply_coverage(&mut self, coverage: &CoverageMap) {
        let mut attribution_obligations = Vec::new();
        for node in &mut self.nodes {
            let Some(source) = node.source.as_ref() else {
                continue;
            };
            let Some(function) = coverage.function_record(&source.file, &node.label) else {
                if !node.opaque {
                    let obligation = format!("COVOPT-ATTR-{}", stable_scope_hash(&node.id.0));
                    node.coverage_obligations.push(obligation.clone());
                    attribution_obligations.push(obligation);
                }
                continue;
            };
            node.entry_count = Some(function.execution_count);
            node.self_count = Some(function.execution_count);
            node.inclusive_count = Some(function.execution_count);
            let hits = coverage
                .hit_counts
                .iter()
                .find(|(file, _)| {
                    let requested = source.file.trim_start_matches("./");
                    file.ends_with(requested)
                        || file
                            .replace('\\', "/")
                            .ends_with(&requested.replace('\\', "/"))
                })
                .map(|(_, lines)| {
                    let total = (function.start_line..=function.end_line)
                        .filter(|line| lines.contains_key(line))
                        .count() as u64;
                    let executed = (function.start_line..=function.end_line)
                        .filter(|line| lines.get(line).is_some_and(|hits| *hits > 0))
                        .count() as u64;
                    (executed, total)
                });
            if let Some((executed, total)) = hits
                && total > 0
            {
                node.line_coverage = Some(executed as f64 / total as f64);
            }
            let branches =
                coverage.branches_for(&source.file, function.start_line, function.end_line);
            if !branches.is_empty() {
                node.total_branches = Some(branches.len() as u64);
                node.executed_branches = Some(
                    branches
                        .iter()
                        .filter(|branch| branch.taken.is_some_and(|taken| taken > 0))
                        .count() as u64,
                );
            }
        }
        self.attribution_obligations = attribution_obligations;
        let reachable = {
            let mut seen = HashSet::new();
            let mut stack = vec![self.root.clone()];
            while let Some(id) = stack.pop() {
                if seen.insert(id.clone()) {
                    stack.extend(
                        self.edges
                            .iter()
                            .filter(|edge| edge.caller == id)
                            .map(|edge| edge.callee.clone()),
                    );
                }
            }
            seen
        };
        let mut total_weight = 0.0;
        let mut covered_weight = 0.0;
        let mut critical_unknown = Vec::new();
        for node in self
            .nodes
            .iter()
            .filter(|node| reachable.contains(&node.id))
        {
            if node.opaque {
                continue;
            }
            let weight = matches!(
                node.risk,
                ScopeRisk::CriticalSafety | ScopeRisk::CriticalConcurrency
            )
            .then_some(2.0)
            .unwrap_or(1.0);
            total_weight += weight;
            if let Some(coverage) = node.line_coverage {
                covered_weight += weight * coverage;
            }
            let branch_incomplete = node
                .total_branches
                .zip(node.executed_branches)
                .is_some_and(|(total, executed)| executed < total);
            if matches!(
                node.risk,
                ScopeRisk::CriticalSafety | ScopeRisk::CriticalConcurrency
            ) && (node.line_coverage.is_none() || branch_incomplete)
            {
                critical_unknown.push(node.id.clone());
            }
        }
        self.scope_coverage_percent =
            (total_weight > 0.0).then_some(covered_weight / total_weight * 100.0);
        self.critical_unknown_scopes = critical_unknown;
    }

    pub fn set_expected_complexity(&mut self, function: Option<&str>, expected: Option<&str>) {
        let Some(expected) = expected else {
            return;
        };
        for node in &mut self.nodes {
            if function.is_none_or(|name| node.label == name) {
                node.expected_complexity = Some(expected.to_string());
            }
        }
    }

    pub fn set_fitted_complexity(&mut self, function: Option<&str>, fitted: &str) {
        for node in &mut self.nodes {
            if function.is_none_or(|name| node.label == name) {
                node.local_fitted_complexity = Some(fitted.to_string());
                node.inclusive_fitted_complexity = Some(fitted.to_string());
            }
        }
    }

    pub fn first_contract_break(&self) -> Option<&ScopeNode> {
        self.nodes.iter().find(|node| {
            node.expected_complexity
                .as_ref()
                .zip(node.local_fitted_complexity.as_ref())
                .is_some_and(|(expected, fitted)| expected != fitted)
        })
    }
}

fn stable_scope_hash(value: &str) -> String {
    let hash = value.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}")
}

#[derive(Debug, Clone)]
struct Definition {
    scope_id: ScopeId,
    label: String,
}

struct GraphBuilder {
    source: PathBuf,
    nodes: Vec<ScopeNode>,
    edges: Vec<ScopeEdge>,
    definitions: HashMap<String, Definition>,
    externals: HashSet<String>,
    opaque: HashMap<String, ScopeId>,
    assumptions: Vec<AssumptionId>,
}

impl GraphBuilder {
    fn add_node(
        &mut self,
        function_id: Option<FunctionId>,
        label: String,
        kind: ScopeNodeKind,
        line: usize,
    ) -> ScopeId {
        let id = function_id.clone().map_or_else(
            || {
                ScopeId::from_parts([
                    self.source.to_string_lossy().as_ref(),
                    &label,
                    &line.to_string(),
                ])
            },
            |function| ScopeId::from_function(&function, format!("{:?}", kind), 0),
        );
        self.nodes.push(ScopeNode {
            id: id.clone(),
            function_id,
            label,
            kind,
            source: Some(SourceLocation {
                file: self.source.to_string_lossy().to_string(),
                line,
            }),
            opaque: matches!(kind, ScopeNodeKind::Opaque | ScopeNodeKind::External),
            entry_count: None,
            self_count: None,
            inclusive_count: None,
            executed_branches: None,
            total_branches: None,
            line_coverage: None,
            expected_complexity: None,
            local_fitted_complexity: None,
            inclusive_fitted_complexity: None,
            assumptions: Vec::new(),
            coverage_obligations: Vec::new(),
            risk: ScopeRisk::Normal,
        });
        id
    }

    fn opaque_node(&mut self, name: &str, kind: ScopeNodeKind, line: usize) -> ScopeId {
        if let Some(id) = self.opaque.get(name) {
            return id.clone();
        }
        let id = self.add_node(None, name.to_string(), kind, line);
        let assumption = AssumptionId::new(format!("opaque-call::{name}"));
        if let Some(node) = self.nodes.iter_mut().find(|node| node.id == id) {
            node.assumptions.push(assumption.clone());
        }
        self.assumptions.push(assumption);
        self.opaque.insert(name.to_string(), id.clone());
        id
    }

    fn add_edge(&mut self, caller: ScopeId, callee: ScopeId, kind: ScopeEdgeKind, line: usize) {
        let caller_fn = self
            .nodes
            .iter()
            .find(|node| node.id == caller)
            .and_then(|node| node.function_id.clone())
            .unwrap_or_else(|| FunctionId::new(caller.0.clone()));
        let callee_fn = self
            .nodes
            .iter()
            .find(|node| node.id == callee)
            .and_then(|node| node.function_id.clone())
            .unwrap_or_else(|| FunctionId::new(callee.0.clone()));
        let ordinal = self
            .edges
            .iter()
            .filter(|edge| edge.caller == caller)
            .count();
        self.edges.push(ScopeEdge {
            id: CallEdgeId::from_source(&caller_fn, &callee_fn, ordinal),
            caller,
            callee,
            kind,
            source: Some(SourceLocation {
                file: self.source.to_string_lossy().to_string(),
                line,
            }),
            call_multiplicity: None,
        });
    }
}

struct CallVisitor<'a> {
    builder: &'a mut GraphBuilder,
    caller: ScopeId,
}

impl<'ast> Visit<'ast> for CallVisitor<'_> {
    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        let name = match &*node.func {
            syn::Expr::Path(path) => path
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string()),
            _ => None,
        };
        if let Some(name) = name {
            let line = node.span().start().line;
            let (callee, kind) = if let Some(definition) = self.builder.definitions.get(&name) {
                (definition.scope_id.clone(), ScopeEdgeKind::Call)
            } else if self.builder.externals.contains(&name) {
                (
                    self.builder.opaque_node(
                        &format!("ffi::{name}"),
                        ScopeNodeKind::External,
                        line,
                    ),
                    ScopeEdgeKind::Ffi,
                )
            } else {
                (
                    self.builder.opaque_node(
                        &format!("opaque::{name}"),
                        ScopeNodeKind::Opaque,
                        line,
                    ),
                    ScopeEdgeKind::External,
                )
            };
            self.builder
                .add_edge(self.caller.clone(), callee, kind, line);
        }
        visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let name = node.method.to_string();
        let line = node.span().start().line;
        let callee = self
            .builder
            .definitions
            .get(&name)
            .map(|definition| definition.scope_id.clone())
            .unwrap_or_else(|| {
                self.builder
                    .opaque_node(&format!("dynamic::{name}"), ScopeNodeKind::Opaque, line)
            });
        self.builder.add_edge(
            self.caller.clone(),
            callee,
            ScopeEdgeKind::DynamicDispatch,
            line,
        );
        visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_closure(&mut self, node: &'ast syn::ExprClosure) {
        let parent = self.caller.clone();
        let line = node.span().start().line;
        let label = format!("closure@{line}");
        let child = self
            .builder
            .add_node(None, label, ScopeNodeKind::Closure, line);
        self.builder
            .add_edge(parent.clone(), child.clone(), ScopeEdgeKind::Call, line);
        self.caller = child;
        visit::visit_expr_closure(self, node);
        self.caller = parent;
    }

    fn visit_expr_async(&mut self, node: &'ast syn::ExprAsync) {
        let parent = self.caller.clone();
        let line = node.span().start().line;
        let child = self.builder.add_node(
            None,
            format!("async@{line}"),
            ScopeNodeKind::AsyncBlock,
            line,
        );
        self.builder
            .add_edge(parent.clone(), child.clone(), ScopeEdgeKind::Call, line);
        self.caller = child;
        visit::visit_expr_async(self, node);
        self.caller = parent;
    }
}

pub fn build_scope_graph(
    package: &PackageId,
    source_path: impl AsRef<Path>,
    target_function: Option<&str>,
) -> Result<ScopeGraph, String> {
    let source_path = source_path.as_ref().to_path_buf();
    let source = fs::read_to_string(&source_path).map_err(|error| error.to_string())?;
    let ast =
        syn::parse_file(&source).map_err(|error| format!("{}: {error}", source_path.display()))?;
    let mut builder = GraphBuilder {
        source: source_path.clone(),
        nodes: Vec::new(),
        edges: Vec::new(),
        definitions: HashMap::new(),
        externals: HashSet::new(),
        opaque: HashMap::new(),
        assumptions: Vec::new(),
    };

    for item in &ast.items {
        match item {
            syn::Item::Fn(function) => {
                let label = function.sig.ident.to_string();
                let generic_context = function.sig.generics.params.to_token_stream().to_string();
                let id = FunctionId::from_source(
                    package,
                    source_path.to_string_lossy(),
                    &label,
                    generic_context,
                );
                let scope_id = builder.add_node(
                    Some(id.clone()),
                    label.clone(),
                    ScopeNodeKind::Function,
                    function.sig.ident.span().start().line,
                );
                builder
                    .definitions
                    .insert(label.clone(), Definition { scope_id, label });
            }
            syn::Item::Impl(item_impl) => {
                for item in &item_impl.items {
                    let syn::ImplItem::Fn(function) = item else {
                        continue;
                    };
                    let label = function.sig.ident.to_string();
                    let generic_context =
                        function.sig.generics.params.to_token_stream().to_string();
                    let id = FunctionId::from_source(
                        package,
                        source_path.to_string_lossy(),
                        &label,
                        format!("impl:{}", generic_context),
                    );
                    let scope_id = builder.add_node(
                        Some(id.clone()),
                        label.clone(),
                        ScopeNodeKind::Method,
                        function.sig.ident.span().start().line,
                    );
                    builder
                        .definitions
                        .entry(label.clone())
                        .or_insert(Definition { scope_id, label });
                }
            }
            syn::Item::ForeignMod(foreign) => {
                for item in &foreign.items {
                    if let syn::ForeignItem::Fn(function) = item {
                        builder.externals.insert(function.sig.ident.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    let definitions = builder.definitions.values().cloned().collect::<Vec<_>>();
    for definition in definitions {
        let Some(item) = find_function_body(&ast, &definition.label) else {
            continue;
        };
        let mut visitor = CallVisitor {
            builder: &mut builder,
            caller: definition.scope_id,
        };
        visitor.visit_block(item);
    }
    let mut multiplicities: HashMap<(ScopeId, ScopeId, ScopeEdgeKind), u64> = HashMap::new();
    for edge in &builder.edges {
        *multiplicities
            .entry((edge.caller.clone(), edge.callee.clone(), edge.kind))
            .or_default() += 1;
    }
    for edge in &mut builder.edges {
        edge.call_multiplicity = multiplicities
            .get(&(edge.caller.clone(), edge.callee.clone(), edge.kind))
            .copied();
    }
    let root = target_function
        .and_then(|target| builder.definitions.get(target))
        .or_else(|| builder.definitions.values().next())
        .map(|definition| definition.scope_id.clone())
        .ok_or_else(|| "no function definitions found for scope graph".to_string())?;
    let mut graph = ScopeGraph {
        schema_version: crate::model::MODEL_SCHEMA_VERSION,
        root,
        nodes: builder.nodes,
        edges: builder.edges,
        recursive_components: Vec::new(),
        assumptions: builder.assumptions,
    };
    graph.recursive_components = recursive_components(&graph);
    Ok(graph)
}

fn find_function_body<'ast>(ast: &'ast syn::File, label: &str) -> Option<&'ast syn::Block> {
    for item in &ast.items {
        if let syn::Item::Fn(function) = item
            && function.sig.ident == label
        {
            return Some(&function.block);
        }
        if let syn::Item::Impl(item_impl) = item {
            for item in &item_impl.items {
                if let syn::ImplItem::Fn(function) = item
                    && function.sig.ident == label
                {
                    return Some(&function.block);
                }
            }
        }
    }
    None
}

fn recursive_components(graph: &ScopeGraph) -> Vec<Vec<ScopeId>> {
    let mut adjacency: HashMap<ScopeId, Vec<ScopeId>> = HashMap::new();
    for edge in &graph.edges {
        adjacency
            .entry(edge.caller.clone())
            .or_default()
            .push(edge.callee.clone());
    }
    #[allow(clippy::too_many_arguments)]
    fn strong_connect(
        vertex: ScopeId,
        adjacency: &HashMap<ScopeId, Vec<ScopeId>>,
        next_index: &mut usize,
        indices: &mut HashMap<ScopeId, usize>,
        lowlinks: &mut HashMap<ScopeId, usize>,
        stack: &mut Vec<ScopeId>,
        on_stack: &mut HashSet<ScopeId>,
        components: &mut Vec<Vec<ScopeId>>,
    ) {
        indices.insert(vertex.clone(), *next_index);
        lowlinks.insert(vertex.clone(), *next_index);
        *next_index += 1;
        stack.push(vertex.clone());
        on_stack.insert(vertex.clone());
        let neighbors = adjacency.get(&vertex).cloned().unwrap_or_default();
        for neighbor in neighbors {
            if !indices.contains_key(&neighbor) {
                strong_connect(
                    neighbor.clone(),
                    adjacency,
                    next_index,
                    indices,
                    lowlinks,
                    stack,
                    on_stack,
                    components,
                );
                let child_lowlink = lowlinks[&neighbor];
                let current_lowlink = lowlinks[&vertex];
                lowlinks.insert(vertex.clone(), current_lowlink.min(child_lowlink));
            } else if on_stack.contains(&neighbor) {
                let neighbor_index = indices[&neighbor];
                let current_lowlink = lowlinks[&vertex];
                lowlinks.insert(vertex.clone(), current_lowlink.min(neighbor_index));
            }
        }
        if lowlinks[&vertex] == indices[&vertex] {
            let mut component = Vec::new();
            while let Some(item) = stack.pop() {
                on_stack.remove(&item);
                component.push(item.clone());
                if item == vertex {
                    break;
                }
            }
            if component.len() > 1
                || adjacency
                    .get(&vertex)
                    .is_some_and(|edges| edges.contains(&vertex))
            {
                components.push(component);
            }
        }
    }

    let mut next_index = 0;
    let mut indices = HashMap::new();
    let mut lowlinks = HashMap::new();
    let mut stack = Vec::new();
    let mut on_stack = HashSet::new();
    let mut result = Vec::new();
    for node in &graph.nodes {
        if !indices.contains_key(&node.id) {
            strong_connect(
                node.id.clone(),
                &adjacency,
                &mut next_index,
                &mut indices,
                &mut lowlinks,
                &mut stack,
                &mut on_stack,
                &mut result,
            );
        }
    }
    result
}

pub fn build_scope_envelope(
    package: &PackageId,
    source_path: impl AsRef<Path>,
    target_function: Option<&str>,
    coverage: Option<&CoverageMap>,
    samples: Vec<SampleKey>,
) -> Result<ScopeEnvelope, String> {
    let graph = build_scope_graph(package, source_path, target_function)?;
    let mut envelope = ScopeEnvelope::from_graph(graph, samples);
    if let Some(coverage) = coverage {
        envelope.apply_coverage(coverage);
    }
    Ok(envelope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn graph_is_finite_and_preserves_recursive_edges_and_opaque_calls() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sample.rs");
        fs::write(
            &path,
            "fn root(n: usize) { if n > 0 { root(n - 1); } unknown_external(); }\n",
        )
        .unwrap();
        let graph = build_scope_graph(&PackageId::new("pkg"), &path, Some("root")).unwrap();
        assert_eq!(graph.root, graph.node_for_function("root").unwrap().id);
        assert!(
            graph
                .recursive_components
                .iter()
                .any(|component| component.len() == 1)
        );
        assert!(graph.nodes.iter().any(|node| node.opaque));
        assert!(!graph.reachable_nodes().is_empty());
    }

    #[test]
    fn mutual_recursion_is_collapsed_into_one_component() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("mutual.rs");
        fs::write(
            &path,
            "fn a(n: usize) { if n > 0 { b(n - 1); } }\nfn b(n: usize) { if n > 0 { a(n - 1); } }\n",
        )
        .unwrap();
        let graph = build_scope_graph(&PackageId::new("pkg"), &path, Some("a")).unwrap();
        assert!(
            graph
                .recursive_components
                .iter()
                .any(|component| component.len() == 2)
        );
    }
}
