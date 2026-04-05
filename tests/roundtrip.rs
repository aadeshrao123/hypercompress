use std::io::Cursor;

#[test]
fn test_lz_roundtrip_simple() {
    let data = b"hello world hello world hello hello hello world world";
    let encoded = hypercompress::entropy::encode(data, hypercompress::format::CodecType::Lz);
    let decoded = hypercompress::entropy::decode(&encoded, hypercompress::format::CodecType::Lz);
    assert_eq!(&decoded[..], &data[..], "LZ roundtrip failed");
}

#[test]
fn test_bwt_then_lz_roundtrip() {
    let text = b"The quick brown fox jumps over the lazy dog. Repeated text here. The quick brown fox again.";
    let bwt = hypercompress::transform::bwt::forward(text);
    let encoded = hypercompress::entropy::encode(&bwt, hypercompress::format::CodecType::Lz);
    let decoded = hypercompress::entropy::decode(&encoded, hypercompress::format::CodecType::Lz);
    assert_eq!(decoded, bwt, "BWT+LZ roundtrip failed at LZ step");
    let original = hypercompress::transform::bwt::inverse(&decoded);
    assert_eq!(&original[..], &text[..], "BWT+LZ roundtrip failed at BWT inverse step");
}

#[test]
fn test_full_compress_decompress_text() {
    let mut text = Vec::new();
    for i in 0..100 {
        text.extend_from_slice(format!("Line {} of test data with some content.\n", i).as_bytes());
    }

    let mut compressed = Vec::new();
    hypercompress::compress::compress(&text, &mut compressed).unwrap();

    let mut cursor = Cursor::new(&compressed);
    let decompressed = hypercompress::decompress::decompress(&mut cursor).unwrap();
    assert_eq!(decompressed, text, "Full text roundtrip failed");
}

#[test]
fn test_full_compress_decompress_binary() {
    let data: Vec<u8> = (0..10000).map(|i| (i * 7 % 256) as u8).collect();

    let mut compressed = Vec::new();
    hypercompress::compress::compress(&data, &mut compressed).unwrap();

    let mut cursor = Cursor::new(&compressed);
    let decompressed = hypercompress::decompress::decompress(&mut cursor).unwrap();
    assert_eq!(decompressed, data, "Full binary roundtrip failed");
}

#[test]
fn test_full_compress_decompress_sparse() {
    let mut data = vec![0u8; 10000];
    for i in (0..10000).step_by(100) {
        data[i] = 42;
    }

    let mut compressed = Vec::new();
    hypercompress::compress::compress(&data, &mut compressed).unwrap();

    let mut cursor = Cursor::new(&compressed);
    let decompressed = hypercompress::decompress::decompress(&mut cursor).unwrap();
    assert_eq!(decompressed, data, "Full sparse roundtrip failed");
}
