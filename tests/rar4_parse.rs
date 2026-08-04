mod common;

fn payload() -> Vec<u8> {
    (0..80_000u32).map(|i| (i % 241) as u8).collect()
}

#[test]
fn parses_single_volume_store_member() {
    let tmp = tempfile::tempdir().unwrap();
    let pl = payload();
    let pf = tmp.path().join("payload.bin");
    std::fs::write(&pf, &pl).unwrap();
    let Some(vols) = common::build_rar4(tmp.path(), "one", &pf, true, None, false) else {
        eprintln!("SKIP: rar binary not available");
        return;
    };
    assert_eq!(vols.len(), 1);
    let members = rarfs::rarhdr::rar4::parse_volume(&vols[0]).unwrap();
    assert_eq!(members.len(), 1);
    let m = &members[0];
    assert_eq!(m.name, "payload.bin");
    assert_eq!(m.unpacked_size, pl.len() as u64);
    assert_eq!(m.method, rarfs::rarhdr::Method::Store);
    assert!(!m.split_before && !m.split_after);
    let bytes = std::fs::read(&vols[0]).unwrap();
    let s = &m.segment;
    assert_eq!(&bytes[s.data_offset as usize..(s.data_offset + s.data_len) as usize], &pl[..]);
}

#[test]
fn parses_multivolume_new_naming() {
    let tmp = tempfile::tempdir().unwrap();
    let pl = payload();
    let pf = tmp.path().join("payload.bin");
    std::fs::write(&pf, &pl).unwrap();
    let Some(vols) = common::build_rar4(tmp.path(), "mv", &pf, true, Some(20), false) else {
        eprintln!("SKIP: rar binary not available");
        return;
    };
    assert!(vols.len() >= 3, "expected several 20k volumes, got {}", vols.len());
    let mut joined = Vec::new();
    let mut first_flags = None;
    let mut last_flags = None;
    for (i, v) in vols.iter().enumerate() {
        let ms = rarfs::rarhdr::rar4::parse_volume(v).unwrap();
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].unpacked_size, pl.len() as u64); // total, not per-volume
        if i == 0 {
            first_flags = Some((ms[0].split_before, ms[0].split_after));
        }
        if i + 1 == vols.len() {
            last_flags = Some((ms[0].split_before, ms[0].split_after));
        }
        let bytes = std::fs::read(v).unwrap();
        let s = &ms[0].segment;
        joined.extend_from_slice(&bytes[s.data_offset as usize..(s.data_offset + s.data_len) as usize]);
    }
    assert_eq!(first_flags, Some((false, true)));
    assert_eq!(last_flags, Some((true, false)));
    assert_eq!(joined, pl);
}

#[test]
fn parses_multivolume_old_naming() {
    let tmp = tempfile::tempdir().unwrap();
    let pl = payload();
    let pf = tmp.path().join("payload.bin");
    std::fs::write(&pf, &pl).unwrap();
    let Some(vols) = common::build_rar4(tmp.path(), "old", &pf, true, Some(20), true) else {
        eprintln!("SKIP: rar binary not available");
        return;
    };
    assert!(vols[0].extension().unwrap() == "rar");
    assert!(vols[1].to_string_lossy().ends_with(".r00"));
    let mut joined = Vec::new();
    for v in &vols {
        let ms = rarfs::rarhdr::rar4::parse_volume(v).unwrap();
        let bytes = std::fs::read(v).unwrap();
        let s = &ms[0].segment;
        joined.extend_from_slice(&bytes[s.data_offset as usize..(s.data_offset + s.data_len) as usize]);
    }
    assert_eq!(joined, pl);
}

#[test]
fn decodes_unicode_names() {
    let tmp = tempfile::tempdir().unwrap();
    let pl = &payload()[..1000];
    let pf = tmp.path().join("tëst-fïle-文件.mkv");
    std::fs::write(&pf, pl).unwrap();
    let Some(vols) = common::build_rar4(tmp.path(), "uni", &pf, true, None, false) else {
        eprintln!("SKIP: rar binary not available");
        return;
    };
    let members = rarfs::rarhdr::rar4::parse_volume(&vols[0]).unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].name, "tëst-fïle-文件.mkv");
}

#[test]
fn parses_compressed_method_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let pl = payload();
    let pf = tmp.path().join("payload.bin");
    std::fs::write(&pf, &pl).unwrap();
    let Some(vols) = common::build_rar4(tmp.path(), "cmp", &pf, false, None, false) else {
        eprintln!("SKIP: rar binary not available");
        return;
    };
    let members = rarfs::rarhdr::rar4::parse_volume(&vols[0]).unwrap();
    assert!(matches!(members[0].method, rarfs::rarhdr::Method::Compressed(_)));
}
