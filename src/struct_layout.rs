use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use syn::visit_mut::VisitMut;
use syn::{ItemStruct, parse_quote};

pub struct StructLayoutTuner {
    pub target_dir: String,
}

struct StructMutator {
    pub mutations_made: usize,
}

impl VisitMut for StructMutator {
    fn visit_item_struct_mut(&mut self, node: &mut ItemStruct) {
        let mut has_repr = false;
        for attr in &node.attrs {
            if attr.path().is_ident("repr") {
                has_repr = true;
                break;
            }
        }

        if !has_repr && matches!(node.vis, syn::Visibility::Public(_)) {
            node.attrs.push(parse_quote!(#[repr(C, align(64))]));
            self.mutations_made += 1;
        }

        syn::visit_mut::visit_item_struct_mut(self, node);
    }
}

impl StructLayoutTuner {
    pub fn new(target_dir: &str) -> Self {
        Self {
            target_dir: target_dir.to_string(),
        }
    }

    pub fn run(&self) -> Result<()> {
        println!("🚀 Starting CovOpt Struct Layout Auto-Tuner...");

        let mut files_to_scan = Vec::new();
        collect_rust_files(Path::new(&self.target_dir), &mut files_to_scan);

        let mut total_mutations = 0;

        for file_path in files_to_scan {
            let file_path_str = file_path.to_string_lossy().to_string();
            if let Ok(content) = fs::read_to_string(&file_path)
                && let Ok(mut syntax_tree) = syn::parse_file(&content) {
                    let mut mutator = StructMutator { mutations_made: 0 };

                    mutator.visit_file_mut(&mut syntax_tree);

                    if mutator.mutations_made > 0 {
                        let new_content = quote::quote!(#syntax_tree).to_string();
                        fs::write(&file_path, new_content)
                            .context("Failed to write struct layout injected file")?;

                        let _ = std::process::Command::new("cargo")
                            .arg("fmt")
                            .arg("--")
                            .arg(&file_path)
                            .output();

                        println!(
                            "  -> Optimized memory layout for {} structs in {}",
                            mutator.mutations_made, file_path_str
                        );
                        total_mutations += mutator.mutations_made;
                    }
                }
        }

        println!(
            "🏆 Struct Layout Tuning Complete. Total structs optimized: {}",
            total_mutations
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
                    if file_name != "target"
                        && file_name != ".git"
                        && file_name != "fuzz"
                        && !file_name.starts_with('.')
                    {
                        collect_rust_files(&path, files);
                    }
                } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                    files.push(path);
                }
            }
        }
}
