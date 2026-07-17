pub mod analyzer;
pub mod config;
pub mod coverage;
pub mod entropy;
pub mod explore;
pub mod harden;
pub mod heuristic;
pub mod mca;
pub mod optimizer;
pub mod parameter_optimizer;
pub mod profiler;
pub mod runner;
pub mod static_analysis;

use clap::{Parser, Subcommand};
use std::fs;
use std::path::{Path, PathBuf};

use analyzer::{Complexity, ConvergenceAnalyzer};
use config::CovOptConfig;
use mca::McaRunner;
use runner::CargoTestRunner;

#[derive(Parser, Debug)]
#[command(name = "covopt")]
#[command(
    author,
    version,
    about = "Coverage-based Complexity & Safety Analyzer",
    long_about = "RECOMMENDED WORKFLOWS:\n\n\
  🧑 [FOR HUMANS]\n\
    - fuzz / mutate      : Harden your test coverage manually.\n\
    - profile            : Diagnose CPU bottlenecks and visualize with flamegraphs in the browser.\n\
    - sanitize --auto-fix: Interactively catch and repair Use-After-Free/Data Races using LLMs.\n\n\
  🤖 [FOR AI AGENTS]\n\
    - audit              : Fast, low-entropy verification checking with quiet checklist output.\n\
    - profile            : Automatically parses flamegraph SVGs into text-based CPU hotspots for AI tuning.\n\
    - sanitize --auto-fix: Automate self-healing ReAct compilation loops to auto-patch memory bugs."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[clap(flatten)]
    run_args: RunArgs,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Install a pre-commit hook in the current git repository
    InstallHook,
    /// Initialize a default .covopt.toml in the current directory
    Init,
    /// Automatically fix Clippy warnings and formatting
    Fix,
    /// Audit all targets defined in .covopt.toml
    Audit,
    /// Automatically discover the hottest loop and generate .covopt.toml
    Auto(AutoArgs),
    /// Run mutation testing on a target (Requires cargo-mutants)
    Mutate(MutateArgs),
    /// Run fuzzing on a target (Requires cargo-fuzz and nightly)
    Fuzz(FuzzArgs),
    /// Run tests with LLVM sanitizers (Requires nightly)
    Sanitize(SanitizeArgs),
    /// Profile a target to diagnose CPU hotspots and lock contention
    Profile(ProfileArgs),
    /// Run a single analysis target (legacy mode)
    #[command(hide = true)]
    Run(RunArgs),
    /// Tune parameters using NP-hard Monte Carlo diffusion
    #[command(hide = true)]
    Tune(TuneArgs),
    /// Logarithmic NP-Hard Exploration via Meta-Programming
    #[command(hide = true)]
    Explore(ExploreArgs),
}

#[derive(clap::Args, Debug, Clone)]
struct TuneArgs {
    /// The name of the test to evaluate fitness score
    #[arg(short, long)]
    test: String,

    /// Comma-separated parameter ranges (e.g. "WARMUP_PCT:50..100, WARMUP_STEP:1..20")
    #[arg(short, long)]
    params: String,

    /// Number of Monte Carlo iterations
    #[arg(short, long, default_value_t = 100)]
    iterations: usize,
}

#[derive(clap::Args, Debug, Clone)]
struct ExploreArgs {
    /// Directory to scan for source files
    #[arg(long, default_value = "src")]
    src: String,

    /// Target trait name to look for (e.g. ScienceObjective)
    #[arg(long)]
    trait_name: String,

    /// Target method to extract tokens from for similarity projection
    #[arg(long, default_value = "evaluate_fitness")]
    method_name: String,

    /// Similarity threshold for perfect resonance (0.0 to 1.0)
    #[arg(long, default_value_t = 0.99)]
    threshold: f64,
}

#[derive(clap::Args, Debug, Clone)]
struct AutoArgs {
    /// The name of the test to automatically profile
    test: String,
}

#[derive(clap::Args, Debug, Clone)]
struct MutateArgs {
    #[arg(short, long)]
    test: String,
}

#[derive(clap::Args, Debug, Clone)]
struct FuzzArgs {
    #[arg(short, long)]
    target: String,
}

#[derive(clap::Args, Debug, Clone)]
struct SanitizeArgs {
    #[arg(short, long)]
    test: String,

    #[arg(long, default_value = "address")]
    san_type: String, // address or thread

    /// Automatically repair memory safety crashes using LLM
    #[arg(long, default_value_t = false)]
    auto_fix: bool,
}

#[derive(clap::Args, Debug, Clone)]
struct ProfileArgs {
    /// The name of the test to profile
    #[arg(long)]
    test: Option<String>,

    /// The name of the binary to profile
    #[arg(long)]
    bin: Option<String>,

    /// Profiling tool to use: flamegraph (default) or samply
    #[arg(long, default_value = "flamegraph")]
    tool: String,
}

#[derive(clap::Args, Debug, Clone)]
struct RunArgs {
    /// The name of the test to run
    #[arg(short, long)]
    test: Option<String>,

    /// Expected complexity (e.g. O1, OLogN, ON, ONLogN, ON2)
    #[arg(short, long)]
    expected: Option<String>,

    /// Comma-separated list of N values (e.g. 100,1000,10000)
    #[arg(short, long)]
    n_values: Option<String>,

    /// Target file to track coverage in
    #[arg(long)]
    target_file: Option<String>,

    /// Target line number to track hit count
    #[arg(long)]
    target_line: Option<u64>,

    /// Optional LLVM-MCA CPU target (e.g. apple-m1, skylake)
    #[arg(long)]
    mca_cpu: Option<String>,

    /// Require static cache padding detection
    #[arg(long, hide = true)]
    require_cache_padding: bool,

    /// Enable symbolic regression to reinvent Lean 4 style formal mathematical proofs
    #[arg(long, hide = true)]
    formalize: bool,

    /// Require static branch prediction hint detection
    #[arg(long, hide = true)]
    require_branch_hints: bool,

    /// Require strict aerospace grade static analysis
    #[arg(long, hide = true)]
    require_aerospace_grade: bool,

    /// Require watchdog timeout detection in the target file
    #[arg(long, hide = true)]
    require_watchdog_timeout: bool,

    /// Require high-pressure stress test detection in the target file
    #[arg(long, hide = true)]
    require_stress_test: bool,

    /// Optional polling threshold for high-frequency polling detection
    #[arg(long, hide = true)]
    polling_threshold: Option<u64>,

    /// Run the discrete diffusion NP-hard solver to superoptimize ASM
    #[arg(long, hide = true)]
    optimize: bool,
}

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

fn run_analysis(args: &RunArgs, compact: bool) -> bool {
    let mut log = LogBuffer::new(compact);

    let test_name = args.test.as_ref().expect("--test is required");
    let expected_str = args.expected.as_ref().expect("--expected is required");
    let n_values_str = args.n_values.as_ref().expect("--n-values is required");
    let target_file = args
        .target_file
        .as_ref()
        .expect("--target-file is required");
    let target_line = args.target_line.expect("--target-line is required");

    let expected = parse_complexity(expected_str);

    let _n_values: Vec<usize> = n_values_str
        .split(',')
        .map(|s| s.trim().parse().expect("Failed to parse N value"))
        .collect();

    let dir = tempfile::tempdir().expect("Failed to create temporary directory");
    let output_dir = dir.path().to_path_buf();
    let runner = CargoTestRunner::new(test_name, &output_dir);

    wlog!(log, "Starting CovOpt Analysis for test '{}'...", test_name);
    wlog!(log, "Target: {}:{}", target_file, target_line);
    wlog!(log, "Expected Complexity: {:?}", expected);

    let mut data = Vec::new();
    let mut space_data = Vec::new();
    let mut target_symbol: Option<String> = None;
    let mut target_coverage_rate = None;
    let mut mca_stats = None;

    for n_str in args.n_values.as_ref().unwrap().split(',') {
        let n: u64 = n_str.trim().parse().expect("Invalid N value");
        wlog!(log, "---------------------------------------------------");
        wlog!(log, "Running for N = {}...", n);

        let (map, peak_rss) = match runner.run(n as usize, None) {
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

        let hit_count = map.find_hit_count(target_file, target_line);
        if let Some(h) = hit_count {
            wlog!(
                log,
                "  -> Hit count = {} | Peak RSS = {} bytes",
                h,
                peak_rss
            );
            data.push((n as usize, h));
        } else {
            wlog!(
                log,
                "  -> WARNING: No hit count found for target file/line. Assuming 0."
            );
            data.push((n as usize, 0));
        }
        space_data.push((n as usize, peak_rss));

        if let Some(ref sym) = target_symbol {
            target_coverage_rate = map.get_function_coverage(sym);
        } else if let Some(sym) = map.find_symbol(target_file, target_line) {
            target_symbol = Some(sym.clone());
            target_coverage_rate = map.get_function_coverage(&sym);
        }
    }

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
        let has_padding =
            static_analysis::analyze_cache_padding(std::path::Path::new(&target_file));
        static_cache_padding = Some(has_padding);
        if has_padding {
            wlog!(log, "Static Cache Padding: Detected");
        } else {
            wlog!(
                log,
                "\n[ERROR] Missing Cache Padding! Strict mode requires cache alignment for target."
            );
            success = false;
        }
    }

    let mut static_branch_hints = None;
    if args.require_branch_hints {
        let has_hints = static_analysis::analyze_branch_hints(std::path::Path::new(&target_file));
        static_branch_hints = Some(has_hints);
        if has_hints {
            wlog!(log, "Static Branch Hints: Detected");
        } else {
            wlog!(
                log,
                "\n[ERROR] Missing Branch Prediction Hints! Strict mode requires likely/unlikely markers for target."
            );
            success = false;
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
        let has_watchdog =
            static_analysis::analyze_watchdog_timeout(std::path::Path::new(&target_file));
        static_watchdog_timeout = Some(has_watchdog);
        if has_watchdog {
            wlog!(log, "Static Watchdog Timeout: Detected");
        } else {
            wlog!(
                log,
                "\n[ERROR] Missing Watchdog Timeout! Strict mode requires timeout mechanisms (e.g. recv_timeout) to prevent infinite spin deadlocks."
            );
            success = false;
        }
    }

    let mut static_stress_test = None;
    if args.require_stress_test {
        let has_stress = static_analysis::analyze_stress_test(std::path::Path::new(&target_file));
        static_stress_test = Some(has_stress);
        if has_stress {
            wlog!(log, "Static Stress Test: Detected");
        } else {
            wlog!(
                log,
                "\n[ERROR] Missing High-Pressure Stress Test! Target file lacks heavy concurrent thread spawning logic."
            );
            success = false;
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
                    runner.extract_asm_block_by_loc(&asm_content, target_file, target_line);
                if asm_block_opt.is_none() {
                    asm_block_opt = runner.extract_asm_block(&asm_content, &symbol);
                }
                if asm_block_opt.is_none() {
                    let demangled = rustc_demangle::demangle(&symbol).to_string();
                    let no_trailing_generics = match demangled.rfind(">::") {
                        Some(idx) => &demangled[..idx + 1],
                        None => &demangled,
                    };
                    let parts: Vec<&str> = no_trailing_generics.split("::").collect();
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
                            .trim_start_matches(['<', '[']);

                        wlog!(
                            log,
                            "  -> Target symbol exact match failed. Searching by keywords: '{}', '{}'...",
                            struct_name,
                            fn_name
                        );
                        asm_block_opt = runner
                            .extract_asm_block_by_keywords(&asm_content, &[struct_name, fn_name]);
                    }
                    if asm_block_opt.is_none() {
                        wlog!(
                            log,
                            "  -> Still not found. Target symbol inlined. Walking up to test caller '{}'...",
                            test_name
                        );
                        asm_block_opt =
                            runner.extract_asm_block_by_keywords(&asm_content, &[test_name]);
                    }
                }

                if let Some(asm_block) = asm_block_opt {
                    let mem_profile = static_analysis::analyze_memory_ops(&asm_block);
                    wlog!(log, "\n[Static Memory Operations]");
                    wlog!(log, "Loads:  {}", mem_profile.loads);
                    wlog!(log, "Stores: {}", mem_profile.stores);
                    wlog!(log, "Allocs: {}", mem_profile.allocs);

                    let mca_runner = McaRunner::new(args.mca_cpu.clone());
                    match mca_runner.run(&asm_block) {
                        Ok(mca_report) => {
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
                println!("  - LLVM-MCA: IPC {:.2}, RThroughput {:.2}", ipc, rt);
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

fn install_hook() {
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
- Before submitting performance-related code, you MUST run `covopt audit` and ensure the Entropy Score is `0.0`. If it is not, use `covopt tune` or `covopt explore` to converge the parameters.

## Rule 2: Anti-DCE in Benchmarks
- When writing tests or benchmarks for `covopt` or `llvm-mca`, **ALWAYS** wrap loop variables and return values with `std::hint::black_box()`.
- This prevents LLVM Dead Code Elimination (DCE) from optimizing `O(N)` loops into `O(1)` during `--release` coverage builds.

## Rule 3: Lock-Free Critical Paths
- For extreme performance tiers (e.g., `< 50ns` p50 latency), **NEVER** use standard library `Mutex` or `RwLock` on the critical path.
- **ALWAYS** use QSBR (Quiescent State Based Reclamation) or Thread Local Storage (TLS) for thread-safe memory and state management.

## Rule 4: Strict Clippy Cleanliness
- All macro-generated code from `covopt_param!` must be strictly typed. **DO NOT** use `#[allow(...)]` to ignore `as u8` or `as u16` cast warnings. Fix the underlying type inference instead.
"#;

fn init_config() {
    let config_path = std::path::PathBuf::from(".covopt.toml");
    if config_path.exists() {
        eprintln!("CovOpt-Analyzer: .covopt.toml already exists in the current directory.");
        std::process::exit(1);
    }

    let default_config = r#"agent_deterrence = true

[[target]]
test = "my_benchmark_test"
expected = "O(1)"
n_values = "100,500,1000"
target_file = "src/lib.rs"
target_line = 10
require_cache_padding = false
require_branch_hints = false
require_aerospace_grade = false
require_watchdog_timeout = false
require_stress_test = false
"#;

    if let Err(e) = std::fs::write(&config_path, default_config) {
        eprintln!("Failed to write .covopt.toml: {}", e);
        std::process::exit(1);
    }
    println!("Successfully initialized .covopt.toml. Please edit it to match your target.");

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
        if !current_agents_md.contains("CovOpt Optimization & Tuning Rules") {
            let mut new_agents_md = current_agents_md;
            if !new_agents_md.ends_with('\n') && !new_agents_md.is_empty() {
                new_agents_md.push('\n');
            }
            new_agents_md.push('\n');
            new_agents_md.push_str(COVOPT_AGENT_RULES);
            new_agents_md.push('\n');
            if let Err(e) = std::fs::write(&agents_md, new_agents_md) {
                eprintln!("Failed to append to {:?}: {}", agents_md, e);
            } else {
                println!("Appended CovOpt rules to {:?}.", agents_md);
            }
        }
    }
}

fn run_fix() {
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

fn run_audit() {
    unsafe {
        std::env::set_var("COVOPT_COMPACT", "1");
    }
    let config_path = ".covopt.toml";
    if !PathBuf::from(config_path).exists() {
        eprintln!("Config file {} not found.", config_path);
        std::process::exit(1);
    }

    let config = match CovOptConfig::load(config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };

    let mut all_success = true;
    for target in config.target {
        let args = RunArgs {
            test: Some(target.test.clone()),
            expected: Some(target.expected.clone()),
            n_values: Some(target.n_values.clone()),
            target_file: Some(target.target_file.clone()),
            target_line: Some(target.target_line),
            mca_cpu: target.mca_cpu.clone(),
            require_cache_padding: target.require_cache_padding.unwrap_or(false),
            require_branch_hints: target.require_branch_hints.unwrap_or(false),
            require_aerospace_grade: target.require_aerospace_grade.unwrap_or(false),
            require_watchdog_timeout: target.require_watchdog_timeout.unwrap_or(false),
            require_stress_test: target.require_stress_test.unwrap_or(false),
            polling_threshold: target.polling_threshold,
            formalize: false, // Audit defaults to false unless specified
            optimize: false,
        };
        println!("\n===================================================");
        println!("Auditing target: {}", target.test);
        println!("===================================================");
        if !run_analysis(&args, true) {
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

fn run_auto_discovery(test_name: &str) {
    println!(
        "CovOpt-Analyzer: Running Auto-Discovery for test '{}'...",
        test_name
    );

    let config_path = PathBuf::from(".covopt.toml");
    if config_path.exists() {
        eprintln!(
            "CovOpt-Analyzer: .covopt.toml already exists! Auto-discovery will overwrite it."
        );
        eprintln!("Please remove or rename it first if you want to regenerate.");
        std::process::exit(1);
    }

    let output_dir = PathBuf::from(".covopt");
    let runner = CargoTestRunner::new(test_name, &output_dir);

    // Run the test once with N=1000 (a reasonably high number to saturate loops)
    let (map, _) = match runner.run(1000, None) {
        Ok(res) => res,
        Err(e) => {
            eprintln!("CovOpt-Analyzer: Failed to run test {}: {}", test_name, e);
            std::process::exit(1);
        }
    };

    let mut max_count = 0;
    let mut hot_file = String::new();
    let mut hot_line = 0;

    for (file, file_cov) in &map.hit_counts {
        // Skip std and registry paths to focus on user code
        if file.contains("/.cargo/registry") || file.contains("/rustc/") {
            continue;
        }

        for (&line, &count) in file_cov {
            if count > max_count {
                max_count = count;
                hot_file = file.clone();
                hot_line = line;
            }
        }
    }

    if max_count == 0 {
        eprintln!("CovOpt-Analyzer: Auto-discovery failed. No execution counts found!");
        std::process::exit(1);
    }

    // Clean up absolute paths to relative paths if possible
    let cwd = std::env::current_dir().unwrap_or_default();
    let relative_file = if let Ok(rel) = Path::new(&hot_file).strip_prefix(&cwd) {
        rel.display().to_string()
    } else {
        hot_file
    };

    println!("\n> [!TIP] Auto-Discovery Complete!");
    println!(
        "🔥 Hottest Loop Detected: {}:{} (Hit Count: {})",
        relative_file, hot_line, max_count
    );

    let generated_config = format!(
        r#"agent_deterrence = true

[[target]]
test = "{}"
expected = "O(N)"
n_values = "100,500,1000"
target_file = "{}"
target_line = {}
require_cache_padding = false
require_branch_hints = false
require_aerospace_grade = false
require_watchdog_timeout = false
require_stress_test = false
"#,
        test_name, relative_file, hot_line
    );

    if let Err(e) = std::fs::write(&config_path, generated_config) {
        eprintln!("Failed to write .covopt.toml: {}", e);
        std::process::exit(1);
    }

    println!("Successfully generated `.covopt.toml` targeting the absolute hottest path.");
    println!("You can now run `covopt audit` to perform the complexity and MCA analysis.");
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Init) => init_config(),
        Some(Commands::Auto(args)) => run_auto_discovery(&args.test),
        Some(Commands::Fix) => run_fix(),
        Some(Commands::InstallHook) => install_hook(),
        Some(Commands::Audit) => run_audit(),
        Some(Commands::Mutate(args)) => {
            if !harden::run_mutants(&args.test) {
                std::process::exit(1);
            }
        }
        Some(Commands::Fuzz(args)) => {
            if !harden::run_fuzz(&args.target) {
                std::process::exit(1);
            }
        }
        Some(Commands::Sanitize(args)) => {
            if !harden::run_sanitizer(&args.test, &args.san_type, args.auto_fix) {
                std::process::exit(1);
            }
        }
        Some(Commands::Profile(args)) => {
            if !profiler::run_profile(args.test.as_deref(), args.bin.as_deref(), &args.tool) {
                std::process::exit(1);
            }
        }
        Some(Commands::Run(args)) => {
            if !run_analysis(&args, false) {
                std::process::exit(1);
            }
        }
        Some(Commands::Tune(args)) => {
            let opt = parameter_optimizer::ParameterOptimizer::new(
                args.test.clone(),
                &args.params,
                args.iterations,
            );
            opt.run();
        }
        Some(Commands::Explore(args)) => {
            explore::run(
                &args.src,
                &args.trait_name,
                &args.method_name,
                args.threshold,
            );
        }
        None => {
            // Default to legacy run mode if flags are provided
            if cli.run_args.test.is_some() {
                if !run_analysis(&cli.run_args, false) {
                    std::process::exit(1);
                }
            } else {
                eprintln!("No command provided. Use `covopt --help` for usage.");
                std::process::exit(1);
            }
        }
    }
}
