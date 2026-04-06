use serde::Serialize;
use std::path::PathBuf;
use std::io::Cursor;
use std::sync::{Arc, atomic::{AtomicU64, AtomicBool, Ordering}};
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Clone)]
struct Progress { pct: f64, msg: String }

#[derive(Serialize, Clone)]
struct CompressResult {
    input_path: String, output_path: String,
    original_size: u64, compressed_size: u64, ratio: f64, encrypted: bool,
}

#[derive(Serialize, Clone)]
struct DecompressResult { output_path: String, size: u64, file_count: usize }

#[derive(Serialize, Clone)]
struct AnalyzeResult {
    entropy: f64, ascii_ratio: f64, zero_ratio: f64,
    unique_bytes: u16, detected_type: String, size: u64,
}

fn emit(app: &AppHandle, pct: f64, msg: &str) {
    let _ = app.emit("progress", Progress { pct, msg: msg.to_string() });
}

#[tauri::command]
async fn compress_file(app: AppHandle, path: String, output: String, level: u32, password: String) -> Result<CompressResult, String> {
    let a = app.clone();
    bg(move || {
        let input = PathBuf::from(&path);
        let out = PathBuf::from(&output);
        let has_pw = !password.is_empty();
        hypercompress::compress::set_level(level.max(1));

        if level == 0 {
            // Store mode: no compression, just copy (optionally encrypt)
            emit(&a, 10.0, "Storing...");
            let data = std::fs::read(&input).map_err(|e| format!("Cannot read: {}", e))?;
            let orig = data.len() as u64;
            let final_data = if has_pw {
                emit(&a, 50.0, "Encrypting...");
                hypercompress::crypto::encrypt(&data, &password)
            } else { data };
            std::fs::write(&out, &final_data).map_err(|e| format!("Cannot write: {}", e))?;
            emit(&a, 100.0, "Done!");
            return Ok(CompressResult {
                input_path: path, output_path: out.to_string_lossy().to_string(),
                original_size: orig, compressed_size: final_data.len() as u64,
                ratio: 1.0, encrypted: has_pw,
            });
        }

        if input.is_dir() {
            compress_folder_with_progress(&a, &input, &out, has_pw, &password, &path)
        } else {
            compress_single_with_progress(&a, &input, &out, has_pw, &password, &path)
        }
    })
}

fn compress_folder_with_progress(
    app: &AppHandle, input: &PathBuf, out: &PathBuf,
    has_pw: bool, password: &str, orig_path: &str,
) -> Result<CompressResult, String> {
    // gather all files
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    collect_dir(input, input, &mut files);
    let total = files.len();
    let total_bytes: u64 = files.iter().map(|(_, d)| d.len() as u64).sum();

    emit(app, 0.0, &format!("0/{} files", total));

    // compress each file individually with progress
    let mut compressed: Vec<(String, u64, Vec<u8>)> = Vec::new();
    let mut processed_bytes = 0u64;

    for (i, (rel, data)) in files.iter().enumerate() {
        let pct = (i as f64 / total as f64) * 85.0;
        emit(app, pct, &format!("{}/{} — {}", i + 1, total, rel.split('/').last().unwrap_or(rel)));

        let orig_sz = data.len() as u64;
        let mut comp = Vec::new();
        let _ = hypercompress::compress::compress(data, &mut comp);

        if comp.len() < data.len() {
            let mut buf = Vec::with_capacity(1 + comp.len());
            buf.push(0x01);
            buf.extend_from_slice(&comp);
            compressed.push((rel.clone(), orig_sz, buf));
        } else {
            let mut buf = Vec::with_capacity(1 + data.len());
            buf.push(0x00);
            buf.extend_from_slice(data);
            compressed.push((rel.clone(), orig_sz, buf));
        }
        processed_bytes += orig_sz;
    }

    emit(app, 88.0, "Writing archive...");

    // write archive in HCAR v3 format
    let file = std::fs::File::create(out).map_err(|e| format!("Cannot create: {}", e))?;
    let mut w = std::io::BufWriter::new(file);
    use std::io::Write;

    w.write_all(b"HCAR").map_err(|e| e.to_string())?;
    w.write_all(&3u16.to_le_bytes()).map_err(|e| e.to_string())?;
    w.write_all(&(compressed.len() as u32).to_le_bytes()).map_err(|e| e.to_string())?;

    for (rel, orig, comp) in &compressed {
        let pb = rel.as_bytes();
        w.write_all(&(pb.len() as u16).to_le_bytes()).map_err(|e| e.to_string())?;
        w.write_all(pb).map_err(|e| e.to_string())?;
        w.write_all(&orig.to_le_bytes()).map_err(|e| e.to_string())?;
        w.write_all(&(comp.len() as u64).to_le_bytes()).map_err(|e| e.to_string())?;
    }
    for (_, _, comp) in &compressed {
        w.write_all(comp).map_err(|e| e.to_string())?;
    }
    w.flush().map_err(|e| e.to_string())?;
    drop(w);

    if has_pw {
        emit(app, 93.0, "Encrypting...");
        let data = std::fs::read(out).map_err(|e| e.to_string())?;
        let enc = hypercompress::crypto::encrypt(&data, password);
        std::fs::write(out, &enc).map_err(|e| e.to_string())?;
    }

    let final_size = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
    emit(app, 100.0, "Done!");

    Ok(CompressResult {
        input_path: orig_path.to_string(), output_path: out.to_string_lossy().to_string(),
        original_size: total_bytes, compressed_size: final_size,
        ratio: total_bytes as f64 / final_size.max(1) as f64, encrypted: has_pw,
    })
}

fn compress_single_with_progress(
    app: &AppHandle, input: &PathBuf, out: &PathBuf,
    has_pw: bool, password: &str, orig_path: &str,
) -> Result<CompressResult, String> {
    emit(app, 2.0, "Reading...");
    let data = std::fs::read(input).map_err(|e| format!("Cannot read: {}", e))?;
    let orig = data.len() as u64;

    // start a timer thread for smooth progress
    let done = Arc::new(AtomicBool::new(false));
    let done2 = done.clone();
    let app2 = app.clone();
    let size = data.len();

    // estimate speed: ~2MB/s at level 6, ~20MB/s at level 1
    let lvl = hypercompress::compress::get_level();
    let speed = match lvl { 1..=2 => 20.0, 3..=4 => 8.0, 5..=6 => 2.0, _ => 1.0 };
    let est_secs = size as f64 / (speed * 1_000_000.0);

    std::thread::spawn(move || {
        let start = std::time::Instant::now();
        loop {
            if done2.load(Ordering::Relaxed) { break; }
            std::thread::sleep(std::time::Duration::from_millis(200));

            let elapsed = start.elapsed().as_secs_f64();
            // smooth curve: fast start, slow end (never reaches 95% until done)
            let raw_pct = if est_secs > 0.0 { elapsed / est_secs } else { 0.9 };
            let pct = 5.0 + 85.0 * (1.0 - (-3.0 * raw_pct).exp()); // exponential approach to 90%
            let pct = pct.min(90.0);

            let remaining = (est_secs - elapsed).max(0.0);
            let msg = if remaining > 1.0 { format!("Compressing... ~{}s", remaining.ceil() as u32) }
                else { "Compressing...".to_string() };

            let _ = app2.emit("progress", Progress { pct, msg });
        }
    });

    let mut compressed = Vec::new();
    hypercompress::compress::compress(&data, &mut compressed)
        .map_err(|e| format!("Compression failed: {}", e))?;

    done.store(true, Ordering::Relaxed);

    emit(app, 92.0, "Writing...");

    let final_data = if has_pw {
        emit(app, 95.0, "Encrypting...");
        hypercompress::crypto::encrypt(&compressed, password)
    } else {
        compressed
    };

    std::fs::write(out, &final_data).map_err(|e| format!("Cannot write: {}", e))?;
    emit(app, 100.0, "Done!");

    Ok(CompressResult {
        input_path: orig_path.to_string(), output_path: out.to_string_lossy().to_string(),
        original_size: orig, compressed_size: final_data.len() as u64,
        ratio: orig as f64 / final_data.len().max(1) as f64, encrypted: has_pw,
    })
}

#[tauri::command]
async fn decompress_file(app: AppHandle, path: String, output: String, password: String) -> Result<DecompressResult, String> {
    let a = app.clone();
    bg(move || {
        let input = PathBuf::from(&path);
        let out = PathBuf::from(&output);
        let has_pw = !password.is_empty();

        emit(&a, 5.0, "Reading...");
        let raw = std::fs::read(&input).map_err(|e| format!("Cannot read: {}", e))?;

        let data = if has_pw {
            emit(&a, 15.0, "Decrypting...");
            hypercompress::crypto::decrypt(&raw, &password)
                .map_err(|_| "Wrong password".to_string())?
        } else { raw };

        emit(&a, 25.0, "Decompressing...");

        if data.len() >= 4 && &data[..4] == b"HCAR" {
            let tmp = std::env::temp_dir().join("hc_tmp.hc");
            std::fs::write(&tmp, &data).map_err(|e| e.to_string())?;
            std::fs::create_dir_all(&out).map_err(|e| format!("Cannot create folder: {}", e))?;

            emit(&a, 40.0, "Extracting files...");
            let entries = hypercompress::archive::decompress_folder(&tmp, &out)
                .map_err(|e| format!("Extraction failed: {}", e))?;

            let _ = std::fs::remove_file(&tmp);
            let total: u64 = entries.iter().map(|e| e.size).sum();
            emit(&a, 100.0, "Done!");

            Ok(DecompressResult { output_path: out.to_string_lossy().to_string(), size: total, file_count: entries.len() })
        } else {
            let mut cursor = Cursor::new(&data);
            let dec = hypercompress::decompress::decompress(&mut cursor).map_err(|e| {
                if !has_pw { "File may be encrypted — try providing a password.".into() }
                else { format!("Decompression failed: {}", e) }
            })?;

            emit(&a, 85.0, "Writing...");
            std::fs::write(&out, &dec).map_err(|e| format!("Cannot write: {}", e))?;
            emit(&a, 100.0, "Done!");

            Ok(DecompressResult { output_path: out.to_string_lossy().to_string(), size: dec.len() as u64, file_count: 1 })
        }
    })
}

#[tauri::command]
async fn analyze_file(path: String) -> Result<AnalyzeResult, String> {
    bg(move || {
        let data = std::fs::read(&path).map_err(|e| e.to_string())?;
        let fp = hypercompress::fingerprint::Fingerprint::compute(&data);
        Ok(AnalyzeResult {
            entropy: fp.entropy, ascii_ratio: fp.ascii_ratio, zero_ratio: fp.zero_ratio,
            unique_bytes: fp.unique_bytes, detected_type: format!("{:?}", fp.classify()),
            size: data.len() as u64,
        })
    })
}

#[tauri::command] fn pick_file() -> Option<String> { rfd::FileDialog::new().set_title("Select file").pick_file().map(|p| p.to_string_lossy().to_string()) }
#[tauri::command] fn pick_hc_file() -> Option<String> { rfd::FileDialog::new().set_title("Select .hc archive").add_filter("HyperCompress", &["hc"]).pick_file().map(|p| p.to_string_lossy().to_string()) }
#[tauri::command] fn pick_folder() -> Option<String> { rfd::FileDialog::new().set_title("Select folder").pick_folder().map(|p| p.to_string_lossy().to_string()) }
#[tauri::command] fn pick_save_file(default_name: String) -> Option<String> { rfd::FileDialog::new().set_title("Save as").set_file_name(&default_name).add_filter("HyperCompress", &["hc"]).save_file().map(|p| p.to_string_lossy().to_string()) }
#[tauri::command] fn pick_extract_folder() -> Option<String> { rfd::FileDialog::new().set_title("Extract to").pick_folder().map(|p| p.to_string_lossy().to_string()) }

#[derive(Serialize)]
struct StartupArgs { action: String, path: String }

#[tauri::command]
fn get_startup_args() -> StartupArgs {
    let a: Vec<String> = std::env::args().collect();
    if a.len() >= 3 { StartupArgs { action: a[1].clone(), path: a[2].clone() } }
    else { StartupArgs { action: String::new(), path: String::new() } }
}

fn collect_dir(base: &std::path::Path, dir: &std::path::Path, out: &mut Vec<(String, Vec<u8>)>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() { collect_dir(base, &p, out); }
            else if p.is_file() {
                let rel = p.strip_prefix(base).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                if let Ok(data) = std::fs::read(&p) { out.push((rel, data)); }
            }
        }
    }
}

fn bg<F, T>(f: F) -> T where F: FnOnce() -> T + Send + 'static, T: Send + 'static {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || { tx.send(f()).ok(); });
    rx.recv().unwrap()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            compress_file, decompress_file, analyze_file,
            get_startup_args, pick_file, pick_hc_file, pick_folder,
            pick_save_file, pick_extract_folder,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start");
}
