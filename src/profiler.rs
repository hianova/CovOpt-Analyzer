use std::process::Command;
use std::io::{self, Write};

pub fn run_profile(test_name: &str, tool: &str) -> bool {
    println!("Starting profiler '{}' for test '{}'...", tool, test_name);

    match tool.to_lowercase().as_str() {
        "flamegraph" => run_flamegraph(test_name),
        "samply" => run_samply(test_name),
        _ => {
            eprintln!("[ERROR] Unknown profiling tool '{}'. Supported tools are 'flamegraph' and 'samply'.", tool);
            false
        }
    }
}

fn check_command_exists(cmd: &str, install_hint: &str) -> bool {
    let output = Command::new(cmd).arg("--version").output();
    if output.is_err() {
        eprintln!("\n[ERROR] Tool '{}' not found in PATH.", cmd);
        eprintln!("Please install it via: {}", install_hint);
        eprintln!("Wait, is it installed? Make sure ~/.cargo/bin is in your PATH.");
        false
    } else {
        true
    }
}

fn run_flamegraph(test_name: &str) -> bool {
    // Check if cargo-flamegraph is installed
    if !check_command_exists("cargo-flamegraph", "cargo install cargo-flamegraph") {
        return false;
    }

    println!("Running: cargo flamegraph --test {}", test_name);
    
    let mut child = Command::new("cargo")
        .arg("flamegraph")
        .arg("--test")
        .arg(test_name)
        .spawn()
        .expect("Failed to start cargo flamegraph");

    let status = child.wait().expect("Failed to wait for cargo flamegraph");

    if status.success() {
        println!("\n[SUCCESS] Flamegraph generated successfully (usually flamegraph.svg).");
        println!("Open it in your browser to analyze allocations and CPU hotspots.");
        parse_and_print_svg_bottlenecks();
        true
    } else {
        eprintln!("\n[ERROR] cargo flamegraph failed with status: {}", status);
        false
    }
}

fn parse_and_print_svg_bottlenecks() {
    let svg_content = match std::fs::read_to_string("flamegraph.svg") {
        Ok(c) => c,
        Err(_) => return,
    };
    
    let mut hotspots = Vec::new();
    
    for line in svg_content.lines() {
        let mut search_from = 0;
        while let Some(start_idx) = line[search_from..].find("<title>") {
            let abs_start = search_from + start_idx;
            if let Some(end_idx) = line[abs_start..].find("</title>") {
                let abs_end = abs_start + end_idx;
                let inner = &line[abs_start + 7..abs_end];
                search_from = abs_end + 8;
                
                if inner == "all" || inner.starts_with("all (") { continue; }
                
                if let Some(paren_idx) = inner.rfind(" (") {
                    let func_name = &inner[0..paren_idx];
                    let stats = &inner[paren_idx + 2..inner.len() - 1]; // "X samples, Y%"
                    
                    if let Some(comma_idx) = stats.find(" samples, ") {
                        let samples_str = &stats[0..comma_idx].replace(",", "");
                        let pct_str = &stats[comma_idx + 10..stats.len() - 1]; // "Y" without %
                        
                        if let (Ok(samples), Ok(pct)) = (samples_str.parse::<u64>(), pct_str.parse::<f64>()) {
                            hotspots.push((func_name.to_string(), samples, pct));
                        }
                    }
                }
            } else {
                break;
            }
        }
    }
    
    if hotspots.is_empty() { return; }
    
    hotspots.sort_by(|a, b| b.1.cmp(&a.1));
    hotspots.dedup_by(|a, b| a.0 == b.0);
    
    println!("\n🔥 Top 5 CPU Hotspots (Actionable Guidance):");
    println!("---------------------------------------------------");
    for (i, (name, samples, pct)) in hotspots.iter().take(5).enumerate() {
        println!("{}. {} - {} samples ({:.1}%)", i + 1, name, samples, pct);
    }
    println!("---------------------------------------------------");
}

fn run_samply(test_name: &str) -> bool {
    // Check if samply is installed
    if !check_command_exists("samply", "cargo install samply") {
        return false;
    }

    println!("Running: samply record cargo test --test {} --release", test_name);
    
    let mut child = Command::new("samply")
        .arg("record")
        .arg("cargo")
        .arg("test")
        .arg("--test")
        .arg(test_name)
        .arg("--release")
        .spawn()
        .expect("Failed to start samply");

    let status = child.wait().expect("Failed to wait for samply");

    if status.success() {
        println!("\n[SUCCESS] Samply profile recorded successfully.");
        println!("Samply should have automatically opened the profiler UI in your browser.");
        println!("Check the Timeline view to find lock contention and thread synchronization bottlenecks.");
        true
    } else {
        eprintln!("\n[ERROR] samply failed with status: {}", status);
        false
    }
}
