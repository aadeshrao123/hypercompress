use crate::format::DataType;

pub const DEFAULT_CHUNK_SIZE: usize = 256 * 1024;
pub const MIN_CHUNK_SIZE: usize = 4 * 1024;
pub const MAX_CHUNK_SIZE: usize = 1024 * 1024;

#[derive(Debug)]
pub struct Chunk {
    pub data: Vec<u8>,
    pub original_offset: u64,
    pub data_type: DataType,
}

/// Split data into chunks. Small files get adaptive splitting;
/// larger files use fixed-size chunks.
pub fn split_into_chunks(data: &[u8], chunk_size: usize) -> Vec<Chunk> {
    if data.len() <= chunk_size * 2 {
        return split_adaptive(data);
    }

    let size = chunk_size.clamp(MIN_CHUNK_SIZE, MAX_CHUNK_SIZE);
    let mut chunks = Vec::new();
    let mut off = 0usize;

    while off < data.len() {
        let end = (off + size).min(data.len());
        chunks.push(Chunk {
            data: data[off..end].to_vec(),
            original_offset: off as u64,
            data_type: DataType::Binary,
        });
        off = end;
    }

    chunks
}

/// Scan for data type transitions (entropy/zero-ratio shifts) and split there.
fn split_adaptive(data: &[u8]) -> Vec<Chunk> {
    if data.is_empty() {
        return Vec::new();
    }

    let step = 4096;
    let mut regions: Vec<(usize, usize, f64, f64)> = Vec::new();

    let mut pos = 0;
    while pos < data.len() {
        let end = (pos + step).min(data.len());
        let slice = &data[pos..end];

        let mut hist = [0u32; 256];
        let mut zeros = 0u32;
        for &b in slice {
            hist[b as usize] += 1;
            if b == 0 { zeros += 1; }
        }

        let len = slice.len() as f64;
        let mut ent = 0.0;
        for &c in &hist {
            if c > 0 {
                let p = c as f64 / len;
                ent -= p * p.log2();
            }
        }

        regions.push((pos, end, ent, zeros as f64 / len));
        pos = end;
    }

    let mut chunks = Vec::new();
    let mut chunk_start = 0;

    for i in 1..regions.len() {
        let prev = &regions[i - 1];
        let curr = &regions[i];

        let ent_diff = (curr.2 - prev.2).abs();
        let zero_diff = (curr.3 - prev.3).abs();
        let transition = ent_diff > 2.5 || zero_diff > 0.4;

        let chunk_len = curr.1 - chunk_start;

        if transition && chunk_len >= MIN_CHUNK_SIZE {
            chunks.push(Chunk {
                data: data[chunk_start..curr.0].to_vec(),
                original_offset: chunk_start as u64,
                data_type: DataType::Binary,
            });
            chunk_start = curr.0;
        }

        if chunk_len >= MAX_CHUNK_SIZE {
            chunks.push(Chunk {
                data: data[chunk_start..curr.0].to_vec(),
                original_offset: chunk_start as u64,
                data_type: DataType::Binary,
            });
            chunk_start = curr.0;
        }
    }

    if chunk_start < data.len() {
        chunks.push(Chunk {
            data: data[chunk_start..].to_vec(),
            original_offset: chunk_start as u64,
            data_type: DataType::Binary,
        });
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_large_data() {
        let data = vec![0u8; 600_000];
        let chunks = split_into_chunks(&data, DEFAULT_CHUNK_SIZE);
        assert!(chunks.len() >= 2 && chunks.len() <= 4);
    }

    #[test]
    fn keeps_small_data_together() {
        let chunks = split_into_chunks(&vec![42u8; 100], DEFAULT_CHUNK_SIZE);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].data.len(), 100);
    }

    #[test]
    fn detects_transition() {
        let mut data = Vec::new();
        for i in 0..50_000 {
            data.push((65 + (i % 26)) as u8);
        }
        data.extend_from_slice(&vec![0u8; 50_000]);

        let chunks = split_into_chunks(&data, DEFAULT_CHUNK_SIZE);
        assert!(chunks.len() >= 2, "got {} chunks", chunks.len());
    }

    #[test]
    fn uniform_stays_single() {
        let chunks = split_into_chunks(&vec![42u8; 300_000], DEFAULT_CHUNK_SIZE);
        assert_eq!(chunks.len(), 1);
    }
}
