import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

if (process.platform !== "darwin") {
  console.log("apple-speech: non-macOS platform, skipped.");
  process.exit(0);
}

const scriptPath = fileURLToPath(import.meta.url);
const root = resolve(dirname(scriptPath), "..");
const source = resolve(root, "src-tauri/macos/SayItAppleSpeech.swift");
const infoPlist = resolve(root, "src-tauri/macos/SayItAppleSpeech-Info.plist");
const targetTriple = process.arch === "arm64"
  ? "aarch64-apple-darwin"
  : "x86_64-apple-darwin";
const swiftTarget = process.arch === "arm64"
  ? "arm64-apple-macosx15.5"
  : "x86_64-apple-macosx15.5";
const output = resolve(root, `src-tauri/binaries/sayit-apple-speech-${targetTriple}`);
const developmentBundle = resolve(root, "src-tauri/binaries/SayItAppleSpeech.app");
const developmentExecutable = resolve(developmentBundle, "Contents/MacOS/sayit-apple-speech");
const developmentInfoPlist = resolve(root, "src-tauri/target/apple-speech-development-Info.plist");
const moduleCache = resolve(root, "src-tauri/target/swift-module-cache");
const buildStampPath = resolve(root, "src-tauri/binaries/apple-speech-build.json");

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

if (!existsSync(infoPlist)) throw new Error(`缺少 Apple 语音助手 Info.plist：${infoPlist}`);
const buildKey = createHash("sha256")
  .update(readFileSync(scriptPath))
  .update(readFileSync(source))
  .update(readFileSync(infoPlist))
  .update(`${sdkVersion}:${process.arch}`)
  .digest("hex");
let buildStamp = {};
if (existsSync(buildStampPath)) {
  try {
    buildStamp = JSON.parse(readFileSync(buildStampPath, "utf8"));
  } catch {
    buildStamp = {};
  }
}
const shouldRebuild =
  buildStamp.buildKey !== buildKey ||
  !existsSync(output) ||
  !existsSync(developmentExecutable) ||
  !existsSync(resolve(developmentBundle, "Contents/Info.plist"));

if (shouldRebuild) {
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
  args.push(
    source,
    "-Xlinker", "-sectcreate",
    "-Xlinker", "__TEXT",
    "-Xlinker", "__info_plist",
    "-Xlinker", infoPlist,
    "-o", output,
  );

  const result = spawnSync("xcrun", args, { stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);

  const signResult = spawnSync("/usr/bin/codesign", [
    "--force",
    "--sign", "-",
    "--identifier", "com.henjicc.sayit",
    output,
  ], { stdio: "inherit" });
  if (signResult.error) throw signResult.error;
  if (signResult.status !== 0) process.exit(signResult.status ?? 1);

  mkdirSync(dirname(developmentExecutable), { recursive: true });
  const developmentPlistContents = readFileSync(infoPlist, "utf8").replace(
    "<string>com.henjicc.sayit</string>",
    "<string>com.henjicc.sayit.dev.apple-speech</string>",
  );
  writeFileSync(developmentInfoPlist, developmentPlistContents);
  writeFileSync(resolve(developmentBundle, "Contents/Info.plist"), developmentPlistContents);
  const developmentArgs = [
    "swiftc",
    "-parse-as-library",
    "-O",
    "-module-cache-path",
    moduleCache,
    "-target",
    swiftTarget,
    "-D", "SAYIT_DEVELOPMENT_BUNDLE",
  ];
  if (sdkMajor >= 26) developmentArgs.push("-D", "SAYIT_HAS_SPEECH_ANALYZER");
  developmentArgs.push(
    source,
    "-Xlinker", "-sectcreate",
    "-Xlinker", "__TEXT",
    "-Xlinker", "__info_plist",
    "-Xlinker", developmentInfoPlist,
    "-o", developmentExecutable,
  );
  const developmentBuildResult = spawnSync("xcrun", developmentArgs, { stdio: "inherit" });
  if (developmentBuildResult.error) throw developmentBuildResult.error;
  if (developmentBuildResult.status !== 0) process.exit(developmentBuildResult.status ?? 1);
  chmodSync(developmentExecutable, 0o755);
  const developmentSignResult = spawnSync("/usr/bin/codesign", [
    "--force",
    "--sign", "-",
    "--identifier", "com.henjicc.sayit.dev.apple-speech",
    developmentBundle,
  ], { stdio: "inherit" });
  if (developmentSignResult.error) throw developmentSignResult.error;
  if (developmentSignResult.status !== 0) process.exit(developmentSignResult.status ?? 1);
  buildStamp = {
    buildKey,
    registeredDevelopmentKey: buildStamp.registeredDevelopmentKey || "",
  };
  writeFileSync(buildStampPath, `${JSON.stringify(buildStamp, null, 2)}\n`);
}

const checkResult = spawnSync(output, ["--self-check"], { encoding: "utf8" });
if (checkResult.error) throw checkResult.error;
const checkLine = checkResult.stdout.trim().split("\n").findLast(Boolean);
let check;
try {
  check = JSON.parse(checkLine || "{}");
} catch {
  throw new Error(`Apple 语音助手自检返回无效：${checkResult.stdout || checkResult.stderr}`);
}
if (
  checkResult.status !== 0 ||
  check.identityValid !== true ||
  check.bundleIdentifier !== "com.henjicc.sayit" ||
  check.usageDescriptionPresent !== true
) {
  throw new Error(`Apple 语音助手缺少稳定的权限身份：${check.message || checkLine || "未知错误"}`);
}
const accumulatorCheckResult = spawnSync(output, ["--accumulator-check"], { encoding: "utf8" });
if (accumulatorCheckResult.error) throw accumulatorCheckResult.error;
const accumulatorCheckLine = accumulatorCheckResult.stdout.trim().split("\n").findLast(Boolean);
const accumulatorCheck = JSON.parse(accumulatorCheckLine || "{}");
if (accumulatorCheckResult.status !== 0 || accumulatorCheck.available !== true) {
  throw new Error(`Apple 转写累加器自检失败：${accumulatorCheck.message || accumulatorCheckLine || "未知错误"}`);
}
const developmentCheckResult = spawnSync(developmentExecutable, ["--self-check"], { encoding: "utf8" });
if (developmentCheckResult.error) throw developmentCheckResult.error;
const developmentCheckLine = developmentCheckResult.stdout.trim().split("\n").findLast(Boolean);
const developmentCheck = JSON.parse(developmentCheckLine || "{}");
if (
  developmentCheckResult.status !== 0 ||
  developmentCheck.identityValid !== true ||
  developmentCheck.bundleIdentifier !== "com.henjicc.sayit.dev.apple-speech"
) {
  throw new Error(`Apple 语音助手开发 Bundle 自检失败：${developmentCheckResult.stdout || developmentCheckResult.stderr}`);
}
if (
  process.argv.includes("--development") &&
  buildStamp.registeredDevelopmentKey !== buildKey
) {
  // macOS 15 的 lsregister -f 可能错误地转交给 Spotlight 并返回 -10822。
  // 用无界面的自检启动一次可让 Launch Services 稳定登记开发 Bundle，进程会立即退出。
  const registerResult = spawnSync(
    "/usr/bin/open",
    ["-n", "-g", developmentBundle, "--args", "--self-check"],
    { stdio: "inherit" },
  );
  if (registerResult.error) throw registerResult.error;
  if (registerResult.status !== 0) process.exit(registerResult.status ?? 1);
  // open 在 Launch Services 完成登记前就会返回；稍候再重置，避免命中上一次签名的记录。
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 2_500);
  const resetResult = spawnSync(
    "/usr/bin/tccutil",
    ["reset", "SpeechRecognition", "com.henjicc.sayit.dev.apple-speech"],
    { stdio: "inherit" },
  );
  if (resetResult.error) throw resetResult.error;
  if (resetResult.status !== 0) process.exit(resetResult.status ?? 1);
  buildStamp.registeredDevelopmentKey = buildKey;
  writeFileSync(buildStampPath, `${JSON.stringify(buildStamp, null, 2)}\n`);
}
console.log(`apple-speech: ${shouldRebuild ? "built" : "reused"} ${output} with macOS SDK ${sdkVersion}${sdkMajor < 26 ? " (legacy backend only)" : ""}.`);
