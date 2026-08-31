import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/store/useDictPrefs", () => ({ useDictPrefs: { getState: vi.fn() } }));

const oscillators: { start: ReturnType<typeof vi.fn>; stop: ReturnType<typeof vi.fn>; frequency: { value: number } }[] = [];
let playCueKind: typeof import("./cues").playCueKind;

describe("cue sound envelopes", () => {
  beforeEach(async () => {
    vi.resetModules();
    oscillators.length = 0;
    vi.stubGlobal("AudioContext", class {
      state = "running";
      currentTime = 0;
      destination = {};
      createOscillator() {
        const oscillator = {
          type: "sine", connect: vi.fn(), start: vi.fn(), stop: vi.fn(),
          frequency: { value: 0 },
        };
        oscillators.push(oscillator);
        return oscillator;
      }
      createGain() {
        return { connect: vi.fn(), gain: { setValueAtTime: vi.fn(), exponentialRampToValueAtTime: vi.fn() } };
      }
    });
    ({ playCueKind } = await import("./cues"));
  });
  afterEach(() => vi.unstubAllGlobals());

  it.each([
    ["beep-up", "start", [660, 990], 0.1, 0.02],
    ["beep-down", "end", [880, 520], 0.12, 0.02],
    ["beep-double", "start", [880, 880], 0.07, 0.05],
  ] as const)("preserves the original %s frequencies and timing", (kind, which, frequencies, duration, gap) => {
    playCueKind(kind, which);
    expect(oscillators).toHaveLength(2);
    oscillators.forEach((oscillator, index) => {
      const start = 0.01 + index * (duration + gap);
      expect(oscillator.frequency.value).toBe(frequencies[index]);
      expect(oscillator.start).toHaveBeenCalledExactlyOnceWith(start);
      expect(oscillator.stop).toHaveBeenCalledExactlyOnceWith(start + duration + 0.02);
    });
  });

  it("does not produce a fallback beep for the none preset", () => {
    playCueKind("none", "start");
    expect(oscillators).toHaveLength(0);
  });
});
