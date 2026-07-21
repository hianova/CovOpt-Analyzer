use crate::analyzer::ConvergenceAnalyzer;
use crate::config::CovOptConfig;
use crate::entropy;
use crate::mca::McaRunner;
use crate::runner::CargoTestRunner;
use crate::*;
use std::fs;
use std::path::{Path, PathBuf};

fn parse_complexity(s: &str) -> Complexity {
    match s.to_uppercase().as_str() {
        "O1" | "O(1)" => Complexity::O1,
        "OLOGN" | "O(LOGN)" => Complexity::OLogN,
        "ON" | "O(N)" => Complexity::ON,
        "ONLOGN" | "O(NLOGN)" => Complexity::ONLogN,
        "ON2" | "O(N2)" | "O(N^2)" => Complexity::ON2,
        "O2N" | "O(2^N)" | "O(2N)" => Complexity::O2N,
        "OSQRTN" | "O(SQRT(N))" | "O(SQRTN)" => Complexity::OSqrtN,
        _ => panic!("Unknown complexity: {}", s),
    }
}

struct LogBuffer {
    buffer: String,
    compact: bool,
}

impl LogBuffer {
    fn new(compact: bool) -> Self {
        Self {
            buffer: String::new(),
            compact,
        }
    }
}

macro_rules! wlog {
    ($log:expr, $($arg:tt)*) => {{
        let s = format!($($arg)*);
        if !$log.compact {
            println!("{}", s);
        }
        $log.buffer.push_str(&s);
        $log.buffer.push('\n');
    }};
}

pub fn run_analysis(args: &RunArgs, compact: bool, workspace_executables: Option<&[PathBuf]>) -> bool {
    let mut log = LogBuffer::new(compact);

    let test_name = match args.test.as_ref() {
        Some(t) => t.as_str(),
        None => {
            wlog!(
                log,
                "[ERROR] --test is required or must be configured in .covopt.toml"
            );
            return false;
        }
    };
    let mut ast_expected = None;
    let mut ast_n_values = None;
    if args.expected.is_none() || args.n_values.is_none() {
        if let Some((e, n, _)) = crate::static_analysis::find_covopt_test_metadata(test_name) {
            ast_expected = Some(e);
            ast_n_values = Some(n);
        }
    }

    let expected_str = match args.expected.as_ref().or(ast_expected.as_ref()) {
        Some(e) => e,
        None => {
            wlog!(
                log,
                "[ERROR] --expected is required, must be configured in .covopt.toml, or provided via #[covopt::test(expected = \"...\")]"
            );
            return false;
        }
    };
    let n_values_str = match args.n_values.as_ref().or(ast_n_values.as_ref()) {
        Some(n) => n,
        None => {
            wlog!(
                log,
                "[ERROR] --n-values is required, must be configured in .covopt.toml, or provided via #[covopt::test(n_values = \"...\")]"
            );
            return false;
        }
    };
    let mut discovered_target_file: Option<String> = None;
    let mut discovered_target_line: Option<u64> = None;
    let mut target_symbol: Option<String> = None;

    let expected = parse_complexity(expected_str);

    let _n_values: Vec<usize> = n_values_str
        .split(',')
        .map(|s| s.trim().parse().unwrap_or(0))
        .collect();

    let output_dir_temp = tempfile::tempdir().map_err(|e| {
        wlog!(log, "[ERROR] Failed to create tempdir: {}", e);
    });
    if output_dir_temp.is_err() {
        return false;
    }
    let output_dir = output_dir_temp.unwrap().path().to_path_buf();

    let executables = if let Some(exes) = workspace_executables {
        exes.to_vec()
    } else {
        let mut packages_to_compile = Vec::new();
        if let Some(pkg) = crate::static_analysis::resolve_package_for_target(test_name, None) {
            packages_to_compile.push(pkg);
        }
        match crate::runner::compile_workspace_tests(&output_dir, &packages_to_compile) {
            Ok(exes) => exes,
            Err(e) => {
                wlog!(log, "[ERROR] Failed to compile workspace tests: {}", e);
                return false;
            }
        }
    };

    let runner = std::sync::Arc::new(CargoTestRunner::new(test_name, &output_dir, executables));

    wlog!(log, "Starting CovOpt Analysis for test '{}'...", test_name);
    wlog!(log, "Target: Auto-Discovery Mode");
    wlog!(log, "Expected Complexity: {:?}", expected);

    let mut data = Vec::new();
    let mut space_data = Vec::new();
    let mut target_coverage_rate = None;
    let mut mca_stats = None;

    let mut handles = Vec::new();

    for n_str in n_values_str.split(',') {
        let n: u64 = n_str.trim().parse().unwrap_or(0);
        let runner_clone = std::sync::Arc::clone(&runner);
        
        handles.push(std::thread::spawn(move || {
            let result = runner_clone.run(n as usize, None);
            (n, result)
        }));
    }

    let mut results = Vec::new();
    for handle in handles {
        match handle.join() {
            Ok((n, result)) => results.push((n, result)),
            Err(_) => {
                wlog!(log, "[ERROR] A worker thread panicked during execution.");
                return false;
            }
        }
    }
    
    // Sort results to process them sequentially
    results.sort_by_key(|(n, _)| *n);

    for (n, result) in results {
        wlog!(log, "---------------------------------------------------");
        wlog!(log, "Running for N = {}...", n);

        let (map, peak_rss) = match result {
            Ok(m) => m,
            Err(e) => {
                wlog!(log, "[ERROR] Failed to run coverage for N={}: {}", n, e);
                if compact {
                    println!("\n=== DETAILED ANALYSIS LOG (FAILURE) ===");
                    println!("{}", log.buffer);
                    println!("========================================\n");
                }
                return false;
            }
        };

        if target_symbol.is_none() {
            let anchor = crate::static_analysis::find_covopt_track_anchor();
            if let Some((anchor_f, anchor_l)) = anchor {
                let filename_suffix = std::path::Path::new(&anchor_f)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&anchor_f);

                wlog!(log, "DEBUG: Found anchor at {}:{}", anchor_f, anchor_l);

                if let Some(sym) = map.find_symbol(filename_suffix, anchor_l) {
                    discovered_target_file = Some(anchor_f.clone());
                    discovered_target_line = Some(anchor_l);
                    target_symbol = Some(sym.clone());
                    wlog!(
                        log,
                        "Anchored target via covopt_track! at {}:{} ({})",
                        anchor_f,
                        anchor_l,
                        sym
                    );
                } else {
                    wlog!(
                        log,
                        "DEBUG: find_symbol returned None for {}, {}",
                        filename_suffix,
                        anchor_l
                    );
                }
            } else {
                wlog!(log, "DEBUG: find_covopt_track_anchor returned None");
            }
        }

        if target_symbol.is_none() {
            let mut ignore_patterns = Vec::new();
            if let Some(ig_str) = &args.ignore {
                ignore_patterns.extend(ig_str.split(',').map(|s| s.trim().to_string()));
            }
            if let Some((f, l, sym, _)) = map.find_peak_location(&ignore_patterns) {
                discovered_target_file = Some(f.clone());
                discovered_target_line = Some(l);
                target_symbol = Some(sym.clone());
                wlog!(log, "Auto-discovered target: {}:{} ({})", f, l, sym);
            }
        }

        let hit_count = if let Some(f) = &discovered_target_file {
            map.get_hit_count(f, discovered_target_line.unwrap_or(0))
        } else {
            None
        };

        if let Some(h) = hit_count {
            wlog!(
                log,
                "  -> Hit count = {} | Peak RSS = {} bytes",
                h,
                peak_rss
            );
            data.push((n as usize, h));

            if h == 0 {
                wlog!(log, "\n> [!WARNING] COVOPT GUIDANCE: HIT COUNT = 0 <");
                wlog!(
                    log,
                    "The target function was executed 0 times during profiling, but the test succeeded."
                );
                wlog!(
                    log,
                    "This often happens to pure math functions due to LLVM Auto-Vectorization or Dead Code Elimination (DCE)."
                );
                wlog!(
                    log,
                    "=> SUGGESTION: Add `#[inline(never)]` to the target function during testing, and ensure loop variables are wrapped with `std::hint::black_box()`."
                );
            }
        } else {
            wlog!(log, "  -> WARNING: No hit count found. Assuming 0.");
            data.push((n as usize, 0));
        }
        space_data.push((n as usize, peak_rss));

        if let Some(ref sym) = target_symbol {
            target_coverage_rate = map.get_function_coverage(sym);
        }
    }

    let target_file = discovered_target_file.unwrap_or_else(|| "src/lib.rs".to_string());
    let target_line = discovered_target_line.unwrap_or(0);
    wlog!(log, "---------------------------------------------------");
    wlog!(log, "Time Analysis Results:");
    let report = ConvergenceAnalyzer::analyze(&data, expected);
    wlog!(log, "{:#?}", report);

    wlog!(log, "---------------------------------------------------");
    wlog!(log, "Space Analysis Results (Dynamic Memory):");
    let space_report = ConvergenceAnalyzer::analyze(&space_data, Complexity::O1);
    wlog!(
        log,
        "  -> Actual Space Complexity: {:?}",
        space_report.actual_trend
    );

    if args.formalize {
        wlog!(log, "---------------------------------------------------");
        wlog!(
            log,
            "🔮 [Heuristic Engine] Lean 4 Mode: Synthesizing Formal Mathematical AST Proof..."
        );
        let exact_formula = heuristic::SymbolicRegressor::formalize(&data);
        wlog!(log, "  => Formal Proof Discovered: {}", exact_formula);
    }

    let var_count = static_analysis::analyze_variables(
        std::path::Path::new(&target_file),
        target_line as usize,
    );
    wlog!(log, "Static Variable Declarations: {}", var_count);

    let thread_activities =
        static_analysis::analyze_thread_activity(std::path::Path::new(&target_file));
    if !thread_activities.is_empty() {
        wlog!(log, "Static Thread Activities:");
        for act in thread_activities {
            wlog!(log, "  - {}", act);
        }
    } else {
        wlog!(log, "Static Thread Activities: None");
    }

    let mut success = true;

    if report.is_converged && report.actual_trend > expected {
        wlog!(
            log,
            "\n[ERROR] Algorithm complexity degraded! Expected {:?}, got {:?}",
            expected,
            report.actual_trend
        );
        success = false;
    }

    let mut static_cache_padding = None;
    if args.require_cache_padding {
        let (has_padding, applicable) =
            static_analysis::analyze_cache_padding(std::path::Path::new(&target_file));
        static_cache_padding = Some(has_padding);
        if applicable {
            if has_padding {
                wlog!(log, "Static Cache Padding: Detected");
            } else {
                wlog!(
                    log,
                    "\n[ERROR] Missing Cache Padding! Strict mode requires cache alignment for target."
                );
                success = false;
            }
        } else {
            static_cache_padding = Some(true); // Treat as passed
            wlog!(log, "Static Cache Padding: Not Applicable (Pure Function)");
        }
    }

    let mut static_branch_hints = None;
    if args.require_branch_hints {
        let (has_hints, applicable) =
            static_analysis::analyze_branch_hints(std::path::Path::new(&target_file));
        static_branch_hints = Some(has_hints);
        if applicable {
            if has_hints {
                wlog!(log, "Static Branch Hints: Detected");
            } else {
                wlog!(
                    log,
                    "\n[ERROR] Missing Branch Prediction Hints! Strict mode requires likely/unlikely markers for target."
                );
                success = false;
            }
        } else {
            static_branch_hints = Some(true); // Treat as passed
            wlog!(log, "Static Branch Hints: Not Applicable (Pure Function)");
        }
    }

    let mut static_aerospace_grade = None;
    if args.require_aerospace_grade {
        let violations =
            static_analysis::analyze_aerospace_grade(std::path::Path::new(&target_file));
        static_aerospace_grade = Some(violations.clone());
        if violations.is_empty() {
            wlog!(log, "Static Aerospace Grade: Passed");
        } else {
            wlog!(
                log,
                "\n[ERROR] Aerospace Grade Violations Detected in {}!",
                target_file
            );
            for v in violations {
                wlog!(log, "  - {}", v);
            }
            success = false;
        }
    }

    let mut static_watchdog_timeout = None;
    if args.require_watchdog_timeout {
        let (has_watchdog, applicable) =
            static_analysis::analyze_watchdog_timeout(std::path::Path::new(&target_file));
        static_watchdog_timeout = Some(has_watchdog);
        if applicable {
            if has_watchdog {
                wlog!(log, "Static Watchdog Timeout: Detected");
            } else {
                wlog!(
                    log,
                    "\n[ERROR] Missing Watchdog Timeout! Strict mode requires timeout mechanisms (e.g. recv_timeout) to prevent infinite spin deadlocks."
                );
                success = false;
            }
        } else {
            static_watchdog_timeout = Some(true); // Treat as passed
            wlog!(
                log,
                "Static Watchdog Timeout: Not Applicable (Pure Function)"
            );
        }
    }

    let mut static_stress_test = None;
    if args.require_stress_test {
        let (has_stress, applicable) =
            static_analysis::analyze_stress_test(std::path::Path::new(&target_file));
        static_stress_test = Some(has_stress);
        if applicable {
            if has_stress {
                wlog!(log, "Static Stress Test: Detected");
            } else {
                wlog!(
                    log,
                    "\n[ERROR] Missing High-Pressure Stress Test! Target file lacks heavy concurrent thread spawning logic."
                );
                success = false;
            }
        } else {
            static_stress_test = Some(true); // Treat as passed
            wlog!(log, "Static Stress Test: Not Applicable (Pure Function)");
        }
    }

    let mut coverage_rate_val = None;
    wlog!(log, "---------------------------------------------------");
    if let Some(symbol) = target_symbol {
        if let Some((executed, total)) = target_coverage_rate {
            let rate = (executed as f64 / total as f64) * 100.0;
            coverage_rate_val = Some(rate);
            wlog!(
                log,
                "Coverage Rate (Target Function): {:.1}% ({}/{} lines)",
                rate,
                executed,
                total
            );
            if rate < 90.0 {
                wlog!(
                    log,
                    "[WARNING] Function coverage is below 90%. The measured mathematical complexity might not reflect the worst-case scenario. Consider adding more branches to your test."
                );
                success = false; // Fail audit if coverage is below 90%
            }
            wlog!(log, "---------------------------------------------------");
        }

        wlog!(log, "Target Symbol Found: {}", symbol);
        wlog!(log, "Extracting ASM and running LLVM-MCA analysis...");

        match runner.compile_asm() {
            Ok(asm_content) => {
                let mut asm_block_opt =
                    runner.extract_asm_block_by_loc(&asm_content, &target_file, target_line);
                if asm_block_opt.is_none() {
                    asm_block_opt = runner.extract_asm_block(&asm_content, &symbol);
                }
                if asm_block_opt.is_none() {
                    let demangled = rustc_demangle::demangle(&symbol).to_string();
                    let clean_demangled = if demangled.ends_with('>') && demangled.contains("::<") {
                        let idx = demangled.rfind("::<").unwrap_or(demangled.len());
                        &demangled[..idx]
                    } else {
                        &demangled
                    };
                    let parts: Vec<&str> = clean_demangled.split("::").collect();
                    if parts.len() >= 2 {
                        let fn_name = parts
                            .last()
                            .unwrap_or(&"")
                            .split('<')
                            .next()
                            .unwrap_or("")
                            .trim();
                        let struct_part = parts[parts.len() - 2];
                        let struct_name = struct_part
                            .split('<')
                            .next()
                            .unwrap_or("")
                            .split('[')
                            .next()
                            .unwrap_or("")
                            .trim()
                            .trim_matches(['<', '>', '[', ']']);

                        wlog!(
                            log,
                            "  -> Target symbol exact match failed. Searching by keywords: '{}', '{}'...",
                            struct_name,
                            fn_name
                        );
                        let t_extract = std::time::Instant::now();
                        asm_block_opt = runner
                            .extract_asm_block_by_keywords(&asm_content, &[struct_name, fn_name]);
                        println!(
                            "[Profile] extract_asm_block_by_keywords 2: {:?}",
                            t_extract.elapsed()
                        );
                    }
                    if asm_block_opt.is_none() {
                        wlog!(
                            log,
                            "  -> Still not found. Target symbol inlined. Walking up to test caller '{}'...",
                            test_name
                        );
                        let t_extract = std::time::Instant::now();
                        asm_block_opt =
                            runner.extract_asm_block_by_keywords(&asm_content, &[test_name]);
                        println!(
                            "[Profile] extract_asm_block_by_keywords 3: {:?}",
                            t_extract.elapsed()
                        );
                    }
                }

                if let Some(asm_block) = asm_block_opt {
                    let mem_profile = static_analysis::analyze_memory_ops(&asm_block);
                    wlog!(log, "\n[Static Memory Operations]");
                    wlog!(log, "Loads:  {}", mem_profile.loads);
                    wlog!(log, "Stores: {}", mem_profile.stores);
                    wlog!(log, "Allocs: {}", mem_profile.allocs);

                    let mca_runner = McaRunner::new(args.mca_cpu.clone());
                    let t_mca = std::time::Instant::now();
                    match mca_runner.run(&asm_block) {
                        Ok(mca_report) => {
                            println!("[Profile] llvm-mca: {:?}", t_mca.elapsed());
                            wlog!(log, "\n[MCA Report]");

                            wlog!(
                                log,
                                "Block RThroughput: {:.2}",
                                mca_report.block_rthroughput
                            );
                            wlog!(log, "IPC:               {:.2}", mca_report.ipc);
                            mca_stats = Some((mca_report.ipc, mca_report.block_rthroughput));
                        }
                        Err(e) => wlog!(log, "LLVM-MCA failed: {}", e),
                    }

                    if args.optimize {
                        wlog!(
                            log,
                            "\n🚀 [Superoptimization] Launching NP-hard Discrete Diffusion Engine..."
                        );
                        let optimizer = crate::optimizer::DiscreteDiffusionEngine::new(20);
                        let base_asm_lines: Vec<String> =
                            asm_block.lines().map(|s| s.to_string()).collect();

                        let optimized_asm =
                            optimizer.optimize_asm(base_asm_lines, 20, args.mca_cpu.clone());
                        let optimized_text = optimized_asm.join("\n");

                        wlog!(log, "\n[Optimizer Output] Best ASM schedule found:");
                        wlog!(log, "{}", optimized_text);

                        if let Ok(opt_report) = mca_runner.run(&optimized_text) {
                            wlog!(log, "\n[Optimized MCA Report]");
                            wlog!(
                                log,
                                "Block RThroughput: {:.2}",
                                opt_report.block_rthroughput
                            );
                            wlog!(log, "IPC:               {:.2}", opt_report.ipc);
                        }
                    }
                } else {
                    wlog!(
                        log,
                        "Could not extract ASM block for symbol. The function might be inlined in release mode."
                    );
                }
            }
            Err(e) => wlog!(log, "ASM compilation failed: {}", e),
        }
    } else {
        wlog!(
            log,
            "Could not extract target symbol name from coverage data. Skipping MCA analysis."
        );
    }

    // --- Energy / Thermal Guidance (High Frequency Polling Detection) ---
    let max_hit_count = data.iter().map(|&(_, h)| h).max().unwrap_or(0);
    let max_n = data.iter().map(|&(n, _)| n).max().unwrap_or(1);
    let threshold = args.polling_threshold.unwrap_or(50_000);

    if max_hit_count > threshold && max_hit_count > (max_n as u64) * 100 {
        wlog!(
            log,
            "\n> [!CAUTION] COVOPT GUIDANCE: THERMAL & ENERGY WARNING <"
        );
        wlog!(
            log,
            "Detected astronomically high hit count ({}) relative to workload (N={}).",
            max_hit_count,
            max_n
        );
        wlog!(
            log,
            "This indicates 'High-Frequency Invalid Polling' (Busy-waiting) in a loop, which will cause 100% single-core CPU usage and severe device overheating."
        );
        wlog!(
            log,
            "=> SUGGESTION: Introduce an adaptive sleep (`std::thread::sleep`), Exponential Backoff, or an OS Yield (`core::hint::spin_loop()` is NOT enough to prevent heating) inside the empty polling branch."
        );
    }

    if success {
        if compact {
            println!("\n> [x] CovOpt Analysis PASSED (Target: {})", target_file);
            println!(
                "  - Time Complexity: {:?} (Expected: {:?})",
                report.actual_trend, expected
            );
            println!("  - Space Complexity: {:?}", space_report.actual_trend);

            let mut checks = Vec::new();
            if args.require_cache_padding {
                checks.push(format!(
                    "Cache Padding: {}",
                    if static_cache_padding.unwrap_or(false) {
                        "Yes"
                    } else {
                        "No"
                    }
                ));
            }
            if args.require_branch_hints {
                checks.push(format!(
                    "Branch Hints: {}",
                    if static_branch_hints.unwrap_or(false) {
                        "Yes"
                    } else {
                        "No"
                    }
                ));
            }
            if args.require_aerospace_grade {
                checks.push(format!(
                    "Aerospace: {}",
                    if static_aerospace_grade.as_ref().is_none_or(|v| v.is_empty()) {
                        "Passed"
                    } else {
                        "Failed"
                    }
                ));
            }
            if args.require_watchdog_timeout {
                checks.push(format!(
                    "Watchdog: {}",
                    if static_watchdog_timeout.unwrap_or(false) {
                        "Yes"
                    } else {
                        "No"
                    }
                ));
            }
            if args.require_stress_test {
                checks.push(format!(
                    "Stress Test: {}",
                    if static_stress_test.unwrap_or(false) {
                        "Yes"
                    } else {
                        "No"
                    }
                ));
            }
            if !checks.is_empty() {
                println!("  - Static Checks: {}", checks.join(", "));
            } else {
                println!("  - Static Checks: None Required");
            }

            if let Some(rate) = coverage_rate_val {
                println!("  - Function Coverage: {:.1}%", rate);
            }
            if let Some((ipc, rt)) = mca_stats {
                println!(
                    "  - LLVM-MCA (Static Block): IPC {:.2}, RThroughput {:.2}",
                    ipc, rt
                );
            }
        }
    } else {
        if compact {
            println!("\n=== DETAILED ANALYSIS LOG (FAILURE) ===");
            println!("{}", log.buffer);
            println!("========================================\n");
        }
    }

    success
}

pub fn install_hook() {
    let hook_path = PathBuf::from(".git/hooks/pre-commit");
    let hook_content = r#"#!/bin/sh
echo "Running CovOpt-Analyzer on local commit..."
if [ -f .covopt.toml ]; then
    covopt audit
    if [ $? -ne 0 ]; then
        echo "CovOpt-Analyzer: Commit rejected due to complexity degradation or low coverage."
        exit 1
    fi
else
    echo "CovOpt-Analyzer: .covopt.toml not found, skipping audit."
fi
"#;
    if let Err(e) = fs::write(&hook_path, hook_content) {
        eprintln!("Failed to write hook to {}: {}", hook_path.display(), e);
        std::process::exit(1);
    }
    // Set executable bit
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&hook_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&hook_path, perms).unwrap();
    }
    println!(
        "Successfully installed pre-commit hook to {}",
        hook_path.display()
    );
}

const COVOPT_AGENT_RULES: &str = r#"# CovOpt Optimization & Tuning Rules (Google Antigravity)

**Usage**: AI Agent instructions for performance verification and parameter tuning.

## Rule 1: Zero-Entropy Tuning
- **NEVER** use hardcoded magical numbers for caching thresholds, buffer sizes, or critical performance parameters.
- **ALWAYS** use the `covopt_param!` macro to define these parameters.
- Before submitting performance-related code, you MUST run `covopt audit` and ensure the Entropy Score is `0.0`. If it is not, use `covopt optimize` or `covopt optimize --explore` to converge the parameters.

## Rule 2: Anti-DCE in Benchmarks
- When writing tests or benchmarks for `covopt` or `llvm-mca`, **ALWAYS** wrap loop variables and return values with `std::hint::black_box()`.
- This prevents LLVM Dead Code Elimination (DCE) from optimizing `O(N)` loops into `O(1)` during `--release` coverage builds.

## Rule 3: Lock-Free Critical Paths
- For extreme performance tiers (e.g., `< 50ns` p50 latency), **NEVER** use standard library `Mutex` or `RwLock` on the critical path.
- **ALWAYS** use QSBR (Quiescent State Based Reclamation) or Thread Local Storage (TLS) for thread-safe memory and state management.

## Rule 4: Strict Clippy Cleanliness
- All macro-generated code from `covopt_param!` must be strictly typed. **DO NOT** use `#[allow(...)]` to ignore `as u8` or `as u16` cast warnings. Fix the underlying type inference instead.

## Rule 5: No Magic Numbers
- All magic numbers must be uniformly defined using `covopt_parm!`.

## Available Commands (CovOpt-Analyzer)
- `covopt init`: Initializes a `.covopt.toml` and injects these rules. Use `--hook` to install a pre-commit hook.
- `covopt ci`: Unified Auto-Pilot Pipeline (Fix -> Audit -> Optimize).
- `covopt fix`: Automatically fix Clippy warnings and formatting.
- `covopt report`: Generate an HTML dashboard report.

- `covopt check audit`: Fast, low-entropy verification checking with quiet checklist output.
- `covopt check magic`: Scan Rust files for hardcoded magic numbers.
- `covopt check advise`: Advanced qualitative analysis. Detect micro-architectural pipeline stalls, God Functions, and cross-file Semantic Clones.

- `covopt tune params`: Performance Parameter Auto-Tuning & Optimization.
- `covopt tune profile`: Automatically parses flamegraph SVGs into text-based CPU hotspots for AI tuning.
- `covopt tune layout`: Tune struct memory layouts for cache efficiency.
- `covopt tune vectorize`: Scan for SIMD auto-vectorization opportunities.
- `covopt tune pgo`: Inject dynamic PGO (likely/unlikely) probes based on coverage.
- `covopt tune refactor`: Scaffold Advanced AI Refactoring (O(N^2) -> O(N log N)).

- `covopt harden run`: Robustness & Security Hardening (Mutation, Fuzzing, Sanitizers). Use `--sanitize --auto-fix` to automate self-healing.
- `covopt harden fuzz`: Generate fuzzing harnesses for public functions.

- `covopt --test <TEST> --expected <EXPECTED>`: Runs a direct mathematical complexity analysis on a specific test target.
- `covopt --help`: View all available commands and detailed usage instructions.
"#;

pub fn init_config(args: crate::InitArgs) {
    if let Some(p) = args.path
        && let Err(e) = std::env::set_current_dir(&p)
    {
        eprintln!("Failed to change directory to {}: {}", p, e);
        std::process::exit(1);
    }
    let config_path = std::path::PathBuf::from(".covopt.toml");
    let has_config = config_path.exists();
    if has_config {
        println!(
            "CovOpt-Analyzer: .covopt.toml already exists. Skipping config creation, but will ensure rules are injected."
        );
    } else {
        use std::io::Write;
        let require_aerospace = if args.yes {
            false
        } else {
            print!("Enable Aerospace Grade checks? [y/N]: ");
            std::io::stdout().flush().unwrap();
            let mut input = String::new();
            std::io::stdin().read_line(&mut input).unwrap();
            input.trim().eq_ignore_ascii_case("y")
        };

        let default_config = format!(
            r#"[[target]]
test = "my_benchmark_test"
expected = "O(1)"
n_values = "1,500,10000"
require_cache_padding = true
require_branch_hints = true
require_aerospace_grade = {}
require_watchdog_timeout = true
require_stress_test = true
"#,
            require_aerospace
        );

        if let Err(e) = std::fs::write(&config_path, default_config) {
            eprintln!("Failed to write .covopt.toml: {}", e);
            std::process::exit(1);
        }
        println!("Successfully initialized .covopt.toml. Please edit it to match your target.");
    }

    // Append to .gitignore
    if let Ok(mut content) = std::fs::read_to_string(".gitignore") {
        if !content.contains(".covopt/") {
            if !content.ends_with('\n') && !content.is_empty() {
                content.push('\n');
            }
            content.push_str(".covopt/\n");
            let _ = std::fs::write(".gitignore", content);
            println!("Added .covopt/ to .gitignore.");
        }
    } else {
        let _ = std::fs::write(".gitignore", ".covopt/\n");
        println!("Created .gitignore and added .covopt/.");
    }

    // Append to Cargo.toml exclude
    if let Ok(mut content) = std::fs::read_to_string("Cargo.toml")
        && !content.contains("\".covopt/\"")
        && !content.contains("'.covopt/'")
    {
        if let Some(idx) = content.find("exclude = [") {
            let insert_pos = idx + "exclude = [".len();
            content.insert_str(insert_pos, "\".covopt/\", ");
            let _ = std::fs::write("Cargo.toml", content);
            println!("Added .covopt/ to exclude array in Cargo.toml.");
        } else if let Some(idx) = content.find("[package]") {
            let end_idx = content[idx..]
                .find("\n[")
                .map(|i| idx + i)
                .unwrap_or(content.len());
            content.insert_str(end_idx, "\nexclude = [\".covopt/\"]\n");
            let _ = std::fs::write("Cargo.toml", content);
            println!("Added exclude = [\".covopt/\"] to Cargo.toml [package] section.");
        }
    }

    // Inject AI Agent Rules
    let agents_dir = Path::new(".agents");
    let rules_dir = agents_dir.join("rules");

    if let Err(e) = std::fs::create_dir_all(&rules_dir) {
        eprintln!("Failed to create .agents/rules directory: {}", e);
    } else {
        let rule_file = rules_dir.join("covopt-rules.md");
        if let Err(e) = std::fs::write(&rule_file, COVOPT_AGENT_RULES) {
            eprintln!("Failed to write rule file {:?}: {}", rule_file, e);
        } else {
            println!("Injected AI agent rules to {:?}.", rule_file);
        }

        let agents_md = agents_dir.join("AGENTS.md");
        let current_agents_md = std::fs::read_to_string(&agents_md).unwrap_or_default();

        // Remove the old block if it exists
        let mut new_agents_md = current_agents_md.clone();
        if let Some(start_idx) =
            new_agents_md.find("# CovOpt Optimization & Tuning Rules (Google Antigravity)")
        {
            // Skip the current header and find the next top-level header (e.g., "\n# ")
            if let Some(end_offset) = new_agents_md[start_idx + 2..].find("\n# ") {
                let end_idx = start_idx + 2 + end_offset;
                // There's another rule block after this one, replace just this block
                new_agents_md.replace_range(start_idx..end_idx, "");
            } else {
                // It's the last rule block, truncate from start_idx
                new_agents_md.truncate(start_idx);
            }
        }

        if !new_agents_md.ends_with('\n') && !new_agents_md.is_empty() {
            new_agents_md.push('\n');
        }
        new_agents_md.push('\n');
        new_agents_md.push_str(COVOPT_AGENT_RULES);
        new_agents_md.push('\n');

        if let Err(e) = std::fs::write(&agents_md, new_agents_md) {
            eprintln!("Failed to update {:?}: {}", agents_md, e);
        } else {
            println!("Updated CovOpt rules in {:?}.", agents_md);
        }
    }
}

pub fn run_fix() {
    println!("CovOpt-Analyzer: Running auto-correction (cargo clippy --fix)...");
    let status = std::process::Command::new("cargo")
        .args([
            "clippy",
            "--fix",
            "--allow-dirty",
            "--allow-no-vcs",
            "--all-targets",
        ])
        .status()
        .expect("Failed to execute cargo clippy");

    if status.success() {
        println!("CovOpt-Analyzer: Auto-correction completed successfully.");
    } else {
        eprintln!(
            "CovOpt-Analyzer: Auto-correction encountered errors (some issues might need manual fixing)."
        );
        std::process::exit(1);
    }
}

pub fn run_audit() {
    unsafe {
        std::env::set_var("COVOPT_COMPACT", "1");
    }
    let config_path = ".covopt.toml";
    if !PathBuf::from(config_path).exists() {
        eprintln!("CovOpt-Analyzer: Config file {} not found.", config_path);
        eprintln!("Please run `covopt init` to initialize the project first.");
        std::process::exit(1);
    }

    let config = match CovOptConfig::load(config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };

    let global_output_dir = tempfile::tempdir().unwrap().path().to_path_buf();
    println!("CovOpt-Analyzer: Resolving packages for Batch Compilation Mode...");
    
    let mut packages_to_compile = Vec::new();
    for target in &config.target {
        if let Some(pkg) = crate::static_analysis::resolve_package_for_target(&target.test, target.package.as_ref()) {
            if !packages_to_compile.contains(&pkg) {
                println!("Resolved test '{}' to package '{}'", target.test, pkg);
                packages_to_compile.push(pkg);
            }
        }
    }

    if packages_to_compile.is_empty() {
        println!("CovOpt-Analyzer: Compiling ENTIRE workspace tests (no packages resolved)...");
    } else {
        println!("CovOpt-Analyzer: Compiling specific packages: {:?}", packages_to_compile);
    }

    let workspace_executables = match crate::runner::compile_workspace_tests(&global_output_dir, &packages_to_compile) {
        Ok(exes) => exes,
        Err(e) => {
            eprintln!("Failed to compile workspace tests: {}", e);
            std::process::exit(1);
        }
    };

    let mut all_success = true;
    for target in config.target {
        let args = RunArgs {
            test: Some(target.test.clone()),
            expected: target.expected.clone(),
            n_values: target.n_values.clone(),
            mca_cpu: target.mca_cpu.clone(),
            require_cache_padding: target.require_cache_padding,
            require_branch_hints: target.require_branch_hints,
            require_aerospace_grade: target.require_aerospace_grade,
            require_watchdog_timeout: target.require_watchdog_timeout,
            require_stress_test: target.require_stress_test,
            polling_threshold: target.polling_threshold,
            ignore: target.ignore.as_ref().map(|vec| vec.join(",")),
            formalize: false, // Audit defaults to false unless specified
            optimize: false,
        };
        println!("\n===================================================");
        println!("Auditing target: {}", target.test);
        println!("===================================================");
        if !run_analysis(&args, true, Some(&workspace_executables)) {
            all_success = false;
        }

        // --- COVOPT 2.0 ENTROPY EVALUATION ---
        let entropy_result = entropy::calculate_entropy_score(&target, true);
        println!("\n=== COVOPT 2.0 ENTROPY REPORT ===");
        println!(
            "  A. Fuzz-Cov Variance: {:.1}/30.0",
            entropy_result.fuzz_variance_score
        );
        println!(
            "  B. API Branch Sprawl: {:.1}/40.0",
            entropy_result.branch_sprawl_score
        );
        println!(
            "  C. CLI Noise Index:   {:.1}/30.0",
            entropy_result.cli_noise_score
        );
        println!("  --------------------------------");
        println!(
            "  TOTAL ENTROPY SCORE:  {:.1}/100.0",
            entropy_result.total_score
        );

        if entropy_result.total_score > 50.0 {
            eprintln!(
                "  [!] WARNING: High Entropy Detected! Codebase is unstable, tangled, or noisy."
            );
            all_success = false;
        } else {
            println!("  [OK] Low Entropy. Code is well encapsulated and stable.");
        }
        println!("===================================");
    }

    if !all_success {
        eprintln!("\n[AUDIT FAILED] One or more targets failed complexity or coverage checks.");
        std::process::exit(1);
    } else {
        println!("\n[AUDIT PASSED] All targets passed complexity and coverage checks.");
    }
}

pub fn run_advise(args: &crate::AdviseArgs) -> Result<(), String> {
    use crate::advisor::EncapsulationAdvisor;
    use std::fs;
    use std::path::{Path, PathBuf};

    let target_path = Path::new(&args.target);
    let mut files_to_analyze = Vec::new();

    if target_path.is_file() {
        if target_path.extension().and_then(|s| s.to_str()) == Some("rs") {
            files_to_analyze.push(target_path.to_path_buf());
        } else {
            return Err("Target must be a Rust file or a directory".to_string());
        }
    } else if target_path.is_dir() {
        fn collect_rs_files(dir: &Path, files: &mut Vec<PathBuf>) {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
                    
                    if file_name.starts_with('.') || file_name == "target" || file_name == "tests" || file_name == "benches" {
                        continue;
                    }

                    if path.is_dir() {
                        collect_rs_files(&path, files);
                    } else if path.is_file()
                        && path.extension().and_then(|s| s.to_str()) == Some("rs")
                    {
                        files.push(path);
                    }
                }
            }
        }
        collect_rs_files(target_path, &mut files_to_analyze);
    } else {
        return Err("Target path does not exist".to_string());
    }

    if files_to_analyze.is_empty() {
        return Err("No Rust files found to analyze.".to_string());
    }

    println!(
        "Running Encapsulation Advisor on {} ({} files found)",
        args.target,
        files_to_analyze.len()
    );

    // Initialize ASM Extractor
    use crate::asm_extractor::AsmExtractor;
    println!("  [Phase 1] Compiling target to extract assembly (--emit=asm)...");
    let asm_extractor_opt = match std::env::current_dir() {
        Ok(dir) => {
            let extractor = AsmExtractor::new(dir);
            if let Err(e) = extractor.compile_asm() {
                println!(
                    "  [Warning] ASM compilation failed: {}. Continuing with static AST only.",
                    e
                );
                None
            } else {
                println!("  [OK] Assembly generated successfully.");
                Some(extractor)
            }
        }
        Err(_) => None,
    };

    let mut collected_asm_blocks: Vec<(String, String)> = Vec::new();

    for file_path in files_to_analyze {
        let content = match fs::read_to_string(&file_path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let ast = match syn::parse_file(&content) {
            Ok(a) => a,
            Err(_) => continue,
        };

        for item in ast.items {
            if let syn::Item::Fn(item_fn) = item {
                // Skip public functions (often just routing or facades) from analysis
                if matches!(item_fn.vis, syn::Visibility::Public(_)) {
                    continue;
                }

                // Exclude test and bench functions
                let mut is_test_or_bench = false;
                for attr in &item_fn.attrs {
                    if let syn::Meta::Path(path) = &attr.meta {
                        if path.is_ident("test") || path.is_ident("bench") {
                            is_test_or_bench = true;
                            break;
                        }
                    } else if let syn::Meta::List(list) = &attr.meta {
                         let path_str = list.path.segments.iter()
                                .map(|s| s.ident.to_string())
                                .collect::<Vec<_>>()
                                .join("::");
                         if path_str.contains("test") || path_str.contains("bench") {
                             is_test_or_bench = true;
                             break;
                         }
                    }
                }
                
                if is_test_or_bench {
                    continue;
                }

                let name = item_fn.sig.ident.to_string();

                if let Some(target_func) = &args.func
                    && &name != target_func
                {
                    continue;
                }

                // For now, we run without MCA report (static AST only).
                let mut asm_block_size = None;
                let mut mca_report_opt = None;
                if let Some(ref asm_extractor) = asm_extractor_opt
                    && let Ok(asm) = asm_extractor.extract_function(&name)
                {
                    asm_block_size = Some(asm.len());
                    collected_asm_blocks.push((name.clone(), asm.clone()));
                    use crate::mca::McaRunner;
                    let runner = McaRunner::new(None);
                    if let Ok(report) = runner.run(&asm) {
                        mca_report_opt = Some(report);
                    }
                }

                let report = EncapsulationAdvisor::analyze(&item_fn, mca_report_opt.as_ref());

                if !report.warnings.is_empty() || asm_block_size.is_some() {
                    println!("\n[File: {} | Function: {}]", file_path.display(), name);
                    if let Some(size) = asm_block_size {
                        println!(
                            "  - [ASM Extracted] {} bytes of assembly instructions",
                            size
                        );
                    }
                    if let Some(mca) = &mca_report_opt {
                        println!(
                            "  - [MCA Report] IPC: {:.2}, Block RThroughput: {:.2}",
                            mca.ipc, mca.block_rthroughput
                        );
                    }
                    for w in report.warnings {
                        println!("  - {}", w);
                    }
                }
            }
        }
    }

    // Phase 3: Semantic Clone Detection
    if !collected_asm_blocks.is_empty() {
        println!(
            "\n  [Phase 3] Scanning for Semantic Assembly Clones across {} functions...",
            collected_asm_blocks.len()
        );
        let asm_refs: Vec<(&str, &str)> = collected_asm_blocks
            .iter()
            .map(|(n, a)| (n.as_str(), a.as_str()))
            .collect();

        let clone_warnings = EncapsulationAdvisor::detect_asm_clones(&asm_refs);
        if clone_warnings.is_empty() {
            println!("  - No semantic clones detected.");
        } else {
            for w in clone_warnings {
                println!("  - [WARNING] {}", w);
            }
        }
    }

    Ok(())
}
