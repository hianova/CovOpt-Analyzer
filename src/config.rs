use serde::Deserialize;
use std::fs;
use std::path::Path;

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

#[derive(Deserialize, Debug, Clone)]
pub struct PipelineConfig {
    #[serde(default = "default_true")]
    pub run_fix: bool,
    #[serde(default = "default_true")]
    pub run_audit: bool,
    #[serde(default = "default_false")]
    pub run_optimize: bool,
    #[serde(default = "default_false")]
    pub run_harden: bool,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            run_fix: true,
            run_audit: true,
            run_optimize: false,
            run_harden: false,
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct TargetConfig {
    pub test: String,
    pub tests: Option<String>,
    pub expected: Option<String>,
    pub n_values: Option<String>,
    pub fuzz_iterations: Option<u32>,
    pub mca_cpu: Option<String>,
    #[serde(default = "default_true")]
    pub require_cache_padding: bool,
    #[serde(default = "default_true")]
    pub require_branch_hints: bool,
    #[serde(default = "default_true")]
    pub require_aerospace_grade: bool,
    #[serde(default = "default_true")]
    pub require_watchdog_timeout: bool,
    #[serde(default = "default_true")]
    pub require_stress_test: bool,
    pub polling_threshold: Option<u64>,
}

impl TargetConfig {
    // Deprecated methods removed.
}

#[derive(Deserialize, Debug, Clone)]
pub struct CovOptConfig {
    #[serde(default)]
    pub target: Vec<TargetConfig>,
    #[serde(default)]
    pub pipeline: PipelineConfig,
}

impl CovOptConfig {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let content = fs::read_to_string(&path).map_err(|e| {
            format!(
                "Failed to read config file {}: {}",
                path.as_ref().display(),
                e
            )
        })?;
        toml::from_str(&content)
            .map_err(|e| format!("Failed to parse {}: {}", path.as_ref().display(), e))
    }
}
