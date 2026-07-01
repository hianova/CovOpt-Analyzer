use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct CoverageExport {
    pub version: String,
    pub data: Vec<CoverageData>,
}

#[derive(Debug, Deserialize)]
pub struct CoverageData {
    pub files: Vec<CoverageFile>,
    pub functions: Vec<CoverageFunction>,
}

#[derive(Debug, Deserialize)]
pub struct CoverageFile {
    pub filename: String,
}

#[derive(Debug, Deserialize)]
pub struct CoverageFunction {
    pub name: String,
    pub filenames: Vec<String>,
    // [line_start, col_start, line_end, col_end, count, file_id, exp_file_id, kind]
    pub regions: Vec<Vec<u64>>, 
}

#[derive(Debug, Default)]
pub struct CoverageMap {
    // hit_counts[file_path][line_number] = execution_count
    hit_counts: HashMap<String, HashMap<u64, u64>>,
}

impl CoverageMap {
    /// Parse from llvm-cov JSON export format
    pub fn from_json(json_str: &str) -> Result<Self, serde_json::Error> {
        let export: CoverageExport = serde_json::from_str(json_str)?;
        let mut hit_counts: HashMap<String, HashMap<u64, u64>> = HashMap::new();

        for data in export.data {
            for func in data.functions {
                let filenames = &func.filenames;

                for region in func.regions {
                    if region.len() < 6 {
                        continue; // Not a valid region format
                    }

                    let line_start = region[0];
                    let line_end = region[2];
                    let count = region[4];
                    let file_id = region[5] as usize;

                    if let Some(filename) = filenames.get(file_id) {
                        let file_map = hit_counts
                            .entry(filename.clone())
                            .or_default();

                        for line in line_start..=line_end {
                            // Take the max hit count if multiple regions overlap
                            let current_count = file_map.entry(line).or_insert(0);
                            if count > *current_count {
                                *current_count = count;
                            }
                        }
                    }
                }
            }
        }

        Ok(Self { hit_counts })
    }

    /// Get the hit count for a specific file and line number.
    pub fn get_hit_count(&self, file_path: &str, line_number: u64) -> Option<u64> {
        self.hit_counts
            .get(file_path)
            .and_then(|file_map| file_map.get(&line_number).copied())
    }

    /// Retrieve the hit count for a specific line by matching the end of the filename.
    /// This is useful because llvm-cov returns absolute paths.
    pub fn find_hit_count(&self, filename_suffix: &str, line_number: u64) -> Option<u64> {
        for (full_path, lines) in &self.hit_counts {
            if full_path.ends_with(filename_suffix)
                && let Some(&count) = lines.get(&line_number) {
                    return Some(count);
                }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coverage_map_parsing() {
        let json_data = r#"{
            "type": "llvm.coverage.json.export",
            "version": "3.1.0",
            "data": [
                {
                    "files": [
                        { "filename": "/src/dummy.rs" }
                    ],
                    "functions": [
                        {
                            "name": "loop_test",
                            "filenames": ["/src/dummy.rs"],
                            "regions": [
                                [1, 1, 1, 23, 1, 0, 0, 0],
                                [3, 9, 3, 10, 10, 0, 0, 0],
                                [3, 19, 5, 6, 10, 0, 0, 0]
                            ]
                        }
                    ]
                }
            ]
        }"#;

        let map = CoverageMap::from_json(json_data).expect("Failed to parse JSON");

        // The function executes 1 time
        assert_eq!(map.get_hit_count("/src/dummy.rs", 1), Some(1));
        
        // Uncovered line should return None since it's not in regions
        assert_eq!(map.get_hit_count("/src/dummy.rs", 2), None);

        // Loop body executes 10 times
        assert_eq!(map.get_hit_count("/src/dummy.rs", 3), Some(10));
        assert_eq!(map.get_hit_count("/src/dummy.rs", 4), Some(10));
        assert_eq!(map.get_hit_count("/src/dummy.rs", 5), Some(10));
        
        // Missing line should be None
        assert_eq!(map.get_hit_count("/src/dummy.rs", 6), None);
    }

    #[test]
    fn test_invalid_json() {
        let result = CoverageMap::from_json("{ invalid json }");
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_region() {
        let json_data = r#"{
            "type": "llvm.coverage.json.export",
            "version": "3.1.0",
            "data": [
                {
                    "files": [ { "filename": "/src/dummy.rs" } ],
                    "functions": [
                        {
                            "name": "loop_test",
                            "filenames": ["/src/dummy.rs"],
                            "regions": [
                                [1, 1, 1, 23, 1]
                            ]
                        }
                    ]
                }
            ]
        }"#;

        let map = CoverageMap::from_json(json_data).expect("Failed to parse JSON");
        assert_eq!(map.get_hit_count("/src/dummy.rs", 1), None); // Region too short, skipped
    }

    #[test]
    fn test_invalid_file_id() {
        let json_data = r#"{
            "type": "llvm.coverage.json.export",
            "version": "3.1.0",
            "data": [
                {
                    "files": [ { "filename": "/src/dummy.rs" } ],
                    "functions": [
                        {
                            "name": "loop_test",
                            "filenames": ["/src/dummy.rs"],
                            "regions": [
                                [1, 1, 1, 23, 1, 999, 0, 0]
                            ]
                        }
                    ]
                }
            ]
        }"#;
        let map = CoverageMap::from_json(json_data).unwrap();
        assert_eq!(map.get_hit_count("/src/dummy.rs", 1), None);
    }

    #[test]
    fn test_json_complexity() {
        let n: usize = std::env::var("COVOPT_N").unwrap_or_else(|_| "10".to_string()).parse().unwrap();
        let mut regions = String::new();
        for i in 0..n {
            regions.push_str("[1, 1, 1, 23, 1, 0, 0, 0]");
            if i < n - 1 {
                regions.push(',');
            }
        }
        let json_data = format!(r#"{{
            "type": "llvm.coverage.json.export",
            "version": "3.1.0",
            "data": [
                {{
                    "files": [ {{ "filename": "/src/dummy.rs" }} ],
                    "functions": [
                        {{
                            "name": "loop_test",
                            "filenames": ["/src/dummy.rs"],
                            "regions": [{}]
                        }}
                    ]
                }}
            ]
        }}"#, regions);
        
        let _map = CoverageMap::from_json(&json_data).unwrap();
    }
}
