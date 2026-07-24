use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct AsmExtractor {
    target_dir: PathBuf,
}

impl AsmExtractor {
    pub fn new<P: AsRef<Path>>(target_dir: P) -> Self {
        Self {
            target_dir: target_dir.as_ref().to_path_buf(),
        }
    }

    /// Triggers compilation to generate assembly
    pub fn compile_asm(&self) -> Result<(), String> {
        let status = Command::new("cargo")
            .args(["rustc", "--release", "--", "--emit=asm"])
            .current_dir(&self.target_dir)
            .status()
            .map_err(|e| format!("Failed to run cargo rustc: {}", e))?;

        if !status.success() {
            return Err("cargo rustc --emit=asm failed".to_string());
        }
        Ok(())
    }

    /// Finds and extracts the assembly block for a specific function name
    pub fn extract_function(&self, func_name: &str) -> Result<String, String> {
        let deps_dir = self.target_dir.join("target").join("release").join("deps");
        if !deps_dir.exists() {
            return Err(format!("deps dir not found: {:?}", deps_dir));
        }

        let mut all_s_files = Vec::new();
        for entry in fs::read_dir(&deps_dir)
            .map_err(|e| e.to_string())?
            .flatten()
        {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("s") {
                all_s_files.push(path);
            }
        }

        if all_s_files.is_empty() {
            return Err("No .s files found. Did you call compile_asm()?".to_string());
        }

        // We look for a label that contains our demangled function name
        // Assembly labels usually look like:
        // _ZN15covopt_analyzer...:
        for s_file in all_s_files {
            if let Ok(content) = fs::read_to_string(&s_file) {
                let mut in_target_func = false;
                let mut block = String::new();

                for line in content.lines() {
                    if line.starts_with(".globl\t") || line.starts_with(".type\t") {
                        continue;
                    }

                    if line.ends_with(':')
                        && !line.starts_with('.')
                        && !line.starts_with("L")
                        && !line.starts_with(".L")
                    {
                        let label = line.trim_end_matches(':');
                        let demangled = rustc_demangle::demangle(label).to_string();

                        if demangled.contains(func_name) {
                            in_target_func = true;
                            // Add the label itself
                            block.push_str(line);
                            block.push('\n');
                            continue;
                        } else if in_target_func {
                            // Hit another global function label, stop reading
                            break;
                        }
                    }

                    if in_target_func {
                        block.push_str(line);
                        block.push('\n');
                    }
                }

                if !block.is_empty() {
                    return Ok(block);
                }
            }
        }

        Err(format!(
            "Function '{}' not found in any generated assembly file.",
            func_name
        ))
    }
}
