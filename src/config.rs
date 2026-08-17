use clap::Args;

#[derive(Args, Debug, Clone, Default)]
pub struct DoctorArgs {
    /// Show detailed system toolchain information and paths
    #[clap(short, long)]
    pub verbose: bool,
}

#[derive(Args, Debug, Clone, Default)]
pub struct RunArgs {
    /// Number of annealing iterations for parameter optimization
    #[clap(long, default_value = "5")]
    pub iterations: usize,

    /// Dry run: run the entire Ramanujan pipeline and preview diff without writing
    #[clap(long)]
    pub dry_run: bool,

    /// Suppress progress indicators and non-essential output
    #[clap(short, long)]
    pub quiet: bool,

    /// Show detailed debug logs and AST transformations
    #[clap(short, long)]
    pub verbose: bool,
}

#[derive(Args, Debug, Clone, Default)]
pub struct RollbackArgs {
    /// Target identifier or function name to rollback
    #[clap(short, long)]
    pub target: Option<String>,

    /// Specific backup timestamp ID to restore from
    #[clap(long)]
    pub id: Option<String>,
}
