use std::fs;
use std::path::Path;
use syn::spanned::Spanned;
use syn::visit::Visit;

#[derive(Debug, Default)]
pub struct MemoryProfile {
    pub loads: usize,
    pub stores: usize,
    pub allocs: usize,
}

pub fn analyze_memory_ops(asm_block: &str) -> MemoryProfile {
    let mut profile = MemoryProfile::default();

    for line in asm_block.lines() {
        let l = line.to_lowercase();
        // Simple heuristic for memory ops in assembly
        // ARM uses ldr/str, x86 uses mov with brackets

        if (l.contains("call") || l.contains("bl "))
            && (l.contains("alloc")
                || l.contains("malloc")
                || l.contains("push")
                || l.contains("reserve"))
        {
            profile.allocs += 1;
        }

        if l.contains("ldr ") || l.contains("ldp ") || l.contains("mov") {
            // If it's x86 mov, look for memory brackets []
            // Dest is usually before comma, Source is after comma in Intel syntax,
            // but objdump often outputs AT&T syntax.
            // A simple heuristic: if it contains `mov` and `(`, it's a memory access in AT&T syntax.
            if l.contains("ldr ") || l.contains("ldp ") || (l.contains("mov") && l.contains("(")) {
                // We'll just count as load for now if we can't easily distinguish AT&T src/dest.
                // Let's refine:
                // AT&T: mov src, dest. Memory is like (%rax).
                if l.contains("mov") {
                    let parts: Vec<&str> = l.split(',').collect();
                    if parts.len() == 2 {
                        if parts[0].contains("(") {
                            profile.loads += 1;
                        } else if parts[1].contains("(") {
                            profile.stores += 1;
                        }
                    } else {
                        profile.loads += 1;
                    }
                } else {
                    profile.loads += 1; // ARM ldr
                }
            }
        }

        if l.contains("str ") || l.contains("stp ") {
            profile.stores += 1;
        }
    }

    profile
}

struct VariableVisitor {
    count: usize,
}

impl<'ast> Visit<'ast> for VariableVisitor {
    fn visit_local(&mut self, i: &'ast syn::Local) {
        self.count += 1;
        syn::visit::visit_local(self, i);
    }
    fn visit_item_const(&mut self, i: &'ast syn::ItemConst) {
        self.count += 1;
        syn::visit::visit_item_const(self, i);
    }
    fn visit_item_static(&mut self, i: &'ast syn::ItemStatic) {
        self.count += 1;
        syn::visit::visit_item_static(self, i);
    }
}

struct TargetFnVisitor<'ast> {
    target_line: usize,
    found_item_fn: Option<&'ast syn::ItemFn>,
    found_impl_fn: Option<&'ast syn::ImplItemFn>,
}

impl<'ast> Visit<'ast> for TargetFnVisitor<'ast> {
    fn visit_item_fn(&mut self, i: &'ast syn::ItemFn) {
        let start = i.span().start().line;
        let end = i.span().end().line;
        if self.target_line >= start && self.target_line <= end {
            self.found_item_fn = Some(i);
        }
        syn::visit::visit_item_fn(self, i);
    }
    fn visit_impl_item_fn(&mut self, i: &'ast syn::ImplItemFn) {
        let start = i.span().start().line;
        let end = i.span().end().line;
        if self.target_line >= start && self.target_line <= end {
            self.found_impl_fn = Some(i);
        }
        syn::visit::visit_impl_item_fn(self, i);
    }
}

pub fn analyze_variables(source_file: &Path, target_line: usize) -> usize {
    let Ok(content) = fs::read_to_string(source_file) else {
        return 0;
    };

    if let Ok(file_ast) = syn::parse_file(&content) {
        let mut fn_visitor = TargetFnVisitor {
            target_line,
            found_item_fn: None,
            found_impl_fn: None,
        };
        fn_visitor.visit_file(&file_ast);

        let mut var_visitor = VariableVisitor { count: 0 };
        if let Some(f) = fn_visitor.found_item_fn {
            var_visitor.visit_item_fn(f);
            return var_visitor.count;
        } else if let Some(f) = fn_visitor.found_impl_fn {
            var_visitor.visit_impl_item_fn(f);
            return var_visitor.count;
        }

        var_visitor.visit_file(&file_ast);
        return var_visitor.count;
    }

    let mut count = 0;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("let ") || line.contains(" let ") {
            count += 1;
        }
    }
    count
}

struct ThreadActivityVisitor {
    spawned_vars: Vec<String>,
    joined_vars: Vec<String>,
    has_spawn: bool,
    has_join: bool,
    has_mutex: bool,
    has_rwlock: bool,
    has_atomic: bool,
    has_mpsc: bool,
    has_arc: bool,
}

impl<'ast> Visit<'ast> for ThreadActivityVisitor {
    fn visit_local(&mut self, node: &'ast syn::Local) {
        if let Some(init) = &node.init {
            let is_spawn = match &*init.expr {
                syn::Expr::Call(call)
                    if let syn::Expr::Path(expr_path) = &*call.func
                        && expr_path
                            .path
                            .segments
                            .last()
                            .is_some_and(|seg| seg.ident == "spawn") =>
                {
                    true
                }
                syn::Expr::MethodCall(call) if call.method == "spawn" => true,
                _ => false,
            };
            if is_spawn {
                self.has_spawn = true;
                if let syn::Pat::Ident(pat_ident) = &node.pat {
                    self.spawned_vars.push(pat_ident.ident.to_string());
                }
            }
        }
        syn::visit::visit_local(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(expr_path) = &*node.func
            && let Some(segment) = expr_path.path.segments.last()
            && segment.ident == "spawn"
        {
            self.has_spawn = true;
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let name = node.method.to_string();
        if name == "join" || name == "await" {
            self.has_join = true;
            if let syn::Expr::Path(expr_path) = &*node.receiver
                && let Some(seg) = expr_path.path.segments.last()
            {
                self.joined_vars.push(seg.ident.to_string());
            }
        } else if name == "spawn" {
            self.has_spawn = true;
        }
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_await(&mut self, node: &'ast syn::ExprAwait) {
        self.has_join = true;
        if let syn::Expr::Path(expr_path) = &*node.base
            && let Some(seg) = expr_path.path.segments.last()
        {
            self.joined_vars.push(seg.ident.to_string());
        }
        syn::visit::visit_expr_await(self, node);
    }

    fn visit_type(&mut self, node: &'ast syn::Type) {
        if let syn::Type::Path(type_path) = node {
            if let Some(segment) = type_path.path.segments.first() {
                let name = segment.ident.to_string();
                if name.contains("Mutex") {
                    self.has_mutex = true;
                }
                if name.contains("RwLock") {
                    self.has_rwlock = true;
                }
                if name.contains("Atomic") {
                    self.has_atomic = true;
                }
                if name == "Arc" {
                    self.has_arc = true;
                }
            }
            for segment in &type_path.path.segments {
                if segment.ident == "mpsc" {
                    self.has_mpsc = true;
                }
            }
        }
        syn::visit::visit_type(self, node);
    }

    fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
        for segment in &node.path.segments {
            let name = segment.ident.to_string();
            if name.contains("Mutex") {
                self.has_mutex = true;
            }
            if name.contains("RwLock") {
                self.has_rwlock = true;
            }
            if name.contains("Atomic") {
                self.has_atomic = true;
            }
            if name == "mpsc" {
                self.has_mpsc = true;
            }
            if name == "Arc" {
                self.has_arc = true;
            }
        }
        syn::visit::visit_expr_path(self, node);
    }
}

pub fn analyze_thread_activity(source_file: &Path) -> Vec<String> {
    let mut activities = Vec::new();
    let Ok(content) = fs::read_to_string(source_file) else {
        return activities;
    };

    let Ok(ast) = syn::parse_file(&content) else {
        return activities;
    };

    let mut visitor = ThreadActivityVisitor {
        spawned_vars: Vec::new(),
        joined_vars: Vec::new(),
        has_spawn: false,
        has_join: false,
        has_mutex: false,
        has_rwlock: false,
        has_atomic: false,
        has_mpsc: false,
        has_arc: false,
    };
    visitor.visit_file(&ast);

    if visitor.has_spawn {
        let mut complete = false;
        if visitor.has_join {
            if visitor.spawned_vars.is_empty() {
                // e.g. `thread::spawn(...).join()`
                complete = true;
            } else {
                for var in &visitor.spawned_vars {
                    if visitor.joined_vars.contains(var) {
                        complete = true;
                        break;
                    }
                }
            }
        }
        if complete {
            activities
                .push("Thread/Task Spawning (Lifecycle Complete: join/await found)".to_string());
        } else {
            activities.push(
                "Thread/Task Spawning [WARNING: Lifecycle INCOMPLETE (spawned thread handle not joined)]"
                    .to_string(),
            );
        }
    }
    if visitor.has_mutex {
        activities.push("Mutex synchronization".to_string());
    }
    if visitor.has_rwlock {
        activities.push("RwLock synchronization".to_string());
    }
    if visitor.has_atomic {
        activities.push("Atomic operations".to_string());
    }
    if visitor.has_mpsc {
        activities.push("MPSC Channels".to_string());
    }
    if visitor.has_arc {
        activities.push("Arc reference counting".to_string());
    }

    activities
}

struct CachePaddingVisitor {
    has_padding: bool,
}

impl<'ast> Visit<'ast> for CachePaddingVisitor {
    fn visit_attribute(&mut self, node: &'ast syn::Attribute) {
        if node.path().is_ident("repr")
            && let syn::Meta::List(meta) = &node.meta
            && meta.tokens.to_string().contains("align")
        {
            self.has_padding = true;
        }
        syn::visit::visit_attribute(self, node);
    }

    fn visit_type(&mut self, node: &'ast syn::Type) {
        if let syn::Type::Path(type_path) = node
            && let Some(segment) = type_path.path.segments.last()
        {
            let name = segment.ident.to_string();
            if name == "CachePadded" || name == "cache_padded" {
                self.has_padding = true;
            }
        }
        syn::visit::visit_type(self, node);
    }
}

pub fn analyze_cache_padding(source_file: &Path) -> bool {
    let Ok(content) = fs::read_to_string(source_file) else {
        return false;
    };
    if let Ok(ast) = syn::parse_file(&content) {
        let mut visitor = CachePaddingVisitor { has_padding: false };
        visitor.visit_file(&ast);
        return visitor.has_padding;
    }
    false
}

struct BranchHintVisitor {
    has_hint: bool,
}

impl<'ast> Visit<'ast> for BranchHintVisitor {
    fn visit_attribute(&mut self, node: &'ast syn::Attribute) {
        if node.path().is_ident("cold") {
            self.has_hint = true;
        }
        syn::visit::visit_attribute(self, node);
    }
    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(expr_path) = &*node.func
            && let Some(segment) = expr_path.path.segments.last()
        {
            let name = segment.ident.to_string();
            if name == "likely" || name == "unlikely" {
                self.has_hint = true;
            }
        }
        syn::visit::visit_expr_call(self, node);
    }
}

pub fn analyze_branch_hints(source_file: &Path) -> bool {
    let Ok(content) = fs::read_to_string(source_file) else {
        return false;
    };
    if let Ok(ast) = syn::parse_file(&content) {
        let mut visitor = BranchHintVisitor { has_hint: false };
        visitor.visit_file(&ast);
        return visitor.has_hint;
    }
    false
}

struct AerospaceVisitor {
    in_test: bool,
    has_alloc: bool,
    has_std: bool,
    has_unsafe_allow: bool,
    has_thread_spawn: bool,
    has_heap_containers: bool,
    has_compare_exchange: bool,
    has_load: bool,
    has_spin_loop: bool,
    struct_names: Vec<String>,
    drop_impls: Vec<String>,
}

impl<'ast> Visit<'ast> for AerospaceVisitor {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let is_test = node.attrs.iter().any(|a| {
            if let syn::Meta::List(meta) = &a.meta {
                meta.path.is_ident("cfg") && meta.tokens.to_string().contains("test")
            } else {
                false
            }
        });
        let old = self.in_test;
        if is_test {
            self.in_test = true;
        }
        syn::visit::visit_item_mod(self, node);
        self.in_test = old;
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let is_test = node.attrs.iter().any(|a| a.path().is_ident("test"));
        let old = self.in_test;
        if is_test {
            self.in_test = true;
        }
        syn::visit::visit_item_fn(self, node);
        self.in_test = old;
    }

    fn visit_item_extern_crate(&mut self, node: &'ast syn::ItemExternCrate) {
        if !self.in_test {
            if node.ident == "alloc" {
                self.has_alloc = true;
            }
            if node.ident == "std" {
                self.has_std = true;
            }
        }
        syn::visit::visit_item_extern_crate(self, node);
    }

    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        if !self.in_test {
            match &node.tree {
                syn::UseTree::Path(path) if path.ident == "std" => self.has_std = true,
                syn::UseTree::Path(path) if path.ident == "alloc" => self.has_alloc = true,
                _ => {}
            }
        }
        syn::visit::visit_item_use(self, node);
    }

    fn visit_attribute(&mut self, node: &'ast syn::Attribute) {
        if node.path().is_ident("allow")
            && let syn::Meta::List(meta) = &node.meta
            && meta.tokens.to_string().contains("unsafe_op_in_unsafe_fn")
        {
            self.has_unsafe_allow = true;
        }
        syn::visit::visit_attribute(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(expr_path) = &*node.func
            && let Some(segment) = expr_path.path.segments.last()
        {
            match segment.ident.to_string().as_str() {
                "spawn" => self.has_thread_spawn = true,
                "new"
                    if !self.in_test
                        && expr_path
                            .path
                            .segments
                            .iter()
                            .any(|s| s.ident == "Box" || s.ident == "HashMap") =>
                {
                    self.has_heap_containers = true;
                }
                "with_capacity"
                    if !self.in_test
                        && expr_path.path.segments.iter().any(|s| s.ident == "Vec") =>
                {
                    self.has_heap_containers = true;
                }
                _ => {}
            }
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let name = node.method.to_string();
        if name.contains("compare_exchange") {
            self.has_compare_exchange = true;
        }
        if name == "load" {
            self.has_load = true;
        }
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
        for segment in &node.path.segments {
            if segment.ident == "spin_loop" {
                self.has_spin_loop = true;
            }
        }
        syn::visit::visit_expr_path(self, node);
    }

    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        let name = node.ident.to_string();
        if name.contains("Guard") || name.contains("StateNode") || name.contains("ThreadState") {
            self.struct_names.push(name);
        }
        syn::visit::visit_item_struct(self, node);
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        if let Some((_, path, _)) = &node.trait_
            && path.is_ident("Drop")
            && let syn::Type::Path(type_path) = &*node.self_ty
            && let Some(segment) = type_path.path.segments.last()
        {
            self.drop_impls.push(segment.ident.to_string());
        }
        syn::visit::visit_item_impl(self, node);
    }
}

pub fn analyze_aerospace_grade(source_file: &Path) -> Vec<String> {
    let mut violations = Vec::new();
    let Ok(content) = fs::read_to_string(source_file) else {
        violations.push(format!(
            "Failed to read source file: {}",
            source_file.display()
        ));
        return violations;
    };

    let Ok(ast) = syn::parse_file(&content) else {
        violations.push("Failed to parse file into AST. Strict aerospace grade requires valid Rust syntax.".to_string());
        return violations;
    };

    let mut visitor = AerospaceVisitor {
        in_test: false,
        has_alloc: false,
        has_std: false,
        has_unsafe_allow: false,
        has_thread_spawn: false,
        has_heap_containers: false,
        has_compare_exchange: false,
        has_load: false,
        has_spin_loop: false,
        struct_names: Vec::new(),
        drop_impls: Vec::new(),
    };
    visitor.visit_file(&ast);

    if visitor.has_alloc {
        violations.push(
            "Dynamic memory allocation (`alloc`) is strictly prohibited in aerospace grade."
                .to_string(),
        );
    }
    if visitor.has_std && !source_file.components().any(|c| c.as_os_str() == "tests") {
        violations.push(
            "Standard library (`std`) usage is prohibited. Must be `#![no_std]
use alloc::vec::Vec;`.".to_string(),
        );
    }

    if !source_file.components().any(|c| c.as_os_str() == "tests") && !check_crate_root_no_std() {
        violations.push(
                "Crate root (src/lib.rs or src/main.rs) is missing `#![no_std]
use alloc::vec::Vec;`. Aerospace grade requires strict no_std environment.".to_string(),
            );
    }
    if visitor.has_unsafe_allow {
        violations.push("Suppressing unsafe_op_in_unsafe_fn is prohibited. Must enforce `#![deny(unsafe_op_in_unsafe_fn)]`.".to_string());
    }
    if visitor.has_thread_spawn {
        violations.push("Dynamic thread spawning is prohibited.".to_string());
    }
    if visitor.has_heap_containers {
        violations.push("Heap-allocated containers (`Box`, `Vec`, `HashMap`) are prohibited. Use static fixed-size collections.".to_string());
    }
    if visitor.has_compare_exchange && visitor.has_spin_loop && !visitor.has_load {
        violations.push("Potential Cache Line Bouncing detected! Spinlocks must implement Test-and-Test-and-Set (TTAS) by checking `.load()` before `compare_exchange_weak`.".to_string());
    }

    for s in &visitor.struct_names {
        if !visitor.drop_impls.contains(s) {
            violations.push("Potential Resource Leak: Structs handling state or locks ('Guard', 'StateNode') must explicitly implement `Drop` to ensure deterministic thread resource cleanup.".to_string());
            break;
        }
    }

    violations
}

struct WatchdogVisitor {
    has_watchdog: bool,
}

impl<'ast> Visit<'ast> for WatchdogVisitor {
    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let name = node.method.to_string();
        if name.contains("timeout") || name.contains("watchdog") {
            self.has_watchdog = true;
        }
        syn::visit::visit_expr_method_call(self, node);
    }
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let name = node.sig.ident.to_string();
        if name.contains("timeout") || name.contains("watchdog") {
            self.has_watchdog = true;
        }
        syn::visit::visit_item_fn(self, node);
    }
}

pub fn analyze_watchdog_timeout(source_file: &Path) -> bool {
    if let Ok(content) = fs::read_to_string(source_file)
        && let Ok(ast) = syn::parse_file(&content) {
            let mut visitor = WatchdogVisitor {
                has_watchdog: false,
            };
            visitor.visit_file(&ast);
            return visitor.has_watchdog;
        }
    false
}

struct StressVisitor {
    has_stress: bool,
}

impl<'ast> Visit<'ast> for StressVisitor {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let name = node.sig.ident.to_string();
        if name.contains("stress") || name.contains("fuzzy") || name.contains("heavy_contention") {
            self.has_stress = true;
        }
        syn::visit::visit_item_fn(self, node);
    }
}

pub fn analyze_stress_test(source_file: &Path) -> bool {
    if let Ok(content) = fs::read_to_string(source_file)
        && let Ok(ast) = syn::parse_file(&content) {
            let mut visitor = StressVisitor { has_stress: false };
            visitor.visit_file(&ast);
            return visitor.has_stress;
        }
    false
}

fn check_crate_root_no_std() -> bool {
    let roots = ["src/lib.rs", "src/main.rs"];
    for root in roots {
        if let Ok(content) = fs::read_to_string(root)
            && let Ok(ast) = syn::parse_file(&content)
        {
            for attr in &ast.attrs {
                if let syn::AttrStyle::Inner(_) = attr.style
                    && attr.path().is_ident("no_std")
                {
                    return true;
                }
            }
        }
    }
    false
}
