/// BCJ filter for x86/x64: converts relative CALL/JMP addresses to
/// absolute, so calls to the same function produce identical bytes
/// regardless of call site.

fn transform(data: &[u8], to_absolute: bool) -> Vec<u8> {
    let mut buf = data.to_vec();
    let mut i = 0;

    while i + 5 <= buf.len() {
        // E8 = CALL, E9 = JMP
        let is_call_jmp = buf[i] == 0xE8 || buf[i] == 0xE9;
        // 0F 8x = conditional jump (Jcc near) — same encoding as 7-Zip BCJ
        let is_jcc = i + 6 <= buf.len() && buf[i] == 0x0F && (buf[i+1] & 0xF0) == 0x80;

        if is_call_jmp {
            convert_branch(&mut buf, i + 1, i as i32, data.len(), to_absolute);
            i += 5;
        } else if is_jcc {
            convert_branch(&mut buf, i + 2, (i + 2) as i32, data.len(), to_absolute);
            i += 6;
        } else {
            i += 1;
        }
    }

    buf
}

fn convert_branch(buf: &mut [u8], off: usize, pc: i32, file_len: usize, to_absolute: bool) {
    let val = i32::from_le_bytes([buf[off], buf[off+1], buf[off+2], buf[off+3]]);
    let abs_addr = if to_absolute { val.wrapping_add(pc) } else { val };

    if abs_addr >= 0 && (abs_addr as usize) < file_len + 65536 {
        let converted = if to_absolute { abs_addr } else { val.wrapping_sub(pc) };
        let bytes = converted.to_le_bytes();
        buf[off] = bytes[0];
        buf[off+1] = bytes[1];
        buf[off+2] = bytes[2];
        buf[off+3] = bytes[3];
    }
}

pub fn encode(data: &[u8]) -> Vec<u8> {
    transform(data, true)
}

pub fn decode(data: &[u8]) -> Vec<u8> {
    transform(data, false)
}

/// Heuristic check for x86 machine code (PE/ELF headers or E8/E9 density).
pub fn is_likely_executable(data: &[u8]) -> bool {
    if data.len() < 64 {
        return false;
    }

    if data.len() > 0x40 && data[0] == b'M' && data[1] == b'Z' {
        return true;
    }
    if data.len() > 4 && data[0] == 0x7F && data[1] == b'E' && data[2] == b'L' && data[3] == b'F' {
        return true;
    }

    let slen = 4096.min(data.len());
    let mut hits = 0u32;
    let mut total = 0u32;

    let mut i = 0;
    while i + 5 <= slen {
        if data[i] == 0xE8 || data[i] == 0xE9 {
            let rel = i32::from_le_bytes([data[i+1], data[i+2], data[i+3], data[i+4]]);
            if rel.abs() < 1_000_000 { hits += 1; }
        } else if i + 6 <= slen && data[i] == 0x0F && (data[i+1] & 0xF0) == 0x80 {
            let rel = i32::from_le_bytes([data[i+2], data[i+3], data[i+4], data[i+5]]);
            if rel.abs() < 1_000_000 { hits += 1; }
        }
        total += 1;
        i += 1;
    }

    hits > 0 && (hits as f64 / total as f64) > 0.01
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calls_to_same_target_become_identical() {
        let mut data = vec![0x90u8; 1000];
        // CALL to absolute 0x200 from 0x10: relative = 0x1EB
        data[0x10] = 0xE8;
        data[0x11..0x15].copy_from_slice(&0x1EBi32.to_le_bytes());
        // CALL to absolute 0x200 from 0x100: relative = 0xFB
        data[0x100] = 0xE8;
        data[0x101..0x105].copy_from_slice(&0xFBi32.to_le_bytes());

        let enc = encode(&data);
        let a1 = i32::from_le_bytes([enc[0x11], enc[0x12], enc[0x13], enc[0x14]]);
        let a2 = i32::from_le_bytes([enc[0x101], enc[0x102], enc[0x103], enc[0x104]]);
        assert_eq!(a1, a2);
    }

    #[test]
    fn roundtrip() {
        let mut data = vec![0x90u8; 1000];
        data[10] = 0xE8;
        data[11..15].copy_from_slice(&100i32.to_le_bytes());
        data[500] = 0xE8;
        data[501..505].copy_from_slice(&100i32.to_le_bytes());
        data[800] = 0xE9;
        data[801..805].copy_from_slice(&(-80i32).to_le_bytes());

        assert_eq!(decode(&encode(&data)), data);
    }

    #[test]
    fn pe_header_detected() {
        let mut data = vec![0u8; 1000];
        data[0] = b'M';
        data[1] = b'Z';
        assert!(is_likely_executable(&data));
    }

    #[test]
    fn non_code_survives_roundtrip() {
        let data: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();
        assert_eq!(decode(&encode(&data)), data);
    }
}
