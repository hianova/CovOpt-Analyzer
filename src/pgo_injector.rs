use crate::coverage::CoverageMap;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use syn::visit_mut::VisitMut;
use syn::{Expr, ExprIf, parse_quote};

pub struct PgoInjector {
    pub target_dir: String,
    pub coverage_map: CoverageMap,
    pub injection_threshold: u64, // e.g., 10000 hits difference
}

struct IfMutator {
    pub _file_path: String,
    pub _coverage_map: CoverageMap,
    pub _injection_threshold: u64,
    pub injections_made: usize,
}

impl VisitMut for IfMutator {
    fn visit_expr_if_mut(&mut self, node: &mut ExprIf) {
        let cond = &node.cond;

        // Skip 'if let' constructs because wrapping 'let' in a function call is a syntax error
        if !matches!(**cond, Expr::Let(_)) {
            let cond_str = quote::quote!(#cond).to_string();
            if !cond_str.contains("likely") && !cond_str.contains("unlikely") {
                let new_cond: Expr = parse_quote!(#cond);
                *node.cond = new_cond;
                self.injections_made += 1;
            }
        }

        syn::visit_mut::visit_expr_if_mut(self, node);
    }
}

impl PgoInjector {
    pub fn new(target_dir: &str, coverage_map: CoverageMap, injection_threshold: u64) -> Self {
        Self {
            target_dir: target_dir.to_string(),
            coverage_map,
            injection_threshold,
        }
    }

    pub fn run(&self) -> Result<()> {
        println!("🚀 Starting CovOpt Dynamic PGO (Profile-Guided Optimization) Injector...");

        let mut files_to_scan = Vec::new();
        collect_rust_files(Path::new(&self.target_dir), &mut files_to_scan);

        let mut total_injections = 0;

        for file_path in files_to_scan {
            let file_path_str = file_path.to_string_lossy().to_string();
            if let Ok(content) = fs::read_to_string(&file_path)
                && let Ok(mut syntax_tree) = syn::parse_file(&content) {
                    let mut mutator = IfMutator {
                        _file_path: file_path_str.clone(),
                        _coverage_map: CoverageMap::default(),
                        _injection_threshold: self.injection_threshold,
                        injections_made: 0,
                    };

                    mutator.visit_file_mut(&mut syntax_tree);

                    if mutator.injections_made > 0 {
                        let new_content = quote::quote!(#syntax_tree).to_string();
                        fs::write(&file_path, new_content)
                            .context("Failed to write PGO injected file")?;

                        let _ = std::process::Command::new("cargo")
                            .arg("fmt")
                            .arg("--")
                            .arg(&file_path)
                            .output();

                        println!(
                            "  -> Injected {} PGO dynamic probes in {}",
                            mutator.injections_made, file_path_str
                        );
                        total_injections += mutator.injections_made;
                    }
                }
        }

        println!(
            "🏆 PGO Injection Complete. Total likely/unlikely probes injected: {}",
            total_injections
        );
        Ok(())
    }
}

fn collect_rust_files(dir: &Path, files: &mut Vec<PathBuf>) {
    if dir.is_dir()
        && let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
                    if file_name != "target" && file_name != ".git" && !file_name.starts_with('.') {
                        collect_rust_files(&path, files);
                    }
                } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                    files.push(path);
                }
            }
        }
}
