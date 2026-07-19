use proc_macro2::LineColumn;
use std::fs;
use std::path::{Path, PathBuf};
use syn::visit::{self, Visit};
use syn::{ExprLit, Lit};

pub struct MagicNumberScanner {
    pub file_path: String,
    pub found_magics: Vec<(LineColumn, String)>,
}

impl<'ast> Visit<'ast> for MagicNumberScanner {
    fn visit_expr_lit(&mut self, node: &'ast ExprLit) {
        match &node.lit {
            Lit::Int(lit_int) => {
                let value_str = lit_int.base10_digits();
                if let Ok(val) = value_str.parse::<i64>() {
                    // Ignore common safe numbers
                    if val != 0 && val != 1 && val != 2 && val != -1 {
                        self.found_magics.push((node.lit.span().start(), value_str.to_string()));
                    }
                }
            }
            Lit::Float(lit_float) => {
                let value_str = lit_float.base10_digits();
                if let Ok(val) = value_str.parse::<f64>() {
                    // Ignore 0.0, 1.0, etc.
                    if (val.abs() - 0.0).abs() > f64::EPSILON && (val.abs() - 1.0).abs() > f64::EPSILON {
                        self.found_magics.push((node.lit.span().start(), value_str.to_string()));
                    }
                }
            }
            _ => {}
        }
        // Delegate to the default impl to visit any nested expressions (though literals don't have them)
        visit::visit_expr_lit(self, node);
    }
}

pub fn run_scan(path: Option<String>) {
    let start_dir = path.unwrap_or_else(|| ".".to_string());
    let mut files_to_scan = Vec::new();
    collect_rs_files(Path::new(&start_dir), &mut files_to_scan);

    println!("Scanning {} for magic numbers...", start_dir);
    let mut total_found = 0;

    for file_path in files_to_scan {
        if let Ok(content) = fs::read_to_string(&file_path)
            && let Ok(syntax_tree) = syn::parse_file(&content)
        {
            let mut scanner = MagicNumberScanner {
                file_path: file_path.to_string_lossy().to_string(),
                found_magics: Vec::new(),
            };
            scanner.visit_file(&syntax_tree);

            if !scanner.found_magics.is_empty() {
                println!("\n[{}]", scanner.file_path);
                for (loc, val) in scanner.found_magics {
                    println!("  Line {}: Found magic number `{}`", loc.line, val);
                    total_found += 1;
                }
            }
        }
    }

    if total_found > 0 {
        println!(
            "\n[!] Found {} magic numbers. Consider wrapping them with `covopt_param!(\"name\", value)`.",
            total_found
        );
        std::process::exit(1);
    } else {
        println!("\n[OK] No magic numbers found! The codebase is highly tunable.");
    }
}

fn collect_rs_files(dir: &Path, files: &mut Vec<PathBuf>) {
    if dir.is_dir()
        && let Ok(entries) = fs::read_dir(dir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Ignore common non-source directories
                let file_name = path.file_name().unwrap_or_default().to_string_lossy();
                if file_name != "target" && file_name != ".git" && file_name != ".agents" && !file_name.starts_with('.') {
                    collect_rs_files(&path, files);
                }
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                files.push(path);
            }
        }
    }
}
