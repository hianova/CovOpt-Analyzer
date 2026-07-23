use std::fs;
use std::path::Path;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{ExprForLoop, ExprLoop, ExprMethodCall, ExprReturn, ExprWhile, ItemFn};

pub struct DataflowScanner {
    pub file_path: String,
    pub func_name: String,
    pub loop_depth: usize,
    pub warnings: Vec<String>,
}

struct LockEscapeScanner {
    has_lock: bool,
}

impl<'ast> Visit<'ast> for LockEscapeScanner {
    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if node.method == "lock" {
            self.has_lock = true;
        }
        visit::visit_expr_method_call(self, node);
    }
}

impl<'ast> Visit<'ast> for DataflowScanner {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let prev_func = self.func_name.clone();
        self.func_name = node.sig.ident.to_string();

        // Visit the function body to track clones and explicit returns
        visit::visit_item_fn(self, node);

        // Check implicit return (the last statement if it's an expression without a semicolon)
        if let Some(syn::Stmt::Expr(expr, None)) = node.block.stmts.last() {
            let mut escape_scanner = LockEscapeScanner { has_lock: false };
            escape_scanner.visit_expr(expr);
            if escape_scanner.has_lock {
                self.warnings.push(format!(
                    "  - [Line {}] [Taint Analysis] Lock guard implicitly escapes the function scope! This can lead to uncontrolled lock durations and deadlocks.",
                    expr.span().start().line
                ));
            }
        }

        self.func_name = prev_func;
    }

    fn visit_expr_loop(&mut self, node: &'ast ExprLoop) {
        self.loop_depth += 1;
        visit::visit_expr_loop(self, node);
        self.loop_depth -= 1;
    }

    fn visit_expr_while(&mut self, node: &'ast ExprWhile) {
        self.loop_depth += 1;
        visit::visit_expr_while(self, node);
        self.loop_depth -= 1;
    }

    fn visit_expr_for_loop(&mut self, node: &'ast ExprForLoop) {
        self.loop_depth += 1;
        visit::visit_expr_for_loop(self, node);
        self.loop_depth -= 1;
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        let method_name = node.method.to_string();

        if method_name == "clone" && self.loop_depth > 0 {
            self.warnings.push(format!(
                "  - [Line {}] [Data-Flow] Detected `.clone()` inside a loop block. Consider borrowing to avoid allocation overhead on the hot path.",
                node.method.span().start().line
            ));
        }

        visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_return(&mut self, node: &'ast ExprReturn) {
        if let Some(expr) = &node.expr {
            let mut escape_scanner = LockEscapeScanner { has_lock: false };
            escape_scanner.visit_expr(expr);
            if escape_scanner.has_lock {
                self.warnings.push(format!(
                    "  - [Line {}] [Taint Analysis] Lock guard explicitly escapes the function scope! This can lead to uncontrolled lock durations and deadlocks.",
                    expr.span().start().line
                ));
            }
        }
        visit::visit_expr_return(self, node);
    }
}

pub fn analyze_file(file_path: &Path) -> Vec<String> {
    let mut all_warnings = Vec::new();
    if let Ok(content) = fs::read_to_string(file_path)
        && let Ok(syntax_tree) = syn::parse_file(&content) {
            let mut scanner = DataflowScanner {
                file_path: file_path.to_string_lossy().into_owned(),
                func_name: String::new(),
                loop_depth: 0,
                warnings: Vec::new(),
            };
            scanner.visit_file(&syntax_tree);
            all_warnings = scanner.warnings;
        }
    all_warnings
}

pub fn run_dataflow(path: Option<String>) {
    let start_dir = path.unwrap_or_else(|| ".".to_string());
    let mut files_to_scan = Vec::new();
    crate::scanner::collect_rs_files(Path::new(&start_dir), &mut files_to_scan);

    println!("CovOpt-Analyzer: Running Data-Flow & Taint Analysis on {}...", start_dir);
    let mut total_warnings = 0;

    for file in files_to_scan {
        let warnings = analyze_file(&file);
        if !warnings.is_empty() {
            println!("\n[{}]", file.display());
            for w in &warnings {
                println!("{}", w);
            }
            total_warnings += warnings.len();
        }
    }

    if total_warnings == 0 {
        println!("\n✅ No data-flow violations (implicit clones or lock escapes) found.");
    } else {
        println!("\n⚠️ Found {} data-flow violations.", total_warnings);
    }
}
