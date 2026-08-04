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
