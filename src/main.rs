pub mod advisor;
pub mod analyzer;
pub mod asm_extractor;
pub mod auto_fixer;
pub mod auto_harness;
pub mod auto_refactor;
pub mod auto_simd;
pub mod ci;
pub mod commands;
pub mod config;
pub mod coverage;
pub mod dashboard;
pub mod entropy;
pub mod explore;
pub mod harden;
pub mod heuristic;
pub mod mca;
pub mod optimizer;
pub mod parameter_optimizer;
pub mod pgo_injector;
pub mod profiler;
pub mod runner;
pub mod scanner;
pub mod static_analysis;
pub mod struct_layout;

use clap::{Parser, Subcommand};

use analyzer::Complexity;

#[derive(Parser, Debug)]
#[command(name = "covopt")]
#[command(author, version, about = "Coverage-based Complexity & Safety Analyzer")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[clap(flatten)]
    run_args: RunArgs,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize a default .covopt.toml in the target directory
    Init(InitArgs),

    /// Unified Auto-Pilot Pipeline (Fix -> Audit -> Optimize)
    Ci(CiArgs),

    /// Automatically fix Clippy warnings and formatting
    Fix,

    /// Generate an HTML dashboard report
    #[command(name = "report")]
    Report(ReportArgs),

    /// Group of check commands (Audit, Magic, Advise)
    #[command(subcommand)]
    Check(CheckCommands),

    /// Group of performance tuning commands
    #[command(subcommand)]
    Tune(TuneCommands),

    /// Group of security and robustness commands
    #[command(subcommand)]
    Harden(HardenCommands),
}

#[derive(Subcommand, Debug, Clone)]
pub enum CheckCommands {
    /// Audit all targets defined in .covopt.toml
    Audit,
    /// Scan Rust files for hardcoded magic numbers
    #[command(name = "magic")]
    Magic(ScanMagicArgs),
    /// Analyze encapsulation quality (Abstraction Penalty & Missing Encapsulation)
    Advise(AdviseArgs),
}

#[derive(Subcommand, Debug, Clone)]
pub enum TuneCommands {
    /// Performance Parameter Auto-Tuning & Optimization
    #[command(name = "params")]
    Params(OptimizeArgs),
    /// Profile a target to diagnose CPU hotspots and lock contention
    Profile(ProfileArgs),
    /// Tune struct memory layouts for cache efficiency
    #[command(name = "layout")]
    Layout(TuneLayoutArgs),
    /// Scan for SIMD auto-vectorization opportunities
    Vectorize(VectorizeArgs),
    /// Inject dynamic PGO (likely/unlikely) probes based on coverage
    #[command(name = "pgo")]
    Pgo(PgoInjectArgs),
    /// Scaffold Advanced AI Refactoring (O(N^2) -> O(N log N))
    #[command(name = "refactor")]
    Refactor(AiRefactorArgs),
}

#[derive(Subcommand, Debug, Clone)]
pub enum HardenCommands {
    /// Robustness & Security Hardening (Mutation, Fuzzing, Sanitizers)
    Run(HardenArgs),
    /// Generate fuzzing harnesses for public functions
    #[command(name = "fuzz")]
    Fuzz(GenerateFuzzArgs),
}

#[derive(clap::Args, Debug, Clone)]
pub struct GenerateFuzzArgs {
    #[arg(long, default_value = "src")]
    pub target_dir: String,
}

#[derive(clap::Args, Debug, Clone)]
pub struct PgoInjectArgs {
    #[arg(long, default_value = "src")]
    pub target_dir: String,

    #[arg(long, default_value_t = 1000)]
    pub threshold: u64,
}

#[derive(clap::Args, Debug, Clone)]
pub struct TuneLayoutArgs {
    #[arg(long, default_value = "src")]
    pub target_dir: String,
}

#[derive(clap::Args, Debug, Clone)]
pub struct ReportArgs {
    #[arg(long, default_value = "target/covopt")]
    pub output_dir: String,
}

#[derive(clap::Args, Debug, Clone)]
pub struct VectorizeArgs {
    #[arg(long, default_value = "src")]
    pub target_dir: String,
}

#[derive(clap::Args, Debug, Clone)]
pub struct AiRefactorArgs {
    #[arg(long, default_value = "src")]
    pub target_dir: String,
}

#[derive(clap::Args, Debug, Clone)]
pub struct AdviseArgs {
    /// Target file to analyze
    #[arg(long)]
    pub target: String,

    /// Optional function name to analyze
    #[arg(long)]
    pub func: Option<String>,
}

#[derive(clap::Args, Debug, Clone)]
pub struct InitArgs {
    /// Optional path to initialize in (defaults to current directory)
    pub path: Option<String>,

    /// Skip interactive prompts and accept default values
    #[arg(short, long)]
    pub yes: bool,

    /// Install a pre-commit hook in the target git repository
    #[arg(long, default_value_t = false)]
    pub hook: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct ScanMagicArgs {
    /// Optional path to scan (defaults to current directory)
    pub path: Option<String>,
}

#[derive(clap::Args, Debug, Clone)]
pub struct HardenArgs {
    /// The name of the test target
    pub test: String,

    /// Run mutation testing using cargo-mutants
    #[arg(long, default_value_t = false)]
    pub mutate: bool,

    /// Run fuzzing using cargo-fuzz
    #[arg(long, default_value_t = false)]
    pub fuzz: bool,

    /// Run tests with LLVM sanitizers
    #[arg(long, default_value_t = false)]
    pub sanitize: bool,

    /// Sanitizer type (address or thread)
    #[arg(long, default_value = "address")]
    pub san_type: String,

    /// Automatically repair memory safety crashes using LLM
    #[arg(long, default_value_t = false)]
    pub auto_fix: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct OptimizeArgs {
    /// The name of the test target
    pub test: String,

    /// Comma-separated parameter ranges (e.g. "WARMUP_PCT:50..100, WARMUP_STEP:1..20")
    #[arg(short, long)]
    pub params: Option<String>,

    /// Number of Monte Carlo iterations for parameter tuning
    #[arg(short, long, default_value_t = 100)]
    pub iterations: usize,

    /// Run meta-programming similarity exploration
    #[arg(long, default_value_t = false)]
    pub explore: bool,

    /// Directory to scan for source files (for explore)
    #[arg(long, default_value = "src")]
    pub src: String,

    /// Target trait name (for explore)
    #[arg(long)]
    pub trait_name: Option<String>,

    /// Target method to extract tokens from (for explore)
    #[arg(long, default_value = "evaluate_fitness")]
    pub method_name: String,

    /// Similarity threshold for perfect resonance (for explore, 0.0 to 1.0)
    #[arg(long, default_value_t = 0.99)]
    pub threshold: f64,
}

#[derive(clap::Args, Debug, Clone)]
pub struct CiArgs {
    /// Skip the hardening (fuzz/mutate) step
    #[arg(long, default_value_t = false)]
    pub skip_harden: bool,

    /// Fail the CI if any step produces a non-perfect result
    #[arg(long, default_value_t = false)]
    pub strict: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct ProfileArgs {
    /// The name of the test to profile
    #[arg(long)]
    pub test: Option<String>,

    /// The name of the binary to profile
    #[arg(long)]
    pub bin: Option<String>,

    /// Profiling tool to use: flamegraph (default) or samply
    #[arg(long, default_value = "flamegraph")]
    pub tool: String,
}

#[derive(clap::Args, Debug, Clone)]
pub struct RunArgs {
    /// The name of the test to run
    #[arg(short, long)]
    pub test: Option<String>,

    /// Expected complexity (e.g. O1, OLogN, ON, ONLogN, ON2)
    #[arg(short, long)]
    pub expected: Option<String>,

    /// Comma-separated list of N values (e.g. 100,1000,10000)
    #[arg(short, long)]
    pub n_values: Option<String>,

    /// Optional LLVM-MCA CPU target (e.g. apple-m1, skylake)
    #[arg(long)]
    pub mca_cpu: Option<String>,

    /// Comma-separated list of symbols to ignore in coverage peak search
    #[arg(long)]
    pub ignore: Option<String>,

    /// Require static cache padding detection
    #[arg(long, hide = true)]
    pub require_cache_padding: bool,

    /// Enable symbolic regression to reinvent Lean 4 style formal mathematical proofs
    #[arg(long, hide = true)]
    pub formalize: bool,

    /// Require static branch prediction hint detection
    #[arg(long, hide = true)]
    pub require_branch_hints: bool,

    /// Require strict aerospace grade static analysis
    #[arg(long, hide = true)]
    pub require_aerospace_grade: bool,

    /// Require watchdog timeout detection in the target file
    #[arg(long, hide = true)]
    pub require_watchdog_timeout: bool,

    /// Require high-pressure stress test detection in the target file
    #[arg(long, hide = true)]
    pub require_stress_test: bool,

    /// Optional polling threshold for high-frequency polling detection
    #[arg(long, hide = true)]
    pub polling_threshold: Option<u64>,

    /// Run the discrete diffusion NP-hard solver to superoptimize ASM
    #[arg(long, hide = true)]
    pub optimize: bool,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Init(args)) => {
            if args.hook {
                commands::install_hook();
            } else {
                commands::init_config(args);
            }
        }
        Some(Commands::Fix) => commands::run_fix(),
        Some(Commands::Report(args)) => {
            let engine = dashboard::DashboardGenerator::new(&args.output_dir);
            if let Err(e) = engine.generate() {
                eprintln!("CovOpt Error: {:?}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Check(cmd)) => match cmd {
            CheckCommands::Audit => commands::run_audit(),
            CheckCommands::Magic(args) => crate::scanner::run_scan(args.path),
            CheckCommands::Advise(args) => {
                if let Err(e) = commands::run_advise(&args) {
                    eprintln!("CovOpt Error: {:?}", e);
                    std::process::exit(1);
                }
            }
        },
        Some(Commands::Tune(cmd)) => match cmd {
            TuneCommands::Params(args) => {
                if args.explore {
                    let trait_name = args
                        .trait_name
                        .expect("--trait-name is required for explore");
                    explore::run(&args.src, &trait_name, &args.method_name, args.threshold);
                } else if let Some(params) = &args.params {
                    let opt = parameter_optimizer::ParameterOptimizer::new(
                        args.test,
                        params,
                        args.iterations,
                    );
                    opt.run();
                } else {
                    eprintln!("Optimize: Please specify either --explore or --params <PARAMS>.");
                    std::process::exit(1);
                }
            }
            TuneCommands::Profile(args) => {
                if !profiler::run_profile(args.test.as_deref(), args.bin.as_deref(), &args.tool) {
                    std::process::exit(1);
                }
            }
            TuneCommands::Layout(args) => {
                let engine = struct_layout::StructLayoutTuner::new(&args.target_dir);
                if let Err(e) = engine.run() {
                    eprintln!("CovOpt Error: {:?}", e);
                    std::process::exit(1);
                }
            }
            TuneCommands::Vectorize(args) => {
                let engine = auto_simd::AutoSimd::new(&args.target_dir);
                if let Err(e) = engine.run() {
                    eprintln!("CovOpt Error: {:?}", e);
                    std::process::exit(1);
                }
            }
            TuneCommands::Pgo(args) => {
                let cov_map = coverage::CoverageMap::default();
                let engine =
                    pgo_injector::PgoInjector::new(&args.target_dir, cov_map, args.threshold);
                if let Err(e) = engine.run() {
                    eprintln!("CovOpt Error: {:?}", e);
                    std::process::exit(1);
                }
            }
            TuneCommands::Refactor(args) => {
                let engine = auto_refactor::AutoRefactor::new(&args.target_dir);
                if let Err(e) = engine.run() {
                    eprintln!("CovOpt Error: {:?}", e);
                    std::process::exit(1);
                }
            }
        },
        Some(Commands::Harden(cmd)) => match cmd {
            HardenCommands::Run(args) => {
                let mut success = true;
                let run_all = !args.mutate && !args.fuzz && !args.sanitize;

                if (args.sanitize || run_all)
                    && !harden::run_sanitizer(&args.test, &args.san_type, args.auto_fix)
                {
                    success = false;
                }
                if (args.mutate || run_all) && success && !harden::run_mutants(&args.test) {
                    success = false;
                }
                if (args.fuzz || run_all) && success && !harden::run_fuzz(&args.test) {
                    success = false;
                }

                if !success {
                    std::process::exit(1);
                }
            }
            HardenCommands::Fuzz(args) => {
                let engine = auto_harness::AutoHarness::new(&args.target_dir);
                if let Err(e) = engine.generate() {
                    eprintln!("CovOpt Error: {:?}", e);
                    std::process::exit(1);
                }
            }
        },
        Some(Commands::Ci(args)) => {
            let config = match config::CovOptConfig::load(".covopt.toml") {
                Ok(c) => c,
                Err(e) => {
                    eprintln!(
                        "CovOpt-Analyzer: Failed to load config (.covopt.toml) - {}",
                        e
                    );
                    eprintln!("Please run `covopt init` to initialize the project first.");
                    std::process::exit(1);
                }
            };
            if let Err(e) = ci::run_pipeline(config, &args) {
                eprintln!("CI Pipeline failed: {}", e);
                std::process::exit(1);
            }
        }

        None => {
            if cli.run_args.test.is_some() {
                if !commands::run_analysis(&cli.run_args, false, None) {
                    std::process::exit(1);
                }
            } else {
                eprintln!("No command provided. Use `covopt --help` for usage.");
                std::process::exit(1);
            }
        }
    }
}
