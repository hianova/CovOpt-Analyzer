use serde::Deserialize;
use std::fs;
use std::path::Path;

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

#[derive(Deserialize, Debug, Clone)]
pub struct PipelineConfig {
    #[serde(default = "default_true")]
    pub run_fix: bool,
    #[serde(default = "default_true")]
    pub run_audit: bool,
    #[serde(default = "default_false")]
    pub run_optimize: bool,
    #[serde(default = "default_false")]
    pub run_harden: bool,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            run_fix: true,
            run_audit: true,
            run_optimize: false,
            run_harden: false,
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct TargetConfig {
    pub test: String,
    pub tests: Option<String>,
    pub package: Option<String>,
    pub expected: Option<String>,
    pub n_values: Option<String>,
    pub fuzz_iterations: Option<u32>,
    pub mca_cpu: Option<String>,
    pub ignore: Option<Vec<String>>,
    #[serde(default = "default_true")]
    pub require_cache_padding: bool,
    #[serde(default = "default_true")]
    pub require_branch_hints: bool,
    #[serde(default = "default_true")]
    pub require_aerospace_grade: bool,
    #[serde(default = "default_true")]
    pub require_watchdog_timeout: bool,
    #[serde(default = "default_true")]
    pub require_stress_test: bool,
    pub polling_threshold: Option<u64>,
}

impl TargetConfig {
    // Deprecated methods removed.
}

#[derive(Deserialize, Debug, Clone)]
pub struct CovOptConfig {
    #[serde(default)]
    pub target: Vec<TargetConfig>,
    #[serde(default)]
    pub pipeline: PipelineConfig,
}

impl CovOptConfig {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let content = fs::read_to_string(&path).map_err(|e| {
            format!(
                "Failed to read config file {}: {}",
                path.as_ref().display(),
                e
            )
        })?;
        toml::from_str(&content)
            .map_err(|e| format!("Failed to parse {}: {}", path.as_ref().display(), e))
    }
}

pub fn should_color() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal() && std::env::var("NO_COLOR").is_err()
}

#[derive(clap::Args, Debug, Clone)]
pub struct ReportArgs {
    #[arg(long, default_value = "target/covopt")]
    pub output_dir: String,

    /// Output format (html or sarif)
    #[arg(long, default_value = "html")]
    pub format: String,
}

#[derive(clap::Args, Debug, Clone)]
pub struct AuditArgs {
    /// Run audit only on explicitly git staged files
    #[arg(long)]
    pub staged: bool,
    /// The name of the test target to audit
    #[arg(long)]
    pub test: Option<String>,

    /// Run in fast mode (only use min and max N values)
    #[arg(long)]
    pub fast: bool,

    /// Output report as structured JSON for AI Agents
    #[arg(long)]
    pub json: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct AdviseArgs {
    /// Only analyze files modified compared to the specified git branch
    #[arg(long)]
    pub diff: Option<String>,
    /// Target file or directory to analyze (defaults to "src/")
    #[arg(default_value = "src/")]
    pub path: String,

    /// Optional function name to analyze
    #[arg(short, long)]
    pub func: Option<String>,
}

#[derive(clap::Args, Debug, Clone)]
pub struct InitArgs {
    /// Optional path to initialize in (defaults to current directory)
    pub path: Option<String>,

    /// Skip interactive prompts and accept default values
    #[arg(short, long)]
    pub yes: bool,

    /// Install a pre-commit hook in the target git repository
    #[arg(long, default_value_t = false)]
    pub hook: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct HardenArgs {
    /// Target directory for harness generation
    #[arg(default_value = "src/")]
    pub path: String,

    /// The name of the test target
    #[arg(short, long)]
    pub test: Option<String>,

    /// Ignore uninstalled tools instead of failing
    #[arg(long, default_value_t = false)]
    pub fast: bool,

    /// Generate fuzzing harnesses for public functions instead of running hardening
    #[arg(long, default_value_t = false)]
    pub generate_harness: bool,

    /// Run mutation testing using cargo-mutants
    #[arg(long, default_value_t = false)]
    pub mutate: bool,

    /// Run fuzzing using cargo-fuzz
    #[arg(long, default_value_t = false)]
    pub fuzz: bool,

    /// Run tests with LLVM sanitizers
    #[arg(long, default_value_t = false)]
    pub sanitize: bool,

    /// Sanitizer type (address or thread)
    #[arg(long, default_value = "address")]
    pub san_type: String,

    /// Automatically repair memory safety crashes using LLM
    #[arg(long, default_value_t = false)]
    pub auto_fix: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct CiArgs {
    /// Only run CI on files modified compared to the specified git branch
    #[arg(long)]
    pub base: Option<String>,
    /// Skip the hardening (fuzz/mutate) step
    #[arg(long, default_value_t = false)]
    pub skip_harden: bool,

    /// Fail the CI if any step produces a non-perfect result
    #[arg(long, default_value_t = false)]
    pub strict: bool,

    /// Run in fast mode (skips heavy tuning/fuzzing and uses fast audit)
    #[arg(long, default_value_t = false)]
    pub fast: bool,

    /// Generate an HTML dashboard report after CI completes
    #[arg(long, default_value_t = false)]
    pub report: bool,

    /// Generate a SARIF report after CI completes
    #[arg(long, default_value_t = false)]
    pub sarif: bool,
}


#[derive(clap::Args, Debug, Clone)]
pub struct FixArgs {
    /// Optional path to scan and fix (defaults to current directory)
    pub path: Option<String>,

    /// Only run cargo clippy --fix
    #[arg(long, default_value_t = false)]
    pub only_clippy: bool,

    /// Only run magic number to covopt_param! substitution
    #[arg(long, default_value_t = false)]
    pub only_magic: bool,
}


#[derive(clap::Args, Debug, Clone)]
pub struct ProfileArgs {
    /// The name of the test to profile
    #[arg(long)]
    pub test: Option<String>,

    /// The name of the binary to profile
    #[arg(long)]
    pub bin: Option<String>,

    /// Profiling tool to use
    #[arg(long, default_value = "flamegraph", value_name = "flamegraph|samply")]
    pub tool: String,
}

#[derive(clap::Args, Debug, Clone)]
#[command(next_help_heading = "Default Run Mode Options")]
pub struct RunArgs {
    /// The name of the test to run
    #[arg(short, long)]
    pub test: Option<String>,

    /// Expected complexity (e.g. O1, OLogN, ON, ONLogN, ON2)
    #[arg(short, long)]
    pub expected: Option<String>,

    /// Comma-separated list of N values (e.g. 100,1000,10000)
    #[arg(short, long)]
    pub n_values: Option<String>,

    /// Optional LLVM-MCA CPU target (e.g. apple-m1, skylake)
    #[arg(long)]
    pub mca_cpu: Option<String>,

    /// Comma-separated list of symbols to ignore in coverage peak search
    #[arg(long)]
    pub ignore: Option<String>,

    /// Require static cache padding detection
    #[arg(long, hide = true)]
    pub require_cache_padding: bool,

    /// Enable symbolic regression to reinvent Lean 4 style formal mathematical proofs
    #[arg(long, hide = true)]
    pub formalize: bool,

    /// Require static branch prediction hint detection
    #[arg(long, hide = true)]
    pub require_branch_hints: bool,

    /// Require strict aerospace grade static analysis (#![no_std], zero-alloc, TTAS locks, RAII)
    #[arg(long)]
    pub require_aerospace_grade: bool,

    /// Require watchdog timeout detection in the target file
    #[arg(long, hide = true)]
    pub require_watchdog_timeout: bool,

    /// Require high-pressure stress test detection in the target file
    #[arg(long, hide = true)]
    pub require_stress_test: bool,

    /// Optional polling threshold for high-frequency polling detection
    #[arg(long, hide = true)]
    pub polling_threshold: Option<u64>,

    /// Run the discrete diffusion NP-hard solver to superoptimize ASM
    #[arg(long, hide = true)]
    pub optimize: bool,
    /// Output report as structured JSON for AI Agents
    #[arg(long)]
    pub json: bool,
}
