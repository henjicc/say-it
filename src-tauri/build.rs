fn main() {
    build_sdk_runtime_bridge();
    #[cfg(target_os = "macos")]
    build_macos_native_bridge();
    distribute_sherpa_runtime_libs();
    tauri_build::build()
}

/// 增删运行库时同步 tauri.macos.conf.json 的 frameworks 与 nsis-hooks.nsh 的复制和删除；
/// Windows 的打包资源按通配符取 target/runtime-libs 下全部 DLL。
const WINDOWS_RUNTIME_LIBS: [&str; 4] = [
    "onnxruntime.dll",
    "onnxruntime_providers_shared.dll",
    "sherpa-onnx-c-api.dll",
    "sherpa-onnx-cxx-api.dll",
];
const MACOS_RUNTIME_LIBS: [&str; 2] = ["libsherpa-onnx-c-api.dylib", "libonnxruntime.dylib"];

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
    println!("cargo:rerun-if-changed=../sdk-runtime/groq-endpoint-identity.ts");
    println!("cargo:rerun-if-changed=../sdk-runtime/llm-modules-entry.ts");
    println!("cargo:rerun-if-changed=../sdk-runtime/sdk-bootstrap.ts");
    println!("cargo:rerun-if-changed=../scripts/build-ai-sdk-runtime.mjs");
    println!("cargo:rerun-if-changed=../package.json");
    println!("cargo:rerun-if-changed=../package-lock.json");
}

#[cfg(target_os = "macos")]
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

/// sherpa-onnx-sys（直接依赖且声明了 `links`，构建脚本先于本脚本运行）把共享运行库复制到当前 profile
/// 的输出目录，这里再分发两处：
/// - `target/runtime-libs/`：打包配置引用的固定位置。tauri-build 在每个 profile 都会复制打包
///   资源与 frameworks，配置若引用 `target/release`，dev、测试和 cargo check 都得先打过一次包。
/// - Windows 的 `deps/`：cargo test 的测试程序在这里运行，需要旁边就有 DLL。
fn distribute_sherpa_runtime_libs() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let names: &[&str] = match target_os.as_str() {
        "windows" => &WINDOWS_RUNTIME_LIBS,
        "macos" => &MACOS_RUNTIME_LIBS,
        _ => return,
    };
    let out_dir = std::env::var("OUT_DIR").expect("Cargo 缺少 OUT_DIR");
    let profile = std::env::var("PROFILE").expect("Cargo 缺少 PROFILE");
    let profile_dir = std::path::Path::new(&out_dir)
        .ancestors()
        .find(|path| path.file_name() == Some(std::ffi::OsStr::new(&profile)))
        .expect("无法从 OUT_DIR 定位 Cargo profile 输出目录");
    // 打包配置的路径相对 src-tauri 解析，所以固定写到清单目录下的 target，而不是实际 target 目录。
    let stage_dir = std::path::Path::new(
        &std::env::var("CARGO_MANIFEST_DIR").expect("Cargo 缺少 CARGO_MANIFEST_DIR"),
    )
    .join("target")
    .join("runtime-libs");
    std::fs::create_dir_all(&stage_dir).expect("无法创建 target/runtime-libs");
    // Windows 打包资源按通配符取整个目录，从清单移除的旧库不能留在这里。
    for entry in std::fs::read_dir(&stage_dir)
        .expect("无法读取 target/runtime-libs")
        .flatten()
    {
        if !names
            .iter()
            .any(|name| entry.file_name() == std::ffi::OsStr::new(name))
        {
            std::fs::remove_file(entry.path())
                .unwrap_or_else(|error| panic!("无法删除 {}：{error}", entry.path().display()));
        }
    }
    for name in names {
        let source = profile_dir.join(name);
        if !source.is_file() {
            panic!(
                "sherpa-onnx-sys 没有在 {} 生成 {name}，无法准备打包运行库",
                profile_dir.display()
            );
        }
        copy_if_changed(&source, &stage_dir.join(name));
    }
    if target_os == "windows" {
        let deps_dir = profile_dir.join("deps");
        std::fs::create_dir_all(&deps_dir).expect("无法创建 deps 目录");
        for name in names {
            copy_if_changed(&profile_dir.join(name), &deps_dir.join(name));
        }
    }
}

// 内容未变时不重写：运行中的程序或并行的打包可能正占用目标文件。
fn copy_if_changed(source: &std::path::Path, dest: &std::path::Path) {
    let same = match (std::fs::metadata(source), std::fs::metadata(dest)) {
        (Ok(source), Ok(dest)) => {
            source.len() == dest.len() && source.modified().ok() == dest.modified().ok()
        }
        _ => false,
    };
    if !same {
        std::fs::copy(source, dest).unwrap_or_else(|error| {
            panic!(
                "复制 {} 到 {} 失败：{error}",
                source.display(),
                dest.display()
            )
        });
    }
}
