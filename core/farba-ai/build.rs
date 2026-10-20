fn main() {
    // Linux: older glibc. These come from src/glibc_compat.rs (see there).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        let wrapped = [
            "acosf",
            "asinf",
            "sinhf",
            "coshf",
            "acoshf",
            "atanhf",
            "remainderf",
            "__isoc23_strtol",
            "__isoc23_strtoul",
            "__isoc23_strtoll",
            "__isoc23_strtoull",
        ];
        let args: Vec<String> = wrapped.iter().map(|f| format!("--wrap={f}")).collect();
        println!("cargo:rustc-cdylib-link-arg=-Wl,{}", args.join(","));
        // ONNX Runtime is C++: the C++ library goes into the add-on, so it loads on systems
        // with an older libstdc++ (built with ORT_CXX_STDLIB="", see build-addons.mjs).
        println!("cargo:rerun-if-env-changed=ORT_CXX_STDLIB");
        if std::env::var("ORT_CXX_STDLIB").is_ok_and(|v| v.is_empty()) {
            println!("cargo:rustc-cdylib-link-arg=-Wl,-Bstatic,-lstdc++,-Bdynamic");
        }
    }
}
