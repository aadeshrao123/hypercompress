use crate::chunk::Chunk;
use crate::format::DataType;

/// Fingerprint statistics computed from a chunk of data.
#[derive(Debug, Clone)]
pub struct Fingerprint {
    pub entropy: f64,
    pub ascii_ratio: f64,
    pub zero_ratio: f64,
    pub unique_bytes: u16,
    pub histogram: [u32; 256],
    pub avg_delta: f64,
    pub utf8_ratio: f64,
    pub numeric_word_size: u8,
    /// IEEE 754 float likelihood score (0.0 - 1.0)
    pub float_score: f64,
    /// Sorted/sequential integer likelihood score
    pub int_score: f64,
}

impl Fingerprint {
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
                float_score: 0.0,
                int_score: 0.0,
            };
        }

        let len = data.len() as f64;
        let mut histogram = [0u32; 256];
        let mut ascii_count = 0u64;
        let mut zero_count = 0u64;
        let mut delta_sum = 0u64;

        for i in 0..data.len() {
            let b = data[i];
            histogram[b as usize] += 1;
            if (b >= 0x20 && b <= 0x7E) || b == b'\n' || b == b'\r' || b == b'\t' {
                ascii_count += 1;
            }
            if b == 0 {
                zero_count += 1;
            }
            if i > 0 {
                delta_sum += (data[i] as i16 - data[i - 1] as i16).unsigned_abs() as u64;
            }
        }

        let entropy = compute_entropy(&histogram, data.len());
        let unique_bytes = histogram.iter().filter(|&&c| c > 0).count() as u16;
        let utf8_ratio = compute_utf8_ratio(data);
        let numeric_word_size = detect_word_size(data);
        let float_score = detect_float_score(data);
        let int_score = detect_int_score(data);

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
            float_score,
            int_score,
        }
    }

    pub fn classify(&self) -> DataType {
        // Near-maximum entropy → compressed or random, don't waste time
        if self.entropy > 7.5 {
            return DataType::CompressedOrRandom;
        }

        // Very low entropy with lots of zeros → sparse data
        if self.entropy < 2.5 && self.zero_ratio > 0.4 {
            return DataType::Sparse;
        }

        // Float detection (before ASCII check — floats can have mixed ASCII ratios)
        if self.float_score > 0.4 && self.ascii_ratio < 0.7 {
            return DataType::NumericFloat;
        }

        // Integer detection
        if self.int_score > 0.5 && self.ascii_ratio < 0.4 {
            return DataType::NumericInt;
        }

        // High ASCII ratio → text or structured
        if self.ascii_ratio > 0.85 {
            if self.has_structured_patterns() {
                return DataType::Structured;
            }
            return DataType::Text;
        }

        // Medium ASCII with structured patterns (mixed data)
        if self.ascii_ratio > 0.5 && self.has_structured_patterns() {
            return DataType::Structured;
        }

        // Low ASCII with word alignment → numeric
        if self.ascii_ratio < 0.4 {
            if self.numeric_word_size == 4 && self.entropy > 5.5 {
                return DataType::NumericFloat;
            }
            if self.numeric_word_size >= 2 {
                return DataType::NumericInt;
            }
        }

        // Sparse data
        if self.zero_ratio > 0.3 && self.entropy < 4.0 {
            return DataType::Sparse;
        }

        DataType::Binary
    }

    fn has_structured_patterns(&self) -> bool {
        let delimiters = [b'{', b'}', b'[', b']', b',', b':', b'"', b'<', b'>'];
        let delimiter_count: u32 = delimiters.iter().map(|&d| self.histogram[d as usize]).sum();
        let total: u32 = self.histogram.iter().sum();
        if total == 0 {
            return false;
        }
        (delimiter_count as f64 / total as f64) > 0.08
    }
}

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
            0
        } else if b < 0xE0 {
            2
        } else if b < 0xF0 {
            3
        } else if b < 0xF8 {
            4
        } else {
            0
        };
        if seq_len == 0 || i + seq_len > data.len() {
            i += 1;
            continue;
        }
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

/// Detect IEEE 754 float patterns in data.
/// Floats have very specific exponent byte distributions.
fn detect_float_score(data: &[u8]) -> f64 {
    if data.len() < 16 || data.len() % 4 != 0 {
        return 0.0;
    }

    let num_floats = data.len() / 4;
    let mut valid_float_count = 0u64;
    let mut exponent_histogram = [0u32; 256];
    let mut _sign_changes = 0u64;
    let mut last_sign = 0u8;

    for i in 0..num_floats {
        let offset = i * 4;
        let bytes = [data[offset], data[offset + 1], data[offset + 2], data[offset + 3]];
        let bits = u32::from_le_bytes(bytes);
        let f = f32::from_bits(bits);

        // Check if it's a "normal" float (not NaN, not infinity, not subnormal garbage)
        if f.is_finite() && (f == 0.0 || f.abs() > 1e-30 && f.abs() < 1e30) {
            valid_float_count += 1;
        }

        // Track exponent byte (byte 3 = sign + high exponent)
        let exp_byte = bytes[3];
        exponent_histogram[exp_byte as usize] += 1;

        let sign = exp_byte >> 7;
        if i > 0 && sign != last_sign {
            _sign_changes += 1;
        }
        last_sign = sign;
    }

    let valid_ratio = valid_float_count as f64 / num_floats as f64;

    // Exponent bytes should have LOW entropy (clustered around a few values)
    let exp_entropy = {
        let mut e = 0.0;
        let total = num_floats as f64;
        for &count in &exponent_histogram {
            if count > 0 {
                let p = count as f64 / total;
                e -= p * p.log2();
            }
        }
        e
    };

    // Strong float signals:
    // 1. Most values are valid finite floats
    // 2. Exponent byte has low entropy (typically 1-3 bits for similar-magnitude floats)
    // 3. Byte 3 (exponent) has much lower entropy than bytes 0-1 (mantissa)
    let mut score: f64 = 0.0;

    if valid_ratio > 0.8 {
        score += 0.4;
    } else if valid_ratio > 0.5 {
        score += 0.2;
    }

    if exp_entropy < 4.0 {
        score += 0.3;
    } else if exp_entropy < 6.0 {
        score += 0.1;
    }

    // Check that mantissa bytes (0,1) have higher entropy than exponent byte (3)
    let mantissa_entropy = {
        let mut hist = [0u32; 256];
        for i in 0..num_floats {
            hist[data[i * 4] as usize] += 1;
        }
        let mut e = 0.0;
        let total = num_floats as f64;
        for &count in &hist {
            if count > 0 {
                let p = count as f64 / total;
                e -= p * p.log2();
            }
        }
        e
    };

    if mantissa_entropy > exp_entropy + 1.0 {
        score += 0.3;
    }

    score.min(1.0)
}

/// Detect sorted/sequential integer patterns.
fn detect_int_score(data: &[u8]) -> f64 {
    if data.len() < 16 {
        return 0.0;
    }

    let mut best_score = 0.0;

    // Check 4-byte integers (most common)
    if data.len() % 4 == 0 {
        let score = int_score_for_width(data, 4);
        if score > best_score {
            best_score = score;
        }
    }

    // Check 2-byte integers
    if data.len() % 2 == 0 {
        let score = int_score_for_width(data, 2);
        if score > best_score {
            best_score = score;
        }
    }

    // Check byte-level delta patterns
    let mut small_deltas = 0u64;
    for i in 1..data.len() {
        let delta = (data[i] as i16 - data[i - 1] as i16).abs();
        if delta <= 4 {
            small_deltas += 1;
        }
    }
    let byte_delta_score = small_deltas as f64 / (data.len() - 1) as f64;
    if byte_delta_score > best_score {
        best_score = byte_delta_score;
    }

    best_score
}

fn int_score_for_width(data: &[u8], width: usize) -> f64 {
    let count = data.len() / width;
    if count < 4 {
        return 0.0;
    }

    let mut small_deltas = 0u64;
    let mut prev_val = read_int(data, 0, width);

    for i in 1..count {
        let val = read_int(data, i * width, width);
        let delta = (val as i64 - prev_val as i64).unsigned_abs();
        // "Small" delta relative to the value range
        if delta <= (1 << (width * 2)) as u64 {
            small_deltas += 1;
        }
        prev_val = val;
    }

    small_deltas as f64 / (count - 1) as f64
}

fn read_int(data: &[u8], offset: usize, width: usize) -> u64 {
    match width {
        2 => u16::from_le_bytes([data[offset], data[offset + 1]]) as u64,
        4 => u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]) as u64,
        8 => u64::from_le_bytes([
            data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
            data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7],
        ]),
        _ => data[offset] as u64,
    }
}

fn detect_word_size(data: &[u8]) -> u8 {
    if data.len() < 16 {
        return 0;
    }

    // Check 4-byte first (most common for both floats and ints)
    if data.len() % 4 == 0 {
        let score = alignment_score(data, 4);
        if score > 0.15 {
            return 4;
        }
    }

    if data.len() % 8 == 0 {
        let score = alignment_score(data, 8);
        if score > 0.15 {
            return 8;
        }
    }

    if data.len() % 2 == 0 {
        let score = alignment_score(data, 2);
        if score > 0.15 {
            return 2;
        }
    }

    0
}

fn alignment_score(data: &[u8], word_size: usize) -> f64 {
    if data.len() < word_size * 4 {
        return 0.0;
    }

    let num_words = data.len() / word_size;
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

    let var_mean: f64 = variance_per_pos.iter().sum::<f64>() / word_size as f64;
    if var_mean < 0.5 {
        return 0.0;
    }

    let var_of_var: f64 = variance_per_pos
        .iter()
        .map(|v| (v - var_mean).powi(2))
        .sum::<f64>()
        / word_size as f64;

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
    }

    #[test]
    fn test_random_detection() {
        let data: Vec<u8> = (0..10000).map(|i| ((i * 7 + 13) % 256) as u8).collect();
        let fp = Fingerprint::compute(&data);
        assert!(fp.entropy > 7.0);
    }

    #[test]
    fn test_sparse_detection() {
        let mut data = vec![0u8; 10000];
        for i in (0..data.len()).step_by(100) {
            data[i] = 42;
        }
        let fp = Fingerprint::compute(&data);
        assert_eq!(fp.classify(), DataType::Sparse);
    }

    #[test]
    fn test_json_detection() {
        let json = br#"{"name":"Alice","age":30,"scores":[100,95,87],"active":true}"#;
        let mut data = Vec::new();
        for _ in 0..100 {
            data.extend_from_slice(json);
            data.push(b'\n');
        }
        let fp = Fingerprint::compute(&data);
        assert_eq!(fp.classify(), DataType::Structured);
    }

    #[test]
    fn test_float_detection() {
        // Array of IEEE 754 floats — should detect as NumericFloat
        let floats: Vec<f32> = (0..10000).map(|i| 1.0 + (i as f32) * 0.001).collect();
        let data: Vec<u8> = floats.iter().flat_map(|f| f.to_le_bytes()).collect();
        let fp = Fingerprint::compute(&data);
        assert_eq!(
            fp.classify(),
            DataType::NumericFloat,
            "Expected NumericFloat, got {:?}. float_score={:.3}, entropy={:.3}, ascii={:.3}",
            fp.classify(),
            fp.float_score,
            fp.entropy,
            fp.ascii_ratio
        );
    }

    #[test]
    fn test_int_detection() {
        // Array of sequential 32-bit integers — should detect as NumericInt
        let data: Vec<u8> = (0u32..5000).flat_map(|i| i.to_le_bytes()).collect();
        let fp = Fingerprint::compute(&data);
        let dt = fp.classify();
        assert!(
            dt == DataType::NumericInt || dt == DataType::NumericFloat,
            "Expected numeric type, got {:?}. int_score={:.3}, word_size={}",
            dt,
            fp.int_score,
            fp.numeric_word_size
        );
    }

    #[test]
    fn test_empty() {
        let fp = Fingerprint::compute(&[]);
        assert_eq!(fp.entropy, 0.0);
        assert_eq!(fp.unique_bytes, 0);
    }
}
