/// Move-to-Front transform: converts byte streams into small-integer
/// streams by tracking recency. Pairs well with BWT output.

pub fn encode(data: &[u8]) -> Vec<u8> {
    let mut alpha: Vec<u8> = (0..=255).collect();
    let mut out = Vec::with_capacity(data.len());

    for &b in data {
        let pos = alpha.iter().position(|&a| a == b).unwrap();
        out.push(pos as u8);
        if pos > 0 {
            let v = alpha.remove(pos);
            alpha.insert(0, v);
        }
    }

    out
}

pub fn decode(data: &[u8]) -> Vec<u8> {
    let mut alpha: Vec<u8> = (0..=255).collect();
    let mut out = Vec::with_capacity(data.len());

    for &idx in data {
        let pos = idx as usize;
        let b = alpha[pos];
        out.push(b);
        if pos > 0 {
            alpha.remove(pos);
            alpha.insert(0, b);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_chars_produce_zeros() {
        let data = b"aaaaaabbbbbbcccccc";
        let enc = encode(data);
        let zeros = enc.iter().filter(|&&b| b == 0).count();
        assert!(zeros > data.len() / 2, "Expected many zeros, got {}/{}", zeros, data.len());
        assert_eq!(&decode(&enc)[..], &data[..]);
    }

    #[test]
    fn roundtrip() {
        let data = b"bananaaa";
        assert_eq!(&decode(&encode(data))[..], &data[..]);
    }

    #[test]
    fn bwt_like_output() {
        let data = b"eeeeeddddbbbbaaaccc";
        let enc = encode(data);
        assert_eq!(&decode(&enc)[..], &data[..]);
        let small = enc.iter().filter(|&&b| b < 5).count();
        assert!(small > data.len() / 2);
    }

    #[test]
    fn empty() {
        assert_eq!(encode(&[]), Vec::<u8>::new());
        assert_eq!(decode(&[]), Vec::<u8>::new());
    }
}
