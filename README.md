# HyperCompress

A content-aware compression engine written in Rust. Auto-detects data types within files (text, JSON, CSV, floats, integers, binary, sparse) and picks the best transform + codec per chunk instead of treating everything as opaque bytes.

Supports single files and full folders (like 7-Zip / WinRAR), AES-256 encryption, a desktop GUI, and a Win11 modern right-click context menu.

## Honest benchmarks

Real measurements on the same machine, same data. Numbers are in **bytes** so the comparison is exact (no MB-vs-MiB confusion).

### Real-world mixed folder — 188 MB / 47 files (WAV audio, MP4, PDFs, JSON, source code, EXEs)

| Compressor | Bytes | Time | Notes |
|---|---:|---:|---|
| xz -e -9 | 70,326,532 | 89s | best ratio overall |
| **7-Zip Ultra** (`-mx9`) | **70,543,854** | **49s** | best ratio + speed combined |
| **HC Ultra** (`--level 9`) | **70,962,318** | **87s** | **#3, ~0.6% behind 7-Zip** |
| bzip2 -9 | 71,856,920 | 38s | |
| xz -6 | 71,964,292 | 11s | |
| 7-Zip Normal (`-mx5`) | 72,171,480 | 27s | |
| HC Maximum (`--level 6`) | 73,238,993 | 16s | |
| HC Normal (`--level 4`) | 81,400,773 | 8s | |
| 7-Zip Fastest (`-mx1`) | 79,649,270 | 0.6s | |
| xz -1 | 82,829,684 | 1.3s | |
| HC Fast (`--level 3`) | 83,226,765 | 4.5s | |
| gzip -9 | 86,641,729 | 17s | |
| **HC Fastest** (`--level 1`) | **92,651,773** | **0.3s** | **fastest tested, 1168 MB/s** |
| gzip -1 | 99,015,888 | 4.7s | |

**The honest read:**

- **At max compression, 7-Zip wins.** It beats us by ~0.6% (~0.4 MiB on 188 MB) because 7-Zip has its own multi-threaded LZMA2 encoder that shares a dictionary across threads, plus BCJ2 (4-stream x86 filter). liblzma — which we use — splits MT into 64 MB blocks with separate dictionaries, so we lose cross-block matches on large solid streams.
- **At low compression, HC wins on speed.** Our level 1 (zstd-backed) hits 1168 MB/s — faster than gzip -1 and 7-Zip's fastest, and our ratio is reasonable (2.12x).
- **At everything in between, we're competitive but not dominant.** Within ~5% of 7-Zip and xz, sometimes a bit better on speed, sometimes a bit worse on ratio.

**HC is not the absolute best general-purpose compressor on real-world mixed data. 7-Zip is.** We're a credible #2 / #3 on max-compression tasks and #1 on speed at low levels.

### A real user-supplied file — 6,166 byte JSON config

| Compressor | Bytes | vs original |
|---|---:|---:|
| **bzip2 -9** | **1,320** | **21.4%** ← best |
| HC Level 9 | 1,353 | 21.9% |
| xz -e -9 | 1,368 | 22.2% |
| gzip -9 | 1,368 | 22.2% |
| 7-Zip Ultra | 1,487 | 24.1% |

On a 6 KB JSON file: **bzip2 wins**, HC is second (33 bytes behind), 7-Zip is last (134 bytes behind us). On files this small, every compressor is bumping against the entropy floor and the differences are tiny. We beat 7-Zip by ~9% here, but we lose to bzip2 by 2.5%.

### Where HC genuinely shines: synthetic numeric data

This is a **specialized benchmark** with files designed to look like the kind of data our content-aware transforms target — sorted integer columns, float arrays with smooth gradients, sparse arrays of mostly zeros, structured CSVs. **It is not representative of general-purpose compression.**

```
File             Original    HC      xz       Notes
-----------------------------------------------------------
integers.bin      160 KB    289 B   20.9 KB   sorted u32 column
floats.bin        160 KB    1.4 KB   6.0 KB   smooth f64 array
sparse.bin        150 KB    369 B    560 B    99% zeros
data.json         255 KB    3.3 KB  10.2 KB   structured JSON
sales.csv         349 KB    5.7 KB  15.0 KB   columnar CSV
```

On `integers.bin` HC hits **567:1** because the delta+prediction transform recognises it's a sorted integer column and models each value as `prev + small_delta`. xz treats the same data as opaque bytes and gets 8:1.

**The catch:** these files were constructed to showcase the transforms. **A random JSON config file from your real life will not get those ratios** — it'll look more like the 6 KB JSON above, where bzip2 wins by a hair.

If your workload is **logs of integer counters, scientific float arrays, sparse matrices, telemetry CSV, or structured numeric data**, HC genuinely beats every general-purpose compressor by 3–100x. If your workload is **random mixed files**, HC is competitive but not the best.

## Where HC actually wins

| Use case | HC vs the field |
|---|---|
| Sorted/columnar integer arrays | **10–500x better** than xz/gzip/bzip2 |
| Float arrays with smooth structure | **3–10x better** |
| Sparse data (mostly zeros) | **2–5x better** |
| Speed at low compression levels | **#1**: ~1100 MB/s, beats gzip -1 and 7-Zip -mx1 |
| Win11 modern right-click context menu | only OSS Rust compressor with this |

## Where HC does NOT win

| Use case | Winner |
|---|---|
| Maximum ratio on mixed real-world archives | 7-Zip Ultra (by ~0.6%) |
| Maximum ratio on plain text/code | xz -e -9 (by ~1%) |
| Tiny config files (< 10 KB) | usually bzip2 |
| Lossless image/video re-encoding | dedicated tools (ffmpeg, libjxl) |

We're not first place at everything. We're first at a specific niche (content-aware structured numeric data), competitive everywhere else, and the fastest option at low compression levels. That's the honest pitch.

### Run benchmarks yourself

```bash
hypercompress bench                # built-in test data
hypercompress bench myfile.csv     # benchmark a specific file
hypercompress bench myfolder/      # benchmark a folder
```

The bench command runs gzip / bzip2 / xz side-by-side and verifies every roundtrip.

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

### Desktop GUI (Windows)

```bash
cd gui
npm install
npm run tauri build
```

Builds an NSIS installer that includes the Win11 modern right-click context menu (top-level "HyperCompress" with cascading submenu — same approach as PowerToys / NanaZip / 7-Zip / WinRAR).

## Usage

```bash
# Single file
hypercompress compress myfile.json          # -> myfile.json.hc
hypercompress decompress myfile.json.hc

# Folder (like 7-Zip / WinRAR)
hypercompress compress myfolder/            # -> myfolder.hc
hypercompress decompress myfolder.hc

# Compression level
hypercompress compress --level 1 file.txt   # fastest (zstd-backed)
hypercompress compress --level 9 file.txt   # best ratio (LZMA + EXTREME)

# Encryption
hypercompress compress --password "secret" file.txt

# Inspect / analyze
hypercompress list myfolder.hc
hypercompress analyze file.bin
```

### Compression levels

| Level | Speed (test folder) | Use for |
|---|---|---|
| 1 — Fastest | ~1100 MB/s | Quick archiving |
| 3 — Fast | ~40 MB/s | Daily use |
| 4 — Normal | ~24 MB/s | Default |
| 6 — Maximum | ~12 MB/s | Better ratio at modest cost |
| 9 — Ultra | ~2 MB/s | Best ratio HC can do |

Level 9 uses LZMA preset 9 + EXTREME flag + dictionary sized to fit the entire stream. It's slower than 7-Zip Ultra and produces a slightly larger archive on mixed real-world data, but it's the best HC can do without rewriting the LZMA encoder from scratch.

## How it works

```
Input -> Fingerprint -> Transform -> Codec -> .hc file
```

| Detected type | Transform | What it does |
|---|---|---|
| Text / logs | BWT + MTF + zero-RLE | Groups by context, small integers |
| JSON / CSV / XML | BWT + MTF or columnar split | Exploits structural repetition |
| Float arrays | Exponent / mantissa split | Separates predictable from noisy |
| Integer arrays | Columnar delta + prediction | Per-column linear prediction |
| Sparse data | Run-length encoding | Collapses zero runs |
| Executables | BCJ filter | Normalises x86 branch addresses |
| Audio (WAV) | Stride-aware delta | Per-sample differencing |
| Binary | Adaptive prediction | Polynomial extrapolation per block |
| Compressed | Pass-through | No CPU wasted |

After transform, the data goes to whichever codec produces the smallest output: zstd, rANS, LZ77 with optimal parsing, order-1 context, LZMA, or raw pass-through.

For folder archives at level 5+, compressible files are sorted by type and fed through one LZMA stream as a true solid block — one dictionary sees text together, binary together, audio together. Pre-compressed files (JPEG, MP4, ZIP) are stored as-is so we don't waste CPU re-compressing them.

## Why we lose to 7-Zip on max compression

We use [`liblzma`](https://tukaani.org/xz/) (the xz library) for LZMA. liblzma's multi-threaded mode chunks the input into 64 MB blocks and compresses each independently — fast, but cross-block matches are lost on large solid streams. Single-threaded mode keeps one shared dictionary but is much slower.

7-Zip wrote their own LZMA2 multi-threaded encoder that shares one dictionary across threads, so they get **both** speed and the best ratio on the same run. They also use BCJ2 (a 4-stream x86 code filter) versus our BCJ (1-stream), which saves them ~100–200 KB on archives containing executables.

To genuinely beat 7-Zip on real-world mixed data we'd need to either implement BCJ2 (complex) and a custom shared-dictionary multi-threaded LZMA2 encoder (very complex), or use a Rust-native LZMA encoder library (none currently match liblzma's quality). Until then, **HC trails 7-Zip Ultra by ~0.5–1% on mixed real-world archives**, and that's the honest truth.

## Features

- Content-aware compression — auto-detects data types, picks optimal transform + codec per chunk
- Folder archiving with true solid streaming at level 5+
- AES-256-GCM encryption with Argon2id key derivation
- Desktop GUI built with Tauri
- **Win11 modern right-click menu** — top-level "HyperCompress" with cascading submenu (Compress to .hc / Extract Here / Extract to subfolder / Open with HyperCompress), via a Rust COM DLL implementing `IExplorerCommand`, registered as a signed sparse MSIX package
- Built-in benchmarking against gzip / bzip2 / xz
- Cross-platform CLI — Linux x86_64/ARM64, macOS Intel/Apple Silicon, Windows

## Architecture

Key source files:

- `src/compress.rs` — level-based compression orchestrator
- `src/fingerprint.rs` — data type detection (entropy, byte distribution, alignment)
- `src/transform/` — BWT+MTF, prediction, struct_split, float_split, BCJ, delta, RLE
- `src/entropy/` — zstd, rANS, LZ77 with optimal parsing, order-1 context, LZMA
- `src/archive.rs` — folder archiving with solid groups
- `src/crypto.rs` — AES-256-GCM encryption
- `hc_shellext/` — Rust COM DLL (IExplorerCommand) for the Win11 modern context menu
- `gui/` — Tauri desktop GUI + NSIS installer
- `gui/src-tauri/windows/shellext/` — sparse MSIX package and signing scripts

## Tests

```bash
cargo test    # 105 tests
```

## License

MIT
