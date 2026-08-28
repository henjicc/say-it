fn main() {
    build_sdk_runtime_bridge();
    build_macos_native_bridge();
    copy_sherpa_runtime_for_cargo_tests();
    tauri_build::build()
}

fn build_sdk_runtime_bridge() {
    let out_dir = std::env::var("OUT_DIR").expect("Cargo 缺少 OUT_DIR");
    let output = std::path::Path::new(&out_dir).join("sayit-ai-sdk-runtime");
    let status = std::process::Command::new("node")
        .arg("../scripts/build-ai-sdk-runtime.mjs")
        .arg(&output)
        .status()
        .expect("无法启动 Node 构建 SDK Runtime bridge");
    if !status.success() {
        panic!("SDK Runtime bridge 构建失败");
    }
    println!("cargo:rerun-if-changed=../sdk-runtime/web-compat.ts");
    println!("cargo:rerun-if-changed=../sdk-runtime/host-adapter.ts");
    println!("cargo:rerun-if-changed=../sdk-runtime/capability-entry.ts");
    println!("cargo:rerun-if-changed=../sdk-runtime/groq-entry.ts");
    println!("cargo:rerun-if-changed=../sdk-runtime/groq-text-only-preprocess.ts");
    println!("cargo:rerun-if-changed=../sdk-runtime/llm-modules-entry.ts");
    println!("cargo:rerun-if-changed=../sdk-runtime/sdk-bootstrap.ts");
    println!("cargo:rerun-if-changed=../scripts/build-ai-sdk-runtime.mjs");
    println!("cargo:rerun-if-changed=../package.json");
    println!("cargo:rerun-if-changed=../package-lock.json");
}

fn build_macos_native_bridge() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    cc::Build::new()
        .file("macos/SayItMacBridge.m")
        .flag("-fobjc-arc")
        .flag("-fblocks")
        .warnings(true)
        .compile("sayit_macos_bridge");
    for framework in [
        "AppKit",
        "ApplicationServices",
        "AudioToolbox",
        "AVFoundation",
        "CoreAudio",
        "CoreGraphics",
        "CoreMedia",
        "Foundation",
        "IOKit",
        "ScreenCaptureKit",
        "Vision",
    ] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
    println!("cargo:rerun-if-changed=macos/SayItMacBridge.m");
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
