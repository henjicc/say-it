fn main() {
    copy_sherpa_runtime_for_cargo_tests();
    tauri_build::build()
}

fn copy_sherpa_runtime_for_cargo_tests() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let Ok(out_dir) = std::env::var("OUT_DIR") else {
        return;
    };
    let Ok(profile) = std::env::var("PROFILE") else {
        return;
    };
    let Some(profile_dir) = std::path::Path::new(&out_dir)
        .ancestors()
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name == std::ffi::OsStr::new(&profile))
        })
    else {
        return;
    };
    let deps_dir = profile_dir.join("deps");
    if std::fs::create_dir_all(&deps_dir).is_err() {
        return;
    }
    for name in [
        "onnxruntime.dll",
        "onnxruntime_providers_shared.dll",
        "sherpa-onnx-c-api.dll",
        "sherpa-onnx-cxx-api.dll",
    ] {
        let source = profile_dir.join(name);
        if source.is_file() {
            let _ = std::fs::copy(source, deps_dir.join(name));
        }
    }
}
