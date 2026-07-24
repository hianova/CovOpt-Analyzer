pub mod auto_fixer;
pub mod auto_harness;
pub mod ci;
pub mod commands;
pub mod dashboard;
pub mod explore;
pub mod harden;

use clap::{Parser, Subcommand};
use covopt_core::config::{
    RunArgs, InitArgs, CiArgs, ReportArgs, FixArgs, AuditArgs, AdviseArgs, ProfileArgs, HardenArgs
};

#[derive(Parser, Debug)]
#[command(name = "covopt")]
#[command(author, version, about = "Coverage-based Complexity & Safety Analyzer")]
#[command(after_help = "EXAMPLES:\n  1. Quick setup:          covopt init\n  2. Audit codebase:       covopt audit\n  3. Auto-fix & optimize:  covopt fix\n  4. Senior Advisor:       covopt advise\n  5. Profile CPU hotspots: covopt profile --test my_test\n  6. Auto-Pilot Pipeline:  covopt ci")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[clap(flatten)]
    run_args: RunArgs,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize a default .covopt.toml and inject AI Agent rules
    Init(InitArgs),

    /// Unified Auto-Pilot Pipeline (Fix -> Audit -> Report)
    Ci(CiArgs),

    /// Generate visual HTML or SARIF dashboard report
    Report(ReportArgs),

    /// Automatic code repair (Clippy fixes + covopt_param! substitution)
    Fix(FixArgs),

    /// Audit time/space complexity, IPC coverage & entropy across targets
    Audit(AuditArgs),

    /// Senior Engineer Advisor: Detect hot-path allocations, async blocking & lock contention
    Advise(AdviseArgs),

    /// CPU hotspot & lock contention profiler (Flamegraph & Samply)
    Profile(ProfileArgs),

    /// Robustness & Security Hardening (Mutation, Fuzzing, Sanitizers)
    Harden(HardenArgs),
}

fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "covopt" {
        args.remove(1);
    }
    let cli = Cli::parse_from(args);

    match cli.command {
        Some(Commands::Init(args)) => {
            if args.hook {
                commands::install_hook();
            } else {
                commands::init_config(args);
            }
        }
        Some(Commands::Fix(args)) => {
            let run_all = !args.only_clippy && !args.only_magic;
            if args.only_clippy || run_all {
                commands::run_fix(args.path.clone());
            }
            if args.only_magic || run_all {
                covopt_core::scanner::run_scan(args.path, true, false);
            }
        }
        Some(Commands::Report(args)) => {
            let engine = dashboard::DashboardGenerator::new(&args.output_dir);
            let res = if args.format == "sarif" {
                engine.generate_sarif()
            } else {
                engine.generate()
            };
            if let Err(e) = res {
                eprintln!("CovOpt Error: {:?}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Audit(args)) => commands::run_audit(&args),
        Some(Commands::Profile(args)) => {
            if !covopt_core::profiler::run_profile(args.test.as_deref(), args.bin.as_deref(), &args.tool) {
                std::process::exit(1);
            }
        }
        Some(Commands::Advise(args)) => {
            if let Err(e) = commands::run_advise(&args) {
                eprintln!("CovOpt Error: {:?}", e);
                std::process::exit(1);
            }
            covopt_core::dataflow::run_dataflow(Some(args.path.clone()));
        }
        Some(Commands::Harden(args)) => {
            if args.generate_harness {
                let engine = auto_harness::AutoHarness::new(&args.path);
                if let Err(e) = engine.generate() {
                    eprintln!("CovOpt Error: {:?}", e);
                    std::process::exit(1);
                }
            } else {
                let test = match &args.test {
                    Some(t) => t,
                    None => {
                        eprintln!("Error: The name of the test target is required when running hardening tests.");
                        std::process::exit(1);
                    }
                };
                let mut success = true;
                let run_all = !args.mutate && !args.fuzz && !args.sanitize;

                if args.mutate || run_all {
                    if std::process::Command::new("cargo").arg("mutants").arg("--version").output().is_err() {
                        if !args.fast {
                            eprintln!("Error: cargo-mutants is not installed.");
                            std::process::exit(1);
                        } else {
                            println!("[Pre-flight] Skipping cargo-mutants (not installed).");
                        }
                    } else if !harden::run_mutants(test) {
                        success = false;
                    }
                }
                if (args.sanitize || run_all)
                    && success
                    && !harden::run_sanitizer(test, &args.san_type, args.auto_fix)
                {
                    success = false;
                }
                if args.fuzz || run_all {
                    if std::process::Command::new("cargo").arg("fuzz").arg("--version").output().is_err() {
                        if !args.fast {
                            eprintln!("Error: cargo-fuzz is not installed.");
                            std::process::exit(1);
                        } else {
                            println!("[Pre-flight] Skipping cargo-fuzz (not installed).");
                        }
                    } else if success && !harden::run_fuzz(test) {
                        success = false;
                    }
                }

                if !success {
                    std::process::exit(1);
                }
            }
        }
        Some(Commands::Ci(args)) => {
            let config = match covopt_core::config::CovOptConfig::load(".covopt.toml") {
                Ok(c) => c,
                Err(e) => {
                    eprintln!(
                        "CovOpt-Analyzer: Failed to load config (.covopt.toml) - {}",
                        e
                    );
                    std::process::exit(1);
                }
            };
            if let Err(e) = ci::run_pipeline(config, &args) {
                eprintln!("CI Pipeline failed: {}", e);
                std::process::exit(1);
            }
            if args.report || args.sarif {
                let engine = dashboard::DashboardGenerator::new("target/covopt");
                let res = if args.sarif {
                    engine.generate_sarif()
                } else {
                    engine.generate()
                };
                if let Err(e) = res {
                    eprintln!("CovOpt Error generating report: {:?}", e);
                    std::process::exit(1);
                }
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
