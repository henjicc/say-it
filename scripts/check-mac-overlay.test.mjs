import assert from "node:assert/strict";
import path from "node:path";
import test from "node:test";

import { macCheckTargetDir } from "./check-mac-overlay.mjs";

test("交叉检查产物放在 CARGO_TARGET_DIR 下的 mac-check", () => {
  const shared = path.resolve("shared-target");
  assert.equal(macCheckTargetDir({ CARGO_TARGET_DIR: shared }), path.join(shared, "mac-check"));
});

test("CARGO_TARGET_DIR 未设置或为空时回退到 src-tauri/target", () => {
  const fallback = path.join(path.resolve("src-tauri/target"), "mac-check");
  assert.equal(macCheckTargetDir({}), fallback);
  assert.equal(macCheckTargetDir({ CARGO_TARGET_DIR: "" }), fallback);
});
