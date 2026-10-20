//! Linux: keeps the add-on loadable on older glibc (Debian 12 / MX Linux 23 have 2.36), like
//! `app/src-tauri/src/glibc_compat.rs`.
//!
//! Linked on a new distribution, the add-on (mostly ONNX Runtime) needs glibc 2.43 for float
//! maths that got new symbol versions there, and 2.38 for the C23 `strtol` family that new
//! headers redirect to. `build.rs` links with `--wrap=<name>`, so every call goes to these:
//! pure-Rust maths (`libm`) and the classic `strtol` functions (the C23 ones only add `0b`
//! prefixes, which ONNX Runtime does not parse).

use std::ffi::{c_char, c_int, c_long, c_longlong, c_ulong, c_ulonglong};

#[unsafe(no_mangle)]
pub extern "C" fn __wrap_acosf(x: f32) -> f32 {
    libm::acosf(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn __wrap_asinf(x: f32) -> f32 {
    libm::asinf(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn __wrap_sinhf(x: f32) -> f32 {
    libm::sinhf(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn __wrap_coshf(x: f32) -> f32 {
    libm::coshf(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn __wrap_acoshf(x: f32) -> f32 {
    libm::acoshf(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn __wrap_atanhf(x: f32) -> f32 {
    libm::atanhf(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn __wrap_remainderf(x: f32, y: f32) -> f32 {
    libm::remainderf(x, y)
}

unsafe extern "C" {
    fn strtol(s: *const c_char, end: *mut *mut c_char, base: c_int) -> c_long;
    fn strtoul(s: *const c_char, end: *mut *mut c_char, base: c_int) -> c_ulong;
    fn strtoll(s: *const c_char, end: *mut *mut c_char, base: c_int) -> c_longlong;
    fn strtoull(s: *const c_char, end: *mut *mut c_char, base: c_int) -> c_ulonglong;
}

/// # Safety
/// As C's `strtol`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap___isoc23_strtol(
    s: *const c_char,
    end: *mut *mut c_char,
    base: c_int,
) -> c_long {
    unsafe { strtol(s, end, base) }
}

/// # Safety
/// As C's `strtoul`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap___isoc23_strtoul(
    s: *const c_char,
    end: *mut *mut c_char,
    base: c_int,
) -> c_ulong {
    unsafe { strtoul(s, end, base) }
}

/// # Safety
/// As C's `strtoll`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap___isoc23_strtoll(
    s: *const c_char,
    end: *mut *mut c_char,
    base: c_int,
) -> c_longlong {
    unsafe { strtoll(s, end, base) }
}

/// # Safety
/// As C's `strtoull`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap___isoc23_strtoull(
    s: *const c_char,
    end: *mut *mut c_char,
    base: c_int,
) -> c_ulonglong {
    unsafe { strtoull(s, end, base) }
}
