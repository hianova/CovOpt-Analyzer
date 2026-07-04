#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq)]
pub enum Complexity {
    O1,
    OLogN,
    ON,
    ONLogN,
    ON2,
    O2N,
    OSqrtN,
}

impl Complexity {
    /// Convert N into its theoretical mathematical value f(N).
    pub fn to_fn_value(&self, n: usize) -> f64 {
        let n = n as f64;
        match self {
            Complexity::O1 => 1.0,
            Complexity::OLogN => n.log2().max(1.0), // Prevent log(0) issues, though N shouldn't be 0
            Complexity::ON => n,
            Complexity::ONLogN => n * n.log2().max(1.0),
            Complexity::ON2 => n * n,
            Complexity::O2N => {
                // To prevent f64 infinity, we might cap N if it's too large, or just use 2.0_f64.powf.
                if n > 1023.0 {
                    f64::MAX
                } else {
                    2.0_f64.powf(n)
                }
            }
            Complexity::OSqrtN => n.sqrt(),
        }
    }

    /// List all complexities for comparison.
    pub fn all() -> &'static [Complexity] {
        &[
            Complexity::O1,
            Complexity::OLogN,
            Complexity::ON,
            Complexity::ONLogN,
            Complexity::ON2,
            Complexity::O2N,
            Complexity::OSqrtN,
        ]
    }
}

#[derive(Debug)]
pub struct AnalysisReport {
    pub is_converged: bool,
    pub expected: Complexity,
    pub r_squared: f64,
    pub actual_trend: Complexity,
}

pub struct ConvergenceAnalyzer;

impl ConvergenceAnalyzer {
    /// Calculate R-squared (Coefficient of Determination) for a given complexity model.
    /// Data is an array of (N, Hit_Count) pairs.
    pub fn calculate_r_squared(data: &[(usize, u64)], complexity: Complexity) -> f64 {
        if data.is_empty() {
            return 0.0;
        }

        let n = data.len() as f64;

        // x represents theoretical f(N), y represents actual Hit_Count
        let mut x_values = Vec::with_capacity(data.len());
        let mut y_values = Vec::with_capacity(data.len());

        for &(size, hit_count) in data {
            x_values.push(complexity.to_fn_value(size));
            y_values.push(hit_count as f64);
        }

        let sum_x: f64 = x_values.iter().sum();
        let sum_y: f64 = y_values.iter().sum();

        let mean_x = sum_x / n;
        let mean_y = sum_y / n;

        // Calculate coefficients for simple linear regression: y = cx + b
        let mut sum_xy_diff = 0.0;
        let mut sum_xx_diff = 0.0;

        for i in 0..data.len() {
            let x_diff = x_values[i] - mean_x;
            let y_diff = y_values[i] - mean_y;
            sum_xy_diff += x_diff * y_diff;
            sum_xx_diff += x_diff * x_diff;
        }

        let c = if sum_xx_diff.abs() < 1e-9 {
            // If all x values are the same (e.g. O(1)), c is 0
            0.0
        } else {
            sum_xy_diff / sum_xx_diff
        };

        let b = mean_y - c * mean_x;

        // Calculate Total Sum of Squares (SST) and Residual Sum of Squares (SSR)
        let mut ss_tot = 0.0;
        let mut ss_res = 0.0;

        for i in 0..data.len() {
            let y = y_values[i];
            let predicted_y = c * x_values[i] + b;

            ss_tot += (y - mean_y).powi(2);
            ss_res += (y - predicted_y).powi(2);
        }

        if ss_tot.abs() < 1e-9 {
            // Hit counts are perfectly flat (constant).
            if complexity == Complexity::O1 {
                return 1.0; // Perfect O(1) match
            } else {
                // Better than expected scaling. For curve fitting, it means the variable
                // didn't impact it, but technically R^2 is undefined/1.0 for flat lines.
                // We'll return 0.0 because the variable X explains 0% of the variance (there is no variance).
                return 0.0;
            }
        }

        1.0 - (ss_res / ss_tot)
    }

    /// Analyze convergence given data and expected complexity.
    pub fn analyze(data: &[(usize, u64)], expected: Complexity) -> AnalysisReport {
        let expected_r2 = Self::calculate_r_squared(data, expected);

        // Find the best fitting complexity model
        let mut best_complexity = expected;
        let mut max_r2 = expected_r2;

        for &comp in Complexity::all() {
            let r2 = Self::calculate_r_squared(data, comp);
            // We want the simplest complexity that fits well.
            // But for now, we just pick the one with max R^2.
            if r2 > max_r2 {
                max_r2 = r2;
                best_complexity = comp;
            }
        }

        // We consider it converged if R^2 is >= 0.95
        let is_converged = expected_r2 >= 0.95;

        AnalysisReport {
            is_converged,
            expected,
            r_squared: expected_r2,
            actual_trend: best_complexity,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_perfect_ologn_convergence() {
        // N values: 10, 100, 1000
        // Let's assume H(N) = 2 * log2(N) + 5
        let data = vec![(10, 11), (100, 18), (1000, 25)];

        let report = ConvergenceAnalyzer::analyze(&data, Complexity::OLogN);

        assert!(report.is_converged, "Should converge to O(logN)");
        assert_eq!(report.expected, Complexity::OLogN);
        assert_eq!(report.actual_trend, Complexity::OLogN);
        assert!(report.r_squared > 0.95);
    }

    #[test]
    fn test_divergence_to_on() {
        // Expected O(logN), but actual is O(N)
        let data = vec![(10, 10), (100, 100), (1000, 1000)];

        let report = ConvergenceAnalyzer::analyze(&data, Complexity::OLogN);

        assert!(!report.is_converged, "Should not converge to O(logN)");
        assert_eq!(report.expected, Complexity::OLogN);
        assert_eq!(report.actual_trend, Complexity::ON);
        assert!(report.r_squared < 0.95);
    }

    #[test]
    fn test_perfect_o1_convergence() {
        // H(N) = 42 constant
        let data = vec![(10, 42), (100, 42), (1000, 42)];

        let report = ConvergenceAnalyzer::analyze(&data, Complexity::O1);

        assert!(report.is_converged, "Should converge to O(1)");
        assert_eq!(report.actual_trend, Complexity::O1);
        assert_eq!(report.r_squared, 1.0);
    }

    #[test]
    fn test_empty_data() {
        let data = vec![];
        let r2 = ConvergenceAnalyzer::calculate_r_squared(&data, Complexity::ON);
        assert_eq!(r2, 0.0);
    }

    #[test]
    fn test_flat_non_o1() {
        let data = vec![(10, 42), (100, 42), (1000, 42)];
        let r2 = ConvergenceAnalyzer::calculate_r_squared(&data, Complexity::ON);
        assert_eq!(r2, 0.0); // Explained variance is 0% because true variance is 0
    }

    #[test]
    fn test_calculate_r_squared_complexity() {
        // Read N from environment variable for CovOpt
        let n: usize = std::env::var("COVOPT_N")
            .unwrap_or_else(|_| "100".to_string())
            .parse()
            .unwrap_or(100);

        // Generate N data points
        let mut data = Vec::with_capacity(n);
        for i in 0..n {
            data.push((i, i as u64));
        }

        // Run the function we want to measure
        let _r2 = ConvergenceAnalyzer::calculate_r_squared(&data, Complexity::ON);
    }
}
