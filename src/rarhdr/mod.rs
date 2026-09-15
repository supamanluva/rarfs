use std::path::PathBuf;
use std::time::SystemTime;

pub mod rar4;
pub mod rar5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Store,
    Compressed(u8),
}

#[derive(Debug, Clone)]
pub struct Segment {
    pub volume: PathBuf,
    pub data_offset: u64,
    pub data_len: u64,
}

/// One file header encountered in one volume. For split files each volume
/// yields its own MemberHeader; assembly across volumes happens in `aset`.
#[derive(Debug, Clone)]
pub struct MemberHeader {
    pub name: String,
    pub unpacked_size: u64,
    pub method: Method,
    pub crc32: u32,
    pub mtime: Option<SystemTime>,
    pub split_before: bool,
    pub split_after: bool,
    pub segment: Segment,
}

pub fn bad(msg: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg.to_string())
}

/// RAR5 variable-length integer: 7-bit groups, high bit = continuation.
pub fn read_vint(buf: &[u8]) -> Option<(u64, usize)> {
    let mut v: u64 = 0;
    for (i, &b) in buf.iter().take(10).enumerate() {
        v |= ((b & 0x7f) as u64) << (7 * i);
        if b & 0x80 == 0 {
            return Some((v, i + 1));
        }
    }
    None
}
