use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Deserialize, Debug, Clone)]
pub struct TargetConfig {
    pub test: String,
    pub tests: Option<String>,
    pub expected: String,
    pub n_values: String,
    pub target_file: String,
    #[serde(default)]
    pub target_line: Option<u64>,
    #[serde(default)]
    pub target_marker: Option<String>,
    pub fuzz_iterations: Option<u32>,
    pub mca_cpu: Option<String>,
    pub require_cache_padding: Option<bool>,
    pub require_branch_hints: Option<bool>,
    pub require_aerospace_grade: Option<bool>,
    pub require_watchdog_timeout: Option<bool>,
    pub require_stress_test: Option<bool>,
    pub polling_threshold: Option<u64>,
}

impl TargetConfig {
    pub fn resolve_target_line(&self) -> u64 {
        if let Some(marker) = &self.target_marker {
            let file_content = fs::read_to_string(&self.target_file).unwrap_or_else(|_| panic!("Failed to read {}", self.target_file));
            for (i, line) in file_content.lines().enumerate() {
                if line.contains(marker) {
                    return (i + 1) as u64;
                }
            }
            panic!("CovOpt-Analyzer: target_marker '{}' not found in {}", marker, self.target_file);
        } else if let Some(line) = self.target_line {
            return line;
        } else {
            panic!("CovOpt-Analyzer: either target_line or target_marker must be provided in config");
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct CovOptConfig {
    #[serde(default)]
    pub target: Vec<TargetConfig>,
    pub agent_deterrence: Option<bool>,
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
