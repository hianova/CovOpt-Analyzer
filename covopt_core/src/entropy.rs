use crate::config::TargetConfig;
use crate::runner::CargoTestRunner;
use covopt_macro::covopt_param;
use std::fmt::Write;
use std::process::Command;

pub struct EntropyResult {
    pub fuzz_variance_score: f64, // 0 - 30
    pub branch_sprawl_score: f64, // 0 - 40
    pub cli_noise_score: f64,     // 0 - 30
    pub total_score: f64,         // 0 - 100
}

pub fn calculate_entropy_score(config: &TargetConfig, compact: bool) -> EntropyResult {
    let mut details = String::new();
    let _ = writeln!(details, "\n[Entropy Analyzer] Starting Evaluation...");
    let cli_noise = compute_cli_noise(&mut details);
    let fuzz_variance = compute_fuzz_variance(config, &mut details);
    let branch_sprawl = compute_branch_sprawl(config, &mut details);

    let total = fuzz_variance + branch_sprawl + cli_noise;

    if !compact || total > covopt_param!("M_22_27", 50.0) {
        print!("{}", details);
    }

    EntropyResult {
        fuzz_variance_score: fuzz_variance,
        branch_sprawl_score: branch_sprawl,
        cli_noise_score: cli_noise,
        total_score: total,
    }
}

fn compute_cli_noise(details: &mut String) -> f64 {
    let _ = writeln!(details, "  -> Calculating CLI Noise Index (C)...");
    let output = Command::new("cargo")
        .args(["check", "--message-format=json"])
        .output()
        .expect("Failed to run cargo check");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut warning_count = 0;

    for line in stdout.lines() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line)
            && let Some(msg) = v.get("message")
            && let Some(level) = msg.get("level").and_then(|l| l.as_str())
        {
            if level == "warning" {
                warning_count += 1;
            } else if level == "error" || level == "error: internal compiler error" {
                warning_count += covopt_param!("M_52_33", 5); // Heavily penalize errors/ICE
            }
        }
    }

    // Each warning adds 2 points to entropy, up to 30.
    let score =
        (warning_count as f64 * covopt_param!("M_58_40", 2.0)).min(covopt_param!("M_58_49", 30.0));
    let _ = writeln!(
        details,
        "     Found {} warnings. CLI Noise Score: {:.1}/30.0",
        warning_count, score
    );
    score
}

fn compute_fuzz_variance(config: &TargetConfig, details: &mut String) -> f64 {
    let _ = writeln!(details, "  -> Calculating Fuzz-Cov Variance (A)...");
    let iterations = config
        .fuzz_iterations
        .unwrap_or(covopt_param!("M_69_54", 10));
    let n_value = covopt_param!("M_70_18", 100); // Use a fixed N for fuzzing loops

    let output_dir = tempfile::tempdir().unwrap().path().to_path_buf();
    let mut packages_to_compile = Vec::new();
    if let Some(pkg) =
        crate::static_analysis::resolve_package_for_target(&config.test, config.package.as_ref())
    {
        packages_to_compile.push(pkg);
    }
    let executables = crate::runner::compile_workspace_tests(&output_dir, &packages_to_compile)
        .unwrap_or_default();
    let runner = crate::runner::CargoTestRunner::new(&config.test, &output_dir, executables);

    let hit_counts: Vec<f64> = (0..iterations)
        .into_iter()
        .filter_map(|i| {
            let seed = i as u64 * covopt_param!("M_85_34", 1337) + covopt_param!("M_85_41", 1000);
            let iter_dir = tempfile::tempdir()
                .expect("Failed to create tempdir")
                .path()
                .to_path_buf();
            let local_runner = crate::runner::CargoTestRunner::new(
                &config.test,
                &iter_dir,
                runner.executables.clone(),
            );

            if let Ok((map, _)) = local_runner.run(n_value, Some(seed))
                && let Some((_, _, _, hits)) =
                    map.find_peak_location(config.ignore.as_deref().unwrap_or(&[]), None)
            {
                Some(hits as f64)
            } else {
                None
            }
        })
        .collect();

    if hit_counts.is_empty() {
        let msg = if crate::config::should_color() {
            "     \x1b[31m[WARN] Failed to gather Fuzz-Cov data. (Did your test panic or lack loops?) Defaulting to 15.0 penalty.\x1b[0m"
        } else {
            "     [WARN] Failed to gather Fuzz-Cov data. (Did your test panic or lack loops?) Defaulting to 15.0 penalty."
        };
        let _ = writeln!(details, "{}", msg);
        return covopt_param!("M_107_15", 15.0);
    }

    let mean = hit_counts.iter().sum::<f64>() / hit_counts.len() as f64;
    let variance = hit_counts
        .iter()
        .map(|value| {
            let diff = mean - *value;
            diff * diff
        })
        .sum::<f64>()
        / hit_counts.len() as f64;

    let std_dev = variance.sqrt();
    let cv = if mean > 0.0 { std_dev / mean } else { 0.0 }; // Coefficient of variation

    // CV > 0.5 means highly unstable -> score 30
    let score = (cv * covopt_param!("M_124_22", 60.0)).min(covopt_param!("M_124_32", 30.0));
    let _ = writeln!(
        details,
        "     Fuzz Variance (StdDev: {:.1}, Mean: {:.1}, CV: {:.2}). Score: {:.1}/30.0",
        std_dev, mean, cv, score
    );
    score
}

fn compute_branch_sprawl(config: &TargetConfig, details: &mut String) -> f64 {
    let _ = writeln!(details, "  -> Calculating API Branch Sprawl (B)...");

    let tests_str = match &config.tests {
        Some(t) => t,
        None => {
            let _ = writeln!(
                details,
                "     No `tests` field provided for multi-scenario. Defaulting to 0 branch sprawl."
            );
            return 0.0;
        }
    };

    let test_cases: Vec<&str> = tests_str.split(',').map(|s| s.trim()).collect();
    if test_cases.len() < 2 {
        let _ = writeln!(
            details,
            "     Need at least 2 tests to measure branch sprawl. Defaulting to 0."
        );
        return 0.0;
    }

    let mut covered_lines_per_test: Vec<std::collections::HashSet<u64>> = Vec::new();
    let output_dir = tempfile::tempdir()
        .expect("Failed to create tempdir")
        .path()
        .to_path_buf();

    let mut packages_to_compile = Vec::new();
    if let Some(pkg) =
        crate::static_analysis::resolve_package_for_target(&config.test, config.package.as_ref())
    {
        packages_to_compile.push(pkg);
    }
    let executables = crate::runner::compile_workspace_tests(&output_dir, &packages_to_compile)
        .unwrap_or_default();

    for tc in &test_cases {
        let runner = CargoTestRunner::new(tc, &output_dir, executables.clone());
        if let Ok((map, _)) = runner.run(covopt_param!("M_170_41", 100), None) {
            let mut lines = std::collections::HashSet::new();
            if let Some((target_file, _, _, _)) =
                map.find_peak_location(config.ignore.as_deref().unwrap_or(&[]), None)
            {
                for (file, file_cov) in &map.hit_counts {
                    if file == &target_file {
                        for (&line_number, &count) in file_cov {
                            if count > 0 {
                                lines.insert(line_number);
                            }
                        }
                    }
                }
            }
            covered_lines_per_test.push(lines);
        }
    }

    if covered_lines_per_test.len() < 2 {
        return covopt_param!("M_188_15", 20.0); // Fail safe
    }

    let mut intersection = covered_lines_per_test[0].clone();
    let mut union = covered_lines_per_test[0].clone();

    for lines in covered_lines_per_test.iter().skip(1) {
        intersection.retain(|x| lines.contains(x));
        union.extend(lines);
    }

    let intersection_count = intersection.len() as f64;
    let union_count = union.len() as f64;

    let ratio = if union_count > 0.0 {
        intersection_count / union_count
    } else {
        1.0
    };
    // ratio 1.0 -> score 0. ratio 0.0 -> score 40.
    let score = (1.0 - ratio) * covopt_param!("M_208_32", 40.0);
    let _ = writeln!(
        details,
        "     Branch Sprawl (Intersection: {}, Union: {}, Ratio: {:.2}). Score: {:.1}/40.0",
        intersection_count, union_count, ratio, score
    );
    score
}
