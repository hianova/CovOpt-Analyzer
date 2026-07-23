use covopt_macro::covopt_param;
use crate::RunArgs;
use crate::analysis::run_analysis;
use crate::config::CovOptConfig;
use crate::entropy;
use std::path::PathBuf;
pub fn run_audit() {
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
            expected: target.expected.clone(),
            n_values: target.n_values.clone(),
            mca_cpu: target.mca_cpu.clone(),
            require_cache_padding: target.require_cache_padding,
            require_branch_hints: target.require_branch_hints,
            require_aerospace_grade: target.require_aerospace_grade,
            require_watchdog_timeout: target.require_watchdog_timeout,
            require_stress_test: target.require_stress_test,
            polling_threshold: target.polling_threshold,
            formalize: false,
            optimize: false,
        };
        println!("\n===================================================");
        println!("Auditing target: {}", target.test);
        println!("===================================================");
        if !run_analysis(&args, true) {
            all_success = false;
        }
        let entropy_result = match entropy::calculate_entropy_score(&target, true) {
            Ok(res) => res,
            Err(e) => {
                eprintln!("Failed to calculate entropy: {}", e);
                std::process::exit(1);
            }
        };
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
        if entropy_result.total_score > covopt_param!("M_69_40", 50.0) {
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
