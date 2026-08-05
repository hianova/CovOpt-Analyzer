use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Mutex;

pub struct WalArchive {
    filepath: String,
    seen_hashes: Mutex<HashSet<u64>>,
}

impl WalArchive {
    pub fn new(filepath: &str) -> Self {
        let mut seen = HashSet::new();
        // Load existing hashes if file exists
        if let Ok(content) = std::fs::read_to_string(filepath) {
            for line in content.lines() {
                seen.insert(Self::calculate_hash(line.trim()));
            }
        }

        Self {
            filepath: filepath.to_string(),
            seen_hashes: Mutex::new(seen),
        }
    }

    pub fn calculate_hash(data: &str) -> u64 {
        let mut hash: u64 = 0xcbf29ce484222325; // FNV offset basis
        for b in data.bytes() {
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x100000001b3); // FNV prime
        }
        hash
    }

    /// Attempts to append the record. Returns true if successful (new record), false if duplicate.
    pub fn append(&self, record: &str) -> std::io::Result<bool> {
        let hash = Self::calculate_hash(record);

        let mut seen = self.seen_hashes.lock().unwrap();
        if seen.contains(&hash) {
            return Ok(false);
        }

        // Write to WAL
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.filepath)?;

        writeln!(file, "{}", record)?;
        file.sync_data()?;

        seen.insert(hash);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_wal_archive() {
        let path = "test_wal.txt";
        let _ = fs::remove_file(path);

        let archive = WalArchive::new(path);

        // Append new
        assert!(archive.append("data1").unwrap());
        // Append duplicate
        assert!(!archive.append("data1").unwrap());
        // Append new
        assert!(archive.append("data2").unwrap());

        // Check file exists
        assert!(std::path::Path::new(path).exists());

        // Load existing
        let archive2 = WalArchive::new(path);
        assert!(!archive2.append("data1").unwrap());
        assert!(!archive2.append("data2").unwrap());
        assert!(archive2.append("data3").unwrap());

        let _ = fs::remove_file(path);
    }
}
