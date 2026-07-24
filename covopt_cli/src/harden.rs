use covopt_macro::covopt_param;
use std::path::Path;
use std::process::Command;

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
    println!(
        "cargo-mutants will intentionally inject bugs into your code to see if your tests catch them."
    );

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
        eprintln!(
            "\n[ERROR] Mutation testing failed. Some injected bugs survived, meaning your tests need stricter assertions."
        );
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
            eprintln!(
                "[ERROR] Failed to initialize cargo fuzz. Do you have nightly rust installed? (rustup toolchain install nightly)"
            );
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

fn find_target_file(asan_log: &str) -> Option<(String, usize)> {
    for line in asan_log.lines() {
        if let Some(pos) = line.find(".rs:") {
            let chars: Vec<char> = line[..pos].chars().collect();
            let mut start_idx = 0;
            for i in (0..chars.len()).rev() {
                if chars[i] == ' '
                    || chars[i] == '\t'
                    || chars[i] == '"'
                    || chars[i] == '\''
                    || (chars[i] == '/' && i > 0 && chars[i - 1] == ' ')
                {
                    start_idx = i + 1;
                    break;
                }
            }
            let path_str = line[start_idx..pos + covopt_param!("M_106_49", 3)].trim();

            let remaining = &line[pos + covopt_param!("M_108_40", 4)..];
            let mut end_idx = 0;
            for (i, c) in remaining.char_indices() {
                if c.is_ascii_digit() {
                    end_idx = i + 1;
                } else {
                    break;
                }
            }
            if end_idx > 0
                && let Ok(line_num) = remaining[..end_idx].parse::<usize>()
            {
                let path = std::path::Path::new(path_str);
                if path.exists() {
                    if let Ok(path_canon) = path.canonicalize() {
                        let path_str_canon = path_canon.to_string_lossy().to_string();
                        if !path_str_canon.contains(".cargo")
                            && !path_str_canon.contains(".rustup")
                            && !path_str_canon.contains("/rustc/")
                        {
                            return Some((path_str_canon, line_num));
                        }
                    }
                } else {
                    for prefix in &["", "src/", "tests/"] {
                        let test_path = std::path::Path::new(prefix).join(path);
                        if test_path.exists()
                            && let Ok(path_canon) = test_path.canonicalize()
                        {
                            let path_str_canon = path_canon.to_string_lossy().to_string();
                            if !path_str_canon.contains(".cargo")
                                && !path_str_canon.contains(".rustup")
                                && !path_str_canon.contains("/rustc/")
                            {
                                return Some((path_str_canon, line_num));
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

fn call_llm(prompt: &str) -> Option<String> {
    let (url, body, auth_header) = if let Ok(key) = std::env::var("GEMINI_API_KEY") {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:generateContent?key={}",
            key
        );
        let body = serde_json::json!({
            "contents": [
                {
                    "parts": [
                        {
                            "text": prompt
                        }
                    ]
                }
            ],
            "generationConfig": {
                "temperature": 0.1
            }
        });
        (url, body, None)
    } else {
        let endpoint = std::env::var("COVOPT_LLM_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:11434/v1/chat/completions".to_string());
        let model =
            std::env::var("COVOPT_LLM_MODEL").unwrap_or_else(|_| "qwen-1.7b-instruct".to_string());
        let api_key = std::env::var("COVOPT_LLM_API_KEY").ok();

        let body = serde_json::json!({
            "model": model,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "temperature": 0.1
        });
        let auth = api_key.map(|k| format!("Bearer {}", k));
        (endpoint, body, auth)
    };

    let body_str = body.to_string();

    let mut curl = Command::new("curl");
    curl.arg("-s")
        .arg("-X")
        .arg("POST")
        .arg("-H")
        .arg("Content-Type: application/json")
        .arg("-d")
        .arg(&body_str);

    if let Some(ref auth) = auth_header {
        curl.arg("-H").arg(auth);
    }
    curl.arg(&url);

    let output = curl.output().ok()?;
    if output.status.success() {
        let stdout_str = String::from_utf8_lossy(&output.stdout);
        let res: serde_json::Value = serde_json::from_str(&stdout_str).ok()?;

        if let Some(text) = res["candidates"][0]["content"]["parts"][0]["text"].as_str() {
            return Some(text.to_string());
        }
        if let Some(text) = res["choices"][0]["message"]["content"].as_str() {
            return Some(text.to_string());
        }
    }
    None
}

fn extract_code(llm_output: &str) -> Option<String> {
    if let Some(start) = llm_output.find("```rust") {
        let after_start = &llm_output[start + covopt_param!("M_228_46", 7)..];
        if let Some(end) = after_start.find("```") {
            return Some(after_start[..end].to_string());
        }
    }
    if let Some(start) = llm_output.find("```") {
        let after_start = &llm_output[start + covopt_param!("M_234_46", 3)..];
        let mut code_start = 0;
        if let Some(nl) = after_start.find('\n') {
            let tag = after_start[..nl].trim();
            if tag.chars().all(|c| c.is_alphabetic()) {
                code_start = nl + 1;
            }
        }
        let code_content = &after_start[code_start..];
        if let Some(end) = code_content.find("```") {
            return Some(code_content[..end].to_string());
        }
    }
    None
}

pub fn run_sanitizer(test_name: &str, san_type: &str, auto_fix: bool) -> bool {
    let max_fix_attempts = if auto_fix {
        covopt_param!("M_251_41", 3)
    } else {
        1
    };
    let mut attempt = 0;

    while attempt < max_fix_attempts {
        attempt += 1;
        if attempt > 1 {
            println!(
                "\n--- [Auto-Fix] Verification Attempt {}/{} ---",
                attempt, max_fix_attempts
            );
        } else {
            println!(
                "Starting Sanitizer '{}' on target '{}'...",
                san_type, test_name
            );
        }

        let arch = std::env::consts::ARCH;
        let os = std::env::consts::OS;
        
        let target = match (arch, os) {
            ("aarch64", "macos") => "aarch64-apple-darwin",
            ("x86_64", "macos") => "x86_64-apple-darwin",
            ("x86_64", "linux") => "x86_64-unknown-linux-gnu",
            ("aarch64", "linux") => "aarch64-unknown-linux-gnu",
            ("x86_64", "windows") => {
                if san_type == "thread" {
                    eprintln!("ThreadSanitizer is currently unsupported on Windows target.");
                    return false;
                }
                "x86_64-pc-windows-msvc"
            }
            _ => {
                eprintln!("Unsupported architecture or OS for sanitizers: {}-{}", arch, os);
                return false;
            }
        };

        let z_sanitizer = format!("-Zsanitizer={}", san_type);

        let mut cmd = Command::new("cargo");
        cmd.arg("+nightly")
            .arg("test")
            .arg("-Zbuild-std")
            .arg("--target")
            .arg(target)
            .arg("--test")
            .arg(test_name)
            .env("RUSTFLAGS", &z_sanitizer);

        let success = if auto_fix {
            let output = cmd.output().expect("Failed to run sanitizer test");
            let stdout_str = String::from_utf8_lossy(&output.stdout);
            let stderr_str = String::from_utf8_lossy(&output.stderr);

            if output.status.success() {
                println!("{}", stdout_str);
                true
            } else {
                eprintln!("{}", stderr_str);

                let full_log = format!("{}\n{}", stdout_str, stderr_str);
                if let Some((filepath, line_num)) = find_target_file(&full_log) {
                    println!(
                        "\n[Auto-Fix] Sanitizer detected crash in {} at line {}",
                        filepath, line_num
                    );
                    if let Ok(content) = std::fs::read_to_string(&filepath) {
                        let prompt = format!(
                            "You are an expert Rust systems programmer. Fix the memory safety bug (Use-After-Free, Double Free, Data Race, or Out-of-bounds) in the following Rust code. \
                             The code failed AddressSanitizer/ThreadSanitizer checks.\n\n\
                             Sanitizer Diagnostics Log:\n{}\n\n\
                             Source File: {}\n\
                             Source Code:\n\
                             ```rust\n{}\n```\n\n\
                             Please output the complete fixed file content inside a single ```rust block. Do not include any other text, warnings, or explanations.",
                            full_log, filepath, content
                        );

                        println!("[Auto-Fix] Querying LLM to repair memory safety issue...");
                        if let Some(response) = call_llm(&prompt) {
                            if let Some(fixed_code) = extract_code(&response) {
                                println!(
                                    "[Auto-Fix] Received fix suggestion from LLM. Overwriting {}...",
                                    filepath
                                );
                                if std::fs::write(&filepath, fixed_code).is_ok() {
                                    continue;
                                }
                            } else {
                                println!(
                                    "[Auto-Fix] Could not extract valid code from LLM response."
                                );
                            }
                        } else {
                            println!(
                                "[Auto-Fix] Failed to get response from LLM API (make sure GEMINI_API_KEY is set or local LLM server is running)."
                            );
                        }
                    }
                } else {
                    println!(
                        "[Auto-Fix] Could not resolve offending file path from sanitizer logs."
                    );
                }
                false
            }
        } else {
            let mut child = cmd.spawn().expect("Failed to start sanitizers");
            let status = child.wait().expect("Failed to wait for sanitizer test");
            status.success()
        };

        if success {
            println!("\n[SUCCESS] Sanitizer tests passed! No memory or threading issues found.");
            return true;
        } else {
            break;
        }
    }

    eprintln!(
        "\n[ERROR] Sanitizer tests failed. You might have Data Races (TSAN) or Use-After-Free/Buffer Overflows (ASAN)."
    );
    false
}
