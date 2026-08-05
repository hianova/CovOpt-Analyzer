use covopt_schema::{
    EvaluationMode, ParameterClass, ParameterDescriptor, ParameterDomain, ParameterId,
    ParameterRange, ParameterTag, ParameterValue, SourceAnchor,
};
use proc_macro2::LineColumn;
use quote::ToTokens;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use syn::parse::Parser;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{ExprLit, Lit};

#[derive(Debug, Clone)]
pub struct MagicNumber {
    pub start: LineColumn,
    pub end: LineColumn,
    pub value: String,
    pub descriptor: ParameterDescriptor,
    pub scope: String,
}

pub struct MagicNumberScanner {
    pub file_path: String,
    pub found_magics: Vec<MagicNumber>,
    evaluation: EvaluationMode,
    class_override: Option<ParameterClass>,
    scope: Vec<String>,
    semantic_occurrences: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScannerRepairEdit {
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub parameter_id: ParameterId,
    pub default: ParameterValue,
    pub class: ParameterClass,
    pub domain: ParameterDomain,
    pub affected_scopes: Vec<String>,
    pub associated_obligations: Vec<String>,
    pub replacement: String,
    pub evaluation: EvaluationMode,
    pub requires_evidence: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScannerRepairPlan {
    pub schema_version: u32,
    pub source_hashes: BTreeMap<String, String>,
    pub edits: Vec<ScannerRepairEdit>,
    pub applied: bool,
}

#[derive(Debug, Clone, Serialize)]
struct ScannerRollbackEntry {
    source: String,
    backup: String,
    original_hash: String,
}

impl MagicNumberScanner {
    fn record(&mut self, node: &ExprLit, value: String) {
        let scope = self
            .scope
            .last()
            .cloned()
            .unwrap_or_else(|| "module".to_string());
        let class = self
            .class_override
            .unwrap_or_else(|| infer_class(&scope, &value));
        let role = format!(
            "{}::{:?}::{}",
            scope,
            class,
            match self.evaluation {
                EvaluationMode::Runtime => "runtime",
                EvaluationMode::CompileTime => "compile-time",
            }
        );
        let semantic_key = format!("{role}::{value}::{}", node.to_token_stream());
        let occurrence = self
            .semantic_occurrences
            .entry(semantic_key.clone())
            .or_insert(0);
        let occurrence_index = *occurrence;
        *occurrence = occurrence.saturating_add(1);
        let fingerprint_input = format!("{semantic_key}::{occurrence_index}");
        let fingerprint = fingerprint_input
            .bytes()
            .fold(0xcbf29ce484222325u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            });
        let id = ParameterId::new(format!(
            "{}::literal::{fingerprint:016x}",
            scope.replace(['/', '\\'], "::")
        ));
        let descriptor = ParameterDescriptor {
            schema_version: covopt_schema::SCHEMA_VERSION,
            id,
            default: value_to_parameter_value(&value),
            domain: inferred_domain(&value, class),
            class,
            evaluation: self.evaluation,
            tags: vec![class_to_tag(class)],
            source: SourceAnchor {
                file: self.file_path.clone(),
                line: node.span().start().line,
                column: node.span().start().column,
            },
            unit: None,
            inferred: true,
            confidence: Some(0.35),
            inference_source: Some("literal scope/value AST inference".to_string()),
        };
        self.found_magics.push(MagicNumber {
            start: node.lit.span().start(),
            end: node.lit.span().end(),
            value,
            descriptor,
            scope,
        });
    }
}

impl<'ast> Visit<'ast> for MagicNumberScanner {
    fn visit_expr_lit(&mut self, node: &'ast ExprLit) {
        match &node.lit {
            Lit::Int(lit_int) => {
                self.record(node, lit_int.to_token_stream().to_string());
            }
            Lit::Float(lit_float) => {
                self.record(node, lit_float.to_token_stream().to_string());
            }
            _ => {}
        }
    }

    fn visit_generic_argument(&mut self, node: &'ast syn::GenericArgument) {
        if let syn::GenericArgument::Const(value) = node {
            let previous = self.evaluation;
            self.evaluation = EvaluationMode::CompileTime;
            self.visit_expr(value);
            self.evaluation = previous;
        }
    }

    fn visit_type_array(&mut self, node: &'ast syn::TypeArray) {
        let previous = self.evaluation;
        self.evaluation = EvaluationMode::CompileTime;
        self.visit_expr(&node.len);
        self.evaluation = previous;
        visit::visit_type(self, &node.elem);
    }

    fn visit_expr_repeat(&mut self, node: &'ast syn::ExprRepeat) {
        visit::visit_expr(self, &node.expr);
        let previous = self.evaluation;
        self.evaluation = EvaluationMode::CompileTime;
        self.visit_expr(&node.len);
        self.evaluation = previous;
    }

    fn visit_item_const(&mut self, _node: &'ast syn::ItemConst) {
        // Skip global const declarations
    }

    fn visit_item_static(&mut self, node: &'ast syn::ItemStatic) {
        // The value remains an explicit invariant, but layout attributes are
        // compile-time numeric decisions that must still be surfaced.
        for attribute in &node.attrs {
            self.visit_attribute(attribute);
        }
    }

    fn visit_impl_item_const(&mut self, _node: &'ast syn::ImplItemConst) {
        // Skip impl const items (const context)
    }

    fn visit_trait_item_const(&mut self, _node: &'ast syn::TraitItemConst) {
        // Skip trait const items (const context)
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        self.scope.push(node.sig.ident.to_string());
        visit::visit_item_fn(self, node);
        self.scope.pop();
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        self.scope.push(node.sig.ident.to_string());
        visit::visit_impl_item_fn(self, node);
        self.scope.pop();
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        self.scope.push(node.sig.ident.to_string());
        visit::visit_trait_item_fn(self, node);
        self.scope.pop();
    }

    fn visit_pat(&mut self, _node: &'ast syn::Pat) {
        // Skip pattern matching arms (const context)
    }

    fn visit_expr_const(&mut self, node: &'ast syn::ExprConst) {
        let previous = self.evaluation;
        self.evaluation = EvaluationMode::CompileTime;
        self.visit_block(&node.block);
        self.evaluation = previous;
    }

    fn visit_attribute(&mut self, node: &'ast syn::Attribute) {
        if !node.path().is_ident("repr") && !node.path().is_ident("covopt_hoist") {
            return;
        }
        let syn::Meta::List(list) = &node.meta else {
            return;
        };
        let Ok(items) = syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
        else {
            return;
        };
        for item in items {
            match item {
                syn::Meta::List(list)
                    if node.path().is_ident("repr")
                        && (list.path.is_ident("align") || list.path.is_ident("packed")) =>
                {
                    let Ok(expr) = syn::parse2::<syn::Expr>(list.tokens) else {
                        continue;
                    };
                    let previous_evaluation = self.evaluation;
                    let previous_class = self.class_override;
                    self.evaluation = EvaluationMode::CompileTime;
                    self.class_override = Some(ParameterClass::Layout);
                    self.visit_expr(&expr);
                    self.class_override = previous_class;
                    self.evaluation = previous_evaluation;
                }
                syn::Meta::NameValue(value)
                    if node.path().is_ident("covopt_hoist") && value.path.is_ident("capacity") =>
                {
                    let previous_evaluation = self.evaluation;
                    let previous_class = self.class_override;
                    self.evaluation = EvaluationMode::CompileTime;
                    self.class_override = Some(ParameterClass::Capacity);
                    self.visit_expr(&value.value);
                    self.class_override = previous_class;
                    self.evaluation = previous_evaluation;
                }
                _ => {}
            }
        }
    }

    fn visit_variant(&mut self, node: &'ast syn::Variant) {
        if let Some((_, value)) = &node.discriminant {
            let previous = self.evaluation;
            self.evaluation = EvaluationMode::CompileTime;
            self.visit_expr(value);
            self.evaluation = previous;
        }
    }

    fn visit_expr_unary(&mut self, node: &'ast syn::ExprUnary) {
        if matches!(node.op, syn::UnOp::Neg(_))
            && let syn::Expr::Lit(literal) = &*node.expr
            && matches!(literal.lit, Lit::Int(_) | Lit::Float(_))
        {
            self.record(literal, format!("-{}", literal.lit.to_token_stream()));
        } else {
            visit::visit_expr_unary(self, node);
        }
    }
}

fn infer_class(scope: &str, value: &str) -> ParameterClass {
    let lower = scope.to_ascii_lowercase();
    if lower.contains("timeout") {
        ParameterClass::Timeout
    } else if lower.contains("seed") {
        ParameterClass::Seed
    } else if lower.contains("retry") {
        ParameterClass::Retry
    } else if lower.contains("capacity") || lower.contains("size") {
        ParameterClass::Capacity
    } else if value.contains('.') {
        ParameterClass::Coefficient
    } else {
        ParameterClass::Threshold
    }
}

fn class_to_tag(class: ParameterClass) -> ParameterTag {
    match class {
        ParameterClass::Threshold => ParameterTag::Threshold,
        ParameterClass::Capacity => ParameterTag::Capacity,
        ParameterClass::Budget => ParameterTag::Budget,
        ParameterClass::Timeout => ParameterTag::Timeout,
        ParameterClass::Retry => ParameterTag::Retry,
        ParameterClass::Tolerance => ParameterTag::Tolerance,
        ParameterClass::Coefficient => ParameterTag::Coefficient,
        ParameterClass::Seed => ParameterTag::Seed,
        ParameterClass::Layout => ParameterTag::Layout,
        ParameterClass::Ordering => ParameterTag::Ordering,
        ParameterClass::Unknown => ParameterTag::Custom("unknown".to_string()),
    }
}

fn value_to_parameter_value(value: &str) -> ParameterValue {
    let value = value.trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '_');
    if value.starts_with('-')
        && let Ok(value) = value.parse::<i128>()
    {
        return ParameterValue::Signed(value);
    }
    if let Ok(value) = value.parse::<u128>() {
        ParameterValue::Unsigned(value)
    } else if let Ok(value) = value.parse::<f64>() {
        ParameterValue::Float(value)
    } else {
        ParameterValue::Categorical(value.to_string())
    }
}

fn inferred_domain(value: &str, class: ParameterClass) -> ParameterDomain {
    let value = value_to_parameter_value(value);
    let Some(max) = (match value {
        ParameterValue::Unsigned(value) => Some(value),
        _ => None,
    }) else {
        return ParameterDomain::Unknown;
    };
    let min = if matches!(class, ParameterClass::Capacity | ParameterClass::Timeout) {
        1
    } else {
        0
    };
    ParameterDomain::Range(ParameterRange {
        min: ParameterValue::Unsigned(min),
        max: ParameterValue::Unsigned(max.saturating_mul(4).max(min)),
        inclusive_max: true,
    })
}

pub fn run_scan(path: Option<String>, auto_fix: bool, restore: bool) -> Result<(), String> {
    let start_dir = path.unwrap_or_else(|| ".".to_string());

    if restore {
        let backup_dir = Path::new(".covopt_backup");
        if backup_dir.exists() {
            println!("Restoring files from .covopt_backup/...");
            let mut restored = 0;

            fn restore_recursive(
                current_dir: &Path,
                base_backup: &Path,
                base_target: &Path,
                count: &mut usize,
            ) {
                if let Ok(entries) = fs::read_dir(current_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            restore_recursive(&path, base_backup, base_target, count);
                        } else if path.is_file()
                            && let Ok(relative) = path.strip_prefix(base_backup)
                        {
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
        return Ok(());
    }

    let mut files_to_scan = Vec::new();
    collect_rs_files(Path::new(&start_dir), &mut files_to_scan);

    println!("Scanning {} for magic numbers...", start_dir);
    let mut total_found = 0;
    let mut total_fixed = 0;
    let mut repair_edits = Vec::new();
    let mut source_hashes = BTreeMap::new();
    let mut rollback_entries = Vec::new();

    let config = crate::config::CovOptConfig::load_or_embedded(".covopt.toml").ok();
    let macro_path = config
        .and_then(|c| c.macro_path)
        .unwrap_or_else(|| "covopt_macro::covopt_param".to_string());

    let mut crate_no_std_cache: std::collections::HashMap<std::path::PathBuf, bool> =
        std::collections::HashMap::new();

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
                            if let Ok(toml_content) = fs::read_to_string(&cargo_toml)
                                && toml_content.contains("proc-macro = true")
                            {
                                crate_has_no_std = true;
                            }

                            if !crate_has_no_std {
                                for root_file in &[lib_rs, main_rs] {
                                    if let Ok(root_content) = fs::read_to_string(root_file)
                                        && root_content.contains("#![no_std]")
                                    {
                                        crate_has_no_std = true;
                                        break;
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
                println!(
                    "  [Skip] {} (no_std detected in crate root, requires custom macro_path)",
                    file_path.display()
                );
                continue;
            }
            if let Ok(syntax_tree) = syn::parse_file(&content) {
                let mut scanner = MagicNumberScanner {
                    file_path: file_path.to_string_lossy().to_string(),
                    found_magics: Vec::new(),
                    evaluation: EvaluationMode::Runtime,
                    class_override: None,
                    scope: Vec::new(),
                    semantic_occurrences: BTreeMap::new(),
                };
                scanner.visit_file(&syntax_tree);

                if !scanner.found_magics.is_empty() {
                    let scanned_file_path = scanner.file_path.clone();
                    println!("\n[{}]", scanned_file_path);
                    source_hashes.insert(
                        scanned_file_path.clone(),
                        crate::repair::SourceEdit::hash_source(&content),
                    );

                    // Sort by line, then column, descending, to safely rewrite
                    scanner.found_magics.sort_by_key(|magic| {
                        std::cmp::Reverse((magic.start.line, magic.start.column))
                    });

                    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
                    let mut file_changed = false;
                    let mut abort_scan = false;

                    for magic in scanner.found_magics {
                        if abort_scan {
                            break;
                        }
                        let start_loc = magic.start;
                        let end_loc = magic.end;
                        let val = magic.value;
                        let descriptor = magic.descriptor;
                        let evaluation = match descriptor.evaluation {
                            EvaluationMode::Runtime => "runtime",
                            EvaluationMode::CompileTime => "compile_time",
                        };
                        let class = format!("{:?}", descriptor.class).to_ascii_lowercase();
                        let replacement = format!(
                            "{}!(\"{}\", {}, class = \"{}\", evaluation = \"{}\")",
                            macro_path, descriptor.id, val, class, evaluation
                        );
                        repair_edits.push(ScannerRepairEdit {
                            file: scanned_file_path.clone(),
                            line: start_loc.line,
                            column: start_loc.column,
                            parameter_id: descriptor.id.clone(),
                            default: descriptor.default.clone(),
                            class: descriptor.class,
                            domain: descriptor.domain.clone(),
                            affected_scopes: vec![magic.scope.clone()],
                            associated_obligations: vec![format!(
                                "COVOPT-PARAMETER-{}",
                                descriptor.id.0.bytes().fold(
                                    0xcbf29ce484222325u64,
                                    |hash, byte| {
                                        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
                                    }
                                )
                            )],
                            replacement: replacement.clone(),
                            evaluation: descriptor.evaluation,
                            requires_evidence: true,
                        });
                        let line_idx = start_loc.line - 1;
                        if auto_fix && line_idx < lines.len() && start_loc.line == end_loc.line {
                            let line_str = &mut lines[line_idx];
                            let start_col = start_loc.column;
                            let end_col = end_loc.column;

                            if start_col <= end_col && end_col <= line_str.len() {
                                let old_line = line_str.clone();
                                let mut new_line = line_str.clone();
                                new_line.replace_range(start_col..end_col, &replacement);

                                println!(
                                    "  Line {}: Found parameter `{}` ({})",
                                    start_loc.line, descriptor.id, val
                                );
                                println!("- {}", old_line.trim_start());
                                println!("+ {}", new_line.trim_start());

                                use std::io::IsTerminal;
                                let is_non_interactive = !std::io::stdout().is_terminal()
                                    || std::env::var("COVOPT_NON_INTERACTIVE").is_ok()
                                    || std::env::var("CI").is_ok();

                                let explicitly_enabled = matches!(
                                    std::env::var("COVOPT_APPLY").as_deref(),
                                    Ok("1") | Ok("true") | Ok("yes")
                                );
                                let mut apply = is_non_interactive && explicitly_enabled;
                                if !is_non_interactive {
                                    loop {
                                        use std::io::{self, Write};
                                        print!("Apply this fix? [y]es / [n]o / [q]uit: ");
                                        let _ = io::stdout().flush();
                                        let mut input = String::new();
                                        let _ = io::stdin().read_line(&mut input);
                                        match input.trim().to_lowercase().as_str() {
                                            "y" | "yes" => {
                                                apply = true;
                                                break;
                                            }
                                            "n" | "no" => {
                                                apply = false;
                                                break;
                                            }
                                            "q" | "quit" => {
                                                abort_scan = true;
                                                break;
                                            }
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

                    let updated = lines.join("\n") + "\n";
                    if file_changed {
                        let static_ok = syn::parse_file(&updated).is_ok()
                            && crate::parameters::ParameterDependencyGraph::from_source(
                                &updated,
                                &file_path.display().to_string(),
                            )
                            .is_ok();
                        let compile_ok = if static_ok {
                            std::process::Command::new("cargo")
                                .args(["check", "--quiet"])
                                .output()
                                .map(|output| output.status.success())
                                .unwrap_or(false)
                        } else {
                            false
                        };
                        if !static_ok || !compile_ok {
                            eprintln!(
                                "  -> Skipped write for {}: compile/static scope verification failed",
                                file_path.display()
                            );
                            file_changed = false;
                        }
                    }

                    if file_changed {
                        // Backup the original file before modifying
                        let backup_base = Path::new(".covopt_backup");
                        let file_path_obj = Path::new(&file_path);
                        let backup_path = if let Ok(relative) =
                            file_path_obj.strip_prefix(Path::new(&start_dir))
                        {
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

                        if let Err(e) = fs::write(&file_path, updated) {
                            eprintln!("Failed to write {}: {}", file_path.display(), e);
                        } else {
                            rollback_entries.push(ScannerRollbackEntry {
                                source: file_path.display().to_string(),
                                backup: backup_path.display().to_string(),
                                original_hash: source_hashes
                                    .get(&file_path.display().to_string())
                                    .cloned()
                                    .unwrap_or_default(),
                            });
                        }
                    }

                    if abort_scan {
                        break;
                    }
                }
            }
        }
    }

    if !repair_edits.is_empty() {
        let plan = ScannerRepairPlan {
            schema_version: covopt_schema::SCHEMA_VERSION,
            source_hashes,
            edits: repair_edits,
            applied: total_fixed > 0,
        };
        let _ = fs::create_dir_all("target/covopt");
        fs::write(
            "target/covopt/scanner-repair-plan.json",
            serde_json::to_vec_pretty(&plan).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        if !rollback_entries.is_empty() {
            fs::create_dir_all(".covopt_backup").map_err(|error| error.to_string())?;
            fs::write(
                ".covopt_backup/manifest.json",
                serde_json::to_vec_pretty(&rollback_entries).map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
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
            return Err(format!(
                "found {} magic numbers and fixed {}",
                total_found, total_fixed
            ));
        }
    } else {
        println!("\n[OK] No magic numbers found! The codebase is highly tunable.");
    }
    Ok(())
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
                    s == "covopt-macro"
                        || s == "covopt_macro"
                        || s.contains("proc-macro")
                        || s.contains("proc_macro")
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

            #[repr(align(64))]
            struct Aligned(u8);

            #[covopt_hoist(capacity = 32, partition = "fixture")]
            struct Hoisted(u8);

            fn regular_fn(x: i32) -> i32 {
                let y = 999;
                let first = 0;
                let second = 0;
                match x {
                    123 => y + 888 + first,
                    _ => second,
                }
            }
        "#;

        let syntax_tree = syn::parse_file(code).expect("failed to parse test code");
        let mut scanner = MagicNumberScanner {
            file_path: "test.rs".to_string(),
            found_magics: Vec::new(),
            evaluation: EvaluationMode::Runtime,
            class_override: None,
            scope: Vec::new(),
            semantic_occurrences: BTreeMap::new(),
        };
        scanner.visit_file(&syntax_tree);

        let found_vals: Vec<&str> = scanner
            .found_magics
            .iter()
            .map(|magic| magic.value.as_str())
            .collect();

        // Explicit const/static declarations and patterns remain invariants;
        // compile-time use sites are still discovered for explicit modeling.
        assert!(
            !found_vals.contains(&"42"),
            "Static item magic number 42 should be skipped"
        );
        assert!(
            !found_vals.contains(&"100"),
            "Const item magic number 100 should be skipped"
        );
        assert!(
            found_vals.contains(&"50"),
            "const fn body should be discoverable"
        );
        assert!(
            found_vals.contains(&"10"),
            "enum discriminants should be discoverable"
        );
        assert!(
            found_vals.contains(&"20"),
            "enum discriminants should be discoverable"
        );
        assert!(
            found_vals.contains(&"64"),
            "alignment attributes should be discoverable"
        );
        assert!(
            found_vals.contains(&"32"),
            "hoist section capacity should be discoverable"
        );
        let ids = scanner
            .found_magics
            .iter()
            .map(|magic| magic.descriptor.id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(ids.len(), scanner.found_magics.len());
        assert!(ids.iter().all(|id| !id.0.contains("M_")));
        assert!(
            !found_vals.contains(&"123"),
            "Pattern arm magic number 123 should be skipped"
        );

        // Runtime function-body literals, including 0/1/2, are discovered.
        assert!(
            found_vals.contains(&"999"),
            "Regular fn body magic number 999 should be found"
        );
        assert!(
            found_vals.contains(&"888"),
            "Regular fn body magic number 888 should be found"
        );
        assert!(found_vals.contains(&"0"), "runtime zero should be found");
    }
}
