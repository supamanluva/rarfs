#[test]
fn help_lists_required_args() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rarfs"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("SOURCE_DIR"));
    assert!(text.contains("MOUNTPOINT"));
    assert!(text.contains("--log"));
    assert!(text.contains("--allow-other"));
}

#[test]
fn missing_args_fail_with_usage() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rarfs"))
        .output()
        .unwrap();
    assert!(!out.status.success());
}
