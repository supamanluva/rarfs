use std::fs::File;
use std::io::{self, BufReader, Read, Seek};
use std::path::Path;

use super::{bad, MemberHeader, Method, Segment};

pub const SIG4: [u8; 7] = *b"Rar!\x1a\x07\x00";

const LHD_SPLIT_BEFORE: u16 = 0x0001;
const LHD_SPLIT_AFTER: u16 = 0x0002;
const LHD_LARGE: u16 = 0x0100;
const LHD_UNICODE: u16 = 0x0200;
const LONG_BLOCK: u16 = 0x8000;

const T_FILE: u8 = 0x74;
const T_ENDARC: u8 = 0x7b;

pub fn parse_volume(path: &Path) -> io::Result<Vec<MemberHeader>> {
    let mut f = BufReader::new(File::open(path)?);
    let mut sig = [0u8; 7];
    f.read_exact(&mut sig)?;
    if sig != SIG4 {
        return Err(bad("not a RAR4 volume"));
    }
    let mut members = Vec::new();
    loop {
        let mut base = [0u8; 7];
        match f.read_exact(&mut base) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e),
        }
        let btype = base[2];
        let flags = u16::from_le_bytes([base[3], base[4]]);
        let head_size = u16::from_le_bytes([base[5], base[6]]) as u64;
        let mut add_size = 0u64;
        if flags & LONG_BLOCK != 0 {
            let mut a = [0u8; 4];
            f.read_exact(&mut a)?;
            add_size = u32::from_le_bytes(a) as u64;
        }
        let fixed = 7 + if flags & LONG_BLOCK != 0 { 4 } else { 0 };
        if head_size < fixed {
            return Err(bad("bad head size"));
        }
        let body_len = (head_size - fixed) as usize;
        if btype == T_FILE {
            let mut body = vec![0u8; body_len];
            f.read_exact(&mut body)?;
            let data_offset = f.stream_position()?;
            if let Some(m) = parse_file(&body, flags, add_size, path, data_offset)? {
                members.push(m);
            }
        } else {
            f.seek_relative(body_len as i64)?;
        }
        f.seek_relative(add_size as i64)?; // skip packed data
        if btype == T_ENDARC {
            break;
        }
    }
    Ok(members)
}

fn parse_file(
    body: &[u8],
    flags: u16,
    pack_lo: u64,
    volume: &Path,
    data_offset: u64,
) -> io::Result<Option<MemberHeader>> {
    if body.len() < 21 {
        return Err(bad("short file header"));
    }
    let unp_lo = u32::from_le_bytes(body[0..4].try_into().unwrap()) as u64;
    let crc32 = u32::from_le_bytes(body[5..9].try_into().unwrap());
    let method_b = body[14];
    let name_size = u16::from_le_bytes([body[15], body[16]]) as usize;
    let mut o = 21usize;
    let mut pack_hi = 0u64;
    let mut unp_hi = 0u64;
    if flags & LHD_LARGE != 0 {
        if body.len() < o + 8 {
            return Err(bad("short LHD_LARGE fields"));
        }
        pack_hi = u32::from_le_bytes(body[o..o + 4].try_into().unwrap()) as u64;
        unp_hi = u32::from_le_bytes(body[o + 4..o + 8].try_into().unwrap()) as u64;
        o += 8;
    }
    let name_bytes = body.get(o..o + name_size).ok_or(bad("bad name"))?;
    let name = if flags & LHD_UNICODE != 0 {
        // With LHD_UNICODE the field is the plain codepage name followed by a
        // NUL byte and the encoded unicode name (unrar's EncName convention).
        let enc = match name_bytes.iter().position(|&b| b == 0) {
            Some(p) => &name_bytes[p + 1..],
            None => name_bytes,
        };
        decode_unicode_name(enc)
    } else {
        String::from_utf8_lossy(name_bytes).into_owned()
    };
    let pack = pack_lo | (pack_hi << 32);
    let unp = unp_lo | (unp_hi << 32);
    Ok(Some(MemberHeader {
        name,
        unpacked_size: unp,
        method: if method_b == 0x30 {
            Method::Store
        } else {
            Method::Compressed(method_b - 0x30)
        },
        crc32,
        split_before: flags & LHD_SPLIT_BEFORE != 0,
        split_after: flags & LHD_SPLIT_AFTER != 0,
        segment: Segment {
            volume: volume.to_path_buf(),
            data_offset,
            data_len: pack,
        },
    }))
}

/// RAR4 LHD_UNICODE name decoding (same algorithm as unrar's
/// EncodeFileName::Decode). Produces UTF-16 code units, then a String.
fn decode_unicode_name(enc: &[u8]) -> String {
    if enc.is_empty() {
        return String::new();
    }
    let high = enc[0] as u16;
    let mut units: Vec<u16> = Vec::new();
    let mut pos = 1usize;
    let mut flag_bits = 0;
    let mut flags = 0u8;
    while pos < enc.len() {
        if flag_bits == 0 {
            flags = enc[pos];
            pos += 1;
            flag_bits = 8;
        }
        match flags >> 6 {
            0 => {
                units.push(enc[pos] as u16);
                pos += 1;
            }
            1 => {
                units.push((high << 8) | enc[pos] as u16);
                pos += 1;
            }
            2 => {
                if pos + 1 >= enc.len() {
                    break;
                }
                units.push(((enc[pos + 1] as u16) << 8) | enc[pos] as u16);
                pos += 2;
            }
            _ => {
                let len = enc[pos] as usize;
                pos += 1;
                if len & 0x80 != 0 {
                    if pos >= enc.len() {
                        break;
                    }
                    let corr = enc[pos] as u16;
                    pos += 1;
                    for _ in 0..(len & 0x7f) + 2 {
                        if pos >= enc.len() {
                            break;
                        }
                        units.push((high << 8) | ((enc[pos] as u16 + corr) & 0xff));
                        pos += 1;
                    }
                } else {
                    for _ in 0..len + 2 {
                        if pos >= enc.len() {
                            break;
                        }
                        units.push(enc[pos] as u16);
                        pos += 1;
                    }
                }
            }
        }
        flags <<= 2;
        flag_bits -= 2;
    }
    String::from_utf16_lossy(&units)
}
