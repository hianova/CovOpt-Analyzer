use crate::coverage::CoverageMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
        self.merge_profdata()?;
        let lcov_str = self.export_lcov()?;

        CoverageMap::from_lcov(&lcov_str).map_err(|e| format!("Failed to parse LCOV: {}", e))
    }

    fn compile(&self) -> Result<(), String> {
        let out_bin = self.output_dir.join(&self.target_name);
        let status = Command::new(&self.rustc_cmd)
            .env(
                "LLVM_PROFILE_FILE",
                self.output_dir.join("default_%m_%p.profraw"),
            )
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

        let profraw = self
            .output_dir
            .join(format!("{}.profraw", self.target_name));

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
        let profraw = self
            .output_dir
            .join(format!("{}.profraw", self.target_name));
        let profdata = self
            .output_dir
            .join(format!("{}.profdata", self.target_name));

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

    fn export_lcov(&self) -> Result<String, String> {
        let profdata = self
            .output_dir
            .join(format!("{}.profdata", self.target_name));
        let out_bin = self.output_dir.join(&self.target_name);

        let output = Command::new(&self.cov_cmd)
            .arg("export")
            .arg("-format=lcov")
            .arg("-instr-profile")
            .arg(&profdata)
            .arg("-object")
            .arg(&out_bin)
            .output()
            .map_err(|e| format!("Failed to run llvm-cov export: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("LLVM Cov Export failed: {}", stderr));
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

    pub fn run(&self, n: usize, seed: Option<u64>) -> Result<(CoverageMap, u64), String> {
        if !self.output_dir.exists() {
            fs::create_dir_all(&self.output_dir)
                .map_err(|e| format!("Failed to create output directory: {}", e))?;
        }

        let executables = self.compile_tests()?;
        if executables.is_empty() {
            return Err("No test executables found".to_string());
        }

        let peak_rss = self.execute_tests(&executables, n, seed)?;
        self.merge_profdata(n)?;
        let lcov_str = self.export_lcov(&executables, n)?;

        let map = CoverageMap::from_lcov(&lcov_str)?;
        std::fs::write("/tmp/covopt_debug.json", &lcov_str).unwrap();
        Ok((map, peak_rss))
    }

    fn compile_tests(&self) -> Result<Vec<PathBuf>, String> {
        let output = Command::new("cargo")
            .env("RUSTFLAGS", "-C instrument-coverage")
            .env(
                "LLVM_PROFILE_FILE",
                self.output_dir.join("default_%m_%p.profraw"),
            )
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

        // Clean up garbage `default_*.profraw` generated by build scripts
        // Cargo strips LLVM_PROFILE_FILE when running build.rs, so they fallback to CWD.
        for dir in &[".", ".."] {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    if let Some(name) = entry.file_name().to_str()
                        && name.starts_with("default_")
                        && name.ends_with(".profraw")
                    {
                        let _ = fs::remove_file(entry.path());
                    }
                }
            }
        }

        for line in stdout.lines() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line)
                && v.get("reason").and_then(|r| r.as_str()) == Some("compiler-artifact")
                && v.get("profile")
                    .and_then(|p| p.get("test"))
                    .and_then(|t| t.as_bool())
                    == Some(true)
                && let Some(exe) = v.get("executable").and_then(|e| e.as_str())
            {
                executables.push(PathBuf::from(exe));
            }
        }

        Ok(executables)
    }

    fn execute_tests(
        &self,
        executables: &[PathBuf],
        n: usize,
        seed: Option<u64>,
    ) -> Result<u64, String> {
        // Clean up any existing profraw files for this N to prevent accumulating hit counts
        if let Ok(entries) = fs::read_dir(&self.output_dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str()
                    && name.starts_with(&format!("covopt_{}_", n))
                    && name.ends_with(".profraw")
                {
                    fs::remove_file(entry.path()).ok();
                }
            }
        }

        let mut max_rss = 0u64;

        for exe in executables {
            let profraw = self.output_dir.join(format!("covopt_{}_%p.profraw", n));

            // On macOS, `/usr/bin/time -l` outputs peak RSS. On Linux, `/usr/bin/time -v` works if installed.
            // For cross-platform simplicity in this specialized tool, we'll try `/usr/bin/time -l`.
            let mut cmd = Command::new("/usr/bin/time");
            cmd.arg("-l").arg(exe);
            cmd.arg(&self.test_name)
                .arg("--exact")
                .env("LLVM_PROFILE_FILE", &profraw)
                .env("COVOPT_N", n.to_string());

            if let Some(s) = seed {
                cmd.env("COVOPT_FUZZ_SEED", s.to_string());
            }

            let output = cmd
                .output()
                .map_err(|e| format!("Failed to run test {}: {}", exe.display(), e))?;

            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);

            // Parse peak RSS
            for line in stderr.lines() {
                if line.contains("maximum resident set size")
                    && let Some(num_str) = line.split_whitespace().next()
                    && let Ok(rss) = num_str.parse::<u64>()
                    && rss > max_rss
                {
                    max_rss = rss;
                }
            }

            if !output.status.success() {
                if !stdout.contains("0 passed") {
                    eprintln!("Test {} failed: {}", exe.display(), stderr);
                }
            } else {
                if stdout.contains("1 passed") && std::env::var("COVOPT_COMPACT").is_err() {
                    println!("Test ran successfully.");
                }
            }
        }

        Ok(max_rss)
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
                    && name.starts_with(&format!("covopt_{}_", n))
                    && name.ends_with(".profraw")
                {
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

        let status = cmd
            .status()
            .map_err(|e| format!("Failed to run llvm-profdata: {}", e))?;
        if !status.success() {
            return Err("Profdata merge failed".to_string());
        }
        Ok(())
    }

    fn export_lcov(&self, executables: &[PathBuf], n: usize) -> Result<String, String> {
        let profdata = self.output_dir.join(format!("covopt_{}.profdata", n));

        let mut cmd = Command::new("llvm-cov");
        cmd.arg("export");
        cmd.arg("-format=lcov");
        cmd.arg("-instr-profile").arg(&profdata);

        // Add all executables
        for exe in executables.iter() {
            cmd.arg("-object").arg(exe);
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run llvm-cov export: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("LLVM Cov Export failed: {}", stderr));
        }

        let lcov = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(lcov)
    }

    pub fn compile_asm(&self) -> Result<String, String> {
        // Compile the tests in release mode with ASM generation and debug symbols for .loc mapping
        let output = Command::new("cargo")
            .env("RUSTFLAGS", "-g --emit=asm")
            .arg("test")
            .arg("--release")
            .arg("--no-run")
            // .arg(&self.test_name) // compiling all tests is safer to find the unit test
            .output()
            .map_err(|e| format!("Failed to run cargo test for ASM: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("ASM Compilation failed: {}", stderr));
        }

        let mut all_asm = String::new();
        let possible_targets = vec!["target/release/deps", "../target/release/deps", "../../target/release/deps"];
        for target_dir in possible_targets {
            if let Ok(entries) = fs::read_dir(target_dir) {
                for entry in entries.flatten() {
                    if let Some(ext) = entry.path().extension() {
                        if ext == "s" {
                            if let Ok(content) = fs::read_to_string(entry.path()) {
                                all_asm.push_str(&content);
                                all_asm.push('\n');
                            }
                        }
                    }
                }
            }
        }

        if all_asm.is_empty() {
            Err("Could not find any generated .s file".to_string())
        } else {
            Ok(all_asm)
        }
    }

    pub fn extract_asm_block(&self, asm_content: &str, symbol: &str) -> Option<String> {
        let symbol_label = format!("{}:", symbol);
        let lines = asm_content.lines();
        let mut in_block = false;
        let mut block = String::new();

        for line in lines {
            if in_block {
                if (line.ends_with(':')
                    && !line.trim_start().starts_with('.')
                    && !line.trim_start().starts_with('L'))
                    || line.starts_with(&format!("\t.size\t{}", symbol))
                {
                    break;
                }
                let tline = line.trim();
                if tline.starts_with(".loc") || tline.starts_with(".file") {
                    continue;
                }
                block.push_str(line);
                block.push('\n');
            } else if line == symbol_label {
                in_block = true;
                block.push_str(line);
                block.push('\n');
            }
        }

        if in_block { Some(block) } else { None }
    }

    pub fn extract_asm_block_by_loc(
        &self,
        asm_content: &str,
        target_file: &str,
        target_line: u64,
    ) -> Option<String> {
        let mut file_id = None;
        let mut in_target_loc = false;
        let mut block = String::new();

        // Pass 1: Find file ID mapping
        for line in asm_content.lines() {
            let tline = line.trim();
            if tline.starts_with(".file") {
                let parts: Vec<&str> = tline.split_whitespace().collect();
                if parts.len() >= 3 {
                    let id = parts[1];
                    let path = parts[2].trim_matches('"');
                    if path.contains(target_file) {
                        file_id = Some(id.to_string());
                        break;
                    }
                }
            }
        }

        let file_id = file_id?;

        // Pass 2: Extract block
        for line in asm_content.lines() {
            let tline = line.trim();
            if tline.starts_with(".loc ") {
                let parts: Vec<&str> = tline.split_whitespace().collect();
                if parts.len() >= 3 {
                    let id = parts[1];
                    let l_num = parts[2];
                    if id == file_id && l_num == target_line.to_string() {
                        in_target_loc = true;
                        continue; // Skip the .loc line itself
                    } else {
                        if in_target_loc {
                            break; // End of our target loc
                        }
                    }
                }
            } else if (tline.starts_with(".Lfunc_end") || tline.starts_with(".cfi_endproc"))
                && in_target_loc
            {
                break;
            }

            if in_target_loc && !tline.starts_with(".loc") && !tline.starts_with(".file") {
                block.push_str(line);
                block.push('\n');
            }
        }

        if block.trim().is_empty() {
            None
        } else {
            Some(block)
        }
    }

    pub fn extract_asm_block_by_keywords(
        &self,
        asm_content: &str,
        keywords: &[&str],
    ) -> Option<String> {
        let lines: Vec<&str> = asm_content.lines().collect();
        let mut target_symbol = String::new();

        for (i, &line) in lines.iter().enumerate() {
            if line.ends_with(':')
                && !line.trim_start().starts_with('.')
                && !line.trim_start().starts_with('L')
            {
                let mut all_match = true;
                for &kw in keywords {
                    if !line.contains(kw) {
                        all_match = false;
                        break;
                    }
                }

                if all_match {
                    // Verify it's a function by looking ahead 5 lines for .cfi_startproc or .loc
                    let mut is_function = false;
                    for j in 1..=5 {
                        if let Some(&next_line) = lines.get(i + j)
                            && (next_line.contains(".cfi_startproc")
                                || next_line.contains(".loc")
                                || next_line.contains("Lfunc_begin"))
                        {
                            is_function = true;
                            break;
                        }
                    }

                    if is_function {
                        target_symbol = line[..line.len() - 1].to_string();
                        break;
                    }
                }
            }
        }

        if target_symbol.is_empty() {
            return None;
        }

        self.extract_asm_block(asm_content, &target_symbol)
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

        assert!(
            map_result.is_ok(),
            "Pipeline failed: {:?}",
            map_result.err()
        );

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
        let res = runner.export_lcov();
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
        assert!(runner.export_lcov().is_err());
    }

    #[test]
    fn test_runner_invalid_lcov_output() {
        let dir = tempdir().unwrap();
        let test_dir = dir.path();

        let fake_cov_path = test_dir.join("fake_cov.sh");
        let mut file = fs::File::create(&fake_cov_path).unwrap();
        file.write_all(b"#!/bin/sh\necho 'invalid lcov'\n").unwrap();

        let out_bin_path = test_dir.join("dummy");
        let mut file = fs::File::create(&out_bin_path).unwrap();
        file.write_all(b"#!/bin/sh\nexit 0\n").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&fake_cov_path, fs::Permissions::from_mode(0o755)).unwrap();
            fs::set_permissions(&out_bin_path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let mut runner =
            CoverageRunner::new(test_dir.join("dummy"), "dummy", test_dir.to_path_buf());
        fs::File::create(test_dir.join("dummy.profdata")).unwrap();

        runner.cov_cmd = fake_cov_path.to_str().unwrap().to_string();
        runner.rustc_cmd = "true".to_string();
        runner.profdata_cmd = "true".to_string();

        let res = runner.run();
        assert!(res.is_ok());
        let map = res.unwrap();
        assert_eq!(map.get_hit_count("dummy", 1), None);
    }
}
