import { existsSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname } from "node:path";

const [command, ...args] = process.argv.slice(2);
if (!command) throw new Error("缺少 Tauri 子命令");

const env = { ...process.env };
if (process.platform === "darwin" && !env.MACOSX_DEPLOYMENT_TARGET) {
  env.MACOSX_DEPLOYMENT_TARGET = "15.5";
}
if (
  process.platform === "darwin" &&
  command === "build" &&
  !env.APPLE_SIGNING_IDENTITY
) {
  env.APPLE_SIGNING_IDENTITY = "-";
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
if (result.status !== 0) process.exit(result.status ?? 1);
if (process.platform === "darwin" && command === "build" && !args.includes("--no-bundle")) {
  const validation = spawnSync(
    process.execPath,
    ["scripts/validate-macos-bundle.mjs"],
    { stdio: "inherit", env },
  );
  if (validation.error) throw validation.error;
  if (validation.status !== 0) process.exit(validation.status ?? 1);
}
