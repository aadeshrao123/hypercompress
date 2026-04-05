use std::io::{self, Write};

use rayon::prelude::*;

use crate::chunk::{self, Chunk};
use crate::entropy;
use crate::fingerprint;
use crate::format::{ChunkMeta, CodecType, DataType, FileHeader, TransformType, FORMAT_VERSION};
use crate::transform;

pub fn compress<W: Write>(data: &[u8], writer: &mut W) -> io::Result<CompressStats> {
    let mut stats = CompressStats::default();
    stats.original_size = data.len() as u64;

    // Try per-chunk and whole-file LZMA, keep whichever is smaller.

    let mut chunks = chunk::split_into_chunks(data, chunk::DEFAULT_CHUNK_SIZE);
    fingerprint::fingerprint_chunks(&mut chunks);

    let compressed_chunks: Vec<CompressedChunk> = chunks
        .par_iter()
        .map(|chunk| compress_chunk_optimal(chunk))
        .collect();

    let per_chunk_data: usize = compressed_chunks.iter().map(|c| c.data.len()).sum();
    let per_chunk_overhead = FileHeader::SIZE
        + compressed_chunks.len() * 15
        + compressed_chunks.len() * ChunkMeta::SIZE;
    let approach_a = per_chunk_overhead + per_chunk_data;

    let approach_b = if data.len() >= 256 {
        try_whole_file_lzma(data)
    } else {
        None
    };

    let approach_b_size = approach_b
        .as_ref()
        .map(|(d, o, _)| d.len() + o)
        .unwrap_or(usize::MAX);

    if approach_b_size < approach_a {
        let (lzma_data, _, tf) = approach_b.unwrap();
        write_single_chunk(data, &lzma_data, tf, writer, &mut stats)?;
    } else {
        write_chunks(data, &chunks, &compressed_chunks, writer, &mut stats)?;
    }

    Ok(stats)
}

fn try_whole_file_lzma(data: &[u8]) -> Option<(Vec<u8>, usize, TransformType)> {
    let overhead = FileHeader::SIZE + 6;
    let mut best: Option<(Vec<u8>, TransformType)> = None;

    let lzma_raw = entropy::encode(data, CodecType::Lzma);
    if lzma_raw.len() < data.len() {
        best = Some((lzma_raw, TransformType::None));
    }

    let fp = crate::fingerprint::Fingerprint::compute(data);
    let dtype = fp.classify();
    let primary_tf = transform::select_transform(dtype);

    let mut transforms = vec![primary_tf];
    if crate::transform::bcj::is_likely_executable(data) {
        transforms.push(TransformType::Bcj);
    }
    if dtype == DataType::Binary {
        transforms.push(TransformType::Prediction);
        transforms.push(TransformType::Delta);
    }

    for tf in transforms {
        if tf == TransformType::None {
            continue;
        }
        let transformed = transform::apply_transform(data, tf);
        let lzma_tf = entropy::encode(&transformed, CodecType::Lzma);
        if lzma_tf.len() < best.as_ref().map(|(d, _)| d.len()).unwrap_or(data.len()) {
            let dec = entropy::decode(&lzma_tf, CodecType::Lzma);
            let restored = transform::reverse_transform(&dec, tf);
            if restored.len() >= data.len() && restored[..data.len()] == data[..] {
                best = Some((lzma_tf, tf));
            }
        }
    }

    best.map(|(d, tf)| (d, overhead, tf))
}

/// Single-stream mode: [header(27B)] [transform(1B)] [codec(1B)] [checksum(4B)] [data]
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
        flags: 1,
        original_size: original.len() as u64,
        chunk_count: 1,
        segment_map_offset: 0,
    };
    header.write_to(writer)?;

    writer.write_all(&[tf as u8])?;
    writer.write_all(&[CodecType::Lzma as u8])?;
    writer.write_all(&checksum.to_le_bytes())?;
    writer.write_all(compressed)?;

    stats.compressed_size = (FileHeader::SIZE + 6 + compressed.len()) as u64;
    stats.chunk_count = 1;
    stats.binary_chunks = 1;

    Ok(())
}

fn write_chunks<W: Write>(
    _original: &[u8],
    chunks: &[Chunk],
    cc: &[CompressedChunk],
    writer: &mut W,
    stats: &mut CompressStats,
) -> io::Result<()> {
    let header_size = FileHeader::SIZE as u64;
    let mut chunk_offset = header_size;

    let mut metas: Vec<ChunkMeta> = Vec::with_capacity(cc.len());

    for (i, c) in cc.iter().enumerate() {
        let file_size = 15 + c.data.len();
        metas.push(ChunkMeta {
            offset_in_file: chunk_offset,
            original_offset: chunks[i].original_offset,
            original_size: chunks[i].data.len() as u32,
            compressed_size: c.data.len() as u32,
            data_type: chunks[i].data_type,
            transform: c.transform,
            codec: c.codec,
            checksum: c.checksum,
        });
        chunk_offset += file_size as u64;
    }

    let seg_offset = chunk_offset;

    let header = FileHeader {
        version: FORMAT_VERSION,
        flags: 0,
        original_size: chunks.iter().map(|c| c.data.len() as u64).sum(),
        chunk_count: cc.len() as u32,
        segment_map_offset: seg_offset,
    };
    header.write_to(writer)?;

    let mut total_compressed = 0u64;
    for (i, c) in cc.iter().enumerate() {
        writer.write_all(&(c.data.len() as u32).to_le_bytes())?;
        writer.write_all(&(chunks[i].data.len() as u32).to_le_bytes())?;
        writer.write_all(&[chunks[i].data_type as u8])?;
        writer.write_all(&[c.transform as u8])?;
        writer.write_all(&[c.codec as u8])?;
        writer.write_all(&c.checksum.to_le_bytes())?;
        writer.write_all(&c.data)?;
        total_compressed += c.data.len() as u64;
    }

    for meta in &metas {
        meta.write_to(writer)?;
    }

    stats.compressed_size = seg_offset + (metas.len() as u64 * ChunkMeta::SIZE as u64);
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

fn compress_chunk_optimal(chunk: &Chunk) -> CompressedChunk {
    let checksum = xxhash_rust::xxh32::xxh32(&chunk.data, 0);
    let raw_size = chunk.data.len();

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
            candidates.extend([TransformType::BwtMtf, TransformType::Prediction, TransformType::Delta]);
        }
        DataType::Structured => {
            candidates.extend([TransformType::BwtMtf, TransformType::StructSplit, TransformType::Transpose, TransformType::Prediction]);
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
            candidates.extend([TransformType::StructSplit, TransformType::Prediction, TransformType::Delta]);
        }
        DataType::NumericFloat => {
            candidates.extend([TransformType::FloatSplit, TransformType::StructSplit, TransformType::Prediction]);
        }
        DataType::Sparse => {
            candidates.extend([TransformType::Rle, TransformType::Delta, TransformType::Prediction]);
        }
        DataType::CompressedOrRandom => unreachable!(),
    }

    candidates.sort_by_key(|t| *t as u8);
    candidates.dedup();

    let mut best_tf = TransformType::None;
    let mut best_codec = CodecType::Raw;
    let mut best_data = chunk.data.clone();
    let mut best_size = raw_size;

    for &tf in &candidates {
        let transformed = transform::apply_transform(&chunk.data, tf);
        let (codec, encoded) = entropy::select_best_codec(&transformed);

        if encoded.len() < best_size {
            let dec = entropy::decode(&encoded, codec);
            let restored = transform::reverse_transform(&dec, tf);
            if restored.len() < chunk.data.len()
                || restored[..chunk.data.len()] != chunk.data[..]
            {
                continue;
            }

            best_size = encoded.len();
            best_tf = tf;
            best_codec = codec;
            best_data = encoded;
        }
    }

    CompressedChunk {
        data: best_data,
        transform: best_tf,
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
