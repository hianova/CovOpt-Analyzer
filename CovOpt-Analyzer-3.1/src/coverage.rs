use covopt_macro::covopt_param;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCoverage {
    pub file: String,
    pub name: String,
    pub start_line: u64,
    pub end_line: u64,
    pub execution_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchCoverage {
    pub file: String,
    pub line: u64,
    pub block: String,
    pub branch: String,
    pub taken: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CoverageMap {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    // hit_counts[file_path][line_number] = execution_count
    pub hit_counts: HashMap<String, HashMap<u64, u64>>,
    // symbol_map[file_path][line_number] = mangled_symbol_name
    pub symbol_map: HashMap<String, HashMap<u64, String>>,
    /// Exact LCOV function records, including execution counts and regions.
    #[serde(default)]
    pub functions: Vec<FunctionCoverage>,
    /// Branch records retain taken/not-taken information.
    #[serde(default)]
    pub branches: Vec<BranchCoverage>,
}

fn default_schema_version() -> u32 {
    crate::model::MODEL_SCHEMA_VERSION
}

impl CoverageMap {
    /// Parse from llvm-cov LCOV export format
    pub fn from_lcov(lcov_str: &str) -> Result<Self, String> {
        let mut hit_counts: HashMap<String, HashMap<u64, u64>> = HashMap::new();
        let mut symbol_map: HashMap<String, HashMap<u64, String>> = HashMap::new();

        let mut current_file = String::new();
        let mut current_functions: Vec<(u64, String)> = Vec::new();
        let mut current_function_hits: HashMap<String, u64> = HashMap::new();
        let mut functions = Vec::new();
        let mut branches = Vec::new();
        let mut current_file_hit_counts: HashMap<u64, u64> = HashMap::new();

        for line in lcov_str.lines() {
            let line = line.trim();
            if let Some(stripped) = line.strip_prefix("SF:") {
                current_file = stripped.to_string();
                current_functions.clear();
                current_function_hits.clear();
                current_file_hit_counts.clear();
            } else if let Some(stripped) = line.strip_prefix("FN:") {
                // FN:<line>,<name>
                let parts: Vec<&str> = stripped.splitn(2, ',').collect();
                if parts.len() == 2
                    && let Ok(line_num) = parts[0].parse::<u64>()
                {
                    current_functions.push((line_num, parts[1].to_string()));
                }
            } else if let Some(stripped) = line.strip_prefix("FNDA:") {
                // FNDA:<execution-count>,<name>
                let parts: Vec<&str> = stripped.splitn(2, ',').collect();
                if parts.len() == 2
                    && let Ok(hits) = parts[0].parse::<u64>()
                {
                    current_function_hits.insert(parts[1].to_string(), hits);
                }
            } else if let Some(stripped) = line.strip_prefix("BRDA:") {
                // BRDA:<line>,<block>,<branch>,<taken|->
                let parts = stripped.splitn(4, ',').collect::<Vec<_>>();
                if parts.len() == 4
                    && let Ok(line_number) = parts[0].parse::<u64>()
                {
                    branches.push(BranchCoverage {
                        file: current_file.clone(),
                        line: line_number,
                        block: parts[1].to_string(),
                        branch: parts[2].to_string(),
                        taken: (parts[3] != "-").then(|| parts[3].parse::<u64>().unwrap_or(0)),
                    });
                }
            } else if let Some(stripped) = line.strip_prefix("DA:") {
                // DA:<line>,<hits>
                let parts: Vec<&str> = stripped.splitn(2, ',').collect();
                if parts.len() == 2
                    && let (Ok(line_num), Ok(hits)) =
                        (parts[0].parse::<u64>(), parts[1].parse::<u64>())
                {
                    current_file_hit_counts.insert(line_num, hits);
                }
            } else if line == "end_of_record" && !current_file.is_empty() {
                // Sort functions by start line
                current_functions.sort_by_key(|k| k.0);

                for (index, (start_line, name)) in current_functions.iter().enumerate() {
                    let end_line = current_functions.get(index + 1).map_or_else(
                        || {
                            current_file_hit_counts
                                .keys()
                                .max()
                                .copied()
                                .unwrap_or(*start_line)
                        },
                        |(next_start, _)| next_start.saturating_sub(1),
                    );
                    functions.push(FunctionCoverage {
                        file: current_file.clone(),
                        name: name.clone(),
                        start_line: *start_line,
                        end_line: end_line.max(*start_line),
                        execution_count: current_function_hits.get(name).copied().unwrap_or(0),
                    });
                }

                let symbol_file_map = symbol_map.entry(current_file.clone()).or_default();
                let hit_file_map = hit_counts.entry(current_file.clone()).or_default();

                for (line_num, hits) in &current_file_hit_counts {
                    hit_file_map.insert(*line_num, *hits);

                    // Find the function this line belongs to (largest start line <= line_num)
                    let mut func_name = None;
                    for (start_line, name) in current_functions.iter().rev() {
                        if *start_line <= *line_num {
                            func_name = Some(name.clone());
                            break;
                        }
                    }
                    if let Some(name) = func_name {
                        symbol_file_map.insert(*line_num, name);
                    }
                }
            }
        }

        Ok(Self {
            schema_version: crate::model::MODEL_SCHEMA_VERSION,
            hit_counts,
            symbol_map,
            functions,
            branches,
        })
    }

    /// Get the hit count for a specific file and line number.
    pub fn get_hit_count(&self, file_path: &str, line_number: u64) -> Option<u64> {
        self.hit_counts
            .get(file_path)
            .and_then(|file_map| file_map.get(&line_number).copied())
    }

    /// Retrieve the hit count for a specific line by matching the end of the filename.
    pub fn find_hit_count(&self, filename_suffix: &str, line_number: u64) -> Option<u64> {
        for (full_path, lines) in &self.hit_counts {
            if full_path.ends_with(filename_suffix) {
                let mut current_line = line_number;
                while current_line > 0 {
                    if let Some(&count) = lines.get(&current_line) {
                        return Some(count);
                    }
                    current_line -= 1;
                    if line_number - current_line > covopt_param!("M_92_52", 20) {
                        break;
                    }
                }
            }
        }
        None
    }

    /// Retrieve the function symbol for a specific line by matching the end of the filename.
    pub fn find_symbol(&self, filename_suffix: &str, line_number: u64) -> Option<String> {
        for (full_path, symbols) in &self.symbol_map {
            if full_path.ends_with(filename_suffix) {
                // Try to find exact match or nearest preceding line
                let mut current_line = line_number;
                while current_line > 0 {
                    if let Some(sym) = symbols.get(&current_line) {
                        return Some(sym.clone());
                    }
                    current_line -= 1;
                    // Don't search too far back
                    if line_number - current_line > covopt_param!("M_113_52", 20) {
                        break;
                    }
                }
            }
        }
        None
    }

    /// Finds the location (file, line, symbol, hits) with the maximum hit count across all files.
    /// If `target_fn_name` is provided, restricts the search to symbols containing that name.
    pub fn find_peak_location(
        &self,
        ignore_patterns: &[String],
        target_fn_name: Option<&str>,
    ) -> Option<(String, u64, String, u64)> {
        let mut candidates: Vec<(&String, u64, Option<&String>, u64)> = Vec::new();

        for (file, file_hits) in &self.hit_counts {
            for (line, &hits) in file_hits {
                let sym_opt = self.symbol_map.get(file).and_then(|m| m.get(line));

                candidates.push((file, *line, sym_opt, hits));
            }
        }

        candidates.sort_by_key(|b| std::cmp::Reverse(b.3));
        let unknown_sym = "unknown".to_string();

        for (file, line, sym_opt, hits) in candidates {
            let sym_str = sym_opt.unwrap_or(&unknown_sym);
            let demangled = rustc_demangle::demangle(sym_str).to_string();

            if let Some(target_fn) = target_fn_name
                && !demangled.contains(target_fn)
            {
                continue;
            }

            if demangled.contains("unlikely")
                || demangled.contains("likely")
                || demangled.contains("black_box")
                || demangled.contains("ignore")
                || ignore_patterns.iter().any(|pat| demangled.contains(pat))
            {
                continue;
            }

            // Read the source file to check for AST-level #[covopt::ignore] or #![cfg_attr(covopt, ignore)]
            if let Ok(source) = std::fs::read_to_string(file) {
                let lines: Vec<&str> = source.lines().collect();
                let start_idx = line.saturating_sub(covopt_param!("M_160_52", 20)) as usize;
                let end_idx = (line as usize).min(lines.len());
                let mut should_ignore = false;
                for i in start_idx..end_idx {
                    if let Some(src_line) = lines.get(i)
                        && (src_line.contains("covopt::ignore")
                            || src_line.contains("cfg_attr(covopt, ignore)"))
                    {
                        should_ignore = true;
                        break;
                    }
                }
                if should_ignore {
                    continue;
                }
            }

            return Some((file.clone(), line, sym_str.clone(), hits));
        }

        None
    }

    /// Calculate the coverage rate for a specific function globally.
    /// Returns (executed_lines, total_lines).
    pub fn get_function_coverage(&self, function_name: &str) -> Option<(u64, u64)> {
        let mut executed = 0;
        let mut total = 0;
        let mut found = false;

        for (full_path, symbols) in &self.symbol_map {
            if let Some(hit_file_map) = self.hit_counts.get(full_path) {
                for (line_num, sym) in symbols {
                    if sym == function_name || sym.contains(function_name) {
                        found = true;
                        if let Some(&hits) = hit_file_map.get(line_num) {
                            total += 1;
                            if hits > 0 {
                                executed += 1;
                            }
                        }
                    }
                }
            }
        }

        if found && total > 0 {
            Some((executed, total))
        } else {
            None
        }
    }

    pub fn function_record(
        &self,
        filename_suffix: &str,
        function_name: &str,
    ) -> Option<&FunctionCoverage> {
        self.functions.iter().find(|record| {
            path_matches(&record.file, filename_suffix)
                && (record.name == function_name || record.name.contains(function_name))
        })
    }

    pub fn branches_for(
        &self,
        filename_suffix: &str,
        start_line: u64,
        end_line: u64,
    ) -> Vec<&BranchCoverage> {
        self.branches
            .iter()
            .filter(|branch| {
                path_matches(&branch.file, filename_suffix)
                    && branch.line >= start_line
                    && branch.line <= end_line
            })
            .collect()
    }
}

fn path_matches(actual: &str, requested: &str) -> bool {
    let requested = requested.trim_start_matches("./");
    actual.ends_with(requested)
        || actual
            .replace('\\', "/")
            .ends_with(&requested.replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coverage_map_parsing_lcov() {
        let lcov_data = "\
TN:
SF:/src/dummy.rs
FN:1,_dummy_loop_test
FNDA:10,_dummy_loop_test
FNF:1
FNH:1
DA:1,1
DA:2,0
DA:3,10
DA:4,10
DA:5,10
end_of_record
";

        let map = CoverageMap::from_lcov(lcov_data).expect("Failed to parse LCOV");

        // The function starts at line 1
        assert_eq!(map.get_hit_count("/src/dummy.rs", 1), Some(1));
        assert_eq!(map.get_hit_count("/src/dummy.rs", 2), Some(0));
        assert_eq!(map.get_hit_count("/src/dummy.rs", 3), Some(10));
        assert_eq!(map.get_hit_count("/src/dummy.rs", 4), Some(10));
        assert_eq!(map.get_hit_count("/src/dummy.rs", 5), Some(10));

        // Missing line should be None
        assert_eq!(map.get_hit_count("/src/dummy.rs", 6), None);

        // Test symbol mapping
        assert_eq!(
            map.find_symbol("dummy.rs", 3),
            Some("_dummy_loop_test".to_string())
        );
        let function = map.function_record("dummy.rs", "_dummy_loop_test").unwrap();
        assert_eq!(function.execution_count, 10);
        assert_eq!(function.start_line, 1);

        // Test coverage calculation
        let (executed, total) = map.get_function_coverage("_dummy_loop_test").unwrap();
        assert_eq!(total, 5); // lines 1, 2, 3, 4, 5
        assert_eq!(executed, 4); // line 2 has 0 hits
    }
}
