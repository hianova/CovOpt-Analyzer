use crate::config::TargetConfig;
use crate::runner::CargoTestRunner;
use std::process::Command;

pub struct EntropyResult {
    pub fuzz_variance_score: f64, // 0 - 30
    pub branch_sprawl_score: f64, // 0 - 40
    pub cli_noise_score: f64,     // 0 - 30
    pub total_score: f64,         // 0 - 100
}

pub fn calculate_entropy_score(config: &TargetConfig) -> EntropyResult {
    println!("\n[Entropy Analyzer] Starting Evaluation...");
    let cli_noise = compute_cli_noise();
    let fuzz_variance = compute_fuzz_variance(config);
    let branch_sprawl = compute_branch_sprawl(config);

    let total = fuzz_variance + branch_sprawl + cli_noise;
    EntropyResult {
        fuzz_variance_score: fuzz_variance,
        branch_sprawl_score: branch_sprawl,
        cli_noise_score: cli_noise,
        total_score: total,
    }
}

fn compute_cli_noise() -> f64 {
    println!("  -> Calculating CLI Noise Index (C)...");
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
                warning_count += 5; // Heavily penalize errors/ICE
            }
        }
    }

    // Each warning adds 2 points to entropy, up to 30.
    let score = (warning_count as f64 * 2.0).min(30.0);
    println!(
        "     Found {} warnings. CLI Noise Score: {:.1}/30.0",
        warning_count, score
    );
    score
}

fn compute_fuzz_variance(config: &TargetConfig) -> f64 {
    println!("  -> Calculating Fuzz-Cov Variance (A)...");
    let iterations = config.fuzz_iterations.unwrap_or(10);
    let mut hit_counts = Vec::new();
    let n_value = 100; // Use a fixed N for fuzzing loops

    let output_dir = tempfile::tempdir()
        .expect("Failed to create tempdir")
        .path()
        .to_path_buf();
    let runner = CargoTestRunner::new(&config.test, &output_dir);

    for i in 0..iterations {
        let seed = i as u64 * 1337 + 1000;
        if let Ok((map, _)) = runner.run(n_value, Some(seed))
            && let Some(hits) = map.find_hit_count(&config.target_file, config.target_line)
        {
            hit_counts.push(hits as f64);
        }
    }

    if hit_counts.is_empty() {
        println!("     Could not gather Fuzz-Cov data. Defaulting to 15.0");
        return 15.0;
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
    let score = (cv * 60.0).min(30.0);
    println!(
        "     Fuzz Variance (StdDev: {:.1}, Mean: {:.1}, CV: {:.2}). Score: {:.1}/30.0",
        std_dev, mean, cv, score
    );
    score
}

fn compute_branch_sprawl(config: &TargetConfig) -> f64 {
    println!("  -> Calculating API Branch Sprawl (B)...");

    let tests_str = match &config.tests {
        Some(t) => t,
        None => {
            println!(
                "     No `tests` field provided for multi-scenario. Defaulting to 0 branch sprawl."
            );
            return 0.0;
        }
    };

    let test_cases: Vec<&str> = tests_str.split(',').map(|s| s.trim()).collect();
    if test_cases.len() < 2 {
        println!("     Need at least 2 tests to measure branch sprawl. Defaulting to 0.");
        return 0.0;
    }

    let mut covered_lines_per_test: Vec<std::collections::HashSet<u64>> = Vec::new();

    for test_name in &test_cases {
        let output_dir = tempfile::tempdir()
            .expect("Failed to create tempdir")
            .path()
            .to_path_buf();
        let runner = CargoTestRunner::new(test_name, &output_dir);

        if let Ok(map) = runner.run(100, None) {
            let mut lines = std::collections::HashSet::new();
            for (file, file_cov) in &map.0.hit_counts {
                if file.contains(&config.target_file) {
                    for (&line_number, &count) in file_cov {
                        if count > 0 {
                            lines.insert(line_number);
                        }
                    }
                }
            }
            covered_lines_per_test.push(lines);
        }
    }

    if covered_lines_per_test.len() < 2 {
        return 20.0; // Fail safe
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
    let score = (1.0 - ratio) * 40.0;
    println!(
        "     Branch Sprawl (Intersection: {}, Union: {}, Ratio: {:.2}). Score: {:.1}/40.0",
        intersection_count, union_count, ratio, score
    );
    score
}
