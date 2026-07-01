pub mod coverage;
pub mod analyzer;
pub mod runner;

use clap::Parser;


use analyzer::{Complexity, ConvergenceAnalyzer};
use runner::CargoTestRunner;

#[derive(Parser, Debug)]
#[command(name = "covopt")]
#[command(author, version, about = "Coverage-based Complexity Analyzer", long_about = None)]
struct Args {
    /// The name of the test to run
    #[arg(short, long)]
    test: String,

    /// Expected complexity (e.g. O1, OLogN, ON, ONLogN, ON2)
    #[arg(short, long)]
    expected: String,

    /// Comma-separated list of N values (e.g. 100,1000,10000)
    #[arg(short, long)]
    n_values: String,

    /// Target file to track coverage in
    #[arg(long)]
    target_file: String,

    /// Target line number to track hit count
    #[arg(long)]
    target_line: u64,
}

fn parse_complexity(s: &str) -> Complexity {
    match s.to_uppercase().as_str() {
        "O1" | "O(1)" => Complexity::O1,
        "OLOGN" | "O(LOGN)" => Complexity::OLogN,
        "ON" | "O(N)" => Complexity::ON,
        "ONLOGN" | "O(NLOGN)" => Complexity::ONLogN,
        "ON2" | "O(N2)" | "O(N^2)" => Complexity::ON2,
        _ => panic!("Unknown complexity: {}", s),
    }
}

fn main() {
    let args = Args::parse();
    
    let expected = parse_complexity(&args.expected);

    let n_values: Vec<usize> = args.n_values
        .split(',')
        .map(|s| s.trim().parse().expect("Failed to parse N value"))
        .collect();

    let dir = tempfile::tempdir().expect("Failed to create temporary directory");
    let output_dir = dir.path().to_path_buf();
    let runner = CargoTestRunner::new(&args.test, &output_dir);
    
    let mut data = Vec::new();

    println!("Starting CovOpt Analysis for test '{}'...", args.test);
    println!("Target: {}:{}", args.target_file, args.target_line);
    println!("Expected Complexity: {:?}", expected);
    println!("Testing N values: {:?}", n_values);
    println!("---------------------------------------------------");
    
    for n in n_values {
        println!("Running for N = {}...", n);
        let map = match runner.run(n) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("Failed to run coverage for N={}: {}", n, e);
                std::process::exit(1);
            }
        };

        let hit_count = map.find_hit_count(&args.target_file, args.target_line);
        if let Some(h) = hit_count {
            println!("  -> Hit count = {}", h);
            data.push((n, h));
        } else {
            eprintln!("  -> WARNING: No hit count found for target file/line. Assuming 0.");
            data.push((n, 0));
        }
    }

    println!("---------------------------------------------------");
    println!("Analysis Results:");
    let report = ConvergenceAnalyzer::analyze(&data, expected);
    println!("{:#?}", report);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_dummy() {}
}
