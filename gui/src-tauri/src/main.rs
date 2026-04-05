// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // If called with CLI args (from context menu), run the operation directly
    if args.len() >= 3 {
        match args[1].as_str() {
            "compress" => {
                let path = &args[2];
                let input = std::path::PathBuf::from(path);
                let output_name = format!("{}.hc", input.file_name().unwrap_or_default().to_string_lossy());
                let output = input.parent().unwrap_or(&input).join(&output_name);

                hypercompress::compress::set_level(6);

                if input.is_dir() {
                    match hypercompress::archive::compress_folder(&input, &output) {
                        Ok(stats) => {
                            let msg = format!(
                                "Compressed folder to {}\nRatio: {:.1}:1",
                                output.display(),
                                stats.total_original as f64 / stats.archive_size as f64
                            );
                            msgbox("HyperCompress", &msg);
                        }
                        Err(e) => msgbox("HyperCompress Error", &e.to_string()),
                    }
                } else {
                    match std::fs::read(&input) {
                        Ok(data) => {
                            let mut compressed = Vec::new();
                            if hypercompress::compress::compress(&data, &mut compressed).is_ok() {
                                if std::fs::write(&output, &compressed).is_ok() {
                                    let msg = format!(
                                        "Compressed to {}\n{} → {} ({:.1}:1)",
                                        output.display(),
                                        fmt_sz(data.len()), fmt_sz(compressed.len()),
                                        data.len() as f64 / compressed.len() as f64
                                    );
                                    msgbox("HyperCompress", &msg);
                                }
                            }
                        }
                        Err(e) => msgbox("HyperCompress Error", &e.to_string()),
                    }
                }
                return;
            }
            "decompress" => {
                let path = &args[2];
                let input = std::path::PathBuf::from(path);

                if hypercompress::archive::is_archive_file(&input) {
                    let out_name = input.file_name().unwrap_or_default().to_string_lossy()
                        .strip_suffix(".hc").unwrap_or("extracted").to_string();
                    let out_dir = input.parent().unwrap_or(&input).join(&out_name);
                    match hypercompress::archive::decompress_folder(&input, &out_dir) {
                        Ok(entries) => {
                            msgbox("HyperCompress", &format!("Extracted {} files to {}", entries.len(), out_dir.display()));
                        }
                        Err(e) => msgbox("HyperCompress Error", &e.to_string()),
                    }
                } else {
                    match std::fs::read(&input) {
                        Ok(raw) => {
                            let mut cursor = std::io::Cursor::new(&raw);
                            match hypercompress::decompress::decompress(&mut cursor) {
                                Ok(data) => {
                                    let out_name = input.file_name().unwrap_or_default().to_string_lossy()
                                        .strip_suffix(".hc").unwrap_or("output").to_string();
                                    let out = input.parent().unwrap_or(&input).join(&out_name);
                                    let _ = std::fs::write(&out, &data);
                                    msgbox("HyperCompress", &format!("Extracted to {}\nSize: {}", out.display(), fmt_sz(data.len())));
                                }
                                Err(e) => msgbox("HyperCompress Error", &e.to_string()),
                            }
                        }
                        Err(e) => msgbox("HyperCompress Error", &e.to_string()),
                    }
                }
                return;
            }
            _ => {}
        }
    }

    // No CLI args — launch the GUI
    gui_lib::run()
}

fn fmt_sz(bytes: usize) -> String {
    if bytes >= 1_000_000 { format!("{:.1} MB", bytes as f64 / 1_000_000.0) }
    else if bytes >= 1_000 { format!("{:.1} KB", bytes as f64 / 1_000.0) }
    else { format!("{} B", bytes) }
}

#[cfg(windows)]
fn msgbox(title: &str, msg: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    let title_w: Vec<u16> = OsStr::new(title).encode_wide().chain(Some(0)).collect();
    let msg_w: Vec<u16> = OsStr::new(msg).encode_wide().chain(Some(0)).collect();

    unsafe {
        MessageBoxW(ptr::null_mut(), msg_w.as_ptr(), title_w.as_ptr(), 0x00000040);
    }
}

#[cfg(windows)]
extern "system" {
    fn MessageBoxW(hwnd: *mut std::ffi::c_void, text: *const u16, caption: *const u16, utype: u32) -> i32;
}

#[cfg(not(windows))]
fn msgbox(_title: &str, msg: &str) {
    println!("{}", msg);
}
