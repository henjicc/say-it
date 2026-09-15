import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

/**
 * 从 CHANGELOG 中取出某个版本那一节的全文。
 *
 * 原来的正则写成 `^## \[版本\].*?(?=^## \[|$)` 并带 `ms` 标志。`m` 让 `$` 匹配**每行**
 * 行尾，惰性的 `.*?` 于是在标题那一行的行尾就被前瞻满足——每次发布的 GitHub Release
 * 说明都只剩 `## [x.y.z] - 日期` 这一行标题，正文全部丢失，而且流程本身不会报错。
 *
 * 所以：用 `[\s\S]*?` 跨行，用 `(?:^|\n)` 定位小节开头（不再需要 `m`），结束条件用
 * 下一节的 `\n## [` 或字符串真正的结尾。
 */
export function extractReleaseNotes(changelog, version) {
  const escaped = version.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = changelog.match(
    new RegExp(`(?:^|\\n)## \\[${escaped}\\][\\s\\S]*?(?=\\n## \\[|$)`),
  );
  if (!match) {
    throw new Error(`Release notes for version ${version} were not found in CHANGELOG.md.`);
  }
  return match[0].trim();
}

/** 发布说明 = 该版本的 CHANGELOG 小节 + 模型包说明。 */
export function buildReleaseNotes(changelog, version, modelNotes) {
  return `${extractReleaseNotes(changelog, version)}\n\n---\n\n${modelNotes.trim()}\n`;
}

function main() {
  const root = process.cwd();
  const version = JSON.parse(fs.readFileSync(path.join(root, "package.json"), "utf8")).version;
  const changelog = fs.readFileSync(path.join(root, "CHANGELOG.md"), "utf8");
  const modelNotes = fs.readFileSync(path.join(root, "model-packs/RELEASE_NOTES.md"), "utf8");
  const output = process.argv[2];
  if (!output) throw new Error("用法：node scripts/release-notes.mjs <输出文件>");
  fs.writeFileSync(output, buildReleaseNotes(changelog, version, modelNotes));
  process.stdout.write(`已写入 ${output}（${version}）\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
