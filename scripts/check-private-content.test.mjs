import assert from "node:assert/strict";
import { test } from "node:test";
import { findTextIssues, isMediaAllowed, parseAddedLines, pathNeedles } from "./check-private-content.mjs";

const none = { paths: [], terms: [], user: undefined };

test("媒体只允许放在软件资源与测试素材目录", () => {
  assert.equal(isMediaAllowed("src-tauri/icons/tray/macos.png"), true);
  assert.equal(isMediaAllowed("ui/public/cues/tick-up.wav"), true);
  assert.equal(isMediaAllowed("docs/images/设置.png"), false);
  assert.equal(isMediaAllowed("docs/task/界面/录屏.mp4"), false);
  assert.equal(isMediaAllowed("src/main.rs"), true);
});

test("任意用户主目录路径都被拦，占位与约定假名放行", () => {
  assert.notEqual(findTextIssues(String.raw`日志在 C:\Users\alice\AppData`, none).length, 0);
  assert.notEqual(findTextIssues("见 /Users/bob/Documents/a.md", none).length, 0);
  assert.deepEqual(findTextIssues(String.raw`%USERPROFILE%\AppData 或 C:\Users\<用户名>\AppData`, none), []);
  assert.deepEqual(findTextIssues("/Users/example/a.txt", none), []);
});

test("本机路径的各种写法都能认出，且不误伤同名前缀的目录", () => {
  const needles = { paths: [{ label: "仓库所在目录", paths: pathNeedles(String.raw`E:\Work\仓库`) }], terms: [], user: undefined };
  for (const line of [String.raw`'E:\Work\仓库\src'`, "E:/work/仓库/src", String.raw`"E:\\Work\\仓库"`, "/e/Work/仓库/src"]) {
    assert.deepEqual(findTextIssues(line, needles), ["仓库所在目录"], line);
  }
  assert.deepEqual(findTextIssues("E:/Work/仓库备份/src", needles), []);
});

test("以 ++ 开头的新增行和带引号的文件名都按正确文件计入", () => {
  const diff = [
    "diff --git a/a.md b/a.md",
    "--- a/a.md",
    "+++ b/a.md",
    "@@ -0,0 +1,2 @@",
    "+++ b/假文件头",
    "+第二行",
    'diff --git "a/q\\"x.md" "b/q\\"x.md"',
    "--- /dev/null",
    '+++ "b/q\\"x.md"',
    "@@ -0,0 +5 @@",
    "+内容",
  ].join("\n");
  assert.deepEqual(parseAddedLines(diff), [
    { file: "a.md", lineNo: 1, text: "++ b/假文件头" },
    { file: "a.md", lineNo: 2, text: "第二行" },
    { file: 'q"x.md', lineNo: 5, text: "内容" },
  ]);
});

test("网址里的 /home/ 不算用户主目录", () => {
  assert.deepEqual(findTextIssues("见 https://example.com/home/docs/", none), []);
});
