import { spawnSync } from "node:child_process";
import path from "node:path";

// 主 crate 依赖需要 Apple SDK 的 C 库，无法在非 Mac 主机上检查；Mac 悬浮窗代码集中在只依赖
// objc2 系的子 crate 里，这里只对它做 aarch64-apple-darwin 类型检查。产物放进独立目录，
// 不与主构建争 Cargo 锁，也不让交叉检查的产物混进 debug 体积统计。
const macTarget = "aarch64-apple-darwin";
const targetRoot = path.resolve(process.env.CARGO_TARGET_DIR ?? "src-tauri/target");

function run(command, args, options = {}) {
  const result = spawnSync(command, args, options);
  if (result.error) throw result.error;
  return result;
}

const installed = run("rustup", ["target", "list", "--installed"], { encoding: "utf8" });
if (installed.status !== 0 || !installed.stdout.split(/\r?\n/).includes(macTarget)) {
  process.stderr.write(`[check:mac-overlay] 未安装 ${macTarget} 目标，先运行：rustup target add ${macTarget}\n`);
  process.exit(1);
}

const check = run(
  "cargo",
  [
    "check",
    "--manifest-path",
    "src-tauri/Cargo.toml",
    "-p",
    "sayit-overlay-macos",
    "--target",
    macTarget,
    "--all-targets",
    "--target-dir",
    path.join(targetRoot, "mac-check"),
  ],
  { stdio: "inherit" },
);
if (check.status !== 0) process.exit(check.status ?? 1);
