import { describe, expect, it } from "vitest";
import { floatingOrbWaveLayout } from "./waveform";

describe("floating orb waveform pixel geometry", () => {
  it.each([1, 1.25, 1.5, 1.75, 2, 2.25, 2.5, 2.75, 3])("keeps five equal-width bars on physical pixels at %sx", (ratio) => {
    for (let extent = 28; extent <= 72; extent++) {
      for (const scale of [0.54, 0.57, 0.594]) {
        const width = (extent - 2) * scale;
        const left = (extent - width) / 2;
        const layout = floatingOrbWaveLayout(width, left, ratio);
        const pixels = layout.width * ratio;
        expect(pixels).toBeCloseTo(Math.round(pixels), 8);
        const positions = layout.offsets.map((offset) => (offset + left) * ratio);
        positions.forEach((position) => expect(position).toBeCloseTo(Math.round(position), 8));
        const gaps = positions.slice(1).map((position, index) => position - positions[index] - pixels);
        gaps.forEach((gap) => expect(gap).toBeCloseTo(gaps[0], 8));
        expect(positions.at(-1)! + pixels - positions[0]).toBeLessThanOrEqual(width * ratio + 1);
      }
    }
  });

  it("corrects the fractional origins reproduced at 150%", () => {
    const layout = floatingOrbWaveLayout(36.45, 50.27, 1.5);
    expect(layout.width * 1.5).toBe(5);
    // 5 × 5px + 4 × 4px = 41px；居中后从第 82 个物理像素开始。
    expect(layout.offsets.map((offset) => Math.round((offset + 50.27) * 1.5))).toEqual([82, 91, 100, 109, 118]);
  });
});
