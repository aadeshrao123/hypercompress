pub mod ans;
pub mod lz_optimal;
pub mod order1;

use crate::format::CodecType;

/// Encode data using the specified codec.
pub fn encode(data: &[u8], codec: CodecType) -> Vec<u8> {
    match codec {
        CodecType::Raw => data.to_vec(),
        CodecType::Ans => ans::encode(data),
        CodecType::Lz => lz_encode(data),
        CodecType::LzAns => {
            let lz_out = lz_encode(data);
            if lz_out.len() < data.len() {
                let ans_out = ans::encode(&lz_out);
                if ans_out.len() < lz_out.len() {
                    return ans_out;
                }
                return lz_out;
            }
            let ans_out = ans::encode(data);
            if ans_out.len() < data.len() {
                return ans_out;
            }
            data.to_vec()
        }
        CodecType::Order1 => {
            order1::encode(data).unwrap_or_else(|| data.to_vec())
        }
        CodecType::LzOptimal => lz_optimal::compress(data),
        CodecType::Lzma => lzma_compress(data),
    }
}

/// Decode data using the specified codec.
pub fn decode(data: &[u8], codec: CodecType) -> Vec<u8> {
    match codec {
        CodecType::Raw => data.to_vec(),
        CodecType::Ans => ans::decode(data),
        CodecType::Lz => lz_decode(data),
        CodecType::LzAns => {
            let ans_dec = ans::decode(data);
            lz_decode(&ans_dec)
        }
        CodecType::Order1 => order1::decode(data),
        CodecType::LzOptimal => lz_optimal::decompress(data),
        CodecType::Lzma => lzma_decompress(data),
    }
}

/// LZMA compression via xz2 (liblzma bindings — the real deal).
fn lzma_compress(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut encoder = xz2::write::XzEncoder::new(Vec::new(), 6);
    if encoder.write_all(data).is_ok() {
        if let Ok(output) = encoder.finish() {
            return output;
        }
    }
    data.to_vec()
}

/// LZMA decompression via xz2.
fn lzma_decompress(data: &[u8]) -> Vec<u8> {
    use std::io::Read;
    let mut decoder = xz2::read::XzDecoder::new(data);
    let mut output = Vec::new();
    if decoder.read_to_end(&mut output).is_ok() {
        output
    } else {
        data.to_vec()
    }
}

/// Select the best codec for the given (already-transformed) data.
/// Tries multiple options and returns the one that compresses best.
pub fn select_best_codec(data: &[u8]) -> (CodecType, Vec<u8>) {
    if data.is_empty() {
        return (CodecType::Raw, Vec::new());
    }

    // Quick entropy check — skip everything if data is random
    let entropy = quick_entropy(data);
    if entropy > 7.8 {
        return (CodecType::Raw, data.to_vec());
    }

    let mut best_codec = CodecType::Raw;
    let mut best_data = data.to_vec();
    let mut best_size = data.len();

    // Try ANS
    let ans_out = ans::encode(data);
    if ans_out.len() < best_size {
        best_size = ans_out.len();
        best_codec = CodecType::Ans;
        best_data = ans_out;
    }

    // Try LZ
    let lz_out = lz_encode(data);
    let lz_helped = lz_out.len() < data.len();
    if lz_out.len() < best_size {
        best_size = lz_out.len();
        best_codec = CodecType::Lz;
        best_data = lz_out.clone();
    }

    // Try LZ+ANS pipeline (best for most data)
    if lz_helped {
        let lzans_out = ans::encode(&lz_out);
        if lzans_out.len() < best_size {
            best_size = lzans_out.len();
            best_codec = CodecType::LzAns;
            best_data = lzans_out;
        }
    }

    // Try Order-1 context model (best for structured/text data)
    if data.len() >= 64 {
        if let Some(o1_out) = order1::encode(data) {
            if o1_out.len() < best_size {
                best_size = o1_out.len();
                best_codec = CodecType::Order1;
                best_data = o1_out;
            }
        }
    }

    // Try optimal-parsing LZ (best overall for binary/mixed data)
    if data.len() >= 32 {
        let opt_out = lz_optimal::compress(data);
        if opt_out.len() < best_size {
            best_size = opt_out.len();
            best_codec = CodecType::LzOptimal;
            best_data = opt_out;
        }
    }

    // Try LZMA — the heavyweight champion. Our transforms run FIRST,
    // so LZMA compresses pre-transformed data = better than standalone LZMA.
    if data.len() >= 32 {
        let lzma_out = lzma_compress(data);
        if lzma_out.len() < best_size {
            best_size = lzma_out.len();
            best_codec = CodecType::Lzma;
            best_data = lzma_out;
        }
    }

    let _ = best_size;
    (best_codec, best_data)
}

fn quick_entropy(data: &[u8]) -> f64 {
    let mut histogram = [0u32; 256];
    for &b in data {
        histogram[b as usize] += 1;
    }
    let len = data.len() as f64;
    let mut entropy = 0.0;
    for &count in &histogram {
        if count > 0 {
            let p = count as f64 / len;
            entropy -= p * p.log2();
        }
    }
    entropy
}

// ============================================================================
// Hash-chain LZ77 encoder — O(n) average with fast hash lookups
// ============================================================================
//
// Format: stream of tokens, each starting with a control byte:
//   Bit 7 = 0: Literal run
//     Bits 0-6 = literal count - 1 (1..128 literals follow)
//   Bit 7 = 1: Match
//     Bits 0-6 + next byte = offset (15-bit, 1..32768)
//     Next byte = match length - 3 (0..255 → 3..258)
//
// This is much more compact than the old flag-per-byte format.

const HASH_BITS: usize = 18;
const HASH_SIZE: usize = 1 << HASH_BITS;
const HASH_MASK: usize = HASH_SIZE - 1;
const MAX_CHAIN: usize = 256; // deep search for best matches
const WINDOW_SIZE: usize = 262143; // 18-bit offset — covers 256KB chunks
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;

fn hash4(data: &[u8], pos: usize) -> usize {
    if pos + 4 > data.len() {
        return 0;
    }
    let v = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
    // Knuth multiplicative hash
    ((v.wrapping_mul(2654435761)) >> (32 - HASH_BITS)) as usize & HASH_MASK
}

fn lz_encode(data: &[u8]) -> Vec<u8> {
    if data.len() < 8 {
        // Too small for LZ, store as literal run
        return encode_literals(data);
    }

    let mut result = Vec::with_capacity(data.len());

    // Hash table: maps hash → most recent position
    let mut head = vec![0u32; HASH_SIZE]; // head of chain
    let mut chain = vec![0u32; data.len()]; // chain links
    // Initialize heads to a sentinel
    for h in head.iter_mut() {
        *h = u32::MAX;
    }

    let mut i = 0;
    let mut literal_start = 0;

    while i < data.len() {
        if i + MIN_MATCH > data.len() {
            break;
        }

        let h = hash4(data, i);
        let mut best_len = 0usize;
        let mut best_offset = 0usize;

        // Search the hash chain
        let mut chain_pos = head[h];
        let mut chain_count = 0;
        let min_pos = if i > WINDOW_SIZE { i - WINDOW_SIZE } else { 0 };

        while chain_pos != u32::MAX && (chain_pos as usize) >= min_pos && chain_count < MAX_CHAIN {
            let j = chain_pos as usize;
            if j < i {
                // Compare
                let max_len = MAX_MATCH.min(data.len() - i);
                let mut len = 0;
                while len < max_len && data[j + len] == data[i + len] {
                    len += 1;
                }
                if len >= MIN_MATCH && len > best_len {
                    best_len = len;
                    best_offset = i - j;
                    if len == max_len {
                        break;
                    }
                }
            }
            chain_pos = chain[j];
            chain_count += 1;
        }

        // Update hash chain
        chain[i] = head[h];
        head[h] = i as u32;

        if best_len >= MIN_MATCH {
            // LAZY MATCHING: check if position i+1 has an even longer match.
            // If so, emit i as a literal and use the longer match at i+1.
            // This is the key optimization gzip -9 uses.
            let mut use_match = true;
            if best_len < MAX_MATCH && i + 1 + MIN_MATCH <= data.len() {
                let h2 = hash4(data, i + 1);
                let mut lazy_len = 0usize;
                let mut chain_pos2 = head[h2];
                let mut chain_count2 = 0;
                let min_pos2 = if i + 1 > WINDOW_SIZE { i + 1 - WINDOW_SIZE } else { 0 };

                while chain_pos2 != u32::MAX && (chain_pos2 as usize) >= min_pos2 && chain_count2 < MAX_CHAIN / 2 {
                    let j2 = chain_pos2 as usize;
                    if j2 < i + 1 {
                        let max_len2 = MAX_MATCH.min(data.len() - (i + 1));
                        let mut len2 = 0;
                        while len2 < max_len2 && data[j2 + len2] == data[i + 1 + len2] {
                            len2 += 1;
                        }
                        if len2 > lazy_len {
                            lazy_len = len2;
                        }
                    }
                    chain_pos2 = chain[j2];
                    chain_count2 += 1;
                }

                // If lazy match at i+1 is significantly longer, skip current match
                if lazy_len > best_len + 1 {
                    use_match = false;
                }
            }

            if use_match {
                // Flush pending literals
                if i > literal_start {
                    emit_literals(&data[literal_start..i], &mut result);
                }

                // Emit match
                emit_match(best_offset, best_len, &mut result);

                // Update hash for skipped positions
                for k in 1..best_len {
                    if i + k + 4 <= data.len() {
                        let hk = hash4(data, i + k);
                        chain[i + k] = head[hk];
                        head[hk] = (i + k) as u32;
                    }
                }

                i += best_len;
                literal_start = i;
            } else {
                // Skip this match, emit as literal, try again at i+1
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    // Flush remaining literals
    if literal_start < data.len() {
        emit_literals(&data[literal_start..data.len()], &mut result);
    }

    result
}

fn encode_literals(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(data.len() + (data.len() / 128) + 1);
    emit_literals(data, &mut result);
    result
}

fn emit_literals(literals: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < literals.len() {
        let count = (literals.len() - i).min(128);
        out.push((count - 1) as u8); // bit 7 = 0, bits 0-6 = count-1
        out.extend_from_slice(&literals[i..i + count]);
        i += count;
    }
}

fn emit_match(offset: usize, length: usize, out: &mut Vec<u8>) {
    debug_assert!(offset >= 1 && offset <= WINDOW_SIZE);
    debug_assert!(length >= MIN_MATCH && length <= MAX_MATCH);

    let offset_minus1 = offset - 1;

    if offset_minus1 < 0x7F00 {
        // Short match: 3 bytes total — ctrl(1) + offset_lo(1) + length(1)
        out.push(0x80 | ((offset_minus1 >> 8) as u8 & 0x7F));
        out.push((offset_minus1 & 0xFF) as u8);
        out.push((length - MIN_MATCH) as u8);
    } else {
        // Long match: 5 bytes total — 0xFF marker + offset(3 bytes LE) + length(1)
        out.push(0xFF);
        out.push((offset_minus1 & 0xFF) as u8);
        out.push(((offset_minus1 >> 8) & 0xFF) as u8);
        out.push(((offset_minus1 >> 16) & 0xFF) as u8);
        out.push((length - MIN_MATCH) as u8);
    }
}

fn lz_decode(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    let mut i = 0;

    while i < data.len() {
        let ctrl = data[i];
        i += 1;

        if ctrl & 0x80 == 0 {
            // Literal run: count = (ctrl & 0x7F) + 1
            let count = (ctrl & 0x7F) as usize + 1;
            if i + count > data.len() {
                break;
            }
            result.extend_from_slice(&data[i..i + count]);
            i += count;
        } else if ctrl == 0xFF {
            // Extended match: 3-byte LE offset + 1-byte length
            if i + 4 > data.len() {
                break;
            }
            let offset = (data[i] as usize)
                | ((data[i + 1] as usize) << 8)
                | ((data[i + 2] as usize) << 16);
            let offset = offset + 1;
            let length = data[i + 3] as usize + MIN_MATCH;
            i += 4;

            let start = result.len().saturating_sub(offset);
            for j in 0..length {
                let idx = start + (j % offset);
                if idx < result.len() {
                    result.push(result[idx]);
                } else {
                    result.push(0);
                }
            }
        } else {
            // Normal match: ctrl has bit 7 set, bits 0-6 = high offset bits
            if i + 2 > data.len() {
                break;
            }
            let offset_hi = (ctrl & 0x7F) as u16;
            let offset_lo = data[i] as u16;
            let offset = ((offset_hi << 8) | offset_lo) as usize + 1;
            let length = data[i + 1] as usize + MIN_MATCH;
            i += 2;

            let start = result.len().saturating_sub(offset);
            for j in 0..length {
                let idx = start + (j % offset);
                if idx < result.len() {
                    result.push(result[idx]);
                } else {
                    result.push(0);
                }
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lz_roundtrip() {
        let data = b"the quick brown fox jumps over the lazy dog and the quick brown fox jumps again";
        let encoded = lz_encode(data);
        let decoded = lz_decode(&encoded);
        assert_eq!(&decoded[..], &data[..]);
    }

    #[test]
    fn test_lz_highly_repetitive() {
        let data = "hello world! ".repeat(1000);
        let encoded = lz_encode(data.as_bytes());
        assert!(
            encoded.len() < data.len() / 5,
            "expected >5:1 ratio, got {}:{}",
            data.len(),
            encoded.len()
        );
        let decoded = lz_decode(&encoded);
        assert_eq!(decoded, data.as_bytes());
    }

    #[test]
    fn test_lz_small() {
        let data = b"hi";
        let encoded = lz_encode(data);
        let decoded = lz_decode(&encoded);
        assert_eq!(&decoded[..], &data[..]);
    }

    #[test]
    fn test_lz_empty() {
        let data: &[u8] = &[];
        let encoded = lz_encode(data);
        let decoded = lz_decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_lzans_roundtrip() {
        let data = "the quick brown fox ".repeat(500);
        let encoded = encode(data.as_bytes(), CodecType::LzAns);
        let decoded = decode(&encoded, CodecType::LzAns);
        assert_eq!(decoded, data.as_bytes());
    }

    #[test]
    fn test_select_best() {
        let data = "hello world hello world hello hello world world ".repeat(100);
        let (codec, compressed) = select_best_codec(data.as_bytes());
        assert!(compressed.len() < data.len() / 3);
        let decoded = decode(&compressed, codec);
        assert_eq!(decoded, data.as_bytes());
    }
}
