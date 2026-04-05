/// Bit-plane decomposition: separate each bit position into its own stream.
///
/// Format: [4-byte original_len] [plane_0] [plane_1] ... [plane_7]
/// Each plane is ceil(original_len / 8) bytes, packed MSB-first.

pub fn encode(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return vec![0, 0, 0, 0];
    }

    let n = data.len();
    let plane_sz = (n + 7) / 8;

    let mut out = Vec::with_capacity(4 + plane_sz * 8);
    out.extend_from_slice(&(n as u32).to_le_bytes());

    for bit in 0..8u8 {
        let mut plane = vec![0u8; plane_sz];
        for (i, &b) in data.iter().enumerate() {
            if b & (1 << bit) != 0 {
                plane[i / 8] |= 1 << (7 - (i % 8));
            }
        }
        out.extend_from_slice(&plane);
    }

    out
}

pub fn decode(data: &[u8]) -> Vec<u8> {
    if data.len() < 4 {
        return Vec::new();
    }

    let n = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    if n == 0 {
        return Vec::new();
    }

    let plane_sz = (n + 7) / 8;
    let payload = &data[4..];
    if payload.len() < plane_sz * 8 {
        return Vec::new();
    }

    let mut out = vec![0u8; n];
    for bit in 0..8u8 {
        let plane = &payload[bit as usize * plane_sz..(bit as usize + 1) * plane_sz];
        for i in 0..n {
            if plane[i / 8] & (1 << (7 - (i % 8))) != 0 {
                out[i] |= 1 << bit;
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_zeros_produce_zero_planes() {
        let data = vec![0u8; 100];
        let enc = encode(&data);
        let plane_sz = (100 + 7) / 8;
        for &b in &enc[4..4 + plane_sz * 8] {
            assert_eq!(b, 0);
        }
        assert_eq!(decode(&enc), data);
    }

    #[test]
    fn roundtrip() {
        let data = vec![0xAA, 0x55, 0xFF, 0x00, 0x42, 0x13, 0x37, 0xBE, 0xEF];
        assert_eq!(decode(&encode(&data)), data);
    }

    #[test]
    fn constant_high_nibble() {
        let data: Vec<u8> = (0..100).map(|i| 0x40 | (i & 0x0F) as u8).collect();
        assert_eq!(decode(&encode(&data)), data);
    }

    #[test]
    fn empty() {
        assert_eq!(encode(&[]), vec![0, 0, 0, 0]);
        assert_eq!(decode(&[0, 0, 0, 0]), Vec::<u8>::new());
    }
}
