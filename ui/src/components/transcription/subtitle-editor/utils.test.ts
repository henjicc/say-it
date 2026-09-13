import { describe, expect, it } from "vitest";

import type { EditableCue } from "@/features/transcription/subtitles";

import { BASE_PX_PER_SEC } from "./constants";
import { roundCueTimes } from "./utils";

function cue(beginMs: number, endMs: number): EditableCue {
  return { id: "c1", beginMs, endMs, text: "文本" };
}

describe("roundCueTimes", () => {
  it("把时间轴拖动产生的浮点毫秒取整，避免导出 SRT 被 Rust 侧 i64 拒收", () => {
    // 复刻 SubtitleEditor 的拖动换算：deltaMs = (位移像素 / pxPerSec) * 1000。
    // 100% 缩放下 pxPerSec = 60，拖 1px 即得 16.666…ms。
    const deltaMs = (1 / BASE_PX_PER_SEC) * 1000;
    expect(Number.isInteger(deltaMs)).toBe(false);

    const [rounded] = roundCueTimes([cue(deltaMs, 2000 + deltaMs)]);

    expect(Number.isInteger(rounded.beginMs)).toBe(true);
    expect(Number.isInteger(rounded.endMs)).toBe(true);
    expect(rounded.beginMs).toBe(17);
    expect(rounded.endMs).toBe(2017);
  });

  it("按播放头拆分产生的浮点毫秒同样被取整", () => {
    // 播放头来自 audio.currentTime * 1000，天然是浮点。
    const playheadMs = 12.3456 * 1000;
    const [rounded] = roundCueTimes([cue(0, playheadMs)]);
    expect(rounded.endMs).toBe(12346);
  });

  it("值已是整数时原样返回同一个 cue 对象，不制造无谓的新引用", () => {
    const original = cue(0, 2000);
    const [rounded] = roundCueTimes([original]);
    expect(rounded).toBe(original);
  });

  it("保留 id、文本与其它字段", () => {
    const original: EditableCue = {
      id: "c9",
      beginMs: 1.4,
      endMs: 2.6,
      text: "你好",
      speakerId: "spk-1",
    };
    const [rounded] = roundCueTimes([original]);
    expect(rounded).toEqual({ id: "c9", beginMs: 1, endMs: 3, text: "你好", speakerId: "spk-1" });
  });
});
