use std::collections::HashMap;

/// Strip an ASCII suffix case-insensitively; all slicing stays on the
/// original string, so no index can land inside a multi-byte char.
fn strip_suffix_ascii_ci<'a>(s: &'a str, suffix: &str) -> Option<&'a str> {
    let tail = s.get(s.len().checked_sub(suffix.len())?..)?;
    if tail.eq_ignore_ascii_case(suffix) {
        Some(&s[..s.len() - suffix.len()])
    } else {
        None
    }
}

/// Split "name.partNN.rar" (case-insensitive) into (base, number).
fn parse_part_style(name: &str) -> Option<(String, u64)> {
    let stem = strip_suffix_ascii_ci(name, ".rar")?;
    let (base, tail) = stem.rsplit_once('.')?;
    let digits = tail.get(4..)?;
    if !tail[..4].eq_ignore_ascii_case("part") {
        return None;
    }
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // Keep original casing of the base for filesystem use.
    Some((base.to_string(), digits.parse().ok()?))
}

/// Split "name.rar" / "name.rNN" / "name.sNN" … into (base, sequence index)
/// where the plain .rar first volume has index 0, .r00 has index 1, …
/// letters r..z each cover 100 followers.
fn parse_old_style(name: &str) -> Option<(String, u64)> {
    if let Some(base) = strip_suffix_ascii_ci(name, ".rar") {
        return Some((base.to_string(), 0));
    }
    let (base, ext) = name.rsplit_once('.')?;
    let b = ext.as_bytes();
    if b.len() != 3 {
        return None;
    }
    let letter = b[0].to_ascii_lowercase();
    if !(b'r'..=b'z').contains(&letter) {
        return None;
    }
    if !b[1].is_ascii_digit() || !b[2].is_ascii_digit() {
        return None;
    }
    let nn = (b[1] - b'0') as u64 * 10 + (b[2] - b'0') as u64;
    let idx = (letter - b'r') as u64 * 100 + nn + 1;
    Some((base.to_string(), idx))
}

pub fn is_volume_name(name: &str) -> bool {
    parse_part_style(name).is_some() || parse_old_style(name).is_some()
}

pub fn group_volumes(names: &[String]) -> Vec<Vec<String>> {
    let mut part_sets: HashMap<String, Vec<(u64, String)>> = HashMap::new();
    let mut old_sets: HashMap<String, Vec<(u64, String)>> = HashMap::new();
    for n in names {
        if let Some((base, num)) = parse_part_style(n) {
            part_sets.entry(base).or_default().push((num, n.clone()));
        } else if let Some((base, idx)) = parse_old_style(n) {
            old_sets.entry(base).or_default().push((idx, n.clone()));
        }
    }
    let mut out = Vec::new();
    for (_, mut v) in part_sets {
        v.sort_by_key(|(num, _)| *num);
        v.dedup_by_key(|(num, _)| *num);
        if v.first().map(|(num, _)| *num) != Some(1) {
            continue; // missing first volume
        }
        // Part numbers are explicit, so keep every volume in numeric order;
        // the reader validates contents as it goes.
        out.push(v.into_iter().map(|(_, name)| name).collect());
    }
    for (_, mut v) in old_sets {
        v.sort_by_key(|(idx, _)| *idx);
        v.dedup_by_key(|(idx, _)| *idx);
        if v.first().map(|(idx, _)| *idx) != Some(0) {
            continue; // no .rar first volume
        }
        let mut set = vec![v[0].1.clone()];
        for (i, (idx, name)) in v.iter().enumerate().skip(1) {
            if *idx != i as u64 {
                break;
            }
            set.push(name.clone());
        }
        out.push(set);
    }
    out.sort_by(|a, b| a[0].cmp(&b[0]));
    out
}
