import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";

import { buildReleaseNotes, extractReleaseNotes } from "./release-notes.mjs";

const CHANGELOG = [
  "# 更新日志",
  "",
  "说明文字。",
  "",
  "## [0.4.0] - 2026-07-15",
  "",
  "### 分组标题",
  "",
  "- 第一条。",
  "- 第二条。",
  "",
  "## [0.3.2] - 2026-06-01",
  "",
  "- 上一版的内容。",
  "",
].join("\n");

// 原实现用 `^## \[版本\].*?(?=^## \[|$)` + `ms`：`m` 让 `$` 匹配每行行尾，惰性的
// `.*?` 在标题那一行就被满足，于是每次发布的 Release 说明都只剩一行标题。
test("提取版本小节的完整正文，而不是只剩标题一行", () => {
  const notes = extractReleaseNotes(CHANGELOG, "0.4.0");
  assert.match(notes, /^## \[0\.4\.0\] - 2026-07-15$/m);
  assert.match(notes, /### 分组标题/);
  assert.match(notes, /- 第一条。/);
  assert.match(notes, /- 第二条。/);
  assert.ok(notes.split("\n").length > 1, "正文不能只剩标题一行");
});

test("不串到下一个版本的小节", () => {
  const notes = extractReleaseNotes(CHANGELOG, "0.4.0");
  assert.ok(!notes.includes("0.3.2"), notes);
  assert.ok(!notes.includes("上一版的内容"), notes);
});

test("最后一个版本一直取到文件结尾", () => {
  const notes = extractReleaseNotes(CHANGELOG, "0.3.2");
  assert.match(notes, /- 上一版的内容。/);
});

test("版本号里的点不会被当成通配符", () => {
  assert.throws(() => extractReleaseNotes(CHANGELOG, "0x4x0"), /were not found/);
});

test("找不到版本时明确失败，不静默产出空说明", () => {
  assert.throws(() => extractReleaseNotes(CHANGELOG, "9.9.9"), /9\.9\.9/);
});

test("拼上模型包说明", () => {
  const output = buildReleaseNotes(CHANGELOG, "0.4.0", "  模型包说明  ");
  assert.match(output, /- 第二条。\n\n---\n\n模型包说明\n$/);
});

// 真实仓库内容的回归：实测旧正则在这里只能提取出 23 个字符。
test("真实 CHANGELOG 的当前版本能提取出完整正文", () => {
  const version = JSON.parse(fs.readFileSync("package.json", "utf8")).version;
  const notes = extractReleaseNotes(fs.readFileSync("CHANGELOG.md", "utf8"), version);
  assert.ok(
    notes.length > 200,
    `当前版本的发布说明只有 ${notes.length} 个字符，几乎肯定又退化成只剩标题：${notes}`,
  );
});
