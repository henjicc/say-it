// 构建本地大模型推理进程 sayit-llm（src-tauri/llm-engine），放到 Tauri 的 sidecar 目录。
// 它单独成进程：推理库与语音识别运行时各带一份 ggml，不能进同一个可执行文件。
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import path from "node:path";

if (process.platform !== "win32") {
  console.log("llm-engine: 目前只支持 Windows，已跳过。");
  process.exit(0);
}

const targetRoot = path.resolve(process.env.CARGO_TARGET_DIR ?? "src-tauri/target");
const build = () =>
  spawnSync(
    "cargo",
    ["build", "--release", "--manifest-path", "src-tauri/Cargo.toml", "-p", "sayit-llm-engine"],
    { stdio: "inherit" },
  );

// llama.cpp 的 Vulkan 着色器生成器是嵌套的 CMake 工程，MSBuild 并行时它的构建步骤会抢在配置步骤
// 之前执行（报 MSB1009 项目文件不存在）。配置步骤其实已完成，原样再来一次即可通过。
let result = build();
if (result.status !== 0) result = build();
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status ?? 1);

const binaries = path.resolve("src-tauri/binaries");
mkdirSync(binaries, { recursive: true });
copyFileSync(
  path.join(targetRoot, "release", "sayit-llm.exe"),
  path.join(binaries, "sayit-llm-x86_64-pc-windows-msvc.exe"),
);
