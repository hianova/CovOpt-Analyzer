use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Deserialize, Debug, Clone)]
pub struct TargetConfig {
    pub test: String,
    pub expected: String,
    pub n_values: String,
    pub target_file: String,
    pub target_line: u64,
    pub mca_cpu: Option<String>,
    pub require_cache_padding: Option<bool>,
    pub require_branch_hints: Option<bool>,
    pub require_aerospace_grade: Option<bool>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct CovOptConfig {
    #[serde(default)]
    pub target: Vec<TargetConfig>,
}

impl CovOptConfig {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read config file {}: {}", path.as_ref().display(), e))?;
        toml::from_str(&content)
            .map_err(|e| format!("Failed to parse {}: {}", path.as_ref().display(), e))
    }
}
