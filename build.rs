//! Build script: a large main-thread stack for the desktop binary on Windows. The default
//! 1 MB is little for the geometry kernel's recursion, which runs on the main thread when a
//! command's OK builds the feature (previews run on 64 MB threads already); a stack overflow
//! aborts the process without a crash report.

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.ends_with("windows-msvc") {
        println!("cargo:rustc-link-arg-bins=/STACK:67108864");
    } else if target.ends_with("windows-gnu") {
        println!("cargo:rustc-link-arg-bins=-Wl,--stack,67108864");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
