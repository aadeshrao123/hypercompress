use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::fs;
use std::io::{BufWriter, Cursor};
use std::path::PathBuf;
use std::time::Instant;

#[derive(Parser)]
#[command(
    name = "hypercompress",
    about = "Content-aware compression engine",
    version,
    long_about = "Auto-detects data types within files and applies optimal\ncompression strategies per chunk. Supports files and folders.\n\nFile format: .hc"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Compress a file or folder to .hc format
    #[command(alias = "c")]
    Compress {
        input: PathBuf,
        /// Output .hc file (default: input_name.hc)
        output: Option<PathBuf>,
    },
    /// Decompress a .hc file (auto-extracts folders)
    #[command(alias = "d")]
    Decompress {
        input: PathBuf,
        /// Output file or folder (default: strip .hc extension)
        output: Option<PathBuf>,
    },
    /// Analyze a file's data characteristics
    #[command(alias = "a")]
    Analyze {
        input: PathBuf,
    },
    /// Show info about a .hc compressed file
    Info {
        input: PathBuf,
    },
    /// List contents of a .hc archive
    #[command(alias = "l")]
    List {
        input: PathBuf,
    },
    /// Run compression benchmark (built-in test data or your own folder)
    #[command(alias = "b")]
    Bench {
        /// Folder to benchmark (omit for built-in test data)
        input: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Compress { input, output } => cmd_compress(input, output),
        Commands::Decompress { input, output } => cmd_decompress(input, output),
        Commands::Analyze { input } => cmd_analyze(input),
        Commands::Info { input } => cmd_info(input),
        Commands::List { input } => cmd_list(input),
        Commands::Bench { input } => cmd_bench(input),
    }
}

fn default_output(input: &PathBuf) -> PathBuf {
    let name = input.file_name().unwrap_or_default().to_string_lossy().to_string();
    let clean = name.trim_end_matches('/').trim_end_matches('\\');
    input.parent().unwrap_or(input).join(format!("{}.hc", clean))
}

fn fmt_mb(bytes: u64) -> String {
    format!("{:.2} MB", bytes as f64 / 1_048_576.0)
}

fn cmd_compress(input: PathBuf, output: Option<PathBuf>) -> Result<()> {
    let output = output.unwrap_or_else(|| default_output(&input));
    println!("HyperCompress v{}", env!("CARGO_PKG_VERSION"));

    if input.is_dir() {
        let (file_count, total_size) = hypercompress::archive::archive_stats(&input)
            .with_context(|| format!("failed to read {}", input.display()))?;

        println!("Compressing: {} ({} files, {})", input.display(), file_count, fmt_mb(total_size));
        println!();

        let start = Instant::now();
        let stats = hypercompress::archive::compress_folder(&input, &output)
            .with_context(|| "folder compression failed")?;
        let elapsed = start.elapsed();

        println!("Output:      {}", output.display());
        println!("Compressed:  {} bytes ({})", stats.archive_size, fmt_mb(stats.archive_size));
        println!("Ratio:       {:.2}:1 ({:.1}%)", stats.total_original as f64 / stats.archive_size as f64, stats.archive_size as f64 / stats.total_original as f64 * 100.0);
        println!("Speed:       {:.1} MB/s", stats.total_original as f64 / elapsed.as_secs_f64() / 1_048_576.0);
        println!("Time:        {:.3}s", elapsed.as_secs_f64());
    } else {
        let data = fs::read(&input).with_context(|| format!("failed to read {}", input.display()))?;
        println!("Compressing: {}", input.display());
        println!("Input size:  {} bytes ({})", data.len(), fmt_mb(data.len() as u64));
        println!();

        let start = Instant::now();
        let file = fs::File::create(&output).with_context(|| format!("failed to create {}", output.display()))?;
        let mut writer = BufWriter::new(file);
        hypercompress::compress::compress(&data, &mut writer).context("compression failed")?;
        drop(writer);

        let out_size = fs::metadata(&output)?.len();
        let elapsed = start.elapsed();

        println!("Output:      {}", output.display());
        println!("Compressed:  {} bytes ({})", out_size, fmt_mb(out_size));
        println!("Ratio:       {:.2}:1 ({:.1}%)", data.len() as f64 / out_size as f64, out_size as f64 / data.len() as f64 * 100.0);
        println!("Speed:       {:.1} MB/s", data.len() as f64 / elapsed.as_secs_f64() / 1_048_576.0);
        println!("Time:        {:.3}s", elapsed.as_secs_f64());
    }

    Ok(())
}

fn cmd_decompress(input: PathBuf, output: Option<PathBuf>) -> Result<()> {
    println!("HyperCompress v{}", env!("CARGO_PKG_VERSION"));
    println!("Decompressing: {}", input.display());
    println!();

    let start = Instant::now();

    if hypercompress::archive::is_archive_file(&input) {
        let output_dir = output.unwrap_or_else(|| {
            let name = input.file_name().unwrap_or_default().to_string_lossy().to_string();
            let clean = name.strip_suffix(".hc").unwrap_or(&name);
            input.parent().unwrap_or(&input).join(clean)
        });

        let entries = hypercompress::archive::decompress_folder(&input, &output_dir)
            .map_err(|e| anyhow::anyhow!("extraction failed: {}", e))?;

        let elapsed = start.elapsed();
        let total: u64 = entries.iter().map(|e| e.size).sum();
        println!("Extracted {} files to {}", entries.len(), output_dir.display());
        println!("Total:   {}", fmt_mb(total));
        println!("Time:    {:.3}s", elapsed.as_secs_f64());
    } else {
        let compressed = fs::read(&input).with_context(|| format!("failed to read {}", input.display()))?;
        let mut cursor = Cursor::new(&compressed);
        let data = hypercompress::decompress::decompress(&mut cursor)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let elapsed = start.elapsed();

        let output = output.unwrap_or_else(|| {
            let name = input.file_name().unwrap_or_default().to_string_lossy().to_string();
            if let Some(stripped) = name.strip_suffix(".hc") {
                input.parent().unwrap_or(&input).join(stripped)
            } else {
                input.parent().unwrap_or(&input).join(format!("{}.out", name))
            }
        });

        fs::write(&output, &data).with_context(|| format!("failed to write {}", output.display()))?;

        println!("Output:  {} ({} bytes)", output.display(), data.len());
        println!("Speed:   {:.1} MB/s", data.len() as f64 / elapsed.as_secs_f64() / 1_048_576.0);
        println!("Time:    {:.3}s", elapsed.as_secs_f64());
    }

    Ok(())
}

fn cmd_analyze(input: PathBuf) -> Result<()> {
    let data = fs::read(&input).with_context(|| format!("failed to read {}", input.display()))?;

    println!("File: {}", input.display());
    println!("Size: {} bytes ({})", data.len(), fmt_mb(data.len() as u64));
    println!();

    let fp = hypercompress::fingerprint::Fingerprint::compute(&data);
    println!("  Entropy:      {:.3} bits/byte", fp.entropy);
    println!("  ASCII ratio:  {:.1}%", fp.ascii_ratio * 100.0);
    println!("  Zero ratio:   {:.1}%", fp.zero_ratio * 100.0);
    println!("  Unique bytes: {}", fp.unique_bytes);
    println!("  Avg delta:    {:.1}", fp.avg_delta);
    println!("  UTF-8 ratio:  {:.1}%", fp.utf8_ratio * 100.0);
    println!("  Detected:     {:?}", fp.classify());

    Ok(())
}

fn cmd_info(input: PathBuf) -> Result<()> {
    let data = fs::read(&input).with_context(|| format!("failed to read {}", input.display()))?;
    let mut cursor = Cursor::new(&data);

    let header = hypercompress::format::FileHeader::read_from(&mut cursor)
        .map_err(|e| anyhow::anyhow!("invalid .hc file: {}", e))?;

    println!("File:            {}", input.display());
    println!("File size:       {} bytes", data.len());
    println!("Version:         {}", header.version);
    println!("Original size:   {} bytes ({})", header.original_size, fmt_mb(header.original_size));
    println!("Chunk count:     {}", header.chunk_count);
    println!("Ratio:           {:.2}:1", header.original_size as f64 / data.len() as f64);
    println!();

    cursor.set_position(header.segment_map_offset);

    println!(
        "{:<6} {:<10} {:<10} {:<10} {:<15} {:<12} {:<6}",
        "Chunk", "Offset", "OrigSize", "CompSize", "Type", "Transform", "Codec"
    );
    println!("{}", "-".repeat(75));

    for i in 0..header.chunk_count {
        if let Ok(meta) = hypercompress::format::ChunkMeta::read_from(&mut cursor) {
            println!(
                "{:<6} {:<10} {:<10} {:<10} {:<15} {:<12} {:<6}",
                i, meta.original_offset, meta.original_size, meta.compressed_size,
                format!("{:?}", meta.data_type), format!("{:?}", meta.transform), format!("{:?}", meta.codec),
            );
        }
    }

    Ok(())
}

fn cmd_list(input: PathBuf) -> Result<()> {
    if !hypercompress::archive::is_archive_file(&input) {
        println!("{} is a single compressed file, not a folder archive.", input.display());
        return Ok(());
    }

    let entries = hypercompress::archive::list_archive(&input)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    println!("Archive: {} ({} files)", input.display(), entries.len());
    println!();
    println!("{:<45} {:>12} {:>12} {:>8}", "File", "Original", "Compressed", "Ratio");
    println!("{}", "-".repeat(80));

    let mut total_orig = 0u64;
    for (path, orig, comp) in &entries {
        total_orig += *orig;
        let ratio = if *comp > 0 { *orig as f64 / *comp as f64 } else { 0.0 };
        println!("{:<45} {:>10} B {:>10} B {:>7.1}x", path, orig, comp, ratio);
    }
    println!("{}", "-".repeat(80));

    let archive_size = fs::metadata(&input)?.len();
    println!(
        "{:<45} {:>10} B {:>10} B {:>7.1}x",
        format!("{} files", entries.len()), total_orig, archive_size,
        total_orig as f64 / archive_size as f64
    );

    Ok(())
}

fn cmd_bench(input: Option<PathBuf>) -> Result<()> {
    let tmp_root = std::env::temp_dir().join("hypercompress-bench");
    let _ = fs::remove_dir_all(&tmp_root);
    fs::create_dir_all(&tmp_root)?;

    println!("HyperCompress Benchmark v{}", env!("CARGO_PKG_VERSION"));
    println!();

    match input {
        None => bench_generated(&tmp_root)?,
        Some(ref p) if p.is_file() => bench_single_file(p, &tmp_root)?,
        Some(ref p) if p.is_dir() => bench_folder(p, &tmp_root)?,
        Some(ref p) => anyhow::bail!("{} is not a file or directory", p.display()),
    }

    let _ = fs::remove_dir_all(&tmp_root);
    Ok(())
}

fn bench_single_file(path: &PathBuf, _tmp: &std::path::Path) -> Result<()> {
    let data = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let orig = data.len();

    let fp = hypercompress::fingerprint::Fingerprint::compute(&data);

    println!("File:     {}", path.display());
    println!("Size:     {} ({})", fmt_sz(orig as u64), fmt_mb(orig as u64));
    println!("Type:     {:?}", fp.classify());
    println!("Entropy:  {:.3} bits/byte", fp.entropy);
    println!();

    let mut compressed = Vec::new();
    hypercompress::compress::compress(&data, &mut compressed)?;
    let mut cursor = Cursor::new(&compressed);
    let restored = hypercompress::decompress::decompress(&mut cursor)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    let ok = restored == data;

    let competitors = compress_all_competitors(&data);

    println!("{:<20} {:>10} {:>8} {:>10}", "Compressor", "Size", "Ratio", "vs best");
    println!("{}", "-".repeat(52));

    let best_other = competitors.iter().map(|(_, s)| *s).min().unwrap_or(orig);
    let all: Vec<(&str, usize)> = std::iter::once(("HyperCompress", compressed.len()))
        .chain(competitors.iter().map(|(n, s)| (*n, *s)))
        .collect();

    let winner_sz = all.iter().map(|(_, s)| *s).min().unwrap_or(orig);

    for (name, sz) in &all {
        let ratio = orig as f64 / *sz as f64;
        let tag = if *sz == winner_sz { " << BEST" } else { "" };
        println!("{:<20} {:>10} {:>7.2}x{}", name, fmt_sz(*sz as u64), ratio, tag);
    }

    println!();
    let hc = compressed.len();
    if hc <= winner_sz {
        println!("HyperCompress WINS");
    } else {
        let gap = (hc as f64 / winner_sz as f64 - 1.0) * 100.0;
        println!("HyperCompress: {:.1}% from best", gap);
    }
    println!("Roundtrip: {}", if ok { "VERIFIED" } else { "FAILED" });

    Ok(())
}

fn bench_folder(folder: &PathBuf, tmp: &std::path::Path) -> Result<()> {
    println!("Folder: {}", folder.display());

    let (file_count, total_size) = hypercompress::archive::archive_stats(folder)?;
    println!("Contents: {} files, {}", file_count, fmt_mb(total_size));
    println!();

    // Compress as archive
    let hc_path = tmp.join("bench_archive.hc");
    let stats = hypercompress::archive::compress_folder(folder, &hc_path)?;

    // Decompress + verify
    let extract_dir = tmp.join("extracted");
    hypercompress::archive::decompress_folder(&hc_path, &extract_dir)?;

    let mut files = Vec::new();
    collect_bench_files(folder, folder, &mut files)?;
    files.sort_by_key(|f| f.0.clone());

    let mut all_ok = true;
    for (rel, _) in &files {
        let a = fs::read(folder.join(rel))?;
        let b_path = extract_dir.join(rel);
        if !b_path.exists() || fs::read(&b_path)? != a {
            all_ok = false;
        }
    }

    // Per-file comparison table
    println!("{:<24} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "File", "Orig", "HC", "gzip", "bzip2", "xz", "Winner");
    println!("{}", "-".repeat(80));

    let mut t_orig = 0u64;
    let mut t_hc = 0u64;
    let mut t_gz = 0u64;
    let mut t_bz = 0u64;
    let mut t_xz = 0u64;
    let mut wins = [0u32; 4]; // HC, gzip, bzip2, xz

    for (rel, _) in &files {
        let data = fs::read(folder.join(rel))?;
        let orig = data.len();

        let mut hc_buf = Vec::new();
        hypercompress::compress::compress(&data, &mut hc_buf)?;
        let hc = hc_buf.len();

        let comps = compress_all_competitors(&data);
        let gz = comps.iter().find(|(n,_)| *n == "gzip -9").map(|(_,s)| *s).unwrap_or(orig);
        let bz = comps.iter().find(|(n,_)| *n == "bzip2 -9").map(|(_,s)| *s).unwrap_or(orig);
        let xz = comps.iter().find(|(n,_)| *n == "xz/LZMA -6").map(|(_,s)| *s).unwrap_or(orig);

        t_orig += orig as u64;
        t_hc += hc as u64;
        t_gz += gz as u64;
        t_bz += bz as u64;
        t_xz += xz as u64;

        let all = [("HC", hc), ("gzip", gz), ("bzip2", bz), ("xz", xz)];
        let winner = all.iter().min_by_key(|(_, s)| *s).unwrap().0;
        match winner {
            "HC" => wins[0] += 1,
            "gzip" => wins[1] += 1,
            "bzip2" => wins[2] += 1,
            _ => wins[3] += 1,
        }

        let name = if rel.len() > 22 { format!("...{}", &rel[rel.len()-19..]) } else { rel.clone() };
        println!("{:<24} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
            name, fmt_sz(orig as u64), fmt_sz(hc as u64),
            fmt_sz(gz as u64), fmt_sz(bz as u64), fmt_sz(xz as u64), winner);
    }

    println!("{}", "-".repeat(80));
    println!("{:<24} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "TOTAL", fmt_sz(t_orig), fmt_sz(t_hc), fmt_sz(t_gz), fmt_sz(t_bz), fmt_sz(t_xz));
    println!("{:<24} {:>8} {:>7.2}x {:>7.2}x {:>7.2}x {:>7.2}x",
        "RATIO", "",
        t_orig as f64 / t_hc as f64,
        t_orig as f64 / t_gz as f64,
        t_orig as f64 / t_bz as f64,
        t_orig as f64 / t_xz as f64);

    println!();
    println!("FILES WON:  HC={}  gzip={}  bzip2={}  xz={}", wins[0], wins[1], wins[2], wins[3]);
    println!("Roundtrip:  {}/{} verified", if all_ok { files.len() } else { 0 }, files.len());
    println!();

    // Overall verdict
    let totals = [("HyperCompress", t_hc), ("gzip -9", t_gz), ("bzip2 -9", t_bz), ("xz/LZMA", t_xz)];
    let (best_name, best_sz) = totals.iter().min_by_key(|(_, s)| *s).unwrap();
    if *best_name == "HyperCompress" {
        println!("OVERALL: HyperCompress WINS (smallest total)");
    } else {
        let gap = (t_hc as f64 / *best_sz as f64 - 1.0) * 100.0;
        println!("OVERALL: {} wins total by {:.1}%", best_name, gap);
    }

    if all_ok { println!("RESULT: ALL PASSED"); } else { println!("RESULT: FAILURES DETECTED"); }
    Ok(())
}

fn bench_generated(tmp: &std::path::Path) -> Result<()> {
    let gen_dir = tmp.join("generated");
    fs::create_dir_all(&gen_dir)?;
    println!("Generating test data...");
    generate_test_data(&gen_dir)?;
    println!();

    let mut files: Vec<(String, u64)> = Vec::new();
    collect_bench_files(&gen_dir, &gen_dir, &mut files)?;
    files.sort_by_key(|f| f.0.clone());

    let total_orig: u64 = files.iter().map(|f| f.1).sum();
    println!("{} files, {} total", files.len(), fmt_mb(total_orig));
    println!();

    println!("{:<20} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>6}",
        "File", "Orig", "HC", "gzip", "bzip2", "xz", "Winner", "Check");
    println!("{}", "-".repeat(86));

    let mut t_hc = 0u64; let mut t_gz = 0u64; let mut t_bz = 0u64; let mut t_xz = 0u64;
    let mut all_ok = true;
    let mut wins = [0u32; 4];

    for (rel, orig_size) in &files {
        let data = fs::read(gen_dir.join(rel))?;
        let orig = data.len();

        let mut hc_buf = Vec::new();
        hypercompress::compress::compress(&data, &mut hc_buf)?;

        let mut cursor = Cursor::new(&hc_buf);
        let restored = hypercompress::decompress::decompress(&mut cursor)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        let ok = restored == data;
        if !ok { all_ok = false; }

        let hc = hc_buf.len();
        let comps = compress_all_competitors(&data);
        let gz = comps.iter().find(|(n,_)| *n == "gzip -9").map(|(_,s)| *s).unwrap_or(orig);
        let bz = comps.iter().find(|(n,_)| *n == "bzip2 -9").map(|(_,s)| *s).unwrap_or(orig);
        let xz = comps.iter().find(|(n,_)| *n == "xz/LZMA -6").map(|(_,s)| *s).unwrap_or(orig);

        t_hc += hc as u64; t_gz += gz as u64; t_bz += bz as u64; t_xz += xz as u64;

        let all = [("HC", hc), ("gzip", gz), ("bzip2", bz), ("xz", xz)];
        let winner = all.iter().min_by_key(|(_, s)| *s).unwrap().0;
        match winner { "HC" => wins[0] += 1, "gzip" => wins[1] += 1, "bzip2" => wins[2] += 1, _ => wins[3] += 1 }

        println!("{:<20} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>6}",
            rel, fmt_sz(*orig_size), fmt_sz(hc as u64),
            fmt_sz(gz as u64), fmt_sz(bz as u64), fmt_sz(xz as u64),
            winner, if ok { "OK" } else { "FAIL" });
    }

    println!("{}", "-".repeat(86));
    println!("{:<20} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "TOTAL", fmt_sz(total_orig), fmt_sz(t_hc), fmt_sz(t_gz), fmt_sz(t_bz), fmt_sz(t_xz));
    println!("{:<20} {:>8} {:>7.1}x {:>7.1}x {:>7.1}x {:>7.1}x",
        "RATIO", "", total_orig as f64/t_hc as f64,
        total_orig as f64/t_gz as f64, total_orig as f64/t_bz as f64,
        total_orig as f64/t_xz as f64);
    println!();
    println!("FILES WON:  HC={}  gzip={}  bzip2={}  xz={}", wins[0], wins[1], wins[2], wins[3]);
    println!("Roundtrip:  {}", if all_ok { "ALL VERIFIED" } else { "FAILURES" });
    println!();

    let totals = [("HyperCompress", t_hc), ("gzip", t_gz), ("bzip2", t_bz), ("xz/LZMA", t_xz)];
    let (best_name, _) = totals.iter().min_by_key(|(_, s)| *s).unwrap();
    println!("OVERALL WINNER: {}", best_name);

    Ok(())
}

fn fmt_sz(bytes: u64) -> String {
    if bytes >= 1_000_000 { format!("{:.1}M", bytes as f64 / 1_000_000.0) }
    else if bytes >= 1_000 { format!("{:.1}K", bytes as f64 / 1_000.0) }
    else { format!("{}B", bytes) }
}

/// Compress data with gzip-9, bzip2-9, and xz/LZMA-6 using built-in Rust crates.
/// No external tools needed — everything runs in-process.
fn compress_all_competitors(data: &[u8]) -> Vec<(&'static str, usize)> {
    let mut results = Vec::new();

    // gzip -9 (via flate2 crate)
    {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;
        let mut enc = GzEncoder::new(Vec::new(), Compression::best());
        let _ = enc.write_all(data);
        if let Ok(buf) = enc.finish() {
            results.push(("gzip -9", buf.len()));
        }
    }

    // bzip2 -9 (via bzip2 crate)
    {
        use bzip2::write::BzEncoder;
        use bzip2::Compression;
        use std::io::Write;
        let mut enc = BzEncoder::new(Vec::new(), Compression::best());
        let _ = enc.write_all(data);
        if let Ok(buf) = enc.finish() {
            results.push(("bzip2 -9", buf.len()));
        }
    }

    // xz/LZMA -6 (via xz2 crate — the real liblzma)
    {
        use std::io::Write;
        let mut enc = xz2::write::XzEncoder::new(Vec::new(), 6);
        let _ = enc.write_all(data);
        if let Ok(buf) = enc.finish() {
            results.push(("xz/LZMA -6", buf.len()));
        }
    }

    results
}

fn generate_test_data(dir: &std::path::Path) -> Result<()> {
    // English text
    {
        let mut s = String::new();
        for i in 0..1500 {
            s.push_str(&format!("The quick brown fox jumps over the lazy dog. Sentence number {} here.\n", i));
        }
        fs::write(dir.join("english.txt"), s)?;
        println!("  english.txt        (text)");
    }

    // JSON
    {
        let mut s = String::from("[");
        for i in 0..3000 {
            if i > 0 { s.push(','); }
            s.push_str(&format!(
                r#"{{"id":{},"name":"user_{}","email":"u{}@test.com","score":{:.2},"active":{}}}"#,
                i, i, i, i as f64 * 1.5, i % 2 == 0
            ));
        }
        s.push(']');
        fs::write(dir.join("data.json"), s)?;
        println!("  data.json          (structured)");
    }

    // CSV
    {
        let mut s = String::from("date,product,customer,qty,price,region\n");
        for i in 0..8000 {
            s.push_str(&format!("2024-{:02}-{:02},PROD-{:04},cust_{},{},{:.2},{}\n",
                (i % 12) + 1, (i % 28) + 1, i % 500, i % 2000,
                (i % 20) + 1, 9.99 + (i % 100) as f64 * 1.5,
                ["US","EU","APAC","LATAM"][i % 4]));
        }
        fs::write(dir.join("sales.csv"), s)?;
        println!("  sales.csv          (structured)");
    }

    // Server log
    {
        let mut s = String::new();
        for i in 0..4000 {
            let level = ["INFO","DEBUG","WARN","ERROR"][if i % 20 < 4 { i % 20 } else { 0 }];
            s.push_str(&format!(
                "2024-03-15T{:02}:{:02}:{:02}.{:03}Z [{:5}] [w-{:02}] req={:06} ip=10.0.{}.{} status={} dur={}ms\n",
                10 + i % 14, i % 60, i % 60, i % 1000,
                level, i % 16, i, i % 256, (i * 7) % 256,
                [200, 200, 200, 201, 301, 404, 500][i % 7], i % 2000));
        }
        fs::write(dir.join("server.log"), s)?;
        println!("  server.log         (text/log)");
    }

    // Float array (f32)
    {
        let data: Vec<u8> = (0..40000u32)
            .flat_map(|i| (1.0f32 + i as f32 * 0.001).to_le_bytes())
            .collect();
        fs::write(dir.join("floats.bin"), data)?;
        println!("  floats.bin         (numeric/float)");
    }

    // Integer array (u32)
    {
        let data: Vec<u8> = (0..40000u32)
            .flat_map(|i| (i * 3 + 100).to_le_bytes())
            .collect();
        fs::write(dir.join("integers.bin"), data)?;
        println!("  integers.bin       (numeric/int)");
    }

    // Sparse data (mostly zeros)
    {
        let mut data = vec![0u8; 150_000];
        for i in (0..data.len()).step_by(97) {
            data[i] = (i % 200) as u8 + 1;
        }
        fs::write(dir.join("sparse.bin"), data)?;
        println!("  sparse.bin         (sparse)");
    }

    // Binary (pseudo-random but with some structure)
    {
        let mut data = Vec::with_capacity(100_000);
        let mut state = 42u64;
        for _ in 0..100_000 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            data.push((state >> 33) as u8);
        }
        fs::write(dir.join("binary.bin"), data)?;
        println!("  binary.bin         (random/binary)");
    }

    // Python source code
    {
        let mut s = String::new();
        for m in 0..15 {
            s.push_str(&format!("# Module {}\n\nimport os\nfrom typing import List\n\n", m));
            s.push_str(&format!("class Handler{}:\n    def __init__(self, name: str):\n        self.name = name\n        self.items: List[str] = []\n\n", m));
            s.push_str(&format!("    def process(self, data: bytes) -> bytes:\n        result = bytearray()\n        for b in data:\n            result.append(b ^ 0x{:02X})\n        return bytes(result)\n\n", m * 17 % 256));
        }
        fs::write(dir.join("code.py"), s)?;
        println!("  code.py            (code)");
    }

    // XML config
    {
        let mut s = String::from("<?xml version=\"1.0\"?>\n<config>\n");
        for i in 0..150 {
            s.push_str(&format!("  <server id=\"{}\" host=\"10.0.{}.{}\" port=\"{}\">\n",
                i, i / 256, i % 256, 8000 + i));
            s.push_str(&format!("    <pool size=\"{}\" timeout=\"30\"/>\n", 100 + i * 10));
            s.push_str("  </server>\n");
        }
        s.push_str("</config>");
        fs::write(dir.join("config.xml"), s)?;
        println!("  config.xml         (structured/xml)");
    }

    Ok(())
}

fn collect_bench_files(base: &std::path::Path, dir: &std::path::Path, out: &mut Vec<(String, u64)>) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_bench_files(base, &path, out)?;
        } else if path.is_file() {
            let rel = path.strip_prefix(base).unwrap_or(&path)
                .to_string_lossy().replace('\\', "/");
            let size = fs::metadata(&path)?.len();
            out.push((rel, size));
        }
    }
    Ok(())
}
