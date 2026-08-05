/// Multipole Analytical Solver for 2D Point Vortex Dynamics
///
/// This solver analytically calculates the exact circulations needed for a set of control
/// vortices to perfectly neutralize the far-field multipole expansion of incoming chaotic turbulence.
/// By solving this linear system, we achieve zero aerodynamic drag / perturbation analytically
/// without needing genetic search.

#[derive(Clone, Copy, Debug)]
pub struct Vortex {
    pub x: f64,
    pub y: f64,
    pub gamma: f64,
}

pub struct MultipoleSolver;

impl MultipoleSolver {
    /// Computes (x + iy)^n
    fn complex_pow(x: f64, y: f64, n: usize) -> (f64, f64) {
        if n == 0 {
            return (1.0, 0.0);
        }
        let mut res_x = 1.0;
        let mut res_y = 0.0;
        for _ in 0..n {
            let new_x = res_x * x - res_y * y;
            let new_y = res_x * y + res_y * x;
            res_x = new_x;
            res_y = new_y;
        }
        (res_x, res_y)
    }

    /// Solves a linear system Ax = b using Gaussian Elimination with partial pivoting.
    #[allow(clippy::needless_range_loop)]
    fn solve_linear_system(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
        let n = b.len();

        for i in 0..n {
            let mut max_row = i;
            for k in (i + 1)..n {
                if a[k][i].abs() > a[max_row][i].abs() {
                    max_row = k;
                }
            }

            if a[max_row][i].abs() < 1e-10 {
                return None;
            }

            a.swap(i, max_row);
            b.swap(i, max_row);

            for k in (i + 1)..n {
                let c = -a[k][i] / a[i][i];
                for j in i..n {
                    if i == j {
                        a[k][j] = 0.0;
                    } else {
                        a[k][j] += c * a[i][j];
                    }
                }
                b[k] += c * b[i];
            }
        }

        let mut x = vec![0.0; n];
        for i in (0..n).rev() {
            x[i] = b[i];
            for j in (i + 1)..n {
                x[i] -= a[i][j] * x[j];
            }
            x[i] /= a[i][i];
        }
        Some(x)
    }

    /// Analytically computes the required circulations (gamma) for a set of control points
    /// to neutralize the incoming turbulence multipole moments.
    /// `incoming_eddies`: The large chaotic turbulence vortices.
    /// `control_points`: The (x, y) coordinates of our surface micro-textures.
    /// Returns a vector of circulation strengths corresponding to the `control_points`.
    pub fn neutralize(
        incoming_eddies: &[Vortex],
        control_points: &[(f64, f64)],
    ) -> Option<Vec<f64>> {
        let m = control_points.len();
        if m == 0 {
            return Some(vec![]);
        }

        let mut a = vec![vec![0.0; m]; m];
        let mut b = vec![0.0; m];

        // Fill the M equations
        for row in 0..m {
            // Determine the degree 'n' and whether it's a real or imaginary equation
            // Row 0: Re(n=0)
            // Row 1: Re(n=1), Row 2: Im(n=1)
            // Row 3: Re(n=2), Row 4: Im(n=2)
            // General rule for row > 0: n = (row + 1) / 2
            let n = if row == 0 { 0 } else { row.div_ceil(2) };
            let is_imag = row > 0 && row % 2 == 0;

            // Calculate target moment from incoming eddies
            // M_n = - \sum \Gamma_k Z_k^n
            let mut target_real = 0.0;
            let mut target_imag = 0.0;
            for eddy in incoming_eddies {
                let (zx, zy) = Self::complex_pow(eddy.x, eddy.y, n);
                target_real -= eddy.gamma * zx;
                target_imag -= eddy.gamma * zy;
            }

            b[row] = if is_imag { target_imag } else { target_real };

            // Fill Vandermonde row for control points
            for j in 0..m {
                let (px, py) = Self::complex_pow(control_points[j].0, control_points[j].1, n);
                a[row][j] = if is_imag { py } else { px };
            }
        }

        Self::solve_linear_system(a, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multipole_neutralization() {
        // 2 Incoming chaotic eddies
        let eddies = vec![
            Vortex {
                x: -5.0,
                y: 1.0,
                gamma: 5.0,
            },
            Vortex {
                x: -4.0,
                y: -2.0,
                gamma: -3.0,
            },
        ];

        // 4 Control points at non-symmetric positions to avoid singular matrix
        let controls = vec![(-1.0, 1.2), (-0.5, -0.8), (-1.8, 0.4), (-2.1, -1.5)];

        // Solve!
        let gammas = MultipoleSolver::neutralize(&eddies, &controls).expect("Matrix was singular");
        assert_eq!(gammas.len(), 4);

        // Verify mathematically
        // Monopole (n=0): sum(gamma) should equal -sum(Gamma) = -(5 - 3) = -2
        let total_gamma: f64 = gammas.iter().sum();
        assert!(
            (total_gamma - (-2.0)).abs() < 1e-8,
            "Monopole mismatch: {}",
            total_gamma
        );

        // Dipole Real (n=1, Re): sum(gamma_j * x_j) = -sum(Gamma_k * X_k)
        let mut dipole_real_control = 0.0;
        for j in 0..4 {
            dipole_real_control += gammas[j] * controls[j].0;
        }

        let mut dipole_real_target = 0.0;
        for k in &eddies {
            dipole_real_target -= k.gamma * k.x;
        }

        assert!(
            (dipole_real_control - dipole_real_target).abs() < 1e-8,
            "Dipole Real mismatch"
        );

        // Dipole Imag (n=1, Im): sum(gamma_j * y_j) = -sum(Gamma_k * Y_k)
        let mut dipole_imag_control = 0.0;
        for j in 0..4 {
            dipole_imag_control += gammas[j] * controls[j].1;
        }

        let mut dipole_imag_target = 0.0;
        for k in &eddies {
            dipole_imag_target -= k.gamma * k.y;
        }

        assert!(
            (dipole_imag_control - dipole_imag_target).abs() < 1e-8,
            "Dipole Imag mismatch"
        );
    }
}
