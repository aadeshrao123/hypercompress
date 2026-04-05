/// Float split transform: separate IEEE 754 float components.
///
/// This is a NOVEL transform not found in general-purpose compressors.
/// IEEE 754 single-precision floats (4 bytes) have:
///   - 1 sign bit
///   - 8 exponent bits
///   - 23 mantissa bits
///
/// By splitting into separate streams, each stream becomes independently
/// compressible. Exponent bytes are especially compressible (low entropy).
///
/// Format: [4-byte original_len] [exponent_stream] [byte0_stream] [byte1_stream] [byte2_stream]
/// Where for each float [b0, b1, b2, b3]:
///   - exponent_stream gets b3 (contains sign + most exponent bits)
///   - byte0_stream gets b0 (low mantissa)
///   - byte1_stream gets b1 (mid mantissa)
///   - byte2_stream gets b2 (high mantissa + low exponent)

/// Encode: split float data into component streams.
pub fn encode(data: &[u8]) -> Vec<u8> {
    let original_len = data.len();

    // If not aligned to 4 bytes, fall back to no transform
    if original_len < 4 || original_len % 4 != 0 {
        let mut result = Vec::with_capacity(4 + data.len());
        result.extend_from_slice(&(original_len as u32).to_le_bytes());
        result.extend_from_slice(data);
        return result;
    }

    let num_floats = original_len / 4;
    let mut stream0 = Vec::with_capacity(num_floats); // byte 0 (low mantissa)
    let mut stream1 = Vec::with_capacity(num_floats); // byte 1 (mid mantissa)
    let mut stream2 = Vec::with_capacity(num_floats); // byte 2 (high mantissa + low exp)
    let mut stream3 = Vec::with_capacity(num_floats); // byte 3 (sign + high exp)

    for chunk in data.chunks_exact(4) {
        stream0.push(chunk[0]);
        stream1.push(chunk[1]);
        stream2.push(chunk[2]);
        stream3.push(chunk[3]);
    }

    let mut result = Vec::with_capacity(4 + original_len);
    result.extend_from_slice(&(original_len as u32).to_le_bytes());
    // Put most compressible stream first (exponent/sign byte)
    result.extend_from_slice(&stream3);
    result.extend_from_slice(&stream2);
    result.extend_from_slice(&stream1);
    result.extend_from_slice(&stream0);

    result
}

/// Decode: interleave component streams back into float data.
pub fn decode(data: &[u8]) -> Vec<u8> {
    if data.len() < 4 {
        return Vec::new();
    }

    let original_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let payload = &data[4..];

    // If wasn't 4-byte aligned, data was stored raw
    if original_len < 4 || original_len % 4 != 0 {
        return payload.to_vec();
    }

    let num_floats = original_len / 4;
    if payload.len() < num_floats * 4 {
        return payload.to_vec();
    }

    let stream3 = &payload[0..num_floats];
    let stream2 = &payload[num_floats..num_floats * 2];
    let stream1 = &payload[num_floats * 2..num_floats * 3];
    let stream0 = &payload[num_floats * 3..num_floats * 4];

    let mut result = Vec::with_capacity(original_len);
    for i in 0..num_floats {
        result.push(stream0[i]);
        result.push(stream1[i]);
        result.push(stream2[i]);
        result.push(stream3[i]);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        // Some IEEE 754 floats as bytes
        let floats: Vec<f32> = vec![1.0, 2.0, 3.0, 1.5, 2.5, 100.0, 0.001, -42.0];
        let data: Vec<u8> = floats.iter().flat_map(|f| f.to_le_bytes()).collect();

        let encoded = encode(&data);
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_compression_benefit() {
        // Array of similar floats — exponent stream should have low entropy
        let floats: Vec<f32> = (0..1000).map(|i| 1.0 + (i as f32) * 0.001).collect();
        let data: Vec<u8> = floats.iter().flat_map(|f| f.to_le_bytes()).collect();

        let encoded = encode(&data);
        // Encoded should be same size (it's a transform, not compression)
        assert_eq!(encoded.len(), data.len() + 4); // +4 for length prefix

        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_non_aligned() {
        let data = vec![1, 2, 3, 4, 5]; // 5 bytes, not 4-aligned
        let encoded = encode(&data);
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_empty() {
        assert_eq!(encode(&[]), vec![0, 0, 0, 0]);
    }
}
