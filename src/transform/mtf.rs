/// Move-to-Front transform.
///
/// Converts BWT output into a stream of small integers.
/// After BWT, characters that recently appeared tend to repeat.
/// MTF exploits this by outputting the INDEX of each byte in a
/// recently-used list, then moving that byte to position 0.
///
/// Result: ~60% of output bytes are 0 for typical text. Extremely
/// skewed distribution perfect for entropy coding.

/// Forward MTF: bytes → indices.
pub fn encode(data: &[u8]) -> Vec<u8> {
    let mut alphabet: Vec<u8> = (0..=255).collect();
    let mut result = Vec::with_capacity(data.len());

    for &byte in data {
        // Find position of byte in alphabet
        let pos = alphabet.iter().position(|&b| b == byte).unwrap();
        result.push(pos as u8);

        // Move to front
        if pos > 0 {
            let val = alphabet.remove(pos);
            alphabet.insert(0, val);
        }
    }

    result
}

/// Inverse MTF: indices → bytes.
pub fn decode(data: &[u8]) -> Vec<u8> {
    let mut alphabet: Vec<u8> = (0..=255).collect();
    let mut result = Vec::with_capacity(data.len());

    for &index in data {
        let pos = index as usize;
        let byte = alphabet[pos];
        result.push(byte);

        // Move to front
        if pos > 0 {
            alphabet.remove(pos);
            alphabet.insert(0, byte);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let data = b"bananaaa";
        let encoded = encode(data);
        let decoded = decode(&encoded);
        assert_eq!(&decoded[..], &data[..]);
    }

    #[test]
    fn test_repeated_chars() {
        // Repeated chars should produce lots of zeros
        let data = b"aaaaaabbbbbbcccccc";
        let encoded = encode(data);
        let zeros = encoded.iter().filter(|&&b| b == 0).count();
        assert!(zeros > data.len() / 2, "Expected many zeros, got {}/{}", zeros, data.len());
        let decoded = decode(&encoded);
        assert_eq!(&decoded[..], &data[..]);
    }

    #[test]
    fn test_bwt_output() {
        // Simulated BWT output (locally clustered characters)
        let data = b"eeeeeddddbbbbaaaccc";
        let encoded = encode(data);
        let decoded = decode(&encoded);
        assert_eq!(&decoded[..], &data[..]);
        // Most outputs should be small indices
        let small = encoded.iter().filter(|&&b| b < 5).count();
        assert!(small > data.len() / 2);
    }

    #[test]
    fn test_empty() {
        assert_eq!(encode(&[]), Vec::<u8>::new());
        assert_eq!(decode(&[]), Vec::<u8>::new());
    }
}
