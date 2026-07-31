//! Shared, versioned identifiers and sample dimensions.
//!
//! Every analysis surface uses these types instead of inventing a local ID or
//! reducing an execution sample to only `N`.  The string representation is
//! deliberately source-oriented and does not contain compiler mangled names.

use serde::{Deserialize, Serialize};
use std::fmt;

pub const MODEL_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum AssuranceStatus {
    Proven,
    Modeled,
    Observed,
    Assumed,
    Unknown,
    Failed,
}

impl AssuranceStatus {
    pub fn resolved(self) -> bool {
        matches!(self, Self::Proven | Self::Modeled | Self::Observed)
    }

    pub fn confidence(self) -> f64 {
        match self {
            Self::Proven => 1.0,
            Self::Observed => 0.95,
            Self::Modeled => 0.75,
            Self::Assumed => 0.50,
            Self::Unknown | Self::Failed => 0.0,
        }
    }
}

/// Normalize legacy report envelopes without changing their payload.
///
/// Older reports used `version`; new consumers use `schema_version`.  Keeping
/// both during the transition lets old dashboards read new reports while
/// giving analyzers one explicit migration entry point.
pub fn migrate_legacy_json(mut value: serde_json::Value) -> Result<serde_json::Value, String> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| "versioned model document must be a JSON object".to_string())?;
    if object.get("schema_version").is_none() {
        let version = object
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(1) as u32;
        eprintln!("[CovOpt] migrating legacy metadata schema to schema_version={version}");
        object.insert(
            "schema_version".to_string(),
            serde_json::Value::from(version),
        );
    }
    let schema = object
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "schema_version must be an unsigned integer".to_string())?
        as u32;
    if schema < MODEL_SCHEMA_VERSION {
        eprintln!(
            "[CovOpt] metadata schema {schema} is older than supported schema {MODEL_SCHEMA_VERSION}; migration compatibility is active"
        );
    }
    if schema > MODEL_SCHEMA_VERSION {
        return Err(format!(
            "document schema {} is newer than supported schema {}",
            schema, MODEL_SCHEMA_VERSION
        ));
    }
    Ok(value)
}

macro_rules! stable_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn from_parts(parts: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
                Self(
                    parts
                        .into_iter()
                        .map(|part| part.as_ref().to_string())
                        .collect::<Vec<_>>()
                        .join("::"),
                )
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

stable_id!(PackageId);
stable_id!(FunctionId);
stable_id!(ScopeId);
stable_id!(CallEdgeId);
stable_id!(ObligationId);
stable_id!(EvidenceActionId);
stable_id!(TraceId);
stable_id!(AssumptionId);
stable_id!(SnapshotId);
stable_id!(ProviderId);

impl FunctionId {
    pub fn from_source(
        package: &PackageId,
        source_path: impl AsRef<str>,
        function_path: impl AsRef<str>,
        generic_context: impl AsRef<str>,
    ) -> Self {
        Self::from_parts([
            package.0.as_str(),
            source_path.as_ref(),
            function_path.as_ref(),
            generic_context.as_ref(),
        ])
    }
}

impl ScopeId {
    pub fn from_function(function: &FunctionId, kind: impl AsRef<str>, ordinal: usize) -> Self {
        Self::from_parts([function.0.as_str(), kind.as_ref(), &ordinal.to_string()])
    }
}

impl CallEdgeId {
    pub fn from_source(caller: &FunctionId, callee: &FunctionId, ordinal: usize) -> Self {
        Self::from_parts([caller.0.as_str(), callee.0.as_str(), &ordinal.to_string()])
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SampleKey {
    pub n: Option<usize>,
    pub seed: Option<u64>,
    pub threads: Option<usize>,
    pub queue_capacity: Option<usize>,
    pub cpu: Option<String>,
    pub toolchain: Option<String>,
    pub dependency_set: Option<String>,
}

impl SampleKey {
    pub fn complexity(n: usize, seed: u64) -> Self {
        Self {
            n: Some(n),
            seed: Some(seed),
            ..Self::default()
        }
    }

    pub fn fingerprint(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaVersion {
    pub schema: u32,
}

impl Default for SchemaVersion {
    fn default() -> Self {
        Self {
            schema: MODEL_SCHEMA_VERSION,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_ids_are_stable_and_mangled_name_free() {
        let package = PackageId::new("pkg");
        let id = FunctionId::from_source(&package, "src/lib.rs", "crate::run", "T=u32");
        assert_eq!(id.0, "pkg::src/lib.rs::crate::run::T=u32");
        assert!(!id.0.contains("_ZN"));
    }

    #[test]
    fn sample_key_serializes_all_dimensions_deterministically() {
        let key = SampleKey {
            n: Some(100),
            seed: Some(7),
            threads: Some(4),
            queue_capacity: Some(32),
            cpu: Some("native".to_string()),
            toolchain: Some("stable".to_string()),
            dependency_set: Some("lock-v1".to_string()),
        };
        assert_eq!(
            key,
            serde_json::from_str(&serde_json::to_string(&key).unwrap()).unwrap()
        );
        assert!(key.fingerprint().contains("queue_capacity"));
    }

    #[test]
    fn legacy_json_gets_a_schema_version_without_losing_version() {
        let value = migrate_legacy_json(serde_json::json!({"version": 1, "targets": []})).unwrap();
        assert_eq!(value["version"], 1);
        assert_eq!(value["schema_version"], 1);
    }
}
