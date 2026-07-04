use std::io::Write;
use std::process::{Command, Stdio};

#[derive(Debug, Default)]
pub struct McaReport {
    pub instructions: usize,
    pub total_cycles: usize,
    pub block_rthroughput: f64,
    pub ipc: f64,
}

pub struct McaRunner {
    pub cpu: Option<String>,
}

impl McaRunner {
    pub fn new(cpu: Option<String>) -> Self {
        Self { cpu }
    }

    pub fn run(&self, asm_block: &str) -> Result<McaReport, String> {
        let mut cmd = Command::new("llvm-mca");

        if let Some(cpu) = &self.cpu {
            cmd.arg(format!("-mcpu={}", cpu));
        }

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn llvm-mca: {}", e))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(asm_block.as_bytes())
                .map_err(|e| format!("Failed to write to stdin: {}", e))?;
        }

        let output = child
            .wait_with_output()
            .map_err(|e| format!("Failed to wait for llvm-mca: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("llvm-mca failed: {}", stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        self.parse_report(&stdout)
    }

    fn parse_report(&self, output: &str) -> Result<McaReport, String> {
        let mut report = McaReport::default();

        for line in output.lines() {
            if line.starts_with("Instructions:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() == 2 {
                    report.instructions = parts[1].parse().unwrap_or(0);
                }
            } else if line.starts_with("Total Cycles:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() == 3 {
                    report.total_cycles = parts[2].parse().unwrap_or(0);
                }
            } else if line.starts_with("IPC:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() == 2 {
                    report.ipc = parts[1].parse().unwrap_or(0.0);
                }
            } else if line.starts_with("Block RThroughput:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() == 3 {
                    report.block_rthroughput = parts[2].parse().unwrap_or(0.0);
                }
            }
        }

        Ok(report)
    }
}
