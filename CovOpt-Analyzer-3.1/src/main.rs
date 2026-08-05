#![allow(non_snake_case)]
pub mod auto_fixer;
pub mod auto_harness;
pub mod ci;
pub mod commands;
pub mod concurrency_fuzzer;
pub mod dashboard;
pub mod explore;
pub mod harden;

use CovOpt_Analyzer::config::{
    AdviseArgs, AtomicArgs, AuditArgs, CheckArgs, CiArgs, ConvergeArgs, FixArgs, FuzzArgs,
    HardenArgs, InitArgs, InspectCommandArgs, PlanArgs, ProfileArgs, ReportArgs, RunArgs,
    SelectTrialsArgs, UnifiedOptimizeArgs, VerifyArgs,
};
use clap::{Parser, Subcommand, Args};

#[derive(Args, Debug)]
pub struct EvolveArgs {
    #[clap(short, long)]
    pub target: Option<String>,
}

#[derive(Parser, Debug)]
#[command(name = "covopt")]
#[command(author, version, about = "Coverage-based Complexity & Safety Analyzer")]
#[command(
    after_help = "EXAMPLES:\n  1. Autonomous loop:      covopt converge\n  2. Setup:                covopt init\n  3. Check guarantees:     covopt check --mode adaptive\n  4. Explain findings:     covopt inspect --format json\n  5. Explore candidates:   covopt optimize codegen\n  6. Force evidence:       covopt verify coverage"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[clap(flatten)]
    run_args: RunArgs,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// 🚀 Phase 4: Execute the Top-Down Evolutionary Engine on #[covopt_evolve] targets
    Evolve(EvolveArgs),

    /// Infer or load a GoalSpec, verify exact candidates, and converge safely
    Converge(ConvergeArgs),

    /// Optionally persist the default project policy as .covopt.toml
    Init(InitArgs),

    /// Check obligations and collect planner-selected evidence
    Check(CheckArgs),

    /// Explain structured findings and repair candidates
    Inspect(InspectCommandArgs),

    /// Search optimization candidates without applying them
    Optimize(UnifiedOptimizeArgs),

    /// Plan or apply a minimal repair set
    Fix(FixArgs),

    /// Force execution of a dynamic evidence provider
    Verify(VerifyArgs),

    /// Legacy CI alias
    #[command(hide = true)]
    Ci(CiArgs),

    /// Legacy report alias
    #[command(hide = true)]
    Report(ReportArgs),

    /// Legacy audit alias
    #[command(hide = true)]
    Audit(AuditArgs),

    /// Plan the lowest-cost evidence actions without executing them
    #[command(hide = true)]
    Plan(PlanArgs),

    /// Select a deterministic, budgeted target/N/seed trial set without running tests
    #[command(hide = true)]
    SelectTrials(SelectTrialsArgs),

    /// Analyze or synthesize opt-in atomic orderings
    #[command(hide = true)]
    Atomic(AtomicArgs),

    /// Deprecated compatibility alias
    #[command(hide = true)]
    Advise(AdviseArgs),

    /// CPU hotspot & lock contention profiler (Flamegraph & Samply)
    #[command(hide = true)]
    Profile(ProfileArgs),

    /// Robustness & Security Hardening (Mutation, Fuzzing, Sanitizers)
    #[command(hide = true)]
    Harden(HardenArgs),

    /// Adversarial Concurrency Fuzzer (AST-based In-Process Heuristic Fuzzing)
    #[command(hide = true)]
    Fuzz(FuzzArgs),
}

fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "covopt" {
        args.remove(1);
    }
    let cli = Cli::parse_from(args);

    match cli.command {
        Some(Commands::Evolve(args)) => {
            commands::run_evolve(&args);
        }
        Some(Commands::Converge(args)) => {
            if !commands::run_converge(&args) {
                std::process::exit(1);
            }
        }
        Some(Commands::Init(args)) => {
            if args.hook {
                commands::install_hook(args.path.as_deref());
            } else if args.migrate {
                commands::migrate_config(args.path.as_deref());
            } else {
                commands::init_config(args);
            }
        }
        Some(Commands::Check(args)) => {
            if let Err(error) = commands::run_check(&args) {
                eprintln!("CovOpt check: {error}");
                std::process::exit(1);
            }
        }
        Some(Commands::Fix(args)) => {
            if args.plan || args.apply || args.rollback.is_some() {
                if !commands::run_repair_plan(&args) {
                    std::process::exit(1);
                }
            } else {
                let run_all = !args.only_clippy
                    && !args.only_magic
                    && !args.legacy_clippy
                    && !args.legacy_magic;
                if args.only_clippy || args.legacy_clippy || run_all {
                    commands::run_fix(args.path.clone());
                }
                if (args.only_magic || args.legacy_magic || run_all)
                    && let Err(error) = CovOpt_Analyzer::scanner::run_scan(args.path, true, false)
                {
                    eprintln!("CovOpt fix: {error}");
                    std::process::exit(1);
                }
            }
        }
        Some(Commands::Verify(args)) => {
            if !commands::run_verify(&args) {
                std::process::exit(1);
            }
        }
        Some(Commands::Report(args)) => {
            eprintln!("covopt report is a compatibility alias; use covopt check --format");
            if let Err(error) = commands::run_check(&CheckArgs {
                base: None,
                target: None,
                mode: Some(CovOpt_Analyzer::assurance::AssurancePolicy::Adaptive),
                plan: false,
                format: args.format,
                fast: false,
                staged: false,
                debug_artifacts: false,
                budget: "5m".to_string(),
            }) {
                eprintln!("CovOpt Error: {error}");
                std::process::exit(1);
            }
        }
        Some(Commands::Audit(args)) => {
            eprintln!("covopt audit is a compatibility alias; use covopt check --mode strict");
            if let Err(error) = commands::run_check(&CheckArgs {
                base: args.base,
                target: args.test,
                mode: Some(CovOpt_Analyzer::assurance::AssurancePolicy::Strict),
                plan: false,
                format: if args.json {
                    "json".to_string()
                } else {
                    "text".to_string()
                },
                fast: args.fast,
                staged: args.staged,
                debug_artifacts: args.debug_artifacts,
                budget: "5m".to_string(),
            }) {
                eprintln!("CovOpt audit: {error}");
                std::process::exit(1);
            }
        }
        Some(Commands::Plan(args)) => {
            let config =
                match CovOpt_Analyzer::config::CovOptConfig::load_or_embedded(".covopt.toml") {
                    Ok(config) => config,
                    Err(error) => {
                        eprintln!("CovOpt plan: failed to load .covopt.toml: {}", error);
                        std::process::exit(1);
                    }
                };
            if !commands::run_plan(&args, &config) {
                std::process::exit(1);
            }
        }
        Some(Commands::SelectTrials(args)) => {
            let config =
                match CovOpt_Analyzer::config::CovOptConfig::load_or_embedded(".covopt.toml") {
                    Ok(config) => config,
                    Err(error) => {
                        eprintln!(
                            "CovOpt select-trials: failed to load .covopt.toml: {}",
                            error
                        );
                        std::process::exit(1);
                    }
                };
            if !commands::run_select_trials(&args, &config) {
                std::process::exit(1);
            }
        }
        Some(Commands::Atomic(args)) => {
            let config =
                match CovOpt_Analyzer::config::CovOptConfig::load_or_embedded(".covopt.toml") {
                    Ok(config) => config,
                    Err(error) => {
                        eprintln!("CovOpt atomic: failed to load .covopt.toml: {}", error);
                        std::process::exit(1);
                    }
                };
            if !commands::run_atomic(&args, &config) {
                std::process::exit(1);
            }
        }
        Some(Commands::Optimize(args)) => {
            if !commands::run_unified_optimize(&args) {
                std::process::exit(1);
            }
        }
        Some(Commands::Profile(args)) => {
            eprintln!("covopt profile is a compatibility alias; use covopt verify runtime");
            if !commands::run_verify(&VerifyArgs {
                command: CovOpt_Analyzer::config::VerifySubcommand::Runtime(
                    CovOpt_Analyzer::config::VerifyRuntimeArgs {
                        target: args.test,
                        tool: args.tool,
                        bin: args.bin,
                        json: false,
                    },
                ),
            }) {
                std::process::exit(1);
            }
        }
        Some(Commands::Inspect(args)) => {
            if let Err(e) = commands::run_inspect_command(&args) {
                eprintln!("CovOpt Error: {:?}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Advise(args)) => {
            eprintln!("covopt advise is a compatibility alias; use covopt inspect");
            if let Err(e) = commands::run_inspect(&args) {
                eprintln!("CovOpt Error: {:?}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Harden(args)) => {
            eprintln!("covopt harden is a compatibility alias; use covopt verify safety");
            let Some(target) = args.test else {
                eprintln!("covopt verify safety requires --target");
                std::process::exit(1);
            };
            if !commands::run_verify(&VerifyArgs {
                command: CovOpt_Analyzer::config::VerifySubcommand::Safety(
                    CovOpt_Analyzer::config::VerifySafetyArgs {
                        target: Some(target),
                        sanitizer: args.san_type,
                        json: false,
                    },
                ),
            }) {
                std::process::exit(1);
            }
        }
        Some(Commands::Ci(args)) => {
            eprintln!("covopt ci is a compatibility alias; use covopt check");
            let format = if args.sarif {
                "sarif"
            } else if args.report {
                "html"
            } else {
                "text"
            };
            if let Err(error) = commands::run_check(&CheckArgs {
                base: args.base,
                target: None,
                mode: Some(
                    args.assurance
                        .unwrap_or(CovOpt_Analyzer::assurance::AssurancePolicy::Adaptive),
                ),
                plan: false,
                format: format.to_string(),
                fast: args.fast,
                staged: false,
                debug_artifacts: false,
                budget: args.budget,
            }) {
                eprintln!("CI pipeline failed: {error}");
                std::process::exit(1);
            }
        }
        Some(Commands::Fuzz(args)) => {
            eprintln!("covopt fuzz is a compatibility alias; use covopt verify concurrency");
            if !commands::run_verify(&VerifyArgs {
                command: CovOpt_Analyzer::config::VerifySubcommand::Concurrency(
                    CovOpt_Analyzer::config::VerifyConcurrencyArgs {
                        target: Some(args.target),
                        timeout_ms: args.timeout_ms,
                        max_iters: args.max_iters,
                        seed: 0,
                        json: false,
                    },
                ),
            }) {
                std::process::exit(1);
            }
        }
        None => {
            if cli.run_args.test.is_some() {
                if !commands::run_analysis(&cli.run_args, false, None, false, None, None) {
                    std::process::exit(1);
                }
            } else {
                eprintln!("No command provided. Use `covopt --help` for usage.");
                std::process::exit(1);
            }
        }
    }
}
