import { existsSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname } from "node:path";

const [command, ...args] = process.argv.slice(2);
if (!command) throw new Error("缺少 Tauri 子命令");

const env = { ...process.env };
if (process.platform === "darwin" && !env.MACOSX_DEPLOYMENT_TARGET) {
  env.MACOSX_DEPLOYMENT_TARGET = "11.0";
}
if (process.platform === "darwin" && !env.OPUS_LIB_DIR && !env.LIBOPUS_LIB_DIR) {
  const opusLibraries = [
    "/opt/homebrew/lib/libopus.a",
    "/usr/local/lib/libopus.a",
  ];
  const opusLibrary = opusLibraries.find(existsSync);
  if (opusLibrary) env.OPUS_LIB_DIR = dirname(opusLibrary);
}

const result = spawnSync("tauri", [command, ...args], { stdio: "inherit", env });
if (result.error) throw result.error;
process.exit(result.status ?? 1);
