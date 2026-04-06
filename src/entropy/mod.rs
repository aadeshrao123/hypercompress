pub mod ans;
pub mod lz_optimal;
pub mod order1;

use crate::format::CodecType;

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

/// Fast LZMA: preset 1, multithreaded. Good ratio at reasonable speed.
pub fn lzma_fast(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    use xz2::stream::MtStreamBuilder;

    if data.len() > 64 * 1024 {
        if let Some(out) = MtStreamBuilder::new()
            .preset(1)
            .threads(num_cpus())
            .encoder()
            .ok()
            .and_then(|s| {
                let mut enc = xz2::write::XzEncoder::new_stream(Vec::new(), s);
                enc.write_all(data).ok()?;
                enc.finish().ok()
            })
        {
            if out.len() < data.len() { return out; }
        }
    }

    // fallback: single-threaded preset 1
    use xz2::write::XzEncoder;
    let mut enc = XzEncoder::new(Vec::new(), 1);
    if enc.write_all(data).is_ok() {
        if let Ok(out) = enc.finish() {
            if out.len() < data.len() { return out; }
        }
    }
    data.to_vec()
}

fn lzma_compress(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    use xz2::stream::MtStreamBuilder;

    // scale preset by data size — big files get faster presets
    let preset = if data.len() > 4 * 1024 * 1024 { 3 }
        else if data.len() > 512 * 1024 { 5 }
        else { 6 };

    // large data: use multithreaded encoder (fastest path)
    if data.len() > 256 * 1024 {
        if let Some(out) = MtStreamBuilder::new()
            .preset(preset)
            .threads(num_cpus())
            .encoder()
            .ok()
            .and_then(|stream| {
                let mut enc = xz2::write::XzEncoder::new_stream(Vec::new(), stream);
                enc.write_all(data).ok()?;
                enc.finish().ok()
            })
        {
            if out.len() < data.len() { return out; }
        }
    }

    // small data: try tuned params
    let ascii = data.iter().filter(|&&b| b >= 0x20 && b <= 0x7E).count() as f64
        / data.len().max(1) as f64;

    let (lc, lp, pb) = if ascii > 0.8 { (3, 0, 0) }
        else if data.len() % 4 == 0 && ascii < 0.3 { (0, 2, 2) }
        else { (3, 0, 2) };

    lzma_with_params(data, preset, lc, lp, pb).unwrap_or_else(|| data.to_vec())
}

fn lzma_with_params(data: &[u8], preset: u32, lc: u32, lp: u32, pb: u32) -> Option<Vec<u8>> {
    use std::io::Write;
    use xz2::stream::{Filters, LzmaOptions, Stream};

    let mut opts = LzmaOptions::new_preset(preset).ok()?;
    opts.literal_context_bits(lc);
    opts.literal_position_bits(lp);
    opts.position_bits(pb);

    let mut filters = Filters::new();
    filters.lzma2(&opts);
    let stream = Stream::new_stream_encoder(&filters, xz2::stream::Check::Crc64).ok()?;
    let mut enc = xz2::write::XzEncoder::new_stream(Vec::new(), stream);
    enc.write_all(data).ok()?;
    enc.finish().ok()
}

fn num_cpus() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(2)
}

fn lzma_decompress(data: &[u8]) -> Vec<u8> {
    use std::io::Read;
    let mut dec = xz2::read::XzDecoder::new(data);
    let mut out = Vec::new();
    if dec.read_to_end(&mut out).is_ok() { out } else { data.to_vec() }
}

/// Level 1-2: just LZ+ANS, one pass, no experiments. Like 7-Zip "Fastest".
/// Gzip-style fast encoder: 3-byte hash, chain depth 4, skip insertions on long matches.
/// No entropy check, no ANS — just LZ, single pass. Matches gzip level 1 approach.
pub fn encode_fast(data: &[u8]) -> (CodecType, Vec<u8>) {
    if data.len() < 8 { return (CodecType::Raw, data.to_vec()); }

    let lz = lz_turbo(data);
    if lz.len() < data.len() {
        (CodecType::Lz, lz)
    } else {
        (CodecType::Raw, data.to_vec())
    }
}

// Gzip deflate_fast inspired: 3-byte hash, chain=4, nice=8, skip inserts on long matches.
fn lz_turbo(data: &[u8]) -> Vec<u8> {
    const HASH_BITS: usize = 15;
    const HASH_SIZE: usize = 1 << HASH_BITS;
    const HASH_MASK: usize = HASH_SIZE - 1;
    const CHAIN_MAX: usize = 4;     // gzip level 1
    const NICE_LEN: usize = 8;      // stop searching at 8
    const SKIP_INSERT: usize = 4;   // skip hash inserts for matches > 4
    const WIN: usize = 32768;       // 32K window like gzip

    if data.len() < 4 { return encode_literals_turbo(data); }

    let mut result = Vec::with_capacity(data.len());
    let mut head = vec![u32::MAX; HASH_SIZE];
    let mut chain = vec![u32::MAX; data.len()];

    let mut i = 0;
    let mut lit_start = 0;

    while i + 2 < data.len() {
        // 3-byte shift+XOR hash (like gzip)
        let h = ((data[i] as usize) << 10 ^ (data[i+1] as usize) << 5 ^ data[i+2] as usize) & HASH_MASK;

        let mut best_len = 0;
        let mut best_off = 0;
        let mut cpos = head[h];
        let mut depth = 0;
        let min = if i > WIN { i - WIN } else { 0 };

        while cpos != u32::MAX && (cpos as usize) >= min && depth < CHAIN_MAX {
            let j = cpos as usize;
            if j < i {
                let max_m = 258.min(data.len() - i);
                let mut len = 0;
                while len < max_m && data[j + len] == data[i + len] { len += 1; }
                if len >= 3 && len > best_len {
                    best_len = len;
                    best_off = i - j;
                    if len >= NICE_LEN { break; }
                }
            }
            cpos = chain[j];
            depth += 1;
        }

        // insert into chain
        chain[i] = head[h];
        head[h] = i as u32;

        if best_len >= 3 {
            if i > lit_start { emit_literals_turbo(&data[lit_start..i], &mut result); }
            emit_match_turbo(best_off, best_len, &mut result);

            // gzip optimization: skip hash insertions for long matches
            if best_len <= SKIP_INSERT {
                for k in 1..best_len {
                    if i + k + 2 < data.len() {
                        let hk = ((data[i+k] as usize) << 10 ^ (data[i+k+1] as usize) << 5 ^ data[i+k+2] as usize) & HASH_MASK;
                        chain[i + k] = head[hk];
                        head[hk] = (i + k) as u32;
                    }
                }
            }
            // else: skip all insertions (like gzip level 1 does for matches > max_insert_length)

            i += best_len;
            lit_start = i;
        } else {
            i += 1;
        }
    }

    if lit_start < data.len() { emit_literals_turbo(&data[lit_start..], &mut result); }
    result
}

fn encode_literals_turbo(data: &[u8]) -> Vec<u8> {
    let mut r = Vec::with_capacity(data.len() + data.len() / 128 + 1);
    emit_literals_turbo(data, &mut r);
    r
}

fn emit_literals_turbo(lits: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < lits.len() {
        let n = (lits.len() - i).min(128);
        out.push((n - 1) as u8);
        out.extend_from_slice(&lits[i..i + n]);
        i += n;
    }
}

fn emit_match_turbo(offset: usize, length: usize, out: &mut Vec<u8>) {
    // same format as our main LZ encoder so the same decoder works
    let off = offset - 1;
    if off < 0x7F00 {
        out.push(0x80 | ((off >> 8) as u8 & 0x7F));
        out.push((off & 0xFF) as u8);
        out.push((length - 3) as u8);
    } else {
        out.push(0xFF);
        out.push((off & 0xFF) as u8);
        out.push(((off >> 8) & 0xFF) as u8);
        out.push(((off >> 16) & 0xFF) as u8);
        out.push((length - 3) as u8);
    }
}

/// Level 3-4: LZ + ANS + LzAns. No LZMA, no optimal parsing.
pub fn select_fast_codec(data: &[u8]) -> (CodecType, Vec<u8>) {
    if data.is_empty() { return (CodecType::Raw, Vec::new()); }
    let ent = quick_entropy(data);
    if ent > 7.5 { return (CodecType::Raw, data.to_vec()); }

    let mut best = (CodecType::Raw, data.to_vec(), data.len());
    let try_c = |b: &mut (CodecType, Vec<u8>, usize), c: CodecType, e: Vec<u8>| {
        if e.len() < b.2 { b.2 = e.len(); b.0 = c; b.1 = e; }
    };

    try_c(&mut best, CodecType::Ans, ans::encode(data));

    let lz = lz_encode(data);
    let lz_ok = lz.len() < data.len();
    try_c(&mut best, CodecType::Lz, lz.clone());
    if lz_ok { try_c(&mut best, CodecType::LzAns, ans::encode(&lz)); }

    if data.len() >= 64 {
        if let Some(o1) = order1::encode(data) {
            try_c(&mut best, CodecType::Order1, o1);
        }
    }

    (best.0, best.1)
}

/// Level 5+: try everything including LZMA.
pub fn select_best_codec(data: &[u8]) -> (CodecType, Vec<u8>) {
    if data.is_empty() {
        return (CodecType::Raw, Vec::new());
    }

    let ent = quick_entropy(data);
    if ent > 7.8 {
        return (CodecType::Raw, data.to_vec());
    }

    let mut best = (CodecType::Raw, data.to_vec(), data.len());

    fn try_codec(best: &mut (CodecType, Vec<u8>, usize), codec: CodecType, encoded: Vec<u8>) {
        if encoded.len() < best.2 {
            best.2 = encoded.len();
            best.0 = codec;
            best.1 = encoded;
        }
    }

    let level = crate::compress::get_level();

    // high entropy: skip custom codecs, just try LZMA if level allows
    if ent > 5.5 {
        if level >= 4 {
            try_codec(&mut best, CodecType::Lzma, lzma_compress(data));
        }
        return (best.0, best.1);
    }

    // fast codecs — always run (they're what make us fast + good)
    try_codec(&mut best, CodecType::Ans, ans::encode(data));

    let lz_out = lz_encode(data);
    let lz_helped = lz_out.len() < data.len();
    try_codec(&mut best, CodecType::Lz, lz_out.clone());

    if lz_helped {
        try_codec(&mut best, CodecType::LzAns, ans::encode(&lz_out));
    }

    // mid-tier codecs — level 3+
    if level >= 3 && data.len() >= 64 {
        if let Some(o1) = order1::encode(data) {
            try_codec(&mut best, CodecType::Order1, o1);
        }
    }

    if level >= 3 && data.len() >= 32 {
        try_codec(&mut best, CodecType::LzOptimal, lz_optimal::compress(data));
    }

    // only try LZMA (expensive) if fast codecs didn't compress well (<40% reduction)
    if best.2 > data.len() * 60 / 100 {
        try_codec(&mut best, CodecType::Lzma, lzma_compress(data));
    }

    (best.0, best.1)
}

fn quick_entropy(data: &[u8]) -> f64 {
    let mut hist = [0u32; 256];
    for &b in data {
        hist[b as usize] += 1;
    }
    let len = data.len() as f64;
    let mut ent = 0.0;
    for &c in &hist {
        if c > 0 {
            let p = c as f64 / len;
            ent -= p * p.log2();
        }
    }
    ent
}

// Hash-chain LZ77 encoder.
//
// Token format (stream of control bytes):
//   Bit 7 = 0: Literal run, bits 0-6 = count-1 (1..128 literals follow)
//   Bit 7 = 1: Match, bits 0-6 + next byte = offset (15-bit), next byte = length-3

const HASH_BITS: usize = 18;
const HASH_SIZE: usize = 1 << HASH_BITS;
const HASH_MASK: usize = HASH_SIZE - 1;
const MAX_CHAIN: usize = 256;
const WINDOW_SIZE: usize = 262143;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;

fn hash4(data: &[u8], pos: usize) -> usize {
    if pos + 4 > data.len() {
        return 0;
    }
    let v = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
    ((v.wrapping_mul(2654435761)) >> (32 - HASH_BITS)) as usize & HASH_MASK
}

/// Find best match at `pos` by walking the hash chain.
/// Returns (length, offset) or (0, 0) if no match found.
fn find_match(data: &[u8], pos: usize, head: &[u32], chain: &[u32], max_chain_depth: usize) -> (usize, usize) {
    if pos + MIN_MATCH > data.len() {
        return (0, 0);
    }
    let h = hash4(data, pos);
    let mut best_len = 0;
    let mut best_off = 0;
    let mut cp = head[h];
    let mut count = 0;
    let min_pos = pos.saturating_sub(WINDOW_SIZE);

    while cp != u32::MAX && (cp as usize) >= min_pos && count < max_chain_depth {
        let j = cp as usize;
        if j < pos {
            let max_len = MAX_MATCH.min(data.len() - pos);
            let mut len = 0;
            while len < max_len && data[j + len] == data[pos + len] {
                len += 1;
            }
            if len >= MIN_MATCH && len > best_len {
                best_len = len;
                best_off = pos - j;
                if len == max_len {
                    break;
                }
            }
        }
        cp = chain[j];
        count += 1;
    }
    (best_len, best_off)
}

fn lz_encode(data: &[u8]) -> Vec<u8> {
    if data.len() < 8 {
        return encode_literals(data);
    }

    let mut result = Vec::with_capacity(data.len());

    let mut head = vec![u32::MAX; HASH_SIZE];
    let mut chain = vec![0u32; data.len()];

    let mut i = 0;
    let mut lit_start = 0;

    while i < data.len() {
        let (best_len, best_off) = find_match(data, i, &head, &chain, MAX_CHAIN);

        let h = hash4(data, i);
        chain[i] = head[h];
        head[h] = i as u32;

        if best_len >= MIN_MATCH {
            // Lazy matching: check if i+1 has an even longer match
            let mut use_match = true;
            if best_len < MAX_MATCH && i + 1 + MIN_MATCH <= data.len() {
                let (lazy_len, _) = find_match(data, i + 1, &head, &chain, MAX_CHAIN / 2);
                if lazy_len > best_len + 1 {
                    use_match = false;
                }
            }

            if use_match {
                if i > lit_start {
                    emit_literals(&data[lit_start..i], &mut result);
                }

                emit_match(best_off, best_len, &mut result);

                for k in 1..best_len {
                    if i + k + 4 <= data.len() {
                        let hk = hash4(data, i + k);
                        chain[i + k] = head[hk];
                        head[hk] = (i + k) as u32;
                    }
                }

                i += best_len;
                lit_start = i;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    if lit_start < data.len() {
        emit_literals(&data[lit_start..], &mut result);
    }

    result
}

fn encode_literals(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(data.len() + (data.len() / 128) + 1);
    emit_literals(data, &mut result);
    result
}

fn emit_literals(lits: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < lits.len() {
        let n = (lits.len() - i).min(128);
        out.push((n - 1) as u8);
        out.extend_from_slice(&lits[i..i + n]);
        i += n;
    }
}

fn emit_match(offset: usize, length: usize, out: &mut Vec<u8>) {
    debug_assert!(offset >= 1 && offset <= WINDOW_SIZE);
    debug_assert!(length >= MIN_MATCH && length <= MAX_MATCH);

    let off = offset - 1;

    if off < 0x7F00 {
        out.push(0x80 | ((off >> 8) as u8 & 0x7F));
        out.push((off & 0xFF) as u8);
        out.push((length - MIN_MATCH) as u8);
    } else {
        out.push(0xFF);
        out.push((off & 0xFF) as u8);
        out.push(((off >> 8) & 0xFF) as u8);
        out.push(((off >> 16) & 0xFF) as u8);
        out.push((length - MIN_MATCH) as u8);
    }
}

/// Copy a match from already-decoded output (handles overlapping copies).
fn copy_match(result: &mut Vec<u8>, offset: usize, length: usize) {
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

fn lz_decode(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    let mut i = 0;

    while i < data.len() {
        let ctrl = data[i];
        i += 1;

        if ctrl & 0x80 == 0 {
            let count = (ctrl & 0x7F) as usize + 1;
            if i + count > data.len() {
                break;
            }
            result.extend_from_slice(&data[i..i + count]);
            i += count;
        } else if ctrl == 0xFF {
            if i + 4 > data.len() {
                break;
            }
            let offset = (data[i] as usize)
                | ((data[i + 1] as usize) << 8)
                | ((data[i + 2] as usize) << 16);
            let offset = offset + 1;
            let length = data[i + 3] as usize + MIN_MATCH;
            i += 4;
            copy_match(&mut result, offset, length);
        } else {
            if i + 2 > data.len() {
                break;
            }
            let off_hi = (ctrl & 0x7F) as u16;
            let off_lo = data[i] as u16;
            let offset = ((off_hi << 8) | off_lo) as usize + 1;
            let length = data[i + 1] as usize + MIN_MATCH;
            i += 2;
            copy_match(&mut result, offset, length);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lz_roundtrip() {
        let data = b"the quick brown fox jumps over the lazy dog and the quick brown fox jumps again";
        let decoded = lz_decode(&lz_encode(data));
        assert_eq!(&decoded[..], &data[..]);
    }

    #[test]
    fn lz_highly_repetitive() {
        let data = "hello world! ".repeat(1000);
        let enc = lz_encode(data.as_bytes());
        assert!(enc.len() < data.len() / 5, "ratio {}:{}", data.len(), enc.len());
        assert_eq!(lz_decode(&enc), data.as_bytes());
    }

    #[test]
    fn lz_small_input() {
        let data = b"hi";
        assert_eq!(&lz_decode(&lz_encode(data))[..], &data[..]);
    }

    #[test]
    fn lz_empty() {
        let enc = lz_encode(&[]);
        assert_eq!(lz_decode(&enc), &[] as &[u8]);
    }

    #[test]
    fn lzans_roundtrip() {
        let data = "the quick brown fox ".repeat(500);
        let decoded = decode(&encode(data.as_bytes(), CodecType::LzAns), CodecType::LzAns);
        assert_eq!(decoded, data.as_bytes());
    }

    #[test]
    fn select_best_compresses() {
        let data = "hello world hello world hello hello world world ".repeat(100);
        let (codec, compressed) = select_best_codec(data.as_bytes());
        assert!(compressed.len() < data.len() / 3);
        assert_eq!(decode(&compressed, codec), data.as_bytes());
    }
}
