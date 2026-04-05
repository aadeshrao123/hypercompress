use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::fs;
use std::io::{BufWriter, Cursor};
use std::path::PathBuf;
use std::time::Instant;

#[derive(Parser)]
#[command(
    name = "hypercompress",
    about = "HyperCompress — Autonomous Content-Aware Compression Engine",
    version,
    long_about = "World's first compressor that auto-detects data types within files\nand applies optimal compression strategies per chunk.\n\nSupports files AND folders — just like WinRAR or 7-Zip.\n\nFile format: .hc"
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
        /// Input file or folder to compress
        input: PathBuf,
        /// Output .hc file (default: input_name.hc)
        output: Option<PathBuf>,
    },
    /// Decompress a .hc file (auto-extracts folders)
    #[command(alias = "d")]
    Decompress {
        /// Input .hc file
        input: PathBuf,
        /// Output file or folder (default: strip .hc extension)
        output: Option<PathBuf>,
    },
    /// Analyze a file and show what HyperCompress detects
    #[command(alias = "a")]
    Analyze {
        /// File to analyze
        input: PathBuf,
    },
    /// Show info about a .hc compressed file
    Info {
        /// .hc file to inspect
        input: PathBuf,
    },
    /// List contents of a .hc archive
    #[command(alias = "l")]
    List {
        /// .hc archive to list
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

fn cmd_compress(input: PathBuf, output: Option<PathBuf>) -> Result<()> {
    let is_dir = input.is_dir();

    // Default output: input_name.hc (strip trailing slash for folders)
    let output = output.unwrap_or_else(|| {
        let name = input
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let clean_name = name.trim_end_matches('/').trim_end_matches('\\');
        input
            .parent()
            .unwrap_or(&input)
            .join(format!("{}.hc", clean_name))
    });

    println!("HyperCompress v{}", env!("CARGO_PKG_VERSION"));

    let data = if is_dir {
        // FOLDER MODE: archive the folder first, then compress
        let (file_count, total_size) = hypercompress::archive::archive_stats(&input)
            .with_context(|| format!("failed to read {}", input.display()))?;

        println!(
            "Archiving:   {} ({} files, {:.2} MB)",
            input.display(),
            file_count,
            total_size as f64 / 1_048_576.0
        );

        let blob = hypercompress::archive::pack(&input)
            .with_context(|| format!("failed to archive {}", input.display()))?;

        println!(
            "Archive:     {} bytes ({:.2} MB)",
            blob.len(),
            blob.len() as f64 / 1_048_576.0
        );
        blob
    } else {
        // FILE MODE: read the file directly
        let data =
            fs::read(&input).with_context(|| format!("failed to read {}", input.display()))?;
        println!("Compressing: {}", input.display());
        println!(
            "Input size:  {} bytes ({:.2} MB)",
            data.len(),
            data.len() as f64 / 1_048_576.0
        );
        data
    };

    println!();

    let start = Instant::now();

    let file = fs::File::create(&output)
        .with_context(|| format!("failed to create {}", output.display()))?;
    let mut writer = BufWriter::new(file);

    let stats =
        hypercompress::compress::compress(&data, &mut writer).context("compression failed")?;

    drop(writer); // flush
    let output_size = fs::metadata(&output)?.len();
    let elapsed = start.elapsed();
    let speed = data.len() as f64 / elapsed.as_secs_f64() / 1_048_576.0;

    println!("Compression complete!");
    println!("Output:      {}", output.display());
    println!(
        "Compressed:  {} bytes ({:.2} MB)",
        output_size,
        output_size as f64 / 1_048_576.0
    );
    println!(
        "Ratio:       {:.2}:1 ({:.1}% of original)",
        data.len() as f64 / output_size as f64,
        output_size as f64 / data.len() as f64 * 100.0
    );
    println!("Speed:       {:.1} MB/s", speed);
    println!("Time:        {:.3}s", elapsed.as_secs_f64());

    if stats.chunk_count > 1 {
        println!();
        println!("Chunk analysis ({} chunks):", stats.chunk_count);
        if stats.text_chunks > 0 { println!("  Text:          {} chunks", stats.text_chunks); }
        if stats.structured_chunks > 0 { println!("  Structured:    {} chunks", stats.structured_chunks); }
        if stats.binary_chunks > 0 { println!("  Binary:        {} chunks", stats.binary_chunks); }
        if stats.numeric_int_chunks > 0 { println!("  Numeric (int): {} chunks", stats.numeric_int_chunks); }
        if stats.numeric_float_chunks > 0 { println!("  Numeric (flt): {} chunks", stats.numeric_float_chunks); }
        if stats.random_chunks > 0 { println!("  Random/Compr:  {} chunks", stats.random_chunks); }
        if stats.sparse_chunks > 0 { println!("  Sparse:        {} chunks", stats.sparse_chunks); }
    }

    Ok(())
}

fn cmd_decompress(input: PathBuf, output: Option<PathBuf>) -> Result<()> {
    let compressed =
        fs::read(&input).with_context(|| format!("failed to read {}", input.display()))?;

    println!("HyperCompress v{}", env!("CARGO_PKG_VERSION"));
    println!("Decompressing: {}", input.display());
    println!();

    let start = Instant::now();
    let mut cursor = Cursor::new(&compressed);
    let data = hypercompress::decompress::decompress(&mut cursor)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let elapsed = start.elapsed();
    let speed = data.len() as f64 / elapsed.as_secs_f64() / 1_048_576.0;

    // Check if the decompressed data is an archive
    if hypercompress::archive::is_archive(&data) {
        // FOLDER MODE: extract the archive
        let output_dir = output.unwrap_or_else(|| {
            let name = input
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let clean = name.strip_suffix(".hc").unwrap_or(&name);
            input.parent().unwrap_or(&input).join(clean)
        });

        let entries = hypercompress::archive::unpack(&data, &output_dir)
            .map_err(|e| anyhow::anyhow!("archive extraction failed: {}", e))?;

        println!("Extracted {} files to {}", entries.len(), output_dir.display());
        println!("Speed:   {:.1} MB/s", speed);
        println!("Time:    {:.3}s", elapsed.as_secs_f64());
    } else {
        // FILE MODE: write single file
        let output = output.unwrap_or_else(|| {
            let name = input
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if let Some(stripped) = name.strip_suffix(".hc") {
                input.parent().unwrap_or(&input).join(stripped)
            } else {
                input
                    .parent()
                    .unwrap_or(&input)
                    .join(format!("{}.out", name))
            }
        });

        fs::write(&output, &data)
            .with_context(|| format!("failed to write {}", output.display()))?;

        println!("Decompression complete!");
        println!("Output:  {} ({} bytes)", output.display(), data.len());
        println!("Speed:   {:.1} MB/s", speed);
        println!("Time:    {:.3}s", elapsed.as_secs_f64());
    }

    Ok(())
}

fn cmd_analyze(input: PathBuf) -> Result<()> {
    let data = fs::read(&input).with_context(|| format!("failed to read {}", input.display()))?;

    println!("HyperCompress Analyzer");
    println!("File: {}", input.display());
    println!(
        "Size: {} bytes ({:.2} MB)",
        data.len(),
        data.len() as f64 / 1_048_576.0
    );
    println!();

    let fp = hypercompress::fingerprint::Fingerprint::compute(&data);
    println!("Overall fingerprint:");
    println!("  Entropy:      {:.3} bits/byte", fp.entropy);
    println!("  ASCII ratio:  {:.1}%", fp.ascii_ratio * 100.0);
    println!("  Zero ratio:   {:.1}%", fp.zero_ratio * 100.0);
    println!("  Unique bytes: {}", fp.unique_bytes);
    println!("  Avg delta:    {:.1}", fp.avg_delta);
    println!("  UTF-8 ratio:  {:.1}%", fp.utf8_ratio * 100.0);
    println!("  Detected type: {:?}", fp.classify());

    Ok(())
}

fn cmd_info(input: PathBuf) -> Result<()> {
    let data = fs::read(&input).with_context(|| format!("failed to read {}", input.display()))?;
    let mut cursor = Cursor::new(&data);

    let header = hypercompress::format::FileHeader::read_from(&mut cursor)
        .map_err(|e| anyhow::anyhow!("invalid .hc file: {}", e))?;

    println!("HyperCompress File Info");
    println!("File:            {}", input.display());
    println!("File size:       {} bytes", data.len());
    println!("Version:         {}", header.version);
    println!(
        "Original size:   {} bytes ({:.2} MB)",
        header.original_size,
        header.original_size as f64 / 1_048_576.0
    );
    println!("Chunk count:     {}", header.chunk_count);
    println!(
        "Ratio:           {:.2}:1",
        header.original_size as f64 / data.len() as f64
    );
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
                i,
                meta.original_offset,
                meta.original_size,
                meta.compressed_size,
                format!("{:?}", meta.data_type),
                format!("{:?}", meta.transform),
                format!("{:?}", meta.codec),
            );
        }
    }

    Ok(())
}

fn cmd_list(input: PathBuf) -> Result<()> {
    let compressed =
        fs::read(&input).with_context(|| format!("failed to read {}", input.display()))?;

    let mut cursor = Cursor::new(&compressed);
    let data = hypercompress::decompress::decompress(&mut cursor)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    if !hypercompress::archive::is_archive(&data) {
        println!("{} is a single compressed file, not an archive.", input.display());
        return Ok(());
    }

    // Parse archive directory without extracting
    let file_count =
        u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let mut pos = 8;

    println!(
        "Archive: {} ({} files)",
        input.display(),
        file_count
    );
    println!();
    println!("{:<50} {:>12}", "File", "Size");
    println!("{}", "-".repeat(64));

    let mut total = 0u64;
    for _ in 0..file_count {
        if pos + 2 > data.len() {
            break;
        }
        let path_len = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;
        if pos + path_len > data.len() {
            break;
        }
        let path = String::from_utf8_lossy(&data[pos..pos + path_len]);
        pos += path_len;
        if pos + 8 > data.len() {
            break;
        }
        let size = u64::from_le_bytes([
            data[pos], data[pos + 1], data[pos + 2], data[pos + 3],
            data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7],
        ]);
        pos += 8;
        total += size;
        println!("{:<50} {:>12} B", path, size);
    }
    println!("{}", "-".repeat(64));
    println!(
        "{:<50} {:>12} B",
        format!("{} files", file_count),
        total
    );
    println!(
        "Compressed:  {} B  (ratio {:.2}:1)",
        compressed.len(),
        total as f64 / compressed.len() as f64
    );

    Ok(())
}
