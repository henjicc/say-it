import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import {
  DEBUG_BUDGET_BYTES,
  SIZE_CHECK_INTERVAL_MS,
  debugCleanupReason,
  directorySize,
  replaceRelease,
  replacesLocalRelease,
  settleReleaseLeftovers,
  shouldCheckDebug,
  staleSherpaEntries,
} from "./rust-cache-guard.mjs";

function withTarget(run) {
  const root = mkdtempSync(path.join(tmpdir(), "rust-cache-release-"));
  try {
    run(root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function writeRelease(root, dirName, content) {
  mkdirSync(path.join(root, dirName), { recursive: true });
  writeFileSync(path.join(root, dirName, "onnxruntime.dll"), content);
}

function releaseContent(root) {
  const file = path.join(root, "release", "onnxruntime.dll");
  return existsSync(file) ? readFileSync(file, "utf8") : null;
}

function exitedPid() {
  return spawnSync(process.execPath, ["-e", ""]).pid;
}

const toolchain = "rustc 1.99.0 (b940084d7 2026-09-28)";
const oldToolchain = "rustc 1.98.0 (aaaaaaaaa 2026-08-01)";

test("首次运行没有工具链记录且未超预算时不清理", () => {
  assert.equal(
    debugCleanupReason({ previousToolchain: null, currentToolchain: toolchain, debugBytes: 1024 }),
    null,
  );
});

test("工具链变更后清理，即使体积很小", () => {
  const reason = debugCleanupReason({ previousToolchain: oldToolchain, currentToolchain: toolchain, debugBytes: 0 });
  assert.match(reason, /工具链已变更/);
});

test("体积超过预算时清理，等于预算时不清理", () => {
  const over = debugCleanupReason({
    previousToolchain: toolchain,
    currentToolchain: toolchain,
    debugBytes: DEBUG_BUDGET_BYTES + 1,
  });
  assert.match(over, /超过 20\.0 GiB 预算/);
  assert.equal(
    debugCleanupReason({ previousToolchain: toolchain, currentToolchain: toolchain, debugBytes: DEBUG_BUDGET_BYTES }),
    null,
  );
});

test("工具链未变时每天只检查一次体积，工具链变更立即检查", () => {
  const nowMs = 10 * SIZE_CHECK_INTERVAL_MS;
  const sameToolchain = { previousToolchain: toolchain, currentToolchain: toolchain, nowMs };
  assert.equal(shouldCheckDebug({ ...sameToolchain, lastCheckedMs: nowMs - 1000 }), false);
  assert.equal(shouldCheckDebug({ ...sameToolchain, lastCheckedMs: nowMs - SIZE_CHECK_INTERVAL_MS }), true);
  assert.equal(
    shouldCheckDebug({ previousToolchain: oldToolchain, currentToolchain: toolchain, lastCheckedMs: nowMs, nowMs }),
    true,
  );
  assert.equal(
    shouldCheckDebug({ previousToolchain: null, currentToolchain: toolchain, lastCheckedMs: 0, nowMs }),
    true,
  );
});

test("只回收与锁定版本不同的 sherpa-onnx 预编译库", () => {
  const entries = [
    "sherpa-onnx-v1.13.4-win-x64-shared-MT-Release-lib",
    "sherpa-onnx-v1.13.4-win-x64-static-MT-Release-lib.tar.bz2",
    "sherpa-onnx-v1.13.5-win-x64-shared-MT-Release-lib",
    "sherpa-onnx-v1.13.5-win-x64-shared-MT-Release-lib.tar.bz2",
    "sherpa-onnx-v1.13.50-win-x64-shared-MT-Release-lib",
    "README.txt",
  ];
  assert.deepEqual(staleSherpaEntries(entries, "1.13.5"), [
    "sherpa-onnx-v1.13.4-win-x64-shared-MT-Release-lib",
    "sherpa-onnx-v1.13.4-win-x64-static-MT-Release-lib.tar.bz2",
    "sherpa-onnx-v1.13.50-win-x64-shared-MT-Release-lib",
  ]);
});

test("只有产出 target/release 的打包才替换本地 release", () => {
  assert.equal(replacesLocalRelease([]), true);
  assert.equal(replacesLocalRelease(["--no-bundle"]), true);
  assert.equal(replacesLocalRelease(["--bundles", "nsis"]), true);
  for (const args of [["--debug"], ["-d"], ["--help"], ["-h"], ["--target", "x86_64-pc-windows-msvc"], ["-t", "x"], ["--target=x"]]) {
    assert.equal(replacesLocalRelease(args), false, args.join(" "));
  }
});

test("打包成功后只保留新 release", () => {
  withTarget((root) => {
    writeRelease(root, "release", "old");
    const result = replaceRelease(root, () => {
      assert.equal(releaseContent(root), null);
      writeRelease(root, "release", "new");
      return { status: 0 };
    });
    assert.equal(result.status, 0);
    assert.equal(releaseContent(root), "new");
    assert.deepEqual(readdirSync(root), ["release"]);
  });
});

test("打包失败时恢复旧 release 并丢弃半成品", () => {
  withTarget((root) => {
    writeRelease(root, "release", "old");
    const result = replaceRelease(root, () => {
      writeRelease(root, "release", "half");
      return { status: 1 };
    });
    assert.equal(result.status, 1);
    assert.equal(releaseContent(root), "old");
    assert.deepEqual(readdirSync(root), ["release"]);
  });
});

test("没有旧 release 时直接打包", () => {
  withTarget((root) => {
    replaceRelease(root, () => {
      writeRelease(root, "release", "new");
      return { status: 0 };
    });
    assert.equal(releaseContent(root), "new");
  });
});

test("打包进程已退出时恢复它暂存的旧 release", () => {
  withTarget((root) => {
    writeRelease(root, `release.previous-${exitedPid()}`, "old");
    writeRelease(root, "release", "half");
    assert.equal(settleReleaseLeftovers(root), null);
    assert.equal(releaseContent(root), "old");
    assert.deepEqual(readdirSync(root), ["release"]);
  });
});

test("删到一半的待删目录直接删除，不当作可恢复的构建", () => {
  withTarget((root) => {
    writeRelease(root, "release", "new");
    writeRelease(root, `release.trash-${exitedPid()}`, "old");
    assert.equal(settleReleaseLeftovers(root), null);
    assert.equal(releaseContent(root), "new");
    assert.deepEqual(readdirSync(root), ["release"]);
  });
});

test("另一次打包仍在进行时不动它的目录，并拒绝再次打包", () => {
  withTarget((root) => {
    const previous = `release.previous-${process.pid}`;
    writeRelease(root, previous, "old");
    writeRelease(root, "release", "building");
    assert.deepEqual(settleReleaseLeftovers(root), { pid: process.pid, dir: path.join(root, previous) });
    assert.equal(releaseContent(root), "building");
    assert.throws(() => replaceRelease(root, () => ({ status: 0 })), /另一次打包仍在进行.*改名回 release/);
    assert.equal(releaseContent(root), "building");
  });
});

test("标记为已被取代的旧 release 只删除，不覆盖新 release", () => {
  withTarget((root) => {
    const previous = `release.previous-${exitedPid()}`;
    writeRelease(root, previous, "old");
    writeFileSync(path.join(root, previous, ".sayit-superseded"), "");
    writeRelease(root, "release", "new");
    assert.equal(settleReleaseLeftovers(root), null);
    assert.equal(releaseContent(root), "new");
    assert.deepEqual(readdirSync(root), ["release"]);
  });
});

test("目录体积递归累计文件大小，目录不存在时为 0", () => {
  const root = mkdtempSync(path.join(tmpdir(), "rust-cache-guard-"));
  try {
    mkdirSync(path.join(root, "deps", "nested"), { recursive: true });
    writeFileSync(path.join(root, "a.bin"), Buffer.alloc(10));
    writeFileSync(path.join(root, "deps", "nested", "b.bin"), Buffer.alloc(32));
    assert.equal(directorySize(root), 42);
    assert.equal(directorySize(path.join(root, "missing")), 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
