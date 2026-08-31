import { cleanup, render } from "@testing-library/react";
import { act } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { OrbWaveform } from "./OrbWaveform";

let measure: () => void;
let resolutionChanged: () => void;
let rect: DOMRect;
const disconnect = vi.fn();
const removeListener = vi.fn();

describe("orb waveform layout updates", () => {
  beforeEach(() => {
    rect = new DOMRect(50.27, 0, 36.45, 36.45);
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(() => rect);
    vi.stubGlobal("devicePixelRatio", 1.5);
    vi.stubGlobal("ResizeObserver", class {
      constructor(callback: () => void) { measure = callback; }
      observe() {}
      disconnect = disconnect;
    });
    vi.stubGlobal("matchMedia", () => ({
      addEventListener: (_: string, callback: () => void) => { resolutionChanged = callback; },
      removeEventListener: removeListener,
    }));
  });
  afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

  it("changes only heights when new audio levels arrive", () => {
    const view = render(<OrbWaveform levels={[0.2, 0.4, 0.8, 0.6, 0.3]} />);
    const geometry = () => [...view.container.querySelectorAll<HTMLElement>(".orb-wave-bar")]
      .map((bar) => ({ width: bar.style.width, left: bar.style.left }));
    const previous = geometry();
    expect(new Set(previous.map((bar) => bar.width)).size).toBe(1);
    view.rerender(<OrbWaveform levels={[0.9, 0.2, 0.3, 0.8, 0.7]} />);
    expect(geometry()).toEqual(previous);
  });

  it("remeasures dimensions and monitor density and cleans up observers", () => {
    const view = render(<OrbWaveform levels={[0.6, 0.6, 0.6, 0.6, 0.6]} />);
    rect = new DOMRect(10.25, 0, 40, 40);
    act(() => measure());
    const bars = [...view.container.querySelectorAll<HTMLElement>(".orb-wave-bar")];
    expect(bars.every((bar) => Number.parseFloat(bar.style.width) === 4)).toBe(true);
    vi.stubGlobal("devicePixelRatio", 2);
    act(() => resolutionChanged());
    bars.forEach((bar) => {
      const left = (Number.parseFloat(bar.style.left) + rect.left) * 2;
      expect(left).toBeCloseTo(Math.round(left), 8);
    });
    view.unmount();
    expect(disconnect).toHaveBeenCalledTimes(1);
    expect(removeListener).toHaveBeenCalled();
  });

  it("realigns after a parent entrance animation without a layout resize", () => {
    const view = render(<button><OrbWaveform levels={[0.6, 0.6, 0.6, 0.6, 0.6]} /></button>);
    rect = new DOMRect(10, 0, 40, 40);
    act(() => view.container.querySelector("button")!.dispatchEvent(new Event("animationend")));
    const bars = [...view.container.querySelectorAll<HTMLElement>(".orb-wave-bar")];
    expect(bars.every((bar) => Number.parseFloat(bar.style.width) === 4)).toBe(true);
  });
});
