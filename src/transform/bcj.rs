/// BCJ (Branch/Call/Jump) filter for x86/x64 executables.
///
/// Converts relative addresses in CALL (E8) and JMP (E9) instructions
/// to absolute addresses. This makes calls to the same function produce
/// identical bytes regardless of call site location.
///
/// Before BCJ: CALL +0x1234 at pos 0x1000 → [E8 34 12 00 00]
///              CALL +0x1234 at pos 0x2000 → [E8 34 12 00 00]
///              But CALL to same_func from different sites → different bytes!
///
/// After BCJ:  CALL 0x2234 at pos 0x1000 → [E8 34 22 00 00]  (absolute)
///             CALL 0x2234 at pos 0x2000 → [E8 34 22 00 00]  (same!)
///
/// This makes LZ compression find MANY more matches in executable code.
/// Typical improvement: 6-15% better compression on executables.

/// Apply BCJ filter: convert relative→absolute for E8/E9 instructions.
pub fn encode(data: &[u8]) -> Vec<u8> {
    let mut result = data.to_vec();
    let mut i = 0;

    while i + 5 <= result.len() {
        let opcode = result[i];

        if opcode == 0xE8 || opcode == 0xE9 {
            // Read the 32-bit relative offset (little-endian)
            let rel = i32::from_le_bytes([
                result[i + 1],
                result[i + 2],
                result[i + 3],
                result[i + 4],
            ]);

            // Convert to absolute: abs = rel + position
            let abs = rel.wrapping_add(i as i32);

            // Only convert if the absolute address looks plausible
            // (within the data bounds — avoids corrupting non-code data)
            if abs >= 0 && (abs as usize) < data.len() + 65536 {
                let abs_bytes = abs.to_le_bytes();
                result[i + 1] = abs_bytes[0];
                result[i + 2] = abs_bytes[1];
                result[i + 3] = abs_bytes[2];
                result[i + 4] = abs_bytes[3];
            }

            i += 5;
        } else {
            i += 1;
        }
    }

    result
}

/// Reverse BCJ filter: convert absolute→relative.
pub fn decode(data: &[u8]) -> Vec<u8> {
    let mut result = data.to_vec();
    let mut i = 0;

    while i + 5 <= result.len() {
        let opcode = result[i];

        if opcode == 0xE8 || opcode == 0xE9 {
            let abs = i32::from_le_bytes([
                result[i + 1],
                result[i + 2],
                result[i + 3],
                result[i + 4],
            ]);

            // Convert back: rel = abs - position
            if abs >= 0 && (abs as usize) < data.len() + 65536 {
                let rel = abs.wrapping_sub(i as i32);
                let rel_bytes = rel.to_le_bytes();
                result[i + 1] = rel_bytes[0];
                result[i + 2] = rel_bytes[1];
                result[i + 3] = rel_bytes[2];
                result[i + 4] = rel_bytes[3];
            }

            i += 5;
        } else {
            i += 1;
        }
    }

    result
}

/// Detect if data likely contains x86 machine code.
/// Looks for common opcode patterns that indicate executable code.
pub fn is_likely_executable(data: &[u8]) -> bool {
    if data.len() < 64 {
        return false;
    }

    // Check for PE header (Windows executables)
    if data.len() > 0x40 && data[0] == b'M' && data[1] == b'Z' {
        return true;
    }

    // Check for ELF header (Linux executables)
    if data.len() > 4 && data[0] == 0x7F && data[1] == b'E' && data[2] == b'L' && data[3] == b'F'
    {
        return true;
    }

    // Heuristic: count E8/E9 opcodes followed by plausible relative offsets
    let sample_len = 4096.min(data.len());
    let mut e8_count = 0u32;
    let mut total_bytes = 0u32;

    let mut i = 0;
    while i + 5 <= sample_len {
        if data[i] == 0xE8 || data[i] == 0xE9 {
            let rel = i32::from_le_bytes([data[i + 1], data[i + 2], data[i + 3], data[i + 4]]);
            // Plausible if offset is within ±1MB
            if rel.abs() < 1_000_000 {
                e8_count += 1;
            }
        }
        total_bytes += 1;
        i += 1;
    }

    // If >1% of bytes are E8/E9 with plausible offsets, likely code
    e8_count > 0 && (e8_count as f64 / total_bytes as f64) > 0.01
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        // Simulated x86 code with CALL instructions
        let mut data = vec![0x90u8; 1000]; // NOPs
        // CALL +0x100 at position 10
        data[10] = 0xE8;
        data[11..15].copy_from_slice(&100i32.to_le_bytes());
        // CALL +0x100 at position 500
        data[500] = 0xE8;
        data[501..505].copy_from_slice(&100i32.to_le_bytes());
        // JMP -0x50 at position 800
        data[800] = 0xE9;
        data[801..805].copy_from_slice(&(-80i32).to_le_bytes());

        let encoded = encode(&data);
        let decoded = decode(&encoded);
        assert_eq!(decoded, data, "BCJ roundtrip failed");
    }

    #[test]
    fn test_calls_become_same() {
        // Two CALL instructions to the same absolute address should become identical after BCJ
        let mut data = vec![0x90u8; 1000];
        // CALL to absolute address 0x200 from position 0x10: relative = 0x200 - 0x10 - 5 = 0x1EB
        data[0x10] = 0xE8;
        data[0x11..0x15].copy_from_slice(&0x1EBi32.to_le_bytes());
        // CALL to absolute address 0x200 from position 0x100: relative = 0x200 - 0x100 - 5 = 0xFB
        data[0x100] = 0xE8;
        data[0x101..0x105].copy_from_slice(&0xFBi32.to_le_bytes());

        let encoded = encode(&data);

        // After BCJ, both should encode the same absolute address
        let addr1 = i32::from_le_bytes([encoded[0x11], encoded[0x12], encoded[0x13], encoded[0x14]]);
        let addr2 = i32::from_le_bytes([encoded[0x101], encoded[0x102], encoded[0x103], encoded[0x104]]);
        assert_eq!(addr1, addr2, "Calls to same target should produce same bytes after BCJ");
    }

    #[test]
    fn test_detect_exe() {
        // PE header
        let mut data = vec![0u8; 1000];
        data[0] = b'M';
        data[1] = b'Z';
        assert!(is_likely_executable(&data));
    }

    #[test]
    fn test_no_corruption_on_non_code() {
        // Random data shouldn't be corrupted much (E8/E9 with implausible offsets are skipped)
        let data: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();
        let encoded = encode(&data);
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }
}
