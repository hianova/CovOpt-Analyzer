use serde::Deserialize;
use std::fs;

#[derive(Deserialize, Debug, Clone)]
pub struct ExternalPlugin {
    pub crate_name: String,
    pub features: Option<Vec<String>>,
    pub genes: Vec<String>,
}

#[derive(Deserialize, Debug)]
pub struct PluginConfig {
    pub plugins: Option<PluginsSection>,
}

#[derive(Deserialize, Debug)]
pub struct PluginsSection {
    pub external: Option<Vec<ExternalPlugin>>,
}

pub struct PluginRegistry;

impl PluginRegistry {
    /// Loads the external plugin configurations from `.covopt.toml` if it exists.
    pub fn load_from_toml(path: &str) -> Vec<ExternalPlugin> {
        if let Ok(content) = fs::read_to_string(path) {
            if let Ok(config) = toml::from_str::<PluginConfig>(&content) {
                if let Some(plugins_section) = config.plugins {
                    if let Some(external) = plugins_section.external {
                        return external;
                    }
                }
            }
        }
        vec![]
    }
}
