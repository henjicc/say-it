// 构建本地大模型推理进程 sayit-llm（src-tauri/llm-engine），放到 Tauri 的 sidecar 目录。
// 它单独成进程：推理库与语音识别运行时各带一份 ggml，不能进同一个可执行文件。
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import path from "node:path";

import { withMacBuildEnv } from "./macos-build-env.mjs";

const targets = {
  win32: { built: "sayit-llm.exe", sidecar: "sayit-llm-x86_64-pc-windows-msvc.exe" },
  darwin: {
    built: "sayit-llm",
    sidecar: `sayit-llm-${process.arch === "arm64" ? "aarch64" : "x86_64"}-apple-darwin`,
  },
};
const target = targets[process.platform];
if (!target) {
  console.log("llm-engine: 目前只支持 Windows 与 macOS，已跳过。");
  process.exit(0);
}

const targetRoot = path.resolve(process.env.CARGO_TARGET_DIR ?? "src-tauri/target");
const build = () =>
  spawnSync(
    "cargo",
    ["build", "--release", "--manifest-path", "src-tauri/Cargo.toml", "-p", "sayit-llm-engine"],
    { stdio: "inherit", env: withMacBuildEnv(process.env) },
  );

// llama.cpp 的 Vulkan 着色器生成器是嵌套的 CMake 工程，MSBuild 并行时它的构建步骤会抢在配置步骤
// 之前执行（报 MSB1009 项目文件不存在），安装步骤也会抢在编译完成前执行（报 file INSTALL cannot find
// vulkan-shaders-gen.exe）。已完成的步骤会保留，原样重来通常一两次就能通过。
let result = build();
for (let retry = 0; retry < 3 && result.status !== 0 && process.platform === "win32"; retry += 1) result = build();
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status ?? 1);

const binaries = path.resolve("src-tauri/binaries");
mkdirSync(binaries, { recursive: true });
copyFileSync(path.join(targetRoot, "release", target.built), path.join(binaries, target.sidecar));
