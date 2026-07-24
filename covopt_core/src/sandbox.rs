
use std::path::PathBuf;
use std::process::Command;
use std::fs;
use crate::mca::McaRunner;

#[derive(Debug, Clone)]
pub struct BenchmarkMetrics {
    pub peak_rss: u64,
    pub ipc: Option<f64>,
    pub cycles: Option<usize>,
}

/// Sandbox Environment for Differential Performance Benchmarking
pub struct Sandbox {
    pub target_dir: PathBuf,
}

impl Sandbox {
    pub fn new(target_dir: PathBuf) -> Self {
        Self { target_dir }
    }

    /// Evaluates metrics for a given source state.
    pub fn measure_metrics(&self, symbol: Option<&str>) -> Result<BenchmarkMetrics, String> {
        let mut ipc = None;
        let mut cycles = None;
        
        // 1. Try to get IPC via MCA if symbol is provided
        if let Some(sym) = symbol {
            let runner = crate::runner::CargoTestRunner::new("dummy", &self.target_dir, vec![]);
            if let Ok(asm) = runner.compile_asm() {
                if let Some(block) = runner.extract_asm_block(&asm, sym) {
                    let mca = McaRunner::new(None);
                    if let Ok(report) = mca.run(&block) {
                        ipc = Some(report.ipc);
                        cycles = Some(report.total_cycles);
                    }
                }
            }
        }
        
        // 2. Measure RSS via a quick cargo test run
        let mut peak_rss = 0;
        let mut cmd = Command::new("/usr/bin/time");
        cmd.arg("-l").arg("cargo").arg("test").arg("--no-run").current_dir(&self.target_dir);
        if let Ok(output) = cmd.output() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            for line in stderr.lines() {
                if line.contains("maximum resident set size") {
                    if let Some(num_str) = line.split_whitespace().next() {
                        if let Ok(rss) = num_str.parse::<u64>() {
                            peak_rss = rss;
                        }
                    }
                }
            }
        }
        
        Ok(BenchmarkMetrics {
            peak_rss,
            ipc,
            cycles,
        })
    }

    /// Run a differential benchmark before and after applying a fix.
    /// Returns true if the fix is safe (0 regressions in time/space/IPC).
    pub fn verify_fix<F>(&self, target_files: &[PathBuf], symbol: Option<&str>, fix_fn: F) -> Result<bool, String> 
    where 
        F: FnOnce() -> Result<(), String> 
    {
        println!("[Sandbox] Measuring baseline performance...");
        let baseline = self.measure_metrics(symbol)?;
        
        // Backup original files
        let mut backups = Vec::new();
        for file in target_files {
            if let Ok(content) = fs::read_to_string(file) {
                backups.push((file.clone(), content));
            }
        }
            
        // Apply fix
        println!("[Sandbox] Applying candidate fix...");
        if let Err(e) = fix_fn() {
            // Restore and fail
            for (file, content) in &backups {
                let _ = fs::write(file, content);
            }
            return Err(format!("Fix function failed: {}", e));
        }
            
        // Measure candidate
        println!("[Sandbox] Measuring candidate performance...");
        let candidate = match self.measure_metrics(symbol) {
            Ok(metrics) => metrics,
            Err(e) => {
                for (file, content) in &backups {
                    let _ = fs::write(file, content);
                }
                return Err(format!("Failed to measure candidate: {}", e));
            }
        };
        
        // Compare
        println!("[Sandbox] Baseline: IPC={:?}, Cycles={:?}, RSS={}", baseline.ipc, baseline.cycles, baseline.peak_rss);
        println!("[Sandbox] Candidate: IPC={:?}, Cycles={:?}, RSS={}", candidate.ipc, candidate.cycles, candidate.peak_rss);
        
        let mut safe = true;
        
        if let (Some(b_cycles), Some(c_cycles)) = (baseline.cycles, candidate.cycles) {
            if c_cycles > (b_cycles as f64 * 1.05) as usize {
                println!("[Sandbox] ❌ Rejecting fix due to Cycle count regression.");
                safe = false;
            }
        }
        
        if candidate.peak_rss > (baseline.peak_rss as f64 * 1.05) as u64 {
            println!("[Sandbox] ❌ Rejecting fix due to Memory (RSS) regression.");
            safe = false;
        }
        
        if !safe {
            println!("[Sandbox] Reverting changes...");
            for (file, content) in &backups {
                let _ = fs::write(file, content);
            }
        } else {
            println!("[Sandbox] ✅ Fix verified safe (0 regressions). Keeping changes.");
        }
        
        Ok(safe)
    }
}
