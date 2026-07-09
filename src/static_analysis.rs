use std::fs;
use std::path::Path;

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

pub fn analyze_variables(source_file: &Path, target_line: usize) -> usize {
    let Ok(content) = fs::read_to_string(source_file) else {
        return 0;
    };

    let mut count = 0;
    let mut in_function = false;
    let mut brace_level = 0;

    let lines: Vec<&str> = content.lines().collect();

    let start_idx = if target_line > 0 && target_line <= lines.len() {
        let mut idx = target_line - 1;
        while idx > 0 {
            if lines[idx].contains("fn ") {
                break;
            }
            idx -= 1;
        }
        idx
    } else {
        0
    };

    for &line in lines.iter().skip(start_idx) {
        let line = line.trim();

        if line.contains("fn ") {
            in_function = true;
        }

        if in_function {
            if line.contains("{") {
                brace_level += line.matches("{").count();
            }
            if line.contains("}") {
                brace_level -= line.matches("}").count();
            }

            if line.starts_with("let ") || line.contains(" let ") {
                count += 1;
            }
            if line.starts_with("const ") || line.contains(" const ") {
                count += 1;
            }
            if line.starts_with("static ") || line.contains(" static ") {
                count += 1;
            }

            if brace_level == 0 && count > 0 {
                break; // End of function
            }
        }
    }

    if count == 0 {
        // Fallback: just count the whole file if function bounding failed
        for line in lines {
            let line = line.trim();
            if line.starts_with("let ") || line.contains(" let ") {
                count += 1;
            }
        }
    }

    count
}

pub fn analyze_thread_activity(source_file: &Path) -> Vec<String> {
    let mut activities = Vec::new();
    let Ok(content) = fs::read_to_string(source_file) else {
        return activities;
    };

    let has_spawn = content.contains("thread::spawn")
        || content.contains("tokio::spawn")
        || content.contains("async_std::task::spawn");
    let has_join = content.contains(".join()") || content.contains(".await");

    if has_spawn {
        if has_join {
            activities
                .push("Thread/Task Spawning (Lifecycle Complete: join/await found)".to_string());
        } else {
            activities.push(
                "Thread/Task Spawning [WARNING: Lifecycle INCOMPLETE (no join/await found)]"
                    .to_string(),
            );
        }
    }

    if content.contains("Mutex") {
        activities.push("Mutex synchronization".to_string());
    }
    if content.contains("RwLock") {
        activities.push("RwLock synchronization".to_string());
    }
    if content.contains("Atomic") {
        activities.push("Atomic operations".to_string());
    }
    if content.contains("mpsc::") {
        activities.push("MPSC Channels".to_string());
    }
    if content.contains("Arc<") {
        activities.push("Arc reference counting".to_string());
    }

    activities
}

pub fn analyze_cache_padding(source_file: &Path) -> bool {
    let Ok(content) = fs::read_to_string(source_file) else {
        return false;
    };

    // Look for common cache padding or alignment attributes
    content.contains("#[repr(align")
        || content.contains("CachePadded")
        || content.contains("cache_padded")
        || content.contains("crossbeam_utils::CachePadded")
}

pub fn analyze_branch_hints(source_file: &Path) -> bool {
    let Ok(content) = fs::read_to_string(source_file) else {
        return false;
    };

    // Look for common branch prediction hints
    content.contains("std::intrinsics::likely")
        || content.contains("std::intrinsics::unlikely")
        || content.contains("core::intrinsics::likely")
        || content.contains("core::intrinsics::unlikely")
        || content.contains("#[cold]")
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

    let test_stripped_content = if let Some(idx) = content.find("#[cfg(test)]") {
        &content[..idx]
    } else {
        &content
    };

    if test_stripped_content.contains("extern crate alloc") {
        violations.push(
            "Dynamic memory allocation (`alloc`) is strictly prohibited in aerospace grade."
                .to_string(),
        );
    }

    // We do a simple check for standard library that avoids false positives like "no_std"
    if test_stripped_content.contains("use std::")
        || test_stripped_content.contains("extern crate std")
    {
        violations.push(
            "Standard library (`std`) usage is prohibited. Must be `#![no_std]`.".to_string(),
        );
    }

    if content.contains("#[allow(unsafe_op_in_unsafe_fn)]") {
        violations.push("Suppressing unsafe_op_in_unsafe_fn is prohibited. Must enforce `#![deny(unsafe_op_in_unsafe_fn)]`.".to_string());
    }

    if content.contains("thread::spawn") || content.contains("tokio::spawn") {
        violations.push("Dynamic thread spawning is prohibited.".to_string());
    }

    if content.contains("Box::new")
        || content.contains("Vec::with_capacity")
        || content.contains("HashMap::new")
    {
        violations.push("Heap-allocated containers (`Box`, `Vec`, `HashMap`) are prohibited. Use static fixed-size collections.".to_string());
    }

    if content.contains("compare_exchange")
        && content.contains("spin_loop")
        && !content.contains(".load(")
    {
        violations.push("Potential Cache Line Bouncing detected! Spinlocks must implement Test-and-Test-and-Set (TTAS) by checking `.load()` before `compare_exchange_weak`.".to_string());
    }

    if content.contains("struct ")
        && (content.contains("Guard")
            || content.contains("StateNode")
            || content.contains("ThreadState"))
        && !content.contains("impl Drop for")
        && !content.contains("impl<")
        && !content.contains("Drop for")
    {
        // Simple heuristic for missing drop
        if !content.contains("impl Drop")
            && !content.contains("impl<T> Drop")
            && !content.contains("impl<'a> Drop")
        {
            violations.push("Potential Resource Leak: Structs handling state or locks ('Guard', 'StateNode') must explicitly implement `Drop` to ensure deterministic thread resource cleanup.".to_string());
        }
    }

    violations
}
