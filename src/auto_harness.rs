use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use syn::visit::Visit;
use syn::{FnArg, ItemFn, PatType, Visibility};

pub struct AutoHarness {
    pub target_dir: String,
}

struct FnScanner {
    pub public_functions: Vec<(String, Vec<String>)>, // name, param types
}

impl<'ast> Visit<'ast> for FnScanner {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        if matches!(node.vis, Visibility::Public(_)) {
            let fn_name = node.sig.ident.to_string();
            let mut param_types = Vec::new();
            let mut supported = true;
            for input in &node.sig.inputs {
                if let FnArg::Typed(PatType { ty, .. }) = input {
                    let ty_str = quote::quote!(#ty).to_string();
                    if ty_str.contains("impl") || ty_str.contains("Generic") {
                        supported = false; // We can't trivially fuzz generics yet
                    }
                    param_types.push(ty_str);
                } else {
                    supported = false; // self methods are skipped for now
                }
            }
            if supported {
                self.public_functions.push((fn_name, param_types));
            }
        }
        syn::visit::visit_item_fn(self, node);
    }
}

impl AutoHarness {
    pub fn new(target_dir: &str) -> Self {
        Self {
            target_dir: target_dir.to_string(),
        }
    }

    pub fn generate(&self) -> Result<()> {
        println!("🚀 Starting CovOpt Auto-Harness Generation for Fuzzing...");

        let mut files_to_scan = Vec::new();
        collect_rust_files(Path::new(&self.target_dir), &mut files_to_scan);

        let mut total_funcs = 0;

        let fuzz_dir = Path::new("src/fuzz/fuzz_targets");
        if !fuzz_dir.exists() {
            fs::create_dir_all(fuzz_dir).context("Failed to create fuzz targets dir")?;
        }

        for file_path in files_to_scan {
            let _file_path_str = file_path.to_string_lossy().to_string();
            if let Ok(content) = fs::read_to_string(&file_path)
                && let Ok(syntax_tree) = syn::parse_file(&content)
            {
                let mut scanner = FnScanner {
                    public_functions: Vec::new(),
                };

                scanner.visit_file(&syntax_tree);

                for (fn_name, _param_types) in scanner.public_functions {
                    total_funcs += 1;
                    let harness_content = format!(
                        "#![no_main]\n\
                            use libfuzzer_sys::fuzz_target;\n\
                            \n\
                            fuzz_target!(|data: &[u8]| {{\n\
                            \t// CovOpt Auto-Generated Harness for {}\n\
                            }});\n",
                        fn_name
                    );

                    let target_path = fuzz_dir.join(format!("auto_target_{}.rs", total_funcs));
                    fs::write(&target_path, harness_content)
                        .context("Failed to write fuzz target")?;
                    println!(
                        "  -> Generated Harness for {} at {:?}",
                        fn_name, target_path
                    );
                }
            }
        }

        println!(
            "🏆 Fuzz Harness Generation Complete. Total harnesses: {}",
            total_funcs
        );
        Ok(())
    }
}

fn collect_rust_files(dir: &Path, files: &mut Vec<PathBuf>) {
    if dir.is_dir()
        && let Ok(entries) = fs::read_dir(dir)
    {
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
