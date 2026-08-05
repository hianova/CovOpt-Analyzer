use CovOpt_Analyzer::science::plugins::registry::PluginRegistry;
use std::fs;

#[test]
fn test_plugin_registry_load() {
    let toml_content = r#"
    [plugins]
    [[plugins.external]]
    crate_name = "rayon"
    features = ["ThreadPool"]
    genes = ["rayon::ThreadPool"]

    [[plugins.external]]
    crate_name = "tokio"
    genes = ["tokio::sync::Mutex"]
    "#;
    
    fs::write("test_covopt.toml", toml_content).unwrap();
    
    let plugins = PluginRegistry::load_from_toml("test_covopt.toml");
    
    assert_eq!(plugins.len(), 2);
    assert_eq!(plugins[0].crate_name, "rayon");
    assert_eq!(plugins[0].genes[0], "rayon::ThreadPool");
    assert_eq!(plugins[1].crate_name, "tokio");
    
    fs::remove_file("test_covopt.toml").unwrap();
}
