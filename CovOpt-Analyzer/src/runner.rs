use crate::coverage::CoverageMap;
use covopt_macro::covopt_param;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

static CI_DEADLINE: OnceLock<Instant> = OnceLock::new();

pub fn install_ci_deadline(budget: Duration) -> Result<(), String> {
    let reporting_reserve = Duration::from_millis(covopt_param!("CI_REPORT_RESERVE_MS", 2_000));
    let work_budget = budget.saturating_sub(reporting_reserve);
    CI_DEADLINE
        .set(Instant::now() + work_budget)
        .map_err(|_| "CI deadline was already installed for this process".to_string())
}

pub fn remaining_ci_budget() -> Option<Duration> {
    CI_DEADLINE
        .get()
        .map(|deadline| deadline.saturating_duration_since(Instant::now()))
}

pub fn ci_budget_exhausted() -> bool {
    remaining_ci_budget().is_some_and(|remaining| remaining.is_zero())
}

pub fn command_output_with_ci_deadline(
    command: &mut Command,
    operation: &str,
) -> Result<Output, String> {
    let Some(timeout) = remaining_ci_budget() else {
        return command
            .output()
            .map_err(|error| format!("Failed to run {operation}: {error}"));
    };
    if timeout.is_zero() {
        return Err(format!("CI budget exhausted before {operation}"));
    }

    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("Failed to run {operation}: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("Could not capture stdout for {operation}"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| format!("Could not capture stderr for {operation}"))?;
    let stdout_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut stream = stdout;
        let _ = stream.read_to_end(&mut bytes);
        bytes
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut stream = stderr;
        let _ = stream.read_to_end(&mut bytes);
        bytes
    });

    let started = Instant::now();
    let poll_interval = Duration::from_millis(covopt_param!("CI_PROCESS_POLL_MS", 10));
    let (status, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status, false),
            Ok(None) if started.elapsed() < timeout => std::thread::sleep(poll_interval),
            Ok(None) => {
                #[cfg(unix)]
                {
                    let _ = Command::new("kill")
                        .args(["-TERM", &format!("-{}", child.id())])
                        .status();
                }
                let _ = child.kill();
                let status = child
                    .wait()
                    .map_err(|error| format!("Failed to stop timed-out {operation}: {error}"))?;
                break (status, true);
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Failed while waiting for {operation}: {error}"));
            }
        }
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| format!("stdout reader panicked for {operation}"))?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| format!("stderr reader panicked for {operation}"))?;
    if timed_out {
        return Err(format!(
            "CI budget exhausted while running {operation}: {}",
            String::from_utf8_lossy(&stderr).trim()
        ));
    }
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

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

pub struct WorkspaceCheck {
    pub cargo_check_stdout: String,
}

pub fn check_workspace_with_diagnostics() -> Result<WorkspaceCheck, String> {
    let mut cmd = Command::new("cargo");
    cmd.env("RUSTFLAGS", "--cap-lints warn")
        .env_remove("LLVM_PROFILE_FILE");
    cmd.args([
        "check",
        "--workspace",
        "--all-targets",
        "--message-format=json",
    ]);

    if !crate::config::should_color() {
        cmd.arg("--color=never");
    }

    let output = command_output_with_ci_deadline(&mut cmd, "cargo check --workspace")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut rustc_errors = String::new();
        for line in stdout.lines() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line)
                && let Some(msg) = v
                    .get("message")
                    .and_then(|m| m.get("rendered"))
                    .and_then(|r| r.as_str())
            {
                rustc_errors.push_str(msg);
                rustc_errors.push('\n');
            }
        }
        return Err(format!(
            "Workspace compilation failed.\n{}\n{}",
            stderr, rustc_errors
        ));
    }

    Ok(WorkspaceCheck {
        cargo_check_stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
    })
}

pub fn check_workspace() -> Result<(), String> {
    check_workspace_with_diagnostics().map(|_| ())
}

#[derive(Debug, Clone)]
pub struct CompiledTestExecutable {
    pub path: PathBuf,
    pub package_id: Option<String>,
    pub target_name: Option<String>,
    pub target_kinds: Vec<String>,
    pub tests: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CompiledWorkspaceTests {
    pub executables: Vec<CompiledTestExecutable>,
    pub test_index: HashMap<String, PathBuf>,
}

impl CompiledWorkspaceTests {
    pub fn executable_for(&self, test_name: &str) -> Option<&PathBuf> {
        self.test_index.get(test_name)
    }

    pub fn executable_record_for(&self, test_name: &str) -> Option<&CompiledTestExecutable> {
        self.executables
            .iter()
            .find(|executable| executable.tests.iter().any(|test| test == test_name))
    }
}

pub struct AuditContext {
    pub output_dir: tempfile::TempDir,
    pub workspace_tests: CompiledWorkspaceTests,
    pub packages: Vec<String>,
    pub cli_noise_result: Option<(usize, f64)>,
}

impl AuditContext {
    pub fn compile(packages: &[String]) -> Result<Self, String> {
        let output_dir =
            tempfile::tempdir().map_err(|e| format!("Failed to create audit tempdir: {}", e))?;
        let workspace_tests = compile_workspace_tests(output_dir.path(), packages)?;
        Ok(Self {
            output_dir,
            workspace_tests,
            packages: packages.to_vec(),
            cli_noise_result: None,
        })
    }
}

pub fn compile_workspace_tests(
    output_dir: &Path,
    packages: &[String],
) -> Result<CompiledWorkspaceTests, String> {
    if !output_dir.exists() {
        fs::create_dir_all(output_dir)
            .map_err(|e| format!("Failed to create output directory: {}", e))?;
    }

    let mut cmd = Command::new("cargo");
    cmd.env("RUSTFLAGS", "-C instrument-coverage --cap-lints warn")
        .env(
            "CARGO_ENCODED_RUSTFLAGS",
            "-C\x1finstrument-coverage\x1f--cap-lints\x1fwarn",
        )
        .env(
            "LLVM_PROFILE_FILE",
            output_dir.join("default_%m_%p.profraw"),
        )
        .arg("test")
        .arg("--no-run")
        .arg("--message-format=json");

    if !crate::config::should_color() {
        cmd.arg("--color=never");
    }

    for pkg in packages {
        cmd.arg("-p").arg(pkg);
    }

    let output = command_output_with_ci_deadline(&mut cmd, "instrumented cargo test --no-run")
        .map_err(|e| {
            let err_msg = e.to_string();
            if err_msg.contains("Operation not permitted") || err_msg.contains("Permission denied") {
                format!("Failed to run cargo test: {}\n[Hint] Permission error detected. If running inside a sandboxed environment, ensure RUSTUP_HOME/CARGO_HOME is accessible or try --bypass-sandbox.", e)
            } else {
                format!("Failed to run cargo test: {}", e)
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut rustc_errors = String::new();
        for line in stdout.lines() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line)
                && let Some(msg) = v
                    .get("message")
                    .and_then(|m| m.get("rendered"))
                    .and_then(|r| r.as_str())
            {
                rustc_errors.push_str(msg);
                rustc_errors.push('\n');
            }
        }
        if stderr.contains("Operation not permitted")
            || stderr.contains("Permission denied")
            || stderr.contains("os error 1")
        {
            return Err(format!(
                "Compilation failed: {}\n{}\n[Hint] Permission error detected while accessing toolchain files (e.g. ~/.rustup). If running inside an isolated sandbox, try using `--bypass-sandbox` or check permissions.",
                stderr, rustc_errors
            ));
        }
        return Err(format!("Compilation failed: {}\n{}", stderr, rustc_errors));
    }

    let mut compiled = CompiledWorkspaceTests::default();
    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line)
            && v.get("reason").and_then(|r| r.as_str()) == Some("compiler-artifact")
            && v.get("profile")
                .and_then(|p| p.get("test"))
                .and_then(|t| t.as_bool())
                == Some(true)
            && let Some(exe) = v.get("executable").and_then(|e| e.as_str())
        {
            // Exclude proc-macro binaries (which fail with dyld error on macOS)
            let is_proc_macro = v
                .get("target")
                .and_then(|t| t.get("kind"))
                .and_then(|k| k.as_array())
                .is_some_and(|kinds| {
                    kinds.iter().any(|k| {
                        k.as_str()
                            .is_some_and(|s| s.contains("proc-macro") || s.contains("proc_macro"))
                    })
                })
                || v.get("target")
                    .and_then(|t| t.get("crate_types"))
                    .and_then(|k| k.as_array())
                    .is_some_and(|types| {
                        types.iter().any(|t| {
                            t.as_str().is_some_and(|s| {
                                s.contains("proc-macro") || s.contains("proc_macro")
                            })
                        })
                    })
                || exe.contains("covopt_macro")
                || exe.contains("covopt-macro")
                || exe.contains("proc_macro")
                || exe.contains("proc-macro");

            if !is_proc_macro {
                let path = PathBuf::from(exe);
                let tests = list_tests_in_executable(&path)?;
                for test_name in &tests {
                    compiled
                        .test_index
                        .entry(test_name.clone())
                        .or_insert_with(|| path.clone());
                }
                compiled.executables.push(CompiledTestExecutable {
                    path,
                    package_id: v
                        .get("package_id")
                        .and_then(|p| p.as_str())
                        .map(ToOwned::to_owned),
                    target_name: v
                        .get("target")
                        .and_then(|t| t.get("name"))
                        .and_then(|n| n.as_str())
                        .map(ToOwned::to_owned),
                    target_kinds: v
                        .get("target")
                        .and_then(|target| target.get("kind"))
                        .and_then(|kinds| kinds.as_array())
                        .into_iter()
                        .flatten()
                        .filter_map(|kind| kind.as_str().map(ToOwned::to_owned))
                        .collect(),
                    tests,
                });
            }
        }
    }

    Ok(compiled)
}

fn list_tests_in_executable(executable: &Path) -> Result<Vec<String>, String> {
    let output = Command::new(executable)
        .args(["--list", "--format", "terse"])
        .output()
        .map_err(|e| {
            format!(
                "Failed to list tests in executable {}: {}",
                executable.display(),
                e
            )
        })?;

    if !output.status.success() {
        return Err(format!(
            "Failed to list tests in executable {}: {}",
            executable.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().rsplit_once(": ").map(|(name, _)| name.trim()))
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

pub fn debug_artifacts_enabled() -> bool {
    matches!(
        std::env::var("COVOPT_DEBUG").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    ) || matches!(
        std::env::var("COVOPT_DEBUG_ARTIFACTS").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

pub struct CargoTestRunner {
    pub test_name: String,
    pub output_dir: PathBuf,
    pub executables: Vec<PathBuf>,
    pub test_executable: Option<PathBuf>,
    pub package_id: Option<String>,
    pub cargo_target_name: Option<String>,
    pub cargo_target_kinds: Vec<String>,
}

fn cargo_package_name(package_id: &str) -> Option<&str> {
    let (source, fragment) = package_id
        .rsplit_once('#')
        .map_or((package_id, package_id), |(source, value)| (source, value));
    let name = if let Some((name, _version)) = fragment.split_once('@') {
        name
    } else if fragment
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_digit())
    {
        source.trim_end_matches('/').rsplit('/').next()?
    } else {
        fragment.split_whitespace().next()?
    };
    (!name.is_empty()).then_some(name)
}

fn add_cargo_target_selector(
    command: &mut Command,
    target_name: &str,
    target_kinds: &[String],
) -> Result<(), String> {
    let has_kind = |expected: &str| target_kinds.iter().any(|kind| kind == expected);
    if has_kind("test") {
        command.arg("--test").arg(target_name);
    } else if has_kind("lib") || has_kind("rlib") {
        command.arg("--lib");
    } else if has_kind("bin") {
        command.arg("--bin").arg(target_name);
    } else if has_kind("example") {
        command.arg("--example").arg(target_name);
    } else if has_kind("bench") {
        command.arg("--bench").arg(target_name);
    } else {
        return Err(format!(
            "Unsupported Cargo target kind {:?} for '{}'",
            target_kinds, target_name
        ));
    }
    Ok(())
}

fn asm_artifacts_from_cargo_json(stdout: &[u8], target_name: &str) -> Vec<PathBuf> {
    String::from_utf8_lossy(stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|message| {
            message.get("reason").and_then(|value| value.as_str()) == Some("compiler-artifact")
                && message
                    .get("target")
                    .and_then(|target| target.get("name"))
                    .and_then(|name| name.as_str())
                    == Some(target_name)
        })
        .flat_map(|message| {
            message
                .get("filenames")
                .and_then(|filenames| filenames.as_array())
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|filename| filename.as_str().map(PathBuf::from))
        .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("s"))
        .collect()
}

fn find_target_asm_fallback(target_name: &str) -> Vec<PathBuf> {
    let artifact_prefix = format!("{}-", target_name.replace('-', "_"));
    [
        PathBuf::from("target/release/deps"),
        PathBuf::from("../target/release/deps"),
        PathBuf::from("../../target/release/deps"),
    ]
    .into_iter()
    .filter_map(|directory| fs::read_dir(directory).ok())
    .flatten()
    .filter_map(Result::ok)
    .map(|entry| entry.path())
    .filter(|path| {
        path.extension().and_then(|extension| extension.to_str()) == Some("s")
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&artifact_prefix))
    })
    .max_by_key(|path| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    })
    .into_iter()
    .collect()
}

impl CargoTestRunner {
    pub fn new(test_name: &str, output_dir: &Path, executables: Vec<PathBuf>) -> Self {
        Self {
            test_name: test_name.to_string(),
            output_dir: output_dir.to_path_buf(),
            executables,
            test_executable: None,
            package_id: None,
            cargo_target_name: None,
            cargo_target_kinds: Vec::new(),
        }
    }

    pub fn from_compiled(
        test_name: &str,
        output_dir: &Path,
        compiled: &CompiledWorkspaceTests,
    ) -> Self {
        let record = compiled.executable_record_for(test_name);
        Self {
            test_name: test_name.to_string(),
            output_dir: output_dir.to_path_buf(),
            executables: compiled
                .executables
                .iter()
                .map(|executable| executable.path.clone())
                .collect(),
            test_executable: record
                .map(|executable| executable.path.clone())
                .or_else(|| compiled.executable_for(test_name).cloned()),
            package_id: record.and_then(|executable| executable.package_id.clone()),
            cargo_target_name: record.and_then(|executable| executable.target_name.clone()),
            cargo_target_kinds: record
                .map(|executable| executable.target_kinds.clone())
                .unwrap_or_default(),
        }
    }

    pub fn run(&self, n: usize, seed: Option<u64>) -> Result<(CoverageMap, u64), String> {
        if !self.output_dir.exists() {
            fs::create_dir_all(&self.output_dir)
                .map_err(|e| format!("Failed to create output directory: {}", e))?;
        }

        let executable = self.test_executable.as_ref().ok_or_else(|| {
            format!(
                "No compiled test executable contains target test '{}'; refusing to run all harnesses",
                self.test_name
            )
        })?;

        let t2 = std::time::Instant::now();
        let peak_rss = self.execute_test(executable, n, seed)?;
        let t3 = std::time::Instant::now();
        self.merge_profdata(n)?;
        let t4 = std::time::Instant::now();

        let lcov_str = self.export_lcov(std::slice::from_ref(executable), n)?;
        let t5 = std::time::Instant::now();

        let map = CoverageMap::from_lcov(&lcov_str)?;
        let t6 = std::time::Instant::now();

        if debug_artifacts_enabled() {
            eprintln!(
                "[Profile] execute_tests (incl. OS process spawn overhead): {:?}",
                t3.duration_since(t2)
            );
            eprintln!("[Profile] merge_profdata: {:?}", t4.duration_since(t3));
            eprintln!("[Profile] export_lcov: {:?}", t5.duration_since(t4));
            eprintln!("[Profile] parse_lcov: {:?}", t6.duration_since(t5));
            let _ = std::fs::write(
                self.output_dir.join(format!("covopt_debug_{}.json", n)),
                &lcov_str,
            );
        }
        Ok((map, peak_rss))
    }

    fn execute_test(&self, exe: &Path, n: usize, seed: Option<u64>) -> Result<u64, String> {
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

        let output = command_output_with_ci_deadline(
            &mut cmd,
            &format!("coverage test '{}'", self.test_name),
        )?;

        let stderr = String::from_utf8_lossy(&output.stderr);

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
            return Err(format!(
                "Test '{}' failed in {}: {}",
                self.test_name,
                exe.display(),
                stderr.trim()
            ));
        }
        if std::env::var("COVOPT_COMPACT").is_err() {
            println!("Test ran successfully.");
        }
        let has_profraw = fs::read_dir(&self.output_dir)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .any(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.starts_with(&format!("covopt_{}_", n)) && name.ends_with(".profraw")
            });
        if !has_profraw {
            return Err(format!(
                "Test '{}' completed but produced no coverage profile",
                self.test_name
            ));
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

        let output = command_output_with_ci_deadline(&mut cmd, "llvm-profdata merge")?;
        if !output.status.success() {
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

        // The first executable is the positional BINARY argument
        cmd.arg(&executables[0]);

        // Add the rest using -object
        for exe in executables.iter().skip(1) {
            cmd.arg("-object").arg(exe);
        }

        let output = command_output_with_ci_deadline(&mut cmd, "llvm-cov export")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("LLVM Cov Export failed: {}", stderr));
        }

        let lcov = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(lcov)
    }

    pub fn compile_asm(&self) -> Result<String, String> {
        let target_name = self.cargo_target_name.as_deref().ok_or_else(|| {
            format!(
                "Cannot resolve the Cargo target containing test '{}'",
                self.test_name
            )
        })?;
        let mut command = Command::new("cargo");
        command
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("LLVM_PROFILE_FILE")
            .arg("rustc")
            .arg("--release")
            .arg("--message-format=json");

        if let Some(package_name) = self.package_id.as_deref().and_then(cargo_package_name) {
            command.arg("-p").arg(package_name);
        }
        add_cargo_target_selector(&mut command, target_name, &self.cargo_target_kinds)?;
        command.args(["--", "-g", "--emit=asm"]);

        let output = command_output_with_ci_deadline(&mut command, "targeted release ASM build")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("ASM Compilation failed: {}", stderr));
        }

        let asm_files = asm_artifacts_from_cargo_json(&output.stdout, target_name);
        let asm_files = if asm_files.is_empty() {
            find_target_asm_fallback(target_name)
        } else {
            asm_files
        };
        if asm_files.is_empty() {
            return Err(format!(
                "Cargo produced no assembly artifact for target '{}'",
                target_name
            ));
        }

        let mut target_asm = String::new();
        for path in asm_files {
            let content = fs::read_to_string(&path).map_err(|error| {
                format!("Cannot read ASM artifact {}: {}", path.display(), error)
            })?;
            target_asm.push_str(&content);
            target_asm.push('\n');
        }
        Ok(target_asm)
    }

    pub fn extract_asm_block(&self, asm_content: &str, symbol: &str) -> Option<String> {
        let symbol_label1 = format!("{}:", symbol);
        let symbol_label2 = format!("_{}:", symbol); // macOS adds an extra leading underscore
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
            } else if line == symbol_label1 || line == symbol_label2 {
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
                if parts.len() >= covopt_param!("M_484_34", 3) {
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
                if parts.len() >= covopt_param!("M_502_34", 3) {
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
                    for j in 1..=covopt_param!("M_557_33", 5) {
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
        let hit_count = map.get_hit_count(&source_str, covopt_param!("M_628_55", 4));
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
            fs::set_permissions(
                &fake_cov_path,
                fs::Permissions::from_mode(covopt_param!("M_716_75", 0o755)),
            )
            .unwrap();
            fs::set_permissions(
                &out_bin_path,
                fs::Permissions::from_mode(covopt_param!("M_717_74", 0o755)),
            )
            .unwrap();
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

    #[test]
    fn cargo_package_name_supports_modern_package_ids() {
        assert_eq!(
            cargo_package_name("path+file:///workspace#CovOpt-Analyzer@2.0.0"),
            Some("CovOpt-Analyzer")
        );
        assert_eq!(
            cargo_package_name("path+file:///workspace/CovOpt-Analyzer#2.0.0"),
            Some("CovOpt-Analyzer")
        );
        assert_eq!(
            cargo_package_name("registry+https://example.invalid/index#serde@1.0.0"),
            Some("serde")
        );
    }

    #[test]
    fn cargo_target_selector_uses_the_exact_integration_test() {
        let mut command = Command::new("cargo");
        add_cargo_target_selector(&mut command, "binary_search", &["test".to_string()]).unwrap();
        let arguments = command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(arguments, ["--test", "binary_search"]);
    }

    #[test]
    fn cargo_json_selects_only_the_requested_asm_artifact() {
        let stdout = br#"
{"reason":"compiler-artifact","target":{"name":"other"},"filenames":["/tmp/other.s"]}
{"reason":"compiler-artifact","target":{"name":"binary_search"},"filenames":["/tmp/binary_search.s","/tmp/binary_search.d"]}
"#;
        assert_eq!(
            asm_artifacts_from_cargo_json(stdout, "binary_search"),
            [PathBuf::from("/tmp/binary_search.s")]
        );
    }

    #[test]
    fn test_check_workspace() {
        let res = check_workspace();
        assert!(
            res.is_ok(),
            "check_workspace should succeed on healthy codebase: {:?}",
            res.err()
        );
    }
}
