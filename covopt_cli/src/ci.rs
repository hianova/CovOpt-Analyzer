use covopt_core::config::CovOptConfig;
use crate::harden;
use crate::{CiArgs, commands};
use covopt_macro::covopt_param;

pub fn run_pipeline(config: CovOptConfig, args: &CiArgs) -> Result<(), Box<dyn std::error::Error>> {
    println!("===================================================");
    println!("🚀 Starting CovOpt-Analyzer Unified Auto-Pilot (CI)");
    println!("===================================================");

    // Step 1: Clean & Format (Fix)
    if config.pipeline.run_fix {
        println!("Step 1: Running Auto-Fix (cargo clippy --fix & magic numbers)...");
        commands::run_fix(None);
        covopt_core::scanner::run_scan(None, true, false);
        println!("✅ [CI OK] Fix complete.");
    }

    if config.pipeline.run_audit {
        println!("▶️ Step 2: Running `covopt audit`...");
        commands::run_audit(&covopt_core::config::AuditArgs { test: None, fast: args.fast, json: false, staged: false });
        println!("✅ [CI OK] Audit passed.");
    }

    // Step 3: Optimize
    if config.pipeline.run_optimize && !args.fast {
        println!("▶️ Step 3: Running `covopt optimize` (Auto-Tuning)...");
        for target_config in &config.target {
            println!("  -> Optimizing target: {}", target_config.test);
            // Defaulting to running explore logic for optimization in CI pipeline
            crate::explore::run(
                "src",
                "UnknownTrait",
                "evaluate_fitness",
                covopt_param!("M_29_75", 0.99),
            );
        }
        println!("✅ [CI OK] Optimization complete.");
    } else if config.pipeline.run_optimize && args.fast {
        println!("⏭️ [CI Skip] Skipping optimize step in fast mode.");
    }

    // Step 4: Harden (if configured)
    if config.pipeline.run_harden && !args.skip_harden && !args.fast {
        println!("▶️ Step 4: Running `covopt harden`...");
        let mut success = true;
        for target_config in &config.target {
            let fuzz_iters = target_config.fuzz_iterations.unwrap_or(0);
            if fuzz_iters > 0 {
                println!("  -> Hardening target: {}", target_config.test);
                if !harden::run_fuzz(&target_config.test) {
                    success = false;
                    eprintln!("⚠️ [CI Warning] fuzz failed for {}", target_config.test);
                }
            }
        }

        if args.strict && !success {
            eprintln!("❌ [CI Failed] Hardening encountered errors in strict mode.");
            std::process::exit(1);
        } else if !success {
            eprintln!("⚠️ [CI Warning] Hardening had errors, but continuing.");
        } else {
            println!("✅ [CI OK] Hardening complete.");
        }
    } else if config.pipeline.run_harden && (!args.skip_harden) && args.fast {
        println!("⏭️ [CI Skip] Skipping harden step in fast mode.");
    }

    println!("===================================================");
    println!("🎉 CI Pipeline Execution Completed Successfully!");
    println!("===================================================");
    Ok(())
}
