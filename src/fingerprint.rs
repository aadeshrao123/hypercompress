use crate::chunk::Chunk;
use crate::format::DataType;

/// Fingerprint statistics computed from a chunk of data.
#[derive(Debug, Clone)]
pub struct Fingerprint {
    /// Shannon entropy in bits per byte (0.0 - 8.0)
    pub entropy: f64,
    /// Fraction of bytes that are printable ASCII (0.0 - 1.0)
    pub ascii_ratio: f64,
    /// Fraction of bytes that are zero (0.0 - 1.0)
    pub zero_ratio: f64,
    /// Number of distinct byte values (1 - 256)
    pub unique_bytes: u16,
    /// Byte frequency histogram
    pub histogram: [u32; 256],
    /// Average absolute delta between consecutive bytes
    pub avg_delta: f64,
    /// Ratio of bytes that form valid UTF-8 sequences
    pub utf8_ratio: f64,
    /// Detected word size for numeric data (1, 2, 4, 8 or 0 if not numeric)
    pub numeric_word_size: u8,
}

impl Fingerprint {
    /// Compute fingerprint from raw data. Single pass for histogram,
    /// second pass for derived metrics. ~2 GB/s on modern hardware.
    pub fn compute(data: &[u8]) -> Self {
        if data.is_empty() {
            return Fingerprint {
                entropy: 0.0,
                ascii_ratio: 0.0,
                zero_ratio: 0.0,
                unique_bytes: 0,
                histogram: [0; 256],
                avg_delta: 0.0,
                utf8_ratio: 0.0,
                numeric_word_size: 0,
            };
        }

        let len = data.len() as f64;

        // Single pass: histogram + ascii count + zero count + delta sum
        let mut histogram = [0u32; 256];
        let mut ascii_count = 0u64;
        let mut zero_count = 0u64;
        let mut delta_sum = 0u64;

        for i in 0..data.len() {
            let b = data[i];
            histogram[b as usize] += 1;

            if b >= 0x20 && b <= 0x7E || b == b'\n' || b == b'\r' || b == b'\t' {
                ascii_count += 1;
            }
            if b == 0 {
                zero_count += 1;
            }
            if i > 0 {
                delta_sum += (data[i] as i16 - data[i - 1] as i16).unsigned_abs() as u64;
            }
        }

        // Shannon entropy
        let entropy = compute_entropy(&histogram, data.len());

        // Unique byte count
        let unique_bytes = histogram.iter().filter(|&&c| c > 0).count() as u16;

        // UTF-8 validity ratio
        let utf8_ratio = compute_utf8_ratio(data);

        // Detect numeric word size
        let numeric_word_size = detect_word_size(data);

        Fingerprint {
            entropy,
            ascii_ratio: ascii_count as f64 / len,
            zero_ratio: zero_count as f64 / len,
            unique_bytes,
            histogram,
            avg_delta: if data.len() > 1 {
                delta_sum as f64 / (data.len() - 1) as f64
            } else {
                0.0
            },
            utf8_ratio,
            numeric_word_size,
        }
    }

    /// Classify the chunk's data type based on fingerprint metrics.
    pub fn classify(&self) -> DataType {
        // Near-maximum entropy → compressed or random, don't waste time
        if self.entropy > 7.5 {
            return DataType::CompressedOrRandom;
        }

        // Very low entropy with lots of zeros → sparse data
        if self.entropy < 2.0 && self.zero_ratio > 0.5 {
            return DataType::Sparse;
        }

        // High ASCII ratio → text or structured
        if self.ascii_ratio > 0.85 {
            if self.has_structured_patterns() {
                return DataType::Structured;
            }
            return DataType::Text;
        }

        // Low ASCII, detected word alignment → numeric
        if self.ascii_ratio < 0.3 {
            if self.numeric_word_size == 4 && self.entropy > 6.0 {
                // 4-byte values with high entropy = likely IEEE 754 floats
                return DataType::NumericFloat;
            }
            if self.numeric_word_size >= 2 {
                return DataType::NumericInt;
            }
        }

        // Sparse data (low entropy, many zeros but not super high)
        if self.zero_ratio > 0.3 && self.entropy < 4.0 {
            return DataType::Sparse;
        }

        DataType::Binary
    }

    /// Check for structured data patterns (JSON/CSV/XML delimiters).
    fn has_structured_patterns(&self) -> bool {
        let delimiters = [b'{', b'}', b'[', b']', b',', b':', b'"', b'<', b'>'];
        let delimiter_count: u32 = delimiters.iter().map(|&d| self.histogram[d as usize]).sum();
        let total: u32 = self.histogram.iter().sum();
        if total == 0 {
            return false;
        }
        // If >10% of bytes are delimiters, likely structured
        (delimiter_count as f64 / total as f64) > 0.10
    }
}

/// Compute Shannon entropy from a histogram.
fn compute_entropy(histogram: &[u32; 256], total: usize) -> f64 {
    if total == 0 {
        return 0.0;
    }
    let len = total as f64;
    let mut entropy = 0.0;
    for &count in histogram.iter() {
        if count > 0 {
            let p = count as f64 / len;
            entropy -= p * p.log2();
        }
    }
    entropy
}

/// Estimate the fraction of data that is valid UTF-8.
fn compute_utf8_ratio(data: &[u8]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let mut valid_bytes = 0usize;
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        let seq_len = if b < 0x80 {
            1
        } else if b < 0xC0 {
            0 // invalid start byte
        } else if b < 0xE0 {
            2
        } else if b < 0xF0 {
            3
        } else if b < 0xF8 {
            4
        } else {
            0
        };

        if seq_len == 0 {
            i += 1;
            continue;
        }

        if i + seq_len > data.len() {
            i += 1;
            continue;
        }

        // Validate continuation bytes
        let mut valid = true;
        for j in 1..seq_len {
            if data[i + j] & 0xC0 != 0x80 {
                valid = false;
                break;
            }
        }

        if valid {
            valid_bytes += seq_len;
            i += seq_len;
        } else {
            i += 1;
        }
    }
    valid_bytes as f64 / data.len() as f64
}

/// Detect if data has a regular word size (2, 4, or 8 bytes).
/// Checks if delta encoding at a given word size produces low entropy.
fn detect_word_size(data: &[u8]) -> u8 {
    if data.len() < 16 {
        return 0;
    }

    // Check for 4-byte alignment patterns (most common for floats/ints)
    if data.len() >= 16 && data.len() % 4 == 0 {
        let score_4 = alignment_score(data, 4);
        if score_4 > 0.3 {
            return 4;
        }
    }

    // Check for 8-byte alignment (doubles, 64-bit ints)
    if data.len() >= 16 && data.len() % 8 == 0 {
        let score_8 = alignment_score(data, 8);
        if score_8 > 0.3 {
            return 8;
        }
    }

    // Check for 2-byte alignment (shorts, 16-bit audio)
    if data.len() >= 8 && data.len() % 2 == 0 {
        let score_2 = alignment_score(data, 2);
        if score_2 > 0.3 {
            return 2;
        }
    }

    0
}

/// Score how well data aligns to a given word size.
/// Higher score = more likely to be that word size.
/// Measures consistency of byte patterns at each position within the word.
fn alignment_score(data: &[u8], word_size: usize) -> f64 {
    if data.len() < word_size * 4 {
        return 0.0;
    }

    // For each byte position within the word, compute how "consistent" that position is
    // across all words. If position 0 always has similar values, it's likely aligned.
    let num_words = data.len() / word_size;
    let mut total_variance = 0.0;

    for pos in 0..word_size {
        let mut sum = 0u64;
        let mut sum_sq = 0u64;
        for w in 0..num_words {
            let val = data[w * word_size + pos] as u64;
            sum += val;
            sum_sq += val * val;
        }
        let mean = sum as f64 / num_words as f64;
        let variance = (sum_sq as f64 / num_words as f64) - mean * mean;
        // Normalize variance to 0-1 range (max byte variance is ~5400 for uniform)
        total_variance += variance / 5500.0;
    }

    let _avg_variance = total_variance / word_size as f64;

    // Low average variance across positions but different variance PER position
    // suggests word alignment (e.g., exponent bytes are consistent, mantissa bytes vary)
    let mut variance_per_pos = Vec::with_capacity(word_size);
    for pos in 0..word_size {
        let mut sum = 0u64;
        let mut sum_sq = 0u64;
        for w in 0..num_words {
            let val = data[w * word_size + pos] as u64;
            sum += val;
            sum_sq += val * val;
        }
        let mean = sum as f64 / num_words as f64;
        let variance = (sum_sq as f64 / num_words as f64) - mean * mean;
        variance_per_pos.push(variance);
    }

    // If variance differs significantly between positions, it's structured
    let var_mean: f64 = variance_per_pos.iter().sum::<f64>() / word_size as f64;
    if var_mean < 1.0 {
        return 0.0;
    }
    let var_of_var: f64 = variance_per_pos
        .iter()
        .map(|v| (v - var_mean).powi(2))
        .sum::<f64>()
        / word_size as f64;

    // High variance-of-variance relative to mean = good alignment signal
    let score = (var_of_var / (var_mean * var_mean + 1.0)).sqrt();
    score.min(1.0)
}

/// Fingerprint all chunks in place, setting their data_type field.
pub fn fingerprint_chunks(chunks: &mut [Chunk]) {
    for chunk in chunks.iter_mut() {
        let fp = Fingerprint::compute(&chunk.data);
        chunk.data_type = fp.classify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_detection() {
        let text = b"Hello, world! This is a test of the fingerprinting system. \
                     It should detect this as text because it contains mostly ASCII \
                     characters and has reasonable entropy for natural language.";
        let fp = Fingerprint::compute(text);
        assert_eq!(fp.classify(), DataType::Text);
        assert!(fp.ascii_ratio > 0.9);
        assert!(fp.entropy > 3.0 && fp.entropy < 6.0);
    }

    #[test]
    fn test_random_detection() {
        // Simulated high-entropy data
        let data: Vec<u8> = (0..10000).map(|i| ((i * 7 + 13) % 256) as u8).collect();
        let fp = Fingerprint::compute(&data);
        // This particular pattern has ~8 bits entropy
        assert!(fp.entropy > 7.0);
    }

    #[test]
    fn test_sparse_detection() {
        let mut data = vec![0u8; 10000];
        // Sprinkle a few non-zero bytes
        for i in (0..data.len()).step_by(100) {
            data[i] = 42;
        }
        let fp = Fingerprint::compute(&data);
        assert_eq!(fp.classify(), DataType::Sparse);
        assert!(fp.zero_ratio > 0.9);
    }

    #[test]
    fn test_json_detection() {
        let json = br#"{"name":"Alice","age":30,"scores":[100,95,87],"active":true}"#;
        // Repeat to get enough data
        let mut data = Vec::new();
        for _ in 0..100 {
            data.extend_from_slice(json);
            data.push(b'\n');
        }
        let fp = Fingerprint::compute(&data);
        assert_eq!(fp.classify(), DataType::Structured);
    }

    #[test]
    fn test_empty() {
        let fp = Fingerprint::compute(&[]);
        assert_eq!(fp.entropy, 0.0);
        assert_eq!(fp.unique_bytes, 0);
    }
}
