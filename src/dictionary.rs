use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Dictionary {
    pub entries: Vec<Vec<u8>>,
    lookup: HashMap<u64, Vec<usize>>,
    pub min_match: usize,
}

impl Dictionary {
    pub fn new() -> Self {
        Dictionary {
            entries: Vec::new(),
            lookup: HashMap::new(),
            min_match: 4,
        }
    }

    /// Build a dictionary from frequent n-grams in the input.
    pub fn build_from_data(data: &[u8], max_entries: usize) -> Self {
        let mut dict = Dictionary::new();

        if data.len() < 8 {
            return dict;
        }

        let mut counts: HashMap<&[u8], u32> = HashMap::new();

        for ws in [4, 8, 16] {
            if data.len() < ws { continue; }
            for w in data.windows(ws) {
                *counts.entry(w).or_insert(0) += 1;
            }
        }

        let mut frequent: Vec<(&[u8], u32)> = counts
            .into_iter()
            .filter(|(_, c)| *c >= 3)
            .collect();

        // Sort by estimated savings (count * length)
        frequent.sort_by(|a, b| (b.1 as usize * b.0.len()).cmp(&(a.1 as usize * a.0.len())));

        for (pattern, _) in frequent.into_iter().take(max_entries) {
            let idx = dict.entries.len();
            let h = hash_bytes(pattern);
            dict.lookup.entry(h).or_default().push(idx);
            dict.entries.push(pattern.to_vec());
        }

        dict
    }

    /// Find the longest matching entry. Returns entry index or None.
    pub fn find(&self, data: &[u8]) -> Option<usize> {
        if data.len() < self.min_match {
            return None;
        }

        for len in [16, 8, 4] {
            if data.len() < len { continue; }
            let h = hash_bytes(&data[..len]);
            if let Some(indices) = self.lookup.get(&h) {
                for &idx in indices {
                    if self.entries[idx] == data[..len] {
                        return Some(idx);
                    }
                }
            }
        }

        None
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        for e in &self.entries {
            out.extend_from_slice(&(e.len() as u16).to_le_bytes());
            out.extend_from_slice(e);
        }
        out
    }

    pub fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < 4 {
            return None;
        }

        let count = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let mut dict = Dictionary::new();
        let mut pos = 4;

        for _ in 0..count {
            if pos + 2 > data.len() { break; }
            let len = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
            pos += 2;
            if pos + len > data.len() { break; }
            let pattern = data[pos..pos + len].to_vec();
            let idx = dict.entries.len();
            dict.lookup.entry(hash_bytes(&pattern)).or_default().push(idx);
            dict.entries.push(pattern);
            pos += len;
        }

        Some(dict)
    }
}

fn hash_bytes(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_from_repetitive_data() {
        let data = b"hello world ".repeat(100);
        let dict = Dictionary::build_from_data(&data, 100);
        assert!(!dict.entries.is_empty());
    }

    #[test]
    fn serialize_roundtrip() {
        let data = b"test pattern data here ".repeat(50);
        let dict = Dictionary::build_from_data(&data, 50);
        let restored = Dictionary::deserialize(&dict.serialize()).unwrap();
        assert_eq!(dict.entries.len(), restored.entries.len());
    }

    #[test]
    fn empty_dict() {
        let dict = Dictionary::new();
        assert!(dict.find(b"anything").is_none());
        let restored = Dictionary::deserialize(&dict.serialize()).unwrap();
        assert!(restored.entries.is_empty());
    }
}
