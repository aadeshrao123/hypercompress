use crate::format::DataType;

/// Default chunk size: 256KB — large enough for good LZ matching
pub const DEFAULT_CHUNK_SIZE: usize = 256 * 1024;
/// Minimum chunk: 4KB (avoid per-chunk overhead dominating)
pub const MIN_CHUNK_SIZE: usize = 4 * 1024;
/// Maximum chunk: 1MB
pub const MAX_CHUNK_SIZE: usize = 1024 * 1024;

/// A chunk of data with its detected type and original offset.
#[derive(Debug)]
pub struct Chunk {
    pub data: Vec<u8>,
    pub original_offset: u64,
    pub data_type: DataType,
}

/// Adaptively split data into chunks.
/// For small files (<1MB): use one chunk to maximize LZ window coverage.
/// For medium files: use 256KB chunks for good balance.
/// For large files: split at detected data type boundaries when possible.
pub fn split_into_chunks(data: &[u8], chunk_size: usize) -> Vec<Chunk> {
    // For files smaller than 2 chunks, just use one chunk
    if data.len() <= chunk_size * 2 {
        return split_adaptive(data);
    }

    let size = chunk_size.clamp(MIN_CHUNK_SIZE, MAX_CHUNK_SIZE);
    let mut chunks = Vec::new();
    let mut offset = 0usize;

    while offset < data.len() {
        let end = (offset + size).min(data.len());
        chunks.push(Chunk {
            data: data[offset..end].to_vec(),
            original_offset: offset as u64,
            data_type: DataType::Binary,
        });
        offset = end;
    }

    chunks
}

/// Adaptive splitting: scan for data type transitions and split at boundaries.
fn split_adaptive(data: &[u8]) -> Vec<Chunk> {
    if data.is_empty() {
        return Vec::new();
    }

    // Sample every 4KB to find where data characteristics change
    let sample_step = 4096;
    let mut regions: Vec<(usize, usize, f64, f64)> = Vec::new(); // (start, end, entropy, zero_ratio)

    let mut pos = 0;
    while pos < data.len() {
        let end = (pos + sample_step).min(data.len());
        let slice = &data[pos..end];

        let mut histogram = [0u32; 256];
        let mut zeros = 0u32;
        for &b in slice {
            histogram[b as usize] += 1;
            if b == 0 {
                zeros += 1;
            }
        }

        let len = slice.len() as f64;
        let mut entropy = 0.0;
        for &count in &histogram {
            if count > 0 {
                let p = count as f64 / len;
                entropy -= p * p.log2();
            }
        }

        regions.push((pos, end, entropy, zeros as f64 / len));
        pos = end;
    }

    // Merge adjacent regions with similar characteristics into chunks
    let mut chunks = Vec::new();
    let mut chunk_start = 0;

    for i in 1..regions.len() {
        let prev = &regions[i - 1];
        let curr = &regions[i];

        // Detect a significant transition (sparse↔non-sparse, high↔low entropy)
        let entropy_change = (curr.2 - prev.2).abs();
        let zero_change = (curr.3 - prev.3).abs();
        let is_transition = entropy_change > 2.5 || zero_change > 0.4;

        let chunk_len = curr.1 - chunk_start;

        if is_transition && chunk_len >= MIN_CHUNK_SIZE {
            chunks.push(Chunk {
                data: data[chunk_start..curr.0].to_vec(),
                original_offset: chunk_start as u64,
                data_type: DataType::Binary,
            });
            chunk_start = curr.0;
        }

        // Also split if chunk gets too large
        if chunk_len >= MAX_CHUNK_SIZE {
            chunks.push(Chunk {
                data: data[chunk_start..curr.0].to_vec(),
                original_offset: chunk_start as u64,
                data_type: DataType::Binary,
            });
            chunk_start = curr.0;
        }
    }

    // Final chunk
    if chunk_start < data.len() {
        chunks.push(Chunk {
            data: data[chunk_start..data.len()].to_vec(),
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
    fn test_split_chunks() {
        let data = vec![0u8; 600_000]; // > 2 * 256KB
        let chunks = split_into_chunks(&data, DEFAULT_CHUNK_SIZE);
        assert!(chunks.len() >= 2);
        assert!(chunks.len() <= 4);
    }

    #[test]
    fn test_split_small() {
        let data = vec![42u8; 100];
        let chunks = split_into_chunks(&data, DEFAULT_CHUNK_SIZE);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].data.len(), 100);
    }

    #[test]
    fn test_adaptive_split() {
        // Create data with a clear transition: non-sparse → sparse
        let mut data = Vec::new();
        // 50KB of text-like data
        for i in 0..50_000 {
            data.push((65 + (i % 26)) as u8);
        }
        // 50KB of sparse data
        data.extend_from_slice(&vec![0u8; 50_000]);

        let chunks = split_into_chunks(&data, DEFAULT_CHUNK_SIZE);
        // Should detect the transition and split into 2 chunks
        assert!(
            chunks.len() >= 2,
            "Expected >=2 chunks for transition data, got {}",
            chunks.len()
        );
    }

    #[test]
    fn test_medium_file_single_chunk() {
        // File smaller than 2*chunk_size should be processed adaptively
        let data = vec![42u8; 300_000];
        let chunks = split_into_chunks(&data, DEFAULT_CHUNK_SIZE);
        // Uniform data → should be 1 chunk (no transitions)
        assert_eq!(chunks.len(), 1);
    }
}
