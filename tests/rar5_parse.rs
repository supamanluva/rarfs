mod common;

#[test]
fn parses_single_volume_store_member() {
    let tmp = tempfile::tempdir().unwrap();
    let payload: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
    let vols = common::build_rar5(tmp.path(), "movie", "movie.mkv", &payload, 1);

    let members = rarfs::rarhdr::rar5::parse_volume(&vols[0]).unwrap();
    assert_eq!(members.len(), 1);
    let m = &members[0];
    assert_eq!(m.name, "movie.mkv");
    assert_eq!(m.unpacked_size, payload.len() as u64);
    assert_eq!(m.method, rarfs::rarhdr::Method::Store);
    assert_eq!(m.mtime, None); // fixture headers carry no mtime field
    assert!(!m.split_before && !m.split_after);
    assert_eq!(m.segment.volume, vols[0]);
    assert_eq!(m.segment.data_len, payload.len() as u64);
    // The data segment must be the payload itself (store mode).
    let bytes = std::fs::read(&vols[0]).unwrap();
    let seg = &bytes[m.segment.data_offset as usize..(m.segment.data_offset + m.segment.data_len) as usize];
    assert_eq!(seg, &payload[..]);
}

#[test]
fn parses_multivolume_split_flags() {
    let tmp = tempfile::tempdir().unwrap();
    let payload: Vec<u8> = (0..30_000u32).map(|i| (i % 253) as u8).collect();
    let vols = common::build_rar5(tmp.path(), "show", "show.s01e01.mkv", &payload, 3);

    let m0 = rarfs::rarhdr::rar5::parse_volume(&vols[0]).unwrap();
    let m1 = rarfs::rarhdr::rar5::parse_volume(&vols[1]).unwrap();
    let m2 = rarfs::rarhdr::rar5::parse_volume(&vols[2]).unwrap();
    assert_eq!(m0.len(), 1);
    assert!(!m0[0].split_before && m0[0].split_after);
    assert!(m1[0].split_before && m1[0].split_after);
    assert!(m2[0].split_before && !m2[0].split_after);
    // Concatenated segments reconstruct the payload exactly.
    let mut joined = Vec::new();
    for m in [m0, m1, m2] {
        let v = std::fs::read(&m[0].segment.volume).unwrap();
        let s = &m[0].segment;
        joined.extend_from_slice(&v[s.data_offset as usize..(s.data_offset + s.data_len) as usize]);
    }
    assert_eq!(joined, payload);
}

#[test]
fn parses_mtime_when_present() {
    let tmp = tempfile::tempdir().unwrap();
    let payload: Vec<u8> = (0..1_000u32).map(|i| (i % 251) as u8).collect();
    let mut body = Vec::new();
    common::push_vint(&mut body, 0x0002 | 0x0004); // FILE_FLAGS: mtime + crc32 present
    common::push_vint(&mut body, payload.len() as u64); // unp size
    common::push_vint(&mut body, 0o100644); // ATTR
    body.extend_from_slice(&1_700_000_000u32.to_le_bytes()); // mtime (unix seconds)
    body.extend_from_slice(&0x12345678u32.to_le_bytes()); // crc32
    common::push_vint(&mut body, 0); // COMP_INFO: store
    common::push_vint(&mut body, 1); // HOST_OS: unix
    common::push_vint(&mut body, 4); // name len
    body.extend_from_slice(b"a.mkv");
    let mut vol = b"Rar!\x1a\x07\x01\x00".to_vec();
    vol.extend_from_slice(&common::main_hdr5(false, None));
    vol.extend_from_slice(&common::block5(2, 0, &[], &body, &payload));
    vol.extend_from_slice(&common::endarc5());
    let p = tmp.path().join("m.rar");
    std::fs::write(&p, &vol).unwrap();

    let members = rarfs::rarhdr::rar5::parse_volume(&p).unwrap();
    assert_eq!(members.len(), 1);
    let expect = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
    assert_eq!(members[0].mtime, Some(expect));
}

#[test]
fn truncated_mtime_field_returns_err_not_panic() {
    let tmp = tempfile::tempdir().unwrap();
    // File header whose FILE_FLAGS set the mtime-present bit (0x0002) but
    // whose body ends right after ATTR — the 4-byte mtime field is missing.
    let mut body = Vec::new();
    common::push_vint(&mut body, 0x0002); // FILE_FLAGS: mtime present
    common::push_vint(&mut body, 1234); // unp size
    common::push_vint(&mut body, 0o100644); // ATTR — body truncated here
    let mut vol = b"Rar!\x1a\x07\x01\x00".to_vec();
    vol.extend_from_slice(&common::main_hdr5(false, None));
    vol.extend_from_slice(&common::block5(2, 0, &[], &body, &[]));
    let p = tmp.path().join("bad.rar");
    std::fs::write(&p, &vol).unwrap();
    assert!(rarfs::rarhdr::rar5::parse_volume(&p).is_err());
}

#[test]
fn oversized_head_size_returns_err_not_oom() {
    let tmp = tempfile::tempdir().unwrap();
    // A block advertising a ~4 GiB HEAD_SIZE must be rejected before any
    // allocation is attempted.
    let mut vol = b"Rar!\x1a\x07\x01\x00".to_vec();
    vol.extend_from_slice(&common::main_hdr5(false, None));
    vol.extend_from_slice(&[0, 0, 0, 0]); // HEAD_CRC (not validated)
    common::push_vint(&mut vol, 0xFFFF_FFFF); // HEAD_SIZE
    let p = tmp.path().join("huge.rar");
    std::fs::write(&p, &vol).unwrap();
    assert!(rarfs::rarhdr::rar5::parse_volume(&p).is_err());
}
