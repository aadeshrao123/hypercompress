use std::io::{self, Write};

use crate::chunk::{self, Chunk};
use crate::entropy;
use crate::fingerprint;
use crate::format::{ChunkMeta, CodecType, FileHeader, FORMAT_VERSION};
use crate::transform;

/// Compress input data and write a .hc file to the writer.
pub fn compress<W: Write>(data: &[u8], writer: &mut W) -> io::Result<CompressStats> {
    let mut stats = CompressStats::default();
    stats.original_size = data.len() as u64;

    // Step 1: Split into chunks
    let mut chunks = chunk::split_into_chunks(data, chunk::DEFAULT_CHUNK_SIZE);

    // Step 2: Fingerprint each chunk to detect data types
    fingerprint::fingerprint_chunks(&mut chunks);

    // Step 3: Process each chunk (transform → entropy code)
    let mut compressed_chunks: Vec<CompressedChunk> = Vec::with_capacity(chunks.len());

    for chunk in &chunks {
        let cc = compress_chunk(chunk);
        compressed_chunks.push(cc);
    }

    // Step 4: Write the .hc file

    // First, compute layout: header → chunks → segment map
    // We need to know chunk offsets, so we'll buffer to compute them.
    let header_size = FileHeader::SIZE as u64;
    let mut chunk_offset = header_size;

    let mut metas: Vec<ChunkMeta> = Vec::with_capacity(compressed_chunks.len());

    for (i, cc) in compressed_chunks.iter().enumerate() {
        // Each chunk in file: 4 (compressed_size) + 4 (original_size) + 1 (type) + 1 (transform) + 1 (codec) + 4 (checksum) + data
        let chunk_header_size = 4 + 4 + 1 + 1 + 1 + 4; // 15 bytes
        let chunk_file_size = chunk_header_size + cc.data.len();

        metas.push(ChunkMeta {
            offset_in_file: chunk_offset,
            original_offset: chunks[i].original_offset,
            original_size: chunks[i].data.len() as u32,
            compressed_size: cc.data.len() as u32,
            data_type: chunks[i].data_type,
            transform: cc.transform,
            codec: cc.codec,
            checksum: cc.checksum,
        });

        chunk_offset += chunk_file_size as u64;
    }

    let segment_map_offset = chunk_offset;

    // Write header
    let header = FileHeader {
        version: FORMAT_VERSION,
        flags: 0,
        original_size: data.len() as u64,
        chunk_count: compressed_chunks.len() as u32,
        segment_map_offset,
    };
    header.write_to(writer)?;

    // Write chunks
    let mut total_compressed = 0u64;
    for (i, cc) in compressed_chunks.iter().enumerate() {
        writer.write_all(&(cc.data.len() as u32).to_le_bytes())?;
        writer.write_all(&(chunks[i].data.len() as u32).to_le_bytes())?;
        writer.write_all(&[chunks[i].data_type as u8])?;
        writer.write_all(&[cc.transform as u8])?;
        writer.write_all(&[cc.codec as u8])?;
        writer.write_all(&cc.checksum.to_le_bytes())?;
        writer.write_all(&cc.data)?;
        total_compressed += cc.data.len() as u64;
    }

    // Write segment map
    for meta in &metas {
        meta.write_to(writer)?;
    }

    stats.compressed_size = segment_map_offset + (metas.len() as u64 * ChunkMeta::SIZE as u64);
    stats.chunk_count = metas.len() as u32;
    stats.data_compressed = total_compressed;

    // Count types
    for meta in &metas {
        match meta.data_type {
            crate::format::DataType::Text => stats.text_chunks += 1,
            crate::format::DataType::Structured => stats.structured_chunks += 1,
            crate::format::DataType::Binary => stats.binary_chunks += 1,
            crate::format::DataType::NumericInt => stats.numeric_int_chunks += 1,
            crate::format::DataType::NumericFloat => stats.numeric_float_chunks += 1,
            crate::format::DataType::CompressedOrRandom => stats.random_chunks += 1,
            crate::format::DataType::Sparse => stats.sparse_chunks += 1,
        }
    }

    Ok(stats)
}

struct CompressedChunk {
    data: Vec<u8>,
    transform: crate::format::TransformType,
    codec: CodecType,
    checksum: u32,
}

fn compress_chunk(chunk: &Chunk) -> CompressedChunk {
    // Select and apply transform based on detected data type
    let transform_type = transform::select_transform(chunk.data_type);
    let transformed = transform::apply_transform(&chunk.data, transform_type);

    // Select and apply entropy coding
    let codec_type = entropy::select_codec(&transformed);
    let encoded = entropy::encode(&transformed, codec_type);

    // Only use compressed version if it's actually smaller
    let (final_data, final_transform, final_codec) = if encoded.len() < chunk.data.len() {
        (encoded, transform_type, codec_type)
    } else {
        // Compression didn't help, store raw
        (
            chunk.data.clone(),
            crate::format::TransformType::None,
            CodecType::Raw,
        )
    };

    // Compute checksum of original data
    let checksum = xxhash_rust::xxh32::xxh32(&chunk.data, 0);

    CompressedChunk {
        data: final_data,
        transform: final_transform,
        codec: final_codec,
        checksum,
    }
}

/// Statistics from a compression operation.
#[derive(Debug, Default)]
pub struct CompressStats {
    pub original_size: u64,
    pub compressed_size: u64,
    pub data_compressed: u64,
    pub chunk_count: u32,
    pub text_chunks: u32,
    pub structured_chunks: u32,
    pub binary_chunks: u32,
    pub numeric_int_chunks: u32,
    pub numeric_float_chunks: u32,
    pub random_chunks: u32,
    pub sparse_chunks: u32,
}

impl CompressStats {
    pub fn ratio(&self) -> f64 {
        if self.compressed_size == 0 {
            return 0.0;
        }
        self.original_size as f64 / self.compressed_size as f64
    }
}
