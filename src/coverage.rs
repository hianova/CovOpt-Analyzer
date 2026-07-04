use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct CoverageMap {
    // hit_counts[file_path][line_number] = execution_count
    hit_counts: HashMap<String, HashMap<u64, u64>>,
    // symbol_map[file_path][line_number] = mangled_symbol_name
    symbol_map: HashMap<String, HashMap<u64, String>>,
}

impl CoverageMap {
    /// Parse from llvm-cov LCOV export format
    pub fn from_lcov(lcov_str: &str) -> Result<Self, String> {
        let mut hit_counts: HashMap<String, HashMap<u64, u64>> = HashMap::new();
        let mut symbol_map: HashMap<String, HashMap<u64, String>> = HashMap::new();

        let mut current_file = String::new();
        let mut current_functions: Vec<(u64, String)> = Vec::new();
        let mut current_file_hit_counts: HashMap<u64, u64> = HashMap::new();

        for line in lcov_str.lines() {
            let line = line.trim();
            if let Some(stripped) = line.strip_prefix("SF:") {
                current_file = stripped.to_string();
                current_functions.clear();
                current_file_hit_counts.clear();
            } else if let Some(stripped) = line.strip_prefix("FN:") {
                // FN:<line>,<name>
                let parts: Vec<&str> = stripped.splitn(2, ',').collect();
                if parts.len() == 2
                    && let Ok(line_num) = parts[0].parse::<u64>()
                {
                    current_functions.push((line_num, parts[1].to_string()));
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
            hit_counts,
            symbol_map,
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
            if full_path.ends_with(filename_suffix)
                && let Some(&count) = lines.get(&line_number)
            {
                return Some(count);
            }
        }
        None
    }

    /// Retrieve the function symbol for a specific line by matching the end of the filename.
    pub fn find_symbol(&self, filename_suffix: &str, line_number: u64) -> Option<String> {
        for (full_path, symbols) in &self.symbol_map {
            if full_path.ends_with(filename_suffix)
                && let Some(sym) = symbols.get(&line_number)
            {
                return Some(sym.clone());
            }
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

        // Test coverage calculation
        let (executed, total) = map.get_function_coverage("_dummy_loop_test").unwrap();
        assert_eq!(total, 5); // lines 1, 2, 3, 4, 5
        assert_eq!(executed, 4); // line 2 has 0 hits
    }
}
