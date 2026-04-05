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
    long_about = "World's first compressor that auto-detects data types within a file\nand applies optimal compression strategies per chunk.\n\nFile format: .hc"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Compress a file to .hc format
    #[command(alias = "c")]
    Compress {
        /// Input file to compress
        input: PathBuf,
        /// Output .hc file (default: input.hc)
        output: Option<PathBuf>,
    },
    /// Decompress a .hc file
    #[command(alias = "d")]
    Decompress {
        /// Input .hc file
        input: PathBuf,
        /// Output file (default: strip .hc extension)
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
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Compress { input, output } => cmd_compress(input, output),
        Commands::Decompress { input, output } => cmd_decompress(input, output),
        Commands::Analyze { input } => cmd_analyze(input),
        Commands::Info { input } => cmd_info(input),
    }
}

fn cmd_compress(input: PathBuf, output: Option<PathBuf>) -> Result<()> {
    let output = output.unwrap_or_else(|| {
        let mut p = input.clone();
        let name = format!(
            "{}.hc",
            p.file_name().unwrap_or_default().to_string_lossy()
        );
        p.set_file_name(name);
        p
    });

    let data = fs::read(&input).with_context(|| format!("failed to read {}", input.display()))?;

    println!("HyperCompress v{}", env!("CARGO_PKG_VERSION"));
    println!("Compressing: {}", input.display());
    println!(
        "Input size:  {} bytes ({:.2} MB)",
        data.len(),
        data.len() as f64 / 1_048_576.0
    );
    println!();

    let start = Instant::now();

    let file = fs::File::create(&output)
        .with_context(|| format!("failed to create {}", output.display()))?;
    let mut writer = BufWriter::new(file);

    let stats =
        hypercompress::compress::compress(&data, &mut writer).context("compression failed")?;

    let elapsed = start.elapsed();
    let speed = data.len() as f64 / elapsed.as_secs_f64() / 1_048_576.0;

    println!("Compression complete!");
    println!("Output:      {}", output.display());
    println!(
        "Compressed:  {} bytes ({:.2} MB)",
        stats.compressed_size,
        stats.compressed_size as f64 / 1_048_576.0
    );
    println!(
        "Ratio:       {:.2}:1 ({:.1}% of original)",
        stats.ratio(),
        100.0 / stats.ratio()
    );
    println!("Speed:       {:.1} MB/s", speed);
    println!("Time:        {:.3}s", elapsed.as_secs_f64());
    println!();
    println!("Chunk analysis ({} chunks):", stats.chunk_count);
    if stats.text_chunks > 0 {
        println!("  Text:          {} chunks", stats.text_chunks);
    }
    if stats.structured_chunks > 0 {
        println!("  Structured:    {} chunks", stats.structured_chunks);
    }
    if stats.binary_chunks > 0 {
        println!("  Binary:        {} chunks", stats.binary_chunks);
    }
    if stats.numeric_int_chunks > 0 {
        println!("  Numeric (int): {} chunks", stats.numeric_int_chunks);
    }
    if stats.numeric_float_chunks > 0 {
        println!("  Numeric (flt): {} chunks", stats.numeric_float_chunks);
    }
    if stats.random_chunks > 0 {
        println!("  Random/Compr:  {} chunks", stats.random_chunks);
    }
    if stats.sparse_chunks > 0 {
        println!("  Sparse:        {} chunks", stats.sparse_chunks);
    }

    Ok(())
}

fn cmd_decompress(input: PathBuf, output: Option<PathBuf>) -> Result<()> {
    let output = output.unwrap_or_else(|| {
        let mut p = input.clone();
        let name = input
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if let Some(stripped) = name.strip_suffix(".hc") {
            p.set_file_name(stripped);
        } else {
            p.set_file_name(format!("{}.out", name));
        }
        p
    });

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

    fs::write(&output, &data).with_context(|| format!("failed to write {}", output.display()))?;

    println!("Decompression complete!");
    println!("Output:  {} ({} bytes)", output.display(), data.len());
    println!("Speed:   {:.1} MB/s", speed);
    println!("Time:    {:.3}s", elapsed.as_secs_f64());

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
    println!();

    let mut chunks =
        hypercompress::chunk::split_into_chunks(&data, hypercompress::chunk::DEFAULT_CHUNK_SIZE);
    hypercompress::fingerprint::fingerprint_chunks(&mut chunks);

    println!(
        "Per-chunk analysis ({} chunks of ~{}KB):",
        chunks.len(),
        hypercompress::chunk::DEFAULT_CHUNK_SIZE / 1024
    );
    println!(
        "{:<8} {:<10} {:<18} {:<10} {:<10}",
        "Chunk", "Offset", "Type", "Entropy", "ASCII%"
    );
    println!("{}", "-".repeat(60));

    for (i, chunk) in chunks.iter().enumerate() {
        let fp = hypercompress::fingerprint::Fingerprint::compute(&chunk.data);
        println!(
            "{:<8} {:<10} {:<18} {:<10.3} {:<10.1}",
            i,
            chunk.original_offset,
            format!("{:?}", chunk.data_type),
            fp.entropy,
            fp.ascii_ratio * 100.0,
        );
    }

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
