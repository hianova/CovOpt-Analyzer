use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use syn::ExprForLoop;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
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
pub struct AutoFixer;
impl AutoFixer {
    pub fn run(path: &str) -> Result<()> {
        println!("🚀 Starting CovOpt AST Auto-Fixer (Inspired by ENLIGHTEN)...");
        let mut files_to_scan = Vec::new();
        collect_test_files(Path::new(path), &mut files_to_scan);
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
            let mut needs_import = false;
            for t in targets {
                let line_idx = t.line - 1;
                let col = t.column;
                let len = t.length;
                let original_line = lines[line_idx].clone();
                let prefix = &original_line[..col];
                let suffix = &original_line[col + len..];
                let replacement = format!("std::hint::black_box({})", t.original_expr);
                lines[line_idx] = format!("{}{}{}", prefix, replacement, suffix);
                needs_import = true;
            }
            if needs_import && !lines.iter().any(|l| l.contains("std::hint::black_box")) {
                lines.insert(0, "use std::hint::black_box;".to_string());
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
        let code = "fn main() {\n    for i in std::hint::black_box(0..10) {\n        println!(\"{}\", i);\n    }\n}";
        let syntax_tree = syn::parse_file(code).unwrap();
        let mut scanner = Rule2Scanner {
            file_path: "dummy.rs".to_string(),
            targets: Vec::new(),
            source_content: code.to_string(),
        };
        scanner.visit_file(&syntax_tree);
        assert_eq!(scanner.targets.len(), 0);
    }
}
