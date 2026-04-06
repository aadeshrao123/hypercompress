use std::io::{self, Write};

use rayon::prelude::*;

use crate::chunk::{self, Chunk};
use crate::entropy;
use crate::fingerprint;
use crate::format::{ChunkMeta, CodecType, DataType, FileHeader, TransformType, FORMAT_VERSION};
use crate::transform;

// Like 7-Zip: level controls how hard we search.
// Low levels = ONE transform + ONE codec, no experiments.
// High levels = try multiple, keep best.
static LEVEL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(6);

pub fn set_level(level: u32) {
    LEVEL.store(level.clamp(1, 9), std::sync::atomic::Ordering::Relaxed);
}

pub fn get_level() -> u32 {
    LEVEL.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn compress<W: Write>(data: &[u8], writer: &mut W) -> io::Result<CompressStats> {
    let mut stats = CompressStats::default();
    stats.original_size = data.len() as u64;
    let level = get_level();

    let mut chunks = chunk::split_into_chunks(data, chunk::DEFAULT_CHUNK_SIZE);
    fingerprint::fingerprint_chunks(&mut chunks);

    let compressed_chunks: Vec<CompressedChunk> = chunks
        .par_iter()
        .map(|chunk| compress_chunk(chunk, level))
        .collect();

    let per_chunk_data: usize = compressed_chunks.iter().map(|c| c.data.len()).sum();
    let per_chunk_overhead = FileHeader::SIZE
        + compressed_chunks.len() * 15
        + compressed_chunks.len() * ChunkMeta::SIZE;
    let approach_a = per_chunk_overhead + per_chunk_data;

    // only try whole-file LZMA at level 7+ (expensive comparison)
    let approach_b = if data.len() >= 256 && level >= 7 {
        try_whole_file_lzma(data)
    } else {
        None
    };

    let approach_b_size = approach_b.as_ref()
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

// Like 7-Zip: at low levels, pick ONE transform from fingerprint, ONE codec. Done.
// At high levels, try a few alternatives.
fn compress_chunk(chunk: &Chunk, level: u32) -> CompressedChunk {
    let checksum = xxhash_rust::xxh32::xxh32(&chunk.data, 0);

    if chunk.data_type == DataType::CompressedOrRandom {
        return CompressedChunk {
            data: chunk.data.clone(),
            transform: TransformType::None,
            codec: CodecType::Raw,
            checksum,
        };
    }

    match level {
        1..=2 => compress_fast(chunk, checksum),
        3..=4 => compress_balanced(chunk, checksum),
        5..=6 => compress_good(chunk, checksum),
        _ => compress_best(chunk, checksum),
    }
}

// Level 1-2: ONE transform, ONE fast codec. No verification (codecs are tested).
fn compress_fast(chunk: &Chunk, checksum: u32) -> CompressedChunk {
    let tf = transform::select_transform(chunk.data_type);
    let transformed = transform::apply_transform(&chunk.data, tf);
    let (codec, encoded) = entropy::encode_fast(&transformed);

    if encoded.len() < chunk.data.len() {
        CompressedChunk { data: encoded, transform: tf, codec, checksum }
    } else {
        // transform didn't help, try raw
        let (c2, e2) = entropy::encode_fast(&chunk.data);
        if e2.len() < chunk.data.len() {
            CompressedChunk { data: e2, transform: TransformType::None, codec: c2, checksum }
        } else {
            CompressedChunk { data: chunk.data.clone(), transform: TransformType::None, codec: CodecType::Raw, checksum }
        }
    }
}

// Level 3-4: ONE transform, fast codecs. No verification.
fn compress_balanced(chunk: &Chunk, checksum: u32) -> CompressedChunk {
    let tf = transform::select_transform(chunk.data_type);
    let transformed = transform::apply_transform(&chunk.data, tf);
    let (codec, encoded) = entropy::select_fast_codec(&transformed);

    if encoded.len() < chunk.data.len() {
        CompressedChunk { data: encoded, transform: tf, codec, checksum }
    } else {
        let (c2, e2) = entropy::select_fast_codec(&chunk.data);
        if e2.len() < chunk.data.len() {
            CompressedChunk { data: e2, transform: TransformType::None, codec: c2, checksum }
        } else {
            CompressedChunk { data: chunk.data.clone(), transform: TransformType::None, codec: CodecType::Raw, checksum }
        }
    }
}

// Level 5-6: primary transform + no-transform, pick smaller. Verify roundtrip.
fn compress_good(chunk: &Chunk, checksum: u32) -> CompressedChunk {
    let raw_size = chunk.data.len();
    let primary = transform::select_transform(chunk.data_type);

    let mut best_tf = TransformType::None;
    let mut best_codec = CodecType::Raw;
    let mut best_data = chunk.data.clone();
    let mut best_size = raw_size;

    for &tf in &[primary, TransformType::None] {
        let input = if tf == TransformType::None { chunk.data.clone() } else { transform::apply_transform(&chunk.data, tf) };
        let (codec, encoded) = entropy::select_best_codec(&input);

        if encoded.len() < best_size {
            let dec = entropy::decode(&encoded, codec);
            let restored = transform::reverse_transform(&dec, tf);
            if restored.len() >= chunk.data.len() && restored[..chunk.data.len()] == chunk.data[..] {
                best_size = encoded.len();
                best_tf = tf;
                best_codec = codec;
                best_data = encoded;
            }
        }
    }

    CompressedChunk { data: best_data, transform: best_tf, codec: best_codec, checksum }
}

// Level 7-9: try all plausible transforms × all codecs. Like 7-Zip "Ultra".
fn compress_best(chunk: &Chunk, checksum: u32) -> CompressedChunk {
    let raw_size = chunk.data.len();
    let primary = transform::select_transform(chunk.data_type);
    let mut candidates = vec![TransformType::None, primary];

    match chunk.data_type {
        DataType::Text => candidates.extend([TransformType::BwtMtf, TransformType::Prediction, TransformType::Delta]),
        DataType::Structured => candidates.extend([TransformType::BwtMtf, TransformType::StructSplit, TransformType::Transpose, TransformType::Prediction]),
        DataType::Binary => {
            candidates.extend([TransformType::Prediction, TransformType::StructSplit, TransformType::Delta, TransformType::Precomp]);
            if crate::transform::bcj::is_likely_executable(&chunk.data) {
                candidates.push(TransformType::Bcj);
            }
        }
        DataType::NumericInt => candidates.extend([TransformType::StructSplit, TransformType::Prediction, TransformType::Delta]),
        DataType::NumericFloat => candidates.extend([TransformType::FloatSplit, TransformType::StructSplit, TransformType::Prediction]),
        DataType::Sparse => candidates.extend([TransformType::Rle, TransformType::Delta, TransformType::Prediction]),
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
            best_size = encoded.len();
            best_tf = tf;
            best_codec = codec;
            best_data = encoded;
        }
    }

    CompressedChunk { data: best_data, transform: best_tf, codec: best_codec, checksum }
}

fn try_whole_file_lzma(data: &[u8]) -> Option<(Vec<u8>, usize, TransformType)> {
    let overhead = FileHeader::SIZE + 6;
    let fp = crate::fingerprint::Fingerprint::compute(data);

    if fp.entropy > 7.9 { return None; }

    let dtype = fp.classify();
    if dtype == DataType::CompressedOrRandom || fp.entropy > 7.0 {
        let lzma = entropy::encode(data, CodecType::Lzma);
        return if lzma.len() < data.len() * 97 / 100 {
            Some((lzma, overhead, TransformType::None))
        } else {
            None
        };
    }

    let mut best: Option<(Vec<u8>, TransformType)> = None;

    let lzma_raw = entropy::encode(data, CodecType::Lzma);
    if lzma_raw.len() < data.len() {
        best = Some((lzma_raw, TransformType::None));
    }

    let tf = transform::select_transform(dtype);
    if tf != TransformType::None {
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

fn write_single_chunk<W: Write>(original: &[u8], compressed: &[u8], tf: TransformType, writer: &mut W, stats: &mut CompressStats) -> io::Result<()> {
    let checksum = xxhash_rust::xxh32::xxh32(original, 0);
    let header = FileHeader { version: FORMAT_VERSION, flags: 1, original_size: original.len() as u64, chunk_count: 1, segment_map_offset: 0 };
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

fn write_chunks<W: Write>(_orig: &[u8], chunks: &[Chunk], cc: &[CompressedChunk], writer: &mut W, stats: &mut CompressStats) -> io::Result<()> {
    let mut offset = FileHeader::SIZE as u64;
    let mut metas: Vec<ChunkMeta> = Vec::with_capacity(cc.len());

    for (i, c) in cc.iter().enumerate() {
        metas.push(ChunkMeta {
            offset_in_file: offset,
            original_offset: chunks[i].original_offset,
            original_size: chunks[i].data.len() as u32,
            compressed_size: c.data.len() as u32,
            data_type: chunks[i].data_type,
            transform: c.transform,
            codec: c.codec,
            checksum: c.checksum,
        });
        offset += (15 + c.data.len()) as u64;
    }

    let seg_offset = offset;
    let header = FileHeader { version: FORMAT_VERSION, flags: 0, original_size: chunks.iter().map(|c| c.data.len() as u64).sum(), chunk_count: cc.len() as u32, segment_map_offset: seg_offset };
    header.write_to(writer)?;

    for (i, c) in cc.iter().enumerate() {
        writer.write_all(&(c.data.len() as u32).to_le_bytes())?;
        writer.write_all(&(chunks[i].data.len() as u32).to_le_bytes())?;
        writer.write_all(&[chunks[i].data_type as u8])?;
        writer.write_all(&[c.transform as u8])?;
        writer.write_all(&[c.codec as u8])?;
        writer.write_all(&c.checksum.to_le_bytes())?;
        writer.write_all(&c.data)?;
    }

    for meta in &metas { meta.write_to(writer)?; }

    stats.compressed_size = seg_offset + (metas.len() as u64 * ChunkMeta::SIZE as u64);
    stats.chunk_count = metas.len() as u32;
    stats.data_compressed = cc.iter().map(|c| c.data.len() as u64).sum();

    for m in &metas {
        match m.data_type {
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

struct CompressedChunk { data: Vec<u8>, transform: TransformType, codec: CodecType, checksum: u32 }

#[derive(Debug, Default)]
pub struct CompressStats {
    pub original_size: u64, pub compressed_size: u64, pub data_compressed: u64,
    pub chunk_count: u32,
    pub text_chunks: u32, pub structured_chunks: u32, pub binary_chunks: u32,
    pub numeric_int_chunks: u32, pub numeric_float_chunks: u32,
    pub random_chunks: u32, pub sparse_chunks: u32,
}

impl CompressStats {
    pub fn ratio(&self) -> f64 {
        if self.compressed_size == 0 { 0.0 } else { self.original_size as f64 / self.compressed_size as f64 }
    }
}
