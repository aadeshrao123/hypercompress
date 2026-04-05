/// Run-length encoding using an escape byte scheme.
///
/// Format: escape byte 0xFF followed by:
///   0x00        -> literal 0xFF
///   N (1..=255) -> repeat NEXT byte (N+2) times (3..257 repetitions)

const ESCAPE: u8 = 0xFF;

fn emit_run(val: u8, mut count: usize, out: &mut Vec<u8>) {
    if val == ESCAPE {
        while count >= 3 {
            let chunk = count.min(257);
            out.push(ESCAPE);
            out.push((chunk - 2) as u8);
            out.push(ESCAPE);
            count -= chunk;
        }
        for _ in 0..count {
            out.push(ESCAPE);
            out.push(0x00);
        }
    } else {
        while count >= 3 {
            let chunk = count.min(257);
            out.push(ESCAPE);
            out.push((chunk - 2) as u8);
            out.push(val);
            count -= chunk;
        }
        for _ in 0..count {
            out.push(val);
        }
    }
}

pub fn encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;

    while i < data.len() {
        let val = data[i];
        let mut run = 1usize;
        while i + run < data.len() && data[i + run] == val {
            run += 1;
        }

        if val == ESCAPE || run >= 3 {
            emit_run(val, run, &mut out);
        } else {
            out.push(val);
        }
        i += run;
    }

    out
}

pub fn decode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 2);
    let mut i = 0;

    while i < data.len() {
        if data[i] == ESCAPE {
            if i + 1 >= data.len() {
                break;
            }
            let n = data[i + 1];
            if n == 0x00 {
                out.push(ESCAPE);
                i += 2;
            } else {
                if i + 2 >= data.len() {
                    break;
                }
                let val = data[i + 2];
                for _ in 0..n as usize + 2 {
                    out.push(val);
                }
                i += 3;
            }
        } else {
            out.push(data[i]);
            i += 1;
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_data_compresses_well() {
        let mut data = vec![0u8; 100];
        data[50] = 42;
        let enc = encode(&data);
        assert!(enc.len() < 20, "encoded len={}", enc.len());
        assert_eq!(decode(&enc), data);
    }

    #[test]
    fn all_same() {
        let data = vec![42u8; 200];
        let enc = encode(&data);
        assert!(enc.len() < 10, "encoded len={}", enc.len());
        assert_eq!(decode(&enc), data);
    }

    #[test]
    fn no_runs() {
        let data: Vec<u8> = (0..50).collect();
        assert_eq!(decode(&encode(&data)), data);
    }

    #[test]
    fn literal_0xff() {
        let data = vec![0xFE, 0xFF, 0xFE, 0xFF, 0x00, 0xFF];
        assert_eq!(decode(&encode(&data)), data);
    }

    #[test]
    fn run_of_0xff() {
        let data = vec![0xFF; 50];
        assert_eq!(decode(&encode(&data)), data);
    }

    #[test]
    fn mixed_runs_and_literals() {
        let mut data = Vec::new();
        data.extend_from_slice(&[1, 2, 3]);
        data.extend_from_slice(&[0; 20]);
        data.extend_from_slice(&[0xFF; 10]);
        data.extend_from_slice(&[42, 43, 44]);
        data.extend_from_slice(&[7; 300]);
        assert_eq!(decode(&encode(&data)), data);
    }

    #[test]
    fn all_byte_values() {
        let data: Vec<u8> = (0..=255).collect();
        assert_eq!(decode(&encode(&data)), data);
    }

    #[test]
    fn empty() {
        assert_eq!(encode(&[]), Vec::<u8>::new());
        assert_eq!(decode(&[]), Vec::<u8>::new());
    }
}
