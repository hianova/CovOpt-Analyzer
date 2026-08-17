#![allow(non_snake_case)]
pub mod commands;
use clap::{Parser, Subcommand};
use CovOpt_Analyzer::config::{DoctorArgs, RunArgs, RollbackArgs};

#[derive(Parser, Debug)]
#[command(name = "covopt")]
#[command(author, version, about = "CovOpt 3.1: The Crucible Evolution & ASE Engine")]
#[command(
    after_help = "EXAMPLES:
  1. Auto-discover targets and evolve:  covopt run
  2. Undo recent optimization:          covopt rollback
  3. Check toolchain dependencies:      covopt doctor"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// 🚀 Auto-discover bottlenecks and evolve the optimal architecture
    Run(RunArgs),

    /// ⏪ Undo code modifications if the synthesis breaks compilation
    Rollback(RollbackArgs),

    /// 🩺 Diagnose local toolchain dependencies (rustc, cargo, rustfmt)
    Doctor(DoctorArgs),
}

fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "covopt" {
        args.remove(1);
    }
    
    // Default to Run if no subcommand is provided
    if args.len() == 1 {
        args.push("run".to_string());
    }
    
    let cli = Cli::parse_from(args);

    match cli.command {
        Some(Commands::Run(args)) => {
            commands::run_engine(&args);
        }
        Some(Commands::Rollback(args)) => {
            commands::run_rollback(&args);
        }
        Some(Commands::Doctor(args)) => {
            commands::run_doctor(&args);
        }
        None => {
            // Unreachable because we injected "run" above if empty
            eprintln!("No command provided. Use `covopt --help` for usage.");
            std::process::exit(1);
        }
    }
}
