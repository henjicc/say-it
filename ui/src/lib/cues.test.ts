import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/store/useDictPrefs", () => ({ useDictPrefs: { getState: vi.fn() } }));

const oscillators: { start: ReturnType<typeof vi.fn>; stop: ReturnType<typeof vi.fn>; frequency: { setValueAtTime: ReturnType<typeof vi.fn>; exponentialRampToValueAtTime: ReturnType<typeof vi.fn> } }[] = [];
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
          frequency: { setValueAtTime: vi.fn(), exponentialRampToValueAtTime: vi.fn() },
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
    ["beep-up", "start", 660, 990],
    ["beep-down", "end", 880, 520],
  ] as const)("plays %s as one continuous tone", (kind, which, from, to) => {
    playCueKind(kind, which);
    expect(oscillators).toHaveLength(1);
    expect(oscillators[0].start).toHaveBeenCalledTimes(1);
    expect(oscillators[0].frequency.setValueAtTime).toHaveBeenCalledWith(from, expect.any(Number));
    expect(oscillators[0].frequency.exponentialRampToValueAtTime).toHaveBeenCalledWith(to, expect.any(Number));
  });

  it("keeps two separate attacks only for the explicit double preset", () => {
    playCueKind("beep-double", "start");
    expect(oscillators).toHaveLength(2);
    expect(oscillators[1].start.mock.calls[0][0]).toBeGreaterThan(oscillators[0].stop.mock.calls[0][0]);
  });

  it("does not produce a fallback beep for the none preset", () => {
    playCueKind("none", "start");
    expect(oscillators).toHaveLength(0);
  });
});
