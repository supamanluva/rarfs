use std::cmp::Ordering;
use std::fs::File;
use std::io;
use std::os::unix::fs::FileExt;

use crate::rarhdr::Segment;

pub trait MemberReader: Send {
    fn size(&self) -> u64;
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<usize>;
}

/// Serves a stored (uncompressed) member by mapping logical offsets to
/// byte ranges inside the volume files and pread()ing them directly.
/// Stateless apart from the segment map; safe for unlimited concurrent use.
pub struct StoreReader {
    size: u64,
    spans: Vec<(u64, Segment)>, // (logical start, segment)
}

impl StoreReader {
    pub fn new(size: u64, segments: Vec<Segment>) -> StoreReader {
        let mut spans = Vec::with_capacity(segments.len());
        let mut base = 0u64;
        for s in segments {
            spans.push((base, s.clone()));
            base += s.data_len;
        }
        StoreReader { size, spans }
    }
}

impl MemberReader for StoreReader {
    fn size(&self) -> u64 {
        self.size
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        if offset >= self.size {
            return Ok(0);
        }
        let want = ((self.size - offset).min(buf.len() as u64)) as usize;
        let buf = &mut buf[..want];
        let mut written = 0usize;
        while written < buf.len() {
            let cur = offset + written as u64;
            let idx = self
                .spans
                .binary_search_by(|(base, seg)| {
                    if cur < *base {
                        Ordering::Greater
                    } else if cur >= base + seg.data_len {
                        Ordering::Less
                    } else {
                        Ordering::Equal
                    }
                })
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "gap in segment map"))?;
            let (base, seg) = &self.spans[idx];
            let in_seg = cur - base;
            let n = (seg.data_len - in_seg).min((buf.len() - written) as u64) as usize;
            let f = File::open(&seg.volume)?;
            // A short read here means the volume file is truncated: the
            // clamping above never requests past the member's declared size,
            // so any shortfall is missing data, not EOF. Surface it as an
            // error instead of returning zero-filled garbage.
            let got = f.read_at(&mut buf[written..written + n], seg.data_offset + in_seg)?;
            if got < n {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "short read from volume (file truncated?)",
                ));
            }
            written += n;
        }
        Ok(written)
    }
}
