/// rANS (range Asymmetric Numeral Systems) entropy coder.
///
/// Near-optimal entropy coding at table-lookup speed. Patent-free.
/// This version uses sparse frequency table encoding to minimize overhead.
///
/// Format: [4-byte original_len] [sparse_freq_table] [4-byte state] [encoded_data]
/// Sparse table: [1-byte num_symbols] then for each: [1-byte symbol] [2-byte freq]

const RANS_BYTE_L: u32 = 1 << 23;
const PROB_BITS: u32 = 14;
const PROB_SCALE: u32 = 1 << PROB_BITS;

pub fn encode(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return vec![0, 0, 0, 0];
    }

    // Build frequency table
    let mut freqs = [0u32; 256];
    for &b in data {
        freqs[b as usize] += 1;
    }

    let normalized = normalize_freqs(&freqs, data.len());

    // Build cumulative frequency table
    let mut cum_freqs = [0u32; 257];
    for i in 0..256 {
        cum_freqs[i + 1] = cum_freqs[i] + normalized[i];
    }

    // Encode in reverse (ANS is LIFO)
    let mut state: u32 = RANS_BYTE_L;
    let mut output_bytes: Vec<u8> = Vec::with_capacity(data.len());

    for &sym in data.iter().rev() {
        let s = sym as usize;
        let freq = normalized[s];
        let start = cum_freqs[s];

        if freq == 0 {
            continue;
        }

        // Renormalize
        while state >= (RANS_BYTE_L >> PROB_BITS) * freq << 8 {
            output_bytes.push((state & 0xFF) as u8);
            state >>= 8;
        }

        state = (state / freq) * PROB_SCALE + (state % freq) + start;
    }

    // Build result with sparse frequency table
    let mut result = Vec::with_capacity(4 + 256 * 3 + output_bytes.len() + 4);

    // Original length
    result.extend_from_slice(&(data.len() as u32).to_le_bytes());

    // Sparse frequency table: only store non-zero entries
    let non_zero: Vec<(u8, u16)> = normalized
        .iter()
        .enumerate()
        .filter(|(_, &f)| f > 0)
        .map(|(i, &f)| (i as u8, f as u16))
        .collect();

    // Use u16 for count to support up to 65535 symbols (we'll only have ≤256)
    result.extend_from_slice(&(non_zero.len() as u16).to_le_bytes());
    for &(sym, freq) in &non_zero {
        result.push(sym);
        result.extend_from_slice(&freq.to_le_bytes());
    }

    // Final state
    result.extend_from_slice(&state.to_le_bytes());

    // Encoded bytes (reversed)
    for &b in output_bytes.iter().rev() {
        result.push(b);
    }

    result
}

pub fn decode(data: &[u8]) -> Vec<u8> {
    if data.len() < 4 {
        return Vec::new();
    }

    let original_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    if original_len == 0 {
        return Vec::new();
    }

    let mut pos = 4;

    // Read sparse frequency table
    if pos + 2 > data.len() {
        return Vec::new();
    }
    let num_symbols = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
    pos += 2;

    let mut normalized = [0u32; 256];
    for _ in 0..num_symbols {
        if pos + 3 > data.len() {
            return Vec::new();
        }
        let sym = data[pos] as usize;
        let freq = u16::from_le_bytes([data[pos + 1], data[pos + 2]]) as u32;
        normalized[sym] = freq;
        pos += 3;
    }

    // Build cumulative frequency table
    let mut cum_freqs = [0u32; 257];
    for i in 0..256 {
        cum_freqs[i + 1] = cum_freqs[i] + normalized[i];
    }

    // Build lookup table
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
    if pos + 4 > data.len() {
        return Vec::new();
    }
    let mut state = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
    pos += 4;

    let encoded = &data[pos..];
    let mut enc_pos = 0;

    let mut result = Vec::with_capacity(original_len);

    for _ in 0..original_len {
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

        state = freq * (state / PROB_SCALE) + (state % PROB_SCALE) - start;

        // Renormalize
        while state < RANS_BYTE_L {
            if enc_pos < encoded.len() {
                state = (state << 8) | encoded[enc_pos] as u32;
                enc_pos += 1;
            } else {
                state <<= 8;
            }
        }
    }

    result
}

fn normalize_freqs(raw: &[u32; 256], total: usize) -> [u32; 256] {
    let mut result = [0u32; 256];
    let mut assigned = 0u32;
    let mut num_nonzero = 0u32;

    for i in 0..256 {
        if raw[i] > 0 {
            num_nonzero += 1;
            let f = ((raw[i] as u64 * PROB_SCALE as u64) / total as u64) as u32;
            result[i] = f.max(1);
            assigned += result[i];
        }
    }

    if assigned != PROB_SCALE && num_nonzero > 0 {
        let diff = PROB_SCALE as i64 - assigned as i64;
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
        // Sparse table: 4 (len) + 2 (count) + 3 (1 symbol) + 4 (state) + data ≈ ~20 bytes
        assert!(encoded.len() < 100, "encoded len={}", encoded.len());
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
    fn test_compression_ratio() {
        // Highly skewed distribution should compress well
        let mut data = vec![0u8; 9000];
        data.extend_from_slice(&vec![1u8; 1000]);
        let encoded = encode(&data);
        // Entropy ≈ 0.469 bits/byte → ~587 bytes ideal. Allow some overhead.
        assert!(
            encoded.len() < 2000,
            "Expected good compression, got {} bytes for {} input",
            encoded.len(),
            data.len()
        );
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
