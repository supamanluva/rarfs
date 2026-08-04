mod common;

#[test]
fn lists_and_counts_headers_of_compressed_archive() {
    let tmp = tempfile::tempdir().unwrap();
    let payload: Vec<u8> = (0..30_000u32).map(|i| (i % 241) as u8).collect();
    let pf = tmp.path().join("payload.bin");
    std::fs::write(&pf, &payload).unwrap();
    let Some(vols) = common::build_rar4(tmp.path(), "ffi", &pf, false, None, false) else {
        eprintln!("SKIP: rar binary not available");
        return;
    };
    let a = rarfs::unrar_ffi::UnrarArchive::open(&vols[0]).expect("open failed");
    let mut buf = vec![0u8; 1024];
    let n = a.read_next_name(&mut buf).unwrap();
    assert_eq!(&buf[..n], b"payload.bin");
    let err = a.read_next_name(&mut buf).unwrap_err();
    assert_eq!(err, rarfs::unrar_ffi::ERAR_END_ARCHIVE);
}
