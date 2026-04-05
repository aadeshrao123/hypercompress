/// Hierarchical dictionary engine.
///
/// Three tiers:
/// - Tier 1 (Universal): Built-in common patterns
/// - Tier 2 (Domain): Auto-learned from the file being compressed
/// - Tier 3 (Local): Per-chunk patterns (handled inline during compression)
///
/// For the initial implementation, we focus on Tier 2 (domain dictionary)
/// built by analyzing the input data before compression begins.

use std::collections::HashMap;

/// A compiled dictionary for use during compression.
#[derive(Debug, Clone)]
pub struct Dictionary {
    /// Entries: byte patterns and their IDs
    pub entries: Vec<Vec<u8>>,
    /// Lookup: pattern hash → entry index
    lookup: HashMap<u64, Vec<usize>>,
    /// Minimum pattern length for dictionary matching
    pub min_match: usize,
}

impl Dictionary {
    /// Create an empty dictionary.
    pub fn new() -> Self {
        Dictionary {
            entries: Vec::new(),
            lookup: HashMap::new(),
            min_match: 4,
        }
    }

    /// Build a domain dictionary from input data.
    /// Finds the most frequent n-grams and stores them.
    pub fn build_from_data(data: &[u8], max_entries: usize) -> Self {
        let mut dict = Dictionary::new();

        if data.len() < 8 {
            return dict;
        }

        // Count 4-byte, 8-byte, and 16-byte n-grams
        let mut ngram_counts: HashMap<&[u8], u32> = HashMap::new();

        for window_size in [4, 8, 16] {
            if data.len() < window_size {
                continue;
            }
            for window in data.windows(window_size) {
                *ngram_counts.entry(window).or_insert(0) += 1;
            }
        }

        // Keep only n-grams that appear at least 3 times
        let mut frequent: Vec<(&[u8], u32)> = ngram_counts
            .into_iter()
            .filter(|(_, count)| *count >= 3)
            .collect();

        // Sort by savings: count * length (more savings = better)
        frequent.sort_by(|a, b| {
            let savings_a = a.1 as usize * a.0.len();
            let savings_b = b.1 as usize * b.0.len();
            savings_b.cmp(&savings_a)
        });

        // Take top entries
        for (pattern, _) in frequent.into_iter().take(max_entries) {
            let idx = dict.entries.len();
            let hash = hash_bytes(pattern);
            dict.lookup.entry(hash).or_default().push(idx);
            dict.entries.push(pattern.to_vec());
        }

        dict
    }

    /// Look up a pattern in the dictionary. Returns entry index if found.
    pub fn find(&self, data: &[u8]) -> Option<usize> {
        if data.len() < self.min_match {
            return None;
        }

        // Check different lengths, prefer longest match
        for check_len in [16, 8, 4] {
            if data.len() < check_len {
                continue;
            }
            let hash = hash_bytes(&data[..check_len]);
            if let Some(indices) = self.lookup.get(&hash) {
                for &idx in indices {
                    if self.entries[idx] == data[..check_len] {
                        return Some(idx);
                    }
                }
            }
        }

        None
    }

    /// Serialize dictionary for storage in .hc file header.
    pub fn serialize(&self) -> Vec<u8> {
        let mut result = Vec::new();
        let count = self.entries.len() as u32;
        result.extend_from_slice(&count.to_le_bytes());

        for entry in &self.entries {
            let len = entry.len() as u16;
            result.extend_from_slice(&len.to_le_bytes());
            result.extend_from_slice(entry);
        }

        result
    }

    /// Deserialize dictionary from .hc file header.
    pub fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < 4 {
            return None;
        }

        let count = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let mut dict = Dictionary::new();
        let mut pos = 4;

        for _ in 0..count {
            if pos + 2 > data.len() {
                break;
            }
            let len = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
            pos += 2;
            if pos + len > data.len() {
                break;
            }
            let pattern = data[pos..pos + len].to_vec();
            let idx = dict.entries.len();
            let hash = hash_bytes(&pattern);
            dict.lookup.entry(hash).or_default().push(idx);
            dict.entries.push(pattern);
            pos += len;
        }

        Some(dict)
    }
}

/// Simple hash function for byte patterns.
fn hash_bytes(data: &[u8]) -> u64 {
    // FNV-1a hash
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in data {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_dictionary() {
        // Repetitive data should produce dictionary entries
        let mut data = Vec::new();
        for _ in 0..100 {
            data.extend_from_slice(b"hello world ");
        }
        let dict = Dictionary::build_from_data(&data, 100);
        assert!(!dict.entries.is_empty());
    }

    #[test]
    fn test_serialize_deserialize() {
        let mut data = Vec::new();
        for _ in 0..50 {
            data.extend_from_slice(b"test pattern data here ");
        }
        let dict = Dictionary::build_from_data(&data, 50);
        let serialized = dict.serialize();
        let restored = Dictionary::deserialize(&serialized).unwrap();
        assert_eq!(dict.entries.len(), restored.entries.len());
    }

    #[test]
    fn test_empty_dictionary() {
        let dict = Dictionary::new();
        assert!(dict.find(b"anything").is_none());
        let serialized = dict.serialize();
        let restored = Dictionary::deserialize(&serialized).unwrap();
        assert_eq!(restored.entries.len(), 0);
    }
}
