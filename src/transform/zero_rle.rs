/// Zero run-length encoding for MTF output. Encodes runs of zeros
/// using bijective base-2 (like bzip2's RUNA/RUNB), non-zero bytes
/// are shifted up by 1 to free symbols 0 and 1.

pub fn encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;

    while i < data.len() {
        if data[i] == 0 {
            let mut run = 0u32;
            while i < data.len() && data[i] == 0 {
                run += 1;
                i += 1;
            }
            emit_zero_run(run, &mut out);
        } else {
            out.push(data[i] + 1);
            i += 1;
        }
    }

    out
}

pub fn decode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 2);
    let mut i = 0;

    while i < data.len() {
        if data[i] <= 1 {
            let (run, next) = read_zero_run(data, i);
            out.resize(out.len() + run as usize, 0);
            i = next;
        } else {
            out.push(data[i] - 1);
            i += 1;
        }
    }

    out
}

/// Bijective base-2: 1->[0], 2->[1], 3->[0,0], 4->[1,0], ...
fn emit_zero_run(mut n: u32, out: &mut Vec<u8>) {
    while n > 0 {
        n -= 1;
        out.push((n & 1) as u8);
        n >>= 1;
    }
}

fn read_zero_run(data: &[u8], start: usize) -> (u32, usize) {
    let mut n = 0u32;
    let mut power = 1u32;
    let mut i = start;

    while i < data.len() && data[i] <= 1 {
        n += (data[i] as u32 + 1) * power;
        power <<= 1;
        i += 1;
    }

    (n, i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousand_zeros_encode_small() {
        let data = vec![0u8; 1000];
        let enc = encode(&data);
        assert!(enc.len() < 20, "Expected <20 symbols, got {}", enc.len());
        assert_eq!(decode(&enc), data);
    }

    #[test]
    fn roundtrip_mixed() {
        let data = vec![0, 0, 0, 5, 3, 0, 0, 7, 0, 0, 0, 0, 0, 1, 2];
        assert_eq!(decode(&encode(&data)), data);
    }

    #[test]
    fn no_zeros() {
        let data: Vec<u8> = (1..=100).collect();
        assert_eq!(decode(&encode(&data)), data);
    }

    #[test]
    fn single_zero() {
        assert_eq!(decode(&encode(&[0])), vec![0]);
    }

    #[test]
    fn mtf_like_output() {
        let mut data = Vec::new();
        for _ in 0..100 {
            data.extend_from_slice(&[0, 0, 0, 0, 0, 3, 0, 0, 1, 0, 0, 0, 2]);
        }
        let enc = encode(&data);
        assert!(enc.len() < data.len(), "Expected compression, got {}/{}", enc.len(), data.len());
        assert_eq!(decode(&enc), data);
    }

    #[test]
    fn empty() {
        assert_eq!(encode(&[]), Vec::<u8>::new());
        assert_eq!(decode(&[]), Vec::<u8>::new());
    }
}
