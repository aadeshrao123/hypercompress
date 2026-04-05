pub mod ans;

use crate::format::CodecType;

/// Encode data using the specified codec.
pub fn encode(data: &[u8], codec: CodecType) -> Vec<u8> {
    match codec {
        CodecType::Raw => data.to_vec(),
        CodecType::Ans => ans::encode(data),
        CodecType::Lz => lz_encode(data),
    }
}

/// Decode data using the specified codec.
pub fn decode(data: &[u8], codec: CodecType) -> Vec<u8> {
    match codec {
        CodecType::Raw => data.to_vec(),
        CodecType::Ans => ans::decode(data),
        CodecType::Lz => lz_decode(data),
    }
}

/// Select the best codec for data that has already been transformed.
pub fn select_codec(data: &[u8]) -> CodecType {
    if data.is_empty() {
        return CodecType::Raw;
    }

    // Quick entropy estimate
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

    // High entropy after transform → raw storage (already incompressible)
    if entropy > 7.8 {
        return CodecType::Raw;
    }

    // Try LZ for data with repeated patterns
    if data.len() > 256 && has_repeated_patterns(data) {
        return CodecType::Lz;
    }

    // Default to ANS for entropy coding
    CodecType::Ans
}

/// Quick check for repeated patterns (long matches exist).
fn has_repeated_patterns(data: &[u8]) -> bool {
    if data.len() < 16 {
        return false;
    }

    // Sample: check if any 4-byte sequence in first 1KB appears again
    let check_len = 1024.min(data.len());
    let mut seen = std::collections::HashSet::new();
    let mut repeats = 0u32;

    for window in data[..check_len].windows(4) {
        let key = u32::from_le_bytes([window[0], window[1], window[2], window[3]]);
        if !seen.insert(key) {
            repeats += 1;
        }
    }

    // If >20% of 4-byte windows are repeats, LZ will work well
    repeats as f64 / check_len as f64 > 0.15
}

/// Simple LZ77-style encoder with 16-bit offsets and 8-bit lengths.
/// Format: sequence of packets:
///   [0] + [literal_byte]                         = 1 literal
///   [1] + [offset_hi] + [offset_lo] + [length]   = match (length+3 bytes at offset)
fn lz_encode(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::with_capacity(data.len());
    let mut i = 0;
    let window_size = 65535usize; // 16-bit offset

    while i < data.len() {
        let mut best_offset = 0usize;
        let mut best_length = 0usize;

        // Search for matches in the sliding window
        let search_start = if i > window_size { i - window_size } else { 0 };
        let max_match = 258.min(data.len() - i); // 255 + 3

        if max_match >= 3 {
            for j in search_start..i {
                let mut len = 0;
                while len < max_match && i + len < data.len() && data[j + len] == data[i + len] {
                    len += 1;
                }
                if len >= 3 && len > best_length {
                    best_length = len;
                    best_offset = i - j;
                    if len == max_match {
                        break;
                    }
                }
            }
        }

        if best_length >= 3 {
            result.push(1); // match flag
            result.push((best_offset >> 8) as u8);
            result.push((best_offset & 0xFF) as u8);
            result.push((best_length - 3) as u8);
            i += best_length;
        } else {
            result.push(0); // literal flag
            result.push(data[i]);
            i += 1;
        }
    }

    result
}

/// LZ77 decoder.
fn lz_decode(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    let mut i = 0;

    while i < data.len() {
        if data[i] == 0 {
            // Literal
            if i + 1 < data.len() {
                result.push(data[i + 1]);
            }
            i += 2;
        } else if data[i] == 1 {
            // Match
            if i + 3 < data.len() {
                let offset = ((data[i + 1] as usize) << 8) | (data[i + 2] as usize);
                let length = data[i + 3] as usize + 3;
                let start = result.len().saturating_sub(offset);
                for j in 0..length {
                    let idx = start + (j % offset.max(1));
                    if idx < result.len() {
                        result.push(result[idx]);
                    }
                }
            }
            i += 4;
        } else {
            i += 1; // skip unknown
        }
    }

    result
}
