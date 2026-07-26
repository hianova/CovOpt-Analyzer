use proc_macro2::LineColumn;
use std::fs;
use std::path::{Path, PathBuf};
use syn::visit::{self, Visit};
use syn::{ExprLit, Lit};

pub struct MagicNumberScanner {
    pub file_path: String,
    pub found_magics: Vec<(LineColumn, LineColumn, String)>,
}

impl<'ast> Visit<'ast> for MagicNumberScanner {
    fn visit_expr_lit(&mut self, node: &'ast ExprLit) {
        match &node.lit {
            Lit::Int(lit_int) => {
                let value_str = lit_int.base10_digits();
                if let Ok(val) = value_str.parse::<i64>() {
                    // Ignore common safe numbers
                    if val != 0 && val != 1 && val != 2 && val != -1 {
                        self.found_magics.push((
                            node.lit.span().start(),
                            node.lit.span().end(),
                            value_str.to_string(),
                        ));
                    }
                }
            }
            Lit::Float(lit_float) => {
                let value_str = lit_float.base10_digits();
                if let Ok(val) = value_str.parse::<f64>() {
                    // Ignore 0.0, 1.0, etc.
                    if (val.abs() - 0.0).abs() > f64::EPSILON
                        && (val.abs() - 1.0).abs() > f64::EPSILON
                    {
                        self.found_magics.push((
                            node.lit.span().start(),
                            node.lit.span().end(),
                            value_str.to_string(),
                        ));
                    }
                }
            }
            _ => {}
        }
        // Delegate to the default impl to visit any nested expressions (though literals don't have them)
        visit::visit_expr_lit(self, node);
    }

    fn visit_generic_argument(&mut self, _node: &'ast syn::GenericArgument) {
        // Skip scanning magic numbers in const generics (e.g. Arena<K, V, 128>)
    }

    fn visit_type_array(&mut self, node: &'ast syn::TypeArray) {
        // Skip the length part of [T; N]
        visit::visit_type(self, &node.elem);
    }

    fn visit_expr_repeat(&mut self, node: &'ast syn::ExprRepeat) {
        // Skip the length part of [expr; N]
        visit::visit_expr(self, &node.expr);
    }

    fn visit_item_const(&mut self, _node: &'ast syn::ItemConst) {
        // Skip global const declarations
    }

    fn visit_item_static(&mut self, _node: &'ast syn::ItemStatic) {
        // Skip static variable declarations (const context)
    }

    fn visit_impl_item_const(&mut self, _node: &'ast syn::ImplItemConst) {
        // Skip impl const items (const context)
    }

    fn visit_trait_item_const(&mut self, _node: &'ast syn::TraitItemConst) {
        // Skip trait const items (const context)
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if node.sig.constness.is_some() {
            // Skip const fn (const context)
            return;
        }
        visit::visit_item_fn(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if node.sig.constness.is_some() {
            // Skip const impl fn (const context)
            return;
        }
        visit::visit_impl_item_fn(self, node);
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        if node.sig.constness.is_some() {
            // Skip const trait fn (const context)
            return;
        }
        visit::visit_trait_item_fn(self, node);
    }

    fn visit_variant(&mut self, _node: &'ast syn::Variant) {
        // Skip enum discriminants (const context)
    }

    fn visit_pat(&mut self, _node: &'ast syn::Pat) {
        // Skip pattern matching arms (const context)
    }

    fn visit_expr_const(&mut self, _node: &'ast syn::ExprConst) {
        // Skip inline const blocks (const context)
    }

    fn visit_attribute(&mut self, _node: &'ast syn::Attribute) {
        // Skip attributes (const context)
    }
}


pub fn run_scan(path: Option<String>, auto_fix: bool, restore: bool) {
    let start_dir = path.unwrap_or_else(|| ".".to_string());
    
    if restore {
        let backup_dir = Path::new(".covopt_backup");
        if backup_dir.exists() {
            println!("Restoring files from .covopt_backup/...");
            let mut restored = 0;
            
            fn restore_recursive(current_dir: &Path, base_backup: &Path, base_target: &Path, count: &mut usize) {
                if let Ok(entries) = fs::read_dir(current_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            restore_recursive(&path, base_backup, base_target, count);
                        } else if path.is_file()
                            && let Ok(relative) = path.strip_prefix(base_backup) {
                                let target = base_target.join(relative);
                                if let Some(parent) = target.parent() {
                                    let _ = fs::create_dir_all(parent);
                                }
                                if fs::copy(&path, &target).is_ok() {
                                    println!("Restored: {}", target.display());
                                    *count += 1;
                                }
                            }
                    }
                }
            }
            
            restore_recursive(backup_dir, backup_dir, Path::new(&start_dir), &mut restored);
            let _ = fs::remove_dir_all(backup_dir);
            println!("✅ Successfully restored {} files.", restored);
        } else {
            println!("No .covopt_backup/ directory found. Nothing to restore.");
        }
        return;
    }

    let mut files_to_scan = Vec::new();
    collect_rs_files(Path::new(&start_dir), &mut files_to_scan);

    println!("Scanning {} for magic numbers...", start_dir);
    let mut total_found = 0;
    let mut total_fixed = 0;

    let config = crate::config::CovOptConfig::load(".covopt.toml").ok();
    let macro_path = config.and_then(|c| c.macro_path).unwrap_or_else(|| "covopt_macro::covopt_param".to_string());

    let mut crate_no_std_cache: std::collections::HashMap<std::path::PathBuf, bool> = std::collections::HashMap::new();

    for file_path in files_to_scan {
        if let Ok(content) = fs::read_to_string(&file_path) {
            let mut is_no_std = content.contains("#![no_std]");
            
            if !is_no_std {
                let mut current = file_path.parent();
                while let Some(dir) = current {
                    let cargo_toml = dir.join("Cargo.toml");
                    if cargo_toml.exists() {
                        if let Some(&cached_is_no_std) = crate_no_std_cache.get(dir) {
                            is_no_std = cached_is_no_std;
                        } else {
                            let lib_rs = dir.join("src").join("lib.rs");
                            let main_rs = dir.join("src").join("main.rs");
                            let mut crate_has_no_std = false;
                            
                            // Check if it's a proc-macro crate (which we should also skip)
                            if let Ok(toml_content) = fs::read_to_string(&cargo_toml) {
                                if toml_content.contains("proc-macro = true") {
                                    crate_has_no_std = true;
                                }
                            }

                            if !crate_has_no_std {
                                for root_file in &[lib_rs, main_rs] {
                                    if let Ok(root_content) = fs::read_to_string(root_file) {
                                        if root_content.contains("#![no_std]") {
                                            crate_has_no_std = true;
                                            break;
                                        }
                                    }
                                }
                            }
                            crate_no_std_cache.insert(dir.to_path_buf(), crate_has_no_std);
                            is_no_std = crate_has_no_std;
                        }
                        break; // Stop going up once we hit a Cargo.toml (the closest crate root)
                    }
                    current = dir.parent();
                }
            }

            if is_no_std && macro_path == "covopt_macro::covopt_param" {
                println!("  [Skip] {} (no_std detected in crate root, requires custom macro_path)", file_path.display());
                continue;
            }
            if let Ok(syntax_tree) = syn::parse_file(&content) {
            let mut scanner = MagicNumberScanner {
                file_path: file_path.to_string_lossy().to_string(),
                found_magics: Vec::new(),
            };
            scanner.visit_file(&syntax_tree);

            if !scanner.found_magics.is_empty() {
                println!("\n[{}]", scanner.file_path);

                // Sort by line, then column, descending, to safely rewrite
                scanner.found_magics.sort_by_key(|b| std::cmp::Reverse(b.0));

                let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
                let mut file_changed = false;
                let mut abort_scan = false;

                for (start_loc, end_loc, val) in scanner.found_magics {
                    if abort_scan { break; }
                    let line_idx = start_loc.line - 1;
                    if auto_fix && line_idx < lines.len() && start_loc.line == end_loc.line {
                        let line_str = &mut lines[line_idx];
                        let start_col = start_loc.column;
                        let end_col = end_loc.column;

                        let replacement = format!(
                            "{}!(\"M_{}_{}\", {})",
                            macro_path, start_loc.line, start_loc.column, val
                        );

                        if start_col <= end_col && end_col <= line_str.len() {
                            let old_line = line_str.clone();
                            let mut new_line = line_str.clone();
                            new_line.replace_range(start_col..end_col, &replacement);
                            
                            println!("  Line {}: Found magic number `{}`", start_loc.line, val);
                            println!("- {}", old_line.trim_start());
                            println!("+ {}", new_line.trim_start());
                            
                            use std::io::IsTerminal;
                            let is_non_interactive = !std::io::stdout().is_terminal()
                                || std::env::var("COVOPT_NON_INTERACTIVE").is_ok()
                                || std::env::var("CI").is_ok();
                            
                            let mut apply = is_non_interactive;
                            if !is_non_interactive {
                                loop {
                                    use std::io::{self, Write};
                                    print!("Apply this fix? [y]es / [n]o / [q]uit: ");
                                    let _ = io::stdout().flush();
                                    let mut input = String::new();
                                    let _ = io::stdin().read_line(&mut input);
                                    match input.trim().to_lowercase().as_str() {
                                        "y" | "yes" => { apply = true; break; }
                                        "n" | "no" => { apply = false; break; }
                                        "q" | "quit" => { abort_scan = true; break; }
                                        _ => println!("Invalid input."),
                                    }
                                }
                            }
                            
                            if apply {
                                *line_str = new_line;
                                file_changed = true;
                                total_fixed += 1;
                                println!("    -> Fixed.");
                            } else {
                                println!("    -> Skipped.");
                            }
                        } else {
                            println!(
                                "  Line {}: Found magic number `{}` (auto-fix failed due to offset mismatch)",
                                start_loc.line, val
                            );
                        }
                    } else {
                        println!("  Line {}: Found magic number `{}`", start_loc.line, val);
                    }
                    total_found += 1;
                }
                
                if abort_scan {
                    println!("Aborting scan as requested.");
                }

                if file_changed {
                    // Backup the original file before modifying
                    let backup_base = Path::new(".covopt_backup");
                    let file_path_obj = Path::new(&file_path);
                    let backup_path = if let Ok(relative) = file_path_obj.strip_prefix(Path::new(&start_dir)) {
                        backup_base.join(relative)
                    } else {
                        backup_base.join(file_path_obj.file_name().unwrap())
                    };
                    
                    if let Some(parent) = backup_path.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    if !backup_path.exists() {
                        let _ = fs::copy(&file_path, &backup_path);
                    }


                    if let Err(e) = fs::write(&file_path, lines.join("\n") + "\n") {
                        eprintln!("Failed to write {}: {}", file_path.display(), e);
                    }
                }
                
                if abort_scan {
                    break;
                }
            }
            }
        }
    }

    if total_found > 0 {
        if auto_fix {
            println!(
                "\n[!] Found {} magic numbers, successfully fixed {}.",
                total_found, total_fixed
            );
        } else {
            println!(
                "\n[!] Found {} magic numbers. Consider wrapping them with `covopt_param!(\"name\", value)` or run with `--auto-fix`.",
                total_found
            );
            std::process::exit(1);
        }
    } else {
        println!("\n[OK] No magic numbers found! The codebase is highly tunable.");
    }
}

pub fn collect_rs_files(dir: &Path, files: &mut Vec<PathBuf>) {
    if dir.is_dir()
        && let Ok(entries) = fs::read_dir(dir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Ignore common non-source directories and proc-macro crates
                let file_name = path.file_name().unwrap_or_default().to_string_lossy();
                let is_proc_macro_dir = file_name == "covopt-macro"
                    || file_name == "covopt_macro"
                    || file_name.contains("proc-macro")
                    || file_name.contains("proc_macro");

                if file_name != "target"
                    && file_name != ".git"
                    && file_name != ".agents"
                    && !is_proc_macro_dir
                    && !file_name.starts_with('.')
                {
                    collect_rs_files(&path, files);
                }
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                let is_in_proc_macro = path.components().any(|c| {
                    let s = c.as_os_str().to_string_lossy();
                    s == "covopt-macro" || s == "covopt_macro" || s.contains("proc-macro") || s.contains("proc_macro")
                });
                if !is_in_proc_macro {
                    files.push(path);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_number_scanner_skips_const_contexts() {
        let code = r#"
            static FOO: i32 = 42;
            const BAR: i32 = 100;

            const fn add_const(x: i32) -> i32 {
                x + 50
            }

            enum Status {
                Active = 10,
                Inactive = 20,
            }

            fn regular_fn(x: i32) -> i32 {
                let y = 999;
                match x {
                    123 => y + 888,
                    _ => 0,
                }
            }
        "#;

        let syntax_tree = syn::parse_file(code).expect("failed to parse test code");
        let mut scanner = MagicNumberScanner {
            file_path: "test.rs".to_string(),
            found_magics: Vec::new(),
        };
        scanner.visit_file(&syntax_tree);

        let found_vals: Vec<&str> = scanner.found_magics.iter().map(|(_, _, val)| val.as_str()).collect();

        // 42 (static), 100 (const), 50 (const fn), 10 & 20 (enum discriminants), 123 (pat arm) must NOT be scanned.
        assert!(!found_vals.contains(&"42"), "Static item magic number 42 should be skipped");
        assert!(!found_vals.contains(&"100"), "Const item magic number 100 should be skipped");
        assert!(!found_vals.contains(&"50"), "Const fn magic number 50 should be skipped");
        assert!(!found_vals.contains(&"10"), "Enum discriminant 10 should be skipped");
        assert!(!found_vals.contains(&"20"), "Enum discriminant 20 should be skipped");
        assert!(!found_vals.contains(&"123"), "Pattern arm magic number 123 should be skipped");

        // 999 and 888 in regular function body SHOULD be found
        assert!(found_vals.contains(&"999"), "Regular fn body magic number 999 should be found");
        assert!(found_vals.contains(&"888"), "Regular fn body magic number 888 should be found");
    }


}


