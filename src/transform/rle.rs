/// Run-Length Encoding: compress sequences of repeated bytes.
/// Optimal for sparse data with long runs of zeros or other repeated values.
///
/// Format uses an escape byte approach:
///   - Escape byte 0xFF followed by:
///     - 0x00: literal 0xFF byte
///     - N (1..=255): repeat the NEXT byte (N+2) times (so 3..257 repetitions)
///   - Any other byte: literal
///
/// This cleanly handles the 0xFF literal case and supports runs up to 257.

const ESCAPE: u8 = 0xFF;

pub fn encode(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::with_capacity(data.len());
    let mut i = 0;

    while i < data.len() {
        let value = data[i];

        // Count run length
        let mut run_len = 1usize;
        while i + run_len < data.len() && data[i + run_len] == value {
            run_len += 1;
        }

        if value == ESCAPE {
            // Must escape all 0xFF bytes
            if run_len >= 3 {
                // Encode as runs of up to 257
                let mut remaining = run_len;
                while remaining >= 3 {
                    let chunk = remaining.min(257);
                    result.push(ESCAPE);
                    result.push((chunk - 2) as u8); // 1..=255 → 3..257 reps
                    result.push(ESCAPE);
                    remaining -= chunk;
                }
                // Remaining 0-2 literal 0xFFs
                for _ in 0..remaining {
                    result.push(ESCAPE);
                    result.push(0x00); // literal 0xFF
                }
            } else {
                // 1 or 2 literal 0xFFs
                for _ in 0..run_len {
                    result.push(ESCAPE);
                    result.push(0x00);
                }
            }
            i += run_len;
        } else if run_len >= 3 {
            // Encode runs of non-0xFF bytes
            let mut remaining = run_len;
            while remaining >= 3 {
                let chunk = remaining.min(257);
                result.push(ESCAPE);
                result.push((chunk - 2) as u8);
                result.push(value);
                remaining -= chunk;
            }
            // Remaining 0-2 literals
            for _ in 0..remaining {
                result.push(value);
            }
            i += run_len;
        } else {
            // Literal bytes (not 0xFF, run < 3)
            result.push(value);
            i += 1;
        }
    }

    result
}

pub fn decode(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::with_capacity(data.len() * 2);
    let mut i = 0;

    while i < data.len() {
        if data[i] == ESCAPE {
            if i + 1 >= data.len() {
                break;
            }
            let n = data[i + 1];
            if n == 0x00 {
                // Literal 0xFF
                result.push(ESCAPE);
                i += 2;
            } else {
                // Run: repeat next byte (n+2) times
                if i + 2 >= data.len() {
                    break;
                }
                let value = data[i + 2];
                let count = n as usize + 2;
                for _ in 0..count {
                    result.push(value);
                }
                i += 3;
            }
        } else {
            result.push(data[i]);
            i += 1;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sparse_data() {
        let mut data = vec![0u8; 100];
        data[50] = 42;
        let encoded = encode(&data);
        assert!(encoded.len() < 20, "encoded len={}", encoded.len());
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_no_runs() {
        let data: Vec<u8> = (0..50).collect();
        let encoded = encode(&data);
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_all_same() {
        let data = vec![42u8; 200];
        let encoded = encode(&data);
        assert!(encoded.len() < 10, "encoded len={}", encoded.len());
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_literal_0xff() {
        // Data containing 0xFF bytes that aren't runs
        let data = vec![0xFE, 0xFF, 0xFE, 0xFF, 0x00, 0xFF];
        let encoded = encode(&data);
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_run_of_0xff() {
        let data = vec![0xFF; 50];
        let encoded = encode(&data);
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_mixed_runs_and_literals() {
        let mut data = Vec::new();
        data.extend_from_slice(&[1, 2, 3]); // literals
        data.extend_from_slice(&[0; 20]); // run
        data.extend_from_slice(&[0xFF; 10]); // run of 0xFF
        data.extend_from_slice(&[42, 43, 44]); // literals
        data.extend_from_slice(&[7; 300]); // long run (needs splitting)
        let encoded = encode(&data);
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_all_byte_values() {
        // Every byte value appears once — no runs
        let data: Vec<u8> = (0..=255).collect();
        let encoded = encode(&data);
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_empty() {
        assert_eq!(encode(&[]), Vec::<u8>::new());
        assert_eq!(decode(&[]), Vec::<u8>::new());
    }
}
