use rarfs::volumes::{group_volumes, is_volume_name};

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn groups_old_style() {
    let sets = group_volumes(&names(&["a.rar", "a.r00", "a.r01", "a.nfo", "b.mkv"]));
    assert_eq!(sets, vec![names(&["a.rar", "a.r00", "a.r01"])]);
}

#[test]
fn groups_new_style_unpadded_and_padded() {
    let sets = group_volumes(&names(&["m.part1.rar", "m.part2.rar", "m.part10.rar"]));
    assert_eq!(sets, vec![names(&["m.part1.rar", "m.part2.rar", "m.part10.rar"])]);
    let sets = group_volumes(&names(&["n.part02.rar", "n.part01.rar", "n.part03.rar"]));
    assert_eq!(sets, vec![names(&["n.part01.rar", "n.part02.rar", "n.part03.rar"])]);
}

#[test]
fn single_rar_is_a_set_and_part_style_not_confused_with_plain() {
    let sets = group_volumes(&names(&["solo.rar", "x.part1.rar"]));
    assert!(sets.contains(&names(&["solo.rar"])));
    assert!(sets.contains(&names(&["x.part1.rar"])));
}

#[test]
fn missing_first_volume_yields_no_set() {
    let sets = group_volumes(&names(&["orphan.r03", "orphan2.part2.rar"]));
    assert!(sets.is_empty());
}

#[test]
fn old_style_continues_past_r99_into_s00() {
    let mut v = vec!["big.rar".to_string()];
    for i in 0..100 {
        v.push(format!("big.r{i:02}"));
    }
    v.push("big.s00".into());
    v.push("big.s01".into());
    let sets = group_volumes(&v);
    assert_eq!(sets.len(), 1);
    assert_eq!(sets[0].len(), 103);
    assert_eq!(sets[0][101], "big.s00");
}

#[test]
fn is_volume_name_classification() {
    assert!(is_volume_name("a.rar"));
    assert!(is_volume_name("a.r00"));
    assert!(is_volume_name("a.s17"));
    assert!(is_volume_name("a.part01.rar"));
    assert!(!is_volume_name("a.mkv"));
    assert!(!is_volume_name("a.nfo"));
    assert!(!is_volume_name("rari.png")); // no false positives
}

#[test]
fn case_insensitive_extensions() {
    let sets = group_volumes(&names(&["A.RAR", "A.R00"]));
    assert_eq!(sets, vec![names(&["A.RAR", "A.R00"])]);
}

#[test]
fn non_ascii_names_do_not_panic() {
    assert!(!is_volume_name("éé.tx"));
    assert!(!is_volume_name("abé.cd"));
    // U+212A (KELVIN SIGN) lowercases to 'k' with a different byte length.
    assert!(is_volume_name("\u{212A}.part1.rar"));
}

#[test]
fn groups_unicode_base_names() {
    let sets = group_volumes(&names(&["映画.part1.rar", "映画.part2.rar"]));
    assert_eq!(sets, vec![names(&["映画.part1.rar", "映画.part2.rar"])]);
}
