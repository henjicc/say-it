import { mkdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

if (process.platform !== "darwin") {
  console.log("apple-speech: non-macOS platform, skipped.");
  process.exit(0);
}

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const source = resolve(root, "src-tauri/macos/SayItAppleSpeech.swift");
const targetTriple = process.arch === "arm64"
  ? "aarch64-apple-darwin"
  : "x86_64-apple-darwin";
const swiftTarget = process.arch === "arm64"
  ? "arm64-apple-macosx15.5"
  : "x86_64-apple-macosx15.5";
const output = resolve(root, `src-tauri/binaries/sayit-apple-speech-${targetTriple}`);
const moduleCache = resolve(root, "src-tauri/target/swift-module-cache");

const sdkResult = spawnSync("xcrun", ["--sdk", "macosx", "--show-sdk-version"], {
  encoding: "utf8",
});
if (sdkResult.error) throw sdkResult.error;
if (sdkResult.status !== 0) {
  process.stderr.write(sdkResult.stderr || "无法读取 macOS SDK 版本。\n");
  process.exit(sdkResult.status ?? 1);
}
const sdkVersion = sdkResult.stdout.trim();
const sdkMajor = Number.parseInt(sdkVersion.split(".")[0] || "0", 10);
const requireSdk26 = process.argv.includes("--require-sdk-26") || process.env.SAYIT_REQUIRE_MACOS26_SDK === "1";
if (requireSdk26 && sdkMajor < 26) {
  throw new Error(`Apple 本地语音识别发布构建需要 macOS 26 SDK 以包含新引擎，当前为 ${sdkVersion}`);
}

mkdirSync(dirname(output), { recursive: true });
mkdirSync(moduleCache, { recursive: true });
const args = [
  "swiftc",
  "-parse-as-library",
  "-O",
  "-module-cache-path",
  moduleCache,
  "-target",
  swiftTarget,
];
if (sdkMajor >= 26) args.push("-D", "SAYIT_HAS_SPEECH_ANALYZER");
args.push(source, "-o", output);

const result = spawnSync("xcrun", args, { stdio: "inherit" });
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status ?? 1);
console.log(`apple-speech: built ${output} with macOS SDK ${sdkVersion}${sdkMajor < 26 ? " (legacy backend only)" : ""}.`);
