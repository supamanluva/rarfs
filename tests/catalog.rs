mod common;

use rarfs::catalog::{Catalog, Node};

fn build_tree(tmp: &tempfile::TempDir) -> (Vec<u8>, Vec<u8>) {
    let payload_a: Vec<u8> = (0..40_000u32).map(|i| (i % 211) as u8).collect();
    let payload_b: Vec<u8> = (0..25_000u32).map(|i| (i % 199) as u8).collect();
    common::build_rar5(tmp.path(), "movie", "Movie.2024.1080p.mkv", &payload_a, 3);
    let sub = tmp.path().join("shows");
    std::fs::create_dir(&sub).unwrap();
    common::build_rar5(&sub, "ep", "Show.S01E01.1080p.mkv", &payload_b, 2);
    std::fs::write(tmp.path().join("readme.nfo"), b"hello").unwrap();
    (payload_a, payload_b)
}

#[test]
fn lists_members_and_passthrough() {
    let tmp = tempfile::tempdir().unwrap();
    build_tree(&tmp);
    let cat = Catalog::new(tmp.path().to_path_buf());
    let entries = cat.list(std::path::Path::new("")).unwrap();
    let get = |n: &str| entries.iter().find(|(name, _)| name == n).map(|(_, node)| node);
    // volumes pass through
    assert!(matches!(get("movie.part1.rar"), Some(Node::Passthrough(_))));
    assert!(matches!(get("readme.nfo"), Some(Node::Passthrough(_))));
    // member is exposed with correct size
    match get("Movie.2024.1080p.mkv") {
        Some(Node::Member(m)) => assert_eq!(m.size, 40_000),
        other => panic!("expected member, got {:?}", other.is_some()),
    }
    assert!(matches!(get("shows"), Some(Node::Dir)));
    // nested dir
    let sub = cat.list(std::path::Path::new("shows")).unwrap();
    assert!(sub.iter().any(|(n, _)| n == "Show.S01E01.1080p.mkv"));
    // lookup by path
    let n = cat.lookup(std::path::Path::new("shows/Show.S01E01.1080p.mkv")).unwrap();
    assert!(matches!(n, Some(Node::Member(_))));
    assert!(cat.lookup(std::path::Path::new("nope.mkv")).unwrap().is_none());
}

#[test]
fn unparseable_set_is_hidden_not_fatal() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("broken.rar"), b"garbage").unwrap();
    std::fs::write(tmp.path().join("plain.txt"), b"x").unwrap();
    let cat = Catalog::new(tmp.path().to_path_buf());
    let entries = cat.list(std::path::Path::new("")).unwrap();
    assert!(entries.iter().any(|(n, _)| n == "plain.txt"));
    assert!(entries.iter().any(|(n, _)| n == "broken.rar")); // the volume itself passes through
    assert!(!entries.iter().any(|(n, _)| n == "garbage"));
}

#[test]
fn new_archives_appear_on_rescan() {
    let tmp = tempfile::tempdir().unwrap();
    let cat = Catalog::new(tmp.path().to_path_buf());
    assert!(cat.list(std::path::Path::new("")).unwrap().is_empty());
    let payload: Vec<u8> = (0..5_000u32).map(|i| (i % 197) as u8).collect();
    std::thread::sleep(std::time::Duration::from_millis(1100)); // mtime granularity
    common::build_rar5(tmp.path(), "late", "late.mkv", &payload, 1);
    let entries = cat.list(std::path::Path::new("")).unwrap();
    assert!(entries.iter().any(|(n, _)| n == "late.mkv"));
}
