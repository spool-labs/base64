//! Build Firedancer's base64, vendored rather than reimplemented, for a row

fn main() {
    println!("cargo:rerun-if-changed=firedancer/fd_base64.c");
    println!("cargo:rerun-if-changed=firedancer/fd_shim.h");
    cc::Build::new()
        .file("firedancer/fd_base64.c")
        .include("firedancer")
        .opt_level(3)
        .flag_if_supported("-march=native")
        .compile("fd_base64");
}
