// Archive format v3: solid groups with per-type compression.
//
// Files are grouped by content type (text together, binary together, etc.)
// and each group is compressed as one solid stream — so LZMA's dictionary
// catches cross-file patterns within the group.
//
//   [4-byte magic: "HCAR"] [2-byte version: 3] [4-byte file_count]
//   Directory: for each file:
//     [2-byte path_len] [path] [8-byte orig_size] [8-byte compressed_size]
//   Data: for each file:
//     [1-byte marker: 0x00=raw, 0x01=individual, 0x02=solid_group]
//     [compressed_data]
//
// Solid groups: marker 0x02, then [4-byte group_file_count] [8-byte offsets per file]
// followed by one compressed blob for the whole group.

use std::fs;
use std::io::{self, Cursor, Write};
use std::path::Path;

const ARCHIVE_MAGIC: [u8; 4] = [b'H', b'C', b'A', b'R'];
const ARCHIVE_VERSION: u16 = 3;

#[derive(Debug, Clone)]
pub struct ArchiveEntry {
    pub path: String,
    pub size: u64,
}

pub fn compress_folder(input: &Path, output: &Path) -> io::Result<FolderStats> {
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();

    if input.is_dir() {
        collect_files(input, input, &mut entries)?;
    } else {
        let data = fs::read(input)?;
        let name = input.file_name().unwrap_or_default().to_string_lossy().to_string();
        entries.push((name, data));
    }

    // sort by content type so similar files are adjacent
    entries.sort_by(|a, b| {
        file_sort_key(&a.0, &a.1).cmp(&file_sort_key(&b.0, &b.1)).then(a.0.cmp(&b.0))
    });

    // group by sort key — files with same type get solid-compressed together
    let groups = group_by_type(&entries);

    let mut compressed: Vec<(String, u64, Vec<u8>)> = Vec::new();

    for group in &groups {
        if group.len() == 1 {
            // single file — compress individually
            let (path, data) = &entries[group[0]];
            let comp = compress_single(data);
            compressed.push((path.clone(), data.len() as u64, comp));
        } else {
            // solid group — concatenate then compress as one stream
            let solid = compress_solid_group(&entries, group);
            for (i, &idx) in group.iter().enumerate() {
                let (path, data) = &entries[idx];
                compressed.push((path.clone(), data.len() as u64, solid[i].clone()));
            }
        }
    }

    // write archive
    let file = fs::File::create(output)?;
    let mut w = io::BufWriter::new(file);

    w.write_all(&ARCHIVE_MAGIC)?;
    w.write_all(&ARCHIVE_VERSION.to_le_bytes())?;
    w.write_all(&(compressed.len() as u32).to_le_bytes())?;

    let mut total_original = 0u64;
    for (path, orig, comp) in &compressed {
        let pb = path.as_bytes();
        w.write_all(&(pb.len() as u16).to_le_bytes())?;
        w.write_all(pb)?;
        w.write_all(&orig.to_le_bytes())?;
        w.write_all(&(comp.len() as u64).to_le_bytes())?;
        total_original += orig;
    }

    for (_, _, comp) in &compressed {
        w.write_all(comp)?;
    }

    w.flush()?;
    drop(w);

    Ok(FolderStats {
        file_count: compressed.len(),
        total_original,
        archive_size: fs::metadata(output)?.len(),
    })
}

fn compress_single(data: &[u8]) -> Vec<u8> {
    let mut comp = Vec::new();
    let _ = crate::compress::compress(data, &mut comp);
    if comp.len() < data.len() {
        let mut buf = Vec::with_capacity(1 + comp.len());
        buf.push(0x01);
        buf.extend_from_slice(&comp);
        buf
    } else {
        let mut buf = Vec::with_capacity(1 + data.len());
        buf.push(0x00);
        buf.extend_from_slice(data);
        buf
    }
}

fn compress_solid_group(entries: &[(String, Vec<u8>)], indices: &[usize]) -> Vec<Vec<u8>> {
    // concatenate all files in the group
    let mut concat = Vec::new();
    let mut offsets: Vec<u64> = Vec::new();
    for &idx in indices {
        offsets.push(concat.len() as u64);
        concat.extend_from_slice(&entries[idx].1);
    }

    // compress the concatenated blob
    let mut solid_compressed = Vec::new();
    let _ = crate::compress::compress(&concat, &mut solid_compressed);

    // if solid didn't help, fall back to individual compression
    let individual_total: usize = indices.iter()
        .map(|&idx| compress_single(&entries[idx].1).len())
        .sum();

    if solid_compressed.len() + 20 >= individual_total {
        // solid didn't win, use individual
        return indices.iter()
            .map(|&idx| compress_single(&entries[idx].1))
            .collect();
    }

    // solid wins — first file gets the blob, rest get references
    let mut result = Vec::new();

    // first entry: marker 0x02 + group count + per-file sizes + compressed blob
    let mut header = Vec::new();
    header.push(0x02);
    header.extend_from_slice(&(indices.len() as u32).to_le_bytes());
    for &idx in indices {
        header.extend_from_slice(&(entries[idx].1.len() as u64).to_le_bytes());
    }
    header.extend_from_slice(&solid_compressed);
    result.push(header);

    // remaining entries: marker 0x03 (part of solid group, data in first entry)
    for _ in 1..indices.len() {
        result.push(vec![0x03]);
    }

    result
}

fn group_by_type(entries: &[(String, Vec<u8>)]) -> Vec<Vec<usize>> {
    if entries.is_empty() {
        return Vec::new();
    }

    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut cur_key = file_sort_key(&entries[0].0, &entries[0].1);
    let mut cur_group = vec![0usize];

    for i in 1..entries.len() {
        let key = file_sort_key(&entries[i].0, &entries[i].1);
        if key == cur_key {
            cur_group.push(i);
        } else {
            groups.push(cur_group);
            cur_group = vec![i];
            cur_key = key;
        }
    }
    groups.push(cur_group);
    groups
}

pub fn decompress_folder(input: &Path, output_dir: &Path) -> io::Result<Vec<ArchiveEntry>> {
    let data = fs::read(input)?;

    if data.len() < 10 || data[..4] != ARCHIVE_MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not an HC archive"));
    }

    let version = u16::from_le_bytes([data[4], data[5]]);
    match version {
        2 | 3 => decompress_v2v3(&data, output_dir),
        _ => Err(io::Error::new(io::ErrorKind::InvalidData,
            format!("unsupported archive version {}", version))),
    }
}

fn decompress_v2v3(data: &[u8], output_dir: &Path) -> io::Result<Vec<ArchiveEntry>> {
    let count = u32::from_le_bytes([data[6], data[7], data[8], data[9]]) as usize;
    let mut pos = 10;

    let mut dir: Vec<(String, u64, u64)> = Vec::with_capacity(count);
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
        dir.push((path, orig, comp));
    }

    let mut entries = Vec::with_capacity(count);
    let mut solid_cache: Option<Vec<u8>> = None;
    let mut solid_sizes: Vec<u64> = Vec::new();
    let mut solid_offset = 0usize;

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
        } else if cd[0] == 0x02 {
            // solid group header
            if cd.len() < 5 { Vec::new() } else {
                let group_count = u32::from_le_bytes([cd[1], cd[2], cd[3], cd[4]]) as usize;
                let mut spos = 5;
                solid_sizes.clear();
                for _ in 0..group_count {
                    if spos + 8 > cd.len() { break; }
                    solid_sizes.push(u64::from_le_bytes(cd[spos..spos + 8].try_into().unwrap()));
                    spos += 8;
                }
                // decompress the solid blob
                let mut cursor = Cursor::new(&cd[spos..]);
                let full = crate::decompress::decompress(&mut cursor).unwrap_or_default();
                solid_offset = 0;
                let sz = solid_sizes.first().copied().unwrap_or(0) as usize;
                let chunk = full[solid_offset..solid_offset + sz.min(full.len())].to_vec();
                solid_offset += sz;
                solid_cache = Some(full);
                chunk
            }
        } else if cd[0] == 0x03 {
            if let Some(ref full) = solid_cache {
                let sz = *orig_size as usize;
                let chunk = if solid_offset + sz <= full.len() {
                    full[solid_offset..solid_offset + sz].to_vec()
                } else {
                    Vec::new()
                };
                solid_offset += sz;
                chunk
            } else {
                Vec::new()
            }
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

fn collect_files(base: &Path, current: &Path, entries: &mut Vec<(String, Vec<u8>)>) -> io::Result<()> {
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
                .to_string_lossy().replace('\\', "/");
            entries.push((rel, fs::read(&path)?));
        }
    }
    Ok(())
}

fn file_sort_key(path: &str, data: &[u8]) -> (u8, String) {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    let group = match ext.as_str() {
        "txt" | "md" | "log" | "csv" | "tsv" => 0,
        "py" | "rs" | "js" | "ts" | "c" | "h" | "cpp" | "java" | "go" => 1,
        "json" | "yaml" | "yml" | "toml" | "xml" | "html" | "htm" | "css" => 2,
        "exe" | "dll" | "so" | "dylib" | "o" | "obj" => 3,
        "sav" | "dat" | "bin" | "utrace" => 4,
        "wav" | "raw" | "pcm" | "aiff" => 5,
        "pdf" | "doc" | "docx" | "xls" | "xlsx" => 6,
        "gz" | "bz2" | "xz" | "zst" | "zip" | "rar" | "7z" | "hc" => 10,
        "jpg" | "jpeg" | "png" | "gif" | "webp" => 11,
        "mp3" | "aac" | "ogg" | "flac" | "m4a" => 12,
        "mp4" | "mkv" | "avi" | "mov" | "webm" => 13,
        _ => {
            if data.len() >= 2 {
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
