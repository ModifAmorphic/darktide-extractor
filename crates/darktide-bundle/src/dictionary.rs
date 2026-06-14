/// MurmurHash64A reverse-lookup dictionary for naming extracted files.
///
/// Scans decompressed bundle content for plaintext path strings, computes
/// their hashes, and builds a hash → path mapping for file name resolution.
use std::collections::HashMap;
use std::fs;

use crate::error::Result;
use crate::hash::murmur_hash64;

/// A dictionary mapping MurmurHash64A hashes to file paths.
///
/// Built by scanning bundle content for path-like strings, then hashing each
/// string. At extraction time, name hashes are looked up to resolve file names.
pub struct Dictionary {
    hash_to_path: HashMap<u64, String>,
}

impl Default for Dictionary {
    fn default() -> Self {
        Self::new()
    }
}

impl Dictionary {
    /// Create an empty dictionary.
    pub fn new() -> Self {
        Self {
            hash_to_path: HashMap::new(),
        }
    }

    /// Load a dictionary from a text file (one path per line).
    ///
    /// Computes MurmurHash64A for each path at load time, hashing both the
    /// full path and the stem (without extension).
    pub fn load(path: &str) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let mut dict = Self::new();
        for line in content.lines() {
            let line = line.trim().to_string();
            if !line.is_empty() {
                dict.add_path(&line);
            }
        }
        Ok(dict)
    }

    /// Save dictionary to a text file (one unique path per line, sorted).
    pub fn save(&self, path: &str) -> Result<()> {
        let mut paths: Vec<&str> = self.hash_to_path.values().map(|s| s.as_str()).collect();
        paths.sort();
        paths.dedup();
        fs::write(path, paths.join("\n") + "\n")?;
        Ok(())
    }

    /// Merge another dictionary into this one.
    ///
    /// Existing entries are preserved (first-write-wins for each hash).
    pub fn merge(&mut self, other: &Self) {
        for (hash, path) in &other.hash_to_path {
            self.hash_to_path
                .entry(*hash)
                .or_insert_with(|| path.clone());
        }
    }

    /// Add a single path to the dictionary.
    ///
    /// Hashes both the full path and the stem (without last extension segment),
    /// since the name hash in bundles may use either.
    pub fn add_path(&mut self, path: &str) {
        // Hash the full path
        let hash = murmur_hash64(path.as_bytes());
        self.hash_to_path
            .entry(hash)
            .or_insert_with(|| path.to_string());

        // Hash the stem (path without last extension, e.g. "foo.stream" → "foo")
        if let Some(dot_pos) = path.rfind('.') {
            let stem = &path[..dot_pos];
            let stem_hash = murmur_hash64(stem.as_bytes());
            self.hash_to_path
                .entry(stem_hash)
                .or_insert_with(|| path.to_string());
        }

        // Hash just the file name (last path component)
        if let Some(slash_pos) = path.rfind('/') {
            let filename = &path[slash_pos + 1..];
            let filename_hash = murmur_hash64(filename.as_bytes());
            self.hash_to_path
                .entry(filename_hash)
                .or_insert_with(|| path.to_string());

            // Hash the filename stem too
            if let Some(dot_pos) = filename.rfind('.') {
                let filename_stem = &filename[..dot_pos];
                let stem_hash = murmur_hash64(filename_stem.as_bytes());
                self.hash_to_path
                    .entry(stem_hash)
                    .or_insert_with(|| path.to_string());
            }
        }
    }

    /// Resolve a name hash to a file path.
    pub fn resolve(&self, hash: u64) -> Option<&str> {
        self.hash_to_path.get(&hash).map(|s| s.as_str())
    }

    /// Number of unique hash entries.
    pub fn len(&self) -> usize {
        self.hash_to_path.len()
    }

    /// Whether the dictionary is empty.
    pub fn is_empty(&self) -> bool {
        self.hash_to_path.is_empty()
    }

    /// Iterate over all hash keys.
    pub fn hashes(&self) -> impl Iterator<Item = &u64> {
        self.hash_to_path.keys()
    }
}

/// Scan binary data for path-like strings.
///
/// Finds runs of printable ASCII bytes (0x20–0x7E) terminated by null (0x00)
/// or 0xFF, then filters for strings that look like file paths.
///
/// Returns a sorted, deduplicated vector of discovered paths.
pub fn scan_strings(data: &[u8]) -> Vec<String> {
    let mut strings = Vec::new();
    let mut i = 0;

    while i < data.len() {
        // Skip non-printable bytes (these act as terminators)
        if data[i] < 0x20 || data[i] > 0x7E {
            i += 1;
            continue;
        }

        // Start of a potential string
        let start = i;
        while i < data.len() && data[i] >= 0x20 && data[i] <= 0x7E {
            i += 1;
        }

        let len = i - start;
        if !(5..=1024).contains(&len) {
            continue;
        }

        let string = std::str::from_utf8(&data[start..i]).unwrap_or("");

        if is_path_like(string) {
            strings.push(string.to_string());
        }
    }

    strings.sort();
    strings.dedup();
    strings
}

/// Check if a string looks like a file path relevant to Darktide bundles.
fn is_path_like(s: &str) -> bool {
    // Must contain at least one path separator
    if !s.contains('/') {
        return false;
    }

    // Must not contain spaces (paths shouldn't have spaces)
    if s.contains(' ') {
        return false;
    }

    // Must not look like a URL
    if s.starts_with("http://") || s.starts_with("https://") {
        return false;
    }

    // Check known Darktide path prefixes
    if s.starts_with("data/")
        || s.starts_with("scripts/")
        || s.starts_with("content/")
        || s.starts_with("assets/")
        || s.starts_with("resources/")
    {
        return true;
    }

    // Accept any path with at least 2 segments (e.g. "foo/bar/baz")
    // that contains only valid path characters
    if s.matches('/').count() >= 2 && is_valid_path_string(s) {
        return true;
    }

    false
}

/// Check if a string contains only valid path characters.
fn is_valid_path_string(s: &str) -> bool {
    s.chars().all(|c| {
        c.is_ascii_alphanumeric() || c == '/' || c == '\\' || c == '_' || c == '-' || c == '.'
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_finds_data_paths() {
        let data = b"data/92/92a98617c33549427\0some other stuff\0data/e3/e3a8144660f3cb9a.stream";
        let strings = scan_strings(data);
        assert!(strings.contains(&"data/92/92a98617c33549427".to_string()));
        assert!(strings.contains(&"data/e3/e3a8144660f3cb9a.stream".to_string()));
    }

    #[test]
    fn scan_ignores_short_strings() {
        let data = b"abc\0data/92/92a98617c33549427";
        let strings = scan_strings(data);
        assert!(!strings.contains(&"abc".to_string()));
        assert!(strings.contains(&"data/92/92a98617c33549427".to_string()));
    }

    #[test]
    fn scan_handles_0xff_terminator() {
        let data = b"data/92/92a98617c33549427\xFFmore data";
        let strings = scan_strings(data);
        assert!(strings.contains(&"data/92/92a98617c33549427".to_string()));
    }

    #[test]
    fn is_path_like_rejects_urls() {
        assert!(!is_path_like("https://example.com/path"));
    }

    #[test]
    fn is_path_like_accepts_data_paths() {
        assert!(is_path_like("data/92/92a98617c33549427"));
        assert!(is_path_like("scripts/lua/some_file"));
        assert!(is_path_like("content/models/player"));
    }

    #[test]
    fn dictionary_resolves_hashes() {
        let mut dict = Dictionary::new();
        dict.add_path("data/92/92a98617c33549427");
        let hash = murmur_hash64(b"data/92/92a98617c33549427");
        assert_eq!(dict.resolve(hash), Some("data/92/92a98617c33549427"));
    }

    #[test]
    fn dictionary_resolves_stem_hashes() {
        let mut dict = Dictionary::new();
        dict.add_path("data/92/92a98617c33549427.stream");
        let stem = "data/92/92a98617c33549427";
        let stem_hash = murmur_hash64(stem.as_bytes());
        assert_eq!(
            dict.resolve(stem_hash),
            Some("data/92/92a98617c33549427.stream")
        );
    }
}
