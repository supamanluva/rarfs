mod common;

use std::io::Read;

#[test]
fn mounted_store_member_reads_like_the_original() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let mnt = tmp.path().join("mnt");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&mnt).unwrap();
    let payload: Vec<u8> = (0..120_000u32).map(|i| (i % 251) as u8).collect();
    common::build_rar5(&src, "film", "Film.2024.1080p.mkv", &payload, 5);
    std::fs::write(src.join("note.txt"), b"pass through me").unwrap();

    let fs = rarfs::fs::RarFs::new(src.clone());
    let session = fuser::spawn_mount2(
        fs,
        &mnt,
        &[fuser::MountOption::RO, fuser::MountOption::FSName("rarfs-test".into())],
    )
    .expect("mount failed — is /dev/fuse available?");

    // readdir sees the member and the passthrough file
    let names: Vec<String> = std::fs::read_dir(&mnt)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.contains(&"Film.2024.1080p.mkv".to_string()));
    assert!(names.contains(&"film.part1.rar".to_string()));
    assert!(names.contains(&"note.txt".to_string()));

    // full read through the mount equals the payload
    let mut got = Vec::new();
    std::fs::File::open(mnt.join("Film.2024.1080p.mkv"))
        .unwrap()
        .read_to_end(&mut got)
        .unwrap();
    assert_eq!(got, payload);

    // random seek read through the mount
    use std::os::unix::fs::FileExt;
    let f = std::fs::File::open(mnt.join("Film.2024.1080p.mkv")).unwrap();
    let mut buf = [0u8; 4096];
    f.read_exact_at(&mut buf, 55_555).unwrap();
    assert_eq!(&buf, &payload[55_555..55_555 + 4096]);
    let meta = f.metadata().unwrap();
    assert_eq!(meta.len(), payload.len() as u64);

    // passthrough works
    assert_eq!(std::fs::read_to_string(mnt.join("note.txt")).unwrap(), "pass through me");

    drop(session); // unmounts
    // After unmount the mountpoint is empty again.
    assert!(std::fs::read_dir(&mnt).unwrap().next().is_none());
}
