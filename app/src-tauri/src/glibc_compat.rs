//! Linux: keeps the binary runnable on older glibc (Debian 12 / MX Linux 23 have 2.36).
//!
//! glibc 2.43 gave `acosf` and `atan2f` new symbol versions (correctly rounded), so a binary
//! linked on a newer distribution needs GLIBC_2.43 for them and does not start elsewhere.
//! `build.rs` links with `--wrap=acosf,--wrap=atan2f`: every call (ours and the crates')
//! goes to these pure-Rust versions instead. `build-linux.sh` checks the glibc a build needs.

#[unsafe(no_mangle)]
pub extern "C" fn __wrap_acosf(x: f32) -> f32 {
    libm::acosf(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn __wrap_atan2f(y: f32, x: f32) -> f32 {
    libm::atan2f(y, x)
}
