use crate::mca::McaReport;
use crate::static_analysis::{analyze_complexity, analyze_parameters};
use syn::ItemFn;
use std::collections::HashSet;

pub struct EncapsulationAdvisor;

pub struct AdviseReport {
    pub warnings: Vec<String>,
}

impl EncapsulationAdvisor {
    pub fn analyze(item_fn: &ItemFn, mca_report: Option<&McaReport>) -> AdviseReport {
        let mut warnings = Vec::new();
        
        // 1. Check Missing Encapsulation (God Function)
        let complexity = analyze_complexity(item_fn);
        let params = analyze_parameters(item_fn);
        
        if complexity > 10 {
            warnings.push(format!(
                "God Function detected: Cyclomatic complexity is {}. Consider extracting logic into smaller functions.",
                complexity
            ));
        }
        
        if params > 5 {
            warnings.push(format!(
                "Parameter Bloat: Function takes {} parameters. Consider encapsulating them into a context struct.",
                params
            ));
        }

        // 2. Check Abstraction Penalty (Over-encapsulation)
        if complexity == 1 {
            // Function has no control flow. Is it just a pass-through?
            let is_pass_through = Self::is_pure_pass_through(item_fn);
            if is_pass_through {
                warnings.push(
                    "Abstraction Penalty: This function appears to be a pure pass-through wrapper with no added logic. Consider removing it or marking it #[inline(always)].".to_string()
                );
            }
            
            if let Some(mca) = mca_report
                && mca.ipc < 1.0 && is_pass_through {
                    warnings.push(format!(
                        "Performance Penalty: MCA reports very low IPC ({:.2}) for this pass-through function. The encapsulation is costing CPU cycles overhead.",
                        mca.ipc
                    ));
                }
        }
        
        AdviseReport { warnings }
    }

    fn is_pure_pass_through(item_fn: &ItemFn) -> bool {
        // Very basic heuristic: if it has exactly 1 statement which is an expression.
        if item_fn.block.stmts.len() == 1
            && let syn::Stmt::Expr(_, _) = &item_fn.block.stmts[0] {
                return true;
            }
        false
    }

    pub fn detect_asm_clones(asm_blocks: &[(&str, &str)]) -> Vec<String> {
        let mut warnings = Vec::new();
        let mut opcodes_list: Vec<Vec<String>> = Vec::new();
        
        for (_, block) in asm_blocks {
            opcodes_list.push(Self::extract_opcodes(block));
        }

        for i in 0..opcodes_list.len() {
            for j in (i + 1)..opcodes_list.len() {
                let sim = Self::calculate_similarity(&opcodes_list[i], &opcodes_list[j]);
                if sim > 0.9 {
                    warnings.push(format!(
                        "Semantic Clone Detected: Function '{}' and '{}' share {:.1}% identical machine-level opcodes. Consider deduplication.",
                        asm_blocks[i].0, asm_blocks[j].0, sim * 100.0
                    ));
                }
            }
        }
        warnings
    }

    fn extract_opcodes(asm: &str) -> Vec<String> {
        let mut opcodes = Vec::new();
        for line in asm.lines() {
            let line = line.trim();
            // Ignore labels and comments
            if line.is_empty() || line.starts_with('.') || line.ends_with(':') || line.starts_with('#') {
                continue;
            }
            // First word is usually the opcode
            if let Some(opcode) = line.split_whitespace().next()
                && !opcode.starts_with('.') {
                    opcodes.push(opcode.to_string());
                }
        }
        opcodes
    }

    fn calculate_similarity(a: &[String], b: &[String]) -> f64 {
        if a.is_empty() && b.is_empty() { return 1.0; }
        if a.is_empty() || b.is_empty() { return 0.0; }
        
        // Use Jaccard similarity of opcodes
        let set_a: HashSet<_> = a.iter().collect();
        let set_b: HashSet<_> = b.iter().collect();
        
        let intersection = set_a.intersection(&set_b).count() as f64;
        let union = set_a.union(&set_b).count() as f64;
        
        if union == 0.0 {
            0.0
        } else {
            intersection / union
        }
    }
}
