//! Burrows-Wheeler Transform using SA-IS (Suffix Array by Induced Sorting).

const EMPTY: u32 = u32::MAX;

pub fn forward(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }

    let n = data.len();
    // Build SA on TT$ (doubled text + sentinel) to sort cyclic rotations.
    // Suffixes at positions 0..n-1 in TT$ correspond exactly to rotations of T.
    let dlen = 2 * n + 1;
    let mut text = Vec::with_capacity(dlen);
    for &b in data { text.push(b as u32 + 1); }
    for &b in data { text.push(b as u32 + 1); }
    text.push(0);

    let mut full_sa = vec![0u32; dlen];
    sais_int(&text, &mut full_sa, dlen, 257);

    // Extract only positions < n (these are the rotation starting positions)
    let mut out = Vec::with_capacity(4 + n);
    let mut orig_idx = 0u32;
    let mut rank = 0u32;
    for &sa_val in &full_sa {
        let pos = sa_val as usize;
        if pos < n {
            if pos == 0 { orig_idx = rank; }
            out.push(data[(pos + n - 1) % n]);
            rank += 1;
        }
    }

    let mut result = Vec::with_capacity(4 + n);
    result.extend_from_slice(&orig_idx.to_le_bytes());
    result.extend_from_slice(&out);
    result
}

fn sais_int(text: &[u32], sa: &mut [u32], n: usize, alpha_size: usize) {
    if n <= 2 {
        if n == 1 {
            sa[0] = 0;
        } else if text[0] < text[1] {
            sa[0] = 0; sa[1] = 1;
        } else {
            sa[0] = 1; sa[1] = 0;
        }
        return;
    }

    let mut t = vec![false; n]; // true = S-type
    t[n - 1] = true;
    for i in (0..n - 1).rev() {
        t[i] = text[i] < text[i + 1] || (text[i] == text[i + 1] && t[i + 1]);
    }

    let buckets = bucket_sizes(text, n, alpha_size);

    let lms: Vec<u32> = (1..n)
        .filter(|&i| t[i] && !t[i - 1])
        .map(|i| i as u32)
        .collect();

    for v in sa.iter_mut().take(n) { *v = EMPTY; }
    let mut tails = bucket_tails(&buckets);
    for &pos in lms.iter().rev() {
        let c = text[pos as usize] as usize;
        tails[c] -= 1;
        sa[tails[c]] = pos;
    }

    induce_l(sa, text, &t, &buckets, n);
    induce_s(sa, text, &t, &buckets, n);

    let mut sorted_lms: Vec<u32> = Vec::with_capacity(lms.len());
    for &v in sa.iter().take(n) {
        if v != EMPTY && v > 0 && (v as usize) < n && t[v as usize] && !t[v as usize - 1] {
            sorted_lms.push(v);
        }
    }

    if sorted_lms.len() <= 1 {
        return;
    }

    let mut name_at = vec![EMPTY; n];
    let mut name = 0u32;
    name_at[sorted_lms[0] as usize] = 0;
    for i in 1..sorted_lms.len() {
        if !lms_equal(text, &t, sorted_lms[i - 1] as usize, sorted_lms[i] as usize, n) {
            name += 1;
        }
        name_at[sorted_lms[i] as usize] = name;
    }
    let num_names = name + 1;

    let final_lms_order: Vec<u32> = if (num_names as usize) < lms.len() {
        let reduced: Vec<u32> = lms.iter().map(|&p| name_at[p as usize]).collect();
        let mut sub_sa = vec![0u32; reduced.len()];
        sais_int(&reduced, &mut sub_sa, reduced.len(), num_names as usize);
        sub_sa.iter().map(|&i| lms[i as usize]).collect()
    } else {
        sorted_lms
    };

    for v in sa.iter_mut().take(n) { *v = EMPTY; }
    let mut tails = bucket_tails(&buckets);
    for &pos in final_lms_order.iter().rev() {
        let c = text[pos as usize] as usize;
        tails[c] -= 1;
        sa[tails[c]] = pos;
    }

    induce_l(sa, text, &t, &buckets, n);
    induce_s(sa, text, &t, &buckets, n);
}

fn induce_l(sa: &mut [u32], text: &[u32], t: &[bool], buckets: &[usize], n: usize) {
    let mut heads = bucket_heads(buckets);
    for i in 0..n {
        if sa[i] == EMPTY || sa[i] == 0 { continue; }
        let j = sa[i] as usize - 1;
        if !t[j] {
            let c = text[j] as usize;
            sa[heads[c]] = j as u32;
            heads[c] += 1;
        }
    }
}

fn induce_s(sa: &mut [u32], text: &[u32], t: &[bool], buckets: &[usize], n: usize) {
    let mut tails = bucket_tails(buckets);
    for i in (0..n).rev() {
        if sa[i] == EMPTY || sa[i] == 0 { continue; }
        let j = sa[i] as usize - 1;
        if t[j] {
            let c = text[j] as usize;
            tails[c] -= 1;
            sa[tails[c]] = j as u32;
        }
    }
}

fn bucket_sizes(text: &[u32], n: usize, alpha_size: usize) -> Vec<usize> {
    let mut b = vec![0usize; alpha_size];
    for i in 0..n { b[text[i] as usize] += 1; }
    b
}

fn bucket_heads(buckets: &[usize]) -> Vec<usize> {
    let mut h = vec![0usize; buckets.len()];
    let mut sum = 0;
    for i in 0..buckets.len() { h[i] = sum; sum += buckets[i]; }
    h
}

fn bucket_tails(buckets: &[usize]) -> Vec<usize> {
    let mut t = vec![0usize; buckets.len()];
    let mut sum = 0;
    for i in 0..buckets.len() { sum += buckets[i]; t[i] = sum; }
    t
}

fn lms_equal(text: &[u32], t: &[bool], a: usize, b: usize, n: usize) -> bool {
    for k in 0.. {
        let ak = a + k;
        let bk = b + k;
        if ak >= n || bk >= n { return false; }
        if text[ak] != text[bk] || t[ak] != t[bk] { return false; }
        if k > 0 && t[ak] && !t[ak - 1] { return true; }
    }
    unreachable!()
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

    #[test]
    fn all_same_bytes() {
        let orig = vec![0x42u8; 1000];
        assert_eq!(inverse(&forward(&orig)), orig);
    }

    #[test]
    fn two_bytes() {
        assert_eq!(inverse(&forward(b"ab")), b"ab");
        assert_eq!(inverse(&forward(b"ba")), b"ba");
        assert_eq!(inverse(&forward(b"aa")), b"aa");
    }

    #[test]
    fn binary_data() {
        let orig: Vec<u8> = (0..=255).collect();
        assert_eq!(inverse(&forward(&orig)), orig);
    }
}
