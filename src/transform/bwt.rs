/// Burrows-Wheeler Transform implementation.
///
/// Uses cyclic rotation sorting for correct BWT.
/// The BWT groups bytes by their following context, making the output
/// highly compressible by simple entropy coders.

/// Forward BWT: returns transformed data with the original index prepended as 4 bytes (LE).
pub fn forward(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }

    let n = data.len();

    // Build rotation array: indices 0..n representing cyclic rotations
    let mut rotations: Vec<usize> = (0..n).collect();

    // Sort rotations lexicographically using cyclic comparison
    rotations.sort_by(|&a, &b| {
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

    // BWT output: last character of each sorted rotation
    let mut result = Vec::with_capacity(4 + n);

    // Find the original string position (where rotation starts at 0)
    let orig_idx = rotations.iter().position(|&s| s == 0).unwrap() as u32;
    result.extend_from_slice(&orig_idx.to_le_bytes());

    for &rot in &rotations {
        // Last char of rotation starting at `rot` is data[(rot + n - 1) % n]
        result.push(data[(rot + n - 1) % n]);
    }

    result
}

/// Inverse BWT: reconstruct original data from BWT output.
pub fn inverse(data: &[u8]) -> Vec<u8> {
    if data.len() <= 4 {
        return Vec::new();
    }

    let orig_idx = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let bwt = &data[4..];
    let n = bwt.len();

    // Count occurrences of each byte
    let mut count = [0u32; 256];
    for &b in bwt {
        count[b as usize] += 1;
    }

    // Compute cumulative counts (first occurrence of each byte in sorted order)
    let mut cumul = [0u32; 256];
    let mut sum = 0u32;
    for i in 0..256 {
        cumul[i] = sum;
        sum += count[i];
    }

    // Build the LF mapping (transform vector)
    let mut lf = vec![0u32; n];
    let mut running = cumul;
    for i in 0..n {
        let b = bwt[i] as usize;
        lf[i] = running[b];
        running[b] += 1;
    }

    // Reconstruct: follow LF mapping from orig_idx
    let mut result = vec![0u8; n];
    let mut idx = orig_idx;
    for i in (0..n).rev() {
        result[i] = bwt[idx];
        idx = lf[idx] as usize;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let original = b"banana";
        let transformed = forward(original);
        let restored = inverse(&transformed);
        assert_eq!(restored, original);
    }

    #[test]
    fn test_roundtrip_longer() {
        let original = b"the quick brown fox jumps over the lazy dog";
        let transformed = forward(original);
        let restored = inverse(&transformed);
        assert_eq!(restored, original);
    }

    #[test]
    fn test_roundtrip_repeated() {
        let original = b"abcabcabcabcabcabc";
        let transformed = forward(original);
        let restored = inverse(&transformed);
        assert_eq!(restored, original);
    }

    #[test]
    fn test_roundtrip_with_repeated_phrases() {
        let original = b"The quick brown fox jumps over the lazy dog. Repeated text here. The quick brown fox again.";
        let transformed = forward(original);
        let restored = inverse(&transformed);
        assert_eq!(restored, &original[..]);
    }

    #[test]
    fn test_empty() {
        assert_eq!(forward(b""), Vec::<u8>::new());
        assert_eq!(inverse(&[]), Vec::<u8>::new());
    }

    #[test]
    fn test_single_byte() {
        let original = b"a";
        let transformed = forward(original);
        let restored = inverse(&transformed);
        assert_eq!(restored, original);
    }
}
