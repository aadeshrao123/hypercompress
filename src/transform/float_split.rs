/// Float split: separate IEEE 754 f32 bytes into per-component streams.
///
/// Format: [4-byte original_len] [byte3 stream] [byte2] [byte1] [byte0]
/// Falls back to raw storage when input isn't 4-byte aligned.

pub fn encode(data: &[u8]) -> Vec<u8> {
    let len = data.len();

    if len < 4 || len % 4 != 0 {
        let mut out = Vec::with_capacity(4 + len);
        out.extend_from_slice(&(len as u32).to_le_bytes());
        out.extend_from_slice(data);
        return out;
    }

    let n = len / 4;
    let mut s0 = Vec::with_capacity(n);
    let mut s1 = Vec::with_capacity(n);
    let mut s2 = Vec::with_capacity(n);
    let mut s3 = Vec::with_capacity(n);

    for c in data.chunks_exact(4) {
        s0.push(c[0]);
        s1.push(c[1]);
        s2.push(c[2]);
        s3.push(c[3]);
    }

    let mut out = Vec::with_capacity(4 + len);
    out.extend_from_slice(&(len as u32).to_le_bytes());
    out.extend_from_slice(&s3);
    out.extend_from_slice(&s2);
    out.extend_from_slice(&s1);
    out.extend_from_slice(&s0);
    out
}

pub fn decode(data: &[u8]) -> Vec<u8> {
    if data.len() < 4 {
        return Vec::new();
    }

    let len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let payload = &data[4..];

    if len < 4 || len % 4 != 0 {
        return payload.to_vec();
    }

    let n = len / 4;
    if payload.len() < n * 4 {
        return payload.to_vec();
    }

    let s3 = &payload[0..n];
    let s2 = &payload[n..n * 2];
    let s1 = &payload[n * 2..n * 3];
    let s0 = &payload[n * 3..n * 4];

    let mut out = Vec::with_capacity(len);
    for i in 0..n {
        out.push(s0[i]);
        out.push(s1[i]);
        out.push(s2[i]);
        out.push(s3[i]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_aligned_passthrough() {
        let data = vec![1, 2, 3, 4, 5];
        assert_eq!(decode(&encode(&data)), data);
    }

    #[test]
    fn roundtrip() {
        let floats: Vec<f32> = vec![1.0, 2.0, 3.0, 1.5, 2.5, 100.0, 0.001, -42.0];
        let data: Vec<u8> = floats.iter().flat_map(|f| f.to_le_bytes()).collect();
        assert_eq!(decode(&encode(&data)), data);
    }

    #[test]
    fn size_is_transform_only() {
        let floats: Vec<f32> = (0..1000).map(|i| 1.0 + (i as f32) * 0.001).collect();
        let data: Vec<u8> = floats.iter().flat_map(|f| f.to_le_bytes()).collect();
        let enc = encode(&data);
        assert_eq!(enc.len(), data.len() + 4);
        assert_eq!(decode(&enc), data);
    }

    #[test]
    fn empty() {
        assert_eq!(encode(&[]), vec![0, 0, 0, 0]);
    }
}
