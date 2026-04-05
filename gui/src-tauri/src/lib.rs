use serde::Serialize;
use std::path::PathBuf;
use std::io::Cursor;

#[derive(Serialize)]
struct CompressResult {
    input_path: String,
    output_path: String,
    original_size: u64,
    compressed_size: u64,
    ratio: f64,
    encrypted: bool,
}

#[derive(Serialize)]
struct DecompressResult {
    input_path: String,
    output_path: String,
    size: u64,
}

#[derive(Serialize)]
struct AnalyzeResult {
    entropy: f64,
    ascii_ratio: f64,
    zero_ratio: f64,
    unique_bytes: u16,
    detected_type: String,
}

#[tauri::command]
fn compress_file(path: String, output: Option<String>, level: u32, password: Option<String>) -> Result<CompressResult, String> {
    let input = PathBuf::from(&path);
    let out = output
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let name = input.file_name().unwrap_or_default().to_string_lossy();
            input.parent().unwrap_or(&input).join(format!("{}.hc", name))
        });

    hypercompress::compress::set_level(level);

    if input.is_dir() {
        let stats = hypercompress::archive::compress_folder(&input, &out)
            .map_err(|e| e.to_string())?;

        Ok(CompressResult {
            input_path: path,
            output_path: out.to_string_lossy().to_string(),
            original_size: stats.total_original,
            compressed_size: stats.archive_size,
            ratio: stats.total_original as f64 / stats.archive_size as f64,
            encrypted: false,
        })
    } else {
        let data = std::fs::read(&input).map_err(|e| e.to_string())?;
        let orig_sz = data.len() as u64;

        let mut compressed = Vec::new();
        hypercompress::compress::compress(&data, &mut compressed).map_err(|e| e.to_string())?;

        let final_data = if let Some(ref pw) = password {
            hypercompress::crypto::encrypt(&compressed, pw)
        } else {
            compressed
        };

        std::fs::write(&out, &final_data).map_err(|e| e.to_string())?;

        Ok(CompressResult {
            input_path: path,
            output_path: out.to_string_lossy().to_string(),
            original_size: orig_sz,
            compressed_size: final_data.len() as u64,
            ratio: orig_sz as f64 / final_data.len() as f64,
            encrypted: password.is_some(),
        })
    }
}

#[tauri::command]
fn decompress_file(path: String, output: Option<String>, password: Option<String>) -> Result<DecompressResult, String> {
    let input = PathBuf::from(&path);

    if hypercompress::archive::is_archive_file(&input) {
        let out_dir = output.map(PathBuf::from).unwrap_or_else(|| {
            let name = input.file_name().unwrap_or_default().to_string_lossy().to_string();
            let clean = name.strip_suffix(".hc").unwrap_or(&name);
            input.parent().unwrap_or(&input).join(clean)
        });

        let entries = hypercompress::archive::decompress_folder(&input, &out_dir)
            .map_err(|e| e.to_string())?;

        let total: u64 = entries.iter().map(|e| e.size).sum();
        Ok(DecompressResult {
            input_path: path,
            output_path: out_dir.to_string_lossy().to_string(),
            size: total,
        })
    } else {
        let raw = std::fs::read(&input).map_err(|e| e.to_string())?;

        let compressed = if let Some(ref pw) = password {
            hypercompress::crypto::decrypt(&raw, pw).map_err(|e| e.to_string())?
        } else {
            raw
        };

        let mut cursor = Cursor::new(&compressed);
        let data = hypercompress::decompress::decompress(&mut cursor)
            .map_err(|e| e.to_string())?;

        let out = output.map(PathBuf::from).unwrap_or_else(|| {
            let name = input.file_name().unwrap_or_default().to_string_lossy().to_string();
            let clean = name.strip_suffix(".hc").unwrap_or(&name);
            input.parent().unwrap_or(&input).join(clean)
        });

        std::fs::write(&out, &data).map_err(|e| e.to_string())?;

        Ok(DecompressResult {
            input_path: path,
            output_path: out.to_string_lossy().to_string(),
            size: data.len() as u64,
        })
    }
}

#[tauri::command]
fn analyze_file(path: String) -> Result<AnalyzeResult, String> {
    let data = std::fs::read(&path).map_err(|e| e.to_string())?;
    let fp = hypercompress::fingerprint::Fingerprint::compute(&data);
    Ok(AnalyzeResult {
        entropy: fp.entropy,
        ascii_ratio: fp.ascii_ratio,
        zero_ratio: fp.zero_ratio,
        unique_bytes: fp.unique_bytes,
        detected_type: format!("{:?}", fp.classify()),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            compress_file,
            decompress_file,
            analyze_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
