use crate::science::chaos_state::ChaosState;
use crate::science::ScienceObjective;
use std::fs::File;
use std::io::{BufRead, BufReader};

pub struct SparseMatrix {
    pub rows: Vec<usize>,
    pub cols: Vec<usize>,
    pub vals: Vec<f64>,
    pub dim: usize,
}

impl SparseMatrix {
    pub fn load_from_csv(path: &str, dim: usize) -> Result<Self, Box<dyn std::error::Error>> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        
        let mut rows = Vec::new();
        let mut cols = Vec::new();
        let mut vals = Vec::new();

        for (i, line) in reader.lines().enumerate() {
            if i == 0 { continue; } // Skip header
            let line = line?;
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() >= 3 {
                let r: usize = parts[0].parse()?;
                let c: usize = parts[1].parse()?;
                let v: f64 = parts[2].parse()?;
                rows.push(r);
                cols.push(c);
                vals.push(v);
            }
        }

        Ok(Self { rows, cols, vals, dim })
    }
}

pub struct QuantumObjective {
    pub hamiltonian: SparseMatrix,
}

impl QuantumObjective {
    pub fn new(hamiltonian: SparseMatrix) -> Self {
        Self { hamiltonian }
    }
}

// Implement ScienceObjective for a ChaosState with N=1 branch, and D parameters.
// Here D represents the Hilbert space dimension (e.g., 1024 for N=10 qubits).
impl<const D: usize> ScienceObjective<ChaosState<1, D>> for QuantumObjective {
    fn evaluate_fitness(&self, candidate: &ChaosState<1, D>) -> (u32, u32) {
        // Normalize the state |psi> so that sum_i |psi_i|^2 = 1
        let mut norm_sq = 0.0;
        for val in candidate.base_values.iter() {
            norm_sq += (*val as f64) * (*val as f64);
        }
        
        let mut psi = vec![0.0; D];
        if norm_sq > 0.0 {
            let inv_norm = 1.0 / norm_sq.sqrt();
            for i in 0..D {
                psi[i] = (candidate.base_values[i] as f64) * inv_norm;
            }
        } else {
            psi[0] = 1.0;
        }

        // Calculate Expectation Value: E = <psi | H | psi>
        let mut energy = 0.0;
        for i in 0..self.hamiltonian.rows.len() {
            let r = self.hamiltonian.rows[i];
            let c = self.hamiltonian.cols[i];
            let v = self.hamiltonian.vals[i];
            
            if r < D && c < D {
                energy += psi[r] * v * psi[c];
            }
        }

        // Convert the energy (f64) to a u32 fitness score. 
        // We want to minimize energy. Lower energy should mean lower fitness score.
        // Assuming energy is around -10.0 to 10.0, we map it.
        // Multiply by 100,000 for precision, add a large offset to make it positive.
        let shifted = energy * 100_000.0 + 1_000_000.0;
        let fitness = if shifted < 0.0 { 0 } else { shifted as u32 };
        
        (fitness, 0)
    }

    fn generate_seed(&self, seed: usize, _parent: Option<&ChaosState<1, D>>) -> ChaosState<1, D> {
        // Random initial state
        let mut base = [0.0; D];
        let mut rng = crate::science::chaos_state::RngState::new(seed as u32);
        for b in base.iter_mut() {
            *b = rng.next_f32() * 2.0 - 1.0;
        }
        ChaosState::new(base)
    }

    fn perturb(&self, candidate: &ChaosState<1, D>, scale: f32, seed: usize) -> ChaosState<1, D> {
        let mut next = *candidate;
        let mut rng = crate::science::chaos_state::RngState::new(seed as u32);
        let tweak = crate::science::chaos_state::MicroTweak { s_exponent: 1.5, max_elements: D as u32 };
        next = crate::science::chaos_state::step_forward_nd(&next, &tweak, &mut rng);
        
        // Add random jitter based on scale
        for val in next.base_values.iter_mut() {
            let jitter = (rng.next_f32() * 2.0 - 1.0) * scale;
            *val += jitter;
        }
        next
    }

    fn is_valid(&self, _candidate: &ChaosState<1, D>) -> bool {
        true
    }

    fn check_archival(&self, _candidate: &ChaosState<1, D>, _fitness: (u32, u32)) -> bool {
        false
    }
}
