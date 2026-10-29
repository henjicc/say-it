import assert from "node:assert/strict";
import test from "node:test";

import { verifyReleaseVersion, versionFromTag } from "./verify-release-version.mjs";

test("tag 与两处版本号一致时通过", () => {
  assert.equal(
    verifyReleaseVersion("v0.4.0", {
      "package.json": "0.4.0",
      "src-tauri/tauri.conf.json": "0.4.0",
    }),
    "0.4.0",
  );
});

// 流水线全程只读 package.json 的版本来命名资产、拟标题、取 CHANGELOG 小节，
// 从不看 tag 本身：漏改版本号就会静默发出「tag 是新版、资产是旧版」的 Release。
test("版本号漏改时报错，并指出是哪一处", () => {
  assert.throws(
    () =>
      verifyReleaseVersion("v0.5.0", {
        "package.json": "0.4.0",
        "src-tauri/tauri.conf.json": "0.5.0",
      }),
    /package\.json 是 0\.4\.0/,
  );
});

test("只改了 package.json、忘了 tauri.conf.json 同样报错", () => {
  assert.throws(
    () =>
      verifyReleaseVersion("v0.5.0", {
        "package.json": "0.5.0",
        "src-tauri/tauri.conf.json": "0.4.0",
      }),
    /tauri\.conf\.json 是 0\.4\.0/,
  );
});

test("tag 形状不对时直接拒绝", () => {
  assert.throws(() => versionFromTag("0.4.0"), /必须形如 v1\.2\.3/);
  assert.throws(() => versionFromTag("v0.4"), /必须形如 v1\.2\.3/);
  assert.equal(versionFromTag("v1.2.3-beta.1"), "1.2.3-beta.1");
});
