/// Zero Run-Length Encoding for MTF output.
///
/// MTF output is dominated by zeros. This transform encodes runs of zeros
/// efficiently using a binary representation (like bzip2's RUNA/RUNB).
///
/// Encoding:
///   Non-zero byte N → output N+1 (shift up by 1 to free symbol 0)
///   Run of K zeros → encode K using symbols 0 and 1:
///     K is written in bijective base-2: 1→[0], 2→[1], 3→[0,0], 4→[1,0], 5→[0,1], ...
///
/// This is essentially bzip2's zero-run encoding. A run of 1000 zeros
/// takes only ~10 symbols.

/// Encode: collapse zero runs.
pub fn encode(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(data.len());
    let mut i = 0;

    while i < data.len() {
        if data[i] == 0 {
            // Count zero run
            let mut run_len = 0u32;
            while i < data.len() && data[i] == 0 {
                run_len += 1;
                i += 1;
            }
            // Encode run_len in bijective base-2 using symbols 0 (RUNA) and 1 (RUNB)
            encode_zero_run(run_len, &mut result);
        } else {
            // Non-zero: output value + 1 (shift to make room for RUNA/RUNB)
            result.push(data[i] + 1);
            i += 1;
        }
    }

    result
}

/// Decode: expand zero runs.
pub fn decode(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(data.len() * 2);
    let mut i = 0;

    while i < data.len() {
        if data[i] <= 1 {
            // Zero run encoded as RUNA(0)/RUNB(1) symbols
            let (run_len, new_i) = decode_zero_run(data, i);
            for _ in 0..run_len {
                result.push(0);
            }
            i = new_i;
        } else {
            // Non-zero: subtract 1 to restore original value
            result.push(data[i] - 1);
            i += 1;
        }
    }

    result
}

/// Encode a zero run length using bijective base-2 (RUNA=0, RUNB=1).
/// run_len 1 → [0]
/// run_len 2 → [1]
/// run_len 3 → [0, 0]
/// run_len 4 → [1, 0]
/// run_len 5 → [0, 1]
/// etc.
fn encode_zero_run(mut run_len: u32, out: &mut Vec<u8>) {
    while run_len > 0 {
        run_len -= 1;
        out.push((run_len & 1) as u8);
        run_len >>= 1;
    }
}

/// Decode RUNA/RUNB symbols back to zero count.
fn decode_zero_run(data: &[u8], start: usize) -> (u32, usize) {
    let mut run_len = 0u32;
    let mut power = 1u32;
    let mut i = start;

    while i < data.len() && data[i] <= 1 {
        run_len += (data[i] as u32 + 1) * power;
        power <<= 1;
        i += 1;
    }

    (run_len, i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let data = vec![0, 0, 0, 5, 3, 0, 0, 7, 0, 0, 0, 0, 0, 1, 2];
        let encoded = encode(&data);
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_all_zeros() {
        let data = vec![0u8; 1000];
        let encoded = encode(&data);
        // 1000 zeros should encode to ~10 symbols (bijective base-2)
        assert!(encoded.len() < 20, "Expected <20 symbols, got {}", encoded.len());
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_no_zeros() {
        let data: Vec<u8> = (1..=100).collect();
        let encoded = encode(&data);
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_single_zero() {
        let data = vec![0];
        let encoded = encode(&data);
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_mtf_like_output() {
        // Simulated MTF output: lots of zeros, some small values
        let mut data = Vec::new();
        for _ in 0..100 {
            data.extend_from_slice(&[0, 0, 0, 0, 0, 3, 0, 0, 1, 0, 0, 0, 2]);
        }
        let encoded = encode(&data);
        assert!(
            encoded.len() < data.len(),
            "Expected compression, got {}/{}",
            encoded.len(),
            data.len()
        );
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_empty() {
        assert_eq!(encode(&[]), Vec::<u8>::new());
        assert_eq!(decode(&[]), Vec::<u8>::new());
    }
}
