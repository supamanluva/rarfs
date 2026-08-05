mod common;

use rarfs::reader::{MemberReader, StoreReader};

fn make_set(tmp: &tempfile::TempDir, payload: &[u8], nvols: usize) -> rarfs::aset::AssembledMember {
    let vols = common::build_rar5(tmp.path(), "vid", "vid.mkv", payload, nvols);
    rarfs::aset::parse_set(&vols).unwrap().into_iter().next().unwrap()
}

#[test]
fn full_sequential_read_matches_payload() {
    let tmp = tempfile::tempdir().unwrap();
    let payload: Vec<u8> = (0..70_000u32).map(|i| (i % 227) as u8).collect();
    let m = make_set(&tmp, &payload, 3);
    let mut r = StoreReader::new(m.size, m.segments.clone());
    assert_eq!(r.size(), payload.len() as u64);
    let mut got = vec![0u8; payload.len()];
    let mut off = 0usize;
    while off < got.len() {
        let end = (off + 8192).min(got.len());
        let n = r.read_at(off as u64, &mut got[off..end]).unwrap();
        assert!(n > 0);
        off += n;
    }
    assert_eq!(got, payload);
}

#[test]
fn truncated_volume_yields_eio_not_garbage() {
    let tmp = tempfile::tempdir().unwrap();
    let payload: Vec<u8> = (0..60_000u32).map(|i| (i % 229) as u8).collect();
    let vols = common::build_rar5(tmp.path(), "vid", "vid.mkv", &payload, 3);
    let m = rarfs::aset::parse_set(&vols).unwrap().into_iter().next().unwrap();
    // Truncate the last volume by 100 bytes.
    let last = vols.last().unwrap();
    let orig_len = std::fs::metadata(last).unwrap().len();
    let f = std::fs::OpenOptions::new().write(true).open(last).unwrap();
    f.set_len(orig_len - 100).unwrap();
    drop(f);

    let mut r = StoreReader::new(m.size, m.segments.clone());
    // Reads before the truncated region still succeed.
    let mut buf = vec![0u8; 4096];
    let n = r.read_at(0, &mut buf).unwrap();
    assert_eq!(n, 4096);
    assert_eq!(&buf, &payload[..4096]);
    // A range covering the truncated tail must fail, not return zero-fill.
    let mut tail = vec![0u8; 4096];
    let err = r
        .read_at(payload.len() as u64 - 4096, &mut tail)
        .expect_err("read covering truncated region must fail");
    assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
}

#[test]
fn random_offset_reads_match_payload() {
    let tmp = tempfile::tempdir().unwrap();
    let payload: Vec<u8> = (0..100_000u32).map(|i| (i % 223) as u8).collect();
    let m = make_set(&tmp, &payload, 7);
    let mut r = StoreReader::new(m.size, m.segments.clone());
    // Deterministic pseudo-random probes, incl. ranges spanning volume splits.
    let mut state = 0x12345678u64;
    let mut next = move || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 33) as usize
    };
    for _ in 0..200 {
        let off = next() % payload.len();
        let len = (next() % 9000 + 1).min(payload.len() - off);
        let mut buf = vec![0u8; len];
        let n = r.read_at(off as u64, &mut buf).unwrap();
        assert_eq!(n, len);
        assert_eq!(&buf, &payload[off..off + len]);
    }
    // EOF behavior
    let mut one = [0u8; 16];
    assert_eq!(r.read_at(payload.len() as u64, &mut one).unwrap(), 0);
    let tail = r.read_at(payload.len() as u64 - 4, &mut one).unwrap();
    assert_eq!(tail, 4);
    assert_eq!(&one[..4], &payload[payload.len() - 4..]);
}
