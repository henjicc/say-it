import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import {
  DEBUG_BUDGET_BYTES,
  SIZE_CHECK_INTERVAL_MS,
  debugCleanupReason,
  directorySize,
  replacesLocalRelease,
  shouldCheckDebug,
  staleSherpaEntries,
  toolchainChanged,
} from "./rust-cache-guard.mjs";

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

test("只有存在上次记录且与当前不同才算工具链变更（决定是否删除 mac-check）", () => {
  assert.equal(toolchainChanged(oldToolchain, toolchain), true);
  assert.equal(toolchainChanged(toolchain, toolchain), false);
  assert.equal(toolchainChanged(null, toolchain), false);
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

test("只有产出 target/release 的打包才清理本地 release", () => {
  assert.equal(replacesLocalRelease([]), true);
  assert.equal(replacesLocalRelease(["--no-bundle"]), true);
  assert.equal(replacesLocalRelease(["--bundles", "nsis"]), true);
  for (const args of [["--debug"], ["-d"], ["--help"], ["-h"], ["--target", "x86_64-pc-windows-msvc"], ["-t", "x"], ["--target=x"]]) {
    assert.equal(replacesLocalRelease(args), false, args.join(" "));
  }
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
