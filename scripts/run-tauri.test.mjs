import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import test from "node:test";

const launcher = fileURLToPath(new URL("./run-tauri.mjs", import.meta.url));

function runTauri(...args) {
  const result = spawnSync(process.execPath, [launcher, ...args], {
    encoding: "utf8",
    timeout: 30_000,
  });
  assert.ifError(result.error);
  return result;
}

test("启动项目本地 Tauri CLI", () => {
  const result = runTauri("--version");
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /tauri-cli \d+\.\d+\.\d+/);
});

for (const command of ["dev", "build"]) {
  test(`透传 ${command} 子命令与参数`, () => {
    // --no-bundle 避免 macOS 在 build 帮助命令后执行产物校验。
    const args = command === "build" ? ["--help", "--no-bundle"] : ["--help"];
    const result = runTauri(command, ...args);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, new RegExp(` ${command} \\[OPTIONS\\]`));
  });
}

test("保留 CLI 参数错误的非零退出码和错误信息", () => {
  const result = runTauri("sayit-invalid-command");
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /unrecognized subcommand 'sayit-invalid-command'/);
});

test("未提供子命令时明确报错", () => {
  const result = runTauri();
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /缺少 Tauri 子命令/);
});
