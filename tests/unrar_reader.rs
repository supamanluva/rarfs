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
fn backward_seek_before_window_start_restarts_decode() {
    let tmp = tempfile::tempdir().unwrap();
    let payload: Vec<u8> = (0..50_000u32).map(|i| (i % 241) as u8).collect();
    let vol = make_compressed(&tmp, &payload);
    let mut r = UnrarReader::new(&vol, "payload.bin", payload.len() as u64).unwrap();
    // Reading at 10_000 drains everything before it: dropped advances to 10_000.
    let mut buf = vec![0u8; 4096];
    let n = r.read_at(10_000, &mut buf).unwrap();
    assert_eq!(n, 4096);
    assert_eq!(&buf, &payload[10_000..14_096]);
    // 2_000 < dropped (10_000): data already fell out of the window, so the
    // decoder must restart from the beginning of the archive.
    let n = r.read_at(2_000, &mut buf).unwrap();
    assert_eq!(n, 4096);
    assert_eq!(&buf, &payload[2_000..6_096]);
    // Forward reads still work after the restart.
    let n = r.read_at(20_000, &mut buf).unwrap();
    assert_eq!(n, 4096);
    assert_eq!(&buf, &payload[20_000..24_096]);
}

#[test]
fn streaming_larger_than_window_exercises_backpressure() {
    let tmp = tempfile::tempdir().unwrap();
    // 10 MiB > 8 MiB window: the producer must block when the window is full
    // and resume as the consumer drains it.
    let payload: Vec<u8> = (0..10 * 1024 * 1024u64).map(|i| (i % 251) as u8).collect();
    let vol = make_compressed(&tmp, &payload);
    let mut r = UnrarReader::new(&vol, "payload.bin", payload.len() as u64).unwrap();
    let mut got = vec![0u8; payload.len()];
    let mut off = 0usize;
    while off < got.len() {
        let end = (off + 1024 * 1024).min(got.len());
        let n = r.read_at(off as u64, &mut got[off..end]).unwrap();
        assert!(n > 0);
        off += n;
    }
    assert_eq!(off, payload.len());
    assert_eq!(got, payload);
}

#[test]
fn forward_seek_past_window_frontier_does_not_deadlock() {
    let tmp = tempfile::tempdir().unwrap();
    // 12 MiB > 8 MiB window: a first read at 10 MiB requests data the decoder
    // has not produced yet while the window is already full. Without the
    // drain-past-frontier logic the producer blocks on the full window and
    // this read never returns (deadlock).
    let mib = 1024 * 1024usize;
    let payload: Vec<u8> = (0..12 * mib as u64).map(|i| (i % 251) as u8).collect();
    let vol = make_compressed(&tmp, &payload);
    let mut r = UnrarReader::new(&vol, "payload.bin", payload.len() as u64).unwrap();
    let mut buf = vec![0u8; 4096];
    let n = r.read_at(10 * mib as u64, &mut buf).unwrap();
    assert_eq!(n, 4096);
    assert_eq!(&buf, &payload[10 * mib..10 * mib + 4096]);
    // Reads after the seek still work.
    let n = r.read_at(11 * mib as u64, &mut buf).unwrap();
    assert_eq!(n, 4096);
    assert_eq!(&buf, &payload[11 * mib..11 * mib + 4096]);
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
