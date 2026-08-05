use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::visit_mut::{self, VisitMut};
use syn::{Expr, ExprAsync, ExprClosure, ExprForLoop, ImplItemFn, ItemFn};

#[derive(Debug, Clone, PartialEq)]
#[repr(C, align(64))]
pub struct AstFixTarget {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub length: usize,
    pub original_expr: String,
}

#[repr(C, align(64))]
pub struct Rule2Scanner {
    pub file_path: String,
    pub targets: Vec<AstFixTarget>,
    pub source_content: String,
}

impl<'ast> Visit<'ast> for Rule2Scanner {
    fn visit_expr_for_loop(&mut self, node: &'ast ExprForLoop) {
        let expr = &*node.expr;
        let start = expr.span().start();
        let end = expr.span().end();
        if start.line == end.line {
            let line_idx = start.line - 1;
            if let Some(line_str) = self.source_content.lines().nth(line_idx) {
                let expr_str = &line_str[start.column..end.column];
                if !expr_str.contains("black_box") {
                    self.targets.push(AstFixTarget {
                        file_path: self.file_path.clone(),
                        line: start.line,
                        column: start.column,
                        length: end.column - start.column,
                        original_expr: expr_str.to_string(),
                    });
                }
            }
        }
        visit::visit_expr_for_loop(self, node);
    }
}

#[repr(C, align(64))]
pub struct AsyncStarvationShieldScanner {
    pub file_path: String,
    pub blocking_calls_count: usize,
}

#[derive(Default)]
#[repr(C, align(64))]
pub struct AsyncStarvationShieldRewriter {
    pub async_depth: usize,
    pub rewrites_count: usize,
}

impl AsyncStarvationShieldRewriter {
    pub fn new() -> Self {
        Self::default()
    }
}

fn get_path_string(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

fn is_blocking_call(expr: &Expr) -> bool {
    let mut e = expr;
    while let Expr::Paren(p) = e {
        e = &*p.expr;
    }
    match e {
        Expr::Call(call) => {
            if let Expr::Path(expr_path) = &*call.func {
                let p = get_path_string(&expr_path.path);
                if matches!(
                    p.as_str(),
                    "std::fs::read"
                        | "fs::read"
                        | "read"
                        | "std::fs::read_to_string"
                        | "fs::read_to_string"
                        | "read_to_string"
                        | "std::fs::write"
                        | "fs::write"
                        | "write"
                        | "std::fs::copy"
                        | "fs::copy"
                        | "copy"
                        | "std::fs::File::open"
                        | "fs::File::open"
                        | "File::open"
                        | "std::thread::sleep"
                        | "thread::sleep"
                        | "sleep"
                        | "std::thread::park"
                        | "thread::park"
                        | "park"
                        | "std::process::Command::output"
                        | "Command::output"
                ) {
                    return true;
                }
            }
            false
        }
        Expr::MethodCall(mcall) => {
            let method_name = mcall.method.to_string();
            if method_name == "output" {
                return true;
            }
            if (method_name == "unwrap" || method_name == "expect")
                && let Expr::MethodCall(inner) = &*mcall.receiver
            {
                let inner_method = inner.method.to_string();
                if inner_method == "lock" || inner_method == "read" || inner_method == "write" {
                    return true;
                }
            }
            if method_name == "lock" || method_name == "read" || method_name == "write" {
                return true;
            }
            false
        }
        _ => false,
    }
}

impl VisitMut for AsyncStarvationShieldRewriter {
    fn visit_item_fn_mut(&mut self, node: &mut ItemFn) {
        let is_async = node.sig.asyncness.is_some();
        if is_async {
            self.async_depth += 1;
        }
        visit_mut::visit_item_fn_mut(self, node);
        if is_async {
            self.async_depth -= 1;
        }
    }

    fn visit_impl_item_fn_mut(&mut self, node: &mut ImplItemFn) {
        let is_async = node.sig.asyncness.is_some();
        if is_async {
            self.async_depth += 1;
        }
        visit_mut::visit_impl_item_fn_mut(self, node);
        if is_async {
            self.async_depth -= 1;
        }
    }

    fn visit_expr_async_mut(&mut self, node: &mut ExprAsync) {
        self.async_depth += 1;
        visit_mut::visit_expr_async_mut(self, node);
        self.async_depth -= 1;
    }

    fn visit_expr_closure_mut(&mut self, node: &mut ExprClosure) {
        if node.asyncness.is_some() {
            self.async_depth += 1;
            visit_mut::visit_expr_closure_mut(self, node);
            self.async_depth -= 1;
        } else {
            let saved_depth = self.async_depth;
            self.async_depth = 0;
            visit_mut::visit_expr_closure_mut(self, node);
            self.async_depth = saved_depth;
        }
    }

    fn visit_expr_mut(&mut self, node: &mut Expr) {
        if let Expr::Call(call) = node
            && let Expr::Path(expr_path) = &*call.func
        {
            let p = get_path_string(&expr_path.path);
            if p == "std::thread::spawn"
                || p == "thread::spawn"
                || p == "tokio::task::spawn_blocking"
                || p == "spawn_blocking"
            {
                visit_mut::visit_expr_mut(self, &mut call.func);
                let saved_depth = self.async_depth;
                self.async_depth = 0;
                for arg in &mut call.args {
                    self.visit_expr_mut(arg);
                }
                self.async_depth = saved_depth;
                return;
            }
        }

        if self.async_depth > 0 {
            if let Expr::Unary(syn::ExprUnary {
                op: syn::UnOp::Deref(_),
                expr: inner,
                ..
            }) = node
                && is_blocking_call(inner)
            {
                self.rewrites_count += 1;
                let orig = node.clone();
                *node = syn::parse_quote!(tokio::task::spawn_blocking(move || { #orig }).await.expect("blocking task panicked"));
                return;
            }

            if is_blocking_call(node) {
                self.rewrites_count += 1;
                let orig = node.clone();
                *node = syn::parse_quote!(tokio::task::spawn_blocking(move || { #orig }).await.expect("blocking task panicked"));
                return;
            }
        }

        visit_mut::visit_expr_mut(self, node);
    }
}

pub fn run_async_starvation_shield(target_path: &Path) -> Result<usize> {
    let mut files = Vec::new();
    if target_path.is_file() {
        if target_path.extension().and_then(|s| s.to_str()) == Some("rs") {
            files.push(target_path.to_path_buf());
        }
    } else if target_path.is_dir() {
        collect_rs_files_for_shield(target_path, &mut files);
    }

    let mut total_rewrites = 0;

    for file_path in files {
        let content = match fs::read_to_string(&file_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let mut ast = match syn::parse_file(&content) {
            Ok(a) => a,
            Err(_) => continue,
        };

        let mut rewriter = AsyncStarvationShieldRewriter::new();
        rewriter.visit_file_mut(&mut ast);

        if rewriter.rewrites_count > 0 {
            total_rewrites += rewriter.rewrites_count;
            let formatted = quote::quote!(#ast).to_string();
            let _ = fs::write(&file_path, formatted);
            println!(
                "🛡️ [Async Starvation Shield] Rewrote {} blocking call(s) in {}",
                rewriter.rewrites_count,
                file_path.display()
            );
        }
    }

    Ok(total_rewrites)
}

fn collect_rs_files_for_shield(dir: &Path, files: &mut Vec<PathBuf>) {
    if dir.is_dir()
        && let Ok(entries) = fs::read_dir(dir)
    {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let fname = p.file_name().unwrap_or_default().to_string_lossy();
                if fname != "target" && fname != ".git" && !fname.starts_with('.') {
                    collect_rs_files_for_shield(&p, files);
                }
            } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                files.push(p);
            }
        }
    }
}

#[repr(C, align(64))]
pub struct AutoFixer;

impl AutoFixer {
    pub fn run(path: &str) -> Result<()> {
        println!("🚀 Starting CovOpt AST Auto-Fixer (Inspired by ENLIGHTEN)...");
        let target_path = Path::new(path);
        let _ = run_async_starvation_shield(target_path);

        let mut files_to_scan = Vec::new();
        if target_path.is_file() {
            if target_path.extension().and_then(|s| s.to_str()) == Some("rs") {
                files_to_scan.push(target_path.to_path_buf());
            }
        } else {
            collect_test_files(target_path, &mut files_to_scan);
        }

        let mut all_targets = Vec::new();
        for file_path in files_to_scan {
            let file_path_str = file_path.to_string_lossy().to_string();
            if let Ok(content) = fs::read_to_string(&file_path)
                && let Ok(syntax_tree) = syn::parse_file(&content)
            {
                let mut scanner = Rule2Scanner {
                    file_path: file_path_str,
                    targets: Vec::new(),
                    source_content: content,
                };
                scanner.visit_file(&syntax_tree);
                all_targets.extend(scanner.targets);
            }
        }
        if all_targets.is_empty() {
            println!("✅ No AST auto-fix targets found. Code is clean!");
            return Ok(());
        }
        println!(
            "🔧 Found {} locations needing AST auto-completion (Rule 2: Anti-DCE).",
            all_targets.len()
        );
        let mut by_file: std::collections::HashMap<String, Vec<AstFixTarget>> =
            std::collections::HashMap::new();
        for t in all_targets {
            by_file.entry(t.file_path.clone()).or_default().push(t);
        }
        for (file_path, mut targets) in by_file {
            let content =
                fs::read_to_string(&file_path).context("Failed to read file for AST fixing")?;
            let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
            targets.sort_by(|a, b| b.line.cmp(&a.line).then_with(|| b.column.cmp(&a.column)));
            for t in targets {
                let line_idx = t.line - 1;
                let col = t.column;
                let len = t.length;
                let original_line = lines[line_idx].clone();
                let prefix = &original_line[..col];
                let suffix = &original_line[col + len..];
                let replacement = format!("core::hint::black_box({})", t.original_expr);
                lines[line_idx] = format!("{}{}{}", prefix, replacement, suffix);
            }
            fs::write(&file_path, lines.join("\n")).context("Failed to write AST fix to file")?;
            println!("  -> Auto-fixed AST in {}", file_path);
        }
        println!("🏆 AST Auto-Completion successful.");
        Ok(())
    }
}

fn collect_test_files(dir: &Path, files: &mut Vec<PathBuf>) {
    if dir.is_dir()
        && let Ok(entries) = fs::read_dir(dir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let file_name = path.file_name().unwrap_or_default().to_string_lossy();
                if file_name != "target" && file_name != ".git" && !file_name.starts_with('.') {
                    collect_test_files(&path, files);
                }
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                let path_str = path.to_string_lossy();
                if path_str.contains("/tests/") || path_str.contains("/benches/") {
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
    fn test_rule2_scanner_detects_unwrapped_loop() {
        let code = "fn main() {\n    for i in 0..10 {\n        println!(\"{}\", i);\n    }\n}";
        let syntax_tree = syn::parse_file(code).unwrap();
        let mut scanner = Rule2Scanner {
            file_path: "dummy.rs".to_string(),
            targets: Vec::new(),
            source_content: code.to_string(),
        };
        scanner.visit_file(&syntax_tree);
        assert_eq!(scanner.targets.len(), 1);
        assert_eq!(scanner.targets[0].original_expr, "0..10");
        assert_eq!(scanner.targets[0].line, 2);
    }

    #[test]
    fn test_rule2_scanner_ignores_wrapped_loop() {
        let code = "fn main() {\n    for i in core::hint::black_box(0..10) {\n        println!(\"{}\", i);\n    }\n}";
        let syntax_tree = syn::parse_file(code).unwrap();
        let mut scanner = Rule2Scanner {
            file_path: "dummy.rs".to_string(),
            targets: Vec::new(),
            source_content: code.to_string(),
        };
        scanner.visit_file(&syntax_tree);
        assert_eq!(scanner.targets.len(), 0);
    }

    #[test]
    fn test_async_starvation_shield_rewrites_dummy_fixture() {
        let mut dummy_path = PathBuf::from("tests/dummy_async_shield.rs");
        if !dummy_path.exists() {
            dummy_path = PathBuf::from("CovOpt-Analyzer/tests/dummy_async_shield.rs");
        }
        assert!(
            dummy_path.exists(),
            "dummy_async_shield.rs test fixture does not exist"
        );

        let temp_dir = tempfile::tempdir().unwrap();
        let target_file = temp_dir.path().join("dummy_async_shield_test.rs");
        fs::copy(&dummy_path, &target_file).unwrap();

        let rewrites = run_async_starvation_shield(&target_file).unwrap();
        assert!(
            rewrites >= 2,
            "Expected at least 2 rewrites (sleep, read_to_string, lock), got {}",
            rewrites
        );

        let content = fs::read_to_string(&target_file).unwrap();
        assert!(
            content.contains("spawn_blocking"),
            "Rewritten file must contain spawn_blocking"
        );
        assert!(
            content.contains("blocking task panicked"),
            "Rewritten file must contain expectation message"
        );
    }
}
