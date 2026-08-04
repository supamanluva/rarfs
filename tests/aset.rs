mod common;

#[test]
fn assembles_rar5_multivolume_store() {
    let tmp = tempfile::tempdir().unwrap();
    let payload: Vec<u8> = (0..50_000u32).map(|i| (i % 239) as u8).collect();
    let vols = common::build_rar5(tmp.path(), "film", "film.2024.1080p.mkv", &payload, 4);
    let members = rarfs::aset::parse_set(&vols).unwrap();
    assert_eq!(members.len(), 1);
    let m = &members[0];
    assert_eq!(m.name, "film.2024.1080p.mkv");
    assert_eq!(m.size, payload.len() as u64);
    assert_eq!(m.method, rarfs::rarhdr::Method::Store);
    assert_eq!(m.segments.len(), 4);
    let total: u64 = m.segments.iter().map(|s| s.data_len).sum();
    assert_eq!(total, payload.len() as u64);
}

#[test]
fn assembles_rar4_multivolume_store() {
    let tmp = tempfile::tempdir().unwrap();
    let payload: Vec<u8> = (0..60_000u32).map(|i| (i % 233) as u8).collect();
    let pf = tmp.path().join("payload.bin");
    std::fs::write(&pf, &payload).unwrap();
    let Some(vols) = common::build_rar4(tmp.path(), "m4", &pf, true, Some(20), false) else {
        eprintln!("SKIP: rar binary not available");
        return;
    };
    let members = rarfs::aset::parse_set(&vols).unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].segments.len(), vols.len());
    let total: u64 = members[0].segments.iter().map(|s| s.data_len).sum();
    assert_eq!(total, payload.len() as u64);
}

#[test]
fn drops_incomplete_member_when_last_volume_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let payload: Vec<u8> = (0..30_000u32).map(|i| (i % 229) as u8).collect();
    let vols = common::build_rar5(tmp.path(), "cut", "cut.mkv", &payload, 3);
    let members = rarfs::aset::parse_set(&vols[..2]).unwrap();
    assert!(members.is_empty());
}

#[test]
fn format_detection_rejects_garbage() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("junk.rar");
    std::fs::write(&p, b"not a rar at all").unwrap();
    assert!(rarfs::aset::parse_set(&[p]).is_err());
}

#[test]
fn drops_store_member_when_middle_volume_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let payload: Vec<u8> = (0..30_000u32).map(|i| (i % 227) as u8).collect();
    let vols = common::build_rar5(tmp.path(), "gap", "gap.mkv", &payload, 3);
    std::fs::remove_file(&vols[1]).unwrap();
    let remaining = vec![vols[0].clone(), vols[2].clone()];
    let members = rarfs::aset::parse_set(&remaining).unwrap();
    assert!(members.is_empty());
}
