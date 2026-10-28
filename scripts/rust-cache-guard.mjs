import { spawnSync } from "node:child_process";
import { existsSync, lstatSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

// Cargo 从不回收 target 里的旧产物：工具链升级、依赖或特性变化都会生成新哈希的一代，
// 旧代原样留下。一代新鲜的 debug 构建约 5 GiB，超过预算就说明旧代已经堆积。
export const DEBUG_BUDGET_BYTES = 20 * 1024 ** 3;
// Windows 上逐个统计文件大小，20 GiB 的目录约需数秒；旧代按天累积，每天量一次足够。
export const SIZE_CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;

const manifestPath = "src-tauri/Cargo.toml";
const targetRoot = path.resolve("src-tauri/target");
const toolchainStampPath = path.join(targetRoot, ".sayit-rustc-version");
const sherpaCacheDir = path.join(targetRoot, "sherpa-onnx-prebuilt");
// npm run check:mac-overlay 的交叉检查产物。依赖或 feature 变化同样会在其中留下旧产物，
// 但整体只有几十 MB（约 33 MB 量级），因此不计入 debug 预算，只在工具链变更时整体删除。
const macCheckDir = path.join(targetRoot, "mac-check");

function isMissing(error) {
  return error?.code === "ENOENT";
}

/** 递归累计文件大小；统计期间被并发构建删掉的条目直接跳过。 */
export function directorySize(root) {
  let total = 0;
  const pending = [root];
  while (pending.length > 0) {
    const current = pending.pop();
    let entries;
    try {
      entries = readdirSync(current, { withFileTypes: true });
    } catch (error) {
      if (isMissing(error)) continue;
      throw error;
    }
    for (const entry of entries) {
      const entryPath = path.join(current, entry.name);
      if (entry.isDirectory()) {
        pending.push(entryPath);
      } else if (entry.isFile()) {
        try {
          total += lstatSync(entryPath).size;
        } catch (error) {
          if (!isMissing(error)) throw error;
        }
      }
    }
  }
  return total;
}

function formatGiB(bytes) {
  return `${(bytes / 1024 ** 3).toFixed(1)} GiB`;
}

/** 工具链未变且当天量过体积时，跳过这次检查。 */
export function shouldCheckDebug({ previousToolchain, currentToolchain, lastCheckedMs, nowMs }) {
  return previousToolchain !== currentToolchain || nowMs - lastCheckedMs >= SIZE_CHECK_INTERVAL_MS;
}

/** 有上次记录且与当前不同才算变更；首次运行没有记录，不视为变更。 */
export function toolchainChanged(previousToolchain, currentToolchain) {
  return Boolean(previousToolchain) && previousToolchain !== currentToolchain;
}

/** 返回需要清理 debug 产物的原因；不需要清理时返回 null。 */
export function debugCleanupReason({ previousToolchain, currentToolchain, debugBytes, budgetBytes = DEBUG_BUDGET_BYTES }) {
  if (toolchainChanged(previousToolchain, currentToolchain)) {
    return "Rust 工具链已变更，旧产物全部失效";
  }
  if (debugBytes > budgetBytes) {
    return `debug 产物 ${formatGiB(debugBytes)} 超过 ${formatGiB(budgetBytes)} 预算`;
  }
  return null;
}

/** sherpa-onnx-sys 按自身版本下载同版本预编译库，其他版本的解包目录与压缩包都已无用。 */
export function staleSherpaEntries(entries, currentVersion) {
  const currentPrefix = `sherpa-onnx-v${currentVersion}-`;
  return entries.filter((name) => name.startsWith("sherpa-onnx-v") && !name.startsWith(currentPrefix));
}

/** 只有产出 target/release 的打包才清理本地 release；调试、交叉编译与帮助命令不动它。 */
export function replacesLocalRelease(args) {
  return !args.some(
    (arg) =>
      ["--debug", "-d", "--help", "-h", "--target", "-t"].includes(arg) || arg.startsWith("--target="),
  );
}

function lockedSherpaVersion() {
  const lock = readFileSync("src-tauri/Cargo.lock", "utf8");
  return lock.match(/name = "sherpa-onnx-sys"\r?\nversion = "([^"]+)"/)?.[1] ?? null;
}

function cargoClean(profileArgs, reason) {
  process.stderr.write(`[rust-cache] ${reason}，执行 cargo clean ${profileArgs.join(" ")}\n`);
  const result = spawnSync("cargo", ["clean", "--manifest-path", manifestPath, ...profileArgs], {
    stdio: "inherit",
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error("[rust-cache] 清理失败；若应用、调试器或另一次打包仍在运行，先结束它再重试");
  }
}

function readToolchain() {
  const result = spawnSync("rustc", ["-vV"], { cwd: "src-tauri", encoding: "utf8" });
  if (result.error || result.status !== 0) {
    throw new Error(`[rust-cache] 无法读取 Rust 工具链版本：${result.error?.message ?? result.stderr}`);
  }
  return result.stdout.trim();
}

/** 自定义 target 目录可能与其他项目共用，清理它会误删别人的产物，因此只管理默认目录。 */
function managesDefaultTarget() {
  if (!process.env.CARGO_TARGET_DIR) return true;
  process.stderr.write("[rust-cache] 已设置 CARGO_TARGET_DIR，跳过构建缓存回收\n");
  return false;
}

function pruneSherpaCache() {
  const version = lockedSherpaVersion();
  if (!version || !existsSync(sherpaCacheDir)) return;
  for (const name of staleSherpaEntries(readdirSync(sherpaCacheDir), version)) {
    process.stderr.write(`[rust-cache] 删除不再使用的 sherpa-onnx 预编译库 ${name}\n`);
    rmSync(path.join(sherpaCacheDir, name), { recursive: true, force: true });
  }
}

/** dev 与测试入口调用：回收失效或超预算的 debug 产物，以及旧版本 sherpa 预编译库。 */
export function guardDebugCache() {
  // CI 的 target 由 rust-cache 按工具链与锁文件缓存，回收只会让每次构建从头开始。
  if (process.env.CI || !managesDefaultTarget()) return;
  // target 交给 cargo 首次创建，它会写入 CACHEDIR.TAG 并把目录排除出备份与索引。
  if (!existsSync(targetRoot)) return;
  const currentToolchain = readToolchain();
  const stampExists = existsSync(toolchainStampPath);
  const previousToolchain = stampExists ? readFileSync(toolchainStampPath, "utf8").trim() : null;
  const lastCheckedMs = stampExists ? statSync(toolchainStampPath).mtimeMs : 0;
  if (!shouldCheckDebug({ previousToolchain, currentToolchain, lastCheckedMs, nowMs: Date.now() })) return;
  const reason = debugCleanupReason({
    previousToolchain,
    currentToolchain,
    debugBytes: directorySize(path.join(targetRoot, "debug")),
  });
  if (reason) cargoClean(["--profile", "dev"], reason);
  if (toolchainChanged(previousToolchain, currentToolchain) && existsSync(macCheckDir)) {
    process.stderr.write("[rust-cache] Rust 工具链已变更，删除 Mac 交叉检查产物 mac-check\n");
    rmSync(macCheckDir, { recursive: true, force: true });
  }
  pruneSherpaCache();
  // 记录文件的修改时间即本次检查时间。
  writeFileSync(toolchainStampPath, `${currentToolchain}\n`);
}

/**
 * run-tauri 的 build 入口：本地 release 只保留最新一次构建，打包前清掉旧的 release 产物与安装包，
 * 因此每次本地打包（含失败后的重试）都从零编译 release。
 */
export function buildWithFreshRelease(args, build) {
  if (
    !process.env.CI &&
    replacesLocalRelease(args) &&
    managesDefaultTarget() &&
    existsSync(path.join(targetRoot, "release"))
  ) {
    cargoClean(["--release"], "本地 release 只保留最新一次构建");
  }
  return build();
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  guardDebugCache();
}
