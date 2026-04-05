/// Burrows-Wheeler Transform. Groups bytes by their following context,
/// producing output that's highly amenable to entropy coding.

/// Returns transformed data with the original index prepended as 4 bytes LE.
pub fn forward(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }

    let n = data.len();
    let mut rot: Vec<usize> = (0..n).collect();

    rot.sort_by(|&a, &b| {
        for k in 0..n {
            let ca = data[(a + k) % n];
            let cb = data[(b + k) % n];
            match ca.cmp(&cb) {
                std::cmp::Ordering::Equal => continue,
                ord => return ord,
            }
        }
        std::cmp::Ordering::Equal
    });

    let idx = rot.iter().position(|&s| s == 0).unwrap() as u32;
    let mut out = Vec::with_capacity(4 + n);
    out.extend_from_slice(&idx.to_le_bytes());
    for &r in &rot {
        out.push(data[(r + n - 1) % n]);
    }
    out
}

/// Reconstruct original data using the LF-mapping.
pub fn inverse(data: &[u8]) -> Vec<u8> {
    if data.len() <= 4 {
        return Vec::new();
    }

    let idx = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let bwt = &data[4..];
    let n = bwt.len();

    let mut count = [0u32; 256];
    for &b in bwt {
        count[b as usize] += 1;
    }

    let mut cumul = [0u32; 256];
    let mut sum = 0u32;
    for i in 0..256 {
        cumul[i] = sum;
        sum += count[i];
    }

    let mut lf = vec![0u32; n];
    let mut running = cumul;
    for i in 0..n {
        let b = bwt[i] as usize;
        lf[i] = running[b];
        running[b] += 1;
    }

    let mut out = vec![0u8; n];
    let mut pos = idx;
    for i in (0..n).rev() {
        out[i] = bwt[pos];
        pos = lf[pos] as usize;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banana() {
        let orig = b"banana";
        assert_eq!(inverse(&forward(orig)), orig);
    }

    #[test]
    fn sentence() {
        let orig = b"the quick brown fox jumps over the lazy dog";
        assert_eq!(inverse(&forward(orig)), orig);
    }

    #[test]
    fn repeated_pattern() {
        let orig = b"abcabcabcabcabcabc";
        assert_eq!(inverse(&forward(orig)), orig);
    }

    #[test]
    fn longer_with_repeats() {
        let orig = b"The quick brown fox jumps over the lazy dog. Repeated text here. The quick brown fox again.";
        assert_eq!(inverse(&forward(orig)), &orig[..]);
    }

    #[test]
    fn single_byte() {
        assert_eq!(inverse(&forward(b"a")), b"a");
    }

    #[test]
    fn empty() {
        assert_eq!(forward(b""), Vec::<u8>::new());
        assert_eq!(inverse(&[]), Vec::<u8>::new());
    }
}
