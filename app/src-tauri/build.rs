fn main() {
    // The icons are embedded in the exe (resource) and in the window at compile time:
    // rebuild when they change (tauri-build does not watch them).
    println!("cargo:rerun-if-changed=icons");
    // Linux: the (unused, mobile-only) cdylib links rav1d's assembly, whose PC-relative
    // references to rav1d's exported Rust tables fail in a shared object unless they bind
    // locally.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-cdylib-link-arg=-Wl,-Bsymbolic");
        // Older glibc: these come from src/glibc_compat.rs.
        println!("cargo:rustc-link-arg=-Wl,--wrap=acosf,--wrap=atan2f");
    }
    tauri_build::build()
}
