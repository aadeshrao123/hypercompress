/// Structure-Aware Columnar Delta Transform.
///
/// THIS IS NOVEL: Auto-detect repeating byte periods in binary data,
/// split into columnar byte streams, apply per-column linear prediction.
/// This exploits structure that LZ/LZMA fundamentally cannot see.
///
/// Example: A packed struct {u32 id, f32 value, u32 timestamp} = 12-byte period.
/// Column 0 (low byte of id): after delta = [1,1,1,...] (IDs increment)
/// Column 4 (low byte of value): slowly changing
/// Column 8 (low byte of timestamp): constant stride
///
/// LZMA sees these bytes interleaved and can only partially model the
/// cross-byte correlations. We make them explicit.
///
/// Format: [4-byte original_len] [2-byte period] [period × 1-byte pred_orders]
///         [column_0_residuals] [column_1_residuals] ... [remainder_bytes]

/// Encode with structure detection + columnar prediction.
pub fn encode(data: &[u8]) -> Vec<u8> {
    if data.len() < 32 {
        return encode_passthrough(data);
    }

    let period = detect_period_enhanced(data);
    if period <= 1 || period > 512 || period > data.len() / 4 {
        return encode_passthrough(data);
    }

    let num_records = data.len() / period;
    let remainder = data.len() % period;

    // Split into columns and find best prediction order per column
    let mut orders = Vec::with_capacity(period);
    let mut columns_residuals: Vec<Vec<u8>> = Vec::with_capacity(period);

    for col in 0..period {
        // Extract column
        let column: Vec<u8> = (0..num_records)
            .map(|row| data[row * period + col])
            .collect();

        // Find best prediction order for this column
        let best_order = find_best_order(&column);
        orders.push(best_order);

        // Generate prediction residuals
        let residuals = predict_encode_column(&column, best_order);
        columns_residuals.push(residuals);
    }

    // Pack result
    let mut result = Vec::new();
    result.extend_from_slice(&(data.len() as u32).to_le_bytes());
    result.extend_from_slice(&(period as u16).to_le_bytes());

    // Prediction orders (1 byte each for simplicity)
    for &order in &orders {
        result.push(order);
    }

    // Column residuals
    for col_res in &columns_residuals {
        result.extend_from_slice(col_res);
    }

    // Remainder bytes (raw)
    if remainder > 0 {
        result.extend_from_slice(&data[num_records * period..]);
    }

    // Only use if it's actually smaller than passthrough
    let passthrough = encode_passthrough(data);
    if result.len() < passthrough.len() {
        result
    } else {
        passthrough
    }
}

/// Decode: reverse the columnar split and prediction.
pub fn decode(data: &[u8]) -> Vec<u8> {
    if data.len() < 6 {
        return Vec::new();
    }

    let original_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let period = u16::from_le_bytes([data[4], data[5]]) as usize;

    if original_len == 0 {
        return Vec::new();
    }

    // Period 0 means passthrough
    if period == 0 {
        return data[6..6 + original_len.min(data.len() - 6)].to_vec();
    }

    let num_records = original_len / period;
    let remainder = original_len % period;

    let mut pos = 6;

    // Read prediction orders
    if pos + period > data.len() {
        return Vec::new();
    }
    let orders = &data[pos..pos + period];
    pos += period;

    // Decode each column
    let mut columns: Vec<Vec<u8>> = Vec::with_capacity(period);
    for col in 0..period {
        if pos + num_records > data.len() {
            break;
        }
        let col_residuals = &data[pos..pos + num_records];
        let column = predict_decode_column(col_residuals, orders[col]);
        columns.push(column);
        pos += num_records;
    }

    // Interleave columns back into records
    let mut result = Vec::with_capacity(original_len);
    for row in 0..num_records {
        for col in 0..period {
            if col < columns.len() && row < columns[col].len() {
                result.push(columns[col][row]);
            }
        }
    }

    // Append remainder
    if remainder > 0 && pos + remainder <= data.len() {
        result.extend_from_slice(&data[pos..pos + remainder]);
    }

    result.truncate(original_len);
    result
}

fn encode_passthrough(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(6 + data.len());
    result.extend_from_slice(&(data.len() as u32).to_le_bytes());
    result.extend_from_slice(&0u16.to_le_bytes()); // period=0 means passthrough
    result.extend_from_slice(data);
    result
}

/// Enhanced period detection using autocorrelation with harmonic analysis.
fn detect_period_enhanced(data: &[u8]) -> usize {
    let sample_len = 8192.min(data.len());
    let sample = &data[..sample_len];
    let max_period = 512.min(sample_len / 4);

    if max_period < 2 {
        return 0;
    }

    let mut best_period = 0;
    let mut best_score = 0.0f64;

    for period in 2..=max_period {
        let mut matches = 0u64;
        let comparisons = (sample_len - period) as u64;
        if comparisons == 0 {
            continue;
        }

        for i in 0..sample_len - period {
            if sample[i] == sample[i + period] {
                matches += 1;
            }
        }

        let score = matches as f64 / comparisons as f64;

        // Prefer smaller periods (they're more likely to be the true period)
        // A true period P will also score well at 2P, 3P, etc.
        // So we prefer the smallest period with a high score.
        let adjusted_score = score * (1.0 + 1.0 / period as f64);

        if adjusted_score > best_score && score > 0.35 {
            best_score = adjusted_score;
            best_period = period;
        }
    }

    // Verify: check if a sub-period is the true fundamental
    if best_period > 4 {
        for divisor in 2..=best_period / 2 {
            if best_period % divisor == 0 {
                let sub_period = best_period / divisor;
                if sub_period >= 2 {
                    let mut matches = 0u64;
                    let comparisons = (sample_len - sub_period) as u64;
                    if comparisons > 0 {
                        for i in 0..sample_len - sub_period {
                            if sample[i] == sample[i + sub_period] {
                                matches += 1;
                            }
                        }
                        let sub_score = matches as f64 / comparisons as f64;
                        if sub_score > 0.33 {
                            return sub_period;
                        }
                    }
                }
            }
        }
    }

    best_period
}

fn find_best_order(column: &[u8]) -> u8 {
    let mut best_order = 0u8;
    let mut best_cost = u64::MAX;

    for order in 0..=3u8 {
        let mut cost = 0u64;
        let mut prev = [0u8; 4]; // circular buffer of previous values
        for (i, &val) in column.iter().enumerate() {
            let predicted = predict_col(i, &prev, order);
            let residual = val.wrapping_sub(predicted);
            cost += residual.min(255u8.wrapping_sub(residual).wrapping_add(1)) as u64;
            // Shift history
            prev[3] = prev[2];
            prev[2] = prev[1];
            prev[1] = prev[0];
            prev[0] = val;
        }
        if cost < best_cost {
            best_cost = cost;
            best_order = order;
        }
    }

    best_order
}

fn predict_encode_column(column: &[u8], order: u8) -> Vec<u8> {
    let mut residuals = Vec::with_capacity(column.len());
    let mut prev = [0u8; 4];

    for (i, &val) in column.iter().enumerate() {
        let predicted = predict_col(i, &prev, order);
        residuals.push(val.wrapping_sub(predicted));
        prev[3] = prev[2];
        prev[2] = prev[1];
        prev[1] = prev[0];
        prev[0] = val;
    }

    residuals
}

fn predict_decode_column(residuals: &[u8], order: u8) -> Vec<u8> {
    let mut result = Vec::with_capacity(residuals.len());
    let mut prev = [0u8; 4];

    for (i, &residual) in residuals.iter().enumerate() {
        let predicted = predict_col(i, &prev, order);
        let val = residual.wrapping_add(predicted);
        result.push(val);
        prev[3] = prev[2];
        prev[2] = prev[1];
        prev[1] = prev[0];
        prev[0] = val;
    }

    result
}

#[inline]
fn predict_col(i: usize, prev: &[u8; 4], order: u8) -> u8 {
    match order {
        0 => 0,
        1 => {
            if i >= 1 { prev[0] } else { 0 }
        }
        2 => {
            if i >= 2 {
                (2u16 * prev[0] as u16).wrapping_sub(prev[1] as u16) as u8
            } else if i >= 1 {
                prev[0]
            } else {
                0
            }
        }
        3 => {
            if i >= 3 {
                (3u16 * prev[0] as u16)
                    .wrapping_sub(3u16 * prev[1] as u16)
                    .wrapping_add(prev[2] as u16) as u8
            } else if i >= 2 {
                (2u16 * prev[0] as u16).wrapping_sub(prev[1] as u16) as u8
            } else if i >= 1 {
                prev[0]
            } else {
                0
            }
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_structured() {
        // Simulated 8-byte struct: {u32 id, f32 value}
        let mut data = Vec::new();
        for i in 0u32..500 {
            data.extend_from_slice(&i.to_le_bytes());
            data.extend_from_slice(&(i as f32 * 1.5).to_le_bytes());
        }
        let encoded = encode(&data);
        let decoded = decode(&encoded);
        assert_eq!(decoded, data, "Structured roundtrip failed");
    }

    #[test]
    fn test_roundtrip_random() {
        let data: Vec<u8> = (0..5000).map(|i| ((i * 7 + 3) % 256) as u8).collect();
        let encoded = encode(&data);
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_roundtrip_small() {
        let data = vec![1, 2, 3, 4, 5];
        let encoded = encode(&data);
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_empty() {
        assert_eq!(decode(&encode(&[])), Vec::<u8>::new());
    }

    #[test]
    fn test_detects_period() {
        // Data with clear 4-byte period
        let mut data = Vec::new();
        for i in 0u32..1000 {
            data.extend_from_slice(&i.to_le_bytes());
        }
        let period = detect_period_enhanced(&data);
        assert!(period == 4 || period == 2 || period == 1,
            "Expected small period for sequential u32s, got {}", period);
    }
}
