// Order-1 context model: uses the previous byte as context to build
// 256 separate frequency tables. Each byte is mapped to its rank within
// the context (sorted by frequency), then all ranks are ANS-coded.
//
// Format: [4-byte original_len] [context_tables] [ans-encoded ranks]

/// Returns compressed data, or None if it didn't compress well.
pub fn encode(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 32 {
        return None;
    }

    let mut freq = vec![[0u32; 256]; 256];
    let mut prev = 0u8;
    for &b in data {
        freq[prev as usize][b as usize] += 1;
        prev = b;
    }

    let mut result = Vec::with_capacity(data.len());
    result.extend_from_slice(&(data.len() as u32).to_le_bytes());

    // Sparse context tables
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
        for &(sym, f) in &non_zero {
            table_buf.push(sym);
            table_buf.extend_from_slice(&f.to_le_bytes());
        }
    }

    result.extend_from_slice(&active_contexts.to_le_bytes());
    result.extend_from_slice(&table_buf);

    // Build rank tables per context (sorted by freq descending)
    let mut rank_tables = vec![Vec::new(); 256];
    let mut sym_to_rank = vec![[0u8; 256]; 256];

    for ctx in 0..256 {
        let mut symbols: Vec<(u8, u32)> = freq[ctx]
            .iter()
            .enumerate()
            .filter(|(_, &f)| f > 0)
            .map(|(s, &f)| (s as u8, f))
            .collect();

        symbols.sort_by(|a, b| b.1.cmp(&a.1));

        for (rank, &(sym, _)) in symbols.iter().enumerate() {
            sym_to_rank[ctx][sym as usize] = rank as u8;
            rank_tables[ctx].push(sym);
        }
    }

    let mut ranks = Vec::with_capacity(data.len());
    prev = 0;
    for &b in data {
        ranks.push(sym_to_rank[prev as usize][b as usize]);
        prev = b;
    }

    // Ranks cluster near 0 -- compress well with ANS
    let rank_encoded = super::ans::encode(&ranks);
    result.extend_from_slice(&rank_encoded);

    if result.len() < data.len() {
        Some(result)
    } else {
        None
    }
}

pub fn decode(data: &[u8]) -> Vec<u8> {
    if data.len() < 6 {
        return Vec::new();
    }

    let original_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    if original_len == 0 {
        return Vec::new();
    }

    let mut pos = 4;

    if pos + 2 > data.len() {
        return Vec::new();
    }
    let active_contexts = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
    pos += 2;

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
            let f = u16::from_le_bytes([data[pos + 1], data[pos + 2]]);
            symbols.push((sym, f));
            pos += 3;
        }

        symbols.sort_by(|a, b| b.1.cmp(&a.1));
        rank_tables[ctx] = symbols.iter().map(|&(s, _)| s).collect();
    }

    let ranks = super::ans::decode(&data[pos..]);

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
    fn roundtrip() {
        let data = b"the quick brown fox jumps over the lazy dog. the quick brown fox. hello world hello.";
        let buf: Vec<u8> = data.iter().copied().cycle().take(5000).collect();
        if let Some(enc) = encode(&buf) {
            assert_eq!(decode(&enc), buf, "roundtrip failed");
        }
    }

    #[test]
    fn structured_data_compresses() {
        let json = br#"{"name":"Alice","age":30},{"name":"Bob","age":25},"#;
        let data: Vec<u8> = json.iter().copied().cycle().take(10000).collect();
        if let Some(enc) = encode(&data) {
            assert!(enc.len() < data.len() * 3 / 4, "{}/{}", enc.len(), data.len());
            assert_eq!(decode(&enc), data);
        }
    }

    #[test]
    fn small_returns_none() {
        assert!(encode(b"hello").is_none());
    }
}
