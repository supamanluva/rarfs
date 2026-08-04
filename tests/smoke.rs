#[test]
fn lib_loads() {
    // Modules are added task by task; this just proves the crate builds and links.
    let _ = std::path::PathBuf::from("rarfs");
}
