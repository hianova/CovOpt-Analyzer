//! Shared, versioned metadata used by the proc-macros and analyzer.

use serde::{Deserialize, Serialize};
use std::fmt;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ParameterId(pub String);

impl ParameterId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl fmt::Display for ParameterId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ParameterValue {
    Signed(i128),
    Unsigned(u128),
    Float(f64),
    DurationNs(u128),
    Count(u128),
    Bytes(u128),
    Categorical(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParameterRange {
    pub min: ParameterValue,
    pub max: ParameterValue,
    pub inclusive_max: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ParameterDomain {
    Range(ParameterRange),
    Values(Vec<ParameterValue>),
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ParameterClass {
    Threshold,
    Capacity,
    Budget,
    Timeout,
    Retry,
    Tolerance,
    Coefficient,
    Seed,
    Layout,
    Ordering,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvaluationMode {
    Runtime,
    CompileTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ParameterTag {
    Threshold,
    Capacity,
    Budget,
    Timeout,
    Retry,
    Tolerance,
    Coefficient,
    Seed,
    Layout,
    Ordering,
    Custom(String),
}

impl ParameterTag {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "threshold" => Self::Threshold,
            "capacity" => Self::Capacity,
            "budget" => Self::Budget,
            "timeout" => Self::Timeout,
            "retry" => Self::Retry,
            "tolerance" => Self::Tolerance,
            "coefficient" => Self::Coefficient,
            "seed" => Self::Seed,
            "layout" => Self::Layout,
            "ordering" => Self::Ordering,
            value if !value.is_empty() => Self::Custom(value.to_string()),
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SourceAnchor {
    pub file: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParameterDescriptor {
    pub schema_version: u32,
    pub id: ParameterId,
    pub default: ParameterValue,
    pub domain: ParameterDomain,
    pub class: ParameterClass,
    pub evaluation: EvaluationMode,
    pub tags: Vec<ParameterTag>,
    pub source: SourceAnchor,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub inferred: bool,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub inference_source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ParameterPhase {
    Discovered,
    DomainReady { domain: ParameterDomain },
    Exploring { seed: u64, candidates: usize },
    Evaluated { observations: usize },
    Confirmed { candidate_hash: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ParameterProperty {
    Sensitive { score: f64 },
    Coupled { group: String, strength: f64 },
    Constrained { reason: String },
    Monotonic { direction: String },
    Unstable { score: f64 },
    EnvelopeLimited { bound: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ParameterDisposition {
    Unknown,
    KeepTunable,
    UpdateDefault {
        value: ParameterValue,
        candidate_hash: String,
    },
    InvariantCandidate {
        reason: String,
    },
    Blocked {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CouplingGroupId(pub String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParameterRecord {
    pub descriptor: ParameterDescriptor,
    pub phase: ParameterPhase,
    #[serde(default)]
    pub properties: Vec<ParameterProperty>,
    pub disposition: ParameterDisposition,
    #[serde(default)]
    pub coupling_group: Option<CouplingGroupId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetadataEnvelope<T> {
    pub schema_version: u32,
    pub value: T,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TargetId(pub String);

impl TargetId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl fmt::Display for TargetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetDescriptor {
    pub schema_version: u32,
    pub id: TargetId,
    pub function: String,
    #[serde(default)]
    pub complexity: Option<String>,
    #[serde(default)]
    pub criticality: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceDescriptor {
    pub schema_version: u32,
    pub target: TargetId,
    #[serde(default)]
    pub n_values: Vec<ParameterValue>,
    #[serde(default)]
    pub seeds: Vec<u64>,
    #[serde(default)]
    pub threads: Vec<usize>,
    #[serde(default)]
    pub environment: Vec<String>,
    #[serde(default)]
    pub axes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AtomicContractDescriptor {
    pub schema_version: u32,
    pub target: TargetId,
    #[serde(default)]
    pub ordering: Option<String>,
    #[serde(default)]
    pub liveness: Option<String>,
    #[serde(default)]
    pub forbidden_outcomes: Vec<String>,
    #[serde(default)]
    pub bounds: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameter_records_round_trip_and_sort_by_stable_id() {
        let mut ids = [ParameterId::new("z"), ParameterId::new("a")];
        ids.sort();
        assert_eq!(ids[0].0, "a");
        let record = ParameterRecord {
            descriptor: ParameterDescriptor {
                schema_version: SCHEMA_VERSION,
                id: ids[0].clone(),
                default: ParameterValue::Unsigned(64),
                domain: ParameterDomain::Unknown,
                class: ParameterClass::Capacity,
                evaluation: EvaluationMode::Runtime,
                tags: vec![ParameterTag::Capacity],
                source: SourceAnchor {
                    file: "x.rs".into(),
                    line: 1,
                    column: 1,
                },
                unit: Some("bytes".into()),
                inferred: false,
                confidence: None,
                inference_source: None,
            },
            phase: ParameterPhase::Discovered,
            properties: Vec::new(),
            disposition: ParameterDisposition::Unknown,
            coupling_group: None,
        };
        assert_eq!(
            record,
            serde_json::from_str(&serde_json::to_string(&record).unwrap()).unwrap()
        );
    }
}
