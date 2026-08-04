// Shared fixture builders; some helpers are only used by later tasks' tests.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn push_vint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let mut b = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            b |= 0x80;
        }
        out.push(b);
        if v == 0 {
            break;
        }
    }
}

/// Assemble one RAR5 block. CRC32 covers everything after the crc field
/// (head-size vint + header data); the data area follows the header.
pub fn block5(btype: u64, flags: u64, extra: &[u8], body: &[u8], data: &[u8]) -> Vec<u8> {
    let mut flags = flags;
    if !extra.is_empty() {
        flags |= 0x0001;
    }
    if !data.is_empty() {
        flags |= 0x0002;
    }
    let mut payload = Vec::new(); // HEAD_TYPE .. end of header
    push_vint(&mut payload, btype);
    push_vint(&mut payload, flags);
    if !extra.is_empty() {
        push_vint(&mut payload, extra.len() as u64);
    }
    if !data.is_empty() {
        push_vint(&mut payload, data.len() as u64);
    }
    payload.extend_from_slice(body);
    payload.extend_from_slice(extra);
    let mut sized = Vec::new();
    push_vint(&mut sized, payload.len() as u64);
    sized.extend_from_slice(&payload);
    let mut h = crc32fast::Hasher::new();
    h.update(&sized);
    let mut out = Vec::new();
    out.extend_from_slice(&h.finalize().to_le_bytes());
    out.extend_from_slice(&sized);
    out.extend_from_slice(data);
    out
}

pub fn main_hdr5(multivolume: bool, vnum: Option<u64>) -> Vec<u8> {
    let mut body = Vec::new();
    let mut aflags = 0u64;
    if multivolume {
        aflags |= 0x0001;
    }
    if vnum.is_some() {
        aflags |= 0x0002;
    }
    push_vint(&mut body, aflags);
    if let Some(n) = vnum {
        push_vint(&mut body, n);
    }
    block5(1, 0, &[], &body, &[])
}

pub fn file_hdr5(
    name: &str,
    unp_size: u64,
    crc: u32,
    split_before: bool,
    split_after: bool,
    data: &[u8],
) -> Vec<u8> {
    let mut body = Vec::new();
    push_vint(&mut body, 0x0004); // FILE_FLAGS: crc32 field present
    push_vint(&mut body, unp_size);
    push_vint(&mut body, 0o100644); // ATTR
    body.extend_from_slice(&crc.to_le_bytes());
    push_vint(&mut body, 0); // COMP_INFO: version 0, method 0 (store)
    push_vint(&mut body, 1); // HOST_OS: unix
    push_vint(&mut body, name.len() as u64);
    body.extend_from_slice(name.as_bytes());
    let mut flags = 0u64;
    if split_before {
        flags |= 0x0008;
    }
    if split_after {
        flags |= 0x0010;
    }
    block5(2, flags, &[], &body, data)
}

pub fn endarc5() -> Vec<u8> {
    block5(5, 0, &[], &[], &[])
}

/// Build a store-mode RAR5 archive of `payload` split into `nvols` volumes
/// named `<base>.part1.rar`, `<base>.part2.rar`, … Returns volume paths in order.
pub fn build_rar5(dir: &Path, base: &str, name: &str, payload: &[u8], nvols: usize) -> Vec<PathBuf> {
    let mut h = crc32fast::Hasher::new();
    h.update(payload);
    let crc = h.finalize();
    let chunk = payload.len().div_ceil(nvols);
    let mut paths = Vec::new();
    for i in 0..nvols {
        let mut vol = b"Rar!\x1a\x07\x01\x00".to_vec();
        vol.extend_from_slice(&main_hdr5(nvols > 1, if i == 0 { None } else { Some(i as u64) }));
        let end = ((i + 1) * chunk).min(payload.len());
        vol.extend_from_slice(&file_hdr5(name, payload.len() as u64, crc, i > 0, i + 1 < nvols, &payload[i * chunk..end]));
        if i + 1 == nvols {
            vol.extend_from_slice(&endarc5());
        }
        let p = dir.join(format!("{base}.part{}.rar", i + 1));
        fs::write(&p, &vol).unwrap();
        paths.push(p);
    }
    paths
}

/// Build a RAR4 archive with the system `rar` binary (v4.20). Returns None if
/// `rar` is unavailable so tests can skip.
pub fn build_rar4(
    dir: &Path,
    base: &str,
    payload_file: &Path,
    store: bool,
    vol_kb: Option<usize>,
    old_naming: bool,
) -> Option<Vec<PathBuf>> {
    let mut cmd = Command::new("rar");
    cmd.arg("a")
        .arg(if store { "-m0" } else { "-m1" })
        .arg("-idq"); // quiet
    if old_naming {
        cmd.arg("-vn");
    }
    if let Some(k) = vol_kb {
        cmd.arg(format!("-v{k}k"));
    }
    cmd.arg(dir.join(format!("{base}.rar"))).arg(payload_file);
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    // Collect produced volumes in order. Inlined here so this helper compiles
    // before Task 4 lands the real grouper (`rarfs::volumes::group_volumes`);
    // Task 4 may refactor this to reuse it.
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| {
            let n = e.unwrap().file_name().to_string_lossy().into_owned();
            if n.starts_with(base) && n != payload_file.file_name().unwrap().to_string_lossy() {
                Some(n)
            } else {
                None
            }
        })
        .collect();
    names.sort_by_key(|n| volume_order_key(base, n));
    Some(names.iter().map(|n| dir.join(n)).collect())
}

/// Order volumes of one set so the first volume comes first:
/// old-style `base.rar` then `base.r00`, `base.r01`, …;
/// new-style `base.part1.rar`, `base.part2.rar`, … sorted numerically.
fn volume_order_key(base: &str, name: &str) -> (u8, u64) {
    let rest = &name[base.len()..];
    if rest == ".rar" {
        return (0, 0);
    }
    if let Some(n) = rest
        .strip_prefix(".part")
        .and_then(|s| s.strip_suffix(".rar"))
        .and_then(|s| s.parse::<u64>().ok())
    {
        return (1, n);
    }
    if let Some(n) = rest.strip_prefix(".r").and_then(|s| s.parse::<u64>().ok()) {
        return (2, n);
    }
    (3, 0)
}
