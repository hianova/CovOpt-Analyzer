use rand::RngExt;
use std::collections::HashMap;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct ParamRange {
    pub min: f64,
    pub max: f64,
    pub is_int: bool,
}

pub struct ParameterOptimizer {
    pub params: HashMap<String, ParamRange>,
    pub target_test: String,
    pub iterations: usize,
}

impl ParameterOptimizer {
    pub fn new(target_test: String, param_str: &str, iterations: usize) -> Self {
        let mut params = HashMap::new();
        for p in param_str.split(',') {
            let parts: Vec<&str> = p.split(':').collect();
            if parts.len() == 2 {
                let name = parts[0].trim().to_string();
                let range_parts: Vec<&str> = parts[1].split("..").collect();
                if range_parts.len() == 2
                    && let (Ok(min), Ok(max)) =
                        (range_parts[0].parse::<f64>(), range_parts[1].parse::<f64>())
                {
                    let is_int = !parts[1].replace("..", "").contains('.');
                    params.insert(name, ParamRange { min, max, is_int });
                }
            }
        }
        Self {
            params,
            target_test,
            iterations,
        }
    }

    pub fn run(&self) {
        println!("🚀 Starting CovOpt Parameter Tuning Engine...");
        println!("Target Test: {}", self.target_test);
        println!("Parameters: {:?}", self.params);

        let mut current_bounds = self.params.clone();
        let mut best_score = -f64::MAX;
        let mut best_params = HashMap::new();
        let mut rng = rand::rng();

        let epochs = 5;
        let samples_per_epoch = if self.iterations < epochs {
            1
        } else {
            self.iterations / epochs
        };

        for epoch in 1..=epochs {
            println!(
                "🔍 [Epoch {}/{}] Logarithmic Bound Refinement:",
                epoch, epochs
            );
            for (name, bounds) in &current_bounds {
                println!("    - {}: [{}, {}]", name, bounds.min, bounds.max);
            }

            let mut epoch_best_score = -f64::MAX;
            let mut epoch_best_params = HashMap::new();

            for i in 1..=samples_per_epoch {
                let mut current_params = HashMap::new();
                for (name, range) in &current_bounds {
                    let mut val = if range.min >= range.max {
                        range.min
                    } else {
                        rng.random_range(range.min..=range.max)
                    };
                    if range.is_int {
                        val = val.round();
                    }
                    current_params.insert(name.clone(), val);
                }

                let score = self.evaluate(&current_params);
                if score > epoch_best_score {
                    epoch_best_score = score;
                    epoch_best_params = current_params.clone();
                }

                if score > best_score {
                    best_score = score;
                    best_params = current_params.clone();
                    println!(
                        "  [Iter {}/{}] 🎉 New Global Best Score: {:.4} | Params: {:?}",
                        i, samples_per_epoch, best_score, best_params
                    );
                } else {
                    println!(
                        "  [Iter {}/{}] Score: {:.4} | Params: {:?}",
                        i, samples_per_epoch, score, current_params
                    );
                }
            }

            // Logarithmic Bound Shrinking
            for (name, bounds) in current_bounds.iter_mut() {
                if let Some(&best_val) = epoch_best_params.get(name) {
                    let half_range = (bounds.max - bounds.min) / 4.0;
                    let mut new_min = best_val - half_range;
                    let mut new_max = best_val + half_range;

                    let global_bounds = self.params.get(name).unwrap();
                    if new_min < global_bounds.min {
                        new_min = global_bounds.min;
                    }
                    if new_max > global_bounds.max {
                        new_max = global_bounds.max;
                    }

                    if bounds.is_int {
                        new_min = new_min.floor();
                        new_max = new_max.ceil();
                        if new_min == new_max {
                            if new_min > global_bounds.min {
                                new_min -= 1.0;
                            }
                            if new_max < global_bounds.max {
                                new_max += 1.0;
                            }
                        }
                    }

                    bounds.min = new_min;
                    bounds.max = new_max;
                }
            }
        }

        println!("\n🏆 Logarithmic NP-Hard Optimization Complete!");
        println!("Best Score: {:.4}", best_score);
        println!("Best Parameters: {:?}", best_params);

        let env_content: String = best_params
            .iter()
            .map(|(k, v)| format!("COVOPT_PARAM_{}={}", k, v))
            .collect::<Vec<String>>()
            .join("\n");

        let _ = std::fs::write(
            ".covopt_tuned.env",
            format!(
                "# Auto-generated by CovOpt-Analyzer\n# Target Test: {}\n# Best Score: {:.4}\n{}",
                self.target_test, best_score, env_content
            ),
        );
        println!("💾 Saved optimal parameters to `.covopt_tuned.env`");
    }

    fn evaluate(&self, params: &HashMap<String, f64>) -> f64 {
        let mut cmd = Command::new("cargo");
        cmd.args(["bench", "--bench", &self.target_test]);

        for (name, val) in params {
            let env_name = format!("COVOPT_PARAM_{}", name);
            cmd.env(&env_name, val.to_string());
        }

        let output = cmd.output().expect("Failed to execute cargo test");
        let stdout = String::from_utf8_lossy(&output.stdout);

        for line in stdout.lines() {
            if line.contains("COVOPT_SCORE:") {
                let parts: Vec<&str> = line.split("COVOPT_SCORE:").collect();
                if parts.len() == 2
                    && let Ok(score) = parts[1].trim().parse::<f64>()
                {
                    return score;
                }
            }
        }

        // If the test crashes or doesn't output a score, return a very low score
        -f64::MAX
    }
}
