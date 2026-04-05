/// Delta encoding: store differences between consecutive bytes.
/// Extremely effective for sequential/sorted numeric data.
/// Reduces entropy when values change slowly.

/// Encode: output[i] = input[i] - input[i-1] (wrapping).
/// First byte stored as-is.
pub fn encode(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::with_capacity(data.len());
    result.push(data[0]);

    for i in 1..data.len() {
        result.push(data[i].wrapping_sub(data[i - 1]));
    }

    result
}

/// Decode: input[i] = output[i] + input[i-1] (wrapping).
pub fn decode(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::with_capacity(data.len());
    result.push(data[0]);

    for i in 1..data.len() {
        result.push(data[i].wrapping_add(result[i - 1]));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let data = vec![10, 12, 15, 14, 20, 25, 30];
        assert_eq!(decode(&encode(&data)), data);
    }

    #[test]
    fn test_sequential() {
        // Sequential data should produce lots of same deltas
        let data: Vec<u8> = (0..100).collect();
        let encoded = encode(&data);
        // After first byte, all deltas should be 1
        for &b in &encoded[1..] {
            assert_eq!(b, 1);
        }
        assert_eq!(decode(&encoded), data);
    }

    #[test]
    fn test_wrapping() {
        let data = vec![255, 0, 1, 254];
        let encoded = encode(&data);
        assert_eq!(decode(&encoded), data);
    }

    #[test]
    fn test_empty() {
        assert_eq!(encode(&[]), Vec::<u8>::new());
        assert_eq!(decode(&[]), Vec::<u8>::new());
    }
}
