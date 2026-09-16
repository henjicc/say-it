import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

/**
 * 把 `v1.2.3` 这样的 tag 还原成版本号。
 *
 * 只接受 `v` + 三段数字（可带预发布/构建后缀），与工作流的 `tags: 'v*.*.*'` 保持一致。
 */
export function versionFromTag(tag) {
  const match = /^v(\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?)$/.exec(String(tag ?? "").trim());
  if (!match) throw new Error(`发布 tag 必须形如 v1.2.3，收到的是：${tag}`);
  return match[1];
}

/**
 * 校验推送的 tag 与仓库里记录的版本号一致。
 *
 * 发布流水线全程只读 `package.json` 的版本来命名资产、拟标题、取 CHANGELOG 小节，
 * 从不看 tag 本身。版本号漏改一处就会静默发出「tag 是新版、资产是旧版」的 Release，
 * 而 CHANGELOG 取的也是旧版那一节——没有任何一步会失败。
 *
 * `tauri.conf.json` 一并校验：CLAUDE.md 要求两处版本同步更新，而安装包内的版本号来自它。
 */
export function verifyReleaseVersion(tag, versions) {
  const expected = versionFromTag(tag);
  const mismatches = Object.entries(versions)
    .filter(([, value]) => value !== expected)
    .map(([source, value]) => `${source} 是 ${value}`);
  if (mismatches.length > 0) {
    throw new Error(`发布 tag ${tag} 对应版本 ${expected}，但 ${mismatches.join("，")}。`);
  }
  return expected;
}

function main() {
  const root = process.cwd();
  const read = (file) => JSON.parse(fs.readFileSync(path.join(root, file), "utf8")).version;
  const tag = process.argv[2];
  if (!tag) throw new Error("用法：node scripts/verify-release-version.mjs <tag>");
  const version = verifyReleaseVersion(tag, {
    "package.json": read("package.json"),
    "src-tauri/tauri.conf.json": read("src-tauri/tauri.conf.json"),
  });
  process.stdout.write(`版本一致：${version}\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
