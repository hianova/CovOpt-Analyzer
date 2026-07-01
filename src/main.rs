pub mod coverage;
pub mod analyzer;
pub mod runner;
pub mod mca;


use clap::Parser;

use analyzer::{Complexity, ConvergenceAnalyzer};
use runner::CargoTestRunner;
use mca::McaRunner;

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

    /// Optional LLVM-MCA CPU target (e.g. apple-m1, skylake)
    #[arg(long)]
    mca_cpu: Option<String>,
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
    let mut target_symbol: Option<String> = None;

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

        // Try to find the symbol if not found yet
        if target_symbol.is_none() {
            target_symbol = map.find_symbol(&args.target_file, args.target_line);
        }
    }

    println!("---------------------------------------------------");
    println!("Analysis Results:");
    let report = ConvergenceAnalyzer::analyze(&data, expected);
    println!("{:#?}", report);

    println!("---------------------------------------------------");
    if let Some(symbol) = target_symbol {
        println!("Target Symbol Found: {}", symbol);
        println!("Extracting ASM and running LLVM-MCA analysis...");
        
        match runner.compile_asm() {
            Ok(asm_content) => {
                let mut asm_block_opt = runner.extract_asm_block(&asm_content, &symbol);
                if asm_block_opt.is_none() {
                    // Try to demangle the symbol to find base keywords
                    let demangled = rustc_demangle::demangle(&symbol).to_string();
                    // Example: <dualcache_ff[..]::core::cache_tier::CacheTier<u64>>::insert::<88usize>
                    // Remove trailing generics if any: "::<88usize>" -> ""
                    let no_trailing_generics = match demangled.rfind(">::") {
                        Some(idx) => &demangled[..idx+1],
                        None => &demangled,
                    };
                    
                    let parts: Vec<&str> = no_trailing_generics.split("::").collect();
                    if parts.len() >= 2 {
                        let fn_name = parts.last().unwrap_or(&"").split('<').next().unwrap_or("").trim();
                        let struct_part = parts[parts.len() - 2];
                        let struct_name = struct_part.split('<').next().unwrap_or("").trim().trim_start_matches(|c| c == '<' || c == '[');
                        
                        println!("  -> Target symbol exact match failed. Searching by keywords: '{}', '{}'...", struct_name, fn_name);
                        asm_block_opt = runner.extract_asm_block_by_keywords(&asm_content, &[struct_name, fn_name]);
                    }

                    if asm_block_opt.is_none() {
                        println!("  -> Still not found. Target symbol inlined. Walking up to test caller '{}'...", args.test);
                        asm_block_opt = runner.extract_asm_block_by_keywords(&asm_content, &[&args.test]);
                    }
                }

                if let Some(asm_block) = asm_block_opt {
                    let mca_runner = McaRunner::new(args.mca_cpu.clone());
                    match mca_runner.run(&asm_block) {
                        Ok(mca_report) => {
                            println!("\n[MCA Report]");
                            println!("Block RThroughput: {:.2}", mca_report.block_rthroughput);
                            println!("IPC:               {:.2}", mca_report.ipc);
                            println!("Total Cycles:      {}", mca_report.total_cycles);
                            println!("Instructions:      {}", mca_report.instructions);

                            // Optional heuristic fix suggestion
                            if report.is_converged && report.actual_trend > expected {
                                println!("\n💡 [Fix Suggestion]");
                                if mca_report.block_rthroughput > 5.0 {
                                    println!("High Block RThroughput detected. The loop has a severe structural bottleneck.");
                                    println!("Consider Loop Unrolling or reducing data dependencies (Read-After-Write).");
                                } else {
                                    println!("Algorithmic degradation detected without high pipeline stalls.");
                                    println!("Consider using a better data structure (e.g. HashMap instead of Vec) or algorithm.");
                                }
                            }
                        }
                        Err(e) => eprintln!("LLVM-MCA failed: {}", e),
                    }
                } else {
                    eprintln!("Could not extract ASM block for symbol. The function might be inlined in release mode.");
                }
            }
            Err(e) => eprintln!("ASM compilation failed: {}", e),
        }
    } else {
        println!("Could not extract target symbol name from coverage data. Skipping MCA analysis.");
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_dummy() {}
}
