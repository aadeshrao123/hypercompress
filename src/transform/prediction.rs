/// Adaptive-order linear prediction transform.
///
/// For each block, tests prediction orders 0-3 and picks the one with
/// lowest-entropy residuals:
///   0: raw bytes, 1: delta, 2: linear extrapolation, 3: quadratic
///
/// Format: [4-byte original_len] [2-byte block_size] [order_bytes...] [residuals...]
/// Order bytes pack 4 block orders at 2 bits each.

const BLOCK_SIZE: usize = 1024;

pub fn encode(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return vec![0, 0, 0, 0];
    }

    let nblocks = (data.len() + BLOCK_SIZE - 1) / BLOCK_SIZE;
    let mut orders: Vec<u8> = Vec::with_capacity(nblocks);
    let mut residuals: Vec<u8> = Vec::with_capacity(data.len());

    for bi in 0..nblocks {
        let start = bi * BLOCK_SIZE;
        let block = &data[start..(start + BLOCK_SIZE).min(data.len())];

        let best = (0..=3u8)
            .min_by_key(|&o| predict_cost(block, o))
            .unwrap();

        orders.push(best);
        residuals.extend(encode_block(block, best));
    }

    let order_len = (nblocks + 3) / 4;
    let mut packed = vec![0u8; order_len];
    for (i, &o) in orders.iter().enumerate() {
        packed[i / 4] |= (o & 0x03) << ((i % 4) * 2);
    }

    let mut out = Vec::with_capacity(4 + 2 + order_len + residuals.len());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&(BLOCK_SIZE as u16).to_le_bytes());
    out.extend_from_slice(&packed);
    out.extend_from_slice(&residuals);
    out
}

pub fn decode(data: &[u8]) -> Vec<u8> {
    if data.len() < 6 {
        return Vec::new();
    }

    let orig_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let bsz = u16::from_le_bytes([data[4], data[5]]) as usize;
    if orig_len == 0 || bsz == 0 {
        return Vec::new();
    }

    let nblocks = (orig_len + bsz - 1) / bsz;
    let order_len = (nblocks + 3) / 4;
    if data.len() < 6 + order_len {
        return Vec::new();
    }

    let order_bytes = &data[6..6 + order_len];
    let res_data = &data[6 + order_len..];

    let mut out = Vec::with_capacity(orig_len);
    let mut rpos = 0;

    for bi in 0..nblocks {
        let order = (order_bytes[bi / 4] >> ((bi % 4) * 2)) & 0x03;
        let blen = if bi == nblocks - 1 { orig_len - bi * bsz } else { bsz };

        if rpos + blen > res_data.len() {
            break;
        }
        out.extend(decode_block(&res_data[rpos..rpos + blen], order));
        rpos += blen;
    }

    out.truncate(orig_len);
    out
}

fn predict_cost(block: &[u8], order: u8) -> u64 {
    let mut cost = 0u64;
    for i in 0..block.len() {
        let r = block[i].wrapping_sub(predict(block, i, order));
        cost += r.min(255 - r) as u64;
    }
    cost
}

fn encode_block(block: &[u8], order: u8) -> Vec<u8> {
    (0..block.len())
        .map(|i| block[i].wrapping_sub(predict(block, i, order)))
        .collect()
}

fn decode_block(residuals: &[u8], order: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(residuals.len());
    for i in 0..residuals.len() {
        out.push(residuals[i].wrapping_add(predict(&out, i, order)));
    }
    out
}

#[inline]
fn predict(data: &[u8], i: usize, order: u8) -> u8 {
    match order {
        0 => 0,
        1 if i >= 1 => data[i - 1],
        2 if i >= 2 => (2i16 * data[i - 1] as i16 - data[i - 2] as i16) as u8,
        3 if i >= 3 => {
            (3i16 * data[i - 1] as i16 - 3i16 * data[i - 2] as i16
                + data[i - 3] as i16) as u8
        }
        2 | 3 if i >= 2 => (2i16 * data[i - 1] as i16 - data[i - 2] as i16) as u8,
        1..=3 if i >= 1 => data[i - 1],
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_pseudo_random() {
        let data: Vec<u8> = (0..5000).map(|i| ((i * 7 + 13) % 256) as u8).collect();
        assert_eq!(decode(&encode(&data)), data);
    }

    #[test]
    fn sequential() {
        let data: Vec<u8> = (0..3000).map(|i| (i % 256) as u8).collect();
        assert_eq!(decode(&encode(&data)), data);
    }

    #[test]
    fn quadratic() {
        let data: Vec<u8> = (0..2000).map(|i| ((i * i / 100) % 256) as u8).collect();
        assert_eq!(decode(&encode(&data)), data);
    }

    #[test]
    fn order2_beats_order1_on_linear_ramp() {
        let data: Vec<u8> = (0..2048).map(|i| (i * 3 % 256) as u8).collect();
        assert!(predict_cost(&data, 2) <= predict_cost(&data, 1));
    }

    #[test]
    fn small() {
        let data = vec![1, 2, 3, 4, 5];
        assert_eq!(decode(&encode(&data)), data);
    }

    #[test]
    fn empty() {
        assert_eq!(decode(&encode(&[])), Vec::<u8>::new());
    }
}
