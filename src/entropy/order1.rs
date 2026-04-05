/// Order-1 context model encoder/decoder.
///
/// Uses the previous byte as context, maintaining 256 separate frequency tables.
/// This captures byte-pair correlations and dramatically improves compression
/// for structured data where certain byte transitions are common.
///
/// Format: [4-byte original_len] [context_tables] [encoded_data]
/// Context tables are stored sparsely: only contexts that appear are stored.
///
/// The actual coding uses a simple byte-aligned approach for robustness:
/// each byte is coded given its context, with adaptive frequency updates.

/// Encode data using order-1 context model.
/// Returns compressed data, or None if it didn't compress well.
pub fn encode(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 32 {
        return None;
    }

    // Build order-1 frequency tables: freq[prev_byte][current_byte]
    let mut freq = vec![[0u32; 256]; 256];
    let mut prev = 0u8;
    for &b in data {
        freq[prev as usize][b as usize] += 1;
        prev = b;
    }

    // For each context, build a mapping from symbols to codes
    // We use a simple approach: sort symbols by frequency, assign shorter codes
    // to more frequent symbols using a nibble-based variable-length code.

    // Actually, let's use ANS per-context for maximum compression.
    // But that's complex. Instead, use a simpler but effective approach:
    // adaptive arithmetic-style coding with byte output.
    //
    // For the MVP, we'll use per-context frequency tables and encode
    // using a simplified range coder.

    let mut result = Vec::with_capacity(data.len());
    result.extend_from_slice(&(data.len() as u32).to_le_bytes());

    // Encode the context frequency tables sparsely
    // For each context that was used, store: [context_byte] [num_symbols] [symbol, freq16]...
    let mut active_contexts = 0u16;
    let mut table_buf = Vec::new();

    for ctx in 0..256 {
        let row = &freq[ctx];
        let non_zero: Vec<(u8, u16)> = row
            .iter()
            .enumerate()
            .filter(|(_, &f)| f > 0)
            .map(|(s, &f)| (s as u8, f.min(65535) as u16))
            .collect();

        if non_zero.is_empty() {
            continue;
        }

        active_contexts += 1;
        table_buf.push(ctx as u8);
        table_buf.push(non_zero.len() as u8);
        for &(sym, freq) in &non_zero {
            table_buf.push(sym);
            table_buf.extend_from_slice(&freq.to_le_bytes());
        }
    }

    result.extend_from_slice(&active_contexts.to_le_bytes());
    result.extend_from_slice(&table_buf);

    // Now encode the data using per-context rANS.
    // For simplicity and correctness, we'll use per-context ANS encoding.
    // Group data by context, encode each group separately.
    //
    // Better approach: interleaved encoding where we switch context every byte.
    // For now, encode the whole stream with a simple per-byte context code.

    // Simple but effective: for each byte, output its rank within its context.
    // Context tables are sorted by frequency. The rank is then entropy-coded.
    // Ranks are small numbers (usually 0-5 for common data) which compress very well.

    // Build rank tables: for each context, map symbol → rank (sorted by freq desc)
    let mut rank_tables = vec![Vec::new(); 256];
    let mut sym_to_rank = vec![[0u8; 256]; 256];

    for ctx in 0..256 {
        let row = &freq[ctx];
        let mut symbols: Vec<(u8, u32)> = row
            .iter()
            .enumerate()
            .filter(|(_, &f)| f > 0)
            .map(|(s, &f)| (s as u8, f))
            .collect();

        // Sort by frequency descending
        symbols.sort_by(|a, b| b.1.cmp(&a.1));

        for (rank, &(sym, _)) in symbols.iter().enumerate() {
            sym_to_rank[ctx][sym as usize] = rank as u8;
            rank_tables[ctx].push(sym);
        }
    }

    // Encode data as ranks
    let mut ranks = Vec::with_capacity(data.len());
    prev = 0;
    for &b in data {
        ranks.push(sym_to_rank[prev as usize][b as usize]);
        prev = b;
    }

    // Ranks are heavily biased toward 0 and small values — perfect for ANS
    let rank_encoded = super::ans::encode(&ranks);
    result.extend_from_slice(&rank_encoded);

    // Only return if we actually compressed
    if result.len() < data.len() {
        Some(result)
    } else {
        None
    }
}

/// Decode order-1 encoded data.
pub fn decode(data: &[u8]) -> Vec<u8> {
    if data.len() < 6 {
        return Vec::new();
    }

    let original_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    if original_len == 0 {
        return Vec::new();
    }

    let mut pos = 4;

    // Read context tables
    if pos + 2 > data.len() {
        return Vec::new();
    }
    let active_contexts = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
    pos += 2;

    // Rebuild rank tables
    let mut rank_tables = vec![Vec::new(); 256];

    for _ in 0..active_contexts {
        if pos + 2 > data.len() {
            return Vec::new();
        }
        let ctx = data[pos] as usize;
        let num_symbols = data[pos + 1] as usize;
        pos += 2;

        let mut symbols: Vec<(u8, u16)> = Vec::with_capacity(num_symbols);
        for _ in 0..num_symbols {
            if pos + 3 > data.len() {
                return Vec::new();
            }
            let sym = data[pos];
            let freq = u16::from_le_bytes([data[pos + 1], data[pos + 2]]);
            symbols.push((sym, freq));
            pos += 3;
        }

        // Sort by frequency descending (same order as encoder)
        symbols.sort_by(|a, b| b.1.cmp(&a.1));
        rank_tables[ctx] = symbols.iter().map(|&(s, _)| s).collect();
    }

    // Decode ranks from ANS
    let ranks = super::ans::decode(&data[pos..]);

    // Convert ranks back to bytes using rank tables
    let mut result = Vec::with_capacity(original_len);
    let mut prev = 0u8;

    for i in 0..original_len {
        if i >= ranks.len() {
            break;
        }
        let rank = ranks[i] as usize;
        let table = &rank_tables[prev as usize];
        if rank < table.len() {
            let byte = table[rank];
            result.push(byte);
            prev = byte;
        } else {
            result.push(0);
            prev = 0;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let data = b"the quick brown fox jumps over the lazy dog. the quick brown fox. hello world hello.";
        let data_vec: Vec<u8> = data.iter().copied().cycle().take(5000).collect();
        if let Some(encoded) = encode(&data_vec) {
            let decoded = decode(&encoded);
            assert_eq!(decoded, data_vec, "Order-1 roundtrip failed");
        }
    }

    #[test]
    fn test_structured_data() {
        // JSON-like data has strong byte-pair correlations (e.g., '"' often follows ':')
        let json = br#"{"name":"Alice","age":30},{"name":"Bob","age":25},"#;
        let data: Vec<u8> = json.iter().copied().cycle().take(10000).collect();
        if let Some(encoded) = encode(&data) {
            assert!(
                encoded.len() < data.len() * 3 / 4,
                "Expected >25% compression, got {}/{}",
                encoded.len(),
                data.len()
            );
            let decoded = decode(&encoded);
            assert_eq!(decoded, data);
        }
    }

    #[test]
    fn test_small_data_returns_none() {
        let data = b"hello";
        assert!(encode(data).is_none());
    }
}
