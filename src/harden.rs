use std::process::Command;
use std::path::Path;

fn check_command_exists(cmd: &str, install_hint: &str) -> bool {
    let output = Command::new(cmd).arg("--version").output();
    if output.is_err() {
        eprintln!("\n[ERROR] Tool '{}' not found.", cmd);
        eprintln!("Please install it via: {}", install_hint);
        false
    } else {
        true
    }
}

pub fn run_mutants(test_name: &str) -> bool {
    if !check_command_exists("cargo-mutants", "cargo install cargo-mutants") {
        return false;
    }
    
    println!("Starting Mutation Testing for target: {}", test_name);
    println!("cargo-mutants will intentionally inject bugs into your code to see if your tests catch them.");
    
    let mut child = Command::new("cargo")
        .arg("mutants")
        .arg("--test")
        .arg(test_name)
        .spawn()
        .expect("Failed to start cargo-mutants");

    let status = child.wait().expect("Failed to wait for cargo-mutants");

    if status.success() {
        println!("\n[SUCCESS] Mutation testing passed! Your tests are robust.");
        true
    } else {
        eprintln!("\n[ERROR] Mutation testing failed. Some injected bugs survived, meaning your tests need stricter assertions.");
        false
    }
}

pub fn run_fuzz(target_name: &str) -> bool {
    if !check_command_exists("cargo-fuzz", "cargo install cargo-fuzz") {
        return false;
    }
    
    let fuzz_dir = Path::new("fuzz");
    if !fuzz_dir.exists() {
        println!("Fuzz directory not found. Initializing cargo fuzz...");
        let status = Command::new("cargo")
            .arg("+nightly")
            .arg("fuzz")
            .arg("init")
            .status()
            .expect("Failed to initialize cargo fuzz");
            
        if !status.success() {
            eprintln!("[ERROR] Failed to initialize cargo fuzz. Do you have nightly rust installed? (rustup toolchain install nightly)");
            return false;
        }
    }
    
    println!("Starting Fuzzer on target '{}'...", target_name);
    println!("Press Ctrl+C to stop the fuzzer.");
    
    let mut child = Command::new("cargo")
        .arg("+nightly")
        .arg("fuzz")
        .arg("run")
        .arg(target_name)
        .spawn()
        .expect("Failed to start cargo-fuzz");

    let status = child.wait().expect("Failed to wait for cargo-fuzz");

    if status.success() {
        println!("\n[SUCCESS] Fuzzing completed normally.");
        true
    } else {
        eprintln!("\n[ERROR] Fuzzing found crashes or was aborted.");
        false
    }
}

pub fn run_sanitizer(test_name: &str, san_type: &str) -> bool {
    println!("Starting Sanitizer '{}' on target '{}'...", san_type, test_name);
    
    #[cfg(target_arch = "aarch64")]
    let target = "aarch64-apple-darwin";
    #[cfg(target_arch = "x86_64")]
    let target = "x86_64-apple-darwin";
    
    // Note: We use a hardcoded fallback here for demonstration if not cross compiling, 
    // but in a real generic tool, we might parse `rustc -vV`.
    
    let z_sanitizer = format!("-Zsanitizer={}", san_type);
    
    let mut child = Command::new("cargo")
        .arg("+nightly")
        .arg("test")
        .arg("-Zbuild-std")
        .arg("--target")
        .arg(target)
        .arg("--test")
        .arg(test_name)
        .env("RUSTFLAGS", &z_sanitizer)
        .spawn()
        .expect("Failed to start sanitizers");

    let status = child.wait().expect("Failed to wait for sanitizer test");

    if status.success() {
        println!("\n[SUCCESS] Sanitizer tests passed! No memory or threading issues found.");
        true
    } else {
        eprintln!("\n[ERROR] Sanitizer tests failed. You might have Data Races (TSAN) or Use-After-Free/Buffer Overflows (ASAN).");
        false
    }
}
