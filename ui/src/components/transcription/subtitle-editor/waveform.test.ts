import { describe, expect, it } from "vitest";

import { drawWaveformCanvas, sampleWaveformWindow, type WaveformColumn } from "./waveform";

/** 第 i 根柱子的幅度就是 i/100，方便断言窗口落在哪一段。 */
const columns: WaveformColumn[] = Array.from({ length: 100 }, (_, i) => ({
  min: -i / 100,
  max: i / 100,
}));

describe("sampleWaveformWindow", () => {
  it("按绝对像素取样，窗口跟随 scrollLeft 移动", () => {
    const head = sampleWaveformWindow(columns, 0, 10, 1000);
    const middle = sampleWaveformWindow(columns, 500, 10, 1000);

    expect(head).toHaveLength(10);
    expect(middle).toHaveLength(10);
    // 1000px 对应 100 根柱子 → 第 500px 落在第 50 根。
    expect(head[0].max).toBeCloseTo(0, 5);
    expect(middle[0].max).toBeCloseTo(0.5, 5);
    // 若绘制忽略 scrollLeft（修复前的整条时间轴画法），两个窗口会完全一样。
    expect(middle).not.toEqual(head);
  });

  it("每个可视像素恰好产出一根竖线，与整条时间轴的长度无关", () => {
    expect(sampleWaveformWindow(columns, 0, 640, 1000)).toHaveLength(640);
    expect(sampleWaveformWindow(columns, 0, 640, 5_000_000)).toHaveLength(640);
  });

  it("柱子比像素密时，一个像素归并该区间的 min/max", () => {
    // 100 根柱子压进 10px 宽的时间轴 → 每像素归并 10 根。
    const merged = sampleWaveformWindow(columns, 0, 10, 10);
    expect(merged[0].max).toBeCloseTo(0.09, 5);
    expect(merged[0].min).toBeCloseTo(-0.09, 5);
  });

  it("越过柱子末尾的像素取零而不是残留的哨兵值", () => {
    const tail = sampleWaveformWindow(columns, 999, 3, 1000);
    expect(tail[1]).toEqual({ min: 0, max: 0 });
    expect(tail[2]).toEqual({ min: 0, max: 0 });
  });

  it("没有波形数据时不产出任何竖线", () => {
    expect(sampleWaveformWindow([], 0, 640, 1000)).toEqual([]);
  });
});

describe("drawWaveformCanvas", () => {
  /// 画布宽度一旦跟随整条时间轴，十几分钟的音频就会触及浏览器单边上限，
  /// 绘制静默变 no-op、界面显示纯黑。这里锁死「画布只有视口那么宽」。
  it("画布后备缓冲只有视口宽度，不随时间轴总宽膨胀", () => {
    const canvas = document.createElement("canvas");
    drawWaveformCanvas(canvas, columns, 800, 52, 1, 120_000, 500_000);

    expect(canvas.width).toBe(800);
    expect(canvas.height).toBe(52);
    expect(canvas.style.width).toBe("800px");
    expect(canvas.style.transform).toBe("translateX(120000px)");
  });

  it("canvas 为 null 时安全返回", () => {
    expect(() => drawWaveformCanvas(null, columns, 800, 52, 1, 0, 1000)).not.toThrow();
  });
});
