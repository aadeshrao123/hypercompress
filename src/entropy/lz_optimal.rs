// Optimal-parsing LZ77 with split streams and repeat-offset tracking.
//
// Output format:
//   [4-byte original_len] [4-byte num_tokens]
//   [4-byte token_bits_len] [4-byte literal_len] [4-byte offset_len] [4-byte length_len]
//   [token_bits] [literals] [offsets (varint)] [lengths (varint)]

const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
const WINDOW_SIZE: usize = 262143;
const HASH_BITS: usize = 18;
const HASH_SIZE: usize = 1 << HASH_BITS;
const HASH_MASK: usize = HASH_SIZE - 1;
const MAX_CHAIN: usize = 64;
const FAST_BYTES: usize = 64;

#[derive(Clone, Copy, Debug)]
enum Token {
    Literal(u8),
    Match { offset: u32, length: u16 },
}

pub fn compress(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return vec![0, 0, 0, 0, 0, 0, 0, 0];
    }

    let tokens = optimal_parse(data);

    let mut token_bits: Vec<u8> = Vec::with_capacity(tokens.len() / 8 + 1);
    let mut lit_stream: Vec<u8> = Vec::new();
    let mut off_stream: Vec<u8> = Vec::new();
    let mut len_stream: Vec<u8> = Vec::new();

    let mut bits: u8 = 0;
    let mut bit_n: u8 = 0;
    let mut last_off: u32 = 1;

    for &tok in &tokens {
        match tok {
            Token::Literal(byte) => {
                bit_n += 1;
                if bit_n == 8 {
                    token_bits.push(bits);
                    bits = 0;
                    bit_n = 0;
                }
                lit_stream.push(byte);
            }
            Token::Match { offset, length } => {
                bits |= 1 << bit_n;
                bit_n += 1;
                if bit_n == 8 {
                    token_bits.push(bits);
                    bits = 0;
                    bit_n = 0;
                }

                if offset == last_off {
                    encode_varint(0, &mut off_stream);
                } else {
                    encode_varint(offset as u64, &mut off_stream);
                    last_off = offset;
                }

                encode_varint((length as u16 - MIN_MATCH as u16) as u64, &mut len_stream);
            }
        }
    }
    if bit_n > 0 {
        token_bits.push(bits);
    }

    let mut result = Vec::new();
    result.extend_from_slice(&(data.len() as u32).to_le_bytes());
    result.extend_from_slice(&(tokens.len() as u32).to_le_bytes());
    result.extend_from_slice(&(token_bits.len() as u32).to_le_bytes());
    result.extend_from_slice(&(lit_stream.len() as u32).to_le_bytes());
    result.extend_from_slice(&(off_stream.len() as u32).to_le_bytes());
    result.extend_from_slice(&(len_stream.len() as u32).to_le_bytes());

    result.extend_from_slice(&token_bits);
    result.extend_from_slice(&lit_stream);
    result.extend_from_slice(&off_stream);
    result.extend_from_slice(&len_stream);

    result
}

pub fn decompress(data: &[u8]) -> Vec<u8> {
    if data.len() < 24 {
        return Vec::new();
    }

    let original_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let num_tokens = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let tb_len = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;
    let lit_len = u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as usize;
    let off_len = u32::from_le_bytes([data[16], data[17], data[18], data[19]]) as usize;
    let len_len = u32::from_le_bytes([data[20], data[21], data[22], data[23]]) as usize;

    if original_len == 0 || num_tokens == 0 {
        return Vec::new();
    }

    let mut pos = 24;
    let token_bits = &data[pos..pos + tb_len.min(data.len() - pos)];
    pos += tb_len;
    let literals = &data[pos..pos + lit_len.min(data.len() - pos)];
    pos += lit_len;
    let offsets = &data[pos..pos + off_len.min(data.len() - pos)];
    pos += off_len;
    let lengths = &data[pos..pos + len_len.min(data.len() - pos)];

    let mut result = Vec::with_capacity(original_len);
    let mut lp = 0usize;
    let mut op = 0usize;
    let mut lnp = 0usize;
    let mut last_off: u32 = 1;

    for i in 0..num_tokens {
        let is_match = {
            let bi = i / 8;
            let bit = i % 8;
            bi < token_bits.len() && (token_bits[bi] >> bit) & 1 == 1
        };

        if !is_match {
            if lp < literals.len() {
                result.push(literals[lp]);
                lp += 1;
            }
        } else {
            let (ov, nop) = decode_varint(offsets, op);
            op = nop;
            let (lv, nlnp) = decode_varint(lengths, lnp);
            lnp = nlnp;

            let offset = if ov == 0 { last_off } else { last_off = ov as u32; ov as u32 };
            let mlen = lv as usize + MIN_MATCH;

            let start = result.len().saturating_sub(offset as usize);
            for j in 0..mlen {
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

/// Forward-arrivals optimal parse: finds minimum-cost path through the LZ graph.
fn optimal_parse(data: &[u8]) -> Vec<Token> {
    let n = data.len();
    if n == 0 {
        return Vec::new();
    }

    let mut head = vec![u32::MAX; HASH_SIZE];
    let mut chain = vec![u32::MAX; n];

    let mut cost = vec![u32::MAX; n + 1];
    cost[0] = 0;

    #[derive(Clone, Copy)]
    struct Arrival {
        from: u32,
        offset: u32,
        length: u16,
        rep0: u32,
    }

    let mut arrivals = vec![
        Arrival { from: 0, offset: 0, length: 0, rep0: 1 };
        n + 1
    ];

    for i in 0..n {
        if cost[i] == u32::MAX {
            continue;
        }

        let rep0 = arrivals[i].rep0;

        // Literal option
        let lc = cost[i] + literal_price(data[i]);
        if lc < cost[i + 1] {
            cost[i + 1] = lc;
            arrivals[i + 1] = Arrival { from: i as u32, offset: 0, length: 0, rep0 };
        }

        if i + MIN_MATCH <= n {
            let h = hash4(data, i);

            // Rep0 match (cheapest)
            if rep0 > 0 && (rep0 as usize) <= i {
                let rstart = i - rep0 as usize;
                let mut rlen = 0;
                while i + rlen < n && rlen < MAX_MATCH && data[rstart + rlen] == data[i + rlen] {
                    rlen += 1;
                }
                if rlen >= MIN_MATCH {
                    let rc = cost[i] + rep_match_price(rlen);
                    if rc < cost[i + rlen] {
                        cost[i + rlen] = rc;
                        arrivals[i + rlen] = Arrival { from: i as u32, offset: rep0, length: rlen as u16, rep0 };
                    }
                }
            }

            // Hash chain matches
            let mut cp = head[h];
            let mut cc = 0;
            let min_pos = i.saturating_sub(WINDOW_SIZE);

            while cp != u32::MAX && (cp as usize) >= min_pos && cc < MAX_CHAIN {
                let j = cp as usize;
                if j < i {
                    let max_len = MAX_MATCH.min(n - i);
                    let mut len = 0;
                    while len < max_len && data[j + len] == data[i + len] {
                        len += 1;
                    }

                    if len >= MIN_MATCH {
                        let off = (i - j) as u32;

                        if len >= FAST_BYTES {
                            let mc = cost[i] + match_price(off, len, rep0);
                            if mc < cost[i + len] {
                                cost[i + len] = mc;
                                arrivals[i + len] = Arrival { from: i as u32, offset: off, length: len as u16, rep0: off };
                            }
                            break;
                        }

                        for ml in MIN_MATCH..=len {
                            let mc = cost[i] + match_price(off, ml, rep0);
                            if mc < cost[i + ml] {
                                cost[i + ml] = mc;
                                arrivals[i + ml] = Arrival { from: i as u32, offset: off, length: ml as u16, rep0: off };
                            }
                        }
                    }
                }
                cp = chain[j];
                cc += 1;
            }

            chain[i] = head[h];
            head[h] = i as u32;
        } else if i + 4 <= n {
            let h = hash4(data, i);
            chain[i] = head[h];
            head[h] = i as u32;
        }
    }

    // Backtrack
    let mut tokens = Vec::new();
    let mut pos = n;
    while pos > 0 {
        let arr = arrivals[pos];
        let from = arr.from as usize;
        if arr.length > 0 {
            tokens.push(Token::Match { offset: arr.offset, length: arr.length });
        } else {
            tokens.push(Token::Literal(data[from]));
        }
        pos = from;
    }

    tokens.reverse();
    tokens
}

fn literal_price(_byte: u8) -> u32 {
    9 // 8 bits + 1 flag bit
}

fn rep_match_price(length: usize) -> u32 {
    1 + 1 + varint_cost(length as u64 - MIN_MATCH as u64)
}

fn match_price(offset: u32, length: usize, rep0: u32) -> u32 {
    if offset == rep0 {
        return rep_match_price(length);
    }
    1 + varint_cost(offset as u64) + varint_cost(length as u64 - MIN_MATCH as u64)
}

fn varint_cost(value: u64) -> u32 {
    if value < 128 { 8 }
    else if value < 16384 { 16 }
    else if value < 2097152 { 24 }
    else { 32 }
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
        let b = (value & 0x7F) as u8;
        value >>= 7;
        if value == 0 {
            out.push(b);
            break;
        } else {
            out.push(b | 0x80);
        }
    }
}

fn decode_varint(data: &[u8], mut pos: usize) -> (u64, usize) {
    let mut val: u64 = 0;
    let mut shift = 0;
    while pos < data.len() {
        let b = data[pos];
        pos += 1;
        val |= ((b & 0x7F) as u64) << shift;
        if b & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 64 {
            break;
        }
    }
    (val, pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_simple() {
        let data = b"hello world hello world hello world";
        assert_eq!(&decompress(&compress(data))[..], &data[..]);
    }

    #[test]
    fn roundtrip_repetitive() {
        let data = "the quick brown fox jumps over the lazy dog. ".repeat(200);
        let comp = compress(data.as_bytes());
        assert!(comp.len() < data.len() / 3, "{}:{}", data.len(), comp.len());
        assert_eq!(decompress(&comp), data.as_bytes());
    }

    #[test]
    fn roundtrip_binary() {
        let data: Vec<u8> = (0..10000).map(|i| ((i * 7 + 3) % 256) as u8).collect();
        assert_eq!(decompress(&compress(&data)), data);
    }

    #[test]
    fn all_zeros() {
        let data = vec![0u8; 50000];
        let comp = compress(&data);
        assert!(comp.len() < 1000, "got {}", comp.len());
        assert_eq!(decompress(&comp), data);
    }

    #[test]
    fn roundtrip_small() {
        assert_eq!(&decompress(&compress(b"hi"))[..], b"hi");
    }

    #[test]
    fn empty() {
        assert!(decompress(&compress(&[])).is_empty());
    }

    #[test]
    fn rep_offset() {
        let mut data = Vec::new();
        let pat = b"ABCDEFGH";
        for _ in 0..500 {
            data.extend_from_slice(pat);
            data.push(b'X');
        }
        assert_eq!(decompress(&compress(&data)), data);
    }
}
