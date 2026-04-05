/// Archive module: bundle multiple files/folders into a single .hc archive.
///
/// Like WinRAR (.rar) or 7-Zip (.7z), but using our compression engine.
///
/// Archive format (prepended before compression):
///   [4-byte magic: "HCAR"]
///   [4-byte file_count]
///   For each file:
///     [2-byte path_len]
///     [path_bytes (UTF-8)]
///     [8-byte file_size]
///   [file_1_data]
///   [file_2_data]
///   ...
///
/// The entire archive blob is then compressed by our normal engine.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const ARCHIVE_MAGIC: [u8; 4] = [b'H', b'C', b'A', b'R'];

/// Entry in the archive directory.
#[derive(Debug, Clone)]
pub struct ArchiveEntry {
    pub path: String, // relative path within archive
    pub size: u64,
}

/// Bundle a directory (or single file) into an archive blob.
/// Returns the raw archive bytes ready for compression.
pub fn pack(input: &Path) -> io::Result<Vec<u8>> {
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();

    if input.is_dir() {
        collect_files(input, input, &mut entries)?;
    } else {
        let data = fs::read(input)?;
        let name = input
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        entries.push((name, data));
    }

    // Sort for deterministic output
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    // Build archive blob
    let mut blob = Vec::new();

    // Magic
    blob.extend_from_slice(&ARCHIVE_MAGIC);

    // File count
    blob.extend_from_slice(&(entries.len() as u32).to_le_bytes());

    // Directory (paths + sizes)
    for (path, data) in &entries {
        let path_bytes = path.as_bytes();
        blob.extend_from_slice(&(path_bytes.len() as u16).to_le_bytes());
        blob.extend_from_slice(path_bytes);
        blob.extend_from_slice(&(data.len() as u64).to_le_bytes());
    }

    // File data
    for (_, data) in &entries {
        blob.extend_from_slice(data);
    }

    Ok(blob)
}

/// Extract an archive blob back to disk.
pub fn unpack(blob: &[u8], output_dir: &Path) -> io::Result<Vec<ArchiveEntry>> {
    if blob.len() < 8 || blob[..4] != ARCHIVE_MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not an HC archive"));
    }

    let file_count = u32::from_le_bytes([blob[4], blob[5], blob[6], blob[7]]) as usize;
    let mut pos = 8;

    // Read directory
    let mut entries: Vec<ArchiveEntry> = Vec::with_capacity(file_count);
    for _ in 0..file_count {
        if pos + 2 > blob.len() {
            break;
        }
        let path_len = u16::from_le_bytes([blob[pos], blob[pos + 1]]) as usize;
        pos += 2;

        if pos + path_len > blob.len() {
            break;
        }
        let path = String::from_utf8_lossy(&blob[pos..pos + path_len]).to_string();
        pos += path_len;

        if pos + 8 > blob.len() {
            break;
        }
        let size = u64::from_le_bytes([
            blob[pos],
            blob[pos + 1],
            blob[pos + 2],
            blob[pos + 3],
            blob[pos + 4],
            blob[pos + 5],
            blob[pos + 6],
            blob[pos + 7],
        ]);
        pos += 8;

        entries.push(ArchiveEntry { path, size });
    }

    // Extract files
    for entry in &entries {
        let file_size = entry.size as usize;
        if pos + file_size > blob.len() {
            break;
        }

        let file_data = &blob[pos..pos + file_size];
        pos += file_size;

        let file_path = output_dir.join(&entry.path);

        // Create parent directories
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(&file_path, file_data)?;
    }

    Ok(entries)
}

/// Check if a blob starts with the archive magic.
pub fn is_archive(data: &[u8]) -> bool {
    data.len() >= 4 && data[..4] == ARCHIVE_MAGIC
}

/// Recursively collect all files in a directory.
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
            let rel_path = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/"); // normalize to forward slashes
            let data = fs::read(&path)?;
            entries.push((rel_path, data));
        }
    }

    Ok(())
}

/// Get total size and file count for display.
pub fn archive_stats(input: &Path) -> io::Result<(usize, u64)> {
    let mut count = 0usize;
    let mut total_size = 0u64;

    if input.is_dir() {
        count_files(input, &mut count, &mut total_size)?;
    } else {
        count = 1;
        total_size = fs::metadata(input)?.len();
    }

    Ok((count, total_size))
}

fn count_files(dir: &Path, count: &mut usize, size: &mut u64) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            count_files(&path, count, size)?;
        } else if path.is_file() {
            *count += 1;
            *size += fs::metadata(&path)?.len();
        }
    }
    Ok(())
}
