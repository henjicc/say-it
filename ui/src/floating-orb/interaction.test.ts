import { describe, expect, it } from "vitest";
import {
  floatingOrbClickAction,
  floatingOrbLabel,
  floatingOrbWaveScale,
  normalizeFloatingOrbAppearance,
  shouldStartOrbDrag,
} from "./interaction";

describe("floating orb interaction", () => {
  it("starts dragging only after the five pixel threshold", () => {
    expect(shouldStartOrbDrag(3, 3)).toBe(false);
    expect(shouldStartOrbDrag(3, 4)).toBe(true);
  });

  it("keeps state labels available to the compact icon-only window", () => {
    expect(floatingOrbLabel("recording", "聆听中…")).toBe("聆听中…");
    expect(floatingOrbLabel("fallback", "")).toBe("已复制，请手动粘贴");
    expect(floatingOrbLabel("armed", "")).toBe("点击开始语音输入");
    expect(floatingOrbLabel("positioning", "")).toBe("正在定位…");
    expect(floatingOrbLabel("submitted", "")).toBe("已发送回车");
    expect(floatingOrbLabel("cancelled", "")).toBe("已取消");
  });

  it("routes a successful result to Enter without starting another dictation", () => {
    expect(floatingOrbClickAction("armed", false)).toBe("activate");
    expect(floatingOrbClickAction("recording", false)).toBe("stop");
    expect(floatingOrbClickAction("success", true)).toBe("submit");
    expect(floatingOrbClickAction("success", false)).toBeNull();
    expect(floatingOrbClickAction("processing", true)).toBeNull();
  });

  it("amplifies quiet audio while keeping waveform scale bounded", () => {
    expect(floatingOrbWaveScale(0)).toBe(0);
    expect(floatingOrbWaveScale(0.04)).toBeCloseTo(0.36);
    expect(floatingOrbWaveScale(2)).toBe(1);
  });

  it("clamps continuous appearance controls to supported ranges", () => {
    expect(normalizeFloatingOrbAppearance({ size: 63.6, opacity: 70, glassEnabled: true })).toEqual({
      size: 64,
      opacity: 70,
      glassEnabled: true,
      glassMaterial: "sidebar",
      glassTint: 8,
      glassBorder: 0,
    });
    expect(normalizeFloatingOrbAppearance({
      size: 20,
      opacity: 140,
      glassMaterial: "invalid",
      glassTint: 99,
      glassBorder: -5,
    })).toEqual({
      size: 44,
      opacity: 100,
      glassEnabled: false,
      glassMaterial: "sidebar",
      glassTint: 40,
      glassBorder: 0,
    });
  });
});
