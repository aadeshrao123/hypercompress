use std::io::{self, Read, Seek, SeekFrom};

use crate::entropy;
use crate::format::{ChunkMeta, CodecType, FileHeader, TransformType};
use crate::transform;

pub fn decompress<R: Read + Seek>(reader: &mut R) -> Result<Vec<u8>, DecompressError> {
    let header = FileHeader::read_from(reader).map_err(DecompressError::Format)?;

    if header.flags & 1 == 1 {
        return decompress_single_stream(reader, &header);
    }

    reader.seek(SeekFrom::Start(header.segment_map_offset))?;

    let mut metas = Vec::with_capacity(header.chunk_count as usize);
    for _ in 0..header.chunk_count {
        metas.push(ChunkMeta::read_from(reader)?);
    }

    let mut result = vec![0u8; header.original_size as usize];

    for meta in &metas {
        reader.seek(SeekFrom::Start(meta.offset_in_file))?;

        let mut skip = [0u8; 15];
        reader.read_exact(&mut skip)?;

        let mut buf = vec![0u8; meta.compressed_size as usize];
        reader.read_exact(&mut buf)?;

        let decoded = entropy::decode(&buf, meta.codec);
        let original = transform::reverse_transform(&decoded, meta.transform);

        let actual = xxhash_rust::xxh32::xxh32(&original, 0);
        if actual != meta.checksum {
            return Err(DecompressError::ChecksumMismatch {
                expected: meta.checksum,
                actual,
            });
        }

        let start = meta.original_offset as usize;
        let end = start + meta.original_size as usize;
        if end <= result.len() && original.len() >= meta.original_size as usize {
            result[start..end].copy_from_slice(&original[..meta.original_size as usize]);
        }
    }

    Ok(result)
}

fn decompress_single_stream<R: Read>(
    reader: &mut R,
    _header: &FileHeader,
) -> Result<Vec<u8>, DecompressError> {
    let mut b1 = [0u8; 1];
    let mut b4 = [0u8; 4];

    reader.read_exact(&mut b1)?;
    let tf = TransformType::from_u8(b1[0]).unwrap_or(TransformType::None);

    reader.read_exact(&mut b1)?;
    let codec = CodecType::from_u8(b1[0]).unwrap_or(CodecType::Raw);

    reader.read_exact(&mut b4)?;
    let expected = u32::from_le_bytes(b4);

    let mut compressed = Vec::new();
    reader.read_to_end(&mut compressed)?;

    let decoded = entropy::decode(&compressed, codec);
    let original = transform::reverse_transform(&decoded, tf);

    let actual = xxhash_rust::xxh32::xxh32(&original, 0);
    if actual != expected {
        return Err(DecompressError::ChecksumMismatch { expected, actual });
    }

    Ok(original)
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
                write!(f, "checksum mismatch: expected 0x{:08X}, got 0x{:08X}", expected, actual)
            }
        }
    }
}

impl std::error::Error for DecompressError {}
