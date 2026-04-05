/// Columnar delta transform with auto-detected byte period.
///
/// Detects repeating structure in binary data, splits into columnar
/// streams, and applies per-column linear prediction.
///
/// Format: [4-byte original_len] [2-byte period] [period x 1-byte pred_orders]
///         [col_0_residuals] [col_1_residuals] ... [remainder_bytes]
/// Period 0 means passthrough.

pub fn encode(data: &[u8]) -> Vec<u8> {
    if data.len() < 32 {
        return passthrough(data);
    }

    let period = detect_period(data);
    if period <= 1 || period > 512 || period > data.len() / 4 {
        return passthrough(data);
    }

    let nrec = data.len() / period;
    let rem = data.len() % period;

    let mut orders = Vec::with_capacity(period);
    let mut col_res: Vec<Vec<u8>> = Vec::with_capacity(period);

    for col in 0..period {
        let column: Vec<u8> = (0..nrec).map(|row| data[row * period + col]).collect();
        let best = best_order(&column);
        orders.push(best);
        col_res.push(encode_col(&column, best));
    }

    let mut out = Vec::new();
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&(period as u16).to_le_bytes());
    for &o in &orders {
        out.push(o);
    }
    for cr in &col_res {
        out.extend_from_slice(cr);
    }
    if rem > 0 {
        out.extend_from_slice(&data[nrec * period..]);
    }

    let pt = passthrough(data);
    if out.len() < pt.len() { out } else { pt }
}

pub fn decode(data: &[u8]) -> Vec<u8> {
    if data.len() < 6 {
        return Vec::new();
    }

    let orig_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let period = u16::from_le_bytes([data[4], data[5]]) as usize;

    if orig_len == 0 {
        return Vec::new();
    }
    if period == 0 {
        return data[6..6 + orig_len.min(data.len() - 6)].to_vec();
    }

    let nrec = orig_len / period;
    let rem = orig_len % period;
    let mut pos = 6;

    if pos + period > data.len() {
        return Vec::new();
    }
    let orders = &data[pos..pos + period];
    pos += period;

    let mut cols: Vec<Vec<u8>> = Vec::with_capacity(period);
    for col in 0..period {
        if pos + nrec > data.len() {
            break;
        }
        cols.push(decode_col(&data[pos..pos + nrec], orders[col]));
        pos += nrec;
    }

    let mut out = Vec::with_capacity(orig_len);
    for row in 0..nrec {
        for col in 0..period {
            if col < cols.len() && row < cols[col].len() {
                out.push(cols[col][row]);
            }
        }
    }

    if rem > 0 && pos + rem <= data.len() {
        out.extend_from_slice(&data[pos..pos + rem]);
    }

    out.truncate(orig_len);
    out
}

fn passthrough(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(6 + data.len());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(data);
    out
}

fn detect_period(data: &[u8]) -> usize {
    let slen = 8192.min(data.len());
    let sample = &data[..slen];
    let max_p = 512.min(slen / 4);
    if max_p < 2 {
        return 0;
    }

    let mut best_p = 0;
    let mut best_score = 0.0f64;

    for p in 2..=max_p {
        let comps = (slen - p) as u64;
        if comps == 0 {
            continue;
        }
        let matches: u64 = (0..slen - p)
            .filter(|&i| sample[i] == sample[i + p])
            .count() as u64;

        let score = matches as f64 / comps as f64;
        let adj = score * (1.0 + 1.0 / p as f64);

        if adj > best_score && score > 0.35 {
            best_score = adj;
            best_p = p;
        }
    }

    // Check if a sub-period is the true fundamental
    if best_p > 4 {
        for d in 2..=best_p / 2 {
            if best_p % d != 0 {
                continue;
            }
            let sp = best_p / d;
            if sp < 2 {
                continue;
            }
            let comps = (slen - sp) as u64;
            if comps == 0 {
                continue;
            }
            let matches: u64 = (0..slen - sp)
                .filter(|&i| sample[i] == sample[i + sp])
                .count() as u64;
            if matches as f64 / comps as f64 > 0.33 {
                return sp;
            }
        }
    }

    best_p
}

fn best_order(col: &[u8]) -> u8 {
    (0..=3u8).min_by_key(|&order| {
        let mut cost = 0u64;
        let mut prev = [0u8; 4];
        for (i, &val) in col.iter().enumerate() {
            let r = val.wrapping_sub(predict_col(i, &prev, order));
            cost += r.min(255u8.wrapping_sub(r).wrapping_add(1)) as u64;
            prev[3] = prev[2];
            prev[2] = prev[1];
            prev[1] = prev[0];
            prev[0] = val;
        }
        cost
    }).unwrap()
}

fn encode_col(col: &[u8], order: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(col.len());
    let mut prev = [0u8; 4];
    for (i, &val) in col.iter().enumerate() {
        out.push(val.wrapping_sub(predict_col(i, &prev, order)));
        shift_prev(&mut prev, val);
    }
    out
}

fn decode_col(residuals: &[u8], order: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(residuals.len());
    let mut prev = [0u8; 4];
    for (i, &r) in residuals.iter().enumerate() {
        let val = r.wrapping_add(predict_col(i, &prev, order));
        out.push(val);
        shift_prev(&mut prev, val);
    }
    out
}

#[inline]
fn shift_prev(prev: &mut [u8; 4], val: u8) {
    prev[3] = prev[2];
    prev[2] = prev[1];
    prev[1] = prev[0];
    prev[0] = val;
}

#[inline]
fn predict_col(i: usize, prev: &[u8; 4], order: u8) -> u8 {
    match order {
        0 => 0,
        1 if i >= 1 => prev[0],
        2 if i >= 2 => (2u16 * prev[0] as u16).wrapping_sub(prev[1] as u16) as u8,
        3 if i >= 3 => {
            (3u16 * prev[0] as u16)
                .wrapping_sub(3u16 * prev[1] as u16)
                .wrapping_add(prev[2] as u16) as u8
        }
        2 | 3 if i >= 2 => (2u16 * prev[0] as u16).wrapping_sub(prev[1] as u16) as u8,
        1..=3 if i >= 1 => prev[0],
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_roundtrip() {
        let mut data = Vec::new();
        for i in 0u32..500 {
            data.extend_from_slice(&i.to_le_bytes());
            data.extend_from_slice(&(i as f32 * 1.5).to_le_bytes());
        }
        assert_eq!(decode(&encode(&data)), data, "Structured roundtrip failed");
    }

    #[test]
    fn pseudo_random() {
        let data: Vec<u8> = (0..5000).map(|i| ((i * 7 + 3) % 256) as u8).collect();
        assert_eq!(decode(&encode(&data)), data);
    }

    #[test]
    fn small_passthrough() {
        let data = vec![1, 2, 3, 4, 5];
        assert_eq!(decode(&encode(&data)), data);
    }

    #[test]
    fn detects_u32_period() {
        let mut data = Vec::new();
        for i in 0u32..1000 {
            data.extend_from_slice(&i.to_le_bytes());
        }
        let p = detect_period(&data);
        assert!(p == 4 || p == 2 || p == 1, "Expected small period, got {}", p);
    }

    #[test]
    fn empty() {
        assert_eq!(decode(&encode(&[])), Vec::<u8>::new());
    }
}
