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

                // Sort by line, then column, descending, to safely rewrite from end to start of line
                scanner.found_magics.sort_by(|a, b| b.0.cmp(&a.0));

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
                            "covopt_param!(\"M_{}_{}\", {})",
                            start_loc.line, start_loc.column, val
                        );

                        if start_col <= end_col && end_col <= line_str.len() {
                            let old_line = line_str.clone();
                            let mut new_line = line_str.clone();
                            new_line.replace_range(start_col..end_col, &replacement);
                            
                            println!("  Line {}: Found magic number `{}`", start_loc.line, val);
                            println!("- {}", old_line.trim_start());
                            println!("+ {}", new_line.trim_start());
                            
                            let mut apply = false;
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

                    // Add `use covopt_macro::covopt_param;` at the top if not present
                    if !lines
                        .iter()
                        .any(|l| l.contains("use covopt_macro::covopt_param;"))
                    {
                        lines.insert(0, "use covopt_macro::covopt_param;".to_string());
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
                // Ignore common non-source directories
                let file_name = path.file_name().unwrap_or_default().to_string_lossy();
                if file_name != "target"
                    && file_name != ".git"
                    && file_name != ".agents"
                    && !file_name.starts_with('.')
                {
                    collect_rs_files(&path, files);
                }
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                files.push(path);
            }
        }
    }
}
