pub mod analyzer;
pub mod config;
pub mod coverage;
pub mod mca;
pub mod profiler;
pub mod runner;
pub mod static_analysis;
pub mod harden;

use clap::{Parser, Subcommand};
use std::fs;
use std::path::PathBuf;

use analyzer::{Complexity, ConvergenceAnalyzer};
use config::CovOptConfig;
use mca::McaRunner;
use runner::CargoTestRunner;

#[derive(Parser, Debug)]
#[command(name = "covopt")]
#[command(author, version, about = "Coverage-based Complexity Analyzer", long_about = None)]
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
    /// Audit all targets defined in .covopt.toml
    Audit,
    /// Run mutation testing on a target (Requires cargo-mutants)
    Mutate(MutateArgs),
    /// Run fuzzing on a target (Requires cargo-fuzz and nightly)
    Fuzz(FuzzArgs),
    /// Run tests with LLVM sanitizers (Requires nightly)
    Sanitize(SanitizeArgs),
    /// Profile a target to diagnose CPU hotspots and lock contention
    Profile(ProfileArgs),
    /// Run a single analysis target (legacy mode)
    Run(RunArgs),
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
}

#[derive(clap::Args, Debug, Clone)]
struct ProfileArgs {
    /// The name of the test to profile
    #[arg(short, long)]
    test: String,

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
    #[arg(long)]
    require_cache_padding: bool,

    /// Require static branch prediction hint detection
    #[arg(long)]
    require_branch_hints: bool,

    /// Require strict aerospace grade static analysis
    #[arg(long)]
    require_aerospace_grade: bool,
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

fn run_analysis(args: &RunArgs) -> bool {
    let test_name = args.test.as_ref().expect("--test is required");
    let expected_str = args.expected.as_ref().expect("--expected is required");
    let n_values_str = args.n_values.as_ref().expect("--n-values is required");
    let target_file = args.target_file.as_ref().expect("--target-file is required");
    let target_line = args.target_line.expect("--target-line is required");

    let expected = parse_complexity(expected_str);

    let n_values: Vec<usize> = n_values_str
        .split(',')
        .map(|s| s.trim().parse().expect("Failed to parse N value"))
        .collect();

    let dir = tempfile::tempdir().expect("Failed to create temporary directory");
    let output_dir = dir.path().to_path_buf();
    let runner = CargoTestRunner::new(test_name, &output_dir);

    let mut data = Vec::new();
    let mut target_symbol: Option<String> = None;
    let mut target_coverage_rate: Option<(u64, u64)> = None;

    println!("Starting CovOpt Analysis for test '{}'...", test_name);
    println!("Target: {}:{}", target_file, target_line);
    println!("Expected Complexity: {:?}", expected);
    println!("Testing N values: {:?}", n_values);
    println!("---------------------------------------------------");

    for n in n_values {
        println!("Running for N = {}...", n);
        let map = match runner.run(n) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("Failed to run coverage for N={}: {}", n, e);
                return false;
            }
        };

        let hit_count = map.find_hit_count(target_file, target_line);
        if let Some(h) = hit_count {
            println!("  -> Hit count = {}", h);
            data.push((n, h));
        } else {
            eprintln!("  -> WARNING: No hit count found for target file/line. Assuming 0.");
            data.push((n, 0));
        }

        if let Some(ref sym) = target_symbol {
            target_coverage_rate = map.get_function_coverage(sym);
        } else if let Some(sym) = map.find_symbol(target_file, target_line) {
            target_symbol = Some(sym.clone());
            target_coverage_rate = map.get_function_coverage(&sym);
        }
    }

    println!("---------------------------------------------------");
    println!("Analysis Results:");
    let report = ConvergenceAnalyzer::analyze(&data, expected);
    println!("{:#?}", report);
    
    let var_count = static_analysis::analyze_variables(std::path::Path::new(&target_file), target_line as usize);
    println!("Static Variable Declarations: {}", var_count);
    
    let thread_activities = static_analysis::analyze_thread_activity(std::path::Path::new(&target_file));
    if !thread_activities.is_empty() {
        println!("Static Thread Activities:");
        for act in thread_activities {
            println!("  - {}", act);
        }
    } else {
        println!("Static Thread Activities: None");
    }

    let mut success = true;

    if report.is_converged && report.actual_trend > expected {
        eprintln!("\n[ERROR] Algorithm complexity degraded! Expected {:?}, got {:?}", expected, report.actual_trend);
        success = false;
    }
    
    if args.require_cache_padding {
        let has_padding = static_analysis::analyze_cache_padding(std::path::Path::new(&target_file));
        if has_padding {
            println!("Static Cache Padding: Detected");
        } else {
            eprintln!("\n[ERROR] Missing Cache Padding! Strict mode requires cache alignment for target.");
            success = false;
        }
    }

    if args.require_branch_hints {
        let has_hints = static_analysis::analyze_branch_hints(std::path::Path::new(&target_file));
        if has_hints {
            println!("Static Branch Hints: Detected");
        } else {
            eprintln!("\n[ERROR] Missing Branch Prediction Hints! Strict mode requires likely/unlikely markers for target.");
            success = false;
        }
    }

    if args.require_aerospace_grade {
        let violations = static_analysis::analyze_aerospace_grade(std::path::Path::new(&target_file));
        if violations.is_empty() {
            println!("Static Aerospace Grade: Passed");
        } else {
            eprintln!("\n[ERROR] Aerospace Grade Violations Detected in {}!", target_file);
            for v in violations {
                eprintln!("  - {}", v);
            }
            success = false;
        }
    }

    println!("---------------------------------------------------");
    if let Some(symbol) = target_symbol {
        if let Some((executed, total)) = target_coverage_rate {
            let rate = (executed as f64 / total as f64) * 100.0;
            println!(
                "Coverage Rate (Target Function): {:.1}% ({}/{} lines)",
                rate, executed, total
            );
            if rate < 90.0 {
                println!(
                    "[WARNING] Function coverage is below 90%. The measured mathematical complexity might not reflect the worst-case scenario. Consider adding more branches to your test."
                );
                success = false; // Fail audit if coverage is below 90%
            }
            println!("---------------------------------------------------");
        }

        println!("Target Symbol Found: {}", symbol);
        println!("Extracting ASM and running LLVM-MCA analysis...");

        match runner.compile_asm() {
            Ok(asm_content) => {
                let mut asm_block_opt = runner.extract_asm_block(&asm_content, &symbol);
                if asm_block_opt.is_none() {
                    let demangled = rustc_demangle::demangle(&symbol).to_string();
                    let no_trailing_generics = match demangled.rfind(">::") {
                        Some(idx) => &demangled[..idx + 1],
                        None => &demangled,
                    };
                    let parts: Vec<&str> = no_trailing_generics.split("::").collect();
                    if parts.len() >= 2 {
                        let fn_name = parts.last().unwrap_or(&"").split('<').next().unwrap_or("").trim();
                        let struct_part = parts[parts.len() - 2];
                        let struct_name = struct_part.split('<').next().unwrap_or("").split('[').next().unwrap_or("").trim().trim_start_matches(['<', '[']);
                        
                        println!("  -> Target symbol exact match failed. Searching by keywords: '{}', '{}'...", struct_name, fn_name);
                        asm_block_opt = runner.extract_asm_block_by_keywords(&asm_content, &[struct_name, fn_name]);
                    }
                    if asm_block_opt.is_none() {
                        println!("  -> Still not found. Target symbol inlined. Walking up to test caller '{}'...", test_name);
                        asm_block_opt = runner.extract_asm_block_by_keywords(&asm_content, &[test_name]);
                    }
                }

                if let Some(asm_block) = asm_block_opt {
                    let mem_profile = static_analysis::analyze_memory_ops(&asm_block);
                    println!("\n[Static Memory Operations]");
                    println!("Loads:  {}", mem_profile.loads);
                    println!("Stores: {}", mem_profile.stores);
                    println!("Allocs: {}", mem_profile.allocs);

                    let mca_runner = McaRunner::new(args.mca_cpu.clone());
                    match mca_runner.run(&asm_block) {
                        Ok(mca_report) => {
                            println!("\n[MCA Report]");
                            println!("Block RThroughput: {:.2}", mca_report.block_rthroughput);
                            println!("IPC:               {:.2}", mca_report.ipc);
                        }
                        Err(e) => eprintln!("LLVM-MCA failed: {}", e),
                    }
                } else {
                    eprintln!("Could not extract ASM block for symbol. The function might be inlined in release mode.");
                }
            }
            Err(e) => eprintln!("ASM compilation failed: {}", e),
        }
    } else {
        println!("Could not extract target symbol name from coverage data. Skipping MCA analysis.");
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
    println!("Successfully installed pre-commit hook to {}", hook_path.display());
}

fn init_config() {
    let config_path = std::path::PathBuf::from(".covopt.toml");
    if config_path.exists() {
        eprintln!("CovOpt-Analyzer: .covopt.toml already exists in the current directory.");
        std::process::exit(1);
    }

    let default_config = r#"[[target]]
test = "my_benchmark_test"
expected = "O(1)"
n_values = "100,500,1000"
target_file = "src/lib.rs"
target_line = 10
require_cache_padding = false
require_branch_hints = false
require_aerospace_grade = false
"#;

    if let Err(e) = std::fs::write(&config_path, default_config) {
        eprintln!("Failed to write .covopt.toml: {}", e);
        std::process::exit(1);
    }
    println!("Successfully initialized .covopt.toml. Please edit it to match your target.");
}

fn run_audit() {
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
            mca_cpu: target.mca_cpu,
            require_cache_padding: target.require_cache_padding.unwrap_or(false),
            require_branch_hints: target.require_branch_hints.unwrap_or(false),
            require_aerospace_grade: target.require_aerospace_grade.unwrap_or(false),
        };
        println!("\n===================================================");
        println!("Auditing target: {}", target.test);
        println!("===================================================");
        if !run_analysis(&args) {
            all_success = false;
        }
    }

    if !all_success {
        eprintln!("\n[AUDIT FAILED] One or more targets failed complexity or coverage checks.");
        std::process::exit(1);
    } else {
        println!("\n[AUDIT PASSED] All targets passed complexity and coverage checks.");
    }
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Init) => init_config(),
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
            if !harden::run_sanitizer(&args.test, &args.san_type) {
                std::process::exit(1);
            }
        }
        Some(Commands::Profile(args)) => {
            if !profiler::run_profile(&args.test, &args.tool) {
                std::process::exit(1);
            }
        }
        Some(Commands::Run(args)) => {
            if !run_analysis(&args) {
                std::process::exit(1);
            }
        }
        None => {
            // Default to legacy run mode if flags are provided
            if cli.run_args.test.is_some() {
                if !run_analysis(&cli.run_args) {
                    std::process::exit(1);
                }
            } else {
                eprintln!("No command provided. Use `covopt --help` for usage.");
                std::process::exit(1);
            }
        }
    }
}
