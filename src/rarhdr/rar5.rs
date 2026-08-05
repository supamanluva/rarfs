use std::fs::File;
use std::io::{self, BufReader, Read, Seek};
use std::path::Path;

use super::{bad, read_vint, MemberHeader, Method, Segment};

pub const SIG5: [u8; 8] = *b"Rar!\x1a\x07\x01\x00";

const HFL_EXTRA: u64 = 0x0001;
const HFL_DATA: u64 = 0x0002;
const HFL_SPLIT_BEFORE: u64 = 0x0008;
const HFL_SPLIT_AFTER: u64 = 0x0010;

const T_FILE: u64 = 2;
const T_ENDARC: u64 = 5;

// HEAD_SIZE is an unbounded vint from untrusted input; real RAR5 headers
// are kilobytes at most. Cap it before allocating to avoid OOM on hostile
// archives.
const MAX_HEAD_SIZE: u64 = 16 * 1024 * 1024;

pub fn parse_volume(path: &Path) -> io::Result<Vec<MemberHeader>> {
    let mut f = BufReader::new(File::open(path)?);
    let mut sig = [0u8; 8];
    f.read_exact(&mut sig)?;
    if sig != SIG5 {
        return Err(bad("not a RAR5 volume"));
    }
    let mut members = Vec::new();
    loop {
        let mut crc = [0u8; 4];
        match f.read_exact(&mut crc) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e),
        }
        // HEAD_SIZE vint, read byte by byte.
        let mut size_bytes = Vec::new();
        let head_size = loop {
            let mut b = [0u8; 1];
            f.read_exact(&mut b)?;
            size_bytes.push(b[0]);
            if let Some((v, n)) = read_vint(&size_bytes) {
                if n == size_bytes.len() {
                    break v;
                }
            }
            if size_bytes.len() >= 10 {
                return Err(bad("oversized vint"));
            }
        };
        if head_size > MAX_HEAD_SIZE {
            return Err(bad("oversized header"));
        }
        let mut hdr = vec![0u8; head_size as usize];
        f.read_exact(&mut hdr)?;
        let (btype, n1) = read_vint(&hdr).ok_or(bad("bad block type"))?;
        let (flags, n2) = read_vint(&hdr[n1..]).ok_or(bad("bad block flags"))?;
        let mut o = n1 + n2;
        if flags & HFL_EXTRA != 0 {
            let (_, k) = read_vint(&hdr[o..]).ok_or(bad("bad extra size"))?;
            o += k;
        }
        let mut data_len = 0u64;
        if flags & HFL_DATA != 0 {
            let (d, k) = read_vint(&hdr[o..]).ok_or(bad("bad data size"))?;
            data_len = d;
            o += k;
        }
        if btype == T_FILE {
            let data_offset = f.stream_position()?;
            if let Some(m) = parse_file(&hdr[o..], path, data_offset, data_len, flags)? {
                members.push(m);
            }
        }
        f.seek_relative(data_len as i64)?;
        if btype == T_ENDARC {
            break;
        }
    }
    Ok(members)
}

fn parse_file(
    body: &[u8],
    volume: &Path,
    data_offset: u64,
    data_len: u64,
    hflags: u64,
) -> io::Result<Option<MemberHeader>> {
    let (fflags, n1) = read_vint(body).ok_or(bad("bad file flags"))?;
    let (unp, n2) = read_vint(&body[n1..]).ok_or(bad("bad unp size"))?;
    let mut o = n1 + n2;
    let (_attr, k) = read_vint(&body[o..]).ok_or(bad("bad attr"))?;
    o += k;
    if fflags & 0x0002 != 0 {
        // Bounds-check like the crc read below: a truncated body would
        // otherwise make the next `body[o..]` indexing panic.
        body.get(o..o + 4).ok_or(bad("bad mtime"))?;
        o += 4; // mtime
    }
    let mut crc32 = 0u32;
    if fflags & 0x0004 != 0 {
        let b = body.get(o..o + 4).ok_or(bad("bad crc"))?;
        crc32 = u32::from_le_bytes(b.try_into().unwrap());
        o += 4;
    }
    let (cinfo, k) = read_vint(&body[o..]).ok_or(bad("bad comp info"))?;
    o += k;
    let (_host, k) = read_vint(&body[o..]).ok_or(bad("bad host os"))?;
    o += k;
    let (nlen, k) = read_vint(&body[o..]).ok_or(bad("bad name len"))?;
    o += k;
    let name_bytes = body.get(o..o + nlen as usize).ok_or(bad("bad name"))?;
    let name = String::from_utf8_lossy(name_bytes).into_owned();
    if fflags & 0x0001 != 0 {
        return Ok(None); // directory entry
    }
    let m = (cinfo >> 10) & 0x1f;
    Ok(Some(MemberHeader {
        name,
        unpacked_size: unp,
        method: if m == 0 { Method::Store } else { Method::Compressed(m as u8) },
        crc32,
        split_before: hflags & HFL_SPLIT_BEFORE != 0,
        split_after: hflags & HFL_SPLIT_AFTER != 0,
        segment: Segment {
            volume: volume.to_path_buf(),
            data_offset,
            data_len,
        },
    }))
}
