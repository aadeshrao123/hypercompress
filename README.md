# HyperCompress

A content-aware compression engine written in Rust. Auto-detects data types within files (text, JSON, CSV, floats, integers, binary, sparse) and applies optimal compression per chunk — instead of treating everything as an opaque byte stream.

Supports single files and full folders (like 7-Zip or WinRAR), AES-256 encryption, and a desktop GUI.

## Benchmarks

### Mixed folder (135MB — text, PDFs, WAV audio, code)

Tested on a real-world folder with 44 files. Every compressor at every level.

```
Compressor                   Size   Ratio    Time     Speed
----------------------------------------------------------
xz -9 (best)               16.3MB    8.6x  100.6s       1 MB/s
bzip2 -9                   17.2MB    8.2x    8.9s      16 MB/s
xz -6 (normal)             17.4MB    8.1x   74.5s       2 MB/s
HC Ultra                   26.0MB    5.4x  184.9s       1 MB/s
xz -1 (fast)               28.2MB    5.0x    7.2s      20 MB/s
HC Maximum                 29.8MB    4.7x   19.6s       7 MB/s
gzip -9 (best)             32.3MB    4.4x   21.9s       6 MB/s
WinRAR Best                34.3MB    4.1x    2.2s      63 MB/s
WinRAR Normal              35.4MB    4.0x    1.4s     100 MB/s
WinRAR Fast                36.3MB    3.9x    1.1s     134 MB/s
HC Normal                  40.1MB    3.5x    1.6s      89 MB/s
WinRAR Fastest             41.6MB    3.4x    0.7s     192 MB/s
gzip -1 (fastest)          44.5MB    3.2x    1.7s      81 MB/s
HC Fast                    51.6MB    2.7x    1.7s      82 MB/s
HC Fastest                 79.9MB    1.8x    0.4s     316 MB/s

Original                  140.9MB
```

### Structured data (5MB — JSON + CSV + logs + XML)

This is where HyperCompress dominates. Content-aware transforms exploit the structure that generic compressors can't see.

```
Compressor               Size   Ratio   Time
---------------------------------------------
HC Ultra                0.03MB    151x   4.0s
HC Normal               0.10MB     51x   0.1s
HC Fast                 0.12MB     40x   0.1s
xz -6                   0.14MB     35x   1.1s
WinRAR Normal           0.29MB     17x   0.1s
WinRAR Best             0.31MB     16x   0.1s
bzip2 -9                0.35MB     14x   0.2s
gzip -9                 0.70MB      7x   0.2s

Original                4.90MB
```

**HC Ultra: 151x ratio.** xz gets 35x. WinRAR gets 17x. gzip gets 7x.

### Built-in benchmark

Run it yourself — compares against gzip, bzip2, and xz/LZMA automatically:

```bash
hypercompress bench              # built-in test data
hypercompress bench myfile.csv   # benchmark a specific file
hypercompress bench myfolder/    # benchmark a folder
```

## Install

```bash
cargo install --path .
```

Or build from source:

```bash
git clone https://github.com/aadeshrao123/hypercompress.git
cd hypercompress
cargo build --release
```

### Desktop GUI

```bash
cd gui
npm install
npm run tauri build
```

Builds a standalone `.exe` with Windows installer (`.msi` / `.exe` setup).
Right-click context menu integration included.

## Usage

### Compress / Decompress

```bash
# Single file
hypercompress compress myfile.json          # -> myfile.json.hc
hypercompress decompress myfile.json.hc     # -> myfile.json

# Folder (like 7-Zip / WinRAR)
hypercompress compress myfolder/            # -> myfolder.hc
hypercompress decompress myfolder.hc        # -> myfolder/

# With compression level
hypercompress compress --level 1 file.txt   # fastest
hypercompress compress --level 9 file.txt   # best ratio

# With encryption (AES-256-GCM)
hypercompress compress --password "secret" file.txt
hypercompress decompress --password "secret" file.txt.hc

# List archive contents
hypercompress list myfolder.hc

# Analyze file
hypercompress analyze file.bin
```

### Compression levels

| Level | Name | Speed | Best for |
|-------|------|-------|---------|
| 0 | Store | Instant | No compression, just pack |
| 1 | Fastest | ~300 MB/s | Quick archiving |
| 3 | Fast | ~80 MB/s | Daily use, good ratio |
| 4 | Normal | ~90 MB/s | Default — balanced speed/ratio |
| 6 | Maximum | ~7 MB/s | Better ratio, slower |
| 9 | Ultra | ~1 MB/s | Maximum compression |

## How it works

Traditional compressors treat all data as a generic byte stream. HyperCompress fingerprints data first, then picks the best strategy:

| Detected Type | Transform | Effect |
|--------------|-----------|--------|
| Text / Logs | BWT + MTF + Zero-RLE | Groups by context, small integers |
| JSON / CSV / XML | BWT + MTF or Columnar Split | Exploits structural repetition |
| Float arrays | Exponent/mantissa split | Separates predictable from noisy |
| Integer arrays | Columnar delta + prediction | Per-column linear prediction |
| Sparse data | Run-length encoding | Collapses zero runs |
| Executables | BCJ filter | Normalizes branch addresses |
| Binary | Adaptive prediction | Polynomial extrapolation per block |
| Compressed | Pass-through | No CPU wasted |

After transform, data goes to whichever codec compresses smallest:
rANS, LZ77 with optimal parsing, order-1 context model, LZMA (via liblzma), or raw pass-through.

## Features

- **Content-aware compression** — auto-detects data types, applies optimal transform per chunk
- **Folder archiving** — compress entire folders like WinRAR/7-Zip
- **AES-256-GCM encryption** — password protection with Argon2id key derivation
- **Desktop GUI** — dark minimal interface built with Tauri
- **Right-click integration** — Windows Explorer context menu
- **Built-in benchmarking** — compare against gzip, bzip2, xz with one command
- **Cross-platform** — Linux, macOS, Windows (x86_64 and ARM64)
- **Compression levels** — Store through Ultra, like WinRAR/7-Zip

## Architecture

```
Input → Fingerprint → Transform → Best Codec → .hc file
```

Key source files:
- `src/compress.rs` — level-based compression orchestrator
- `src/fingerprint.rs` — data type detection (entropy, byte distribution, alignment)
- `src/transform/` — BWT+MTF, prediction, float split, struct split, BCJ, delta, RLE
- `src/entropy/` — rANS, LZ turbo/optimal, order-1 context, LZMA
- `src/archive.rs` — folder archiving with per-file optimal compression
- `src/crypto.rs` — AES-256-GCM encryption
- `gui/` — Tauri desktop GUI

## Tests

```bash
cargo test    # 104 tests
```

## License

MIT
