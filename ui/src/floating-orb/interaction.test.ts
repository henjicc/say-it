import { describe, expect, it } from "vitest";
import {
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
  });

  it("amplifies quiet audio while keeping waveform scale bounded", () => {
    expect(floatingOrbWaveScale(0)).toBe(0);
    expect(floatingOrbWaveScale(0.04)).toBeCloseTo(0.36);
    expect(floatingOrbWaveScale(2)).toBe(1);
  });

  it("normalizes size and opacity to supported menu presets", () => {
    expect(normalizeFloatingOrbAppearance({ size: 64, opacity: 70 })).toEqual({
      size: 64,
      opacity: 70,
    });
    expect(normalizeFloatingOrbAppearance({ size: 60, opacity: 90 })).toEqual({
      size: 56,
      opacity: 100,
    });
  });
});
