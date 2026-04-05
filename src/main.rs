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
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Compress { input, output } => cmd_compress(input, output),
        Commands::Decompress { input, output } => cmd_decompress(input, output),
        Commands::Analyze { input } => cmd_analyze(input),
        Commands::Info { input } => cmd_info(input),
        Commands::List { input } => cmd_list(input),
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
