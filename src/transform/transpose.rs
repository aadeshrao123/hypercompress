/// Column transposition transform: converts row-major data to column-major.
///
/// This is the AoS → SoA (Array-of-Structs to Struct-of-Arrays) transform.
/// For structured data with fixed-size records (CSV rows, database records,
/// protocol buffers), transposing makes each "column" a contiguous stream
/// of similar values, dramatically improving compression.
///
/// We auto-detect the record width by finding the most common line length
/// or by looking for repeating patterns.
///
/// Format: [4-byte original_len] [2-byte record_width] [transposed_data]

/// Encode: detect record width and transpose.
pub fn encode(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return vec![0, 0, 0, 0, 0, 0];
    }

    let record_width = detect_record_width(data);

    // If we couldn't detect a useful width, store as-is
    if record_width <= 1 || record_width > data.len() {
        let mut result = Vec::with_capacity(6 + data.len());
        result.extend_from_slice(&(data.len() as u32).to_le_bytes());
        result.extend_from_slice(&1u16.to_le_bytes()); // width=1 means no transpose
        result.extend_from_slice(data);
        return result;
    }

    let num_records = data.len() / record_width;
    let remainder = data.len() % record_width;

    // Transpose: for each column position, gather that byte from all records
    let mut transposed = Vec::with_capacity(num_records * record_width);
    for col in 0..record_width {
        for row in 0..num_records {
            transposed.push(data[row * record_width + col]);
        }
    }

    let mut result = Vec::with_capacity(6 + transposed.len() + remainder);
    result.extend_from_slice(&(data.len() as u32).to_le_bytes());
    result.extend_from_slice(&(record_width as u16).to_le_bytes());
    result.extend_from_slice(&transposed);
    // Append any leftover bytes that didn't fit a full record
    if remainder > 0 {
        result.extend_from_slice(&data[num_records * record_width..]);
    }

    result
}

/// Decode: reverse the transposition.
pub fn decode(data: &[u8]) -> Vec<u8> {
    if data.len() < 6 {
        return Vec::new();
    }

    let original_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let record_width = u16::from_le_bytes([data[4], data[5]]) as usize;
    let payload = &data[6..];

    // Width=1 means no transpose was applied
    if record_width <= 1 {
        return payload[..original_len.min(payload.len())].to_vec();
    }

    let num_records = original_len / record_width;
    let remainder = original_len % record_width;
    let transposed_len = num_records * record_width;

    if payload.len() < transposed_len + remainder {
        return payload.to_vec();
    }

    let transposed = &payload[..transposed_len];

    // Reverse transpose
    let mut result = vec![0u8; original_len];
    for col in 0..record_width {
        for row in 0..num_records {
            result[row * record_width + col] = transposed[col * num_records + row];
        }
    }

    // Copy remainder bytes
    if remainder > 0 {
        let rem_start = num_records * record_width;
        result[rem_start..].copy_from_slice(&payload[transposed_len..transposed_len + remainder]);
    }

    result
}

/// Detect the most likely record width in the data.
/// Looks for newline-delimited records first, then tries period detection.
fn detect_record_width(data: &[u8]) -> usize {
    // Strategy 1: Look for newline-delimited records
    let newline_width = detect_newline_records(data);
    if newline_width > 1 {
        return newline_width;
    }

    // Strategy 2: Auto-correlation to find repeating period
    detect_period(data)
}

/// Find the most common line length (distance between newlines).
fn detect_newline_records(data: &[u8]) -> usize {
    let mut lengths = Vec::new();
    let mut last_nl = 0usize;

    for (i, &b) in data.iter().enumerate() {
        if b == b'\n' {
            let len = i - last_nl + 1; // include the newline
            if len > 1 && len <= 1024 {
                lengths.push(len);
            }
            last_nl = i + 1;
        }
    }

    if lengths.len() < 4 {
        return 0;
    }

    // Find most common length
    lengths.sort_unstable();
    let mut best_len = 0;
    let mut best_count = 0;
    let mut current_len = lengths[0];
    let mut current_count = 1;

    for &len in &lengths[1..] {
        if len == current_len {
            current_count += 1;
        } else {
            if current_count > best_count {
                best_count = current_count;
                best_len = current_len;
            }
            current_len = len;
            current_count = 1;
        }
    }
    if current_count > best_count {
        best_count = current_count;
        best_len = current_len;
    }

    // At least 50% of lines should have the same length for it to be useful
    if best_count * 2 >= lengths.len() {
        best_len
    } else {
        0
    }
}

/// Auto-correlation period detection for binary structured data.
fn detect_period(data: &[u8]) -> usize {
    if data.len() < 64 {
        return 0;
    }

    let max_period = 256.min(data.len() / 4);
    let sample_len = 1024.min(data.len());
    let sample = &data[..sample_len];

    let mut best_period = 0;
    let mut best_score = 0u64;

    for period in 2..=max_period {
        let mut matches = 0u64;
        let comparisons = sample_len - period;
        for i in 0..comparisons {
            if sample[i] == sample[i + period] {
                matches += 1;
            }
        }

        // Normalize by number of comparisons
        let score = matches * 1000 / comparisons as u64;
        if score > best_score && score > 300 {
            // >30% match rate at this period
            best_score = score;
            best_period = period;
        }
    }

    best_period
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_fixed_records() {
        // Simulated CSV-like fixed-width records
        let mut data = Vec::new();
        for i in 0..20 {
            data.extend_from_slice(format!("ROW{:03},VAL{:03}\n", i, i * 2).as_bytes());
        }
        let encoded = encode(&data);
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_roundtrip_binary_records() {
        // 8-byte records: [u32 id, f32 value]
        let mut data = Vec::new();
        for i in 0u32..100 {
            data.extend_from_slice(&i.to_le_bytes());
            data.extend_from_slice(&(i as f32 * 1.5).to_le_bytes());
        }
        let encoded = encode(&data);
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_no_transpose_small() {
        let data = vec![1, 2, 3];
        let encoded = encode(&data);
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_empty() {
        let decoded = decode(&encode(&[]));
        assert_eq!(decoded, Vec::<u8>::new());
    }
}
