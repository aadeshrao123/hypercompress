/// Bit-plane decomposition: separate each bit position into its own stream.
///
/// This is a NOVEL general-purpose transform not found in mainstream compressors.
/// Instead of storing bytes [b0, b1, b2, ...], we store:
///   - All bit-0s of every byte
///   - All bit-1s of every byte
///   - ...
///   - All bit-7s of every byte
///
/// This can expose patterns invisible at the byte level. For example,
/// if the high bits are always the same (common in binary data), those
/// bit-planes become trivially compressible.
///
/// Format: [4-byte original_len] [plane_0] [plane_1] ... [plane_7]
/// Each plane is ceil(original_len / 8) bytes, packed MSB-first.

/// Encode: split bytes into 8 bit planes.
pub fn encode(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return vec![0, 0, 0, 0];
    }

    let n = data.len();
    let plane_bytes = (n + 7) / 8;

    let mut result = Vec::with_capacity(4 + plane_bytes * 8);
    result.extend_from_slice(&(n as u32).to_le_bytes());

    // For each bit position (0 = LSB, 7 = MSB)
    for bit in 0..8u8 {
        let mut plane = vec![0u8; plane_bytes];
        for (i, &byte) in data.iter().enumerate() {
            if byte & (1 << bit) != 0 {
                plane[i / 8] |= 1 << (7 - (i % 8)); // MSB-first packing
            }
        }
        result.extend_from_slice(&plane);
    }

    result
}

/// Decode: reassemble bytes from 8 bit planes.
pub fn decode(data: &[u8]) -> Vec<u8> {
    if data.len() < 4 {
        return Vec::new();
    }

    let n = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    if n == 0 {
        return Vec::new();
    }

    let plane_bytes = (n + 7) / 8;
    let payload = &data[4..];

    if payload.len() < plane_bytes * 8 {
        return Vec::new();
    }

    let mut result = vec![0u8; n];

    for bit in 0..8u8 {
        let plane = &payload[bit as usize * plane_bytes..(bit as usize + 1) * plane_bytes];
        for i in 0..n {
            if plane[i / 8] & (1 << (7 - (i % 8))) != 0 {
                result[i] |= 1 << bit;
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let data = vec![0xAA, 0x55, 0xFF, 0x00, 0x42, 0x13, 0x37, 0xBE, 0xEF];
        let encoded = encode(&data);
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_all_zeros() {
        let data = vec![0u8; 100];
        let encoded = encode(&data);
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);

        // All planes should be zero bytes — very compressible
        let plane_bytes = (100 + 7) / 8;
        for &b in &encoded[4..4 + plane_bytes * 8] {
            assert_eq!(b, 0);
        }
    }

    #[test]
    fn test_high_bits_constant() {
        // Data where high nibble is always 0x40 (ASCII uppercase area)
        let data: Vec<u8> = (0..100).map(|i| 0x40 | (i & 0x0F) as u8).collect();
        let encoded = encode(&data);
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_empty() {
        assert_eq!(encode(&[]), vec![0, 0, 0, 0]);
        assert_eq!(decode(&[0, 0, 0, 0]), Vec::<u8>::new());
    }
}
