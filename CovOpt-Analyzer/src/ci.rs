//! Compatibility adapter for the pre-V3 `ci` entry point.
//!
//! The command implementation lives in `commands::run_check`; this module
//! remains only so downstream callers of the old library API do not break.

use crate::commands;
use CovOpt_Analyzer::config::{CheckArgs, CiArgs, CovOptConfig};

pub fn run_pipeline(
    _config: CovOptConfig,
    args: &CiArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    commands::run_check(&CheckArgs {
        base: args.base.clone(),
        target: None,
        mode: args.assurance,
        plan: false,
        format: if args.sarif {
            "sarif".to_string()
        } else if args.report {
            "html".to_string()
        } else {
            "text".to_string()
        },
        fast: args.fast,
        staged: false,
        debug_artifacts: false,
        budget: args.budget.clone(),
    })?;
    Ok(())
}
