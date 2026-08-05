use std::path::Path;

fn main() {
    let src = Path::new("vendor/unrarsrc");
    if !src.join("dll.hpp").exists() {
        panic!(
            "vendor/unrarsrc is missing. The unrar source is not redistributed in this \
             repository — fetch it once after cloning with:\n\n    \
             scripts/fetch-unrarsrc.sh\n"
        );
    }
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .define("RARDLL", None)
        .define("SILENT", None)
        // Defines from unrarsrc's makefile (dll target); _UNIX is needed
        // because the shim includes dll.hpp standalone, without raros.hpp.
        .define("_UNIX", None)
        .define("_FILE_OFFSET_BITS", "64")
        .define("_LARGEFILE_SOURCE", None)
        .define("RAR_SMP", None)
        .flag_if_supported("-w")
        .include(src)
        .include("vendor/shim")
        .file("vendor/shim/shim.cpp");
    // Object list from unrarsrc's makefile dll target (OBJECTS minus rar.o,
    // plus LIB_OBJ). Many .cpp files are unity-included by these (e.g.
    // recvol.cpp #includes recvol3.cpp) or are Windows-only (motw.cpp), so
    // they must not be compiled standalone.
    const OBJECTS: &[&str] = &[
        "strlist", "strfn", "pathfn", "smallfn", "global", "file", "filefn",
        "filcreat", "archive", "arcread", "unicode", "system", "crypt", "crc",
        "rawread", "encname", "resource", "match", "timefn", "rdwrfn",
        "consio", "options", "errhnd", "rarvm", "secpassword", "rijndael",
        "getbits", "sha1", "sha256", "blake2s", "hash", "extinfo", "extract",
        "volume", "list", "find", "unpack", "headers", "threadpool", "rs16",
        "cmddata", "ui", "largepage", "filestr", "scantree", "dll", "qopen",
    ];
    for obj in OBJECTS {
        build.file(src.join(format!("{obj}.cpp")));
    }
    build.compile("rarfs_unrar");
    println!("cargo:rerun-if-changed=vendor/shim/shim.cpp");
    println!("cargo:rerun-if-changed=vendor/shim/shim.h");
}
