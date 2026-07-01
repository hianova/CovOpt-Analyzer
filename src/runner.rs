use std::process::Command;
use std::path::{Path, PathBuf};
use std::fs;
use crate::coverage::CoverageMap;

pub struct CoverageRunner {
    pub target_name: String,
    pub source_file: PathBuf,
    pub output_dir: PathBuf,
    pub rustc_cmd: String,
    pub profdata_cmd: String,
    pub cov_cmd: String,
}

impl CoverageRunner {
    pub fn new<P: AsRef<Path>>(source_file: P, target_name: &str, output_dir: P) -> Self {
        Self {
            target_name: target_name.to_string(),
            source_file: source_file.as_ref().to_path_buf(),
            output_dir: output_dir.as_ref().to_path_buf(),
            rustc_cmd: "rustc".to_string(),
            profdata_cmd: "llvm-profdata".to_string(),
            cov_cmd: "llvm-cov".to_string(),
        }
    }

    /// Run the full pipeline and return the parsed CoverageMap.
    pub fn run(&self) -> Result<CoverageMap, String> {
        if !self.output_dir.exists() {
            fs::create_dir_all(&self.output_dir)
                .map_err(|e| format!("Failed to create output directory: {}", e))?;
        }

        self.compile()?;
        self.execute()?;
        self.merge_profdata()?;
        let json_str = self.export_json()?;
        
        CoverageMap::from_json(&json_str).map_err(|e| format!("Failed to parse JSON: {}", e))
    }

    fn compile(&self) -> Result<(), String> {
        let out_bin = self.output_dir.join(&self.target_name);
        let status = Command::new(&self.rustc_cmd)
            .arg("-C")
            .arg("instrument-coverage")
            .arg(&self.source_file)
            .arg("-o")
            .arg(&out_bin)
            .status()
            .map_err(|e| format!("Failed to run rustc: {}", e))?;

        if !status.success() {
            return Err("Compilation failed".to_string());
        }
        Ok(())
    }

    fn execute(&self) -> Result<(), String> {
        // Resolve absolute path for the binary to execute it safely
        let out_bin = fs::canonicalize(self.output_dir.join(&self.target_name))
            .map_err(|e| format!("Failed to canonicalize binary path: {}", e))?;
            
        let profraw = self.output_dir.join(format!("{}.profraw", self.target_name));
        
        let status = Command::new(&out_bin)
            .env("LLVM_PROFILE_FILE", profraw)
            .status()
            .map_err(|e| format!("Failed to execute binary: {}", e))?;

        if !status.success() {
            return Err("Execution failed".to_string());
        }
        Ok(())
    }

    fn merge_profdata(&self) -> Result<(), String> {
        let profraw = self.output_dir.join(format!("{}.profraw", self.target_name));
        let profdata = self.output_dir.join(format!("{}.profdata", self.target_name));

        let status = Command::new(&self.profdata_cmd)
            .arg("merge")
            .arg("-sparse")
            .arg(&profraw)
            .arg("-o")
            .arg(&profdata)
            .status()
            .map_err(|e| format!("Failed to run llvm-profdata: {}", e))?;

        if !status.success() {
            return Err("Profdata merge failed".to_string());
        }
        Ok(())
    }

    fn export_json(&self) -> Result<String, String> {
        let out_bin = self.output_dir.join(&self.target_name);
        let profdata = self.output_dir.join(format!("{}.profdata", self.target_name));

        let output = Command::new(&self.cov_cmd)
            .arg("export")
            .arg(&out_bin)
            .arg(format!("-instr-profile={}", profdata.display()))
            .output()
            .map_err(|e| format!("Failed to run llvm-cov export: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("llvm-cov export failed: {}", stderr));
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

pub struct CargoTestRunner {
    pub test_name: String,
    pub output_dir: PathBuf,
}

impl CargoTestRunner {
    pub fn new(test_name: &str, output_dir: &Path) -> Self {
        Self {
            test_name: test_name.to_string(),
            output_dir: output_dir.to_path_buf(),
        }
    }

    pub fn run(&self, n: usize) -> Result<CoverageMap, String> {
        if !self.output_dir.exists() {
            fs::create_dir_all(&self.output_dir)
                .map_err(|e| format!("Failed to create output directory: {}", e))?;
        }

        let executables = self.compile_tests()?;
        if executables.is_empty() {
            return Err("No test executables found".to_string());
        }

        self.execute_tests(&executables, n)?;
        self.merge_profdata(n)?;
        let json_str = self.export_json(&executables, n)?;
        CoverageMap::from_json(&json_str).map_err(|e| format!("Failed to parse JSON: {}", e))
    }

    fn compile_tests(&self) -> Result<Vec<PathBuf>, String> {
        let output = Command::new("cargo")
            .env("RUSTFLAGS", "-C instrument-coverage")
            .arg("test")
            .arg("--no-run")
            .arg("--message-format=json")
            .output()
            .map_err(|e| format!("Failed to run cargo test: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Compilation failed: {}", stderr));
        }

        let mut executables = Vec::new();
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line)
                && v.get("reason").and_then(|r| r.as_str()) == Some("compiler-artifact")
                    && let Some(exe) = v.get("executable").and_then(|e| e.as_str()) {
                        executables.push(PathBuf::from(exe));
                    }
        }
        
        Ok(executables)
    }

    fn execute_tests(&self, executables: &[PathBuf], n: usize) -> Result<(), String> {
        // Clean up any existing profraw files for this N to prevent accumulating hit counts
        if let Ok(entries) = fs::read_dir(&self.output_dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str()
                    && name.starts_with(&format!("covopt_{}_", n)) && name.ends_with(".profraw") {
                        fs::remove_file(entry.path()).ok();
                    }
            }
        }

        for exe in executables {
            let profraw = self.output_dir.join(format!("covopt_{}_%p.profraw", n));
            // Just run it. We ignore the exit status because it might fail if the test
            // is not in this specific executable (though usually it returns 0 for 0 tests run).
            // We just want it to generate profraw if it runs the test.
            let _ = Command::new(exe)
                .arg(&self.test_name)
                .env("LLVM_PROFILE_FILE", &profraw)
                .env("COVOPT_N", n.to_string())
                .output();
        }
        Ok(())
    }

    fn merge_profdata(&self, n: usize) -> Result<(), String> {
        let profdata = self.output_dir.join(format!("covopt_{}.profdata", n));

        // Use glob pattern via shell if needed, but llvm-profdata accepts sparse inputs.
        // Actually llvm-profdata doesn't expand wildcards itself unless we pass it correctly.
        // Let's find the matching files manually.
        let mut profraws = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.output_dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str()
                    && name.starts_with(&format!("covopt_{}_", n)) && name.ends_with(".profraw") {
                        profraws.push(entry.path());
                    }
            }
        }

        if profraws.is_empty() {
            return Err(format!("No profraw files generated for N={}", n));
        }

        let mut cmd = Command::new("llvm-profdata");
        cmd.arg("merge").arg("-sparse");
        for p in profraws {
            cmd.arg(p);
        }
        cmd.arg("-o").arg(&profdata);

        let status = cmd.status().map_err(|e| format!("Failed to run llvm-profdata: {}", e))?;
        if !status.success() {
            return Err("Profdata merge failed".to_string());
        }
        Ok(())
    }

    fn export_json(&self, executables: &[PathBuf], n: usize) -> Result<String, String> {
        let profdata = self.output_dir.join(format!("covopt_{}.profdata", n));
        
        let mut cmd = Command::new("llvm-cov");
        cmd.arg("export");
        cmd.arg("-instr-profile").arg(&profdata);
        
        // Add all executables
        for exe in executables.iter() {
            cmd.arg("-object").arg(exe);
        }

        let output = cmd.output().map_err(|e| format!("Failed to run llvm-cov export: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("llvm-cov export failed: {}", stderr));
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_coverage_runner_pipeline() {
        let dir = tempdir().unwrap();
        let test_dir = fs::canonicalize(dir.path()).unwrap();
        let source_file = test_dir.join("test_target.rs");

        // Write a simple rust file with a loop
        let source_code = r#"
fn loop_test(n: usize) {
    let mut sum = 0;
    for i in 0..n {
        sum += i;
    }
    println!("{}", sum);
}

fn main() {
    loop_test(5);
}
"#;
        let mut file = fs::File::create(&source_file).unwrap();
        file.write_all(source_code.as_bytes()).unwrap();

        // Run pipeline
        let runner = CoverageRunner::new(&source_file, "test_target", &test_dir.to_path_buf());
        let map_result = runner.run();
        
        assert!(map_result.is_ok(), "Pipeline failed: {:?}", map_result.err());
        
        let map = map_result.unwrap();
        let canonical_source = fs::canonicalize(&source_file).unwrap();
        let source_str = canonical_source.to_string_lossy().into_owned();

        // The loop is on line 4, and it runs 5 times based on `loop_test(5)`
        let hit_count = map.get_hit_count(&source_str, 4);
        assert_eq!(hit_count, Some(5), "Loop body should have hit count 5");
    }

    #[test]
    fn test_runner_compile_error() {
        let dir = tempdir().unwrap();
        let runner = CoverageRunner::new(Path::new("does_not_exist.rs"), "test_target", dir.path());
        let res = runner.compile();
        assert!(res.is_err());
    }

    #[test]
    fn test_runner_execute_error() {
        let dir = tempdir().unwrap();
        let test_dir = dir.path();
        let source_file = test_dir.join("panic.rs");
        let mut file = fs::File::create(&source_file).unwrap();
        file.write_all(b"fn main() { panic!(\"fail\"); }").unwrap();

        let runner = CoverageRunner::new(&source_file, "panic_bin", &test_dir.to_path_buf());
        runner.compile().unwrap();
        let res = runner.execute();
        assert!(res.is_err());
    }

    #[test]
    fn test_runner_merge_error() {
        let dir = tempdir().unwrap();
        let runner = CoverageRunner::new(Path::new("dummy"), "dummy", dir.path());
        let res = runner.merge_profdata();
        assert!(res.is_err());
    }

    #[test]
    fn test_runner_export_error() {
        let dir = tempdir().unwrap();
        let runner = CoverageRunner::new(Path::new("dummy"), "dummy", dir.path());
        let res = runner.export_json();
        assert!(res.is_err());
    }

    #[test]
    fn test_runner_mkdir_error() {
        let runner = CoverageRunner::new(Path::new("dummy"), "dummy", Path::new("/dev/null/dummy"));
        let res = runner.run();
        assert!(res.is_err());
    }

    #[test]
    fn test_runner_execute_canonicalize_err() {
        let dir = tempdir().unwrap();
        let runner = CoverageRunner::new(Path::new("dummy"), "dummy", dir.path());
        let res = runner.execute();
        assert!(res.is_err());
    }

    #[test]
    fn test_runner_cmd_not_found() {
        let dir = tempdir().unwrap();
        let mut runner = CoverageRunner::new(Path::new("dummy"), "dummy", dir.path());
        
        runner.rustc_cmd = "does_not_exist_rustc".to_string();
        assert!(runner.compile().is_err());

        runner.profdata_cmd = "does_not_exist_profdata".to_string();
        assert!(runner.merge_profdata().is_err());

        runner.cov_cmd = "does_not_exist_cov".to_string();
        assert!(runner.export_json().is_err());
    }

    #[test]
    fn test_runner_invalid_json_output() {
        let dir = tempdir().unwrap();
        let test_dir = dir.path();
        
        let fake_cov_path = test_dir.join("fake_cov.sh");
        let mut file = fs::File::create(&fake_cov_path).unwrap();
        file.write_all(b"#!/bin/sh\necho 'invalid json'\n").unwrap();
        
        let out_bin_path = test_dir.join("dummy");
        let mut file = fs::File::create(&out_bin_path).unwrap();
        file.write_all(b"#!/bin/sh\nexit 0\n").unwrap();
        
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&fake_cov_path, fs::Permissions::from_mode(0o755)).unwrap();
            fs::set_permissions(&out_bin_path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let mut runner = CoverageRunner::new(test_dir.join("dummy"), "dummy", test_dir.to_path_buf());
        fs::File::create(test_dir.join("dummy.profdata")).unwrap();
        
        runner.cov_cmd = fake_cov_path.to_str().unwrap().to_string();
        runner.rustc_cmd = "true".to_string();
        runner.profdata_cmd = "true".to_string();
        
        let res = runner.run();
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Failed to parse JSON"));
    }
}
