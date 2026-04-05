// Archive format v2: per-file compression with HCAR header.
//
//   [4-byte magic: "HCAR"] [2-byte version: 2] [4-byte file_count]
//   For each file: [2-byte path_len] [path] [8-byte orig_size] [8-byte comp_size]
//   For each file: [compressed_data]

use std::fs;
use std::io::{self, Cursor, Write};
use std::path::Path;

const ARCHIVE_MAGIC: [u8; 4] = [b'H', b'C', b'A', b'R'];
const ARCHIVE_VERSION: u16 = 2;

#[derive(Debug, Clone)]
pub struct ArchiveEntry {
    pub path: String,
    pub size: u64,
}

/// Compress a file or directory into a .hc archive.
/// Each file is compressed independently with its optimal pipeline.
pub fn compress_folder(input: &Path, output: &Path) -> io::Result<FolderStats> {
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();

    if input.is_dir() {
        collect_files(input, input, &mut entries)?;
    } else {
        let data = fs::read(input)?;
        let name = input.file_name().unwrap_or_default().to_string_lossy().to_string();
        entries.push((name, data));
    }

    entries.sort_by(|a, b| {
        let ka = file_sort_key(&a.0, &a.1);
        let kb = file_sort_key(&b.0, &b.1);
        ka.cmp(&kb).then(a.0.cmp(&b.0))
    });

    let compressed: Vec<(String, u64, Vec<u8>)> = entries
        .into_iter()
        .map(|(path, data)| {
            let orig_size = data.len() as u64;
            let mut comp = Vec::new();
            let _ = crate::compress::compress(&data, &mut comp);
            if comp.len() >= data.len() {
                let mut raw = Vec::with_capacity(1 + data.len());
                raw.push(0x00);
                raw.extend_from_slice(&data);
                (path, orig_size, raw)
            } else {
                let mut buf = Vec::with_capacity(1 + comp.len());
                buf.push(0x01);
                buf.extend_from_slice(&comp);
                (path, orig_size, buf)
            }
        })
        .collect();

    let file = fs::File::create(output)?;
    let mut w = io::BufWriter::new(file);

    w.write_all(&ARCHIVE_MAGIC)?;
    w.write_all(&ARCHIVE_VERSION.to_le_bytes())?;
    w.write_all(&(compressed.len() as u32).to_le_bytes())?;

    let mut total_original = 0u64;
    for (path, orig, comp_data) in &compressed {
        let pb = path.as_bytes();
        w.write_all(&(pb.len() as u16).to_le_bytes())?;
        w.write_all(pb)?;
        w.write_all(&orig.to_le_bytes())?;
        w.write_all(&(comp_data.len() as u64).to_le_bytes())?;
        total_original += orig;
    }

    for (_, _, comp_data) in &compressed {
        w.write_all(comp_data)?;
    }

    w.flush()?;
    drop(w);

    Ok(FolderStats {
        file_count: compressed.len(),
        total_original,
        archive_size: fs::metadata(output)?.len(),
    })
}

pub fn decompress_folder(input: &Path, output_dir: &Path) -> io::Result<Vec<ArchiveEntry>> {
    let data = fs::read(input)?;

    if data.len() < 10 || data[..4] != ARCHIVE_MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not an HC archive"));
    }

    let version = u16::from_le_bytes([data[4], data[5]]);
    if version == 2 {
        decompress_v2(&data, output_dir)
    } else {
        Err(io::Error::new(io::ErrorKind::InvalidData, "legacy v1 archive not supported"))
    }
}

fn decompress_v2(data: &[u8], output_dir: &Path) -> io::Result<Vec<ArchiveEntry>> {
    let file_count = u32::from_le_bytes([data[6], data[7], data[8], data[9]]) as usize;
    let mut pos = 10;

    let mut dir: Vec<(String, u64, u64)> = Vec::with_capacity(file_count);
    for _ in 0..file_count {
        if pos + 2 > data.len() { break; }
        let plen = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;
        if pos + plen > data.len() { break; }
        let path = String::from_utf8_lossy(&data[pos..pos + plen]).to_string();
        pos += plen;
        if pos + 16 > data.len() { break; }
        let orig = u64::from_le_bytes(data[pos..pos + 8].try_into().unwrap());
        pos += 8;
        let comp = u64::from_le_bytes(data[pos..pos + 8].try_into().unwrap());
        pos += 8;
        dir.push((path, orig, comp));
    }

    let mut entries = Vec::with_capacity(file_count);
    for (path, orig_size, comp_size) in &dir {
        let csz = *comp_size as usize;
        if pos + csz > data.len() { break; }

        let cd = &data[pos..pos + csz];
        pos += csz;

        let file_data = if cd.is_empty() {
            Vec::new()
        } else if cd[0] == 0x00 {
            cd[1..].to_vec()
        } else if cd[0] == 0x01 {
            let mut cursor = Cursor::new(&cd[1..]);
            crate::decompress::decompress(&mut cursor).unwrap_or_else(|_| cd[1..].to_vec())
        } else {
            cd.to_vec()
        };

        let fp = output_dir.join(&path);
        if let Some(parent) = fp.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&fp, &file_data)?;

        entries.push(ArchiveEntry { path: path.clone(), size: *orig_size });
    }

    Ok(entries)
}

pub fn is_archive_file(path: &Path) -> bool {
    fs::read(path)
        .map(|d| d.len() >= 4 && d[..4] == ARCHIVE_MAGIC)
        .unwrap_or(false)
}

pub fn is_archive(data: &[u8]) -> bool {
    data.len() >= 4 && data[..4] == ARCHIVE_MAGIC
}

pub fn list_archive(input: &Path) -> io::Result<Vec<(String, u64, u64)>> {
    let data = fs::read(input)?;

    if data.len() < 10 || data[..4] != ARCHIVE_MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not an HC archive"));
    }

    let count = u32::from_le_bytes([data[6], data[7], data[8], data[9]]) as usize;
    let mut pos = 10;
    let mut entries = Vec::with_capacity(count);

    for _ in 0..count {
        if pos + 2 > data.len() { break; }
        let plen = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;
        if pos + plen > data.len() { break; }
        let path = String::from_utf8_lossy(&data[pos..pos + plen]).to_string();
        pos += plen;
        if pos + 16 > data.len() { break; }
        let orig = u64::from_le_bytes(data[pos..pos + 8].try_into().unwrap());
        pos += 8;
        let comp = u64::from_le_bytes(data[pos..pos + 8].try_into().unwrap());
        pos += 8;
        entries.push((path, orig, comp));
    }

    Ok(entries)
}

pub fn archive_stats(input: &Path) -> io::Result<(usize, u64)> {
    let mut count = 0usize;
    let mut total = 0u64;
    if input.is_dir() {
        count_files(input, &mut count, &mut total)?;
    } else {
        count = 1;
        total = fs::metadata(input)?.len();
    }
    Ok((count, total))
}

fn count_files(dir: &Path, count: &mut usize, size: &mut u64) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            count_files(&path, count, size)?;
        } else if path.is_file() {
            *count += 1;
            *size += fs::metadata(&path)?.len();
        }
    }
    Ok(())
}

fn collect_files(
    base: &Path,
    current: &Path,
    entries: &mut Vec<(String, Vec<u8>)>,
) -> io::Result<()> {
    let mut dir_entries: Vec<_> = fs::read_dir(current)?
        .filter_map(|e| e.ok())
        .collect();
    dir_entries.sort_by_key(|e| e.file_name());

    for entry in dir_entries {
        let path = entry.path();
        if path.is_dir() {
            collect_files(base, &path, entries)?;
        } else if path.is_file() {
            let rel = path.strip_prefix(base).unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            entries.push((rel, fs::read(&path)?));
        }
    }
    Ok(())
}

fn file_sort_key(path: &str, data: &[u8]) -> (u8, String) {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    let group = match ext.as_str() {
        "txt" | "md" | "log" | "csv" | "tsv" => 0,
        "py" | "rs" | "js" | "ts" | "c" | "h" | "cpp" | "java" | "go" | "rb" | "sh" => 1,
        "json" | "yaml" | "yml" | "toml" | "xml" | "html" | "htm" | "css" | "svg" => 2,
        "exe" | "dll" | "so" | "dylib" | "o" | "obj" => 3,
        "sav" | "dat" | "bin" | "utrace" => 4,
        "wav" | "raw" | "pcm" | "aiff" => 5,
        "pdf" | "doc" | "docx" | "xls" | "xlsx" => 6,
        "gz" | "bz2" | "xz" | "zst" | "zip" | "rar" | "7z" | "hc" => 10,
        "jpg" | "jpeg" | "png" | "gif" | "webp" => 11,
        "mp3" | "aac" | "ogg" | "flac" | "m4a" => 12,
        "mp4" | "mkv" | "avi" | "mov" | "webm" => 13,
        _ => {
            if data.len() >= 4 {
                if data[0] == 0x1F && data[1] == 0x8B { return (10, ext); }
                if data[0] == 0x50 && data[1] == 0x4B { return (10, ext); }
                if data[0] == 0x89 && data[1] == b'P' { return (11, ext); }
                if data[0] == 0xFF && data[1] == 0xD8 { return (11, ext); }
            }
            7
        }
    };
    (group, ext)
}

pub struct FolderStats {
    pub file_count: usize,
    pub total_original: u64,
    pub archive_size: u64,
}
