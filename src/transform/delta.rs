// Delta encoding with auto-detected stride.
// Byte-level delta for generic data; stride-2/4 for 16-bit PCM audio.
// Header: [1 byte stride] [delta-encoded data]

pub fn encode(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }
    let stride = detect_stride(data);
    let mut out = Vec::with_capacity(1 + data.len());
    out.push(stride as u8);
    out.extend_from_slice(&data[..stride]);
    for i in stride..data.len() {
        out.push(data[i].wrapping_sub(data[i - stride]));
    }
    out
}

pub fn decode(data: &[u8]) -> Vec<u8> {
    if data.len() < 2 {
        return data.to_vec();
    }
    let stride = data[0] as usize;
    if stride == 0 || stride > data.len() - 1 {
        return data[1..].to_vec();
    }
    let body = &data[1..];
    let mut out = Vec::with_capacity(body.len());
    out.extend_from_slice(&body[..stride.min(body.len())]);
    for i in stride..body.len() {
        out.push(body[i].wrapping_add(out[i - stride]));
    }
    out
}

fn detect_stride(data: &[u8]) -> usize {
    // WAV header: RIFF....WAVE
    if data.len() > 44 && &data[0..4] == b"RIFF" && &data[8..12] == b"WAVE" {
        let channels = u16::from_le_bytes([data[22], data[23]]) as usize;
        let bits = u16::from_le_bytes([data[34], data[35]]) as usize;
        let frame = channels * (bits / 8);
        if frame >= 2 && frame <= 8 { return frame; }
    }

    // try strides 1, 2, 4 — pick smallest delta variance
    let sample = &data[..data.len().min(4096)];
    let mut best_stride = 1;
    let mut best_score = u64::MAX;
    for &s in &[1, 2, 4] {
        if sample.len() <= s { continue; }
        let score: u64 = sample[s..].iter().enumerate()
            .map(|(i, &b)| {
                let d = b.wrapping_sub(sample[i]) as i8;
                (d as i64 * d as i64) as u64
            })
            .sum();
        if score < best_score {
            best_score = score;
            best_stride = s;
        }
    }
    best_stride
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequential_data_produces_constant_deltas() {
        let data: Vec<u8> = (0..100).collect();
        let enc = encode(&data);
        // enc[0] = stride, enc[1] = first byte, enc[2..] = deltas
        for &b in &enc[2..] {
            assert_eq!(b, 1);
        }
        assert_eq!(decode(&enc), data);
    }

    #[test]
    fn wrapping_arithmetic() {
        let data = vec![255, 0, 1, 254];
        assert_eq!(decode(&encode(&data)), data);
    }

    #[test]
    fn roundtrip() {
        let data = vec![10, 12, 15, 14, 20, 25, 30];
        assert_eq!(decode(&encode(&data)), data);
    }

    #[test]
    fn empty() {
        assert_eq!(encode(&[]), Vec::<u8>::new());
    }

    #[test]
    fn stride2_pcm_like() {
        // 16-bit little-endian samples: 100, 102, 104, 106
        let data: Vec<u8> = (0u16..200).flat_map(|v| v.to_le_bytes()).collect();
        let enc = encode(&data);
        assert_eq!(enc[0], 2); // auto-detected stride 2
        assert_eq!(decode(&enc), data);
    }
}
