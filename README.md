# HyperCompress

A content-aware compression engine written in Rust. It auto-detects data types within files (text, JSON, CSV, floats, integers, binary, sparse) and applies the optimal compression strategy for each — instead of treating everything as an opaque byte stream like traditional compressors do.

Supports single files and full folders (like 7-Zip or WinRAR).

## Results

Tested on 22 real-world files (51 MB total): PDFs, images, audio, video, source code, JSON, CSV, server logs, executables, game saves, XML.

### Per-file comparison (HC vs gzip vs bzip2 vs xz/LZMA)

```
File                         Orig       HC     gzip    bzip2       xz   Winner
--------------------------------------------------------------------------------
database.json              948.4K     2.7K    26.2K    13.0K    12.4K       HC
server.log                 751.2K    14.9K   116.5K    56.1K    41.5K       HC
sales_data.csv             683.8K     5.8K    73.6K    27.3K    20.7K       HC
audio.wav                   14.8M     9.7M    11.5M     9.9M    10.1M       HC
config.xml                  96.0K     937B     3.3K     1.8K     1.6K       HC
dashboard.html              42.2K     1.3K     4.4K     2.4K     2.1K       HC
animation.gif                8.2M     8.1M     8.2M     8.2M     8.1M       xz
app_small.exe                1.9M     1.9M     1.9M     1.9M     1.9M       xz
trace.utrace                 2.7M     1.0M     1.5M     1.2M     1.0M       xz
video.mp4                   21.7M    21.7M    21.7M    21.8M    21.7M       xz
(+ 12 more files)
--------------------------------------------------------------------------------
TOTAL                       53.2M    43.4M    46.0M    44.0M    43.9M
RATIO                                1.22x    1.16x    1.21x    1.21x
```

**HyperCompress wins on total size** — 43.4 MB vs xz's 43.9 MB on the same 22 files.

Where we dominate:
- **JSON**: 351:1 ratio (xz gets 76:1)
- **CSV**: 118:1 ratio (xz gets 33:1)
- **Server logs**: 50:1 ratio (xz gets 18:1)
- **Structured XML**: 102:1 ratio (xz gets 61:1)

Where we match or slightly trail:
- Already-compressed files (PNG, GIF, MP4, PDF) — nobody can compress these much
- Raw binary data — we use LZMA internally, so results are nearly identical to xz

### Built-in benchmark

Run it yourself — no setup needed:

```
hypercompress bench
```

This generates test data, compresses with HyperCompress and all competitors (gzip, bzip2, xz/LZMA), and shows who wins. Everything runs in-process, no external tools required.

## Install

```bash
cargo install --path .
```

Or build from source:

```bash
git clone https://github.com/aadeshrao123/hypercompress.git
cd hypercompress
cargo build --release
# binary is at ./target/release/hypercompress
```

## Usage

### Compress / Decompress

```bash
# Single file
hypercompress compress myfile.json          # -> myfile.json.hc
hypercompress decompress myfile.json.hc     # -> myfile.json

# Folder (like 7-Zip / WinRAR)
hypercompress compress myfolder/            # -> myfolder.hc
hypercompress decompress myfolder.hc        # -> myfolder/

# List archive contents
hypercompress list myfolder.hc
```

### Benchmark

```bash
# Built-in test data (no files needed)
hypercompress bench

# Benchmark a specific file against gzip/bzip2/xz
hypercompress bench myfile.csv

# Benchmark a folder against all competitors
hypercompress bench /path/to/folder/
```

### Analyze

```bash
# See what HyperCompress detects in a file
hypercompress analyze somefile.bin
```

Output:
```
File: somefile.bin
Size: 200000 bytes (0.19 MB)

  Entropy:      6.234 bits/byte
  ASCII ratio:  12.3%
  Zero ratio:   0.1%
  Unique bytes: 256
  Detected:     NumericFloat
```

Short aliases: `c` (compress), `d` (decompress), `a` (analyze), `l` (list), `b` (bench).

## How it works

Traditional compressors (gzip, xz, bzip2) treat all data as a generic byte stream. HyperCompress fingerprints the data first, then picks the best strategy:

| Detected Type | Transform Applied | Why It Helps |
|--------------|-------------------|-------------|
| Text / Logs | BWT + Move-to-Front + Zero-RLE | Groups by context, produces compressible small integers |
| JSON / CSV / XML | BWT + MTF or Columnar Split | Exploits structural repetition |
| Float arrays | Exponent/mantissa stream split | Separates predictable exponent from noisy mantissa |
| Integer arrays | Structure-aware columnar delta | Per-column linear prediction reduces entropy |
| Sparse data | Run-length encoding | Collapses zero runs efficiently |
| Executables | BCJ filter (branch address normalization) | Makes calls to same function produce identical bytes |
| Binary | Adaptive-order linear prediction | Polynomial extrapolation (orders 0-3) per block |
| Already compressed | Pass-through | Detected and skipped — no wasted CPU |

After the transform, data goes to whichever codec compresses it smallest:
- rANS entropy coder (with sparse frequency tables)
- LZ77 with optimal parsing and repeat-offset tracking
- Order-1 context model
- LZMA (via liblzma) for binary data
- Raw pass-through when nothing helps

The engine tries multiple transform+codec combinations per chunk and keeps whichever produces the smallest output, with roundtrip verification on every combination.

For folder archives, each file is compressed independently with its optimal pipeline, then packed. This beats single-stream approaches (like tar+xz) because each file gets type-specific treatment.

## Architecture

```
Input → Fingerprint → [Transform] → [Best Codec] → .hc file
         detect type    BWT+MTF       rANS
         per chunk      delta         LZ optimal
                        float split   LZMA
                        prediction    order-1
                        BCJ           raw
                        RLE
```

Key source files:
- `src/compress.rs` — orchestrator, tries per-chunk vs whole-file LZMA
- `src/fingerprint.rs` — data type detection
- `src/transform/` — all preprocessing transforms
- `src/entropy/` — all codecs (rANS, LZ optimal, order-1, LZMA)
- `src/archive.rs` — folder archiving with per-file compression

## Tests

```bash
cargo test    # 96 tests
```

## License

MIT
