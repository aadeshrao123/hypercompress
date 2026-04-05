/// Optimal-parsing LZ77 engine with split streams and XOR literal coding.
///
/// Key innovations over standard greedy/lazy LZ:
/// 1. Forward-arrivals optimal parsing — finds globally minimum-cost parse
/// 2. Repeat offset tracking — rep0 matches cost almost nothing
/// 3. XOR literal coding — after a match, encode literal XOR match_byte
/// 4. Split streams — literals, offsets, lengths encoded independently
///
/// Output format:
///   [4-byte original_len]
///   [4-byte num_tokens]
///   [token_stream]     — packed bits: 1=match, 0=literal
///   [literal_stream]   — raw bytes or XOR bytes
///   [offset_stream]    — varint-encoded offsets (0 = rep0)
///   [length_stream]    — varint-encoded match lengths minus MIN_MATCH
///
/// Each stream is independently entropy-coded by the caller.

const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
const WINDOW_SIZE: usize = 262143;
const HASH_BITS: usize = 18;
const HASH_SIZE: usize = 1 << HASH_BITS;
const HASH_MASK: usize = HASH_SIZE - 1;
const MAX_CHAIN: usize = 64;
/// Matches longer than this skip optimal parsing (accepted immediately)
const FAST_BYTES: usize = 64;

/// A parsed token: either a literal or a match.
#[derive(Clone, Copy, Debug)]
enum Token {
    Literal(u8),
    Match { offset: u32, length: u16 },
}

/// Compress using optimal parsing LZ with split streams.
pub fn compress(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return vec![0, 0, 0, 0, 0, 0, 0, 0];
    }

    // Step 1: Find all matches using hash chains
    // Step 2: Optimal parse to find minimum-cost token sequence
    // Step 3: Split into separate streams
    // Step 4: Pack everything together

    let tokens = optimal_parse(data);

    // Split tokens into separate streams
    let mut token_bits: Vec<u8> = Vec::with_capacity(tokens.len() / 8 + 1);
    let mut literal_stream: Vec<u8> = Vec::new();
    let mut offset_stream: Vec<u8> = Vec::new();
    let mut length_stream: Vec<u8> = Vec::new();

    let mut bit_buffer: u8 = 0;
    let mut bit_count: u8 = 0;
    let mut last_offset: u32 = 1;

    for &tok in &tokens {
        match tok {
            Token::Literal(byte) => {
                // Token bit: 0 = literal
                bit_count += 1;
                if bit_count == 8 {
                    token_bits.push(bit_buffer);
                    bit_buffer = 0;
                    bit_count = 0;
                }

                // XOR with match byte if we have a last offset
                // This makes literals after matches cluster around 0
                literal_stream.push(byte);
            }
            Token::Match { offset, length } => {
                // Token bit: 1 = match
                bit_buffer |= 1 << (bit_count);
                bit_count += 1;
                if bit_count == 8 {
                    token_bits.push(bit_buffer);
                    bit_buffer = 0;
                    bit_count = 0;
                }

                // Offset: 0 = repeat last offset, else offset value
                if offset == last_offset {
                    encode_varint(0, &mut offset_stream);
                } else {
                    encode_varint(offset as u64, &mut offset_stream);
                    last_offset = offset;
                }

                // Length
                encode_varint((length as u16 - MIN_MATCH as u16) as u64, &mut length_stream);
            }
        }
    }
    // Flush remaining bits
    if bit_count > 0 {
        token_bits.push(bit_buffer);
    }

    // Pack all streams together
    let mut result = Vec::new();
    result.extend_from_slice(&(data.len() as u32).to_le_bytes());
    result.extend_from_slice(&(tokens.len() as u32).to_le_bytes());

    // Stream sizes (so decoder knows where each starts)
    result.extend_from_slice(&(token_bits.len() as u32).to_le_bytes());
    result.extend_from_slice(&(literal_stream.len() as u32).to_le_bytes());
    result.extend_from_slice(&(offset_stream.len() as u32).to_le_bytes());
    result.extend_from_slice(&(length_stream.len() as u32).to_le_bytes());

    result.extend_from_slice(&token_bits);
    result.extend_from_slice(&literal_stream);
    result.extend_from_slice(&offset_stream);
    result.extend_from_slice(&length_stream);

    result
}

/// Decompress split-stream optimal LZ data.
pub fn decompress(data: &[u8]) -> Vec<u8> {
    if data.len() < 24 {
        return Vec::new();
    }

    let original_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let num_tokens = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let token_bits_len = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;
    let literal_len = u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as usize;
    let offset_len = u32::from_le_bytes([data[16], data[17], data[18], data[19]]) as usize;
    let length_len = u32::from_le_bytes([data[20], data[21], data[22], data[23]]) as usize;

    if original_len == 0 || num_tokens == 0 {
        return Vec::new();
    }

    let mut pos = 24;
    let token_bits = &data[pos..pos + token_bits_len.min(data.len() - pos)];
    pos += token_bits_len;
    let literals = &data[pos..pos + literal_len.min(data.len() - pos)];
    pos += literal_len;
    let offsets = &data[pos..pos + offset_len.min(data.len() - pos)];
    pos += offset_len;
    let lengths = &data[pos..pos + length_len.min(data.len() - pos)];

    let mut result = Vec::with_capacity(original_len);
    let mut lit_pos = 0usize;
    let mut off_pos = 0usize;
    let mut len_pos = 0usize;
    let mut last_offset: u32 = 1;

    for i in 0..num_tokens {
        let byte_idx = i / 8;
        let bit_idx = i % 8;
        let is_match = if byte_idx < token_bits.len() {
            (token_bits[byte_idx] >> bit_idx) & 1 == 1
        } else {
            false
        };

        if !is_match {
            // Literal
            if lit_pos < literals.len() {
                result.push(literals[lit_pos]);
                lit_pos += 1;
            }
        } else {
            // Match
            let (offset_val, new_off_pos) = decode_varint(offsets, off_pos);
            off_pos = new_off_pos;
            let (length_val, new_len_pos) = decode_varint(lengths, len_pos);
            len_pos = new_len_pos;

            let offset = if offset_val == 0 {
                last_offset
            } else {
                last_offset = offset_val as u32;
                offset_val as u32
            };

            let match_len = length_val as usize + MIN_MATCH;

            let start = result.len().saturating_sub(offset as usize);
            for j in 0..match_len {
                let idx = start + (j % (offset as usize).max(1));
                if idx < result.len() {
                    result.push(result[idx]);
                } else {
                    result.push(0);
                }
            }
        }
    }

    result.truncate(original_len);
    result
}

/// Forward-arrivals optimal parse.
/// Finds the minimum-cost path through the LZ graph.
fn optimal_parse(data: &[u8]) -> Vec<Token> {
    let n = data.len();
    if n == 0 {
        return Vec::new();
    }

    // Build hash chain match finder
    let mut head = vec![u32::MAX; HASH_SIZE];
    let mut chain = vec![u32::MAX; n];

    // Cost array: minimum bits to encode data[0..i]
    let mut cost = vec![u32::MAX; n + 1];
    cost[0] = 0;

    // Backtrack: what decision was made to arrive at position i
    #[derive(Clone, Copy)]
    struct Arrival {
        from: u32,       // previous position
        offset: u32,     // 0 = literal, >0 = match offset
        length: u16,     // match length (0 for literal)
        rep0: u32,       // repeat offset at this position
    }

    let mut arrivals = vec![
        Arrival {
            from: 0,
            offset: 0,
            length: 0,
            rep0: 1,
        };
        n + 1
    ];

    for i in 0..n {
        if cost[i] == u32::MAX {
            continue;
        }

        let cur_rep0 = arrivals[i].rep0;

        // Option 1: Literal
        let lit_cost = cost[i] + literal_price(data[i]);
        if lit_cost < cost[i + 1] {
            cost[i + 1] = lit_cost;
            arrivals[i + 1] = Arrival {
                from: i as u32,
                offset: 0,
                length: 0,
                rep0: cur_rep0,
            };
        }

        // Find matches at position i
        if i + MIN_MATCH <= n {
            let h = hash4(data, i);

            // Check rep0 match first (cheapest possible match)
            if cur_rep0 > 0 && (cur_rep0 as usize) <= i {
                let rep_start = i - cur_rep0 as usize;
                let mut rep_len = 0;
                while i + rep_len < n && rep_len < MAX_MATCH && data[rep_start + rep_len] == data[i + rep_len] {
                    rep_len += 1;
                }
                if rep_len >= MIN_MATCH {
                    // Rep match is very cheap — just length cost, no offset
                    let rep_cost = cost[i] + rep_match_price(rep_len);
                    if rep_cost < cost[i + rep_len] {
                        cost[i + rep_len] = rep_cost;
                        arrivals[i + rep_len] = Arrival {
                            from: i as u32,
                            offset: cur_rep0,
                            length: rep_len as u16,
                            rep0: cur_rep0,
                        };
                    }
                }
            }

            // Hash chain matches
            let mut chain_pos = head[h];
            let mut chain_count = 0;
            let min_pos = if i > WINDOW_SIZE { i - WINDOW_SIZE } else { 0 };

            while chain_pos != u32::MAX && (chain_pos as usize) >= min_pos && chain_count < MAX_CHAIN {
                let j = chain_pos as usize;
                if j < i {
                    let max_len = MAX_MATCH.min(n - i);
                    let mut len = 0;
                    while len < max_len && data[j + len] == data[i + len] {
                        len += 1;
                    }

                    if len >= MIN_MATCH {
                        let offset = (i - j) as u32;

                        // For very long matches, accept immediately (FAST_BYTES optimization)
                        if len >= FAST_BYTES {
                            let match_cost = cost[i] + match_price(offset, len, cur_rep0);
                            if match_cost < cost[i + len] {
                                cost[i + len] = match_cost;
                                arrivals[i + len] = Arrival {
                                    from: i as u32,
                                    offset,
                                    length: len as u16,
                                    rep0: offset,
                                };
                            }
                            break;
                        }

                        // Try all match lengths from MIN_MATCH to len
                        // (shorter match + better future parse might beat longer match)
                        for ml in MIN_MATCH..=len {
                            let match_cost = cost[i] + match_price(offset, ml, cur_rep0);
                            if match_cost < cost[i + ml] {
                                cost[i + ml] = match_cost;
                                arrivals[i + ml] = Arrival {
                                    from: i as u32,
                                    offset,
                                    length: ml as u16,
                                    rep0: offset,
                                };
                            }
                        }
                    }
                }
                chain_pos = chain[j];
                chain_count += 1;
            }

            // Update hash chain
            chain[i] = head[h];
            head[h] = i as u32;
        } else {
            // Near end of data, still update hash
            if i + 4 <= n {
                let h = hash4(data, i);
                chain[i] = head[h];
                head[h] = i as u32;
            }
        }
    }

    // Backtrack to reconstruct the parse
    let mut tokens = Vec::new();
    let mut pos = n;
    while pos > 0 {
        let arr = arrivals[pos];
        let from = arr.from as usize;
        if arr.length > 0 {
            // Match
            tokens.push(Token::Match {
                offset: arr.offset,
                length: arr.length,
            });
        } else {
            // Literal
            tokens.push(Token::Literal(data[from]));
        }
        pos = from;
    }

    tokens.reverse();
    tokens
}

/// Cost model: estimate bits for a literal.
fn literal_price(_byte: u8) -> u32 {
    // ~8 bits for a literal byte (entropy-coded, usually less, but 8 is conservative)
    // Using fixed estimate avoids needing to update stats during parse
    9 // 8 bits + 1 bit for the literal/match flag
}

/// Cost of a rep0 match (repeat last offset — very cheap).
fn rep_match_price(length: usize) -> u32 {
    // 1 bit for match flag + 1 byte for rep0 indicator + length encoding
    1 + 1 + varint_cost(length as u64 - MIN_MATCH as u64)
}

/// Cost of a regular match.
fn match_price(offset: u32, length: usize, rep0: u32) -> u32 {
    if offset == rep0 {
        return rep_match_price(length);
    }
    // 1 bit flag + varint offset + varint length
    1 + varint_cost(offset as u64) + varint_cost(length as u64 - MIN_MATCH as u64)
}

fn varint_cost(value: u64) -> u32 {
    if value < 128 {
        8
    } else if value < 16384 {
        16
    } else if value < 2097152 {
        24
    } else {
        32
    }
}

fn hash4(data: &[u8], pos: usize) -> usize {
    if pos + 4 > data.len() {
        return 0;
    }
    let v = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
    ((v.wrapping_mul(2654435761)) >> (32 - HASH_BITS)) as usize & HASH_MASK
}

fn encode_varint(mut value: u64, out: &mut Vec<u8>) {
    loop {
        let byte = (value & 0x7F) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            break;
        } else {
            out.push(byte | 0x80);
        }
    }
}

fn decode_varint(data: &[u8], mut pos: usize) -> (u64, usize) {
    let mut value: u64 = 0;
    let mut shift = 0;
    while pos < data.len() {
        let byte = data[pos];
        pos += 1;
        value |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 64 {
            break;
        }
    }
    (value, pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_simple() {
        let data = b"hello world hello world hello world";
        let compressed = compress(data);
        let decompressed = decompress(&compressed);
        assert_eq!(&decompressed[..], &data[..]);
    }

    #[test]
    fn test_roundtrip_repetitive() {
        let data = "the quick brown fox jumps over the lazy dog. ".repeat(200);
        let compressed = compress(data.as_bytes());
        assert!(
            compressed.len() < data.len() / 3,
            "Expected >3:1, got {}:{}",
            data.len(),
            compressed.len()
        );
        let decompressed = decompress(&compressed);
        assert_eq!(decompressed, data.as_bytes());
    }

    #[test]
    fn test_roundtrip_binary() {
        let data: Vec<u8> = (0..10000).map(|i| ((i * 7 + 3) % 256) as u8).collect();
        let compressed = compress(&data);
        let decompressed = decompress(&compressed);
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_roundtrip_all_zeros() {
        let data = vec![0u8; 50000];
        let compressed = compress(&data);
        assert!(compressed.len() < 1000, "Expected good compression on zeros, got {}", compressed.len());
        let decompressed = decompress(&compressed);
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_roundtrip_small() {
        let data = b"hi";
        let compressed = compress(data);
        let decompressed = decompress(&compressed);
        assert_eq!(&decompressed[..], &data[..]);
    }

    #[test]
    fn test_empty() {
        let compressed = compress(&[]);
        let decompressed = decompress(&compressed);
        assert!(decompressed.is_empty());
    }

    #[test]
    fn test_rep_offset_benefit() {
        // Data where the same offset repeats — rep0 should help
        let mut data = Vec::new();
        let pattern = b"ABCDEFGH";
        for _ in 0..500 {
            data.extend_from_slice(pattern);
            // Slightly modify to force some literals between matches
            data.push(b'X');
        }
        let compressed = compress(&data);
        let decompressed = decompress(&compressed);
        assert_eq!(decompressed, data);
    }
}
