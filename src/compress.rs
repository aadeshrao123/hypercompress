use std::io::{self, Write};

use rayon::prelude::*;

use crate::chunk::{self, Chunk};
use crate::entropy;
use crate::fingerprint;
use crate::format::{ChunkMeta, CodecType, DataType, FileHeader, TransformType, FORMAT_VERSION};
use crate::transform;

/// Compress input data and write a .hc file to the writer.
pub fn compress<W: Write>(data: &[u8], writer: &mut W) -> io::Result<CompressStats> {
    let mut stats = CompressStats::default();
    stats.original_size = data.len() as u64;

    // STRATEGY: Try two approaches, keep the smaller one.
    //
    // Approach A: Per-chunk compression (our transforms shine here)
    //   Split → fingerprint → per-chunk transform+codec
    //
    // Approach B: Whole-file LZMA (exploits full-file dictionary)
    //   Single LZMA stream over the entire data
    //
    // We pick whichever produces the smaller output.

    // === Approach A: Per-chunk with transforms ===
    let mut chunks = chunk::split_into_chunks(data, chunk::DEFAULT_CHUNK_SIZE);
    fingerprint::fingerprint_chunks(&mut chunks);

    let compressed_chunks: Vec<CompressedChunk> = chunks
        .par_iter()
        .map(|chunk| compress_chunk_optimal(chunk))
        .collect();

    // Calculate per-chunk total size
    let per_chunk_data_size: usize = compressed_chunks.iter().map(|c| c.data.len()).sum();
    let per_chunk_overhead = FileHeader::SIZE
        + compressed_chunks.len() * 15                    // chunk headers
        + compressed_chunks.len() * ChunkMeta::SIZE;      // segment map
    let approach_a_size = per_chunk_overhead + per_chunk_data_size;

    // === Approach B: Whole-file LZMA (with and without transforms) ===
    // Try the whole file as a single LZMA stream — uses full dictionary.
    // Also try applying the dominant transform first, THEN LZMA.
    let approach_b_result = if data.len() >= 256 {
        let single_overhead = FileHeader::SIZE + 15 + ChunkMeta::SIZE;
        let mut best_b: Option<(Vec<u8>, TransformType)> = None;

        // B1: Raw LZMA
        let lzma_raw = entropy::encode(data, CodecType::Lzma);
        if lzma_raw.len() < data.len() {
            best_b = Some((lzma_raw, TransformType::None));
        }

        // B2: Try dominant transform + LZMA on the whole file
        // Fingerprint the whole file to pick the best transform
        let fp = crate::fingerprint::Fingerprint::compute(data);
        let dtype = fp.classify();
        let primary_tf = transform::select_transform(dtype);

        if primary_tf != TransformType::None {
            let transformed = transform::apply_transform(data, primary_tf);
            let lzma_tf = entropy::encode(&transformed, CodecType::Lzma);
            if lzma_tf.len() < best_b.as_ref().map(|(d, _)| d.len()).unwrap_or(data.len()) {
                // Verify roundtrip
                let dec = entropy::decode(&lzma_tf, CodecType::Lzma);
                let restored = transform::reverse_transform(&dec, primary_tf);
                if restored.len() >= data.len() && restored[..data.len()] == data[..] {
                    best_b = Some((lzma_tf, primary_tf));
                }
            }
        }

        best_b.map(|(d, tf)| (d, single_overhead, tf))
    } else {
        None
    };

    let approach_b_size = approach_b_result
        .as_ref()
        .map(|(d, o, _)| d.len() + o)
        .unwrap_or(usize::MAX);

    // Pick the better approach
    if approach_b_size < approach_a_size {
        // Whole-file LZMA wins — write as single chunk
        let (lzma_data, _, tf) = approach_b_result.unwrap();
        write_single_chunk(data, &lzma_data, tf, writer, &mut stats)?;
    } else {
        // Per-chunk wins — write all chunks
        write_chunks(data, &chunks, &compressed_chunks, writer, &mut stats)?;
    }

    Ok(stats)
}

/// Write a single LZMA-compressed chunk for the whole file.
fn write_single_chunk<W: Write>(
    original: &[u8],
    compressed: &[u8],
    tf: TransformType,
    writer: &mut W,
    stats: &mut CompressStats,
) -> io::Result<()> {
    let checksum = xxhash_rust::xxh32::xxh32(original, 0);

    let header = FileHeader {
        version: FORMAT_VERSION,
        flags: 0,
        original_size: original.len() as u64,
        chunk_count: 1,
        segment_map_offset: (FileHeader::SIZE + 15 + compressed.len()) as u64,
    };
    header.write_to(writer)?;

    // Write chunk
    writer.write_all(&(compressed.len() as u32).to_le_bytes())?;
    writer.write_all(&(original.len() as u32).to_le_bytes())?;
    writer.write_all(&[DataType::Binary as u8])?;
    writer.write_all(&[tf as u8])?;
    writer.write_all(&[CodecType::Lzma as u8])?;
    writer.write_all(&checksum.to_le_bytes())?;
    writer.write_all(compressed)?;

    // Write segment map
    let meta = ChunkMeta {
        offset_in_file: FileHeader::SIZE as u64,
        original_offset: 0,
        original_size: original.len() as u32,
        compressed_size: compressed.len() as u32,
        data_type: DataType::Binary,
        transform: tf,
        codec: CodecType::Lzma,
        checksum,
    };
    meta.write_to(writer)?;

    stats.compressed_size = (FileHeader::SIZE + 15 + compressed.len() + ChunkMeta::SIZE) as u64;
    stats.chunk_count = 1;
    stats.binary_chunks = 1;

    Ok(())
}

/// Write per-chunk compressed data.
fn write_chunks<W: Write>(
    _original: &[u8],
    chunks: &[Chunk],
    compressed_chunks: &[CompressedChunk],
    writer: &mut W,
    stats: &mut CompressStats,
) -> io::Result<()> {
    let header_size = FileHeader::SIZE as u64;
    let mut chunk_offset = header_size;

    let mut metas: Vec<ChunkMeta> = Vec::with_capacity(compressed_chunks.len());

    for (i, cc) in compressed_chunks.iter().enumerate() {
        let chunk_file_size = 15 + cc.data.len();
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

    let header = FileHeader {
        version: FORMAT_VERSION,
        flags: 0,
        original_size: chunks.iter().map(|c| c.data.len() as u64).sum(),
        chunk_count: compressed_chunks.len() as u32,
        segment_map_offset,
    };
    header.write_to(writer)?;

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

    for meta in &metas {
        meta.write_to(writer)?;
    }

    stats.compressed_size = segment_map_offset + (metas.len() as u64 * ChunkMeta::SIZE as u64);
    stats.chunk_count = metas.len() as u32;
    stats.data_compressed = total_compressed;

    for meta in &metas {
        match meta.data_type {
            DataType::Text => stats.text_chunks += 1,
            DataType::Structured => stats.structured_chunks += 1,
            DataType::Binary => stats.binary_chunks += 1,
            DataType::NumericInt => stats.numeric_int_chunks += 1,
            DataType::NumericFloat => stats.numeric_float_chunks += 1,
            DataType::CompressedOrRandom => stats.random_chunks += 1,
            DataType::Sparse => stats.sparse_chunks += 1,
        }
    }

    Ok(())
}

struct CompressedChunk {
    data: Vec<u8>,
    transform: TransformType,
    codec: CodecType,
    checksum: u32,
}

/// Compress a chunk using the OPTIMAL strategy.
fn compress_chunk_optimal(chunk: &Chunk) -> CompressedChunk {
    let checksum = xxhash_rust::xxh32::xxh32(&chunk.data, 0);
    let raw_size = chunk.data.len();

    // For incompressible data: store raw, zero overhead
    if chunk.data_type == DataType::CompressedOrRandom {
        return CompressedChunk {
            data: chunk.data.clone(),
            transform: TransformType::None,
            codec: CodecType::Raw,
            checksum,
        };
    }

    let primary = transform::select_transform(chunk.data_type);
    let mut candidates: Vec<TransformType> = vec![TransformType::None, primary];

    match chunk.data_type {
        DataType::Text => {
            candidates.push(TransformType::BwtMtf);
            candidates.push(TransformType::Prediction);
            candidates.push(TransformType::Delta);
        }
        DataType::Structured => {
            candidates.push(TransformType::BwtMtf);
            candidates.push(TransformType::StructSplit);
            candidates.push(TransformType::Transpose);
            candidates.push(TransformType::Prediction);
        }
        DataType::Binary => {
            candidates.push(TransformType::Prediction);
            candidates.push(TransformType::StructSplit);
            candidates.push(TransformType::Delta);
            if crate::transform::bcj::is_likely_executable(&chunk.data) {
                candidates.push(TransformType::Bcj);
            }
        }
        DataType::NumericInt => {
            candidates.push(TransformType::StructSplit);
            candidates.push(TransformType::Prediction);
            candidates.push(TransformType::Delta);
        }
        DataType::NumericFloat => {
            candidates.push(TransformType::FloatSplit);
            candidates.push(TransformType::StructSplit);
            candidates.push(TransformType::Prediction);
        }
        DataType::Sparse => {
            candidates.push(TransformType::Rle);
            candidates.push(TransformType::Delta);
            candidates.push(TransformType::Prediction);
        }
        DataType::CompressedOrRandom => unreachable!(),
    }

    candidates.sort_by_key(|t| *t as u8);
    candidates.dedup();

    let mut best_transform = TransformType::None;
    let mut best_codec = CodecType::Raw;
    let mut best_data = chunk.data.clone();
    let mut best_size = raw_size;

    for &tf in &candidates {
        let transformed = transform::apply_transform(&chunk.data, tf);
        let (codec, encoded) = entropy::select_best_codec(&transformed);

        if encoded.len() < best_size {
            // Verify roundtrip
            let decoded_entropy = entropy::decode(&encoded, codec);
            let restored = transform::reverse_transform(&decoded_entropy, tf);
            if restored.len() < chunk.data.len()
                || restored[..chunk.data.len()] != chunk.data[..]
            {
                continue;
            }

            best_size = encoded.len();
            best_transform = tf;
            best_codec = codec;
            best_data = encoded;
        }
    }

    CompressedChunk {
        data: best_data,
        transform: best_transform,
        codec: best_codec,
        checksum,
    }
}

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
