import { spawnSync } from "node:child_process";
import {
  existsSync,
  lstatSync,
  readdirSync,
  readFileSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
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
// 目录名带上打包进程的 PID：那次打包还在运行时，其他入口不得恢复或删除它。
const previousReleasePrefix = "release.previous-";
const trashReleasePrefix = "release.trash-";
// 打包已成功、旧 release 却没能移走时写入其中，表示它已被取代，只能删除不能恢复。
const supersededMarker = ".sayit-superseded";

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

/** 返回需要清理 debug 产物的原因；不需要清理时返回 null。 */
export function debugCleanupReason({ previousToolchain, currentToolchain, debugBytes, budgetBytes = DEBUG_BUDGET_BYTES }) {
  if (previousToolchain && previousToolchain !== currentToolchain) {
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

/** 只有产出 target/release 的打包才替换本地 release；调试、交叉编译与帮助命令不动它。 */
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
    throw new Error("[rust-cache] 清理失败；若应用或调试器仍在运行，先关闭它再重试");
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

function isRunning(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error.code === "EPERM";
  }
}

function moveDirectory(from, to) {
  try {
    renameSync(from, to);
  } catch (error) {
    if (error.code !== "EPERM" && error.code !== "EBUSY") throw error;
    throw new Error(
      `[rust-cache] 无法移动 ${from}：其中有文件正被占用（运行中的 release 程序、停在该目录的终端或资源管理器），关闭后重试`,
    );
  }
}

function releaseLeftovers(root, prefix) {
  return readdirSync(root)
    .map((name) => ({ name, pid: name.startsWith(prefix) ? /^(\d+)(?:-\d+)?$/.exec(name.slice(prefix.length))?.[1] : null }))
    .filter(({ pid }) => pid)
    .map(({ name, pid }) => ({ dir: path.join(root, name), pid: Number(pid) }));
}

// 先原子改名为待删目录再删除：删到一半被打断时，残留的只会是待删目录，不会被当成可恢复的构建。
function discardRelease(root, dir) {
  const trashDir = path.join(root, `${trashReleasePrefix}${process.pid}-${Date.now()}`);
  moveDirectory(dir, trashDir);
  rmSync(trashDir, { recursive: true, force: true });
}

/**
 * 收拾已结束的打包留下的目录：待删目录与标记为已被取代的旧 release 直接删除；其余暂存的
 * 旧 release 说明打包被中断，它才是上一次完整的构建，target/release 只是半成品。
 * 返回仍在运行的那次打包，没有则返回 null。
 */
export function settleReleaseLeftovers(root) {
  if (!existsSync(root)) return null;
  for (const { dir, pid } of releaseLeftovers(root, trashReleasePrefix)) {
    if (!isRunning(pid)) rmSync(dir, { recursive: true, force: true });
  }
  for (const { dir, pid } of releaseLeftovers(root, previousReleasePrefix)) {
    if (isRunning(pid)) return { pid, dir };
    if (existsSync(path.join(dir, supersededMarker))) {
      discardRelease(root, dir);
      continue;
    }
    process.stderr.write("[rust-cache] 上次打包未完成，恢复上一次完整的 release 构建\n");
    const releaseDir = path.join(root, "release");
    rmSync(releaseDir, { recursive: true, force: true });
    moveDirectory(dir, releaseDir);
  }
  return null;
}

/**
 * 本地 release 只保留最新一次构建。dev 构建会复制 Tauri 配置引用的 target/release 运行库，
 * 所以先把旧 release 移开，打包成功才删除；失败时原样恢复，dev 与上一版安装包都不受影响。
 */
export function replaceRelease(root, build) {
  const running = settleReleaseLeftovers(root);
  if (running) {
    throw new Error(
      `[rust-cache] 另一次打包仍在进行（PID ${running.pid}）。若确认没有打包在运行，删除半成品 target/release，把 ${running.dir} 改名回 release 后重试`,
    );
  }
  const releaseDir = path.join(root, "release");
  if (!existsSync(releaseDir)) return build();
  process.stderr.write("[rust-cache] 本地 release 只保留最新一次构建，旧构建打包成功后删除\n");
  const previousDir = path.join(root, `${previousReleasePrefix}${process.pid}`);
  moveDirectory(releaseDir, previousDir);
  const result = build();
  // 收尾出错不改变打包结果，留下的目录由下次启动的 settleReleaseLeftovers 处理。
  if (result.error || result.status !== 0) {
    try {
      rmSync(releaseDir, { recursive: true, force: true });
      moveDirectory(previousDir, releaseDir);
      process.stderr.write("[rust-cache] 打包失败，已恢复上一次 release 构建\n");
    } catch (error) {
      process.stderr.write(`[rust-cache] 打包失败，恢复上一次 release 时出错：${error.message}；下次启动会再次恢复\n`);
    }
  } else {
    try {
      discardRelease(root, previousDir);
    } catch (error) {
      process.stderr.write(`[rust-cache] 未能删除上一次 release：${error.message}；${markSuperseded(previousDir)}\n`);
    }
  }
  return result;
}

function markSuperseded(previousDir) {
  if (!existsSync(previousDir)) return "下次启动时再删除";
  try {
    writeFileSync(path.join(previousDir, supersededMarker), "");
    return "下次启动时再删除";
  } catch {
    return `请确认新 release 可用后手动删除 ${previousDir}，否则下次启动会用它覆盖新 release`;
  }
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
  const running = settleReleaseLeftovers(targetRoot);
  if (running) process.stderr.write(`[rust-cache] 打包仍在进行（PID ${running.pid}），不动 ${running.dir}\n`);
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
  pruneSherpaCache();
  // 记录文件的修改时间即本次检查时间。
  writeFileSync(toolchainStampPath, `${currentToolchain}\n`);
}

/** run-tauri 的 build 入口：只在本地、默认 target 且产出 target/release 时替换 release。 */
export function buildWithFreshRelease(args, build) {
  if (process.env.CI || !replacesLocalRelease(args) || !managesDefaultTarget()) return build();
  return replaceRelease(targetRoot, build);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  guardDebugCache();
}
