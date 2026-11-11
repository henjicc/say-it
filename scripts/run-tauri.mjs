import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";

import { withMacBuildEnv } from "./macos-build-env.mjs";
import { buildWithFreshRelease } from "./rust-cache-guard.mjs";

const [command, ...args] = process.argv.slice(2);
if (!command) throw new Error("缺少 Tauri 子命令");

const env = withMacBuildEnv(process.env);
if (
  process.platform === "darwin" &&
  command === "build" &&
  !env.APPLE_SIGNING_IDENTITY
) {
  env.APPLE_SIGNING_IDENTITY = "-";
}

// Windows 的 npm 命令入口是 .cmd，不能直接作为 spawnSync 的可执行文件。
const require = createRequire(import.meta.url);
const tauriCli = require.resolve("@tauri-apps/cli/tauri.js");
const runTauri = () =>
  spawnSync(process.execPath, [tauriCli, command, ...args], {
    stdio: "inherit",
    env,
  });
const result = command === "build" ? buildWithFreshRelease(args, runTauri) : runTauri();
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
