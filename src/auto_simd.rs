use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};
use syn::visit::Visit;
use syn::{ExprForLoop, ItemFn};

pub struct AutoSimd {
    pub target_dir: String,
}

struct SimdScanner {
    pub file_path: String,
    pub function_name: String,
    pub potential_loops: usize,
}

impl<'ast> Visit<'ast> for SimdScanner {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        self.function_name = node.sig.ident.to_string();
        syn::visit::visit_item_fn(self, node);
    }

    fn visit_expr_for_loop(&mut self, node: &'ast ExprForLoop) {
        let body_str = quote::quote!(#node).to_string();
        if body_str.contains("[") && body_str.contains("]") && body_str.contains("+=") {
            println!(
                "  [SIMD Opportunity] Found vectorizable math loop in {}::{}",
                self.file_path, self.function_name
            );
            self.potential_loops += 1;
        }

        syn::visit::visit_expr_for_loop(self, node);
    }
}

impl AutoSimd {
    pub fn new(target_dir: &str) -> Self {
        Self {
            target_dir: target_dir.to_string(),
        }
    }

    pub fn run(&self) -> Result<()> {
        println!("🚀 Starting CovOpt Auto-Vectorization (SIMD) Scanner...");

        let mut files_to_scan = Vec::new();
        collect_rust_files(Path::new(&self.target_dir), &mut files_to_scan);

        let mut total_opportunities = 0;

        for file_path in files_to_scan {
            let file_path_str = file_path.to_string_lossy().to_string();
            if let Ok(content) = fs::read_to_string(&file_path)
                && let Ok(syntax_tree) = syn::parse_file(&content)
            {
                let mut scanner = SimdScanner {
                    file_path: file_path_str.clone(),
                    function_name: String::new(),
                    potential_loops: 0,
                };

                scanner.visit_file(&syntax_tree);
                total_opportunities += scanner.potential_loops;
            }
        }

        println!(
            "🏆 SIMD Scan Complete. Found {} opportunities for Auto-Vectorization.",
            total_opportunities
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
