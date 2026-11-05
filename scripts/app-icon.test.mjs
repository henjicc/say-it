import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const ICO_DIR_ENTRY_OFFSET = 6;

// tauri-codegen 固定取 ICO 第一项做窗口和托盘图标。Windows 缩小大图很清晰，放大小图会糊，
// 所以第一项必须是最大尺寸（目录里宽度字节 0 表示 256）。
test("icon.ico 的第一项是 256 尺寸", () => {
  const ico = readFileSync(new URL("../src-tauri/icons/icon.ico", import.meta.url));
  assert.equal(ico.readUInt8(ICO_DIR_ENTRY_OFFSET) || 256, 256);
});
