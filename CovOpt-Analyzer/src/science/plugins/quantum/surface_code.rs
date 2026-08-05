use ndarray::Array2;

/// Represets a 2D Surface Code, consisting of data qubits and syndrome (measure) qubits.
/// In this classical simulation of topological quantum error correction, we simulate bit-flip errors
/// and local parity checks to construct the 0/1 verification tree.
pub struct SurfaceCode {
    /// The grid of data qubits.
    pub data_qubits: Array2<bool>,
    /// The syndrome measurements (parity checks). True means an error was detected locally.
    pub syndrome_qubits: Array2<bool>,
}

impl SurfaceCode {
    /// Initialize an empty surface code grid of size `rows` x `cols`.
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            data_qubits: Array2::from_elem((rows, cols), false),
            syndrome_qubits: Array2::from_elem((rows - 1, cols - 1), false),
        }
    }

    /// Inject a bit-flip error into a specific data qubit.
    pub fn inject_error(&mut self, row: usize, col: usize) {
        if row < self.data_qubits.nrows() && col < self.data_qubits.ncols() {
            let current = self.data_qubits[[row, col]];
            self.data_qubits[[row, col]] = !current;
        }
    }

    /// Perform the local parity checks (0/1 verification).
    /// Each syndrome qubit checks the parity of its 4 neighboring data qubits.
    /// This acts as the local 0/1 verification tree, avoiding a global superposition collapse.
    pub fn measure_syndromes(&mut self) {
        let rows = self.data_qubits.nrows();
        let cols = self.data_qubits.ncols();

        for r in 0..(rows - 1) {
            for c in 0..(cols - 1) {
                // Check parity of the 4 data qubits around this syndrome qubit
                let q1 = self.data_qubits[[r, c]];
                let q2 = self.data_qubits[[r, c + 1]];
                let q3 = self.data_qubits[[r + 1, c]];
                let q4 = self.data_qubits[[r + 1, c + 1]];

                // Parity is true (1) if there's an odd number of true values (errors)
                let parity = !(q1 as u8 + q2 as u8 + q3 as u8 + q4 as u8).is_multiple_of(2);
                self.syndrome_qubits[[r, c]] = parity;
            }
        }
    }

    /// Iterates over syndromes and attempts a naive local correction.
    /// In a full quantum error correction scheme, this uses Minimum Weight Perfect Matching (MWPM).
    /// Here, we use a greedy approach to find the most likely single-qubit error for the simulation.
    pub fn correct_errors(&mut self) {
        // A simple greedy correction: if a data qubit participates in multiple triggered syndromes,
        // it's highly likely to be the source of the error.
        let rows = self.data_qubits.nrows();
        let cols = self.data_qubits.ncols();

        let mut error_votes = Array2::<u8>::zeros((rows, cols));

        // Tally votes from triggered syndromes
        for r in 0..(rows - 1) {
            for c in 0..(cols - 1) {
                if self.syndrome_qubits[[r, c]] {
                    error_votes[[r, c]] += 1;
                    error_votes[[r, c + 1]] += 1;
                    error_votes[[r + 1, c]] += 1;
                    error_votes[[r + 1, c + 1]] += 1;
                }
            }
        }

        // Apply correction where votes are highest (greedy MWPM approximation)
        // A single error in the middle of the grid triggers 4 syndromes.
        for r in 0..rows {
            for c in 0..cols {
                if error_votes[[r, c]] > 1 {
                    // Correct the error
                    let current = self.data_qubits[[r, c]];
                    self.data_qubits[[r, c]] = !current;
                }
            }
        }

        // Re-measure to confirm fix
        self.measure_syndromes();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_surface_code_correction() {
        let mut surface = SurfaceCode::new(5, 5);

        // Inject an error in the middle
        surface.inject_error(2, 2);
        assert!(surface.data_qubits[[2, 2]]);

        // Measure syndromes - the surrounding 4 parity checks should trigger
        surface.measure_syndromes();
        assert!(surface.syndrome_qubits[[1, 1]]);
        assert!(surface.syndrome_qubits[[1, 2]]);
        assert!(surface.syndrome_qubits[[2, 1]]);
        assert!(surface.syndrome_qubits[[2, 2]]);

        // Correct the error
        surface.correct_errors();

        // The error should be gone
        assert!(!surface.data_qubits[[2, 2]]);
        assert!(!surface.syndrome_qubits[[1, 1]]);
    }
}
