use crate::mca::McaReport;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::hash::{DefaultHasher, Hasher};
use std::path::Path;

#[derive(Serialize, Deserialize, Default)]
pub struct AdviseCache {
    pub file_hash: u64,
    pub mca_reports: HashMap<String, McaReport>, // target_symbol -> McaReport
}

#[derive(Serialize, Deserialize, Default)]
pub struct ProjectCache {
    pub files: HashMap<String, AdviseCache>, // file_path -> AdviseCache
}

impl ProjectCache {
    fn cache_path() -> std::path::PathBuf {
        let mut p = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        p.push(".covopt");
        if !p.exists() {
            let _ = fs::create_dir_all(&p);
        }
        p.push("advise_cache.json");
        p
    }

    pub fn load() -> Self {
        if let Ok(content) = fs::read_to_string(Self::cache_path()) {
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    pub fn save(&self) {
        if let Ok(content) = serde_json::to_string_pretty(self) {
            let _ = fs::write(Self::cache_path(), content);
        }
    }
}

pub fn compute_file_hash(path: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    if let Ok(content) = fs::read(path) {
        hasher.write(&content);
    }
    hasher.finish()
}

pub fn save_mca_cache(file_path: &Path, symbol: &str, report: &McaReport) {
    let file_key = file_path.canonicalize().unwrap_or_else(|_| file_path.to_path_buf()).to_string_lossy().into_owned();
    let hash = compute_file_hash(file_path);
    let mut proj_cache = ProjectCache::load();
    let file_cache = proj_cache.files.entry(file_key).or_insert_with(|| AdviseCache {
        file_hash: hash,
        mca_reports: HashMap::new(),
    });
    // If the file changed, invalidate old reports
    if file_cache.file_hash != hash {
        file_cache.file_hash = hash;
        file_cache.mca_reports.clear();
    }
    file_cache.mca_reports.insert(symbol.to_string(), report.clone());
    proj_cache.save();
}

pub fn load_mca_cache(file_path: &Path, symbol: &str) -> Option<McaReport> {
    let file_key = file_path.canonicalize().unwrap_or_else(|_| file_path.to_path_buf()).to_string_lossy().into_owned();
    let hash = compute_file_hash(file_path);
    let proj_cache = ProjectCache::load();
    if let Some(file_cache) = proj_cache.files.get(&file_key)
        && file_cache.file_hash == hash {
            return file_cache.mca_reports.get(symbol).cloned();
        }
    None
}

pub fn is_file_cache_valid(file_path: &Path) -> bool {
    let file_key = file_path.canonicalize().unwrap_or_else(|_| file_path.to_path_buf()).to_string_lossy().into_owned();
    let hash = compute_file_hash(file_path);
    let proj_cache = ProjectCache::load();
    if let Some(file_cache) = proj_cache.files.get(&file_key) {
        file_cache.file_hash == hash && !file_cache.mca_reports.is_empty()
    } else {
        false
    }
}
