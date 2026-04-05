use std::io::{self, Read, Seek, SeekFrom};

use crate::entropy;
use crate::format::{ChunkMeta, FileHeader};
use crate::transform;

/// Decompress a .hc file from a reader, returning the original data.
pub fn decompress<R: Read + Seek>(reader: &mut R) -> Result<Vec<u8>, DecompressError> {
    // Read header
    let header = FileHeader::read_from(reader).map_err(DecompressError::Format)?;

    // Read the segment map (at end of file) for random-access decompression
    reader.seek(SeekFrom::Start(header.segment_map_offset))?;

    let mut metas = Vec::with_capacity(header.chunk_count as usize);
    for _ in 0..header.chunk_count {
        metas.push(ChunkMeta::read_from(reader)?);
    }

    // Decompress each chunk using the segment map
    let mut result = vec![0u8; header.original_size as usize];

    for meta in &metas {
        reader.seek(SeekFrom::Start(meta.offset_in_file))?;

        // Skip the per-chunk header (15 bytes: 4+4+1+1+1+4)
        let mut skip = [0u8; 15];
        reader.read_exact(&mut skip)?;

        let mut compressed_data = vec![0u8; meta.compressed_size as usize];
        reader.read_exact(&mut compressed_data)?;

        // Decode: entropy decode -> reverse transform
        let decoded_entropy = entropy::decode(&compressed_data, meta.codec);
        let original_data = transform::reverse_transform(&decoded_entropy, meta.transform);

        // Verify checksum
        let actual_checksum = xxhash_rust::xxh32::xxh32(&original_data, 0);
        if actual_checksum != meta.checksum {
            return Err(DecompressError::ChecksumMismatch {
                expected: meta.checksum,
                actual: actual_checksum,
            });
        }

        // Copy to output at the correct position
        let start = meta.original_offset as usize;
        let end = start + meta.original_size as usize;
        if end <= result.len() && original_data.len() >= meta.original_size as usize {
            result[start..end].copy_from_slice(&original_data[..meta.original_size as usize]);
        }
    }

    Ok(result)
}

#[derive(Debug)]
pub enum DecompressError {
    Format(crate::format::FormatError),
    Io(io::Error),
    ChecksumMismatch { expected: u32, actual: u32 },
}

impl From<io::Error> for DecompressError {
    fn from(e: io::Error) -> Self {
        DecompressError::Io(e)
    }
}

impl std::fmt::Display for DecompressError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecompressError::Format(e) => write!(f, "format error: {}", e),
            DecompressError::Io(e) => write!(f, "io error: {}", e),
            DecompressError::ChecksumMismatch { expected, actual } => {
                write!(
                    f,
                    "checksum mismatch: expected 0x{:08X}, got 0x{:08X}",
                    expected, actual
                )
            }
        }
    }
}

impl std::error::Error for DecompressError {}
