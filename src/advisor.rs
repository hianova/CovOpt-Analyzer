use crate::mca::McaReport;
use crate::static_analysis::{analyze_complexity, analyze_parameters};
use covopt_macro::covopt_param;
use std::collections::HashSet;
use syn::visit::Visit;
use syn::{ExprCall, ExprForLoop, ExprMethodCall, ExprWhile, ItemFn};

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

        if complexity > covopt_param!("M_22_24", 10) {
            warnings.push(format!(
                "[Rule 5: God Function] Cyclomatic complexity is {}. Splitting cold paths (e.g., error handling) and marking them #[inline(never)] prevents I-Cache trashing.",
                complexity
            ));
        }

        if params > covopt_param!("M_29_20", 5) {
            warnings.push(format!(
                "[Rule 1: Cache Dynamics] Function takes {} parameters. Consider grouping into an aligned context struct to avoid register spilling and stack thrashing.",
                params
            ));
        }

        let generic_count = item_fn.sig.generics.params.len();
        if generic_count > covopt_param!("M_37_27", 3) {
            warnings.push(format!(
                "[Rule 5: Generic Bloat] Function uses {} generic parameters. This causes massive monomorphization, skyrocketing binary size and destroying I-Cache. Consider Box<dyn Trait> for cold paths.",
                generic_count
            ));
        }

        // 2. Senior Engineer Static Analysis (AST Visitor)
        let mut visitor = SeniorEngineerVisitor::default();
        if item_fn.sig.asyncness.is_some() {
            visitor.in_async = true;
        }
        visitor.visit_item_fn(item_fn);

        if visitor.allocations_in_hot_path > 0 {
            warnings.push(format!(
                "[Rule 2: Hidden Hot-Path Overheads] Found {} implicit allocations (.clone(), format!(), vec![]) inside loops. This destroys Global Allocator performance and fragments memory. Use Cow<str>, compact_str, or pre-allocated arenas instead.",
                visitor.allocations_in_hot_path
            ));
        }

        if visitor.blocking_calls_in_async > 0 {
            warnings.push(format!(
                "[Rule 4: Concurrency Disasters] Found {} standard blocking calls (e.g., std::thread::sleep, fs::read, std::sync::Mutex) inside an async context! This will poison the Tokio reactor and stall the entire thread pool. Use tokio::task::spawn_blocking or async-native equivalents.",
                visitor.blocking_calls_in_async
            ));
        }

        if visitor.threads_spawned_in_loop > 0 {
            warnings.push(format!(
                "[Rule 6: Thread Physical Overbound] Detected thread::spawn inside a loop! Spawning unbounded {} threads shreds L3 cache via massive OS context-switching overhead. Pre-allocate a ThreadPool bounded strictly by the physical CPU core count.",
                visitor.threads_spawned_in_loop
            ));
        }

        if visitor.mutex_in_hot_path > 0 {
            warnings.push(format!(
                "[Rule 7: Lock Contention] Found {} Mutex/RwLock lock() calls inside a loop! This serializes execution and destroys multi-core scalability. Consider Lock-Free Data Structures, Thread-Local Storage, or batching critical sections outside the loop.",
                visitor.mutex_in_hot_path
            ));
        }

        if visitor.io_in_hot_path > 0 {
            warnings.push(format!(
                "[Rule 8: IO in Hot Path] Found {} println!/print! macros inside a loop. Synchronous IO completely stalls the CPU pipeline. Consider accumulating results and printing them outside the loop.",
                visitor.io_in_hot_path
            ));
        }

        if visitor.manual_cas_loops > 0 {
            warnings.push(format!(
                "[Rule 3: Lock-Free Critical Paths & fetch_update] Found {} manual compare_exchange CAS loops. Manual CAS loops cause thread starvation (livelock) under hyper-concurrent loads. ALWAYS use Atomic::fetch_update to bound P99.99 tail latency.",
                visitor.manual_cas_loops
            ));
        }

        // 3. Branch Prediction (Combining MCA data)
        if let Some(mca) = mca_report {
            if mca.ipc < covopt_param!("M_95_25", 1.5) && complexity > covopt_param!("M_95_45", 5) {
                warnings.push(format!(
                    "[Rule 3: Branch Prediction Thrashing] High complexity ({}) combined with low IPC ({:.2}) strongly suggests the Branch Predictor is failing (e.g., iterating over random data). Consider executing data.sort_unstable() before the hot loop or using branchless bitwise arithmetic.",
                    complexity, mca.ipc
                ));
            }
            if mca.ipc < 1.0 && Self::is_pure_pass_through(item_fn) {
                warnings.push(format!(
                    "Abstraction Penalty: Low IPC ({:.2}) for a pass-through function. Consider removing it or marking #[inline(always)].",
                    mca.ipc
                ));
            }
        }

        AdviseReport { warnings }
    }

    pub fn analyze_struct(item_struct: &syn::ItemStruct) -> AdviseReport {
        let mut warnings = Vec::new();
        let mut has_atomic = false;
        let mut has_align = false;

        for attr in &item_struct.attrs {
            let attr_str = quote::quote!(#attr).to_string().replace(" ", "");
            if attr_str.contains("repr(align(64))") {
                has_align = true;
            }
        }

        for field in &item_struct.fields {
            let ty_str = quote::quote!(#field).to_string();
            if ty_str.contains("Atomic") {
                has_atomic = true;
            }
        }

        if has_atomic && !has_align {
            warnings.push(format!(
                "[Rule 4: Cache Dynamics, False Sharing & Padding] Struct '{}' contains Atomic variables but is missing #[repr(align(64))]. This causes False Sharing and MESI cache line bouncing under concurrent load.",
                item_struct.ident
            ));
        }

        AdviseReport { warnings }
    }
    fn is_pure_pass_through(item_fn: &ItemFn) -> bool {
        // Very basic heuristic: if it has exactly 1 statement which is an expression.
        if item_fn.block.stmts.len() == 1
            && let syn::Stmt::Expr(_, _) = &item_fn.block.stmts[0]
        {
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
                if sim > covopt_param!("M_162_25", 0.9) {
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
            if line.is_empty()
                || line.starts_with('.')
                || line.ends_with(':')
                || line.starts_with('#')
            {
                continue;
            }
            // First word is usually the opcode
            if let Some(opcode) = line.split_whitespace().next()
                && !opcode.starts_with('.')
            {
                opcodes.push(opcode.to_string());
            }
        }
        opcodes
    }

    fn calculate_similarity(a: &[String], b: &[String]) -> f64 {
        if a.is_empty() && b.is_empty() {
            return 1.0;
        }
        if a.is_empty() || b.is_empty() {
            return 0.0;
        }

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

#[derive(Default)]
struct SeniorEngineerVisitor {
    in_loop: bool,
    in_async: bool,
    loop_depth: usize,

    // Counters & Flags
    allocations_in_hot_path: usize,
    blocking_calls_in_async: usize,
    threads_spawned_in_loop: usize,
    mutex_in_hot_path: usize,
    io_in_hot_path: usize,
    manual_cas_loops: usize,
}

impl<'ast> Visit<'ast> for SeniorEngineerVisitor {
    fn visit_expr_for_loop(&mut self, i: &'ast ExprForLoop) {
        self.in_loop = true;
        self.loop_depth += 1;
        syn::visit::visit_expr_for_loop(self, i);
        self.loop_depth -= 1;
        if self.loop_depth == 0 {
            self.in_loop = false;
        }
    }

    fn visit_expr_while(&mut self, i: &'ast ExprWhile) {
        self.in_loop = true;
        self.loop_depth += 1;
        syn::visit::visit_expr_while(self, i);
        self.loop_depth -= 1;
        if self.loop_depth == 0 {
            self.in_loop = false;
        }
    }

    fn visit_expr_method_call(&mut self, i: &'ast ExprMethodCall) {
        let method_name = i.method.to_string();
        if self.in_loop && (method_name == "clone" || method_name == "to_string") {
            self.allocations_in_hot_path += 1;
        }
        if self.in_loop
            && (method_name == "lock" || method_name == "read" || method_name == "write")
        {
            self.mutex_in_hot_path += 1;
        }
        if self.in_loop
            && (method_name == "compare_exchange" || method_name == "compare_exchange_weak")
        {
            self.manual_cas_loops += 1;
        }
        syn::visit::visit_expr_method_call(self, i);
    }

    fn visit_macro(&mut self, i: &'ast syn::Macro) {
        let macro_name = i
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default();
        if self.in_loop && (macro_name == "format" || macro_name == "vec") {
            self.allocations_in_hot_path += 1;
        }
        if self.in_loop
            && (macro_name == "println"
                || macro_name == "print"
                || macro_name == "eprintln"
                || macro_name == "eprint")
        {
            self.io_in_hot_path += 1;
        }
        syn::visit::visit_macro(self, i);
    }

    fn visit_expr_macro(&mut self, i: &'ast syn::ExprMacro) {
        self.visit_macro(&i.mac);
        syn::visit::visit_expr_macro(self, i);
    }

    fn visit_stmt_macro(&mut self, i: &'ast syn::StmtMacro) {
        self.visit_macro(&i.mac);
        syn::visit::visit_stmt_macro(self, i);
    }

    fn visit_expr_call(&mut self, i: &'ast ExprCall) {
        if let syn::Expr::Path(p) = &*i.func {
            let func_name = p
                .path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or_default();
            let full_path = quote::quote!(#p).to_string().replace(" ", "");

            if self.in_async {
                if full_path.contains("thread::sleep") || full_path.contains("fs::") {
                    self.blocking_calls_in_async += 1;
                }
                if full_path.contains("Mutex") {
                    self.blocking_calls_in_async += 1; // Also bad in async
                }
            }

            if self.in_loop && (func_name == "spawn" || full_path.contains("thread::spawn")) {
                self.threads_spawned_in_loop += 1;
            }
        }
        syn::visit::visit_expr_call(self, i);
    }
}
