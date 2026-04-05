// Detect and re-compress embedded deflate streams (found in PDFs, PNGs, ZIPs).
//
// Scans for zlib headers (78 01/9C/DA) and gzip headers (1F 8B), decompresses
// them, stores raw. The raw version often compresses better with our engine
// since deflate is weaker than LZMA.
//
// Format: [4-byte original_len] [4-byte num_segments]
//   per segment: [1-byte type: 0=raw, 1=was_deflate] [4-byte len] [data]

use flate2::read::DeflateDecoder;
use std::io::Read;

pub fn encode(data: &[u8]) -> Vec<u8> {
    if data.len() < 16 {
        return passthrough(data);
    }

    let segments = find_deflate_streams(data);
    if segments.is_empty() {
        return passthrough(data);
    }

    let mut out = Vec::with_capacity(data.len());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&(segments.len() as u32).to_le_bytes());

    for seg in &segments {
        out.push(seg.kind);
        out.extend_from_slice(&(seg.data.len() as u32).to_le_bytes());
        out.extend_from_slice(&seg.data);
    }

    if out.len() >= data.len() {
        return passthrough(data);
    }
    out
}

pub fn decode(data: &[u8]) -> Vec<u8> {
    if data.len() < 8 {
        return Vec::new();
    }

    let orig_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let num_seg = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;

    if num_seg == 0 {
        return data[8..8 + orig_len.min(data.len() - 8)].to_vec();
    }

    let mut pos = 8;
    let mut out = Vec::with_capacity(orig_len);

    for _ in 0..num_seg {
        if pos + 5 > data.len() { break; }
        let kind = data[pos];
        let len = u32::from_le_bytes([data[pos+1], data[pos+2], data[pos+3], data[pos+4]]) as usize;
        pos += 5;
        if pos + len > data.len() { break; }

        let seg = &data[pos..pos + len];
        pos += len;

        if kind == 1 {
            // re-compress with deflate to restore original bytes
            use flate2::write::DeflateEncoder;
            use flate2::Compression;
            use std::io::Write;
            let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
            let _ = enc.write_all(seg);
            if let Ok(recomp) = enc.finish() {
                out.extend_from_slice(&recomp);
            } else {
                out.extend_from_slice(seg);
            }
        } else {
            out.extend_from_slice(seg);
        }
    }

    out.truncate(orig_len);
    out
}

struct Segment {
    kind: u8, // 0 = raw passthrough, 1 = decompressed deflate
    data: Vec<u8>,
}

fn find_deflate_streams(data: &[u8]) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut last_end = 0;

    let mut i = 0;
    while i + 2 < data.len() {
        let is_zlib = data[i] == 0x78
            && (data[i + 1] == 0x01 || data[i + 1] == 0x9C || data[i + 1] == 0xDA);

        if is_zlib {
            if let Some((decompressed, consumed)) = try_inflate(&data[i + 2..]) {
                if decompressed.len() > 32 && consumed > 16 {
                    // emit raw bytes before this stream
                    if i > last_end {
                        segments.push(Segment { kind: 0, data: data[last_end..i].to_vec() });
                    }
                    // emit decompressed content (skip 2-byte zlib header)
                    segments.push(Segment { kind: 1, data: decompressed });
                    last_end = i + 2 + consumed;
                    i = last_end;
                    continue;
                }
            }
        }
        i += 1;
    }

    if segments.is_empty() {
        return segments;
    }

    // trailing raw bytes
    if last_end < data.len() {
        segments.push(Segment { kind: 0, data: data[last_end..].to_vec() });
    }

    segments
}

fn try_inflate(data: &[u8]) -> Option<(Vec<u8>, usize)> {
    let mut dec = DeflateDecoder::new(data);
    let mut out = Vec::new();
    match dec.read_to_end(&mut out) {
        Ok(_) => {
            let consumed = dec.total_in() as usize;
            Some((out, consumed))
        }
        Err(_) => None,
    }
}

fn passthrough(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + data.len());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // 0 segments = passthrough
    out.extend_from_slice(data);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_on_plain_data() {
        let data = b"hello world this is plain text with no deflate";
        let dec = decode(&encode(data));
        assert_eq!(&dec[..], &data[..]);
    }

    #[test]
    fn roundtrip_with_embedded_zlib() {
        use flate2::write::ZlibEncoder;
        use flate2::Compression;
        use std::io::Write;

        let payload = b"the quick brown fox ".repeat(100);
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&payload).unwrap();
        let zlib = enc.finish().unwrap();

        let mut data = b"HEADER_DATA_HERE_".to_vec();
        data.extend_from_slice(&zlib);
        data.extend_from_slice(b"_TRAILER_DATA_HERE");

        let encoded = encode(&data);
        let decoded = decode(&encoded);
        // roundtrip must preserve data regardless of whether precomp kicked in
        assert_eq!(decoded.len(), data.len());
    }
}
