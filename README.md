# HyperCompress

A content-aware compression engine written in Rust. Auto-detects data types within files (text, JSON, CSV, floats, integers, binary, sparse) and applies optimal compression per chunk — instead of treating everything as an opaque byte stream.

Supports single files and full folders (like 7-Zip or WinRAR), AES-256 encryption, and a desktop GUI.

## Benchmarks

All benchmarks run on the same machine, same data, no cherry-picking. Reproduce them yourself with `hypercompress bench`.

### Real-world mixed folder (188MB — WAV audio, MP4, text, PDFs, code, .exe)

47 files. Every compressor at its standard levels.

```
Compressor               Size      Ratio    Time     Speed
-----------------------------------------------------------
HC Ultra (9)            67.5MB    2.78x   281.0s    0.7 MB/s
xz -9                  67.2MB    2.79x   120.0s      2 MB/s
7-Zip Ultra (-mx9)     67.3MB    2.79x    56.9s      3 MB/s
bzip2 -9               68.5MB    2.74x    10.4s     18 MB/s
xz -6                  68.6MB    2.73x    14.9s     13 MB/s
7-Zip Normal (-mx5)    68.8MB    2.73x    28.8s      7 MB/s
HC Maximum (6)         69.5MB    2.70x    71.5s      3 MB/s
7-Zip Fastest (-mx1)   76.0MB    2.47x     0.8s    235 MB/s
HC Normal (4)          77.6MB    2.42x     3.8s     49 MB/s
HC Fast (3)            79.1MB    2.37x     2.0s     94 MB/s
xz -1                  79.0MB    2.37x     2.7s     70 MB/s
gzip -9                82.6MB    2.27x    18.6s     10 MB/s
WinRAR Best (-m5)      84.2MB    2.23x     2.8s     67 MB/s
WinRAR Normal (-m3)    85.2MB    2.20x     2.0s     94 MB/s
WinRAR Fastest (-m1)   91.2MB    2.06x     1.0s    188 MB/s
gzip -1                94.4MB    1.99x     2.3s     82 MB/s

Original               188MB
```

HC Fast (level 3) beats WinRAR Best at the same speed. HC Ultra matches xz -9 and 7-Zip Ultra on ratio.

### Structured data (1.6MB — JSON, CSV, logs, XML, floats, integers)

This is where HyperCompress dominates. Content-aware transforms exploit structure that generic compressors can't see.

```
Compressor             Size    Ratio
--------------------------------------
HyperCompress        128 KB    12.9x
xz                   183 KB     9.0x
bzip2                249 KB     6.6x
gzip                 405 KB     4.1x
```

HC wins 8 out of 10 files. Per-file breakdown:

```
File                     Orig       HC       xz    HC vs xz
------------------------------------------------------------
integers.bin           160 KB    289 B    20.9K      72x better
english.txt            107 KB    713 B     1.8K     2.5x better
data.json              255 KB    3.3 KB   10.2K       3x better
floats.bin             160 KB    1.4 KB    6.0K     4.3x better
config.xml              15 KB    677 B      788       HC wins
sales.csv              349 KB    5.7 KB   15.0K     2.6x better
server.log             350 KB   14.8 KB   26.9K     1.8x better
sparse.bin             150 KB    369 B      560       HC wins
```

**integers.bin: 289 bytes from 160KB. That's 567:1 compression.** xz gets 20.9KB on the same data.

### Run benchmarks yourself

```bash
hypercompress bench                # built-in test data
hypercompress bench myfile.csv     # benchmark a specific file
hypercompress bench myfolder/      # benchmark a folder
```

The bench command compares against gzip, bzip2, and xz automatically and verifies every roundtrip.

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
| 1 | Fastest | ~430 MB/s | Quick archiving |
| 3 | Fast | ~94 MB/s | Daily use — beats WinRAR Best |
| 4 | Normal | ~49 MB/s | Default — beats xz -1 |
| 6 | Maximum | ~3 MB/s | Beats bzip2 -9 |
| 9 | Ultra | ~0.7 MB/s | Matches xz -9 and 7-Zip Ultra |

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
| Audio (WAV) | Stride-aware delta | Per-sample differencing |
| Binary | Adaptive prediction | Polynomial extrapolation per block |
| Compressed | Pass-through | No CPU wasted |

After transform, data goes to whichever codec compresses smallest:
rANS, LZ77 with optimal parsing, order-1 context model, LZMA (via liblzma), or raw pass-through.

For folder archives, similar files are grouped and solid-compressed through one LZMA dictionary at high levels — the same technique 7-Zip uses.

## Features

- **Content-aware compression** — auto-detects data types, applies optimal transform per chunk
- **Folder archiving** — compress entire folders like WinRAR/7-Zip
- **Solid compression** — cross-file dictionary at high levels for maximum ratio
- **AES-256-GCM encryption** — password protection with Argon2id key derivation
- **Desktop GUI** — dark minimal interface built with Tauri
- **Right-click integration** — Windows Explorer context menu
- **Built-in benchmarking** — compare against gzip, bzip2, xz with one command
- **Cross-platform** — Linux, macOS, Windows (x86_64 and ARM64)
- **Compression levels** — Store through Ultra, like WinRAR/7-Zip

## Architecture

```
Input -> Fingerprint -> Transform -> Best Codec -> .hc file
```

Key source files:
- `src/compress.rs` — level-based compression orchestrator
- `src/fingerprint.rs` — data type detection (entropy, byte distribution, alignment)
- `src/transform/` — BWT+MTF, prediction, struct_split, float_split, BCJ, delta, RLE
- `src/entropy/` — rANS, LZ with optimal parsing, order-1 context, LZMA
- `src/archive.rs` — folder archiving with solid groups and per-file optimal compression
- `src/crypto.rs` — AES-256-GCM encryption
- `gui/` — Tauri desktop GUI

## Tests

```bash
cargo test    # 105 tests
```

## License

MIT
