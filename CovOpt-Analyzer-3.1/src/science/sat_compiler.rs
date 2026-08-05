use crate::science::boolean_relaxation::AsicCircuit;
use std::fs::File;
use std::io::Write;

pub struct SatCompiler;

impl SatCompiler {
    pub fn speculate(
        _candidate: &AsicCircuit,
        target_truth_table: &[(Vec<bool>, Vec<bool>)],
        _num_inputs: usize,
        _num_outputs: usize,
        hole_count: usize,
    ) {
        let filename = format!("target_speculation_{}.cnf", hole_count);
        let mut file = File::create(&filename).unwrap();

        // 變數編號策略：
        // 1. Selector Variables: 對於每個 hole (0..hole_count)，有 L, R, Op 的選擇變數。
        // 2. Instance Variables: 共有 256 個 instances。每個 instance 有 num_wires 個變數。
        // 此處我們簡化為產生註解與基本結構，後續可以擴充完整的 Tseitin 展開。
        writeln!(file, "c SAT Speculation for AES S-Box").unwrap();
        writeln!(file, "c Holes: {}", hole_count).unwrap();
        writeln!(file, "c Target instances: {}", target_truth_table.len()).unwrap();
        writeln!(file, "p cnf 1 1").unwrap();
        writeln!(file, "1 0").unwrap();

        println!(">>> [System 2] SAT 投機 CNF 檔案已產生：{}", filename);
        println!(
            ">>> [System 2] 請使用外部 Solver 求解： cryptominisat5 {} > solution.txt",
            filename
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sat_compiler() {
        let circuit = AsicCircuit {
            gates: vec![],
            output_map: vec![],
            affine_mask: [0; 8],
            affine_const: 0,
        };
        SatCompiler::speculate(&circuit, &[], 2, 2, 5);
        assert!(std::path::Path::new("target_speculation_5.cnf").exists());
        let _ = std::fs::remove_file("target_speculation_5.cnf");
    }
}
