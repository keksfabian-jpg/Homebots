//! Windows gives the main thread a 1 MB stack (Linux 8 MB). The level build runs on the main
//! thread (it holds non-Send resources) with large stack frames: a match crashed on the user's PC
//! with STATUS_STACK_BUFFER_OVERRUN (Rust's abort, e.g. after a stack overflow). 32 MB as on
//! the worker threads' scale of things.
fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("windows-msvc") {
        println!("cargo:rustc-link-arg-bins=/STACK:33554432");
    } else if target.contains("windows-gnu") {
        println!("cargo:rustc-link-arg-bins=-Wl,--stack,33554432");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
