use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use syn::Item;

#[derive(Debug, Clone)]
pub struct Objective {
    pub path: String,
    pub name: String,
    pub has_new: bool,
    pub tokens: HashMap<String, usize>,
}

pub fn run(src_dir: &str, trait_name: &str, method_name: &str, threshold: f64) {
    println!(
        "🔍 [Explore] Scanning codebase for `{}` implementations in `{}`...",
        trait_name, src_dir
    );

    let mut files = Vec::new();
    find_rs_files(Path::new(src_dir), &mut files);

    let mut objectives = Vec::new();

    for file in files {
        let content = fs::read_to_string(&file).unwrap_or_default();
        if !content.contains(trait_name) {
            continue;
        }

        if let Ok(ast) = syn::parse_file(&content) {
            for item in ast.items {
                if let Item::Impl(impl_item) = item {
                    if let Some((_, path, _)) = &impl_item.trait_ {
                        if path.segments.last().map(|s| s.ident.to_string())
                            == Some(trait_name.to_string())
                        {
                            let struct_name =
                                if let syn::Type::Path(type_path) = &*impl_item.self_ty {
                                    type_path
                                        .path
                                        .segments
                                        .last()
                                        .map(|s| s.ident.to_string())
                                        .unwrap_or_default()
                                } else {
                                    continue;
                                };

                            let mut method_tokens = HashMap::new();
                            let mut found_method = false;

                            for impl_inner in &impl_item.items {
                                if let syn::ImplItem::Fn(method) = impl_inner {
                                    if method.sig.ident.to_string() == method_name {
                                        found_method = true;
                                        // Tokenize the method body
                                        let body_str = quote::quote!(#method).to_string();
                                        let tokens = get_tokens(&body_str);
                                        for t in tokens {
                                            *method_tokens.entry(t).or_insert(0) += 1;
                                        }
                                    }
                                }
                            }

                            if found_method {
                                // Check if there is a 'new' method in the AST (rough check)
                                let has_new = content.contains(&format!("fn new("));

                                // Determine module path
                                let rel_path = file.strip_prefix(src_dir).unwrap_or(&file);
                                let mod_path = rel_path
                                    .with_extension("")
                                    .iter()
                                    .map(|s| s.to_string_lossy().to_string())
                                    .collect::<Vec<_>>()
                                    .join("::");

                                objectives.push(Objective {
                                    path: format!("crate::{}::{}", mod_path, struct_name),
                                    name: struct_name,
                                    has_new,
                                    tokens: method_tokens,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // Deduplicate
    let mut unique_obj = Vec::new();
    let mut seen = HashSet::new();
    for obj in objectives {
        let key = format!("{}::{}", obj.path, obj.name);
        if seen.insert(key) {
            unique_obj.push(obj);
        }
    }

    println!(
        "✅ Found {} valid {} objectives!",
        unique_obj.len(),
        trait_name
    );
    println!("🧠 Computing Cosine Similarity Matrix (P/NP Projection)...");

    let mut perfect_pairs = Vec::new();
    let n = unique_obj.len();
    for i in 0..n {
        for j in (i + 1)..n {
            let sim = cosine_similarity(&unique_obj[i].tokens, &unique_obj[j].tokens);
            if sim >= threshold {
                perfect_pairs.push((sim, unique_obj[i].clone(), unique_obj[j].clone()));
            }
        }
    }

    println!(
        "✅ Projected {} PERFECT pairs (Similarity >= {})!",
        perfect_pairs.len(),
        threshold
    );

    if perfect_pairs.is_empty() {
        println!("⚠️ No resonances found. Exiting.");
        return;
    }

    println!(
        "🚀 [Mega-Batch Engine] Compiling single Mega-Batch of {} pairs...",
        perfect_pairs.len()
    );
    let harness = generate_harness(&perfect_pairs);
    let target_root = Path::new(src_dir).parent().unwrap_or(Path::new("."));
    let tests_dir = target_root.join("tests");
    let _ = fs::create_dir_all(&tests_dir);
    let harness_path = tests_dir.join("covopt_explore_harness.rs");
    let _ = fs::write(&harness_path, harness);

    println!(
        "⏳ Rust is now compiling the Mega-Batch Harness in {:?}. This may take a few minutes...",
        target_root
    );

    let start_time = std::time::Instant::now();
    let output = Command::new("cargo")
        .current_dir(target_root)
        .args(&[
            "test",
            "--test",
            "covopt_explore_harness",
            "--release",
            "--",
            "--nocapture",
        ])
        .output();

    let elapsed = start_time.elapsed();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            if !out.status.success() {
                println!(
                    "❌ Harness compilation or runtime failed! (Took {:.1?}s)",
                    elapsed
                );
                println!("   [Stderr]: {}", stderr);
            } else {
                let successes = stdout.matches("PARETO-FRONT RESONANCE REACHED").count();
                println!("✅ Mega-Batch execution completed in {:.1?}s.", elapsed);
                println!("🏆 Found {} Pareto-Front Resonances!", successes);

                for line in stdout.lines() {
                    if line.contains("PARETO-FRONT") {
                        println!("   {}", line);
                    }
                }
            }
        }
        Err(e) => {
            println!("❌ Failed to execute cargo test: {}", e);
        }
    }
}

fn find_rs_files(dir: &Path, files: &mut Vec<PathBuf>) {
    if dir.is_dir() {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    find_rs_files(&path, files);
                } else if path.extension().map(|s| s == "rs").unwrap_or(false)
                    && path.file_name().unwrap() != "mod.rs"
                {
                    files.push(path);
                }
            }
        }
    }
}

fn get_tokens(text: &str) -> Vec<String> {
    let re = regex::Regex::new(r"[A-Za-z0-9]+").unwrap();
    re.find_iter(text)
        .map(|m| m.as_str().to_lowercase())
        .collect()
}

fn cosine_similarity(counter1: &HashMap<String, usize>, counter2: &HashMap<String, usize>) -> f64 {
    let mut terms = HashSet::new();
    for k in counter1.keys() {
        terms.insert(k.clone());
    }
    for k in counter2.keys() {
        terms.insert(k.clone());
    }

    let mut dotprod = 0.0;
    for k in &terms {
        dotprod +=
            (*counter1.get(k).unwrap_or(&0) as f64) * (*counter2.get(k).unwrap_or(&0) as f64);
    }

    let mag_a = counter1
        .values()
        .map(|&v| (v as f64).powi(2))
        .sum::<f64>()
        .sqrt();
    let mag_b = counter2
        .values()
        .map(|&v| (v as f64).powi(2))
        .sum::<f64>()
        .sqrt();

    if mag_a * mag_b == 0.0 {
        return 0.0;
    }
    dotprod / (mag_a * mag_b)
}

fn generate_harness(pairs: &[(f64, Objective, Objective)]) -> String {
    let mut uses = HashSet::new();
    let mut invocations = Vec::new();

    for (_, a, b) in pairs {
        uses.insert(format!("use {};", a.path));
        uses.insert(format!("use {};", b.path));

        let inst_a = if a.has_new {
            format!("{}::new()", a.name)
        } else {
            a.name.clone()
        };
        let inst_b = if b.has_new {
            format!("{}::new()", b.name)
        } else {
            b.name.clone()
        };

        let name_a = a
            .name
            .replace("Objective", "")
            .replace("Macro", "")
            .replace("Micro", "");
        let name_b = b
            .name
            .replace("Objective", "")
            .replace("Macro", "")
            .replace("Micro", "");

        invocations.push(format!("    crate::generate_cross_pollination!(({inst_a}, \"{name_a}\", {inst_b}, \"{name_b}\"));"));
    }

    let uses_str = uses.into_iter().collect::<Vec<_>>().join("\n");
    let invocations_str = invocations.join("\n");

    format!(
        r#"
#[cfg(test)]
mod tests {{
    use super::*;
    {}

    #[test]
    fn run_mega_batch() {{
        println!("🚀 Running Mega-Batch P/NP Resonance test...");
{}
    }}
}}
"#,
        uses_str, invocations_str
    )
}
