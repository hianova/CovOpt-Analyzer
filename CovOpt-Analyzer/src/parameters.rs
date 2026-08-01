//! Parameter discovery, candidate domains, and dependency metadata.
//!
//! The macro is deliberately a source-contract front end.  This module is the
//! analyzer-side model. Parameter classes and tags describe semantics and
//! constraints; all evaluated optimization uses the shared annealed Monte Carlo
//! engine in `crate::search`.

use covopt_schema::{
    CouplingGroupId, EvaluationMode, ParameterClass, ParameterDescriptor, ParameterDisposition,
    ParameterDomain, ParameterId, ParameterPhase, ParameterProperty, ParameterRange,
    ParameterRecord, ParameterTag, ParameterValue, SourceAnchor,
};
use quote::ToTokens;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use syn::parse::Parse;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

pub use covopt_schema::{
    CouplingGroupId as SharedCouplingGroupId, EvaluationMode as SharedEvaluationMode,
    ParameterClass as SharedParameterClass, ParameterDescriptor as SharedParameterDescriptor,
    ParameterDisposition as SharedParameterDisposition, ParameterDomain as SharedParameterDomain,
    ParameterId as SharedParameterId, ParameterPhase as SharedParameterPhase,
    ParameterProperty as SharedParameterProperty, ParameterRecord as SharedParameterRecord,
    ParameterTag as SharedParameterTag, ParameterValue as SharedParameterValue,
    SourceAnchor as SharedSourceAnchor,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParameterCandidate {
    pub parameter_id: ParameterId,
    pub value: ParameterValue,
    pub seed: u64,
    pub candidate_hash: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParameterObservation {
    pub candidate: ParameterCandidate,
    pub score: Option<f64>,
    pub status: crate::assurance::ObligationStatus,
    pub summary: String,
}

fn candidate(
    parameter: &ParameterDescriptor,
    value: ParameterValue,
    seed: u64,
) -> ParameterCandidate {
    let candidate_hash = format!("{}:{}:{}", parameter.id, value_fingerprint(&value), seed);
    ParameterCandidate {
        parameter_id: parameter.id.clone(),
        value,
        seed,
        candidate_hash,
    }
}

fn value_fingerprint(value: &ParameterValue) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "unknown".to_string())
}

fn domain_values(parameter: &ParameterDescriptor, budget: usize) -> Vec<ParameterValue> {
    match &parameter.domain {
        ParameterDomain::Values(values) => values.iter().take(budget).cloned().collect(),
        ParameterDomain::Range(range) => range_values(&range.min, &range.max, budget)
            .unwrap_or_else(|| vec![parameter.default.clone()]),
        ParameterDomain::Unknown => vec![parameter.default.clone()],
    }
}

fn range_values(
    min: &ParameterValue,
    max: &ParameterValue,
    budget: usize,
) -> Option<Vec<ParameterValue>> {
    if budget == 0 {
        return Some(Vec::new());
    }
    let steps = budget.max(1);
    let fraction = |index: usize| index as f64 / steps.saturating_sub(1).max(1) as f64;
    match (min, max) {
        (ParameterValue::Float(min), ParameterValue::Float(max)) if min <= max => Some(
            (0..steps)
                .map(|index| ParameterValue::Float(min + (max - min) * fraction(index)))
                .collect(),
        ),
        (ParameterValue::Signed(min), ParameterValue::Signed(max)) if min <= max => Some(
            (0..steps)
                .map(|index| {
                    ParameterValue::Signed(
                        *min + ((*max - *min) as f64 * fraction(index)).round() as i128,
                    )
                })
                .collect(),
        ),
        (ParameterValue::Unsigned(min), ParameterValue::Unsigned(max)) if min <= max => Some(
            unsigned_range(*min, *max, steps, fraction, ParameterValue::Unsigned),
        ),
        (ParameterValue::DurationNs(min), ParameterValue::DurationNs(max)) if min <= max => Some(
            unsigned_range(*min, *max, steps, fraction, ParameterValue::DurationNs),
        ),
        (ParameterValue::Count(min), ParameterValue::Count(max)) if min <= max => Some(
            unsigned_range(*min, *max, steps, fraction, ParameterValue::Count),
        ),
        (ParameterValue::Bytes(min), ParameterValue::Bytes(max)) if min <= max => Some(
            unsigned_range(*min, *max, steps, fraction, ParameterValue::Bytes),
        ),
        _ => None,
    }
}

fn unsigned_range(
    min: u128,
    max: u128,
    steps: usize,
    fraction: impl Fn(usize) -> f64,
    map: impl Fn(u128) -> ParameterValue,
) -> Vec<ParameterValue> {
    (0..steps)
        .map(|index| {
            let value = min as f64 + (max.saturating_sub(min) as f64 * fraction(index));
            map(value.round().max(0.0) as u128)
        })
        .collect()
}

pub fn best_observation(observations: &[ParameterObservation]) -> ParameterObservation {
    observations
        .iter()
        .max_by(|left, right| {
            left.score
                .partial_cmp(&right.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .cloned()
        .unwrap_or_else(|| ParameterObservation {
            candidate: ParameterCandidate {
                parameter_id: ParameterId::new("unknown"),
                value: ParameterValue::Categorical("unknown".to_string()),
                seed: 0,
                candidate_hash: "unknown".to_string(),
            },
            score: None,
            status: crate::assurance::ObligationStatus::Unknown,
            summary: "no parameter observation".to_string(),
        })
}

/// Produce an unevaluated domain preview. This is deliberately not a search
/// strategy: class/tag metadata never changes how candidates are explored.
pub fn propose_parameter_candidates(
    parameter: &ParameterDescriptor,
    seed: u64,
    budget: usize,
) -> Vec<ParameterCandidate> {
    domain_values(parameter, budget)
        .into_iter()
        .map(|value| candidate(parameter, value, seed))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CouplingGroup {
    pub id: CouplingGroupId,
    pub members: Vec<ParameterId>,
    pub strength: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerturbationObservation {
    pub parameter_id: ParameterId,
    pub baseline: ParameterValue,
    pub perturbed: ParameterValue,
    pub score_delta: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RobustnessEnvelope {
    pub group: CouplingGroupId,
    pub minimum_score_delta: f64,
    pub maximum_score_delta: f64,
    pub observation_count: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParameterDependencyGraph {
    pub schema_version: u32,
    pub parameters: BTreeMap<ParameterId, ParameterRecord>,
    pub coupling_groups: BTreeMap<CouplingGroupId, CouplingGroup>,
    #[serde(default)]
    pub perturbations: Vec<PerturbationObservation>,
    #[serde(default)]
    pub robustness_envelopes: BTreeMap<CouplingGroupId, RobustnessEnvelope>,
}

impl ParameterDependencyGraph {
    pub fn from_source(source: &str, file: &str) -> Result<Self, String> {
        let syntax = syn::parse_file(source).map_err(|error| error.to_string())?;
        let mut visitor = ParameterVisitor {
            file: file.to_string(),
            functions: Vec::new(),
            parameters: BTreeMap::new(),
            groups: Vec::new(),
            duplicate_ids: BTreeSet::new(),
        };
        visitor.visit_file(&syntax);
        if !visitor.duplicate_ids.is_empty() {
            return Err(format!(
                "duplicate covopt_param IDs: {}",
                visitor
                    .duplicate_ids
                    .into_iter()
                    .map(|id| id.0)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let mut graph = Self {
            schema_version: covopt_schema::SCHEMA_VERSION,
            parameters: visitor.parameters,
            coupling_groups: BTreeMap::new(),
            perturbations: Vec::new(),
            robustness_envelopes: BTreeMap::new(),
        };
        for members in visitor.groups {
            let mut members = members.into_iter().collect::<Vec<_>>();
            members.sort();
            members.dedup();
            if members.len() < 2 {
                continue;
            }
            let id = CouplingGroupId(format!(
                "coupling::{}",
                members
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("+")
            ));
            for member in &members {
                if let Some(record) = graph.parameters.get_mut(member) {
                    record.coupling_group = Some(id.clone());
                    if !record
                        .descriptor
                        .tags
                        .iter()
                        .any(|tag| matches!(tag, ParameterTag::Custom(value) if value == "coupled"))
                    {
                        record
                            .descriptor
                            .tags
                            .push(ParameterTag::Custom("coupled".to_string()));
                    }
                    record.properties.push(ParameterProperty::Coupled {
                        group: id.0.clone(),
                        strength: 1.0,
                    });
                }
            }
            graph.coupling_groups.insert(
                id.clone(),
                CouplingGroup {
                    id,
                    members,
                    strength: 1.0,
                },
            );
        }
        Ok(graph)
    }

    pub fn stable_order(&self) -> Vec<&ParameterRecord> {
        self.parameters.values().collect()
    }

    pub fn diff_ids(&self, other: &Self) -> (Vec<ParameterId>, Vec<ParameterId>) {
        let added = other
            .parameters
            .keys()
            .filter(|id| !self.parameters.contains_key(*id))
            .cloned()
            .collect();
        let removed = self
            .parameters
            .keys()
            .filter(|id| !other.parameters.contains_key(*id))
            .cloned()
            .collect();
        (added, removed)
    }

    pub fn record_perturbation(&mut self, observation: PerturbationObservation) {
        self.perturbations.push(observation);
        self.rebuild_robustness_envelopes();
    }

    pub fn apply_confirmed_candidates(
        &mut self,
        candidates: &[ParameterCandidate],
        clean_confirmation: bool,
    ) -> Result<(), String> {
        if !clean_confirmation {
            return Err(
                "candidate confirmation was not clean; defaults remain unchanged".to_string(),
            );
        }
        for candidate in candidates {
            let Some(record) = self.parameters.get_mut(&candidate.parameter_id) else {
                return Err(format!(
                    "unknown parameter candidate: {}",
                    candidate.parameter_id
                ));
            };
            record.phase = ParameterPhase::Confirmed {
                candidate_hash: candidate.candidate_hash.clone(),
            };
            record.disposition = ParameterDisposition::UpdateDefault {
                value: candidate.value.clone(),
                candidate_hash: candidate.candidate_hash.clone(),
            };
        }
        Ok(())
    }

    pub fn sensitivity_screen(&self) -> Vec<ParameterId> {
        let sensitivity = self
            .perturbations
            .iter()
            .map(|observation| {
                (
                    observation.parameter_id.clone(),
                    observation.score_delta.abs(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut ids = sensitivity.keys().cloned().collect::<Vec<_>>();
        ids.sort_by(|left, right| {
            sensitivity
                .get(right)
                .unwrap_or(&0.0)
                .partial_cmp(sensitivity.get(left).unwrap_or(&0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(left.cmp(right))
        });
        ids
    }

    pub fn minimal_repair_groups(&self, affected: &[ParameterId]) -> Vec<CouplingGroupId> {
        let affected = affected.iter().collect::<BTreeSet<_>>();
        self.coupling_groups
            .iter()
            .filter(|(_, group)| group.members.iter().any(|id| affected.contains(id)))
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub fn propose_joint_candidates(
        &self,
        group_id: &CouplingGroupId,
        seed: u64,
        budget: usize,
    ) -> Vec<BTreeMap<ParameterId, ParameterCandidate>> {
        let Some(group) = self.coupling_groups.get(group_id) else {
            return Vec::new();
        };
        let candidates = group
            .members
            .iter()
            .filter_map(|id| self.parameters.get(id))
            .map(|record| propose_parameter_candidates(&record.descriptor, seed, budget))
            .collect::<Vec<_>>();
        let count = candidates.iter().map(Vec::len).min().unwrap_or(0);
        (0..count)
            .map(|index| {
                group
                    .members
                    .iter()
                    .zip(candidates.iter())
                    .filter_map(|(id, values)| {
                        values.get(index).cloned().map(|value| (id.clone(), value))
                    })
                    .collect()
            })
            .collect()
    }

    fn rebuild_robustness_envelopes(&mut self) {
        self.robustness_envelopes.clear();
        for (group_id, group) in &self.coupling_groups {
            let observations = self
                .perturbations
                .iter()
                .filter(|observation| group.members.contains(&observation.parameter_id))
                .collect::<Vec<_>>();
            if observations.is_empty() {
                continue;
            }
            let minimum_score_delta = observations
                .iter()
                .map(|observation| observation.score_delta)
                .fold(f64::INFINITY, f64::min);
            let maximum_score_delta = observations
                .iter()
                .map(|observation| observation.score_delta)
                .fold(f64::NEG_INFINITY, f64::max);
            self.robustness_envelopes.insert(
                group_id.clone(),
                RobustnessEnvelope {
                    group: group_id.clone(),
                    minimum_score_delta,
                    maximum_score_delta,
                    observation_count: observations.len(),
                },
            );
        }
    }
}

struct ParameterVisitor {
    file: String,
    functions: Vec<String>,
    parameters: BTreeMap<ParameterId, ParameterRecord>,
    groups: Vec<BTreeSet<ParameterId>>,
    duplicate_ids: BTreeSet<ParameterId>,
}

impl<'ast> Visit<'ast> for ParameterVisitor {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        self.functions.push(node.sig.ident.to_string());
        visit::visit_item_fn(self, node);
        self.functions.pop();
    }

    fn visit_expr_macro(&mut self, node: &'ast syn::ExprMacro) {
        if node
            .mac
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "covopt_param")
            && let Ok(args) = syn::parse2::<MacroParamArgs>(node.mac.tokens.clone())
        {
            let id = ParameterId::new(args.id.value());
            let mut options = syn::parse2::<MacroOptions>(args.rest.clone()).unwrap_or_default();
            let inferred = options.class.is_none()
                && options.domain.is_none()
                && options.unit.is_none()
                && options.tags.is_empty();
            if let Some(range) = args.legacy_range {
                options.domain = Some(ParameterDomain::Range(ParameterRange {
                    min: value_from_expr(range.start.as_deref().unwrap_or(&syn::parse_quote!(0))),
                    max: value_from_expr(range.end.as_deref().unwrap_or(&syn::parse_quote!(0))),
                    inclusive_max: matches!(range.limits, syn::RangeLimits::Closed(_)),
                }));
            }
            let function = self
                .functions
                .last()
                .cloned()
                .unwrap_or_else(|| "module".to_string());
            let source = SourceAnchor {
                file: self.file.clone(),
                line: node.mac.path.span().start().line,
                column: node.mac.path.span().start().column,
            };
            let class = options.class.unwrap_or_else(|| {
                infer_class_from_context(&id.0, &function, &args.default, options.unit.as_deref())
            });
            let domain = options.domain.unwrap_or(ParameterDomain::Unknown);
            let mut tags = options.tags;
            let class_tag = class_to_tag(class);
            if !tags.contains(&class_tag) {
                tags.push(class_tag);
            }
            let descriptor = ParameterDescriptor {
                schema_version: covopt_schema::SCHEMA_VERSION,
                id: id.clone(),
                default: value_from_expr_with_unit(&args.default, options.unit.as_deref()),
                domain,
                class,
                evaluation: options.evaluation.unwrap_or(EvaluationMode::Runtime),
                tags,

                source,
                unit: options.unit,
                inferred,
                confidence: Some(if inferred { 0.5 } else { 1.0 }),
                inference_source: Some(if inferred {
                    "parameter ID/function/default AST inference".to_string()
                } else {
                    "explicit covopt_param metadata".to_string()
                }),
            };
            match self.parameters.entry(id) {
                std::collections::btree_map::Entry::Occupied(entry) => {
                    self.duplicate_ids.insert(entry.key().clone());
                }
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(ParameterRecord {
                        descriptor,
                        phase: ParameterPhase::Discovered,
                        properties: Vec::new(),
                        disposition: ParameterDisposition::KeepTunable,
                        coupling_group: None,
                    });
                }
            }
        }
        let ids = parameter_ids_in_tokens(&node.mac.tokens);
        if ids.len() > 1 {
            self.groups.push(ids);
        }
        visit::visit_expr_macro(self, node);
    }

    fn visit_item_macro(&mut self, node: &'ast syn::ItemMacro) {
        if node.mac.path.segments.last().is_some_and(|segment| {
            matches!(segment.ident.to_string().as_str(), "qsbr" | "qsbr_domain")
        }) && let Ok(args) = syn::parse2::<QsbrDomainArgs>(node.mac.tokens.clone())
        {
            self.insert_qsbr_capacity_parameters(&args.name, node.mac.path.span());
        }
        visit::visit_item_macro(self, node);
    }

    fn visit_expr_binary(&mut self, node: &'ast syn::ExprBinary) {
        let ids = parameter_ids_in_expr(node);
        if ids.len() > 1 {
            self.groups.push(ids);
        }
        visit::visit_expr_binary(self, node);
    }

    fn visit_expr_if(&mut self, node: &'ast syn::ExprIf) {
        let ids = parameter_ids_in_expr(node);
        if ids.len() > 1 {
            self.groups.push(ids);
        }
        visit::visit_expr_if(self, node);
    }

    fn visit_expr_for_loop(&mut self, node: &'ast syn::ExprForLoop) {
        let ids = parameter_ids_in_expr(node);
        if ids.len() > 1 {
            self.groups.push(ids);
        }
        visit::visit_expr_for_loop(self, node);
    }
}

impl ParameterVisitor {
    fn insert_qsbr_capacity_parameters(&mut self, domain: &syn::Ident, span: proc_macro2::Span) {
        let source = SourceAnchor {
            file: self.file.clone(),
            line: span.start().line,
            column: span.start().column,
        };
        let group = [
            (
                "participant_capacity",
                64,
                4_096,
                "participants",
                &["memory", "availability"][..],
            ),
            (
                "retire_capacity",
                256,
                65_536,
                "records_per_participant",
                &["memory", "backpressure"][..],
            ),
            (
                "reclaim_batch_capacity",
                128,
                4_096,
                "records_per_callback",
                &["stack", "throughput"][..],
            ),
        ]
        .into_iter()
        .map(|(suffix, default, maximum, unit, risks)| {
            // `module_path!()` is resolved only after macro expansion. The
            // analyzer keeps the stable source-visible suffix; reports can
            // match the concrete runtime ID by this exact domain suffix.
            let id = ParameterId::new(format!("{}::{suffix}", domain));
            let tags = std::iter::once(ParameterTag::Capacity)
                .chain(
                    risks
                        .iter()
                        .map(|risk| ParameterTag::Custom((*risk).to_string())),
                )
                .chain([
                    ParameterTag::Custom("pow2".to_string()),
                    ParameterTag::Custom("qsbr-static-capacity".to_string()),
                ])
                .collect();
            let descriptor = ParameterDescriptor {
                schema_version: covopt_schema::SCHEMA_VERSION,
                id: id.clone(),
                default: ParameterValue::Count(default),
                domain: ParameterDomain::Range(ParameterRange {
                    min: ParameterValue::Count(1),
                    max: ParameterValue::Count(maximum),
                    inclusive_max: true,
                }),
                class: ParameterClass::Capacity,
                evaluation: EvaluationMode::CompileTime,
                tags,
                source: source.clone(),
                unit: Some(unit.to_string()),
                inferred: false,
                confidence: Some(1.0),
                inference_source: Some(
                    "no_std_tool qsbr!/qsbr_domain! expansion contract".to_string(),
                ),
            };
            let record = ParameterRecord {
                descriptor,
                phase: ParameterPhase::Discovered,
                properties: vec![ParameterProperty::Constrained {
                    reason: "compile-time power-of-two static capacity".to_string(),
                }],
                disposition: ParameterDisposition::KeepTunable,
                coupling_group: None,
            };
            (id, record)
        })
        .collect::<Vec<_>>();

        let mut members = BTreeSet::new();
        for (id, record) in group {
            members.insert(id.clone());
            match self.parameters.entry(id) {
                std::collections::btree_map::Entry::Occupied(entry) => {
                    self.duplicate_ids.insert(entry.key().clone());
                }
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(record);
                }
            }
        }
        self.groups.push(members);
    }
}

struct QsbrDomainArgs {
    _visibility: syn::Visibility,
    _mod_token: syn::Token![mod],
    name: syn::Ident,
    _semi: syn::Token![;],
}

impl Parse for QsbrDomainArgs {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        Ok(Self {
            _visibility: input.parse()?,
            _mod_token: input.parse()?,
            name: input.parse()?,
            _semi: input.parse()?,
        })
    }
}

struct MacroParamArgs {
    id: syn::LitStr,
    _comma: syn::Token![,],
    default: syn::Expr,
    legacy_range: Option<syn::ExprRange>,
    rest: proc_macro2::TokenStream,
}

#[derive(Default)]
struct MacroOptions {
    class: Option<ParameterClass>,
    evaluation: Option<EvaluationMode>,
    unit: Option<String>,
    tags: Vec<ParameterTag>,
    domain: Option<ParameterDomain>,
}

impl Parse for MacroOptions {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let mut options = Self::default();
        while !input.is_empty() {
            if input.peek(syn::Token![,]) {
                input.parse::<syn::Token![,]>()?;
            }
            let key: syn::Ident = input.parse()?;
            input.parse::<syn::Token![=]>()?;
            match key.to_string().as_str() {
                "class" => {
                    let value: syn::LitStr = input.parse()?;
                    options.class = Some(parse_class(&value.value()));
                }
                "evaluation" => {
                    let value: syn::LitStr = input.parse()?;
                    options.evaluation = Some(if value.value() == "compile_time" {
                        EvaluationMode::CompileTime
                    } else {
                        EvaluationMode::Runtime
                    });
                }
                "unit" => options.unit = Some(input.parse::<syn::LitStr>()?.value()),
                "risk" => {
                    let values: syn::ExprArray = input.parse()?;
                    for value in values.elems {
                        if let syn::Expr::Lit(syn::ExprLit {
                            lit: syn::Lit::Str(value),
                            ..
                        }) = value
                            && let Some(tag) = ParameterTag::parse(&value.value())
                        {
                            options.tags.push(tag);
                        }
                    }
                }
                "range" => {
                    let range: syn::ExprRange = input.parse()?;
                    if let (Some(start), Some(end)) = (range.start.as_deref(), range.end.as_deref())
                    {
                        options.domain = Some(ParameterDomain::Range(ParameterRange {
                            min: value_from_expr(start),
                            max: value_from_expr(end),
                            inclusive_max: matches!(range.limits, syn::RangeLimits::Closed(_)),
                        }));
                    }
                }
                _ => {
                    let _: syn::Expr = input.parse()?;
                }
            }
        }
        Ok(options)
    }
}

impl Parse for MacroParamArgs {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let id = input.parse()?;
        let comma = input.parse()?;
        let default = input.parse()?;
        let mut legacy_range = None;
        if input.peek(syn::Token![,]) {
            let fork = input.fork();
            if fork.parse::<syn::Token![,]>().is_ok() && fork.parse::<syn::ExprRange>().is_ok() {
                input.parse::<syn::Token![,]>()?;
                legacy_range = Some(input.parse::<syn::ExprRange>()?);
            }
        }
        let rest = input.parse()?;
        Ok(Self {
            id,
            _comma: comma,
            default,
            legacy_range,
            rest,
        })
    }
}

fn parameter_ids_in_tokens(tokens: &proc_macro2::TokenStream) -> BTreeSet<ParameterId> {
    let mut result = BTreeSet::new();
    if let Ok(args) = syn::parse2::<MacroParamArgs>(tokens.clone()) {
        result.insert(ParameterId::new(args.id.value()));
    }
    result
}

struct ParameterIdVisitor {
    ids: BTreeSet<ParameterId>,
}

impl<'ast> Visit<'ast> for ParameterIdVisitor {
    fn visit_expr_macro(&mut self, node: &'ast syn::ExprMacro) {
        if node
            .mac
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "covopt_param")
        {
            self.ids.extend(parameter_ids_in_tokens(&node.mac.tokens));
        }
        visit::visit_expr_macro(self, node);
    }
}

fn parameter_ids_in_expr<T: quote::ToTokens>(value: &T) -> BTreeSet<ParameterId> {
    let Ok(expr) = syn::parse2::<syn::Expr>(value.to_token_stream()) else {
        return BTreeSet::new();
    };
    let mut visitor = ParameterIdVisitor {
        ids: BTreeSet::new(),
    };
    visitor.visit_expr(&expr);
    visitor.ids
}

fn infer_class(id: &str) -> ParameterClass {
    let id = id.to_ascii_lowercase();
    if id.contains("timeout") {
        ParameterClass::Timeout
    } else if id.contains("seed") {
        ParameterClass::Seed
    } else if id.contains("capacity") || id.contains("size") {
        ParameterClass::Capacity
    } else if id.contains("retry") {
        ParameterClass::Retry
    } else if id.contains("threshold") || id.contains("limit") {
        ParameterClass::Threshold
    } else {
        ParameterClass::Unknown
    }
}

fn infer_class_from_context(
    id: &str,
    function: &str,
    default: &syn::Expr,
    unit: Option<&str>,
) -> ParameterClass {
    let combined = format!("{}::{function}", id).to_ascii_lowercase();
    if combined.contains("timeout") {
        ParameterClass::Timeout
    } else if combined.contains("retry") {
        ParameterClass::Retry
    } else if combined.contains("seed") {
        ParameterClass::Seed
    } else if unit.is_some_and(|unit| unit.to_ascii_lowercase().contains("byte"))
        || combined.contains("capacity")
        || combined.contains("size")
    {
        ParameterClass::Capacity
    } else if combined.contains("budget") {
        ParameterClass::Budget
    } else if combined.contains("threshold") || combined.contains("limit") {
        ParameterClass::Threshold
    } else if matches!(
        default,
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Float(_),
            ..
        })
    ) {
        ParameterClass::Coefficient
    } else {
        infer_class(id)
    }
}

fn parse_class(value: &str) -> ParameterClass {
    match value {
        "threshold" => ParameterClass::Threshold,
        "capacity" => ParameterClass::Capacity,
        "budget" => ParameterClass::Budget,
        "timeout" => ParameterClass::Timeout,
        "retry" => ParameterClass::Retry,
        "tolerance" => ParameterClass::Tolerance,
        "coefficient" => ParameterClass::Coefficient,
        "seed" => ParameterClass::Seed,
        "layout" => ParameterClass::Layout,
        "ordering" => ParameterClass::Ordering,
        _ => ParameterClass::Unknown,
    }
}

fn class_to_tag(class: ParameterClass) -> ParameterTag {
    match class {
        ParameterClass::Threshold => ParameterTag::Threshold,
        ParameterClass::Capacity => ParameterTag::Capacity,
        ParameterClass::Budget => ParameterTag::Budget,
        ParameterClass::Timeout => ParameterTag::Timeout,
        ParameterClass::Retry => ParameterTag::Retry,
        ParameterClass::Tolerance => ParameterTag::Tolerance,
        ParameterClass::Coefficient => ParameterTag::Coefficient,
        ParameterClass::Seed => ParameterTag::Seed,
        ParameterClass::Layout => ParameterTag::Layout,
        ParameterClass::Ordering => ParameterTag::Ordering,
        ParameterClass::Unknown => ParameterTag::Custom("unknown".to_string()),
    }
}

fn value_from_expr(expr: &syn::Expr) -> ParameterValue {
    match expr {
        syn::Expr::Unary(syn::ExprUnary {
            op: syn::UnOp::Neg(_),
            expr,
            ..
        }) => match &**expr {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Int(value),
                ..
            }) => value
                .base10_parse::<i128>()
                .map(|value| ParameterValue::Signed(-value))
                .unwrap_or_else(|_| {
                    ParameterValue::Categorical(expr.to_token_stream().to_string())
                }),
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Float(value),
                ..
            }) => value
                .base10_parse::<f64>()
                .map(|value| ParameterValue::Float(-value))
                .unwrap_or_else(|_| {
                    ParameterValue::Categorical(expr.to_token_stream().to_string())
                }),
            _ => ParameterValue::Categorical(expr.to_token_stream().to_string()),
        },
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(value),
            ..
        }) => value
            .base10_parse::<u128>()
            .map(ParameterValue::Unsigned)
            .unwrap_or_else(|_| ParameterValue::Categorical(value.to_string())),
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Float(value),
            ..
        }) => value
            .base10_parse::<f64>()
            .map(ParameterValue::Float)
            .unwrap_or_else(|_| ParameterValue::Categorical(value.to_string())),
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(value),
            ..
        }) => ParameterValue::Categorical(value.value()),
        _ => ParameterValue::Categorical(expr.to_token_stream().to_string()),
    }
}

fn value_from_expr_with_unit(expr: &syn::Expr, unit: Option<&str>) -> ParameterValue {
    let value = value_from_expr(expr);
    match (unit.map(str::to_ascii_lowercase), value) {
        (Some(unit), ParameterValue::Unsigned(value)) if unit.contains("byte") || unit == "b" => {
            ParameterValue::Bytes(value)
        }
        (Some(unit), ParameterValue::Unsigned(value)) if unit.contains("count") => {
            ParameterValue::Count(value)
        }
        (Some(unit), ParameterValue::Unsigned(value))
            if matches!(unit.as_str(), "ns" | "us" | "ms" | "s" | "duration") =>
        {
            ParameterValue::DurationNs(value)
        }
        (_, value) => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_stable_parameters_and_coupling() {
        let graph = ParameterDependencyGraph::from_source(
            "fn run() { let _ = covopt_param!(\"queue::limit\", 64, 1..=64) + covopt_param!(\"queue::retry\", 3, 1..=8); }",
            "src/lib.rs",
        ).unwrap();
        assert!(
            graph
                .parameters
                .contains_key(&ParameterId::new("queue::limit"))
        );
        assert_eq!(graph.coupling_groups.len(), 1);
        assert_eq!(graph.schema_version, covopt_schema::SCHEMA_VERSION);
        let group = graph.coupling_groups.keys().next().unwrap();
        assert_eq!(graph.propose_joint_candidates(group, 0, 3).len(), 3);
    }

    #[test]
    fn parses_structured_and_legacy_parameter_metadata() {
        let graph = ParameterDependencyGraph::from_source(
            r#"
                fn run() {
                    let _ = covopt_param!("structured", 8, class = "capacity", range = 1..=64, evaluation = "compile_time", unit = "bytes", risk = ["latency"]);
                    let _ = covopt_param!("legacy", 4, 1..=32);
                }
            "#,
            "src/lib.rs",
        )
        .unwrap();
        let structured = &graph.parameters[&ParameterId::new("structured")].descriptor;
        assert_eq!(structured.class, ParameterClass::Capacity);
        assert_eq!(structured.evaluation, EvaluationMode::CompileTime);
        assert_eq!(structured.unit.as_deref(), Some("bytes"));
        assert_eq!(structured.default, ParameterValue::Bytes(8));
        assert!(matches!(structured.domain, ParameterDomain::Range(_)));
        assert!(graph.parameters.contains_key(&ParameterId::new("legacy")));
    }

    #[test]
    fn discovers_no_std_tool_qsbr_static_capacities() {
        let graph = ParameterDependencyGraph::from_source(
            r#"
                no_std_tool::qsbr! { pub mod cache_domain; }
                no_std_tool::qsbr_domain! { mod io_domain; }
            "#,
            "src/lib.rs",
        )
        .unwrap();

        assert_eq!(graph.parameters.len(), 6);
        let participant =
            &graph.parameters[&ParameterId::new("cache_domain::participant_capacity")].descriptor;
        assert_eq!(participant.default, ParameterValue::Count(64));
        assert_eq!(participant.evaluation, EvaluationMode::CompileTime);
        assert!(matches!(participant.domain, ParameterDomain::Range(_)));
        assert!(
            participant
                .tags
                .contains(&ParameterTag::Custom("qsbr-static-capacity".to_string()))
        );
        assert_eq!(graph.coupling_groups.len(), 2);
    }

    #[test]
    fn duplicate_parameter_ids_are_rejected() {
        let error = ParameterDependencyGraph::from_source(
            "fn run() { let _ = covopt_param!(\"same\", 1) + covopt_param!(\"same\", 2); }",
            "src/lib.rs",
        )
        .unwrap_err();
        assert!(error.contains("duplicate covopt_param IDs"));
    }

    #[test]
    fn perturbations_produce_sensitivity_and_robustness_metadata() {
        let mut graph = ParameterDependencyGraph::from_source(
            "fn run() { let _ = covopt_param!(\"a\", 1) + covopt_param!(\"b\", 2); }",
            "src/lib.rs",
        )
        .unwrap();
        graph.record_perturbation(PerturbationObservation {
            parameter_id: ParameterId::new("a"),
            baseline: ParameterValue::Unsigned(1),
            perturbed: ParameterValue::Unsigned(2),
            score_delta: 0.8,
        });
        assert_eq!(graph.sensitivity_screen(), vec![ParameterId::new("a")]);
        assert_eq!(graph.robustness_envelopes.len(), 1);
        assert_eq!(
            graph.minimal_repair_groups(&[ParameterId::new("a")]).len(),
            1
        );
    }

    #[test]
    fn only_clean_confirmation_can_update_parameter_disposition() {
        let mut graph = ParameterDependencyGraph::from_source(
            "fn run() { let _ = covopt_param!(\"a\", 1); }",
            "src/lib.rs",
        )
        .unwrap();
        let candidate = ParameterCandidate {
            parameter_id: ParameterId::new("a"),
            value: ParameterValue::Unsigned(2),
            seed: 1,
            candidate_hash: "candidate".to_string(),
        };
        assert!(
            graph
                .apply_confirmed_candidates(std::slice::from_ref(&candidate), false)
                .is_err()
        );
        graph
            .apply_confirmed_candidates(&[candidate], true)
            .unwrap();
        assert!(matches!(
            graph.parameters[&ParameterId::new("a")].phase,
            ParameterPhase::Confirmed { .. }
        ));
    }
}
