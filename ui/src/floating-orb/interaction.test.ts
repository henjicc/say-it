import { describe, expect, it } from "vitest";
import { floatingOrbLabel, shouldStartOrbDrag } from "./interaction";

describe("floating orb interaction", () => {
  it("starts dragging only after the five pixel threshold", () => {
    expect(shouldStartOrbDrag(3, 3)).toBe(false);
    expect(shouldStartOrbDrag(3, 4)).toBe(true);
  });

  it("replaces the recording label while the stop action is hovered", () => {
    expect(floatingOrbLabel("recording", "聆听中…", false)).toBe("聆听中…");
    expect(floatingOrbLabel("recording", "聆听中…", true)).toBe("停止识别");
    expect(floatingOrbLabel("fallback", "", false)).toBe("已复制，请手动粘贴");
  });
});
