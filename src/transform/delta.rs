/// Delta encoding: store differences between consecutive bytes.

pub fn encode(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(data.len());
    out.push(data[0]);
    for i in 1..data.len() {
        out.push(data[i].wrapping_sub(data[i - 1]));
    }
    out
}

pub fn decode(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(data.len());
    out.push(data[0]);
    for i in 1..data.len() {
        out.push(data[i].wrapping_add(out[i - 1]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequential_data_produces_constant_deltas() {
        let data: Vec<u8> = (0..100).collect();
        let enc = encode(&data);
        for &b in &enc[1..] {
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
        assert_eq!(decode(&[]), Vec::<u8>::new());
    }
}
