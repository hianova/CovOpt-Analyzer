//! Rust struct layout inspection and conservative layout candidates.

use crate::findings::{Finding, FindingKind};
use crate::repair::{RepairCandidate, RepairCandidateId, RepairKind, RiskLevel};
use quote::ToTokens;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::HashSet;
use std::path::Path;
use syn::spanned::Spanned;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReprKind {
    Rust,
    C,
    Transparent,
    Packed,
    Align,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutField {
    pub name: String,
    pub ty: String,
    pub estimated_size: Option<usize>,
    pub estimated_alignment: Option<usize>,
    pub atomic: bool,
    #[serde(default)]
    pub access_locations: Vec<String>,
    #[serde(default)]
    pub thread_owners: Vec<String>,
    pub hot: bool,
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutModel {
    pub name: String,
    pub visibility: String,
    pub repr: ReprKind,
    pub repr_public_abi: bool,
    pub explicit_alignment: Option<usize>,
    pub packed: bool,
    pub fields: Vec<LayoutField>,
    pub static_confidence: f64,
    pub dynamic_observed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LayoutCandidateKind {
    FieldReorder,
    ExplicitPadding,
    CacheLineAlignment,
    AtomicIsolation,
    HotColdSplit,
    AoSToSoA,
    RemovePadding,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutDelta {
    pub size_delta: Option<i64>,
    pub alignment_delta: Option<i64>,
    pub estimated_cache_lines_delta: Option<i64>,
    pub false_sharing_edges_delta: Option<i64>,
    pub abi_risk: RiskLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutCandidate {
    pub id: String,
    pub struct_name: String,
    pub kind: LayoutCandidateKind,
    pub field_order: Vec<String>,
    pub delta: LayoutDelta,
    pub repair: RepairCandidate,
    pub locked_fields: Vec<String>,
    pub safety: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutConfig {
    pub max_candidates: usize,
    pub allow_public_abi_suggestions: bool,
    pub cache_line_bytes: usize,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            max_candidates: 32,
            allow_public_abi_suggestions: false,
            cache_line_bytes: 64,
        }
    }
}

pub fn extract_layout(source: &str, struct_name: Option<&str>) -> Result<Vec<LayoutModel>, String> {
    let ast = syn::parse_file(source).map_err(|error| error.to_string())?;
    let mut models = Vec::new();
    for item in ast.items {
        let syn::Item::Struct(item) = item else {
            continue;
        };
        if struct_name.is_some_and(|name| item.ident != name) {
            continue;
        }
        let (repr, packed, alignment) = repr_info(&item.attrs);
        let fields = item
            .fields
            .iter()
            .filter_map(|field| {
                let name = field.ident.as_ref()?.to_string();
                let field_type = &field.ty;
                let ty = quote::quote!(#field_type).to_string();
                let estimated_size = estimate_type_size(&ty);
                let estimated_alignment = estimate_type_alignment(&ty);
                Some(LayoutField {
                    name,
                    atomic: ty.contains("Atomic"),
                    ty,
                    estimated_size,
                    estimated_alignment,
                    access_locations: Vec::new(),
                    thread_owners: Vec::new(),
                    hot: false,
                    offset: None,
                })
            })
            .collect::<Vec<_>>();
        let visibility = match &item.vis {
            syn::Visibility::Public(_) => "public",
            _ => "private",
        }
        .to_string();
        models.push(LayoutModel {
            name: item.ident.to_string(),
            visibility: visibility.clone(),
            repr,
            repr_public_abi: visibility == "public"
                && matches!(repr, ReprKind::C | ReprKind::Transparent),
            explicit_alignment: alignment,
            packed,
            fields,
            static_confidence: 0.50,
            dynamic_observed: false,
        });
    }
    Ok(models)
}

fn repr_info(attrs: &[syn::Attribute]) -> (ReprKind, bool, Option<usize>) {
    let mut repr = ReprKind::Rust;
    let mut packed = false;
    let mut align = None;
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("repr")) {
        let value = quote::quote!(#attr).to_string().replace(' ', "");
        if value.contains("repr(C") {
            repr = ReprKind::C;
        } else if value.contains("transparent") {
            repr = ReprKind::Transparent;
        } else if value.contains("packed") {
            repr = ReprKind::Packed;
            packed = true;
        }
        if let Some(start) = value.find("align(") {
            align = value[start + 6..]
                .split(')')
                .next()
                .and_then(|value| value.parse().ok());
            repr = ReprKind::Align;
        }
    }
    (repr, packed, align)
}

fn estimate_type_size(ty: &str) -> Option<usize> {
    Some(match ty.trim() {
        "u8" | "i8" | "bool" => 1,
        "u16" | "i16" => 2,
        "u32" | "i32" | "f32" => 4,
        "u64" | "i64" | "f64" | "usize" | "isize" | "AtomicUsize" | "AtomicI64" => 8,
        "u128" | "i128" => 16,
        _ if ty.contains("Atomic") => 8,
        _ => return None,
    })
}

fn estimate_type_alignment(ty: &str) -> Option<usize> {
    estimate_type_size(ty).map(|size| size.min(8).next_power_of_two())
}

pub fn layout_findings(model: &LayoutModel, file: impl Into<String>) -> Vec<Finding> {
    let file = file.into();
    let atomic_count = model.fields.iter().filter(|field| field.atomic).count();
    if atomic_count == 0 {
        return Vec::new();
    }
    let kind = if model.fields.iter().any(|field| field.hot) {
        FindingKind::FalseSharing
    } else {
        FindingKind::PoorFieldLocality
    };
    let id = crate::findings::stable_finding_id(kind, &file, 1, Some(&model.name));
    vec![
        Finding::new(
            id.0,
            kind,
            crate::assurance::Severity::High,
            file,
            1,
            Some(model.name.clone()),
        )
        .modeled(),
    ]
}

pub fn generate_candidates(
    model: &LayoutModel,
    findings: &[Finding],
    config: &LayoutConfig,
) -> Vec<LayoutCandidate> {
    let relevant = findings.iter().any(|finding| {
        finding.kind.is_layout() && finding.function.as_deref() == Some(model.name.as_str())
    });
    if !relevant && !model.fields.iter().any(|field| field.atomic) {
        return Vec::new();
    }
    let abi_blocked =
        matches!(model.repr, ReprKind::C | ReprKind::Transparent) && model.repr_public_abi;
    let mut candidates = Vec::new();
    let original = model
        .fields
        .iter()
        .map(|field| field.name.clone())
        .collect::<Vec<_>>();
    let locked = model
        .fields
        .iter()
        .filter(|field| field.atomic && field.thread_owners.len() > 1)
        .map(|field| field.name.clone())
        .collect::<HashSet<_>>();
    let mut reordered = original.clone();
    reordered.sort_by_key(|name| {
        model
            .fields
            .iter()
            .position(|field| &field.name == name)
            .map(|index| {
                (
                    !model.fields[index].hot,
                    Reverse(model.fields[index].estimated_alignment.unwrap_or_default()),
                )
            })
            .unwrap_or((true, Reverse(0)))
    });
    if reordered != original {
        let risk = if abi_blocked {
            RiskLevel::High
        } else if model.packed {
            RiskLevel::Unknown
        } else {
            RiskLevel::Medium
        };
        candidates.push(LayoutCandidate {
            id: format!("layout-{}-reorder", model.name),
            struct_name: model.name.clone(),
            kind: LayoutCandidateKind::FieldReorder,
            field_order: reordered.clone(),
            delta: LayoutDelta {
                size_delta: None,
                alignment_delta: None,
                estimated_cache_lines_delta: None,
                false_sharing_edges_delta: None,
                abi_risk: risk,
            },
            repair: RepairCandidate {
                id: RepairCandidateId(format!("layout-{}-reorder", model.name)),
                kind: RepairKind::ReorderFields,
                resolves: findings
                    .iter()
                    .filter(|finding| finding.kind.is_layout())
                    .map(|finding| finding.id.clone())
                    .collect(),
                changes: Vec::new(),
                dependencies: Vec::new(),
                conflicts: Vec::new(),
                semantic_risk: if model.packed {
                    RiskLevel::Unknown
                } else {
                    RiskLevel::Medium
                },
                api_risk: if model.repr_public_abi {
                    RiskLevel::High
                } else {
                    RiskLevel::Low
                },
                abi_risk: risk,
                estimated_benefit: crate::repair::PerformanceDelta {
                    confidence: model.static_confidence,
                    ..Default::default()
                },
                verification: Vec::new(),
                suggestion_only: abi_blocked || model.packed,
                description: if abi_blocked {
                    "public ABI layout reorder has a materializer but lacks an ABI compatibility evaluator contract"
                        .to_string()
                } else if model.packed {
                    "packed layout reorder has a materializer but lacks an alignment-safety evaluator contract"
                        .to_string()
                } else {
                    "field locality and false-sharing reorder candidate".to_string()
                },
            },
            locked_fields: locked.iter().cloned().collect(),
            safety: if model.packed {
                "packed layout requires alignment safety obligation".to_string()
            } else {
                "repr(Rust) offsets are unknown until compiled".to_string()
            },
        });
    }
    if atomic_count(model) > 0 && model.explicit_alignment != Some(config.cache_line_bytes) {
        let kind = LayoutCandidateKind::CacheLineAlignment;
        candidates.push(LayoutCandidate {
            id: format!("layout-{}-align-{}", model.name, config.cache_line_bytes),
            struct_name: model.name.clone(),
            kind,
            field_order: original.clone(),
            delta: LayoutDelta {
                size_delta: None,
                alignment_delta: None,
                estimated_cache_lines_delta: None,
                false_sharing_edges_delta: None,
                abi_risk: if model.repr_public_abi {
                    RiskLevel::High
                } else {
                    RiskLevel::Medium
                },
            },
            repair: RepairCandidate {
                id: RepairCandidateId(format!("layout-{}-align", model.name)),
                kind: RepairKind::AlignCacheLine,
                resolves: findings
                    .iter()
                    .filter(|finding| finding.kind == FindingKind::FalseSharing)
                    .map(|finding| finding.id.clone())
                    .collect(),
                changes: Vec::new(),
                dependencies: Vec::new(),
                conflicts: Vec::new(),
                semantic_risk: RiskLevel::Medium,
                api_risk: RiskLevel::Low,
                abi_risk: if model.repr_public_abi {
                    RiskLevel::High
                } else {
                    RiskLevel::Low
                },
                estimated_benefit: crate::repair::PerformanceDelta {
                    confidence: model.static_confidence,
                    ..Default::default()
                },
                verification: Vec::new(),
                suggestion_only: model.repr_public_abi || model.packed,
                description: if model.repr_public_abi {
                    "cache-line alignment has a materializer but lacks an ABI compatibility evaluator contract"
                        .to_string()
                } else if model.packed {
                    "cache-line alignment has a materializer but lacks an alignment-safety evaluator contract"
                        .to_string()
                } else {
                    "cache-line alignment candidate; verify object size and false-sharing evidence"
                        .to_string()
                },
            },
            locked_fields: Vec::new(),
            safety: "MCA cannot establish cache-miss improvement; verify compiled size/alignment and workload contention evidence (profiling is optional)"
                .to_string(),
        });
    }
    candidates.sort_by(|left, right| left.id.cmp(&right.id));
    candidates.truncate(config.max_candidates.max(1));
    candidates
}

/// Materialize a layout proposal into a source-hash-bound edit. Suggestions
/// that cannot be represented without changing unsupported syntax remain
/// suggestion-only and return `None`.
pub fn materialize_candidate(
    source: &str,
    file: &Path,
    candidate: &LayoutCandidate,
    cache_line_bytes: usize,
) -> Result<Option<crate::repair::SourceEdit>, String> {
    let syntax = syn::parse_file(source).map_err(|error| error.to_string())?;
    let item = syntax.items.into_iter().find_map(|item| match item {
        syn::Item::Struct(item) if item.ident == candidate.struct_name => Some(item),
        _ => None,
    });
    let Some(mut item) = item else {
        return Err(format!("struct `{}` was not found", candidate.struct_name));
    };
    let span = item.span();
    match candidate.kind {
        LayoutCandidateKind::FieldReorder => {
            let syn::Fields::Named(fields) = &mut item.fields else {
                return Ok(None);
            };
            let original = fields.named.iter().cloned().collect::<Vec<_>>();
            let mut reordered = syn::punctuated::Punctuated::new();
            for name in &candidate.field_order {
                let Some(field) = original.iter().find(|field| {
                    field
                        .ident
                        .as_ref()
                        .is_some_and(|identifier| identifier == name)
                }) else {
                    return Err(format!(
                        "layout candidate references unknown field `{name}`"
                    ));
                };
                reordered.push(field.clone());
            }
            if reordered.len() != original.len() {
                return Err("layout candidate does not preserve every field".to_string());
            }
            fields.named = reordered;
            crate::repair::SourceEdit::from_source(
                file.display().to_string(),
                source,
                span.start().line,
                span.start().column,
                span.end().line,
                span.end().column,
                item.to_token_stream().to_string(),
            )
            .ok_or_else(|| "could not anchor field reorder edit".to_string())
            .map(Some)
        }
        LayoutCandidateKind::CacheLineAlignment => {
            let insertion = format!("#[repr(align({cache_line_bytes}))]\n");
            crate::repair::SourceEdit::from_source(
                file.display().to_string(),
                source,
                span.start().line,
                span.start().column,
                span.start().line,
                span.start().column,
                insertion,
            )
            .ok_or_else(|| "could not anchor cache-line alignment edit".to_string())
            .map(Some)
        }
        _ => Ok(None),
    }
}

fn atomic_count(model: &LayoutModel) -> usize {
    model.fields.iter().filter(|field| field.atomic).count()
}

pub fn best_field_permutation(model: &LayoutModel, locked: &HashSet<String>) -> Vec<String> {
    let movable = model
        .fields
        .iter()
        .filter(|field| !locked.contains(&field.name))
        .map(|field| field.name.clone())
        .collect::<Vec<_>>();
    let mut best = model
        .fields
        .iter()
        .map(|field| field.name.clone())
        .collect::<Vec<_>>();
    let mut permutations = Vec::new();
    permute(&movable, &mut Vec::new(), &mut permutations);
    for permutation in permutations {
        let mut candidate = best.clone();
        let mut index = 0;
        for name in &mut candidate {
            if !locked.contains(name) {
                *name = permutation[index].clone();
                index += 1;
            }
        }
        if layout_score(model, &candidate) < layout_score(model, &best) {
            best = candidate;
        }
    }
    best
}

fn permute(values: &[String], current: &mut Vec<String>, output: &mut Vec<Vec<String>>) {
    if values.is_empty() {
        output.push(current.clone());
        return;
    }
    for index in 0..values.len() {
        current.push(values[index].clone());
        let mut rest = values.to_vec();
        rest.remove(index);
        permute(&rest, current, output);
        current.pop();
    }
}

fn layout_score(model: &LayoutModel, order: &[String]) -> usize {
    order
        .iter()
        .enumerate()
        .map(|(index, name)| {
            model
                .fields
                .iter()
                .find(|field| &field.name == name)
                .map(|field| if field.hot { index } else { 0 })
                .unwrap_or(0)
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_repr_c_reorder_is_suggestion_only() {
        let source = "#[repr(C)] pub struct S { a: u8, b: AtomicUsize }";
        let model = extract_layout(source, Some("S")).unwrap().pop().unwrap();
        assert!(matches!(model.repr, ReprKind::C));
        let finding = Finding::new(
            "f",
            FindingKind::FalseSharing,
            crate::assurance::Severity::High,
            "x.rs",
            1,
            Some("S".to_string()),
        );
        let candidates = generate_candidates(&model, &[finding], &LayoutConfig::default());
        assert!(
            candidates
                .iter()
                .all(|candidate| candidate.repair.suggestion_only)
        );
    }

    #[test]
    fn permutation_solver_is_deterministic() {
        let model = LayoutModel {
            name: "S".to_string(),
            visibility: "private".to_string(),
            repr: ReprKind::Rust,
            repr_public_abi: false,
            explicit_alignment: None,
            packed: false,
            fields: vec![
                LayoutField {
                    name: "a".to_string(),
                    ty: "u8".to_string(),
                    estimated_size: Some(1),
                    estimated_alignment: Some(1),
                    atomic: false,
                    access_locations: Vec::new(),
                    thread_owners: Vec::new(),
                    hot: true,
                    offset: None,
                },
                LayoutField {
                    name: "b".to_string(),
                    ty: "u64".to_string(),
                    estimated_size: Some(8),
                    estimated_alignment: Some(8),
                    atomic: false,
                    access_locations: Vec::new(),
                    thread_owners: Vec::new(),
                    hot: false,
                    offset: None,
                },
            ],
            static_confidence: 0.5,
            dynamic_observed: false,
        };
        assert_eq!(
            best_field_permutation(&model, &HashSet::new()),
            vec!["a", "b"]
        );
    }

    #[test]
    fn field_reorder_materializes_as_a_hash_bound_edit() {
        let source = "struct S { a: u8, b: AtomicUsize }";
        let model = extract_layout(source, Some("S")).unwrap().pop().unwrap();
        let finding = Finding::new(
            "f",
            FindingKind::PoorFieldLocality,
            crate::assurance::Severity::High,
            "x.rs",
            1,
            Some("S".to_string()),
        );
        let candidate = generate_candidates(&model, &[finding], &LayoutConfig::default())
            .into_iter()
            .find(|candidate| matches!(candidate.kind, LayoutCandidateKind::FieldReorder))
            .unwrap();
        let edit = materialize_candidate(source, Path::new("x.rs"), &candidate, 64)
            .unwrap()
            .unwrap();
        let updated = crate::repair::apply_edits_safely(source, &[edit]).unwrap();
        assert!(syn::parse_file(&updated).is_ok());
        assert!(updated.find("b : AtomicUsize").unwrap() < updated.find("a : u8").unwrap());
    }
}
