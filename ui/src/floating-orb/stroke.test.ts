import { describe, expect, it } from "vitest";
import { floatingOrbStrokeWidth } from "./stroke";

describe("floating orb relative stroke", () => {
  it.each([1, 1.25, 1.5, 1.75, 2, 2.25, 2.5, 2.75, 3])("aligns the entire size range to physical pixels at %sx", (ratio) => {
    let previous = 0;
    for (let size = 28; size <= 72; size += 0.25) {
      const width = floatingOrbStrokeWidth(size, ratio);
      expect(width * ratio).toBeCloseTo(Math.round(width * ratio), 8);
      expect(width * ratio).toBeGreaterThanOrEqual(2);
      expect(width).toBeGreaterThanOrEqual(previous);
      expect(width).toBeLessThanOrEqual(2 + 0.5 / ratio);
      expect(width * 2).toBeLessThan(size);
      previous = width;
    }
  });

  it("keeps small low-density orbs legible and scales larger Retina orbs proportionally", () => {
    expect(floatingOrbStrokeWidth(28, 1)).toBe(2);
    expect(floatingOrbStrokeWidth(28, 1.5)).toBeCloseTo(2 / 1.5);
    expect(floatingOrbStrokeWidth(28, 2)).toBe(1.5);
    expect(floatingOrbStrokeWidth(72, 2)).toBe(2);
    expect(floatingOrbStrokeWidth(500, 2)).toBe(2);
  });

  it("never creates invalid CSS from incomplete geometry", () => {
    for (const size of [0, -1, NaN, Infinity]) {
      for (const ratio of [0, -1, NaN, Infinity]) {
        expect(floatingOrbStrokeWidth(size, ratio)).toBe(2);
      }
    }
  });
});
