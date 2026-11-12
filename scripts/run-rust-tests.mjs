import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, readdirSync, rmSync, statSync } from "node:fs";
import path from "node:path";

import { withMacBuildEnv } from "./macos-build-env.mjs";

const sherpaLinkLibraryPattern = /^(?:lib)?sherpa-onnx-c-api\.(?:a|dylib|lib|so)$/i;
/** sherpa-onnx-sys 的构建脚本复制到 profile 目录、再由 build.rs 分发的运行库。 */
const sherpaRuntimeLibrary = {
  win32: "sherpa-onnx-c-api.dll",
  darwin: "libsherpa-onnx-c-api.dylib",
}[process.platform];

const cargoArgs = process.argv.slice(2);
const targetRoot = path.resolve(process.env.CARGO_TARGET_DIR ?? "src-tauri/target");

function containsSherpaLinkLibrary(root) {
  if (!existsSync(root)) return false;
  const pending = [root];
  while (pending.length > 0) {
    const current = pending.pop();
    for (const entry of readdirSync(current, { withFileTypes: true })) {
      if (entry.isDirectory()) {
        pending.push(path.join(current, entry.name));
      } else if (sherpaLinkLibraryPattern.test(entry.name)) {
        return true;
      }
    }
  }
  return false;
}

function removeMatchingEntries(root, pattern) {
  if (!existsSync(root)) return;
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    if (pattern.test(entry.name)) {
      rmSync(path.join(root, entry.name), { recursive: true, force: true });
    }
  }
}

/**
 * 删掉 sherpa-onnx-sys 的构建记录，让 Cargo 重跑它的构建脚本。
 * 旧布局是 `build/sherpa-onnx-sys-<哈希>`、`.fingerprint/`、`deps/`；
 * Rust 1.100 起的新布局把指纹和产物都放在 `build/sherpa-onnx-sys/<哈希>/` 下。
 */
function forgetSherpaBuild(profileRoot) {
  removeMatchingEntries(path.join(profileRoot, "build"), /^sherpa-onnx-sys(?:-|$)/);
  removeMatchingEntries(path.join(profileRoot, ".fingerprint"), /^sherpa-onnx-sys-/);
  removeMatchingEntries(path.join(profileRoot, "deps"), /sherpa_onnx_sys/);
}

/**
 * Rust 缓存会保留「构建脚本已跑过」的指纹，却不保留它在 OUT_DIR 之外产出的文件：下载解压的预编译库，
 * 以及复制到 profile 目录的运行库。Cargo 认为它已是最新，不会重跑，build.rs 就找不到运行库。
 */
function repairIncompleteSherpaCache() {
  if (process.env.SHERPA_ONNX_LIB_DIR) return;
  const prebuiltRoot = path.join(targetRoot, "sherpa-onnx-prebuilt");
  if (!containsSherpaLinkLibrary(prebuiltRoot)) {
    rmSync(prebuiltRoot, { recursive: true, force: true });
    for (const profile of ["debug", "release"]) forgetSherpaBuild(path.join(targetRoot, profile));
    return;
  }
  const profileRoot = path.join(targetRoot, cargoArgs.includes("--release") ? "release" : "debug");
  if (sherpaRuntimeLibrary && !existsSync(path.join(profileRoot, sherpaRuntimeLibrary))) {
    forgetSherpaBuild(profileRoot);
  }
}

function copyIfChanged(source, destination) {
  const from = statSync(source);
  if (existsSync(destination)) {
    const to = statSync(destination);
    if (to.size === from.size && to.mtimeMs >= from.mtimeMs) return;
  }
  copyFileSync(source, destination);
}

repairIncompleteSherpaCache();

// Cargo tests compile the Tauri build script, but do not create installer
// assets. Clear sidecars so a clean checkout can run tests before they are
// built; runtime libraries are staged by build.rs for every profile.
const testConfig = {
  bundle: {
    active: false,
    externalBin: [],
  },
};
const cargoTestPrefix = ["test", "--manifest-path", "src-tauri/Cargo.toml"];
const cargoTest = [...cargoTestPrefix, ...cargoArgs];
const env = {
  ...withMacBuildEnv(process.env),
  TAURI_CONFIG: JSON.stringify(testConfig),
};

// Windows 先找应用目录再找系统目录，而系统目录里有一份旧版 onnxruntime.dll，所以运行库必须放在
// 测试程序旁边，不能靠 PATH。测试程序的位置随 Cargo 版本变化（旧布局在 deps/，新布局在
// build/say-it/<哈希>/out/），先只编译，按 Cargo 报告的实际路径放好运行库再运行。
if (process.platform === "win32") {
  // 加在用户参数之前：用户参数里可能有 `--`（之后的参数交给测试程序），也可能自带 `--no-run`。
  const buildOnly = cargoArgs.includes("--no-run") ? [] : ["--no-run"];
  const build = spawnSync(
    "cargo",
    [...cargoTestPrefix, ...buildOnly, "--message-format=json-render-diagnostics", ...cargoArgs],
    {
      stdio: ["inherit", "pipe", "inherit"],
      env,
      encoding: "utf8",
      maxBuffer: 256 * 1024 * 1024,
    },
  );
  if (build.error) throw build.error;
  if (build.status !== 0) process.exit(build.status ?? 1);
  // build.rs 把 sherpa 与 transcribe.cpp 的运行库集中放在清单目录下的 target/runtime-libs。
  const runtimeDir = path.resolve("src-tauri/target/runtime-libs");
  const runtimeLibs = readdirSync(runtimeDir);
  const appManifest = path.resolve("src-tauri/Cargo.toml");
  for (const line of build.stdout.split("\n")) {
    if (!line.startsWith("{")) continue;
    const message = JSON.parse(line);
    if (message.reason !== "compiler-artifact" || !message.executable) continue;
    // 应用包的每个测试程序都链接了 sherpa；子 crate 的不需要。
    if (path.resolve(message.manifest_path) !== appManifest) continue;
    const testDir = path.dirname(message.executable);
    for (const name of runtimeLibs) {
      copyIfChanged(path.join(runtimeDir, name), path.join(testDir, name));
    }
  }
}

const result = spawnSync("cargo", cargoTest, { stdio: "inherit", env });

if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status ?? 1);
