/// rANS (range Asymmetric Numeral Systems) entropy coder.
///
/// Near-optimal entropy coding (within ~0.01 bits of Shannon entropy)
/// at table-lookup speed. Patent-free, SIMD-friendly.
///
/// This implementation uses 32-bit state with 16-bit renormalization.
///
/// Format: [4-byte original_len] [256×4-byte frequency table] [encoded_data]

const RANS_BYTE_L: u32 = 1 << 23; // Lower bound of state
const PROB_BITS: u32 = 14;
const PROB_SCALE: u32 = 1 << PROB_BITS;

/// Encode data using rANS.
pub fn encode(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return vec![0, 0, 0, 0];
    }

    // Build frequency table
    let mut freqs = [0u32; 256];
    for &b in data {
        freqs[b as usize] += 1;
    }

    // Normalize frequencies to sum to PROB_SCALE
    let normalized = normalize_freqs(&freqs, data.len());

    // Build cumulative frequency table
    let mut cum_freqs = [0u32; 257];
    for i in 0..256 {
        cum_freqs[i + 1] = cum_freqs[i] + normalized[i];
    }

    // Encode in reverse order (ANS is LIFO)
    let mut state: u32 = RANS_BYTE_L;
    let mut output_bytes: Vec<u8> = Vec::with_capacity(data.len());

    for &sym in data.iter().rev() {
        let s = sym as usize;
        let freq = normalized[s];
        let start = cum_freqs[s];

        if freq == 0 {
            // Symbol shouldn't appear if freq is 0, but handle gracefully
            continue;
        }

        // Renormalize: push bytes out if state is too large
        while state >= (RANS_BYTE_L >> PROB_BITS) * freq << 8 {
            output_bytes.push((state & 0xFF) as u8);
            state >>= 8;
        }

        // Encode symbol
        state = (state / freq) * PROB_SCALE + (state % freq) + start;
    }

    // Flush final state (4 bytes)
    let mut result = Vec::with_capacity(4 + 256 * 4 + output_bytes.len() + 4);

    // Header: original length
    result.extend_from_slice(&(data.len() as u32).to_le_bytes());

    // Frequency table (needed for decoding)
    for &f in &normalized {
        result.extend_from_slice(&f.to_le_bytes());
    }

    // Final state
    result.extend_from_slice(&state.to_le_bytes());

    // Encoded bytes (in reverse since we built them backwards)
    for &b in output_bytes.iter().rev() {
        result.push(b);
    }

    result
}

/// Decode rANS-encoded data.
pub fn decode(data: &[u8]) -> Vec<u8> {
    let header_size = 4 + 256 * 4 + 4; // original_len + freq_table + state

    if data.len() < header_size {
        return Vec::new();
    }

    let original_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    if original_len == 0 {
        return Vec::new();
    }

    // Read frequency table
    let mut normalized = [0u32; 256];
    for i in 0..256 {
        let offset = 4 + i * 4;
        normalized[i] = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
    }

    // Build cumulative frequency table
    let mut cum_freqs = [0u32; 257];
    for i in 0..256 {
        cum_freqs[i + 1] = cum_freqs[i] + normalized[i];
    }

    // Build lookup table for fast symbol finding
    let mut sym_table = vec![0u8; PROB_SCALE as usize];
    for s in 0..256u16 {
        let start = cum_freqs[s as usize] as usize;
        let end = cum_freqs[s as usize + 1] as usize;
        for slot in start..end {
            if slot < sym_table.len() {
                sym_table[slot] = s as u8;
            }
        }
    }

    // Read initial state
    let state_offset = 4 + 256 * 4;
    let mut state = u32::from_le_bytes([
        data[state_offset],
        data[state_offset + 1],
        data[state_offset + 2],
        data[state_offset + 3],
    ]);

    let encoded = &data[header_size..];
    let mut enc_pos = 0;

    let mut result = Vec::with_capacity(original_len);

    for _ in 0..original_len {
        // Find symbol
        let slot = (state % PROB_SCALE) as usize;
        if slot >= sym_table.len() {
            break;
        }
        let sym = sym_table[slot];
        result.push(sym);

        let s = sym as usize;
        let freq = normalized[s];
        let start = cum_freqs[s];

        if freq == 0 {
            break;
        }

        // Decode step
        state = freq * (state / PROB_SCALE) + (state % PROB_SCALE) - start;

        // Renormalize: read bytes in if state is too small
        while state < RANS_BYTE_L {
            if enc_pos < encoded.len() {
                state = (state << 8) | encoded[enc_pos] as u32;
                enc_pos += 1;
            } else {
                state = state << 8;
            }
        }
    }

    result
}

/// Normalize raw frequencies to sum to PROB_SCALE.
/// Ensures no symbol with count > 0 gets frequency 0.
fn normalize_freqs(raw: &[u32; 256], total: usize) -> [u32; 256] {
    let mut result = [0u32; 256];
    let mut assigned = 0u32;
    let mut num_nonzero = 0u32;

    for i in 0..256 {
        if raw[i] > 0 {
            num_nonzero += 1;
            // Scale proportionally
            let f = ((raw[i] as u64 * PROB_SCALE as u64) / total as u64) as u32;
            result[i] = f.max(1); // Ensure at least 1
            assigned += result[i];
        }
    }

    // Adjust to exactly sum to PROB_SCALE
    if assigned != PROB_SCALE && num_nonzero > 0 {
        let diff = PROB_SCALE as i64 - assigned as i64;
        // Add/subtract from the most frequent symbol
        let max_idx = raw
            .iter()
            .enumerate()
            .filter(|(_, &c)| c > 0)
            .max_by_key(|(_, &c)| c)
            .map(|(i, _)| i)
            .unwrap_or(0);

        let new_val = result[max_idx] as i64 + diff;
        if new_val > 0 {
            result[max_idx] = new_val as u32;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_simple() {
        let data = b"hello world hello hello world";
        let encoded = encode(data);
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_roundtrip_repeated() {
        let data = vec![42u8; 1000];
        let encoded = encode(&data);
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);

        // Should compress significantly (single symbol → near 0 bits each)
        // Note: overhead includes 1028-byte header (4 len + 1024 freq table + 4 state)
        assert!(encoded.len() < 1100);
    }

    #[test]
    fn test_roundtrip_all_bytes() {
        let data: Vec<u8> = (0..=255).collect();
        let encoded = encode(&data);
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_roundtrip_larger() {
        let data: Vec<u8> = (0..10000).map(|i| (i % 10) as u8).collect();
        let encoded = encode(&data);
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_empty() {
        let encoded = encode(&[]);
        let decoded = decode(&encoded);
        assert_eq!(decoded, Vec::<u8>::new());
    }
}
