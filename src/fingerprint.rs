use crate::chunk::Chunk;
use crate::format::DataType;

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
    pub float_score: f64,
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

        Fingerprint {
            entropy: compute_entropy(&histogram, data.len()),
            ascii_ratio: ascii_count as f64 / len,
            zero_ratio: zero_count as f64 / len,
            unique_bytes: histogram.iter().filter(|&&c| c > 0).count() as u16,
            histogram,
            avg_delta: if data.len() > 1 {
                delta_sum as f64 / (data.len() - 1) as f64
            } else {
                0.0
            },
            utf8_ratio: compute_utf8_ratio(data),
            numeric_word_size: detect_word_size(data),
            float_score: detect_float_score(data),
            int_score: detect_int_score(data),
        }
    }

    pub fn classify(&self) -> DataType {
        if self.entropy > 7.9 {
            return DataType::CompressedOrRandom;
        }

        if self.entropy < 2.5 && self.zero_ratio > 0.4 {
            return DataType::Sparse;
        }

        if self.float_score > 0.4 && self.ascii_ratio < 0.7 {
            return DataType::NumericFloat;
        }

        if self.int_score > 0.5 && self.ascii_ratio < 0.4 {
            return DataType::NumericInt;
        }

        if self.ascii_ratio > 0.85 {
            if self.has_structured_patterns() {
                return DataType::Structured;
            }
            return DataType::Text;
        }

        if self.ascii_ratio > 0.5 && self.has_structured_patterns() {
            return DataType::Structured;
        }

        if self.ascii_ratio < 0.4 {
            if self.numeric_word_size == 4 && self.entropy > 5.5 {
                return DataType::NumericFloat;
            }
            if self.numeric_word_size >= 2 {
                return DataType::NumericInt;
            }
        }

        if self.zero_ratio > 0.3 && self.entropy < 4.0 {
            return DataType::Sparse;
        }

        DataType::Binary
    }

    fn has_structured_patterns(&self) -> bool {
        let delimiters = [b'{', b'}', b'[', b']', b',', b':', b'"', b'<', b'>'];
        let delim_count: u32 = delimiters.iter().map(|&d| self.histogram[d as usize]).sum();
        let total: u32 = self.histogram.iter().sum();
        if total == 0 {
            return false;
        }
        (delim_count as f64 / total as f64) > 0.08
    }
}

fn compute_entropy(histogram: &[u32; 256], total: usize) -> f64 {
    if total == 0 {
        return 0.0;
    }
    let len = total as f64;
    let mut ent = 0.0;
    for &c in histogram.iter() {
        if c > 0 {
            let p = c as f64 / len;
            ent -= p * p.log2();
        }
    }
    ent
}

fn compute_utf8_ratio(data: &[u8]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let mut valid = 0usize;
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        let seq_len = if b < 0x80 { 1 }
            else if b < 0xC0 { 0 }
            else if b < 0xE0 { 2 }
            else if b < 0xF0 { 3 }
            else if b < 0xF8 { 4 }
            else { 0 };
        if seq_len == 0 || i + seq_len > data.len() {
            i += 1;
            continue;
        }
        let ok = (1..seq_len).all(|j| data[i + j] & 0xC0 == 0x80);
        if ok {
            valid += seq_len;
            i += seq_len;
        } else {
            i += 1;
        }
    }
    valid as f64 / data.len() as f64
}

fn detect_float_score(data: &[u8]) -> f64 {
    if data.len() < 16 || data.len() % 4 != 0 {
        return 0.0;
    }

    let nf = data.len() / 4;
    let mut valid_count = 0u64;
    let mut exp_hist = [0u32; 256];

    for i in 0..nf {
        let off = i * 4;
        let bytes = [data[off], data[off + 1], data[off + 2], data[off + 3]];
        let f = f32::from_bits(u32::from_le_bytes(bytes));

        if f.is_finite() && (f == 0.0 || f.abs() > 1e-30 && f.abs() < 1e30) {
            valid_count += 1;
        }
        exp_hist[bytes[3] as usize] += 1;
    }

    let valid_ratio = valid_count as f64 / nf as f64;

    let exp_ent = byte_entropy(&exp_hist, nf);

    let mut score: f64 = 0.0;

    if valid_ratio > 0.8 { score += 0.4; }
    else if valid_ratio > 0.5 { score += 0.2; }

    if exp_ent < 4.0 { score += 0.3; }
    else if exp_ent < 6.0 { score += 0.1; }

    // Mantissa bytes should have higher entropy than exponent byte
    let mut mant_hist = [0u32; 256];
    for i in 0..nf {
        mant_hist[data[i * 4] as usize] += 1;
    }
    if byte_entropy(&mant_hist, nf) > exp_ent + 1.0 {
        score += 0.3;
    }

    score.min(1.0)
}

fn byte_entropy(hist: &[u32; 256], total: usize) -> f64 {
    let mut e = 0.0;
    let t = total as f64;
    for &c in hist {
        if c > 0 {
            let p = c as f64 / t;
            e -= p * p.log2();
        }
    }
    e
}

fn detect_int_score(data: &[u8]) -> f64 {
    if data.len() < 16 {
        return 0.0;
    }

    let mut best = 0.0f64;

    if data.len() % 4 == 0 {
        best = best.max(int_score_for_width(data, 4));
    }
    if data.len() % 2 == 0 {
        best = best.max(int_score_for_width(data, 2));
    }

    // Byte-level delta patterns
    let small: u64 = data.windows(2)
        .filter(|w| (w[1] as i16 - w[0] as i16).abs() <= 4)
        .count() as u64;
    best.max(small as f64 / (data.len() - 1) as f64)
}

fn int_score_for_width(data: &[u8], width: usize) -> f64 {
    let count = data.len() / width;
    if count < 4 {
        return 0.0;
    }

    let mut small_deltas = 0u64;
    let mut prev = read_int(data, 0, width);

    for i in 1..count {
        let val = read_int(data, i * width, width);
        if (val as i64 - prev as i64).unsigned_abs() <= (1 << (width * 2)) as u64 {
            small_deltas += 1;
        }
        prev = val;
    }

    small_deltas as f64 / (count - 1) as f64
}

fn read_int(data: &[u8], off: usize, width: usize) -> u64 {
    match width {
        2 => u16::from_le_bytes([data[off], data[off + 1]]) as u64,
        4 => u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]) as u64,
        8 => u64::from_le_bytes([
            data[off], data[off + 1], data[off + 2], data[off + 3],
            data[off + 4], data[off + 5], data[off + 6], data[off + 7],
        ]),
        _ => data[off] as u64,
    }
}

fn detect_word_size(data: &[u8]) -> u8 {
    if data.len() < 16 {
        return 0;
    }

    if data.len() % 4 == 0 && alignment_score(data, 4) > 0.15 {
        return 4;
    }
    if data.len() % 8 == 0 && alignment_score(data, 8) > 0.15 {
        return 8;
    }
    if data.len() % 2 == 0 && alignment_score(data, 2) > 0.15 {
        return 2;
    }
    0
}

fn alignment_score(data: &[u8], word_size: usize) -> f64 {
    if data.len() < word_size * 4 {
        return 0.0;
    }

    let nw = data.len() / word_size;
    let mut var_per_pos = Vec::with_capacity(word_size);

    for pos in 0..word_size {
        let mut sum = 0u64;
        let mut sum_sq = 0u64;
        for w in 0..nw {
            let v = data[w * word_size + pos] as u64;
            sum += v;
            sum_sq += v * v;
        }
        let mean = sum as f64 / nw as f64;
        var_per_pos.push((sum_sq as f64 / nw as f64) - mean * mean);
    }

    let vm: f64 = var_per_pos.iter().sum::<f64>() / word_size as f64;
    if vm < 0.5 {
        return 0.0;
    }

    let vv: f64 = var_per_pos.iter().map(|v| (v - vm).powi(2)).sum::<f64>() / word_size as f64;
    (vv / (vm * vm + 1.0)).sqrt().min(1.0)
}

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
    fn detects_text() {
        let text = b"Hello, world! This is a test of the fingerprinting system. \
                     It should detect this as text because it contains mostly ASCII \
                     characters and has reasonable entropy for natural language.";
        assert_eq!(Fingerprint::compute(text).classify(), DataType::Text);
    }

    #[test]
    fn high_entropy_data() {
        let data: Vec<u8> = (0..10000).map(|i| ((i * 7 + 13) % 256) as u8).collect();
        assert!(Fingerprint::compute(&data).entropy > 7.0);
    }

    #[test]
    fn detects_sparse() {
        let mut data = vec![0u8; 10000];
        for i in (0..data.len()).step_by(100) {
            data[i] = 42;
        }
        assert_eq!(Fingerprint::compute(&data).classify(), DataType::Sparse);
    }

    #[test]
    fn detects_json() {
        let json = br#"{"name":"Alice","age":30,"scores":[100,95,87],"active":true}"#;
        let data: Vec<u8> = (0..100).flat_map(|_| json.iter().chain(b"\n")).copied().collect();
        assert_eq!(Fingerprint::compute(&data).classify(), DataType::Structured);
    }

    #[test]
    fn detects_floats() {
        let floats: Vec<f32> = (0..10000).map(|i| 1.0 + (i as f32) * 0.001).collect();
        let data: Vec<u8> = floats.iter().flat_map(|f| f.to_le_bytes()).collect();
        let fp = Fingerprint::compute(&data);
        assert_eq!(
            fp.classify(), DataType::NumericFloat,
            "got {:?}, float_score={:.3}", fp.classify(), fp.float_score
        );
    }

    #[test]
    fn detects_ints() {
        let data: Vec<u8> = (0u32..5000).flat_map(|i| i.to_le_bytes()).collect();
        let dt = Fingerprint::compute(&data).classify();
        assert!(dt == DataType::NumericInt || dt == DataType::NumericFloat, "got {:?}", dt);
    }

    #[test]
    fn empty_data() {
        let fp = Fingerprint::compute(&[]);
        assert_eq!(fp.entropy, 0.0);
        assert_eq!(fp.unique_bytes, 0);
    }
}
