use crate::format::DataType;

/// Default chunk size: 64KB
pub const DEFAULT_CHUNK_SIZE: usize = 64 * 1024;
/// Minimum chunk: 4KB (avoid per-chunk overhead dominating)
pub const MIN_CHUNK_SIZE: usize = 4 * 1024;
/// Maximum chunk: 256KB
pub const MAX_CHUNK_SIZE: usize = 256 * 1024;

/// A chunk of data with its detected type and original offset.
#[derive(Debug)]
pub struct Chunk {
    pub data: Vec<u8>,
    pub original_offset: u64,
    pub data_type: DataType,
}

/// Split input data into chunks of the target size.
/// Later this will do adaptive boundary detection.
pub fn split_into_chunks(data: &[u8], chunk_size: usize) -> Vec<Chunk> {
    let size = chunk_size.clamp(MIN_CHUNK_SIZE, MAX_CHUNK_SIZE);
    let mut chunks = Vec::new();
    let mut offset = 0usize;

    while offset < data.len() {
        let end = (offset + size).min(data.len());
        chunks.push(Chunk {
            data: data[offset..end].to_vec(),
            original_offset: offset as u64,
            data_type: DataType::Binary, // placeholder, fingerprinter will set this
        });
        offset = end;
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_chunks() {
        let data = vec![0u8; 200_000];
        let chunks = split_into_chunks(&data, DEFAULT_CHUNK_SIZE);
        // 200KB / 64KB = 3 full + 1 partial = 4 chunks
        // Actually: 65536 + 65536 + 65536 + 3392 = 200000
        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0].data.len(), 65536);
        assert_eq!(chunks[3].data.len(), 200_000 - 3 * 65536);
    }

    #[test]
    fn test_split_small() {
        let data = vec![42u8; 100];
        let chunks = split_into_chunks(&data, DEFAULT_CHUNK_SIZE);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].data.len(), 100);
    }
}
