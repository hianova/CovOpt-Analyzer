use std::env;
use std::process::{Command, exit};

fn main() {
    let mut args: Vec<String> = env::args().collect();
    
    // When invoked as `cargo covopt`, the second argument is the subcommand "covopt".
    // We strip it so the underlying `covopt` binary gets the correct arguments.
    if args.len() > 1 && args[1] == "covopt" {
        args.remove(1);
    }
    
    // The `covopt` executable should be sitting right next to `cargo-covopt`
    let mut exe_path = env::current_exe().expect("Failed to get current executable path");
    exe_path.set_file_name(if cfg!(windows) { "covopt.exe" } else { "covopt" });
    
    // Fallback: If for some reason covopt isn't next to us, assume it's in PATH
    let cmd = if exe_path.exists() {
        exe_path.to_string_lossy().to_string()
    } else {
        "covopt".to_string()
    };

    let status = Command::new(&cmd)
        .args(&args[1..])
        .status()
        .unwrap_or_else(|e| {
            eprintln!("CovOpt Error: Failed to spawn '{}': {}", cmd, e);
            exit(1);
        });

    exit(status.code().unwrap_or(1));
}
