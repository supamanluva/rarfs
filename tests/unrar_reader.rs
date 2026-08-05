mod common;

use rarfs::reader::MemberReader;
use rarfs::unrar_reader::UnrarReader;

fn make_compressed(tmp: &tempfile::TempDir, payload: &[u8]) -> std::path::PathBuf {
    let pf = tmp.path().join("payload.bin");
    std::fs::write(&pf, payload).unwrap();
    common::build_rar4(tmp.path(), "cr", &pf, false, None, false)
        .expect("rar binary required for this test")
        .into_iter()
        .next()
        .unwrap()
}

#[test]
fn full_read_matches_original() {
    let tmp = tempfile::tempdir().unwrap();
    let payload: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let vol = make_compressed(&tmp, &payload);
    let mut r = UnrarReader::new(&vol, "payload.bin", payload.len() as u64).unwrap();
    assert_eq!(r.size(), payload.len() as u64);
    let mut got = vec![0u8; payload.len()];
    let mut off = 0usize;
    while off < got.len() {
        let end = (off + 16_384).min(got.len());
        let n = r.read_at(off as u64, &mut got[off..end]).unwrap();
        assert!(n > 0);
        off += n;
    }
    assert_eq!(got, payload);
}

#[test]
fn small_backward_seek_within_window_does_not_corrupt() {
    let tmp = tempfile::tempdir().unwrap();
    let payload: Vec<u8> = (0..300_000u32).map(|i| (i % 239) as u8).collect();
    let vol = make_compressed(&tmp, &payload);
    let mut r = UnrarReader::new(&vol, "payload.bin", payload.len() as u64).unwrap();
    // Read forward 100k, then seek back 4k (typical player behavior).
    let mut buf = vec![0u8; 100_000];
    let n = r.read_at(0, &mut buf).unwrap();
    assert_eq!(n, 100_000);
    let mut probe = vec![0u8; 4096];
    let n = r.read_at(96_000, &mut probe).unwrap();
    assert_eq!(n, 4096);
    assert_eq!(&probe, &payload[96_000..100_096]);
    // Then continue forward across new data.
    let mut fwd = vec![0u8; 8192];
    let n = r.read_at(150_000, &mut fwd).unwrap();
    assert_eq!(n, 8192);
    assert_eq!(&fwd, &payload[150_000..158_192]);
}

#[test]
fn backward_seek_is_correct() {
    let tmp = tempfile::tempdir().unwrap();
    let payload: Vec<u8> = (0..300_000u32).map(|i| (i % 233) as u8).collect();
    let vol = make_compressed(&tmp, &payload);
    let mut r = UnrarReader::new(&vol, "payload.bin", payload.len() as u64).unwrap();
    let mut buf = vec![0u8; 200_000];
    r.read_at(0, &mut buf).unwrap();
    let mut far = vec![0u8; 4096];
    let n = r.read_at(1024, &mut far).unwrap();
    assert_eq!(n, 4096);
    assert_eq!(&far, &payload[1024..5120]);
}
