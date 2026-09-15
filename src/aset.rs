use std::fs::File;
use std::io::{self, Read};
use std::path::PathBuf;
use std::time::SystemTime;

use crate::rarhdr::{bad, rar4, rar5, Method, Segment};

#[derive(Debug, Clone)]
pub struct AssembledMember {
    pub name: String,
    pub size: u64,
    pub method: Method,
    pub crc32: u32,
    pub mtime: Option<SystemTime>,
    pub segments: Vec<Segment>,
}

enum Format {
    Rar4,
    Rar5,
}

fn detect(path: &std::path::Path) -> io::Result<Format> {
    let mut buf = [0u8; 8];
    let mut f = File::open(path)?;
    let n = f.read(&mut buf)?;
    if n >= 8 && buf == rar5::SIG5 {
        Ok(Format::Rar5)
    } else if n >= 7 && buf[..7] == rar4::SIG4 {
        Ok(Format::Rar4)
    } else {
        Err(bad("unrecognized archive signature"))
    }
}

struct MemberBuilder {
    member: AssembledMember,
    wants_more: bool, // last header for this member had split_after set
}

pub fn parse_set(volumes: &[PathBuf]) -> io::Result<Vec<AssembledMember>> {
    if volumes.is_empty() {
        return Err(bad("empty volume list"));
    }
    let fmt = detect(&volumes[0])?;
    let mut builders: Vec<MemberBuilder> = Vec::new();
    for vol in volumes {
        let headers = match fmt {
            Format::Rar4 => rar4::parse_volume(vol)?,
            Format::Rar5 => rar5::parse_volume(vol)?,
        };
        for h in headers {
            if !h.split_before {
                builders.push(MemberBuilder {
                    member: AssembledMember {
                        name: h.name.clone(),
                        size: h.unpacked_size,
                        method: h.method,
                        crc32: h.crc32,
                        mtime: h.mtime,
                        segments: vec![h.segment.clone()],
                    },
                    wants_more: h.split_after,
                });
            } else {
                match builders.iter_mut().rev().find(|b| b.member.name == h.name) {
                    Some(b) => {
                        b.member.segments.push(h.segment.clone());
                        b.wants_more = h.split_after;
                    }
                    None => return Err(bad("split continuation without start")),
                }
            }
        }
    }
    Ok(builders
        .into_iter()
        .filter(|b| !b.wants_more)
        .filter_map(|b| {
            let m = b.member;
            // Store mode packs bytes 1:1, so a segment-length shortfall
            // means a middle volume is missing. (Compressed members can't
            // be checked this way — packed size != unpacked size.)
            if m.method == Method::Store {
                let have: u64 = m.segments.iter().map(|s| s.data_len).sum();
                if have != m.size {
                    tracing::warn!(
                        member = %m.name,
                        shortfall = m.size.saturating_sub(have),
                        "dropping member: stored data shortfall, missing volume?"
                    );
                    return None;
                }
            }
            Some(m)
        })
        .collect())
}
