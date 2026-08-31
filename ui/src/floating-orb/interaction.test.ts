import { describe, expect, it } from "vitest";
import {
  floatingOrbClickAction,
  floatingOrbContextAction,
  floatingOrbLabel,
  floatingOrbWaveScale,
  normalizeFloatingOrbAppearance,
  shouldStartOrbDrag,
  shouldHandleOrbClick,
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
    expect(floatingOrbClickAction("idle", false)).toBe("activate");
    expect(floatingOrbClickAction("armed", false)).toBe("activate");
    expect(floatingOrbClickAction("recording", false)).toBe("stop");
    expect(floatingOrbClickAction("success", true)).toBe("submit");
    expect(floatingOrbClickAction("success", false)).toBeNull();
    expect(floatingOrbClickAction("processing", true)).toBeNull();
  });

  it("ignores the click after dragging but preserves keyboard activation", () => {
    expect(shouldHandleOrbClick(1, true)).toBe(false);
    expect(shouldHandleOrbClick(1, false)).toBe(true);
    expect(shouldHandleOrbClick(0, true)).toBe(true);
  });

  it("opens details on an error and dismisses only the error on right click", () => {
    for (const canSubmit of [false, true]) {
      for (const transient of [false, true]) {
        expect(floatingOrbClickAction("error", canSubmit)).toBe("showError");
        expect(floatingOrbContextAction("error", canSubmit, transient)).toBe("dismissError");
      }
    }
  });

  it("preserves recording cancellation, submit dismissal and the idle menu", () => {
    expect(floatingOrbContextAction("recording", false, true)).toBe("cancel");
    expect(floatingOrbContextAction("success", true, false)).toBe("dismissSubmit");
    expect(floatingOrbContextAction("success", false, false)).toBeNull();
    expect(floatingOrbContextAction("idle", false, false)).toBe("menu");
    expect(floatingOrbContextAction("idle", false, true)).toBeNull();
    for (const phase of ["processing", "smartProcessing", "moving", "submitting"] as const) {
      expect(floatingOrbClickAction(phase, false)).toBeNull();
      expect(floatingOrbContextAction(phase, false, false)).toBeNull();
    }
  });

  it("amplifies quiet audio while keeping waveform scale bounded", () => {
    expect(floatingOrbWaveScale(0)).toBe(0);
    expect(floatingOrbWaveScale(0.04)).toBeCloseTo(0.36);
    expect(floatingOrbWaveScale(2)).toBe(1);
  });

  it("clamps continuous appearance controls to supported ranges", () => {
    expect(
      normalizeFloatingOrbAppearance({ sizePercent: 63.6, opacity: 70, glassEnabled: true }),
    ).toEqual({
      sizePercent: 64,
      opacity: 70,
      glassEnabled: true,
      glassMaterial: "sidebar",
      glassTint: 8,
      glassBorder: 0,
    });
    expect(normalizeFloatingOrbAppearance({
      sizePercent: 5,
      opacity: 140,
      glassMaterial: "invalid",
      glassTint: 99,
      glassBorder: -5,
    })).toEqual({
      sizePercent: 25,
      opacity: 100,
      glassEnabled: false,
      glassMaterial: "sidebar",
      glassTint: 40,
      glassBorder: 0,
    });
  });
});
